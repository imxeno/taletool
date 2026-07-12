//! Handlers for `taletool effect` asset commands.

use std::fs;

use anyhow::{Context, bail};
use serde_json::json;
use taletool_effect::{EffectAsset, EffectAssetKind, decode_effect_asset};

use crate::cli::{EffectCommand, EffectKindArg};
use crate::effect_file::{pack_effect_file, unpack_effect_file};
use crate::util::fnv1a64;

/// Dispatch an `effect` subcommand.
pub(crate) fn run_effect(command: EffectCommand) -> anyhow::Result<()> {
    match command {
        EffectCommand::Inspect {
            input,
            kind,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let effect = decode_effect_kind(&data, kind)?;
            let checksum_value = checksum.then(|| format!("{:016x}", fnv1a64(&data)));
            inspect_effect(&effect, data.len(), checksum_value.as_deref(), json_output)
        }
        EffectCommand::Unpack { input, out, kind } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let effect = decode_effect_kind(&data, kind)?;
            unpack_effect_file(&effect, &out)?;
            println!(
                "unpacked {} effect payload into {}",
                kind_name(effect.kind()),
                out.display()
            );
            Ok(())
        }
        EffectCommand::Pack { input, out } => {
            let effect = pack_effect_file(&input, &out)?;
            println!(
                "packed {} effect payload into {}",
                kind_name(effect.kind()),
                out.display()
            );
            Ok(())
        }
    }
}

fn decode_effect_kind(data: &[u8], kind: EffectKindArg) -> anyhow::Result<EffectAsset> {
    if let Some(kind) = concrete_kind(kind) {
        return decode_effect_asset(kind, data)
            .with_context(|| format!("decoding {} effect payload", kind_name(kind)));
    }

    let mut matches = Vec::new();
    let mut errors = Vec::new();
    for kind in [
        EffectAssetKind::ColorAnimation,
        EffectAssetKind::Definition,
        EffectAssetKind::TransformAnimation,
        EffectAssetKind::TextureAnimation,
    ] {
        match decode_effect_asset(kind, data) {
            Ok(effect) => matches.push(effect),
            Err(error) => errors.push(format!("{}: {error}", kind_name(kind))),
        }
    }
    match matches.len() {
        1 => Ok(matches.pop().expect("one effect payload matched")),
        0 => bail!(
            "effect payload matches no supported format; {}",
            errors.join("; ")
        ),
        _ => {
            let names = matches
                .iter()
                .map(|effect| kind_name(effect.kind()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("effect payload matches multiple formats ({names}); select --kind")
        }
    }
}

fn concrete_kind(kind: EffectKindArg) -> Option<EffectAssetKind> {
    match kind {
        EffectKindArg::Auto => None,
        EffectKindArg::ColorAnimation => Some(EffectAssetKind::ColorAnimation),
        EffectKindArg::Definition => Some(EffectAssetKind::Definition),
        EffectKindArg::TransformAnimation => Some(EffectAssetKind::TransformAnimation),
        EffectKindArg::TextureAnimation => Some(EffectAssetKind::TextureAnimation),
    }
}

fn kind_name(kind: EffectAssetKind) -> &'static str {
    match kind {
        EffectAssetKind::ColorAnimation => "color-animation",
        EffectAssetKind::Definition => "definition",
        EffectAssetKind::TransformAnimation => "transform-animation",
        EffectAssetKind::TextureAnimation => "texture-animation",
    }
}

fn inspect_effect(
    effect: &EffectAsset,
    encoded_size: usize,
    checksum: Option<&str>,
    json_output: bool,
) -> anyhow::Result<()> {
    let details = match effect {
        EffectAsset::ColorAnimation(animation) => json!({
            "timing": animation.timing,
            "keyframe_count": animation.keyframes.len(),
        }),
        EffectAsset::TextureAnimation(animation) => {
            let mut keys = animation
                .keyframes
                .iter()
                .map(|key| key.texture_resource_key)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            keys.dedup();
            json!({
                "timing": animation.timing,
                "keyframe_count": animation.keyframes.len(),
                "texture_resource_keys": keys,
            })
        }
        EffectAsset::TransformAnimation(animation) => json!({
            "timing": animation.timing,
            "translation_keyframe_count": animation.translation_keyframes.len(),
            "rotation_keyframe_count": animation.rotation_keyframes.len(),
            "scale_keyframe_count": animation.scale_keyframes.len(),
        }),
        EffectAsset::Definition(definition) => {
            let mut classes = definition
                .components
                .iter()
                .map(|component| component.kind_code())
                .collect::<Vec<_>>();
            classes.sort_unstable();
            classes.dedup();
            json!({
                "resource_key": definition.resource_key,
                "component_count": definition.components.len(),
                "component_kind_codes": classes,
                "keyframe_count": definition.keyframe_count(),
                "geometry_resource_keys": definition.geometry_resource_keys(),
            })
        }
    };
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "effect",
                "kind": kind_name(effect.kind()),
                "encoded_size": encoded_size,
                "checksum_fnv1a64": checksum,
                "details": details,
            }))?
        );
    } else {
        println!("type: effect");
        println!("kind: {}", kind_name(effect.kind()));
        println!("encoded_size: {encoded_size}");
        if let Some(checksum) = checksum {
            println!("checksum_fnv1a64: {checksum}");
        }
        let object = details.as_object().expect("effect details are an object");
        for (name, value) in object {
            println!("{name}: {value}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use taletool_effect::{
        AnimationTiming, ColorAnimation, TextureAnimation, write_color_animation_bytes,
        write_texture_animation_bytes,
    };

    use super::*;

    fn timing() -> AnimationTiming {
        AnimationTiming {
            first_frame: 0,
            last_frame: 1,
            frame_rate: 30,
            keyframe_step: 160,
        }
    }

    #[test]
    fn explicit_kinds_decode_structurally_identical_value_animations() {
        let bytes = write_color_animation_bytes(&ColorAnimation {
            timing: timing(),
            keyframes: Vec::new(),
        })
        .unwrap();
        assert!(matches!(
            decode_effect_kind(&bytes, EffectKindArg::ColorAnimation).unwrap(),
            EffectAsset::ColorAnimation(_)
        ));
        assert!(matches!(
            decode_effect_kind(&bytes, EffectKindArg::TextureAnimation).unwrap(),
            EffectAsset::TextureAnimation(_)
        ));
    }

    #[test]
    fn auto_rejects_ambiguous_value_animation() {
        let bytes = write_texture_animation_bytes(&TextureAnimation {
            timing: timing(),
            keyframes: Vec::new(),
        })
        .unwrap();
        let error = decode_effect_kind(&bytes, EffectKindArg::Auto)
            .unwrap_err()
            .to_string();
        assert!(error.contains("multiple formats"));
        assert!(error.contains("select --kind"));
    }
}
