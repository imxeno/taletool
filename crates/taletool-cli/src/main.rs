//! Command-line interface for inspecting and transforming NosTale data files.

mod animation_file;
mod archive_detect;
mod binary_payloads;
mod binary_preset;
mod ccinf_file;
mod cli;
mod commands;
mod effect_file;
mod geometry_file;
mod height_grid_file;
mod map_file;
mod map_neighborhood_file;
mod paths;
mod sound_info;
mod sound_pack;
mod sprite_file;
mod sprite_remap_file;
mod text_payload;
mod texture_file;
mod util;

use clap::Parser;
use cli::{Cli, Command};
use commands::{
    animation::run_animation, archive::run_archive, audio::run_audio, ccinf::run_ccinf,
    cell_flag::run_cell_flag, effect::run_effect, geometry::run_geometry,
    height_grid::run_height_grid, map::run_map, map_neighborhood::run_map_neighborhood,
    patch::run_patch, scan::run_scan, sprite::run_sprite, sprite_remap::run_sprite_remap,
    text::run_text, texture::run_texture,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose)?;

    match cli.command {
        Command::Scan {
            data_dir,
            json,
            show_unsupported,
            no_recursive,
        } => run_scan(
            data_dir,
            cli.verbose > 0,
            json,
            show_unsupported,
            !no_recursive,
        ),
        Command::Archive { command } => run_archive(command),
        Command::Animation { command } => run_animation(command),
        Command::Map { command } => run_map(command),
        Command::Ccinf { command } => run_ccinf(command),
        Command::Effect { command } => run_effect(command),
        Command::Geometry { command } => run_geometry(command),
        Command::HeightGrid { command } => run_height_grid(command),
        Command::MapNeighborhood { command } => run_map_neighborhood(command),
        Command::Sprite { command } => run_sprite(command),
        Command::SpriteRemap { command } => run_sprite_remap(command),
        Command::Patch { command } => run_patch(command).await,
        Command::Text { command } => run_text(command),
        Command::Texture { command } => run_texture(command),
        Command::CellFlag { command } => run_cell_flag(command),
        Command::Audio { command } => run_audio(command),
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
