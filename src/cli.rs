use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// wlsnip — A high-performance Wayland screenshot utility
#[derive(Parser, Debug)]
#[command(name = "wlsnip", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Save screenshot to this file path
    #[arg(global = true, short, long, value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Output image format
    #[arg(global = true, short, long, default_value = "png")]
    pub format: ImageFormat,

    /// Copy screenshot to clipboard natively
    #[arg(global = true, short, long)]
    pub clipboard: Option<bool>,

    /// Disable copying to clipboard
    #[arg(global = true, long)]
    pub no_clipboard: bool,

    /// Write image to stdout instead of file
    #[arg(global = true, short, long)]
    pub stdout: bool,

    /// Force a specific capture backend
    #[arg(global = true, long, value_name = "BACKEND")]
    pub backend: Option<BackendChoice>,

    /// JPEG quality (1-100)
    #[arg(global = true, long)]
    pub jpeg_quality: Option<u8>,

    /// Exclude cursor from capture
    #[arg(global = true, long)]
    pub no_cursor: bool,

    /// Open screenshot in satty for annotation
    #[arg(global = true, short, long)]
    pub annotate: bool,

    /// Delay in seconds before capturing
    #[arg(global = true, long, default_value = "0")]
    pub delay: u64,

    /// Send a desktop notification upon successful capture
    #[arg(global = true, long)]
    pub notify: bool,

    /// Color for the region selector (slurp)
    #[arg(global = true, long)]
    pub selection_color: Option<String>,

    /// Add a drop shadow to the captured image
    #[arg(global = true, long)]
    pub shadow: bool,

    /// Add padding around the captured image (in pixels)
    #[arg(global = true, long)]
    pub padding: Option<u32>,

    /// Upload to image host and copy URL to clipboard
    #[arg(global = true, long)]
    pub upload: bool,

    /// Pin the captured image as a floating window
    #[arg(global = true, long)]
    pub pin: bool,

    /// Comma-separated list of app_ids to ignore when capturing active window
    #[arg(global = true, long, value_delimiter = ',', default_value = "nautilus,dolphin,thunar")]
    pub ignore_apps: Option<Vec<String>>,

    /// Use slurp instead of the built-in Wayland native region selector
    #[arg(global = true, long)]
    pub slurp: bool,

    /// Run speed benchmark and print timing logs. If a number is provided, 
    /// runs an automated benchmark simulating dragging interactions N times.
    #[arg(global = true, long, value_name = "NUM")]
    pub bench_auto: Option<Option<u32>>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Capture a selected region (requires slurp)
    Region,
    /// Capture the entire screen (all outputs)
    Full,
    /// Capture a specific output by name
    Output {
        /// Output name (e.g. "DP-1", "HDMI-A-1")
        name: String,
    },
    /// Capture a specific window by matching title or app_id
    Window {
        /// Match substring in window title or app_id
        query: Option<String>,
    },
}

#[derive(ValueEnum, Clone, Debug)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum BackendChoice {
    /// ext-image-copy-capture-v1
    Ext,
    /// wlr-screencopy-unstable-v1
    Wlr,
    /// XDG Desktop Portal
    Portal,
}
