pub mod config;
mod app;
mod backends;
mod buffers;
mod cli;
mod encoders;
mod error;
mod utils;

pub use cli::*;
use clap::Parser;



fn main() {
    let cli = Cli::parse();

    if let Err(e) = app::run(&cli) {
        eprintln!("wlsnip: {e}");
        std::process::exit(1);
    }
}
