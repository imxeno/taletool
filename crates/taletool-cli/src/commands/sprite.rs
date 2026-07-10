//! Handlers for `taletool sprite` asset commands.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde_json::json;
use taletool_texture::sprite::free_size::{FreeSizeSprite, decode_free_size_sprite};
use taletool_texture::sprite::{DecodedSprite, decode_sprite};

use crate::cli::{SpriteCommand, SpriteKindArg};
use crate::sprite_file::{
    pack_free_size_sprite_png, pack_sprite_dir, unpack_free_size_sprite_png, unpack_sprite_file,
    unpack_sprite_png,
};
use crate::util::fnv1a64;

enum DecodedSpriteKind {
    MapObject(DecodedSprite),
    FreeSize(FreeSizeSprite),
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
            let decoded = decode_sprite_kind(&data, kind)?;
            let checksum_value = checksum.then(|| format!("{:016x}", fnv1a64(&data)));

            match decoded {
                DecodedSpriteKind::MapObject(sprite) => {
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
                DecodedSpriteKind::FreeSize(sprite) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "type": "sprite",
                                "kind": "free-size",
                                "pixel_format": "a8r8g8b8",
                                "encoded_size": data.len(),
                                "width": sprite.width(),
                                "height": sprite.height(),
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
                        println!("size: {}x{}", sprite.width(), sprite.height());
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
            png_only,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            match decode_sprite_kind(&data, kind)? {
                DecodedSpriteKind::MapObject(sprite) if png_only => {
                    unpack_sprite_png(&sprite, &out)?;
                    println!("unpacked map-object sprite PNG into {}", out.display());
                }
                DecodedSpriteKind::MapObject(sprite) => {
                    let count = unpack_sprite_file(&sprite, &out)?;
                    println!(
                        "unpacked {count} map-object sprite frames into {}",
                        out.display()
                    );
                }
                DecodedSpriteKind::FreeSize(sprite) => {
                    unpack_free_size_sprite_png(&sprite, &out)?;
                    println!("unpacked free-size sprite PNG into {}", out.display());
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
                    let (width, height) = pack_free_size_sprite_png(&input, &out)?;
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

fn decode_sprite_kind(data: &[u8], kind: SpriteKindArg) -> anyhow::Result<DecodedSpriteKind> {
    match kind {
        SpriteKindArg::MapObject => decode_sprite(data)
            .map(DecodedSpriteKind::MapObject)
            .context("decoding map-object sprite"),
        SpriteKindArg::FreeSize => decode_free_size_sprite(data)
            .map(DecodedSpriteKind::FreeSize)
            .context("decoding free-size sprite"),
        SpriteKindArg::Auto => {
            let map_object = decode_sprite(data);
            let free_size = decode_free_size_sprite(data);
            match (map_object, free_size) {
                (Ok(_), Ok(_)) => bail!(
                    "sprite payload matches both map-object and free-size formats; select --kind map-object or --kind free-size"
                ),
                (Ok(sprite), Err(_)) => Ok(DecodedSpriteKind::MapObject(sprite)),
                (Err(_), Ok(sprite)) => Ok(DecodedSpriteKind::FreeSize(sprite)),
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
        SpriteKindArg::Auto if input.is_file() && has_png_extension(input) => {
            SpriteKindArg::FreeSize
        }
        SpriteKindArg::Auto => bail!(
            "cannot infer sprite kind from {}; provide a map-object sprite directory or free-size sprite PNG, or select --kind",
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
                "free-size sprite input must be a PNG file: {}",
                input.display()
            )
        }
        SpriteKindArg::FreeSize if !has_png_extension(input) => bail!(
            "free-size sprite input must use a .png extension: {}",
            input.display()
        ),
        _ => Ok(resolved),
    }
}

fn has_png_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
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
            decode_sprite_kind(&map_object, SpriteKindArg::Auto).unwrap(),
            DecodedSpriteKind::MapObject(_)
        ));

        let free_size = write_free_size_sprite_bytes(&RgbaImage::new(1024, 1)).unwrap();
        assert!(matches!(
            decode_sprite_kind(&free_size, SpriteKindArg::Auto).unwrap(),
            DecodedSpriteKind::FreeSize(_)
        ));

        assert!(decode_sprite_kind(&map_object, SpriteKindArg::FreeSize).is_err());
        assert!(decode_sprite_kind(&free_size, SpriteKindArg::MapObject).is_err());
        assert!(decode_sprite_kind(&[], SpriteKindArg::Auto).is_err());
    }

    #[test]
    fn auto_detection_rejects_an_ambiguous_payload() {
        let mut image = RgbaImage::new(257, 256);
        image.put_pixel(1, 0, Rgba([0, 13, 0, 0]));
        let bytes = write_free_size_sprite_bytes(&image).unwrap();

        assert!(decode_sprite(&bytes).is_ok());
        assert!(decode_free_size_sprite(&bytes).is_ok());
        let error = decode_sprite_kind(&bytes, SpriteKindArg::Auto)
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
        let other = root.join("free_size.bin");
        fs::create_dir_all(&directory).unwrap();
        fs::write(&png, b"placeholder").unwrap();
        fs::write(&other, b"placeholder").unwrap();

        assert_eq!(
            resolve_pack_kind(&directory, SpriteKindArg::Auto).unwrap(),
            SpriteKindArg::MapObject
        );
        assert_eq!(
            resolve_pack_kind(&png, SpriteKindArg::Auto).unwrap(),
            SpriteKindArg::FreeSize
        );
        assert!(resolve_pack_kind(&png, SpriteKindArg::MapObject).is_err());
        assert!(resolve_pack_kind(&directory, SpriteKindArg::FreeSize).is_err());
        assert!(resolve_pack_kind(&other, SpriteKindArg::Auto).is_err());

        fs::remove_dir_all(root).unwrap();
    }
}
