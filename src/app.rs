use crate::Cli;
use crate::Command;
use crate::ImageFormat;
use crate::BackendChoice;
use crate::backends::CaptureBackend;
use crate::backends::wlr_screencopy::WlrScreencopyBackend;
use crate::backends::ext_capture::ExtCaptureBackend;
use crate::backends::xdg_portal::XdgPortalBackend;
use crate::encoders::{self, OutputFormat};
use crate::error::{Result, WlsnipError};
use crate::utils::geometry::Region;
use crate::utils::effects;
use crate::utils::pin_overlay;

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::process::{Command as Cmd, Stdio};
use wl_clipboard_rs::copy::{Options, MimeType, Source};

use crate::config::Config;

/// Main orchestrator: connects to Wayland, selects backend, captures, encodes, outputs.
pub fn run(cli: &Cli) -> Result<()> {
    let config = Config::load();

    if cli.upload {
        if !config.allow_upload {
            return Err(WlsnipError::Capture("Upload is not enabled in ~/.config/wlsnip/config.toml (set allow_upload = true)".into()));
        }
    }

    let padding = cli.padding.or(config.padding).unwrap_or(0);
    let shadow = cli.shadow || config.shadow.unwrap_or(false);
    let selection_color = cli.selection_color.as_deref().or(config.selection_color.as_deref());
    let ignore_apps = cli.ignore_apps.as_ref().or(config.ignore_apps.as_ref());
    let clipboard = !cli.no_clipboard && cli.clipboard.or(config.clipboard).unwrap_or(true);
    let no_cursor = cli.no_cursor || config.no_cursor.unwrap_or(false);
    let jpeg_quality = cli.jpeg_quality.or(config.jpeg_quality).unwrap_or(90);

    if let Some(opt_count) = cli.bench_auto {
        let count = opt_count.unwrap_or(10);
        return run_benchmark(cli, count);
    }

    if cli.delay > 0 {
        std::thread::sleep(std::time::Duration::from_secs(cli.delay));
    }

    // ── 1. Select & initialise capture backend ─────────────────────────────
    let mut backend: Box<dyn CaptureBackend> = select_backend(cli)?;

    // ── 2. Determine command (default: Region) ──────────────────────────────
    let command = cli.command.as_ref().unwrap_or(&Command::Region);

    // ── 3. Determine capture parameters ────────────────────────────────────
    let (output_name, region) = resolve_capture_params(command, backend.name(), ignore_apps.map(|v| v.as_slice()))?;

    let include_cursor = !no_cursor;

    // ── 4. Capture ─────────────────────────────────────────────────────────
    let mut capture_buffer = if let Command::Output { name } = command {
        if name == "all" {
            crate::utils::workspace_capture::capture_all_workspaces(&mut backend, include_cursor)?
        } else {
            backend.capture_output(Some(name), None, include_cursor)?
        }
    } else {
        backend.capture_output(
            output_name.as_deref(),
            region.as_ref(),
            include_cursor,
        )?
    };

    // ── 4.5 Freeze Overlay & Region Crop ────────────────────────────────────
    if let Command::Region = command {
        if backend.name() != "xdg-desktop-portal" {
            let region_opt = crate::utils::freeze_overlay::run_with_freeze(
                &capture_buffer, 
                selection_color,
                !cli.slurp,
            )?;
            
            if let Some(region) = region_opt {
                capture_buffer = crate::utils::geometry::crop(&capture_buffer, &region)?;
            } else {
                eprintln!("wlsnip: selection canceled.");
                std::process::exit(0);
            }
        }
    }

    // ── 4.6 Apply Effects ───────────────────────────────────────────────────
    capture_buffer = effects::apply_effects(&capture_buffer, padding, shadow);

    // ── 4.7 Pin Overlay ─────────────────────────────────────────────────────
    if cli.pin {
        pin_overlay::pin_buffer(&capture_buffer)?;
        return Ok(()); // Typically pinning replaces saving, or we could continue to save. Let's just return.
    }

    // ── 5. Choose output format ─────────────────────────────────────────────
    let output_format = match cli.format {
        ImageFormat::Png => OutputFormat::Png,
        ImageFormat::Jpeg => OutputFormat::Jpeg {
            quality: jpeg_quality,
        },
        ImageFormat::Webp => OutputFormat::Webp {
            quality: 100.0, // WebP quality config could be added later, default to 100
        },
    };

    // ── 6. Determine output destination(s) and encode ──────────────────────
    let mut encoded: Vec<u8> = Vec::new();
    encoders::encode(&capture_buffer, output_format, &mut encoded)?;

    // Write to file if requested
    if let Some(ref path) = cli.output_file {
        write_to_file(path, &encoded)?;
    }

    // Write to stdout if requested
    if cli.stdout {
        io::stdout()
            .write_all(&encoded)
            .map_err(|e| WlsnipError::Io(e))?;
    }

    // Upload to Imgur if requested
    if cli.upload {
        upload_to_imgur(&encoded)?;
    } else if clipboard {
        // Copy to clipboard via wl-copy
        copy_to_clipboard(&encoded, &output_format)?;
    }

    // Open in satty if requested
    if cli.annotate {
        let mut child = Cmd::new("satty")
            .args(["--filename", "-"])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| WlsnipError::Capture(format!("failed to launch satty: {e}")))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(&encoded).map_err(WlsnipError::Io)?;
        }

        let status = child.wait().map_err(WlsnipError::Io)?;
        if !status.success() {
            return Err(WlsnipError::Capture("satty exited with failure".to_string()));
        }
    }

    // By default, write to a timestamped file unless stdout or upload is requested
    if cli.output_file.is_none() && !cli.stdout && !cli.upload {
        let path = default_file_path(&output_format);
        eprintln!("wlsnip: saving to {}", path.display());
        write_to_file(&path, &encoded)?;
    }

    if cli.notify {
        let _ = notify_rust::Notification::new()
            .summary("wlsnip")
            .body("Screenshot captured successfully.")
            .show();
    }

    Ok(())
}

