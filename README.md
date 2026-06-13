# wlsnip

A high-performance, modular screenshot utility for Wayland compositors, written in Rust.

## Architecture

```
CLI (clap) → App Orchestrator → Backend Probe → Capture → Encode → Output
```

Three capture backends with automatic fallback:
1. **wlr-screencopy-unstable-v1** — wlroots ecosystem (Sway, Hyprland, River) — **implemented**
2. **ext-image-copy-capture-v1** — Modern Wayland standard (COSMIC, newer wlroots) — *Phase 6*
3. **XDG Desktop Portal** — Universal fallback (GNOME, KDE) — *Phase 7*

## Project Structure

```
src/
├── main.rs                  # CLI entry point (clap)
├── app.rs                   # Orchestrator: probe, capture, encode, output
├── error.rs                 # Unified WlsnipError enum (thiserror)
├── backends/
│   ├── mod.rs               # CaptureBackend trait
│   ├── wlr_screencopy.rs    # zwlr_screencopy_manager_v1 backend ✅
│   ├── ext_capture.rs       # ext-image-copy-capture-v1 backend (stub)
│   └── xdg_portal.rs        # XDG Desktop Portal backend (stub)
├── buffers/
│   ├── mod.rs               # CaptureBuffer struct + PixelFormat enum
│   └── shm.rs               # POSIX SHM pool (memfd_create + mmap)
├── utils/
│   ├── geometry.rs          # Region struct, crop(), slurp integration
│   └── color.rs             # Pixel format conversion (BGRA→RGBA)
└── encoders/
    ├── mod.rs               # OutputFormat enum + encode() dispatch
    ├── png.rs               # PNG encoder via image crate
    └── jpeg.rs              # JPEG encoder via image crate
```

---

## Implementation Status

### Phase 1 — Project Skeleton & Error Types ✅
- [x] `Cargo.toml` with all dependencies
- [x] `src/error.rs` — unified `WlsnipError` enum with `thiserror`
- [x] `src/main.rs` — CLI with `clap` derive (region/full/output commands + all flags)

### Phase 2 — Buffer & Geometry Utilities ✅
- [x] `src/buffers/mod.rs` — `CaptureBuffer` struct, `PixelFormat` enum
- [x] `src/buffers/shm.rs` — POSIX SHM pool (`memfd_create` + `mmap`, safe `Drop`)
- [x] `src/utils/geometry.rs` — `Region` struct, `crop()`, `slurp` output parsing
- [x] `src/utils/color.rs` — BGRA/XRGB → RGBA pixel swizzle, format mapping

### Phase 3 — Capture Backends ✅ (wlr) / Stubbed (ext, portal)
- [x] `src/backends/mod.rs` — `CaptureBackend` trait definition
- [x] `src/backends/wlr_screencopy.rs` — Full implementation:
  - Wayland registry binding (screencopy manager, wl_shm, wl_output)
  - Output enumeration with names
  - SHM buffer allocation matching compositor constraints
  - Frame request → copy → ready event loop
  - Region capture support
  - Cursor overlay toggle
- [x] `src/backends/ext_capture.rs` — Implemented in Phase 6
- [x] `src/backends/xdg_portal.rs` — Implemented in Phase 7

### Phase 4 — Image Encoders ✅
- [x] `src/encoders/mod.rs` — Format dispatch (PNG / JPEG)
- [x] `src/encoders/png.rs` — RGBA conversion + stride stripping + `PngEncoder`
- [x] `src/encoders/jpeg.rs` — RGBA→RGB conversion + `JpegEncoder` with quality

### Phase 5 — CLI & Orchestration ✅
- [x] `src/main.rs` — Full CLI with clap derive:
  - Subcommands: `region`, `full`, `output <name>`
  - Flags: `--output-file`, `--format`, `--clipboard`, `--stdout`, `--backend`, `--jpeg-quality`, `--no-cursor`
