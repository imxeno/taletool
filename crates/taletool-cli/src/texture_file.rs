//! Strict manifest and PNG helpers for texture payloads.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use taletool_texture::{DecodedTexture, TextureFormat, TextureHeader, write_texture_bytes};

pub(crate) const TEXTURE_MANIFEST_FILE: &str = "texture.json";
const TEXTURE_DOCUMENT_FORMAT: &str = "texture";
const TEXTURE_DOCUMENT_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextureDocument {
    format: String,
    version: u32,
    pixel_format: TextureFormat,
    filter_flag: u8,
    unknown_06: u8,
    mip_level_count: u8,
    mip_levels: Vec<TextureMipLevelDocument>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum TextureMipLevelDocument {
    Image {
        image: String,
        width: u32,
        height: u32,
    },
    LegacyImage(String),
}

impl TextureMipLevelDocument {
    fn image(&self) -> &str {
        match self {
            Self::Image { image, .. } | Self::LegacyImage(image) => image,
        }
    }
}

/// Write a decoded texture as ordered PNG levels plus `texture.json`.
pub(crate) fn unpack_texture_file(texture: &DecodedTexture, out: &Path) -> anyhow::Result<usize> {
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let mut mip_levels = Vec::with_capacity(texture.mip_levels.len());
    for (level, image) in texture.mip_levels.iter().enumerate() {
        let name = format!("mip-{level:03}.png");
        let path = out.join(&name);
        image
            .save_with_format(&path, ImageFormat::Png)
            .with_context(|| format!("writing {}", path.display()))?;
        mip_levels.push(TextureMipLevelDocument::Image {
            image: name,
            width: image.width(),
            height: image.height(),
        });
    }

    let document = TextureDocument {
        format: TEXTURE_DOCUMENT_FORMAT.to_owned(),
        version: TEXTURE_DOCUMENT_VERSION,
        pixel_format: texture.header.format,
        filter_flag: texture.header.filter_flag,
        unknown_06: texture.header.unknown_06,
        mip_level_count: texture.header.mip_level_count,
        mip_levels,
    };
    let manifest_path = out.join(TEXTURE_MANIFEST_FILE);
    fs::write(&manifest_path, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", manifest_path.display()))?;
    Ok(document.mip_levels.len())
}

/// Build and write a texture payload from a texture manifest directory.
pub(crate) fn pack_texture_dir(dir: &Path, out: &Path) -> anyhow::Result<(TextureHeader, usize)> {
    if !dir.is_dir() {
        bail!(
            "texture input must be a directory containing texture.json: {}",
            dir.display()
        );
    }
    let manifest_path = dir.join(TEXTURE_MANIFEST_FILE);
    let manifest_bytes =
        fs::read(&manifest_path).with_context(|| format!("reading {}", manifest_path.display()))?;
    let document: TextureDocument = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    validate_document_header(&document)?;

    let effective_count = usize::from(document.mip_level_count.max(1));
    if document.mip_levels.len() != effective_count {
        bail!(
            "texture document declares {} effective mip levels but lists {} images",
            effective_count,
            document.mip_levels.len()
        );
    }

    let mut seen_images = HashSet::new();
    let mut mip_levels = Vec::with_capacity(document.mip_levels.len());
    for (level, mip) in document.mip_levels.iter().enumerate() {
        let image = mip.image();
        let normalized = image.to_lowercase();
        if !seen_images.insert(normalized) {
            bail!("texture mip level {level} repeats image path {image:?}");
        }
        let image_path = safe_mip_image_path(dir, level, image)?;
        let png =
            fs::read(&image_path).with_context(|| format!("reading {}", image_path.display()))?;
        let decoded = image::load_from_memory_with_format(&png, ImageFormat::Png)
            .with_context(|| format!("decoding {} as PNG", image_path.display()))?
            .to_rgba8();
        mip_levels.push(decoded);
    }

    let base = mip_levels
        .first()
        .expect("an effective texture mip chain always contains a base image");
    let width = u16::try_from(base.width()).map_err(|_| {
        anyhow::anyhow!(
            "texture width {} exceeds the u16 format limit",
            base.width()
        )
    })?;
    let height = u16::try_from(base.height()).map_err(|_| {
        anyhow::anyhow!(
            "texture height {} exceeds the u16 format limit",
            base.height()
        )
    })?;
    let header = TextureHeader {
        width,
        height,
        format: document.pixel_format,
        filter_flag: document.filter_flag,
        unknown_06: document.unknown_06,
        mip_level_count: document.mip_level_count,
    };
    let bytes = write_texture_bytes(&header, &mip_levels)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok((header, mip_levels.len()))
}

fn validate_document_header(document: &TextureDocument) -> anyhow::Result<()> {
    if document.format != TEXTURE_DOCUMENT_FORMAT {
        bail!(
            "texture document has unsupported format {:?}; expected {:?}",
            document.format,
            TEXTURE_DOCUMENT_FORMAT
        );
    }
    if !matches!(document.version, 1 | TEXTURE_DOCUMENT_VERSION) {
        bail!(
            "texture document has unsupported version {}; expected 1 or {}",
            document.version,
            TEXTURE_DOCUMENT_VERSION
        );
    }
    for (level, mip) in document.mip_levels.iter().enumerate() {
        match (document.version, mip) {
            (1, TextureMipLevelDocument::LegacyImage(_))
            | (TEXTURE_DOCUMENT_VERSION, TextureMipLevelDocument::Image { .. }) => {}
            _ => bail!(
                "texture document version {} has an invalid mip level {level}; expected {}",
                document.version,
                if document.version == 1 {
                    "an image filename"
                } else {
                    "an object with image, width, and height"
                }
            ),
        }
    }
    Ok(())
}

fn safe_mip_image_path(root: &Path, level: usize, image: &str) -> anyhow::Result<PathBuf> {
    if image.eq_ignore_ascii_case(TEXTURE_MANIFEST_FILE) {
        bail!("texture mip level {level} cannot reference its manifest file");
    }
    let path = Path::new(image);
    if path.is_absolute() {
        bail!("texture mip level {level} image path must be relative: {image}");
    }
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => bail!("texture mip level {level} image path must be a filename: {image}"),
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("png"))
    {
        bail!("texture mip level {level} image must be a PNG file: {image}");
    }
    Ok(root.join(path))
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{Rgba, RgbaImage};
    use serde_json::json;
    use taletool_texture::{DecodedTexture, TextureFormat, TextureHeader, decode_texture};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn sample_texture() -> DecodedTexture {
        DecodedTexture {
            header: TextureHeader {
                width: 2,
                height: 2,
                format: TextureFormat::L8,
                filter_flag: 1,
                unknown_06: 9,
                mip_level_count: 2,
            },
            mip_levels: vec![
                RgbaImage::from_fn(2, 2, |x, y| {
                    let value = (x + y * 2) as u8 * 17;
                    Rgba([value, value, value, 255])
                }),
                RgbaImage::from_pixel(1, 1, Rgba([85, 85, 85, 255])),
            ],
        }
    }

    #[test]
    fn png_manifest_round_trips_byte_for_byte() {
        let root = temp_dir("texture-round-trip");
        let unpacked = root.join("unpacked");
        let output = root.join("rebuilt.bin");
        let texture = sample_texture();
        let source = write_texture_bytes(&texture.header, &texture.mip_levels).unwrap();

        assert_eq!(unpack_texture_file(&texture, &unpacked).unwrap(), 2);
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(unpacked.join(TEXTURE_MANIFEST_FILE)).unwrap())
                .unwrap();
        assert_eq!(document["format"], TEXTURE_DOCUMENT_FORMAT);
        assert_eq!(document["version"], TEXTURE_DOCUMENT_VERSION);
        assert_eq!(document["pixel_format"], "l8");
        assert_eq!(document["filter_flag"], 1);
        assert_eq!(document["unknown_06"], 9);
        assert_eq!(
            document["mip_levels"],
            json!([
                { "image": "mip-000.png", "width": 2, "height": 2 },
                { "image": "mip-001.png", "width": 1, "height": 1 }
            ])
        );

        let (header, count) = pack_texture_dir(&unpacked, &output).unwrap();
        assert_eq!(header, texture.header);
        assert_eq!(count, 2);
        assert_eq!(fs::read(&output).unwrap(), source);
        assert_eq!(decode_texture(&source).unwrap(), texture);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn packs_legacy_filename_only_manifests() {
        let root = temp_dir("texture-legacy-manifest");
        let unpacked = root.join("unpacked");
        let output = root.join("rebuilt.bin");
        let texture = sample_texture();
        let source = write_texture_bytes(&texture.header, &texture.mip_levels).unwrap();
        unpack_texture_file(&texture, &unpacked).unwrap();

        let manifest_path = unpacked.join(TEXTURE_MANIFEST_FILE);
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        document["version"] = json!(1);
        document["mip_levels"] = json!(["mip-000.png", "mip-001.png"]);
        fs::write(&manifest_path, serde_json::to_vec(&document).unwrap()).unwrap();

        let (header, count) = pack_texture_dir(&unpacked, &output).unwrap();
        assert_eq!(header, texture.header);
        assert_eq!(count, 2);
        assert_eq!(fs::read(output).unwrap(), source);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn png_dimensions_determine_rebuilt_dimensions() {
        let root = temp_dir("texture-resized-images");
        let unpacked = root.join("unpacked");
        let output = root.join("rebuilt.bin");
        let mut texture = sample_texture();
        unpack_texture_file(&texture, &unpacked).unwrap();

        texture.header.width = 4;
        texture.header.height = 4;
        texture.mip_levels = vec![RgbaImage::new(4, 4), RgbaImage::new(2, 2)];
        for (level, image) in texture.mip_levels.iter().enumerate() {
            image
                .save_with_format(
                    unpacked.join(format!("mip-{level:03}.png")),
                    ImageFormat::Png,
                )
                .unwrap();
        }

        let (header, count) = pack_texture_dir(&unpacked, &output).unwrap();
        assert_eq!(header, texture.header);
        assert_eq!(count, 2);
        assert_eq!(
            fs::read(output).unwrap(),
            write_texture_bytes(&texture.header, &texture.mip_levels).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserves_zero_as_the_stored_single_level_count() {
        let root = temp_dir("texture-zero-count");
        let unpacked = root.join("unpacked");
        let output = root.join("rebuilt.bin");
        let texture = DecodedTexture {
            header: TextureHeader {
                width: 1,
                height: 1,
                format: TextureFormat::A8,
                filter_flag: 0,
                unknown_06: 4,
                mip_level_count: 0,
            },
            mip_levels: vec![RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 123]))],
        };
        let source = write_texture_bytes(&texture.header, &texture.mip_levels).unwrap();

        unpack_texture_file(&texture, &unpacked).unwrap();
        pack_texture_dir(&unpacked, &output).unwrap();
        assert_eq!(fs::read(output).unwrap(), source);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_documents_and_mip_paths() {
        let root = temp_dir("texture-invalid-document");
        fs::create_dir_all(&root).unwrap();
        let output = root.join("output.bin");
        let write_document = |value: serde_json::Value| {
            fs::write(root.join(TEXTURE_MANIFEST_FILE), value.to_string()).unwrap();
        };
        let valid_header = || {
            json!({
                "format": TEXTURE_DOCUMENT_FORMAT,
                "version": TEXTURE_DOCUMENT_VERSION,
                "pixel_format": "l8",
                "filter_flag": 0,
                "unknown_06": 0,
                "mip_level_count": 1,
                "mip_levels": [{ "image": "mip.png", "width": 1, "height": 1 }]
            })
        };

        let mut value = valid_header();
        value["version"] = json!(999);
        write_document(value);
        assert!(pack_texture_dir(&root, &output).is_err());

        let mut value = valid_header();
        value["extra"] = json!(true);
        write_document(value);
        assert!(pack_texture_dir(&root, &output).is_err());

        let mut value = valid_header();
        value["mip_levels"][0]["image"] = json!("../mip.png");
        write_document(value);
        assert!(
            pack_texture_dir(&root, &output)
                .unwrap_err()
                .to_string()
                .contains("image path must be a filename")
        );

        let mut value = valid_header();
        value["mip_level_count"] = json!(2);
        write_document(value);
        assert!(pack_texture_dir(&root, &output).is_err());

        for mip in [
            json!("mip.png"),
            json!({ "image": "mip.png", "width": 1 }),
            json!({ "image": "mip.png", "height": 1 }),
            json!({ "image": "mip.png", "width": 1, "height": 1, "extra": true }),
        ] {
            let mut value = valid_header();
            value["mip_levels"] = json!([mip]);
            write_document(value);
            assert!(
                pack_texture_dir(&root, &output)
                    .unwrap_err()
                    .chain()
                    .any(|error| error.to_string().contains("mip level")
                        || error.to_string().contains("untagged enum"))
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_duplicate_missing_malformed_and_stale_mips() {
        let root = temp_dir("texture-invalid-images");
        fs::create_dir_all(&root).unwrap();
        let output = root.join("output.bin");
        let write_document = |mip_levels: serde_json::Value, count: u8| {
            fs::write(
                root.join(TEXTURE_MANIFEST_FILE),
                json!({
                    "format": TEXTURE_DOCUMENT_FORMAT,
                    "version": TEXTURE_DOCUMENT_VERSION,
                    "pixel_format": "l8",
                    "filter_flag": 0,
                    "unknown_06": 0,
                    "mip_level_count": count,
                    "mip_levels": mip_levels
                })
                .to_string(),
            )
            .unwrap();
        };

        RgbaImage::new(2, 2)
            .save_with_format(root.join("same.png"), ImageFormat::Png)
            .unwrap();
        write_document(
            json!([
                { "image": "same.png", "width": 2, "height": 2 },
                { "image": "SAME.PNG", "width": 1, "height": 1 }
            ]),
            2,
        );
        assert!(
            pack_texture_dir(&root, &output)
                .unwrap_err()
                .to_string()
                .contains("repeats image path")
        );

        write_document(
            json!([{ "image": "missing.png", "width": 1, "height": 1 }]),
            1,
        );
        assert!(pack_texture_dir(&root, &output).is_err());

        fs::write(root.join("bad.png"), b"not a PNG").unwrap();
        write_document(json!([{ "image": "bad.png", "width": 1, "height": 1 }]), 1);
        assert!(pack_texture_dir(&root, &output).is_err());

        RgbaImage::new(2, 2)
            .save_with_format(root.join("base.png"), ImageFormat::Png)
            .unwrap();
        RgbaImage::new(2, 2)
            .save_with_format(root.join("mip.png"), ImageFormat::Png)
            .unwrap();
        write_document(
            json!([
                { "image": "base.png", "width": 2, "height": 2 },
                { "image": "mip.png", "width": 2, "height": 2 }
            ]),
            2,
        );
        assert!(
            pack_texture_dir(&root, &output)
                .unwrap_err()
                .to_string()
                .contains("mip level 1 dimensions must be 1x1, got 2x2")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
