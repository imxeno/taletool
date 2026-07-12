//! Strict JSON and file helpers for effect payloads.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_effect::{EffectAsset, write_effect_asset_bytes};

const EFFECT_DOCUMENT_FORMAT: &str = "effect";
const EFFECT_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectDocument {
    format: String,
    version: u32,
    #[serde(flatten)]
    effect: EffectAsset,
}

/// Write one decoded effect payload as a JSON document.
pub(crate) fn unpack_effect_file(effect: &EffectAsset, out: &Path) -> anyhow::Result<()> {
    let document = EffectDocument {
        format: EFFECT_DOCUMENT_FORMAT.to_owned(),
        version: EFFECT_DOCUMENT_VERSION,
        effect: effect.clone(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write native effect bytes from a JSON document.
pub(crate) fn pack_effect_file(input: &Path, out: &Path) -> anyhow::Result<EffectAsset> {
    let bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let document: EffectDocument =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&document)?;
    let native = write_effect_asset_bytes(&document.effect)?;
    create_parent_dir(out)?;
    fs::write(out, native).with_context(|| format!("writing {}", out.display()))?;
    Ok(document.effect)
}

fn validate_document_header(document: &EffectDocument) -> anyhow::Result<()> {
    if document.format != EFFECT_DOCUMENT_FORMAT {
        bail!(
            "effect document has unsupported format {:?}; expected {:?}",
            document.format,
            EFFECT_DOCUMENT_FORMAT
        );
    }
    if document.version != EFFECT_DOCUMENT_VERSION {
        bail!(
            "effect document has unsupported version {}; expected {}",
            document.version,
            EFFECT_DOCUMENT_VERSION
        );
    }
    Ok(())
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use taletool_effect::{
        AnimationTiming, ColorTrack, EffectAsset, EffectComponentVariant, EffectDefinition,
        EffectDefinitionLoaderWorkspace, EffectTrackLoaderWorkspace, TextureAnimation,
        TextureKeyframe, decode_texture_animation,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn effect() -> EffectAsset {
        EffectAsset::TextureAnimation(TextureAnimation {
            timing: AnimationTiming {
                first_frame: 0,
                last_frame: 10,
                frame_rate: 30,
                keyframe_step: 160,
            },
            keyframes: vec![TextureKeyframe {
                time: 0,
                texture_resource_key: 42,
            }],
        })
    }

    #[test]
    fn strict_json_round_trips_native_bytes() {
        let root = temp_dir("effect-json-round-trip");
        let json_path = root.join("effect.json");
        let payload_path = root.join("effect.bin");
        fs::create_dir_all(&root).unwrap();
        let expected = effect();

        unpack_effect_file(&expected, &json_path).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(document["format"], EFFECT_DOCUMENT_FORMAT);
        assert_eq!(document["version"], EFFECT_DOCUMENT_VERSION);
        assert_eq!(document["kind"], "texture-animation");

        assert_eq!(
            pack_effect_file(&json_path, &payload_path).unwrap(),
            expected
        );
        assert!(decode_texture_animation(&fs::read(&payload_path).unwrap()).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_versions_and_unknown_fields() {
        let root = temp_dir("effect-invalid-json");
        let json_path = root.join("effect.json");
        fs::create_dir_all(&root).unwrap();

        fs::write(
            &json_path,
            serde_json::to_vec(&EffectDocument {
                format: EFFECT_DOCUMENT_FORMAT.to_owned(),
                version: 999,
                effect: effect(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(pack_effect_file(&json_path, &root.join("bad.bin")).is_err());

        let mut value = serde_json::to_value(EffectDocument {
            format: EFFECT_DOCUMENT_FORMAT.to_owned(),
            version: EFFECT_DOCUMENT_VERSION,
            effect: effect(),
        })
        .unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), json!(true));
        fs::write(&json_path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(pack_effect_file(&json_path, &root.join("bad.bin")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_removed_unknown_component_variant() {
        assert!(
            serde_json::from_value::<EffectComponentVariant>(json!({
                "kind": "unknown",
                "properties": {
                    "code": 3,
                    "data": []
                }
            }))
            .is_err()
        );
    }

    #[test]
    fn loader_workspace_defaults_to_zero_when_omitted_from_json() {
        let definition = serde_json::from_value::<EffectDefinition>(json!({
            "resource_key": 42,
            "components": []
        }))
        .unwrap();
        assert_eq!(
            definition.loader_workspace,
            EffectDefinitionLoaderWorkspace::default()
        );

        let definition = serde_json::from_value::<EffectDefinition>(json!({
            "resource_key": 42,
            "loader_workspace": {
                "loaded_tick_slot": 7
            },
            "components": []
        }))
        .unwrap();
        assert_eq!(definition.loader_workspace.loaded_tick_slot, 7);
        assert_eq!(definition.loader_workspace.source_record_slot, 0);
        assert_eq!(definition.loader_workspace.child_records_slot, 0);
        assert_eq!(definition.loader_workspace.reference_count_slot, 0);
        assert_eq!(definition.loader_workspace.flags_slot, 0);

        let track = serde_json::from_value::<ColorTrack>(json!({
            "keyframes": []
        }))
        .unwrap();
        assert_eq!(
            track.loader_workspace,
            EffectTrackLoaderWorkspace::default()
        );

        let track = serde_json::from_value::<ColorTrack>(json!({
            "loader_workspace": {
                "key_array_slot": 9
            },
            "keyframes": []
        }))
        .unwrap();
        assert_eq!(track.loader_workspace.reserved_01_03, [0; 3]);
        assert_eq!(track.loader_workspace.key_array_slot, 9);
        assert_eq!(track.loader_workspace.value_array_slot, 0);

        let serialized = serde_json::to_value(EffectDefinition {
            resource_key: 42,
            loader_workspace: EffectDefinitionLoaderWorkspace::default(),
            components: Vec::new(),
        })
        .unwrap();
        assert_eq!(serialized["loader_workspace"]["loaded_tick_slot"], 0);
        assert_eq!(serialized["loader_workspace"]["flags_slot"], 0);
    }
}
