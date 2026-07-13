//! JSON helpers for sprite-animation payloads.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_animation::sprite::{SpriteAnimation, write_sprite_animation_bytes};

const SPRITE_ANIMATION_DOCUMENT_FORMAT: &str = "sprite-animation";
const SPRITE_ANIMATION_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpriteAnimationDocument {
    format: String,
    version: u32,
    animation: SpriteAnimation,
}

/// Write a sprite animation as a strict, versioned JSON document.
pub(crate) fn unpack_sprite_animation_file(
    animation: &SpriteAnimation,
    out: &Path,
) -> anyhow::Result<()> {
    let document = SpriteAnimationDocument {
        format: SPRITE_ANIMATION_DOCUMENT_FORMAT.to_owned(),
        version: SPRITE_ANIMATION_DOCUMENT_VERSION,
        animation: animation.clone(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write native sprite-animation bytes from a JSON document.
pub(crate) fn pack_sprite_animation_file(
    input: &Path,
    out: &Path,
) -> anyhow::Result<SpriteAnimation> {
    let document_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let document: SpriteAnimationDocument = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&document)?;

    let bytes = write_sprite_animation_bytes(&document.animation)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(document.animation)
}

fn validate_document_header(document: &SpriteAnimationDocument) -> anyhow::Result<()> {
    if document.format != SPRITE_ANIMATION_DOCUMENT_FORMAT {
        bail!(
            "sprite-animation document has unsupported format {:?}; expected {:?}",
            document.format,
            SPRITE_ANIMATION_DOCUMENT_FORMAT
        );
    }
    if document.version != SPRITE_ANIMATION_DOCUMENT_VERSION {
        bail!(
            "sprite-animation document has unsupported version {}; expected {}",
            document.version,
            SPRITE_ANIMATION_DOCUMENT_VERSION
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
    use taletool_animation::sprite::{
        SpriteAnimationFrame, decode_sprite_animation, write_sprite_animation_bytes,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn sample_animation() -> SpriteAnimation {
        SpriteAnimation {
            playback_flags: 0x85,
            frames: vec![
                SpriteAnimationFrame {
                    sprite_frame_index: 2,
                    event_timing_flag: 0,
                },
                SpriteAnimationFrame {
                    sprite_frame_index: 5,
                    event_timing_flag: 3,
                },
            ],
        }
    }

    #[test]
    fn strict_json_round_trips_native_bytes() {
        let root = temp_dir("sprite-animation-json-round-trip");
        let json_path = root.join("animation.json");
        let payload_path = root.join("animation.bin");
        let expected = sample_animation();
        let expected_bytes = write_sprite_animation_bytes(&expected).unwrap();

        unpack_sprite_animation_file(&expected, &json_path).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(document["format"], SPRITE_ANIMATION_DOCUMENT_FORMAT);
        assert_eq!(document["version"], SPRITE_ANIMATION_DOCUMENT_VERSION);
        assert_eq!(document["animation"]["playback_flags"], 0x85);
        assert_eq!(document["animation"]["frames"][1]["event_timing_flag"], 3);
        assert!(
            document["animation"]["frames"][1]
                .get("event_marker")
                .is_none()
        );

        let actual = pack_sprite_animation_file(&json_path, &payload_path).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(fs::read(&payload_path).unwrap(), expected_bytes);
        assert_eq!(
            decode_sprite_animation(&fs::read(&payload_path).unwrap()).unwrap(),
            expected
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_versions_unknown_fields_and_oversized_frame_lists() {
        let root = temp_dir("sprite-animation-invalid-json");
        let json_path = root.join("animation.json");
        fs::create_dir_all(&root).unwrap();

        fs::write(
            &json_path,
            json!({
                "format": SPRITE_ANIMATION_DOCUMENT_FORMAT,
                "version": 999,
                "animation": sample_animation(),
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_animation_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": SPRITE_ANIMATION_DOCUMENT_FORMAT,
                "version": SPRITE_ANIMATION_DOCUMENT_VERSION,
                "animation": sample_animation(),
                "unexpected": true,
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_animation_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": SPRITE_ANIMATION_DOCUMENT_FORMAT,
                "version": SPRITE_ANIMATION_DOCUMENT_VERSION,
                "animation": {
                    "playback_flags": 0,
                    "frames": [{"sprite_frame_index": 0, "event_marker": 1}],
                },
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_animation_file(&json_path, &root.join("bad.bin")).is_err());

        let frames = vec![json!({"sprite_frame_index": 0, "event_timing_flag": 0}); 256];
        fs::write(
            &json_path,
            json!({
                "format": SPRITE_ANIMATION_DOCUMENT_FORMAT,
                "version": SPRITE_ANIMATION_DOCUMENT_VERSION,
                "animation": {"playback_flags": 0, "frames": frames},
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_sprite_animation_file(&json_path, &root.join("bad.bin")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