- [x] `src/app.rs` — Full orchestrator implementation:
  - [x] Backend auto-detection with priority order (wlr → ext → portal)
  - [x] `--backend` flag for forced backend selection
  - [x] Region selection via `slurp` subprocess
  - [x] Encode to PNG or JPEG (quality-configurable)
  - [x] Write to file (`--output-file`) or stdout (`--stdout`)
  - [x] Clipboard integration via `wl-copy` subprocess (`--clipboard`)
  - [x] Default timestamped output (`wlsnip-<unix>.png`) when no destination given
- [x] 5/5 unit tests passing (`cargo test`)

### Phase 6 — ext-image-copy-capture Backend ✅
- [x] Bind `ext_image_copy_capture_manager_v1`
- [x] Bind `ext_output_image_capture_source_manager_v1`
- [x] Create source → session → negotiate constraints (buffer_size + shm_format + done)
- [x] Create frame → attach buffer → damage_buffer → capture → wait for ready/failed
- [x] Apply region crop post-capture
- [x] Probe function: detects compositor support, returns `None` gracefully if absent

### Phase 7 — XDG Desktop Portal Backend ✅
- [x] Integrate `ashpd` crate for `org.freedesktop.portal.Screenshot`
- [x] Tokio runtime for async D-Bus calls
- [x] Receive screenshot URI → decode → convert to `CaptureBuffer`
- [x] Handle interactive permission dialog gracefully

### Phase 8 — Performance, Output & Packaging ✅
- [x] Integrate `rayon` for parallel pixel format swizzling (BGRA → RGBA).
- [x] Implement `zune-png` encoder for fast pure-Rust PNG compression.
- [x] Implement `turbojpeg` encoder for accelerated JPEG compression via libjpeg-turbo.
- [x] Integrate `wl-clipboard-rs` for native Wayland clipboard support.
- [x] Add `--annotate` flag to instantly pipe captures into `satty`.
- [x] Generate `man` pages and shell completions (`bash`, `zsh`, `fish`) via `build.rs`.

---

## Usage & Features Guide

`wlsnip` offers a variety of powerful modes for capturing your Wayland desktop. It automatically detects the best backend (wlroots, ext-image-copy, or xdg-portal) and saves to your `~/Pictures/Screenshots` folder or copies to your clipboard by default.

### 1. Region Selection (Default)
Capture a specific portion of your screen. `wlsnip` uses a built-in native overlay that freezes the screen, allowing you to drag and select a region accurately.
```bash
wlsnip
wlsnip region
```
- **Dimensions HUD**: As you drag, you'll see a live `WxH` dimension overlay.
- **Cancel Cleanly**: Press `Esc` or `Right-Click` to cancel without saving.
- **Custom Color**: Customize the selection border color: `wlsnip region --selection-color "#00ff0080"`

### 2. Full Desktop / Multi-Output Stitching
Capture your entire desktop. By default, `wlsnip full` will automatically capture your **primary or current output**. To capture and stitch **all visible outputs (monitors and visible workspaces)** together seamlessly based on their native Wayland coordinates, use:
```bash
wlsnip output all
```

### 3. Active Window Capture
Capture the currently focused window automatically via IPC (supports hyprland, sway, dwl/mangowm).
```bash
wlsnip window
```
- **Exclude Apps**: Automatically ignore certain apps (like file managers) from being captured. It defaults to ignoring `nautilus,dolphin,thunar`.
  ```bash
  wlsnip window --ignore-apps "nautilus,dolphin,kitty"
  ```

### 4. Pinned / Floating Screenshots
Instead of saving the screenshot, display it instantly as a floating window on top of everything else. Great for reference!
```bash
wlsnip region --pin
wlsnip window --pin
```
*The pinned image will appear in the top-right corner. Press `Esc` or click it to dismiss.*

### 5. Aesthetics: Drop Shadows & Padding
Give your screenshots a premium look by automatically adding padding and drop shadows before saving.
```bash
wlsnip window --padding 20 --shadow
wlsnip region --padding 10 --shadow
```

