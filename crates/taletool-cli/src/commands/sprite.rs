//! Handlers for `taletool sprite` asset commands.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde_json::json;
use taletool_texture::sprite::free_size::{
    FreeSizeSpriteInfo, decode_free_size_sprite, inspect_free_size_sprite,
};
use taletool_texture::sprite::{SpritePayload, parse_sprite};

use crate::cli::{SpriteCommand, SpriteKindArg};
use crate::sprite_file::{
    SpriteImageFormat, pack_free_size_sprite_image, pack_sprite_dir, unpack_free_size_sprite_file,
    unpack_sprite_file, unpack_sprite_png,
};
use crate::util::fnv1a64;

enum ParsedSpriteKind<'a> {
    MapObject(SpritePayload<'a>),
    FreeSize(FreeSizeSpriteInfo),
}

/// Dispatch a `sprite` subcommand.
pub(crate) fn run_sprite(command: SpriteCommand) -> anyhow::Result<()> {
    match command {
        SpriteCommand::Inspect {
            input,
            kind,
            json,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let parsed = parse_sprite_kind(&data, kind)?;
            let checksum_value = checksum.then(|| format!("{:016x}", fnv1a64(&data)));

            match parsed {
                ParsedSpriteKind::MapObject(sprite) => {
                    let frames = sprite
                        .frames
                        .iter()
                        .enumerate()
                        .map(|(index, frame)| {
                            json!({
                                "index": index,
                                "width": frame.width(),
                                "height": frame.height(),
                                "source_x": frame.source_x,
                                "source_y": frame.source_y,
                                "data_offset": frame.data_offset,
                            })
                        })
                        .collect::<Vec<_>>();

                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "type": "sprite",
                                "kind": "map-object",
                                "pixel_format": "a4r4g4b4",
                                "encoded_size": data.len(),
                                "frame_count": frames.len(),
                                "checksum_fnv1a64": checksum_value,
                                "frames": frames,
                            }))?
                        );
                    } else {
                        println!("type: sprite");
                        println!("kind: map-object");
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
                }
                ParsedSpriteKind::FreeSize(sprite) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "type": "sprite",
                                "kind": "free-size",
                                "pixel_format": "a8r8g8b8",
                                "encoded_size": data.len(),
                                "width": sprite.width,
                                "height": sprite.height,
                                "block_columns": sprite.block_columns(),
                                "block_rows": sprite.block_rows(),
                                "checksum_fnv1a64": checksum_value,
                            }))?
                        );
                    } else {
                        println!("type: sprite");
                        println!("kind: free-size");
                        println!("pixel_format: a8r8g8b8");
                        println!("encoded_size: {}", data.len());
                        println!("size: {}x{}", sprite.width, sprite.height);
                        println!("blocks: {}x{}", sprite.block_columns(), sprite.block_rows());
                        if let Some(checksum_value) = checksum_value {
                            println!("checksum_fnv1a64: {checksum_value}");
                        }
                    }
                }
            }
            Ok(())
        }
        SpriteCommand::Unpack {
            input,
            out,
            kind,
            format,
            png_only,
        } => {
            if png_only && format == SpriteImageFormat::Bmp {
                bail!(
                    "--png-only cannot be combined with --format bmp; use sprite directory output"
                );
            }
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            match parse_sprite_kind(&data, kind)? {
                ParsedSpriteKind::MapObject(sprite) if png_only => {
                    unpack_sprite_png(&sprite, &out)?;
                    println!("unpacked map-object sprite PNG into {}", out.display());
                }
                ParsedSpriteKind::MapObject(sprite) => {
                    let count = unpack_sprite_file(&sprite, &out, format)?;
                    println!(
                        "unpacked {count} map-object sprite frames into {}",
                        out.display()
                    );
                }
                ParsedSpriteKind::FreeSize(_) => {
                    let sprite = decode_free_size_sprite(&data)?;
                    unpack_free_size_sprite_file(&sprite, &out, format)?;
                    println!("unpacked free-size sprite {format} into {}", out.display());
                }
            }
            Ok(())
        }
        SpriteCommand::Pack { input, out, kind } => {
            match resolve_pack_kind(&input, kind)? {
                SpriteKindArg::MapObject => {
                    let count = pack_sprite_dir(&input, &out)?;
                    println!(
                        "packed {count} map-object sprite frames into {}",
                        out.display()
                    );
                }
                SpriteKindArg::FreeSize => {
                    let (width, height) = pack_free_size_sprite_image(&input, &out)?;
                    println!(
                        "packed {width}x{height} free-size sprite into {}",
                        out.display()
                    );
                }
                SpriteKindArg::Auto => unreachable!("pack kind resolution returns a concrete kind"),
            }
            Ok(())
        }
    }
}

