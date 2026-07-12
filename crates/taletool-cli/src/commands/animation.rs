//! Handlers for `taletool animation` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::json;
use taletool_animation::sprite::{SpriteAnimation, decode_sprite_animation};

use crate::animation_file::{pack_sprite_animation_file, unpack_sprite_animation_file};
use crate::cli::AnimationCommand;
use crate::util::fnv1a64;

/// Dispatch an `animation` subcommand.
pub(crate) fn run_animation(command: AnimationCommand) -> anyhow::Result<()> {
    match command {
        AnimationCommand::Inspect {
            input,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let animation = decode_sprite_animation(&data)
                .with_context(|| format!("decoding sprite animation {}", input.display()))?;
            inspect_animation(
                &animation,
                data.len(),
                checksum.then(|| fnv1a64(&data)),
                json_output,
            )
        }
        AnimationCommand::Unpack { input, out } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let animation = decode_sprite_animation(&data)
                .with_context(|| format!("decoding sprite animation {}", input.display()))?;
            unpack_sprite_animation_file(&animation, &out)?;
            println!(
                "unpacked {} animation frames into {}",
                animation.frames.len(),
                out.display()
            );
            Ok(())
        }
        AnimationCommand::Pack { input, out } => {
            let animation = pack_sprite_animation_file(&input, &out)?;
            println!(
                "packed {} animation frames into {}",
                animation.frames.len(),
                out.display()
            );
            Ok(())
        }
    }
}

fn inspect_animation(
    animation: &SpriteAnimation,
    encoded_size: usize,
    checksum: Option<u64>,
    json_output: bool,
) -> anyhow::Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "sprite-animation",
                "encoded_size": encoded_size,
                "frame_count": animation.frames.len(),
                "duration_ticks": animation.duration_ticks(),
                "looping": animation.is_looping(),
                "playback_flags": animation.playback_flags,
                "event_count": animation.event_count(),
                "checksum_fnv1a64": checksum.map(|value| format!("{value:016x}")),
            }))?
        );
    } else {
        println!("type: sprite-animation");
        println!("encoded_size: {encoded_size}");
        println!("frames: {}", animation.frames.len());
        println!("duration_ticks: {}", animation.duration_ticks());
        println!("looping: {}", animation.is_looping());
        println!("playback_flags: 0x{:02x}", animation.playback_flags);
        println!("events: {}", animation.event_count());
        if let Some(checksum) = checksum {
            println!("checksum_fnv1a64: {checksum:016x}");
        }
    }
    Ok(())
}
