//! Handlers for `taletool sprite-remap` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::{Value, json};
use taletool_animation::sprite_remap::{SpriteResourceRemap, decode_sprite_resource_remap};

use crate::cli::SpriteRemapCommand;
use crate::sprite_remap_file::{
    pack_sprite_resource_remap_file, unpack_sprite_resource_remap_file,
};
use crate::util::fnv1a64;

/// Dispatch a `sprite-remap` subcommand.
pub(crate) fn run_sprite_remap(command: SpriteRemapCommand) -> anyhow::Result<()> {
    match command {
        SpriteRemapCommand::Inspect {
            input,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let remap = decode_sprite_resource_remap(&data)
                .with_context(|| format!("decoding sprite-resource remap {}", input.display()))?;
            inspect_sprite_remap(
                &remap,
                data.len(),
                checksum.then(|| fnv1a64(&data)),
                json_output,
            )
        }
        SpriteRemapCommand::Unpack { input, out } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let remap = decode_sprite_resource_remap(&data)
                .with_context(|| format!("decoding sprite-resource remap {}", input.display()))?;
            unpack_sprite_resource_remap_file(&remap, &out)?;
            println!(
                "unpacked {} sprite-resource remap frames into {}",
                remap.frames.len(),
                out.display()
            );
            Ok(())
        }
        SpriteRemapCommand::Pack { input, out } => {
            let remap = pack_sprite_resource_remap_file(&input, &out)?;
            println!(
                "packed {} sprite-resource remap frames into {}",
                remap.frames.len(),
                out.display()
            );
            Ok(())
        }
    }
}

fn inspect_sprite_remap(
    remap: &SpriteResourceRemap,
    encoded_size: usize,
    checksum: Option<u64>,
    json_output: bool,
) -> anyhow::Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&sprite_remap_json_summary(
                remap,
                encoded_size,
                checksum
            ))?
        );
    } else {
        println!("type: sprite-resource-remap");
        println!("encoded_size: {encoded_size}");
        println!("frames: {}", remap.frames.len());
        println!("identity_frames: {}", remap.identity_frame_count());
        println!("skipped_slots: {}", remap.skipped_slot_count());
        if let Some(checksum) = checksum {
            println!("checksum_fnv1a64: {checksum:016x}");
        }
    }
    Ok(())
}

fn sprite_remap_json_summary(
    remap: &SpriteResourceRemap,
    encoded_size: usize,
    checksum: Option<u64>,
) -> Value {
    json!({
        "type": "sprite-resource-remap",
        "encoded_size": encoded_size,
        "frame_count": remap.frames.len(),
        "identity_frame_count": remap.identity_frame_count(),
        "skipped_slot_count": remap.skipped_slot_count(),
        "checksum_fnv1a64": checksum.map(|value| format!("{value:016x}")),
    })
}

#[cfg(test)]
mod tests {
    use taletool_animation::sprite_remap::SpriteFrameResourceRemap;

    use super::*;

    #[test]
    fn inspect_summary_helpers_report_identity_and_skipped_slots() {
        let remap = SpriteResourceRemap {
            frames: vec![
                SpriteFrameResourceRemap {
                    resource_indices: [0, 1, 2, 3, 4, 5, 6, 7],
                },
                SpriteFrameResourceRemap {
                    resource_indices: [0, 1, 2, 3, 4, 5, 8, 255],
                },
            ],
        };

        assert_eq!(remap.identity_frame_count(), 1);
        assert_eq!(remap.skipped_slot_count(), 2);
        assert_eq!(
            sprite_remap_json_summary(&remap, 17, Some(0x0123)),
            json!({
                "type": "sprite-resource-remap",
                "encoded_size": 17,
                "frame_count": 2,
                "identity_frame_count": 1,
                "skipped_slot_count": 2,
                "checksum_fnv1a64": "0000000000000123",
            })
        );
        inspect_sprite_remap(&remap, 17, Some(0x0123), false).unwrap();
        inspect_sprite_remap(&remap, 17, None, true).unwrap();
    }
}
