//! Manifest and image helpers for sprite payloads.

use std::collections::HashSet;
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use clap::ValueEnum;
use image::{ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use taletool_texture::sprite::free_size::{FreeSizeSprite, write_free_size_sprite_bytes};
use taletool_texture::sprite::{
    SPRITE_MAX_DIMENSION, SpriteFrame, SpritePayload, write_sprite_bytes,
};

use crate::sprite_bmp::write_bmp;

pub(crate) const SPRITE_MANIFEST_FILE: &str = "sprite.json";
const SPRITE_DOCUMENT_FORMAT: &str = "sprite";
const SPRITE_DOCUMENT_VERSION: u32 = 1;
const SPRITE_DOCUMENT_VERSION_EXTENDED: u32 = 2;
const SPRITE_PIXEL_FORMAT: &str = "a4r4g4b4";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SpriteImageFormat {
    Png,
    Bmp,
}

impl std::fmt::Display for SpriteImageFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Png => "PNG",
            Self::Bmp => "BMP",
        })
    }
}

impl SpriteImageFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Bmp => "bmp",
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Bmp => ImageFormat::Bmp,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteDocument {
    format: String,
    version: u32,
    pixel_format: String,
    frames: Vec<SpriteFrameDocument>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteFrameDocument {
    image: Option<String>,
    source_x: i16,
    source_y: i16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    encoding: Option<SpriteImageFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
}

/// Export one frame at a time, retaining empty frames in `sprite.json`.
pub(crate) fn unpack_sprite_file(
    sprite: &SpritePayload<'_>,
    out: &Path,
    format: SpriteImageFormat,
) -> anyhow::Result<usize> {
    fs::create_dir_all(out)?;
    let extended =
        format == SpriteImageFormat::Bmp || sprite.frames.iter().any(|frame| frame.is_empty());
    let mut frames = Vec::with_capacity(sprite.frames.len());
    for (index, frame) in sprite.frames.iter().enumerate() {
        let empty = frame.is_empty();
        let image = if empty {
            None
        } else {
            let image = format!("frame-{index:03}.{}", format.extension());
            let image_path = out.join(&image);
            match format {
                SpriteImageFormat::Png => frame
                    .decode()?
                    .frame
                    .image
                    .save_with_format(&image_path, ImageFormat::Png)
                    .with_context(|| format!("writing {}", image_path.display()))?,
                SpriteImageFormat::Bmp => {
                    let mut writer = BufWriter::new(
                        fs::File::create(&image_path)
                            .with_context(|| format!("creating {}", image_path.display()))?,
                    );
                    write_bmp(&mut writer, frame.width(), frame.height(), frame.pixels())
                        .with_context(|| format!("writing {}", image_path.display()))?;
                    writer
                        .flush()
                        .with_context(|| format!("flushing {}", image_path.display()))?;
                }
            }
            Some(image)
        };
        frames.push(SpriteFrameDocument {
            image,
            source_x: frame.source_x,
            source_y: frame.source_y,
            encoding: (extended && !empty).then_some(format),
            width: extended.then_some(frame.width()),
            height: extended.then_some(frame.height()),
        });
    }

    let document = SpriteDocument {
        format: SPRITE_DOCUMENT_FORMAT.to_owned(),
        version: if extended {
            SPRITE_DOCUMENT_VERSION_EXTENDED
        } else {
            SPRITE_DOCUMENT_VERSION
        },
        pixel_format: SPRITE_PIXEL_FORMAT.to_owned(),
        frames,
    };
    fs::write(
        out.join(SPRITE_MANIFEST_FILE),
        serde_json::to_vec_pretty(&document)?,
    )?;
    Ok(document.frames.len())
}

/// Write a single-frame sprite directly to one PNG without a manifest.
pub(crate) fn unpack_sprite_png(sprite: &SpritePayload<'_>, out: &Path) -> anyhow::Result<()> {
    if sprite.frames.len() != 1 {
        bail!(
            "PNG-only sprite extraction requires exactly one frame; payload has {} frames",
            sprite.frames.len()
        );
    }
    if sprite.frames[0].is_empty() {
        bail!("PNG-only extraction cannot represent an empty frame; use sprite directory output");
    }
    ensure_png_path(out, "PNG-only sprite output")?;

    create_parent_dir(out)?;
    sprite.frames[0]
        .decode()?
        .frame
        .image
        .save_with_format(out, ImageFormat::Png)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Write a decoded free-size sprite directly to one image without a manifest.
pub(crate) fn unpack_free_size_sprite_file(
    sprite: &FreeSizeSprite,
    out: &Path,
    format: SpriteImageFormat,
) -> anyhow::Result<()> {
    ensure_image_path(out, format, "free-size sprite output")?;
    create_parent_dir(out)?;
    match format {
        SpriteImageFormat::Png => sprite
            .image
            .save_with_format(out, ImageFormat::Png)
            .with_context(|| format!("writing {}", out.display()))?,
        SpriteImageFormat::Bmp => {
            let mut writer = BufWriter::new(fs::File::create(out)?);
            write_bmp(
                &mut writer,
                sprite.width(),
                sprite.height(),
                sprite.image.pixels().copied(),
            )
            .with_context(|| format!("writing {}", out.display()))?;
            writer
                .flush()
                .with_context(|| format!("flushing {}", out.display()))?;
        }
    }
    Ok(())
}

/// Build and write a sprite payload from a manifest-backed directory.
pub(crate) fn pack_sprite_dir(dir: &Path, out: &Path) -> anyhow::Result<usize> {
    let manifest_path = dir.join(SPRITE_MANIFEST_FILE);
    let manifest_bytes =
        fs::read(&manifest_path).with_context(|| format!("reading {}", manifest_path.display()))?;
    let document: SpriteDocument = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    validate_document(&document)?;

    let mut seen_images = HashSet::new();
    let mut frames = Vec::with_capacity(document.frames.len());
    for (index, frame) in document.frames.iter().enumerate() {
        let Some(image_name) = &frame.image else {
            frames.push(SpriteFrame::new(
                frame.source_x,
                frame.source_y,
                RgbaImage::new(frame.width.unwrap(), frame.height.unwrap()),
            ));
            continue;
        };
        let normalized = image_name.to_lowercase();
        if !seen_images.insert(normalized) {
            bail!("sprite frame {index} repeats image path {image_name:?}");
        }
        let encoding = frame.encoding.unwrap_or(SpriteImageFormat::Png);
        let image_path = safe_frame_image_path(dir, index, image_name, encoding)?;
        let image = image::ImageReader::with_format(
            BufReader::new(
                fs::File::open(&image_path)
                    .with_context(|| format!("reading {}", image_path.display()))?,
            ),
            encoding.image_format(),
        )
        .decode()
        .with_context(|| format!("decoding {}", image_path.display()))?
        .to_rgba8();
        frames.push(SpriteFrame::new(frame.source_x, frame.source_y, image));
    }

    let bytes = write_sprite_bytes(&frames)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(frames.len())
}

/// Build and write a free-size sprite payload from one PNG or BMP.
pub(crate) fn pack_free_size_sprite_image(input: &Path, out: &Path) -> anyhow::Result<(u32, u32)> {
    let format = match input.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("png") => SpriteImageFormat::Png,
        Some(extension) if extension.eq_ignore_ascii_case("bmp") => SpriteImageFormat::Bmp,
        _ => bail!(
            "free-size sprite input must use a .png or .bmp extension: {}",
            input.display()
        ),
    };
    let bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let image = image::load_from_memory_with_format(&bytes, format.image_format())
        .with_context(|| format!("decoding {}", input.display()))?
        .to_rgba8();
    let dimensions = image.dimensions();
    let bytes = write_free_size_sprite_bytes(&image)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(dimensions)
}

fn validate_document(document: &SpriteDocument) -> anyhow::Result<()> {
    if document.format != SPRITE_DOCUMENT_FORMAT {
        bail!(
            "sprite document has unsupported format {:?}; expected {:?}",
            document.format,
            SPRITE_DOCUMENT_FORMAT
        );
    }
    if !matches!(
        document.version,
        SPRITE_DOCUMENT_VERSION | SPRITE_DOCUMENT_VERSION_EXTENDED
    ) {
        bail!(
            "sprite document has unsupported version {}; expected 1 or 2",
            document.version,
        );
    }
    for (index, frame) in document.frames.iter().enumerate() {
        if document.version == SPRITE_DOCUMENT_VERSION {
            if frame.image.is_none()
                || frame.encoding.is_some()
                || frame.width.is_some()
                || frame.height.is_some()
            {
                bail!(
                    "sprite frame {index} requires a PNG image and no version 2 fields in version 1"
                );
            }
        } else {
            let (Some(width), Some(height)) = (frame.width, frame.height) else {
                bail!("sprite frame {index} requires width and height in version 2");
            };
            if width > SPRITE_MAX_DIMENSION || height > SPRITE_MAX_DIMENSION {
                bail!(
                    "sprite frame {index} dimensions exceed the format limit of {SPRITE_MAX_DIMENSION}"
                );
            }
            let empty = width == 0 || height == 0;
            if empty && (frame.image.is_some() || frame.encoding.is_some()) {
                bail!("empty sprite frame {index} must have image: null and no encoding");
            }
            if !empty && (frame.image.is_none() || frame.encoding.is_none()) {
                bail!("populated sprite frame {index} requires an image and encoding");
            }
        }
    }
    if document.pixel_format != SPRITE_PIXEL_FORMAT {
        bail!(
            "sprite document has unsupported pixel format {:?}; expected {:?}",
            document.pixel_format,
            SPRITE_PIXEL_FORMAT
        );
    }
    Ok(())
}

fn safe_frame_image_path(
    root: &Path,
    index: usize,
    image: &str,
    format: SpriteImageFormat,
) -> anyhow::Result<PathBuf> {
    if image == SPRITE_MANIFEST_FILE {
        bail!("sprite frame {index} cannot reference its manifest file");
    }
    let path = Path::new(image);
    if path.is_absolute() {
        bail!("sprite frame {index} image path must be relative: {image}");
    }
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => bail!("sprite frame {index} image path must be a filename: {image}"),
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case(format.extension()))
    {
        bail!(
            "sprite frame {index} image must use a .{} extension: {image}",
            format.extension()
        );
    }
    Ok(root.join(path))
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn ensure_png_path(path: &Path, label: &str) -> anyhow::Result<()> {
    ensure_image_path(path, SpriteImageFormat::Png, label)
}

fn ensure_image_path(path: &Path, format: SpriteImageFormat, label: &str) -> anyhow::Result<()> {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case(format.extension()))
    {
        bail!(
            "{label} must use a .{} extension: {}",
            format.extension(),
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{Rgba, RgbaImage};
    use serde_json::json;
    use taletool_texture::sprite::free_size::{
        decode_free_size_sprite, write_free_size_sprite_bytes,
    };
    use taletool_texture::sprite::{SpriteFrame, decode_sprite, parse_sprite, write_sprite_bytes};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn test_sprite_bytes() -> Vec<u8> {
        let mut first = RgbaImage::new(2, 1);
        first.put_pixel(0, 0, Rgba([17, 34, 51, 255]));
        first.put_pixel(1, 0, Rgba([68, 85, 102, 119]));
        let mut second = RgbaImage::new(1, 1);
        second.put_pixel(0, 0, Rgba([255, 238, 221, 204]));
        write_sprite_bytes(&[
            SpriteFrame::new(-7, 12, first),
            SpriteFrame::new(4, -3, second),
        ])
        .unwrap()
    }

    #[test]
    fn png_manifest_round_trips_byte_for_byte() {
        let root = temp_dir("sprite-round-trip");
        let unpacked = root.join("unpacked");
        let output = root.join("rebuilt.bin");
        fs::create_dir_all(&root).unwrap();
        let bytes = test_sprite_bytes();
        let sprite = parse_sprite(&bytes).unwrap();

        assert_eq!(
            unpack_sprite_file(&sprite, &unpacked, SpriteImageFormat::Png).unwrap(),
            2
        );
        assert!(unpacked.join("frame-000.png").is_file());
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(unpacked.join(SPRITE_MANIFEST_FILE)).unwrap())
                .unwrap();
        assert_eq!(document["format"], SPRITE_DOCUMENT_FORMAT);
        assert_eq!(document["version"], SPRITE_DOCUMENT_VERSION);
        assert_eq!(document["pixel_format"], SPRITE_PIXEL_FORMAT);
        assert_eq!(document["frames"][0]["source_x"], -7);

        assert_eq!(pack_sprite_dir(&unpacked, &output).unwrap(), 2);
        assert_eq!(fs::read(&output).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn png_and_bmp_round_trip_empty_and_oversized_frames_without_losing_placement() {
        let root = temp_dir("sprite-empty-large");
        let frames = [(0, 0), (0, 65535), (65535, 0), (513, 2), (2, 513)]
            .into_iter()
            .enumerate()
            .map(|(index, (width, height))| {
                SpriteFrame::new(
                    -(index as i16),
                    index as i16 * 3,
                    RgbaImage::from_pixel(width, height, Rgba([17, 34, 51, 136])),
                )
            })
            .collect::<Vec<_>>();
        let bytes = write_sprite_bytes(&frames).unwrap();
        let sprite = parse_sprite(&bytes).unwrap();
        for format in [SpriteImageFormat::Png, SpriteImageFormat::Bmp] {
            let out = root.join(format.extension());
            unpack_sprite_file(&sprite, &out, format).unwrap();
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(out.join(SPRITE_MANIFEST_FILE)).unwrap()).unwrap();
            assert_eq!(manifest["version"], 2);
            for (index, frame) in frames.iter().enumerate() {
                let document = &manifest["frames"][index];
                assert_eq!(document["width"], frame.width());
                assert_eq!(document["height"], frame.height());
                if index < 3 {
                    assert!(document["image"].is_null());
                    assert!(document.get("encoding").is_none());
                } else {
                    assert_eq!(document["encoding"], format.extension());
                    let image = image::open(out.join(document["image"].as_str().unwrap()))
                        .unwrap()
                        .to_rgba8();
                    assert_eq!(image, frame.image);
                }
            }
            assert_eq!(fs::read_dir(&out).unwrap().count(), 3);
            let rebuilt = root.join(format!("{}.bin", format.extension()));
            assert_eq!(pack_sprite_dir(&out, &rebuilt).unwrap(), frames.len());
            assert_eq!(fs::read(rebuilt).unwrap(), bytes);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bmp_manifest_round_trips_all_alpha_values_and_uses_edited_image_dimensions() {
        let root = temp_dir("sprite-bmp-edit");
        let out = root.join("frames");
        let image = RgbaImage::from_fn(2, 8, |x, y| Rgba([17, 34, 51, (y * 2 + x) as u8 * 17]));
        let bytes = write_sprite_bytes(&[SpriteFrame::new(-7, 12, image)]).unwrap();
        unpack_sprite_file(&parse_sprite(&bytes).unwrap(), &out, SpriteImageFormat::Bmp).unwrap();
        let rebuilt = root.join("rebuilt.bin");
        pack_sprite_dir(&out, &rebuilt).unwrap();
        assert_eq!(fs::read(&rebuilt).unwrap(), bytes);
        let mut writer = fs::File::create(out.join("frame-000.bmp")).unwrap();
        write_bmp(&mut writer, 3, 1, [Rgba([17, 34, 51, 136]); 3].into_iter()).unwrap();
        pack_sprite_dir(&out, &rebuilt).unwrap();
        let edited = decode_sprite(&fs::read(rebuilt).unwrap()).unwrap();
        assert_eq!(edited.frames[0].frame.image.dimensions(), (3, 1));
        assert_eq!(edited.frames[0].frame.source_x, -7);
        assert_eq!(edited.frames[0].frame.source_y, 12);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zero_frame_sentinel_round_trips_through_both_image_formats() {
        let root = temp_dir("sprite-zero-frames");
        for format in [SpriteImageFormat::Png, SpriteImageFormat::Bmp] {
            let out = root.join(format.extension());
            assert_eq!(
                unpack_sprite_file(&parse_sprite(&[0]).unwrap(), &out, format).unwrap(),
                0
            );
            let rebuilt = root.join("rebuilt.bin");
            assert_eq!(pack_sprite_dir(&out, &rebuilt).unwrap(), 0);
            assert_eq!(fs::read(rebuilt).unwrap(), [0]);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packing_derives_dimensions_from_edited_png() {
        let root = temp_dir("sprite-resize");
        let output = root.join("rebuilt.bin");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(SPRITE_MANIFEST_FILE),
            serde_json::to_vec(&SpriteDocument {
                format: SPRITE_DOCUMENT_FORMAT.to_owned(),
                version: SPRITE_DOCUMENT_VERSION,
                pixel_format: SPRITE_PIXEL_FORMAT.to_owned(),
                frames: vec![SpriteFrameDocument {
                    image: Some("frame-000.png".to_owned()),
                    encoding: None,
                    width: None,
                    height: None,
                    source_x: 1,
                    source_y: 2,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        RgbaImage::new(3, 2)
            .save_with_format(root.join("frame-000.png"), ImageFormat::Png)
            .unwrap();

        pack_sprite_dir(&root, &output).unwrap();
        let sprite = decode_sprite(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(
            (sprite.frames[0].width(), sprite.frames[0].height()),
            (3, 2)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn png_only_extraction_writes_one_image_without_manifest() {
        let root = temp_dir("sprite-png-only");
        let output = root.join("nested").join("sprite.png");
        let bytes = write_sprite_bytes(&[SpriteFrame::new(5, 7, RgbaImage::new(3, 2))]).unwrap();
        let sprite = parse_sprite(&bytes).unwrap();

        unpack_sprite_png(&sprite, &output).unwrap();
        assert_eq!(
            image::open(&output).unwrap().to_rgba8().dimensions(),
            (3, 2)
        );
        assert!(!output.parent().unwrap().join(SPRITE_MANIFEST_FILE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn png_only_extraction_rejects_non_single_frame_payloads_and_non_png_outputs() {
        let empty = parse_sprite(&[0]).unwrap();
        assert!(unpack_sprite_png(&empty, Path::new("sprite.png")).is_err());

        let bytes = test_sprite_bytes();
        let multiple = parse_sprite(&bytes).unwrap();
        assert!(unpack_sprite_png(&multiple, Path::new("sprite.png")).is_err());

        let bytes = write_sprite_bytes(&[SpriteFrame::new(0, 0, RgbaImage::new(1, 1))]).unwrap();
        let single = parse_sprite(&bytes).unwrap();
        assert!(unpack_sprite_png(&single, Path::new("sprite.bin")).is_err());

        let bytes = write_sprite_bytes(&[SpriteFrame::new(0, 0, RgbaImage::new(0, 1))]).unwrap();
        let empty_frame = parse_sprite(&bytes).unwrap();
        let root = temp_dir("sprite-empty-png-only");
        let error = unpack_sprite_png(&empty_frame, &root.join("sprite.png")).unwrap_err();
        assert!(error.to_string().contains("use sprite directory output"));
        assert!(!root.exists());
    }

    #[test]
    fn free_size_bmp_round_trips_alpha_and_partial_blocks() {
        let root = temp_dir("free-size-bmp");
        let out = root.join("background.bmp");
        let payload = root.join("rebuilt.bin");
        let image = RgbaImage::from_fn(257, 257, |x, y| {
            Rgba([x as u8, y as u8, 37, x.wrapping_add(y) as u8])
        });
        let bytes = write_free_size_sprite_bytes(&image).unwrap();
        let sprite = decode_free_size_sprite(&bytes).unwrap();
        unpack_free_size_sprite_file(&sprite, &out, SpriteImageFormat::Bmp).unwrap();
        assert_eq!(image::open(&out).unwrap().to_rgba8(), image);
        assert_eq!(
            pack_free_size_sprite_image(&out, &payload).unwrap(),
            (257, 257)
        );
        assert_eq!(fs::read(payload).unwrap(), bytes);
        assert!(
            unpack_free_size_sprite_file(&sprite, &root.join("wrong.png"), SpriteImageFormat::Bmp)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_inconsistent_v2_frames_and_unsafe_bmp_references() {
        let base = temp_dir("sprite-invalid-v2");
        let root = base.join("frames");
        fs::create_dir_all(&root).unwrap();
        let mut bmp = Vec::new();
        write_bmp(&mut bmp, 1, 1, std::iter::once(Rgba([17, 34, 51, 136]))).unwrap();
        fs::write(base.join("frame.bmp"), &bmp).unwrap();
        fs::write(root.join("frame.bmp"), &bmp).unwrap();
        fs::write(root.join("frame.png"), &bmp).unwrap();
        let absolute_image = base.join("frame.bmp");
        for frame in [
            json!({"image": null, "width": 1, "height": 1}),
            json!({"image": null, "width": 0}),
            json!({"image": null, "width": 65536, "height": 0}),
            json!({"image": "frame.bmp", "encoding": "bmp", "width": 0, "height": 1}),
            json!({"image": "frame.bmp", "width": 1, "height": 1}),
            json!({"image": "../frame.bmp", "encoding": "bmp", "width": 1, "height": 1}),
            json!({"image": absolute_image, "encoding": "bmp", "width": 1, "height": 1}),
            json!({"image": "frame.png", "encoding": "bmp", "width": 1, "height": 1}),
            json!({"image": "frame.bmp", "encoding": "unknown", "width": 1, "height": 1}),
        ] {
            let mut frame = frame;
            frame["source_x"] = json!(0);
            frame["source_y"] = json!(0);
            fs::write(
                root.join(SPRITE_MANIFEST_FILE),
                json!({
                    "format": "sprite", "version": 2, "pixel_format": "a4r4g4b4", "frames": [frame]
                })
                .to_string(),
            )
            .unwrap();
            assert!(pack_sprite_dir(&root, &root.join("rebuilt.bin")).is_err());
        }
        assert!(!root.join("rebuilt.bin").exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn free_size_sprite_png_round_trips_without_manifest() {
        let root = temp_dir("free-size-sprite-round-trip");
        let input = root.join("nested").join("background.png");
        let payload = root.join("background.bin");
        let output = root.join("rebuilt.png");
        fs::create_dir_all(input.parent().unwrap()).unwrap();

        let mut image = RgbaImage::new(257, 2);
        image.put_pixel(0, 0, Rgba([10, 20, 30, 40]));
        image.put_pixel(256, 1, Rgba([50, 60, 70, 80]));
        image.save_with_format(&input, ImageFormat::Png).unwrap();

        assert_eq!(
            pack_free_size_sprite_image(&input, &payload).unwrap(),
            (257, 2)
        );
        let sprite = decode_free_size_sprite(&fs::read(&payload).unwrap()).unwrap();
        assert_eq!(sprite.image, image);
        unpack_free_size_sprite_file(&sprite, &output, SpriteImageFormat::Png).unwrap();
        assert_eq!(image::open(&output).unwrap().to_rgba8(), image);
        assert!(!root.join(SPRITE_MANIFEST_FILE).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn free_size_sprite_png_helpers_require_real_png_files() {
        let root = temp_dir("free-size-sprite-invalid-png");
        fs::create_dir_all(&root).unwrap();
        let payload = root.join("output.bin");

        fs::write(root.join("wrong.bin"), b"not a PNG").unwrap();
        assert!(pack_free_size_sprite_image(&root.join("wrong.bin"), &payload).is_err());

        fs::write(root.join("fake.png"), b"not a PNG").unwrap();
        assert!(pack_free_size_sprite_image(&root.join("fake.png"), &payload).is_err());

        let sprite =
            decode_free_size_sprite(&write_free_size_sprite_bytes(&RgbaImage::new(1, 1)).unwrap())
                .unwrap();
        assert!(
            unpack_free_size_sprite_file(&sprite, &root.join("wrong.bin"), SpriteImageFormat::Png)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_manifests_and_image_paths() {
        let root = temp_dir("sprite-invalid-manifest");
        fs::create_dir_all(&root).unwrap();
        let output = root.join("output.bin");

        fs::write(
            root.join(SPRITE_MANIFEST_FILE),
            json!({
                "format": SPRITE_DOCUMENT_FORMAT,
                "version": 999,
                "pixel_format": SPRITE_PIXEL_FORMAT,
                "frames": []
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_dir(&root, &output).is_err());

        fs::write(
            root.join(SPRITE_MANIFEST_FILE),
            json!({
                "format": SPRITE_DOCUMENT_FORMAT,
                "version": SPRITE_DOCUMENT_VERSION,
                "pixel_format": "a8r8g8b8",
                "frames": []
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_dir(&root, &output).is_err());

        fs::write(
            root.join(SPRITE_MANIFEST_FILE),
            json!({
                "format": SPRITE_DOCUMENT_FORMAT,
                "version": SPRITE_DOCUMENT_VERSION,
                "pixel_format": SPRITE_PIXEL_FORMAT,
                "frames": [],
                "unknown": true
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_dir(&root, &output).is_err());

        fs::write(
            root.join(SPRITE_MANIFEST_FILE),
            json!({
                "format": SPRITE_DOCUMENT_FORMAT,
                "version": SPRITE_DOCUMENT_VERSION,
                "pixel_format": SPRITE_PIXEL_FORMAT,
                "frames": [{"image": "../frame.png", "source_x": 0, "source_y": 0}]
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_dir(&root, &output).is_err());

        fs::write(
            root.join(SPRITE_MANIFEST_FILE),
            json!({
                "format": SPRITE_DOCUMENT_FORMAT,
                "version": SPRITE_DOCUMENT_VERSION,
                "pixel_format": SPRITE_PIXEL_FORMAT,
                "frames": [{"image": "frame.bmp", "source_x": 0, "source_y": 0}]
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_dir(&root, &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_duplicate_missing_and_malformed_images() {
        let root = temp_dir("sprite-invalid-images");
        fs::create_dir_all(&root).unwrap();
        let output = root.join("output.bin");

        let write_document = |frames: serde_json::Value| {
            fs::write(
                root.join(SPRITE_MANIFEST_FILE),
                json!({
                    "format": SPRITE_DOCUMENT_FORMAT,
                    "version": SPRITE_DOCUMENT_VERSION,
                    "pixel_format": SPRITE_PIXEL_FORMAT,
                    "frames": frames
                })
                .to_string(),
            )
            .unwrap();
        };

        write_document(json!([
            {"image": "same.png", "source_x": 0, "source_y": 0},
            {"image": "SAME.PNG", "source_x": 0, "source_y": 0}
        ]));
        assert!(pack_sprite_dir(&root, &output).is_err());

        write_document(json!([
            {"image": "missing.png", "source_x": 0, "source_y": 0}
        ]));
        assert!(pack_sprite_dir(&root, &output).is_err());

        fs::write(root.join("bad.png"), b"not a PNG").unwrap();
        write_document(json!([
            {"image": "bad.png", "source_x": 0, "source_y": 0}
        ]));
        assert!(pack_sprite_dir(&root, &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