fn run_benchmark(cli: &Cli, count: u32) -> Result<()> {
    println!("Starting automated benchmark with {} iterations...", count);
    let mut total_capture_time = std::time::Duration::ZERO;
    let mut total_overlay_time = std::time::Duration::ZERO;
    let mut total_crop_time = std::time::Duration::ZERO;
    let mut total_encode_time = std::time::Duration::ZERO;

    let mut backend: Box<dyn CaptureBackend> = select_backend(cli)?;
    let command = cli.command.as_ref().unwrap_or(&Command::Region);
    let (output_name, region) = resolve_capture_params(command, backend.name(), cli.ignore_apps.as_deref())?;
    let include_cursor = !cli.no_cursor;

    let output_format = match cli.format {
        ImageFormat::Png => OutputFormat::Png,
        ImageFormat::Jpeg => OutputFormat::Jpeg { quality: cli.jpeg_quality.unwrap_or(90) },
        ImageFormat::Webp => OutputFormat::Webp { quality: 100.0 },
    };

    let tool = if cli.slurp { "slurp" } else { "native" };
    println!("Testing Selector: {}", tool);

    for i in 1..=count {
        println!("--- Iteration {} ---", i);
        
        let t0 = std::time::Instant::now();
        let mut capture_buffer = backend.capture_output(
            output_name.as_deref(),
            region.as_ref(),
            include_cursor,
        )?;
        let t_capture = t0.elapsed();
        total_capture_time += t_capture;
        
        let mut t_overlay = std::time::Duration::ZERO;
        let mut t_crop = std::time::Duration::ZERO;

        if let Command::Region = command {
            if backend.name() != "xdg-desktop-portal" {
                let frac = (i as f64) / (count as f64);
                let x_ratio: f64 = 0.1 + (frac * 0.4);
                let y_ratio: f64 = 0.2 + (frac * 0.2);
                let w_ratio: f64 = 0.2;
                let h_ratio: f64 = 0.2;

                crate::utils::virtual_pointer::simulate_drag(
                    x_ratio, y_ratio, w_ratio, h_ratio, 200
                );

                let t1 = std::time::Instant::now();
                let region_opt = crate::utils::freeze_overlay::run_with_freeze(
                    &capture_buffer, 
                    cli.selection_color.as_deref(),
                    !cli.slurp,
                )?;
                t_overlay = t1.elapsed();
                total_overlay_time += t_overlay;

                if let Some(region) = region_opt {
                    let t2 = std::time::Instant::now();
                    capture_buffer = crate::utils::geometry::crop(&capture_buffer, &region)?;
                    t_crop = t2.elapsed();
                    total_crop_time += t_crop;
                }
            }
        }

        let t3 = std::time::Instant::now();
        let mut encoded: Vec<u8> = Vec::new();
        encoders::encode(&capture_buffer, output_format.clone(), &mut encoded)?;
        let t_encode = t3.elapsed();
        total_encode_time += t_encode;

        println!("  Capture: {:?}", t_capture);
        println!("  Overlay: {:?}", t_overlay);
        println!("  Crop:    {:?}", t_crop);
        println!("  Encode:  {:?}", t_encode);
    }

    println!("\n=== Benchmark Results (Average over {} runs) ===", count);
    println!("Capture: {:?}", total_capture_time / count);
    println!("Overlay: {:?}", total_overlay_time / count);
    println!("Crop:    {:?}", total_crop_time / count);
    println!("Encode:  {:?}", total_encode_time / count);

    Ok(())
}

// ── Backend selection ──────────────────────────────────────────────────────