fn parse_sprite_kind(data: &[u8], kind: SpriteKindArg) -> anyhow::Result<ParsedSpriteKind<'_>> {
    match kind {
        SpriteKindArg::MapObject => parse_sprite(data)
            .map(ParsedSpriteKind::MapObject)
            .context("decoding map-object sprite"),
        SpriteKindArg::FreeSize => inspect_free_size_sprite(data)
            .map(ParsedSpriteKind::FreeSize)
            .context("decoding free-size sprite"),
        SpriteKindArg::Auto => {
            let map_object = parse_sprite(data);
            let free_size = inspect_free_size_sprite(data);
            match (map_object, free_size) {
                (Ok(_), Ok(_)) => bail!(
                    "sprite payload matches both map-object and free-size formats; select --kind map-object or --kind free-size"
                ),
                (Ok(sprite), Err(_)) => Ok(ParsedSpriteKind::MapObject(sprite)),
                (Err(_), Ok(sprite)) => Ok(ParsedSpriteKind::FreeSize(sprite)),
                (Err(map_object_error), Err(free_size_error)) => bail!(
                    "sprite payload matches neither supported format; map-object: {map_object_error}; free-size: {free_size_error}"
                ),
            }
        }
    }
}

fn resolve_pack_kind(input: &Path, kind: SpriteKindArg) -> anyhow::Result<SpriteKindArg> {
    let resolved = match kind {
        SpriteKindArg::Auto if input.is_dir() => SpriteKindArg::MapObject,
        SpriteKindArg::Auto if input.is_file() && has_sprite_image_extension(input) => {
            SpriteKindArg::FreeSize
        }
        SpriteKindArg::Auto => bail!(
            "cannot infer sprite kind from {}; provide a map-object sprite directory or free-size sprite PNG/BMP, or select --kind",
            input.display()
        ),
        concrete => concrete,
    };

    match resolved {
        SpriteKindArg::MapObject if !input.is_dir() => bail!(
            "map-object sprite input must be a manifest-backed directory: {}",
            input.display()
        ),
        SpriteKindArg::FreeSize if !input.is_file() => {
            bail!(
                "free-size sprite input must be a PNG or BMP file: {}",
                input.display()
            )
        }
        SpriteKindArg::FreeSize if !has_sprite_image_extension(input) => bail!(
            "free-size sprite input must use a .png or .bmp extension: {}",
            input.display()
        ),
        _ => Ok(resolved),
    }
}

