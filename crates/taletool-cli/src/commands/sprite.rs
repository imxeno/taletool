//! Handlers for `taletool sprite` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::json;
use taletool_texture::sprite::decode_sprite;

use crate::cli::SpriteCommand;
use crate::sprite_file::{pack_sprite_dir, unpack_sprite_file, unpack_sprite_png};
use crate::util::fnv1a64;

/// Dispatch a `sprite` subcommand.
pub(crate) fn run_sprite(command: SpriteCommand) -> anyhow::Result<()> {
    match command {
        SpriteCommand::Inspect {
            input,
            json,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let sprite = decode_sprite(&data)?;
            let frames = sprite
                .frames
                .iter()
                .enumerate()
                .map(|(index, decoded)| {
                    json!({
                        "index": index,
                        "width": decoded.width(),
                        "height": decoded.height(),
                        "source_x": decoded.frame.source_x,
                        "source_y": decoded.frame.source_y,
                        "data_offset": decoded.data_offset,
                    })
                })
                .collect::<Vec<_>>();
            let checksum_value = checksum.then(|| format!("{:016x}", fnv1a64(&data)));

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "type": "sprite",
                        "pixel_format": "a4r4g4b4",
                        "encoded_size": data.len(),
                        "frame_count": frames.len(),
                        "checksum_fnv1a64": checksum_value,
                        "frames": frames,
                    }))?
                );
            } else {
                println!("type: sprite");
                println!("pixel_format: a4r4g4b4");
                println!("encoded_size: {}", data.len());
                println!("frames: {}", frames.len());
                if let Some(checksum_value) = checksum_value {
                    println!("checksum_fnv1a64: {checksum_value}");
                }
                for frame in frames {
                    println!(
                        "  frame={:<3} size={}x{} source=({}, {}) data_offset={}",
                        frame["index"].as_u64().unwrap_or_default(),
                        frame["width"].as_u64().unwrap_or_default(),
                        frame["height"].as_u64().unwrap_or_default(),
                        frame["source_x"].as_i64().unwrap_or_default(),
                        frame["source_y"].as_i64().unwrap_or_default(),
                        frame["data_offset"].as_u64().unwrap_or_default(),
                    );
                }
            }
            Ok(())
        }
        SpriteCommand::Unpack {
            input,
            out,
            png_only,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let sprite = decode_sprite(&data)?;
            if png_only {
                unpack_sprite_png(&sprite, &out)?;
                println!("unpacked sprite PNG into {}", out.display());
            } else {
                let count = unpack_sprite_file(&sprite, &out)?;
                println!("unpacked {count} sprite frames into {}", out.display());
            }
            Ok(())
        }
        SpriteCommand::Pack { dir, out } => {
            let count = pack_sprite_dir(&dir, &out)?;
            println!("packed {count} sprite frames into {}", out.display());
            Ok(())
        }
    }
}
