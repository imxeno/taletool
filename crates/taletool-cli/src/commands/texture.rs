//! Handlers for `taletool texture` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::json;
use taletool_texture::{DecodedTexture, TextureFormat, decode_texture};

use crate::cli::TextureCommand;
use crate::texture_file::{pack_texture_dir, unpack_texture_file};
use crate::util::fnv1a64;

/// Dispatch a `texture` subcommand.
pub(crate) fn run_texture(command: TextureCommand) -> anyhow::Result<()> {
    match command {
        TextureCommand::Inspect {
            input,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let texture = decode_texture(&data)
                .with_context(|| format!("decoding texture {}", input.display()))?;
            inspect_texture(
                &texture,
                data.len(),
                checksum.then(|| fnv1a64(&data)),
                json_output,
            )
        }
        TextureCommand::Unpack { input, out } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let texture = decode_texture(&data)
                .with_context(|| format!("decoding texture {}", input.display()))?;
            let count = unpack_texture_file(&texture, &out)?;
            println!("unpacked {count} texture mip levels into {}", out.display());
            Ok(())
        }
        TextureCommand::Pack { dir, out } => {
            let (header, count) = pack_texture_dir(&dir, &out)?;
            println!(
                "packed {}x{} {} texture with {count} mip levels into {}",
                header.width,
                header.height,
                format_label(header.format),
                out.display()
            );
            Ok(())
        }
    }
}

fn inspect_texture(
    texture: &DecodedTexture,
    encoded_size: usize,
    checksum: Option<u64>,
    json_output: bool,
) -> anyhow::Result<()> {
    let header = texture.header;
    let mip_levels = texture
        .mip_levels
        .iter()
        .enumerate()
        .map(|(level, image)| {
            json!({
                "level": level,
                "width": image.width(),
                "height": image.height(),
                "encoded_size": image.width() as u64
                    * image.height() as u64
                    * header.format.bytes_per_pixel() as u64,
            })
        })
        .collect::<Vec<_>>();

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "texture",
                "encoded_size": encoded_size,
                "width": header.width,
                "height": header.height,
                "pixel_format": format_label(header.format),
                "filter_flag": header.filter_flag,
                "linear_filtering": header.uses_linear_filtering(),
                "unknown_06": header.unknown_06,
                "stored_mip_level_count": header.mip_level_count,
                "effective_mip_level_count": header.effective_mip_level_count(),
                "checksum_fnv1a64": checksum.map(|value| format!("{value:016x}")),
                "mip_levels": mip_levels,
            }))?
        );
    } else {
        println!("type: texture");
        println!("encoded_size: {encoded_size}");
        println!("size: {}x{}", header.width, header.height);
        println!("pixel_format: {}", format_label(header.format));
        println!("filter_flag: {}", header.filter_flag);
        println!("linear_filtering: {}", header.uses_linear_filtering());
        println!("unknown_06: {}", header.unknown_06);
        println!("stored_mip_level_count: {}", header.mip_level_count);
        println!(
            "effective_mip_level_count: {}",
            header.effective_mip_level_count()
        );
        if let Some(checksum) = checksum {
            println!("checksum_fnv1a64: {checksum:016x}");
        }
        for level in mip_levels {
            println!(
                "  level={:<3} size={}x{} encoded_size={}",
                level["level"].as_u64().unwrap_or_default(),
                level["width"].as_u64().unwrap_or_default(),
                level["height"].as_u64().unwrap_or_default(),
                level["encoded_size"].as_u64().unwrap_or_default(),
            );
        }
    }
    Ok(())
}

fn format_label(format: TextureFormat) -> &'static str {
    match format {
        TextureFormat::A4R4G4B4 => "a4r4g4b4",
        TextureFormat::A1R5G5B5 => "a1r5g5b5",
        TextureFormat::A8R8G8B8 => "a8r8g8b8",
        TextureFormat::L8 => "l8",
        TextureFormat::A8 => "a8",
    }
}
