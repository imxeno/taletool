//! Sprite-animation payloads stored in `NSmcData` and `NSpcData` archives.
//!
//! Each payload contains one playback-flags byte and an ordered sequence of
//! sprite-frame indexes paired with event-timing flags.

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// Duration of one sprite-animation frame in game ticks.
pub const ANIMATION_FRAME_TICKS: u32 = 60;
/// Playback flag that makes an animation wrap to its first frame.
pub const ANIMATION_LOOP_FLAG: u8 = 0x80;
/// Maximum number of frames representable by the payload's count byte.
pub const MAX_ANIMATION_FRAMES: usize = u8::MAX as usize;

/// One frame in a sprite-animation sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteAnimationFrame {
    /// Frame selected from each participating sprite resource.
    pub sprite_frame_index: u8,
    /// Raw event-timing flag; zero is unmarked and all non-zero values mark the frame end.
    pub event_timing_flag: u8,
}

/// A complete sprite-animation sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteAnimation {
    /// Raw playback flags. Unknown bits are preserved.
    pub playback_flags: u8,
    /// Ordered sprite frames and their event-timing flags.
    pub frames: Vec<SpriteAnimationFrame>,
}

impl SpriteAnimation {
    /// Whether playback wraps after the last frame.
    pub fn is_looping(&self) -> bool {
        self.playback_flags & ANIMATION_LOOP_FLAG != 0
    }

    /// Total duration of the sequence in game ticks.
    pub fn duration_ticks(&self) -> u32 {
        u32::try_from(self.frames.len())
            .unwrap_or(u32::MAX)
            .saturating_mul(ANIMATION_FRAME_TICKS)
    }

    /// Number of frames whose event-timing flag is non-zero.
    pub fn marked_frame_count(&self) -> usize {
        self.frames
            .iter()
            .filter(|frame| frame.event_timing_flag != 0)
            .count()
    }
}

/// Errors produced while decoding or encoding sprite animations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SpriteAnimationError {
    #[error(transparent)]
    Truncated(#[from] ByteReadError),
    #[error(
        "sprite-animation frame table is truncated: count {frame_count} needs {needed} bytes, got {actual}"
    )]
    TruncatedFrames {
        frame_count: usize,
        needed: usize,
        actual: usize,
    },
    #[error("sprite-animation payload has {count} trailing bytes")]
    TrailingBytes { count: usize },
    #[error("sprite animation has {count} frames; maximum is {maximum}")]
    TooManyFrames { count: usize, maximum: usize },
}

pub type SpriteAnimationResult<T> = std::result::Result<T, SpriteAnimationError>;

/// Decode one `NSmcData` or `NSpcData` archive-entry payload.
pub fn decode_sprite_animation(data: &[u8]) -> SpriteAnimationResult<SpriteAnimation> {
    let mut reader = ByteReader::new(data);
    let frame_count = usize::from(reader.read_u8("sprite_animation.frame_count")?);
    let playback_flags = reader.read_u8("sprite_animation.playback_flags")?;
    let frame_bytes = frame_count * 2;
    if reader.remaining() < frame_bytes {
        return Err(SpriteAnimationError::TruncatedFrames {
            frame_count,
            needed: frame_bytes,
            actual: reader.remaining(),
        });
    }

    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        frames.push(SpriteAnimationFrame {
            sprite_frame_index: reader.read_u8("sprite_animation.frame.sprite_frame_index")?,
            event_timing_flag: reader.read_u8("sprite_animation.frame.event_timing_flag")?,
        });
    }
    if reader.remaining() != 0 {
        return Err(SpriteAnimationError::TrailingBytes {
            count: reader.remaining(),
        });
    }

    Ok(SpriteAnimation {
        playback_flags,
        frames,
    })
}

