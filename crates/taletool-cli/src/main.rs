//! Command-line interface for inspecting and transforming NosTale data files.

mod archive_detect;
mod binary_payloads;
mod binary_preset;
mod ccinf_file;
mod cli;
mod commands;
mod geometry_file;
mod paths;
mod sound_pack;
mod sprite_file;
mod text_payload;
mod util;

use clap::Parser;
use cli::{Cli, Command};
use commands::{
    archive::run_archive, ccinf::run_ccinf, geometry::run_geometry, patch::run_patch,
    scan::run_scan, sprite::run_sprite, text::run_text,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose)?;

    match cli.command {
        Command::Scan { data_dir, json } => run_scan(data_dir, cli.verbose > 0, json),
        Command::Archive { command } => run_archive(command),
        Command::Ccinf { command } => run_ccinf(command),
        Command::Geometry { command } => run_geometry(command),
        Command::Sprite { command } => run_sprite(command),
        Command::Patch { command } => run_patch(command).await,
        Command::Text { command } => run_text(command),
    }
}

/// Initialize process-wide tracing from verbosity flags or `RUST_LOG`.
fn init_tracing(verbose: u8) -> anyhow::Result<()> {
    let default_filter = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = if std::env::var_os("RUST_LOG").is_some() {
        EnvFilter::try_from_default_env()?
    } else {
        EnvFilter::try_new(default_filter)?
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?;
    Ok(())
}
