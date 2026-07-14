//! JSON helpers for sprite-resource remap payloads.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_animation::sprite_remap::{SpriteResourceRemap, write_sprite_resource_remap_bytes};

const SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT: &str = "sprite-resource-remap";
const SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteResourceRemapDocument {
    format: String,
    version: u32,
    remap: SpriteResourceRemap,
}

/// Write a sprite-resource remap as a JSON document.
pub(crate) fn unpack_sprite_resource_remap_file(
    remap: &SpriteResourceRemap,
    out: &Path,
) -> anyhow::Result<()> {
    let document = SpriteResourceRemapDocument {
        format: SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT.to_owned(),
        version: SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION,
        remap: remap.clone(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write native sprite-resource remap bytes from a JSON document.
pub(crate) fn pack_sprite_resource_remap_file(
    input: &Path,
    out: &Path,
) -> anyhow::Result<SpriteResourceRemap> {
    let document_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let document: SpriteResourceRemapDocument = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&document)?;

    let bytes = write_sprite_resource_remap_bytes(&document.remap)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(document.remap)
}

fn validate_document_header(document: &SpriteResourceRemapDocument) -> anyhow::Result<()> {
    if document.format != SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT {
        bail!(
            "sprite-resource remap document has unsupported format {:?}; expected {:?}",
            document.format,
            SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT
        );
    }
    if document.version != SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION {
        bail!(
            "sprite-resource remap document has unsupported version {}; expected {}",
            document.version,
            SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION
        );
    }
    Ok(())
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
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use taletool_animation::sprite_remap::{
        MAX_SPRITE_REMAP_FRAMES, SpriteFrameResourceRemap, decode_sprite_resource_remap,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn sample_remap() -> SpriteResourceRemap {
        SpriteResourceRemap {
            frames: vec![
                SpriteFrameResourceRemap {
                    resource_indices: [0, 1, 2, 3, 4, 5, 6, 7],
                },
                SpriteFrameResourceRemap {
                    resource_indices: [1, 6, 0, 4, 2, 3, 8, u8::MAX],
                },
            ],
        }
    }

    #[test]
    fn json_round_trips_native_bytes() {
        let root = temp_dir("sprite-remap-json-round-trip");
        let json_path = root.join("remap.json");
        let payload_path = root.join("remap.bin");
        let expected = sample_remap();
        let expected_bytes = write_sprite_resource_remap_bytes(&expected).unwrap();

        unpack_sprite_resource_remap_file(&expected, &json_path).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(document["format"], SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT);
        assert_eq!(document["version"], SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION);
        assert_eq!(
            document["remap"]["frames"][1]["resource_indices"],
            json!([1, 6, 0, 4, 2, 3, 8, 255])
        );

        let actual = pack_sprite_resource_remap_file(&json_path, &payload_path).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(fs::read(&payload_path).unwrap(), expected_bytes);
        assert_eq!(
            decode_sprite_resource_remap(&fs::read(&payload_path).unwrap()).unwrap(),
            expected
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_headers_unknown_fields_and_malformed_rows() {
        let root = temp_dir("sprite-remap-invalid-json");
        let json_path = root.join("remap.json");
        fs::create_dir_all(&root).unwrap();

        for document in [
            json!({
                "format": "another-format",
                "version": SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION,
                "remap": sample_remap(),
            }),
            json!({
                "format": SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT,
                "version": 999,
                "remap": sample_remap(),
            }),
            json!({
                "format": SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT,
                "version": SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION,
                "remap": sample_remap(),
                "unexpected": true,
            }),
            json!({
                "format": SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT,
                "version": SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION,
                "remap": {
                    "frames": [{"resource_indices": [0, 1, 2, 3, 4, 5, 6]}],
                },
            }),
            json!({
                "format": SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT,
                "version": SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION,
                "remap": {
                    "frames": [{
                        "resource_indices": [0, 1, 2, 3, 4, 5, 6, 7],
                        "unexpected": true,
                    }],
                },
            }),
        ] {
            fs::write(&json_path, document.to_string()).unwrap();
            assert!(pack_sprite_resource_remap_file(&json_path, &root.join("bad.bin")).is_err());
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_oversized_documents() {
        let root = temp_dir("sprite-remap-oversized-json");
        let json_path = root.join("remap.json");
        fs::create_dir_all(&root).unwrap();
        let frames = vec![
            json!({"resource_indices": [0, 1, 2, 3, 4, 5, 6, 7]});
            MAX_SPRITE_REMAP_FRAMES + 1
        ];
        fs::write(
            &json_path,
            json!({
                "format": SPRITE_RESOURCE_REMAP_DOCUMENT_FORMAT,
                "version": SPRITE_RESOURCE_REMAP_DOCUMENT_VERSION,
                "remap": {"frames": frames},
            })
            .to_string(),
        )
        .unwrap();

        assert!(pack_sprite_resource_remap_file(&json_path, &root.join("bad.bin")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