/// Encode one sprite animation into its native payload layout.
pub fn write_sprite_animation_bytes(animation: &SpriteAnimation) -> SpriteAnimationResult<Vec<u8>> {
    let frame_count = animation.frames.len();
    let frame_count_u8 =
        u8::try_from(frame_count).map_err(|_| SpriteAnimationError::TooManyFrames {
            count: frame_count,
            maximum: MAX_ANIMATION_FRAMES,
        })?;

    let mut output = Vec::with_capacity(2 + frame_count * 2);
    output.push(frame_count_u8);
    output.push(animation.playback_flags);
    for frame in &animation.frames {
        output.push(frame.sprite_frame_index);
        output.push(frame.event_timing_flag);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_and_encodes_animation_losslessly() {
        let bytes = [3, 0x85, 4, 0, 7, 2, 9, 0xff];
        let animation = decode_sprite_animation(&bytes).unwrap();
        assert_eq!(animation.playback_flags, 0x85);
        assert!(animation.is_looping());
        assert_eq!(animation.duration_ticks(), 180);
        assert_eq!(animation.marked_frame_count(), 2);
        assert_eq!(
            animation.frames,
            [
                SpriteAnimationFrame {
                    sprite_frame_index: 4,
                    event_timing_flag: 0,
                },
                SpriteAnimationFrame {
                    sprite_frame_index: 7,
                    event_timing_flag: 2,
                },
                SpriteAnimationFrame {
                    sprite_frame_index: 9,
                    event_timing_flag: 0xff,
                },
            ]
        );
        assert_eq!(write_sprite_animation_bytes(&animation).unwrap(), bytes);
    }

    #[test]
    fn preserves_empty_and_maximum_length_animations() {
        let empty = SpriteAnimation {
            playback_flags: 0x01,
            frames: Vec::new(),
        };
        assert_eq!(write_sprite_animation_bytes(&empty).unwrap(), [0, 1]);
        assert_eq!(decode_sprite_animation(&[0, 1]).unwrap(), empty);

        let maximum = SpriteAnimation {
            playback_flags: 0,
            frames: vec![
                SpriteAnimationFrame {
                    sprite_frame_index: 0xfe,
                    event_timing_flag: 0,
                };
                MAX_ANIMATION_FRAMES
            ],
        };
        let encoded = write_sprite_animation_bytes(&maximum).unwrap();
        assert_eq!(encoded[0], u8::MAX);
        assert_eq!(decode_sprite_animation(&encoded).unwrap(), maximum);
    }

    #[test]
    fn distinguishes_looping_from_unknown_flags() {
        let animation = SpriteAnimation {
            playback_flags: 0x7f,
            frames: Vec::new(),
        };
        assert!(!animation.is_looping());

        let looping = SpriteAnimation {
            playback_flags: ANIMATION_LOOP_FLAG,
            frames: Vec::new(),
        };
        assert!(looping.is_looping());
    }

    #[test]
    fn rejects_truncated_and_trailing_payloads() {
        assert!(matches!(
            decode_sprite_animation(&[]),
            Err(SpriteAnimationError::Truncated(_))
        ));
        assert!(matches!(
            decode_sprite_animation(&[1]),
            Err(SpriteAnimationError::Truncated(_))
        ));
        assert_eq!(
            decode_sprite_animation(&[2, 0, 1, 0]),
            Err(SpriteAnimationError::TruncatedFrames {
                frame_count: 2,
                needed: 4,
                actual: 2,
            })
        );
        assert_eq!(
            decode_sprite_animation(&[0, 0, 1]),
            Err(SpriteAnimationError::TrailingBytes { count: 1 })
        );
    }

    #[test]
    fn rejects_more_than_255_frames() {
        let animation = SpriteAnimation {
            playback_flags: 0,
            frames: vec![
                SpriteAnimationFrame {
                    sprite_frame_index: 0,
                    event_timing_flag: 0,
                };
                MAX_ANIMATION_FRAMES + 1
            ],
        };
        assert_eq!(
            write_sprite_animation_bytes(&animation),
            Err(SpriteAnimationError::TooManyFrames {
                count: MAX_ANIMATION_FRAMES + 1,
                maximum: MAX_ANIMATION_FRAMES,
            })
        );
    }
}
