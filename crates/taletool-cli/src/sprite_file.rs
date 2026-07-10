//! Manifest and PNG helpers for multi-frame sprite payloads.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use taletool_texture::sprite::free_size::{FreeSizeSprite, write_free_size_sprite_bytes};
use taletool_texture::sprite::{DecodedSprite, SpriteFrame, write_sprite_bytes};

pub(crate) const SPRITE_MANIFEST_FILE: &str = "sprite.json";
const SPRITE_DOCUMENT_FORMAT: &str = "sprite";
const SPRITE_DOCUMENT_VERSION: u32 = 1;
const SPRITE_PIXEL_FORMAT: &str = "a4r4g4b4";

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
    image: String,
    source_x: i16,
    source_y: i16,
}

/// Write a decoded sprite as ordered PNG frames plus `sprite.json`.
pub(crate) fn unpack_sprite_file(sprite: &DecodedSprite, out: &Path) -> anyhow::Result<usize> {
    fs::create_dir_all(out)?;
    let mut frames = Vec::with_capacity(sprite.frames.len());
    for (index, decoded) in sprite.frames.iter().enumerate() {
        let image = format!("frame-{index:03}.png");
        let image_path = out.join(&image);
        decoded
            .frame
            .image
            .save_with_format(&image_path, ImageFormat::Png)
            .with_context(|| format!("writing {}", image_path.display()))?;
        frames.push(SpriteFrameDocument {
            image,
            source_x: decoded.frame.source_x,
            source_y: decoded.frame.source_y,
        });
    }

    let document = SpriteDocument {
        format: SPRITE_DOCUMENT_FORMAT.to_owned(),
        version: SPRITE_DOCUMENT_VERSION,
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
pub(crate) fn unpack_sprite_png(sprite: &DecodedSprite, out: &Path) -> anyhow::Result<()> {
    if sprite.frames.len() != 1 {
        bail!(
            "PNG-only sprite extraction requires exactly one frame; payload has {} frames",
            sprite.frames.len()
        );
    }
    ensure_png_path(out, "PNG-only sprite output")?;

    create_parent_dir(out)?;
    sprite.frames[0]
        .frame
        .image
        .save_with_format(out, ImageFormat::Png)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Write a decoded free-size sprite directly to one PNG without a manifest.
pub(crate) fn unpack_free_size_sprite_png(
    sprite: &FreeSizeSprite,
    out: &Path,
) -> anyhow::Result<()> {
    ensure_png_path(out, "free-size sprite output")?;
    create_parent_dir(out)?;
    sprite
        .image
        .save_with_format(out, ImageFormat::Png)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write a canonical sprite payload from a manifest-backed directory.
pub(crate) fn pack_sprite_dir(dir: &Path, out: &Path) -> anyhow::Result<usize> {
    let manifest_path = dir.join(SPRITE_MANIFEST_FILE);
    let manifest_bytes =
        fs::read(&manifest_path).with_context(|| format!("reading {}", manifest_path.display()))?;
    let document: SpriteDocument = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    validate_document_header(&document)?;

    let mut seen_images = HashSet::new();
    let mut frames = Vec::with_capacity(document.frames.len());
    for (index, frame) in document.frames.iter().enumerate() {
        let normalized = frame.image.to_lowercase();
        if !seen_images.insert(normalized) {
            bail!("sprite frame {index} repeats image path {:?}", frame.image);
        }
        let image_path = safe_frame_image_path(dir, index, &frame.image)?;
        let image = image::open(&image_path)
            .with_context(|| format!("decoding {}", image_path.display()))?
            .to_rgba8();
        frames.push(SpriteFrame::new(frame.source_x, frame.source_y, image));
    }

    let bytes = write_sprite_bytes(&frames)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(frames.len())
}

/// Build and write a canonical free-size sprite payload from one PNG.
pub(crate) fn pack_free_size_sprite_png(input: &Path, out: &Path) -> anyhow::Result<(u32, u32)> {
    ensure_png_path(input, "free-size sprite input")?;
    let png = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let image = image::load_from_memory_with_format(&png, ImageFormat::Png)
        .with_context(|| format!("decoding {} as PNG", input.display()))?
        .to_rgba8();
    let dimensions = image.dimensions();
    let bytes = write_free_size_sprite_bytes(&image)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(dimensions)
}

fn validate_document_header(document: &SpriteDocument) -> anyhow::Result<()> {
    if document.format != SPRITE_DOCUMENT_FORMAT {
        bail!(
            "sprite document has unsupported format {:?}; expected {:?}",
            document.format,
            SPRITE_DOCUMENT_FORMAT
        );
    }
    if document.version != SPRITE_DOCUMENT_VERSION {
        bail!(
            "sprite document has unsupported version {}; expected {}",
            document.version,
            SPRITE_DOCUMENT_VERSION
        );
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

fn safe_frame_image_path(root: &Path, index: usize, image: &str) -> anyhow::Result<PathBuf> {
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
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("png"))
    {
        bail!("sprite frame {index} image must be a PNG file: {image}");
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
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("png"))
    {
        bail!("{label} must use a .png extension: {}", path.display());
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
    use taletool_texture::sprite::{SpriteFrame, decode_sprite, write_sprite_bytes};

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
        let sprite = decode_sprite(&bytes).unwrap();

        assert_eq!(unpack_sprite_file(&sprite, &unpacked).unwrap(), 2);
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
                    image: "frame-000.png".to_owned(),
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
        let sprite = decode_sprite(
            &write_sprite_bytes(&[SpriteFrame::new(5, 7, RgbaImage::new(3, 2))]).unwrap(),
        )
        .unwrap();

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
        let empty = DecodedSprite { frames: Vec::new() };
        assert!(unpack_sprite_png(&empty, Path::new("sprite.png")).is_err());

        let multiple = decode_sprite(&test_sprite_bytes()).unwrap();
        assert!(unpack_sprite_png(&multiple, Path::new("sprite.png")).is_err());

        let single = decode_sprite(
            &write_sprite_bytes(&[SpriteFrame::new(0, 0, RgbaImage::new(1, 1))]).unwrap(),
        )
        .unwrap();
        assert!(unpack_sprite_png(&single, Path::new("sprite.bin")).is_err());
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
            pack_free_size_sprite_png(&input, &payload).unwrap(),
            (257, 2)
        );
        let sprite = decode_free_size_sprite(&fs::read(&payload).unwrap()).unwrap();
        assert_eq!(sprite.image, image);
        unpack_free_size_sprite_png(&sprite, &output).unwrap();
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
        assert!(pack_free_size_sprite_png(&root.join("wrong.bin"), &payload).is_err());

        fs::write(root.join("fake.png"), b"not a PNG").unwrap();
        assert!(pack_free_size_sprite_png(&root.join("fake.png"), &payload).is_err());

        let sprite =
            decode_free_size_sprite(&write_free_size_sprite_bytes(&RgbaImage::new(1, 1)).unwrap())
                .unwrap();
        assert!(unpack_free_size_sprite_png(&sprite, &root.join("wrong.bin")).is_err());
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