fn has_sprite_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("png") || extension.eq_ignore_ascii_case("bmp")
        })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{Rgba, RgbaImage};
    use taletool_texture::sprite::SpriteFrame;
    use taletool_texture::sprite::free_size::write_free_size_sprite_bytes;
    use taletool_texture::sprite::write_sprite_bytes;

    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn auto_detects_map_object_and_free_size_payloads() {
        let map_object =
            write_sprite_bytes(&[SpriteFrame::new(0, 0, RgbaImage::new(1, 1))]).unwrap();
        assert!(matches!(
            parse_sprite_kind(&map_object, SpriteKindArg::Auto).unwrap(),
            ParsedSpriteKind::MapObject(_)
        ));

        let free_size = write_free_size_sprite_bytes(&RgbaImage::new(1024, 1)).unwrap();
        assert!(matches!(
            parse_sprite_kind(&free_size, SpriteKindArg::Auto).unwrap(),
            ParsedSpriteKind::FreeSize(_)
        ));

        assert!(parse_sprite_kind(&map_object, SpriteKindArg::FreeSize).is_err());
        assert!(parse_sprite_kind(&free_size, SpriteKindArg::MapObject).is_err());
        assert!(parse_sprite_kind(&[], SpriteKindArg::Auto).is_err());
    }

    #[test]
    fn auto_detection_rejects_an_ambiguous_payload() {
        let mut image = RgbaImage::new(257, 256);
        image.put_pixel(1, 0, Rgba([0, 13, 0, 0]));
        let bytes = write_free_size_sprite_bytes(&image).unwrap();

        assert!(parse_sprite(&bytes).is_ok());
        assert!(decode_free_size_sprite(&bytes).is_ok());
        let error = parse_sprite_kind(&bytes, SpriteKindArg::Auto)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("matches both map-object and free-size formats"));
    }

    #[test]
    fn infers_pack_kind_from_input_shape() {
        let root = temp_path("sprite-pack-kind");
        let directory = root.join("map_object");
        let png = root.join("free_size.PNG");
        let bmp = root.join("free_size.BMP");
        let other = root.join("free_size.bin");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&png, b"placeholder").unwrap();
        fs::write(&bmp, b"placeholder").unwrap();
        fs::write(&other, b"placeholder").unwrap();

        assert_eq!(
            resolve_pack_kind(&directory, SpriteKindArg::Auto).unwrap(),
            SpriteKindArg::MapObject
        );
        assert_eq!(
            resolve_pack_kind(&png, SpriteKindArg::Auto).unwrap(),
            SpriteKindArg::FreeSize
        );
        assert_eq!(
            resolve_pack_kind(&bmp, SpriteKindArg::Auto).unwrap(),
            SpriteKindArg::FreeSize
        );
        assert!(resolve_pack_kind(&png, SpriteKindArg::MapObject).is_err());
        assert!(resolve_pack_kind(&directory, SpriteKindArg::FreeSize).is_err());
        assert!(resolve_pack_kind(&other, SpriteKindArg::Auto).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_rejects_bmp_with_png_only_before_creating_output() {
        let root = temp_path("sprite-format-conflict");
        let error = run_sprite(SpriteCommand::Unpack {
            input: root.join("input.bin"),
            out: root.join("output"),
            kind: SpriteKindArg::Auto,
            format: SpriteImageFormat::Bmp,
            png_only: true,
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("--png-only cannot be combined with --format bmp")
        );
        assert!(!root.exists());
    }

    #[test]
    fn command_round_trips_bmp_directory_with_empty_and_oversized_frames() {
        let root = temp_path("sprite-bmp-command");
        fs::create_dir_all(&root).unwrap();
        let input = root.join("input.bin");
        let out = root.join("frames");
        let rebuilt = root.join("rebuilt.bin");
        let bytes = write_sprite_bytes(&[
            SpriteFrame::new(-7, 12, RgbaImage::new(0, 17)),
            SpriteFrame::new(
                4,
                -3,
                RgbaImage::from_pixel(513, 2, Rgba([17, 34, 51, 136])),
            ),
        ])
        .unwrap();
        fs::write(&input, &bytes).unwrap();
        run_sprite(SpriteCommand::Unpack {
            input,
            out: out.clone(),
            kind: SpriteKindArg::Auto,
            format: SpriteImageFormat::Bmp,
            png_only: false,
        })
        .unwrap();
        assert!(out.join("frame-001.bmp").is_file());
        assert!(!out.join("frame-000.bmp").exists());
        run_sprite(SpriteCommand::Pack {
            input: out,
            out: rebuilt.clone(),
            kind: SpriteKindArg::Auto,
        })
        .unwrap();
        assert_eq!(fs::read(rebuilt).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }
}
