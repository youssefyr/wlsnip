#[path = "src/cli.rs"]
mod cli;

use clap::CommandFactory;
use clap_complete::{generate_to, shells::{Bash, Fish, Zsh}};
use clap_mangen::Man;
use std::env;
use std::fs::File;
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    // Determine the output directory
    let out_dir = match env::var_os("OUT_DIR") {
        None => return Ok(()),
        Some(out_dir) => out_dir,
    };
    let out_dir = PathBuf::from(out_dir);

    // Create the App Command
    let mut app = cli::Cli::command();

    // Generate man page
    let man_file = out_dir.join("wlsnip.1");
    let mut file = File::create(&man_file)?;
    Man::new(app.clone()).render(&mut file)?;
    println!("cargo:warning=Man page generated: {}", man_file.display());

    // Generate shell completions
    generate_to(Bash, &mut app, "wlsnip", &out_dir)?;
    generate_to(Zsh, &mut app, "wlsnip", &out_dir)?;
    generate_to(Fish, &mut app, "wlsnip", &out_dir)?;

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}
