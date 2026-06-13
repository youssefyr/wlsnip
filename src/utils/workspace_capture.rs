use crate::backends::CaptureBackend;
use crate::buffers::CaptureBuffer;
use crate::error::{Result, WlsnipError};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub fn capture_all_workspaces(
    backend: &mut Box<dyn CaptureBackend>,
    include_cursor: bool,
) -> Result<CaptureBuffer> {
    eprintln!("wlsnip: capturing all workspaces (this may cause screen flickering)...");

    let compositor = detect_compositor();
    let original_workspace = get_current_workspace(&compositor)?;
    let workspaces = get_workspaces(&compositor)?;

    let mut buffers = Vec::new();

    for ws in &workspaces {
        if switch_workspace(&compositor, ws).is_ok() {
            // Wait for compositor to render the new workspace
            thread::sleep(Duration::from_millis(150));
            
            // Capture all outputs (monitors) for this workspace
            if let Ok(buf) = backend.capture_all_outputs(include_cursor) {
                buffers.push(buf);
            }
        }
    }

    // Restore original workspace
    let _ = switch_workspace(&compositor, &original_workspace);

    if buffers.is_empty() {
        return Err(WlsnipError::Capture("Failed to capture any workspaces".into()));
    }

    stitch_buffers_horizontally(buffers)
}

fn detect_compositor() -> String {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        "hyprland".into()
    } else if std::env::var("SWAYSOCK").is_ok() {
        "sway".into()
    } else {
        // Fallback to mangowm if mmsg exists
        if Command::new("mmsg").arg("-h").output().is_ok() {
            "mangowm".into()
        } else {
            "unknown".into()
        }
    }
}

fn get_current_workspace(compositor: &str) -> Result<String> {
    match compositor {
        "hyprland" => {
            let out = Command::new("hyprctl").args(["activeworkspace", "-j"]).output().map_err(WlsnipError::Io)?;
            let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            Ok(json["id"].as_i64().unwrap_or(1).to_string())
        }
        "sway" => {
            let out = Command::new("swaymsg").args(["-t", "get_workspaces"]).output().map_err(WlsnipError::Io)?;
            let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            if let Some(arr) = json.as_array() {
                for ws in arr {
                    if ws["focused"].as_bool() == Some(true) {
                        return Ok(ws["name"].as_str().unwrap_or("1").to_string());
                    }
                }
            }
            Ok("1".into())
        }
        "mangowm" => {
            let out = Command::new("mmsg").args(["get", "all-tags"]).output().map_err(WlsnipError::Io)?;
            let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            if let Some(monitors) = json["all_tags"].as_array() {
                for mon in monitors {
                    if let Some(tags) = mon["tags"].as_array() {
                        for tag in tags {
                            if tag["is_active"].as_bool() == Some(true) {
                                return Ok(tag["index"].as_i64().unwrap_or(1).to_string());
                            }
                        }
                    }
                }
            }
            Ok("1".into())
        }
        _ => Ok("1".into()),
    }
}

fn get_workspaces(compositor: &str) -> Result<Vec<String>> {
    match compositor {
        "hyprland" => {
            let out = Command::new("hyprctl").args(["workspaces", "-j"]).output().map_err(WlsnipError::Io)?;
            let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            let mut ws_list = Vec::new();
            if let Some(arr) = json.as_array() {
                for ws in arr {
                    if let Some(id) = ws["id"].as_i64() {
                        ws_list.push(id.to_string());
                    }
                }
            }
            ws_list.sort_by_key(|a| a.parse::<i32>().unwrap_or(0));
            Ok(ws_list)
        }
        "sway" => {
            let out = Command::new("swaymsg").args(["-t", "get_workspaces"]).output().map_err(WlsnipError::Io)?;
            let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            let mut ws_list = Vec::new();
            if let Some(arr) = json.as_array() {
                for ws in arr {
                    if let Some(name) = ws["name"].as_str() {
                        ws_list.push(name.to_string());
                    }
                }
            }
            Ok(ws_list)
        }
        "mangowm" => {
            let out = Command::new("mmsg").args(["get", "all-tags"]).output().map_err(WlsnipError::Io)?;
            let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            let mut ws_list = Vec::new();
            if let Some(monitors) = json["all_tags"].as_array() {
                for mon in monitors {
                    if let Some(tags) = mon["tags"].as_array() {
                        for tag in tags {
                            if tag["client_count"].as_i64().unwrap_or(0) > 0 {
                                ws_list.push(tag["index"].as_i64().unwrap_or(1).to_string());
                            }
                        }
                    }
                }
            }
            if ws_list.is_empty() {
                ws_list.push("1".into());
            }
            Ok(ws_list)
        }
        _ => Err(WlsnipError::Capture(format!("Unsupported compositor: {}", compositor))),
    }
}

fn switch_workspace(compositor: &str, ws: &str) -> Result<()> {
    match compositor {
        "hyprland" => {
            Command::new("hyprctl").args(["dispatch", "workspace", ws]).output().map_err(WlsnipError::Io)?;
        }
        "sway" => {
            Command::new("swaymsg").args(["workspace", ws]).output().map_err(WlsnipError::Io)?;
        }
        "mangowm" => {
            let _ = Command::new("mmsg").args(["dispatch", &format!("view,{}", ws)]).output();
        }
        _ => {}
    }
    Ok(())
}

fn stitch_buffers_horizontally(buffers: Vec<CaptureBuffer>) -> Result<CaptureBuffer> {
    if buffers.is_empty() {
        return Err(WlsnipError::Capture("No buffers to stitch".into()));
    }
    if buffers.len() == 1 {
        return Ok(buffers.into_iter().next().unwrap());
    }

    let total_width: u32 = buffers.iter().map(|b| b.width).sum();
    let max_height: u32 = buffers.iter().map(|b| b.height).max().unwrap_or(0);
    let format = buffers[0].format;

    let mut stitched_data = vec![0u8; (total_width * max_height * 4) as usize];

    let mut current_x = 0;
    for buf in buffers {
        for y in 0..buf.height {
            let src_start = (y * buf.stride) as usize;
            let src_end = src_start + (buf.width * 4) as usize;
            let src_row = &buf.data[src_start..src_end];

            let dst_start = (y * total_width * 4 + current_x * 4) as usize;
            let dst_end = dst_start + (buf.width * 4) as usize;
            
            stitched_data[dst_start..dst_end].copy_from_slice(src_row);
        }
        current_x += buf.width;
    }

    Ok(CaptureBuffer {
        data: stitched_data,
        width: total_width,
        height: max_height,
        stride: total_width * 4,
        format,
    })
}