fn select_backend(cli: &Cli) -> Result<Box<dyn CaptureBackend>> {
    if let Some(crate::Command::Window { query }) = &cli.command {
        if let Some(mut b) = crate::backends::window_capture::WindowCaptureBackend::probe() {
            b.set_query(query.clone());
            return Ok(Box::new(b));
        }
        // Fallthrough if not available, we will use normal backend and crop via IPC
    }

    match &cli.backend {
        Some(BackendChoice::Wlr) => {
            WlrScreencopyBackend::probe()
                .map(|b| Box::new(b) as Box<dyn CaptureBackend>)
                .ok_or_else(|| WlsnipError::NoBackendAvailable)
        }
        Some(BackendChoice::Ext) => {
            ExtCaptureBackend::probe()
                .map(|b| Box::new(b) as Box<dyn CaptureBackend>)
                .ok_or_else(|| WlsnipError::NoBackendAvailable)
        }
        Some(BackendChoice::Portal) => {
            XdgPortalBackend::probe()
                .map(|b| Box::new(b) as Box<dyn CaptureBackend>)
                .ok_or_else(|| WlsnipError::NoBackendAvailable)
        }
        None => {
            // Auto-detect: prefer wlr-screencopy (most widely supported), fall back to ext, then portal
            if let Some(b) = WlrScreencopyBackend::probe() {
                return Ok(Box::new(b));
            }
            if let Some(b) = ExtCaptureBackend::probe() {
                return Ok(Box::new(b));
            }
            if let Some(b) = XdgPortalBackend::probe() {
                return Ok(Box::new(b));
            }
            Err(WlsnipError::NoBackendAvailable)
        }
    }
}

// ── Capture parameter resolution ───────────────────────────────────────────

fn resolve_capture_params(
    command: &Command,
    backend_name: &str,
    ignore_apps: Option<&[String]>,
) -> Result<(Option<String>, Option<Region>)> {
    match command {
        Command::Full => Ok((None, None)),
        Command::Output { name } => Ok((Some(name.clone()), None)),
        Command::Window { query } => {
            if backend_name == "ext-window-capture" {
                // Native backend handles querying internally
                Ok((None, None))
            } else {
                // Fallback: resolve geometry via IPC
                let region = crate::utils::window_resolver::resolve_window(query.as_deref(), ignore_apps)?;
                Ok((None, Some(region)))
            }
        }
        Command::Region => {
            // We return None for region here. The full screen will be captured,
            // and the freeze overlay logic will invoke slurp afterwards.
            Ok((None, None))
        }
    }
}

// ── Output helpers ─────────────────────────────────────────────────────────

fn write_to_file(path: &Path, data: &[u8]) -> Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(data)?;
    Ok(())
}

fn copy_to_clipboard(data: &[u8], format: &OutputFormat) -> Result<()> {
    let mime = match format {
        OutputFormat::Png => "image/png",
        OutputFormat::Jpeg { .. } => "image/jpeg",
        OutputFormat::Webp { .. } => "image/webp",
    };

    let opts = Options::new();
    opts.copy(
        Source::Bytes(data.into()),
        MimeType::Specific(mime.to_string()),
    ).map_err(|e| WlsnipError::Capture(format!("Clipboard error: {e}")))?;

    Ok(())
}

fn default_file_path(format: &OutputFormat) -> std::path::PathBuf {
    let ext = match format {
        OutputFormat::Png => "png",
        OutputFormat::Jpeg { .. } => "jpg",
        OutputFormat::Webp { .. } => "webp",
    };
    // e.g. "wlsnip-2026-05-29-180000.png"
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filename = format!("wlsnip-{now}.{ext}");

    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(pic_dir) = user_dirs.picture_dir() {
            let screenshots_dir = pic_dir.join("Screenshots");
            let _ = std::fs::create_dir_all(&screenshots_dir);
            if screenshots_dir.exists() {
                return screenshots_dir.join(filename);
            }
            return pic_dir.join(filename);
        }
    }

    std::env::current_dir().unwrap_or_default().join(filename)
}

fn upload_to_imgur(data: &[u8]) -> Result<()> {
    eprintln!("wlsnip: uploading image to Imgur...");
    
    // Use a public anonymous client ID. In production, users should supply their own.
    let client_id = "Client-ID 546c25a59c58ad7";

    let mut child = Cmd::new("curl")
        .args([
            "-s",
            "--location",
            "--request", "POST",
            "https://api.imgur.com/3/image",
            "--header", &format!("Authorization: {}", client_id),
            "--form", "image=@-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| WlsnipError::Capture(format!("Failed to launch curl: {}", e)))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(data).map_err(WlsnipError::Io)?;
    }

    let output = child.wait_with_output().map_err(WlsnipError::Io)?;
    if !output.status.success() {
        return Err(WlsnipError::Capture("Imgur upload failed via curl".into()));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| WlsnipError::Capture(format!("Invalid response: {}", e)))?;
    
    if let Some(link) = json["data"]["link"].as_str() {
        eprintln!("wlsnip: uploaded successfully to {}", link);
        // Copy link to clipboard
        let opts = Options::new();
        opts.copy(
            Source::Bytes(link.as_bytes().into()),
            MimeType::Specific("text/plain".to_string()),
        ).map_err(|e| WlsnipError::Capture(format!("Clipboard error: {}", e)))?;
        Ok(())
    } else {
        Err(WlsnipError::Capture("Upload failed: No link in response".into()))
    }
}
