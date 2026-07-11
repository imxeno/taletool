//! Strict manifest and PNG helpers for extracted texture payloads.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use taletool_texture::{DecodedTexture, TextureFormat, TextureHeader, write_texture_bytes};

pub(crate) const TEXTURE_MANIFEST_FILE: &str = "texture.json";
const TEXTURE_DOCUMENT_FORMAT: &str = "texture";
const TEXTURE_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextureDocument {
    format: String,
    version: u32,
    pixel_format: TextureFormat,
    filter_flag: u8,
    unknown_06: u8,
    mip_level_count: u8,
    mip_levels: Vec<String>,
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
        mip_levels.push(name);
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

/// Build and write a texture payload from a manifest-backed directory.
pub(crate) fn pack_texture_dir(dir: &Path, out: &Path) -> anyhow::Result<(TextureHeader, usize)> {
    if !dir.is_dir() {
        bail!(
            "texture input must be a manifest-backed directory: {}",
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
    for (level, image) in document.mip_levels.iter().enumerate() {
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
    if document.version != TEXTURE_DOCUMENT_VERSION {
        bail!(
            "texture document has unsupported version {}; expected {}",
            document.version,
            TEXTURE_DOCUMENT_VERSION
        );
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

        let (header, count) = pack_texture_dir(&unpacked, &output).unwrap();
        assert_eq!(header, texture.header);
        assert_eq!(count, 2);
        assert_eq!(fs::read(&output).unwrap(), source);
        assert_eq!(decode_texture(&source).unwrap(), texture);
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
                "mip_levels": ["mip.png"]
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
        value["mip_levels"] = json!(["../mip.png"]);
        write_document(value);
        assert!(pack_texture_dir(&root, &output).is_err());

        let mut value = valid_header();
        value["mip_level_count"] = json!(2);
        write_document(value);
        assert!(pack_texture_dir(&root, &output).is_err());
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

        write_document(json!(["same.png", "SAME.PNG"]), 2);
        assert!(pack_texture_dir(&root, &output).is_err());

        write_document(json!(["missing.png"]), 1);
        assert!(pack_texture_dir(&root, &output).is_err());

        fs::write(root.join("bad.png"), b"not a PNG").unwrap();
        write_document(json!(["bad.png"]), 1);
        assert!(pack_texture_dir(&root, &output).is_err());

        RgbaImage::new(2, 2)
            .save_with_format(root.join("base.png"), ImageFormat::Png)
            .unwrap();
        RgbaImage::new(2, 2)
            .save_with_format(root.join("mip.png"), ImageFormat::Png)
            .unwrap();
        write_document(json!(["base.png", "mip.png"]), 2);
        assert!(pack_texture_dir(&root, &output).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