### 6. File & Clipboard Output
By default, `wlsnip` saves to a file **AND** copies to the clipboard natively.
```bash
# Disable copying to clipboard
wlsnip region --no-clipboard

# Save to a specific file instead of ~/Pictures/Screenshots/
wlsnip full -o my_screenshot.png

# Write raw image data to stdout (for piping)
wlsnip full -s | feh -
```

### 7. Instant Annotation
Pipe the capture directly into `satty` (if installed) for immediate drawing and annotation.
```bash
wlsnip region -a
```

### 8. Misc Utilities
```bash
# JPEG with custom quality
wlsnip full -f jpeg --jpeg-quality 85

# Exclude cursor from capture
wlsnip full --no-cursor

# Add a delay before capturing (e.g. 4 seconds)
wlsnip full --delay 4

# Capture a specific monitor output
wlsnip output DP-1
```

## Dependencies

| Crate | Version | Purpose |
| ----- | ------- | ------- |
| `clap` | 4.6 | CLI argument parsing |
| `wayland-client` | 0.31 | Wayland protocol communication |
| `wayland-protocols` | 0.32 | ext-image-copy-capture-v1 bindings |
| `wayland-protocols-wlr` | 0.3 | wlr-screencopy bindings |
| `ashpd` | 0.13 | XDG Desktop Portal (D-Bus) |
| `tokio` | 1.x | Async runtime for D-Bus |
| `image` | 0.25 | PNG/JPEG encoding |
| `thiserror` | 2.x | Error derive macros |
| `nix` | 0.30 | `memfd_create`, `ftruncate` |
| `libc` | 0.2 | `mmap`, `munmap` |

### Runtime (optional)
- `slurp` — interactive region selection (`wlsnip region`)
- `wl-copy` — clipboard integration (`-c` / `--clipboard`)

## Building

```bash
cargo build --release
```

The binary will be at `target/release/wlsnip`.

Install system-wide:
```bash
sudo install -Dm755 target/release/wlsnip /usr/local/bin/wlsnip
```

## Future (v2+)

### Performance
- [ ] DMA-BUF zero-copy capture path (`linux-dmabuf-v1`) [PHASE 10]
- [x] `zune-png` encoder (faster pure-Rust PNG) [PHASE 8]
- [x] `turbojpeg` encoder (libjpeg-turbo FFI for maximum JPEG speed) [PHASE 8]
- [x] `rayon` parallelized pixel conversion for large buffers [PHASE 8]
- [x] WebP output format support [PHASE 9]

### Features
- [x] Native region selector (replace `slurp` dependency with built-in Wayland overlay) [PHASE 9]
- [x] Annotation mode (pipe to `satty` or built-in annotation overlay) [PHASE 8]
- [x] Delay timer (`--delay <seconds>`) [PHASE 9]
- [x] Native clipboard (`wl_data_device` protocol instead of `wl-copy` subprocess) [PHASE 8]
- [x] Freeze overlay (capture full frame → display as `wlr-layer-shell` surface → run slurp on top) [PHASE 8.5, Note: make sure the freeze overlay is a floating window above all other windows so it doesn't open side by side with the current used apps and missing up the select region (must work on mangowc and hyprland)]
- [x] Window capture via `ext_foreign_toplevel_image_capture_source_manager_v1` [PHASE 8.5]
- [x] Active window detection and capture [PHASE 9, Also it would be best if we could ignore windows that we don't want to capture like file manager] 
- [x] Configurable slurp colors via config file or CLI [PHASE 9]
- [x] Multi-output stitching (capture all monitors into one image) [PHASE 10]
- [x] Notification integration (`notify-send` or native D-Bus notification) [PHASE 9]
- [ ] Output to specific display by index (not just name) [PHASE 10]

### Packaging
- [x] `man` page generation [PHASE 8]
- [x] Shell completions (bash, zsh, fish) via clap [PHASE 8]
- [ ] AUR package [PHASE 10]
- [ ] Flatpak manifest [PHASE 10]
- [ ] CI/CD pipeline [PHASE 10]

## License

MIT
