//! Decoding and encoding for player sprite-resource remap payloads.

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// Number of rendering resource slots in every remap frame.
pub const SPRITE_RESOURCE_SLOT_COUNT: usize = 8;

/// Maximum frame count representable by the native one-byte header.
pub const MAX_SPRITE_REMAP_FRAMES: usize = u8::MAX as usize;

const IDENTITY_RESOURCE_INDICES: [u8; SPRITE_RESOURCE_SLOT_COUNT] = [0, 1, 2, 3, 4, 5, 6, 7];

/// Resource-index ordering used for one sprite animation frame.
///
/// Indices outside the eight-slot range are preserved because the client uses
/// them to skip the corresponding rendering slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteFrameResourceRemap {
    pub resource_indices: [u8; SPRITE_RESOURCE_SLOT_COUNT],
}

impl SpriteFrameResourceRemap {
    /// Returns whether this frame keeps every rendering resource in its
    /// original slot.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.resource_indices == IDENTITY_RESOURCE_INDICES
    }

    /// Returns the number of rendering slots skipped by this frame.
    #[must_use]
    pub fn skipped_slot_count(&self) -> usize {
        self.resource_indices
            .iter()
            .filter(|&&resource_index| usize::from(resource_index) >= SPRITE_RESOURCE_SLOT_COUNT)
            .count()
    }
}

/// Ordered resource remaps selected by the active sprite animation frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteResourceRemap {
    pub frames: Vec<SpriteFrameResourceRemap>,
}

impl SpriteResourceRemap {
    /// Returns how many frames use the identity resource ordering.
    #[must_use]
    pub fn identity_frame_count(&self) -> usize {
        self.frames
            .iter()
            .filter(|frame| frame.is_identity())
            .count()
    }

    /// Returns the total number of skipped rendering slots across all frames.
    #[must_use]
    pub fn skipped_slot_count(&self) -> usize {
        self.frames
            .iter()
            .map(SpriteFrameResourceRemap::skipped_slot_count)
            .sum()
    }
}

/// Errors produced while decoding or encoding a sprite-resource remap.
#[derive(Debug, Error)]
pub enum SpriteResourceRemapError {
    #[error(transparent)]
    Truncated(#[from] ByteReadError),
    #[error(
        "sprite-resource remap declares {frame_count} frames ({needed} bytes), but only {actual} frame bytes remain"
    )]
    TruncatedFrames {
        frame_count: usize,
        needed: usize,
        actual: usize,
    },
    #[error("sprite-resource remap has {count} trailing bytes")]
    TrailingBytes { count: usize },
    #[error("sprite-resource remap has {count} frames; maximum is {maximum}")]
    TooManyFrames { count: usize, maximum: usize },
}

/// Decodes a complete native sprite-resource remap payload.
pub fn decode_sprite_resource_remap(
    bytes: &[u8],
) -> Result<SpriteResourceRemap, SpriteResourceRemapError> {
    let mut reader = ByteReader::new(bytes);
    let frame_count = usize::from(reader.read_u8("sprite remap frame count")?);
    let needed = frame_count * SPRITE_RESOURCE_SLOT_COUNT;
    let actual = reader.remaining();
    if actual < needed {
        return Err(SpriteResourceRemapError::TruncatedFrames {
            frame_count,
            needed,
            actual,
        });
    }

    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        frames.push(SpriteFrameResourceRemap {
            resource_indices: reader.read_array("sprite frame resource indices")?,
        });
    }

    let trailing = reader.remaining();
    if trailing != 0 {
        return Err(SpriteResourceRemapError::TrailingBytes { count: trailing });
    }

    Ok(SpriteResourceRemap { frames })
}

/// Encodes a sprite-resource remap in its native representation.
pub fn write_sprite_resource_remap_bytes(
    remap: &SpriteResourceRemap,
) -> Result<Vec<u8>, SpriteResourceRemapError> {
    let frame_count = remap.frames.len();
    let encoded_frame_count =
        u8::try_from(frame_count).map_err(|_| SpriteResourceRemapError::TooManyFrames {
            count: frame_count,
            maximum: MAX_SPRITE_REMAP_FRAMES,
        })?;

    let mut bytes = Vec::with_capacity(1 + frame_count * SPRITE_RESOURCE_SLOT_COUNT);
    bytes.push(encoded_frame_count);
    for frame in &remap.frames {
        bytes.extend_from_slice(&frame.resource_indices);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(resource_indices: [u8; SPRITE_RESOURCE_SLOT_COUNT]) -> SpriteFrameResourceRemap {
        SpriteFrameResourceRemap { resource_indices }
    }

    #[test]
    fn decodes_representative_real_layout_rows_and_round_trips_exactly() {
        let bytes = [2, 1, 6, 0, 4, 2, 3, 5, 7, 1, 6, 0, 4, 2, 3, 5, 7];

        let remap = decode_sprite_resource_remap(&bytes).unwrap();

        assert_eq!(
            remap.frames,
            vec![
                frame([1, 6, 0, 4, 2, 3, 5, 7]),
                frame([1, 6, 0, 4, 2, 3, 5, 7]),
            ]
        );
        assert_eq!(write_sprite_resource_remap_bytes(&remap).unwrap(), bytes);
    }

    #[test]
    fn supports_empty_and_maximum_frame_payloads() {
        let empty = decode_sprite_resource_remap(&[0]).unwrap();
        assert!(empty.frames.is_empty());
        assert_eq!(write_sprite_resource_remap_bytes(&empty).unwrap(), [0]);

        let maximum = SpriteResourceRemap {
            frames: vec![frame(IDENTITY_RESOURCE_INDICES); MAX_SPRITE_REMAP_FRAMES],
        };
        let bytes = write_sprite_resource_remap_bytes(&maximum).unwrap();
        assert_eq!(bytes.len(), 1 + MAX_SPRITE_REMAP_FRAMES * 8);
        assert_eq!(decode_sprite_resource_remap(&bytes).unwrap(), maximum);
    }

    #[test]
    fn reports_truncated_and_trailing_data() {
        assert!(matches!(
            decode_sprite_resource_remap(&[]),
            Err(SpriteResourceRemapError::Truncated(_))
        ));
        assert!(matches!(
            decode_sprite_resource_remap(&[2, 0, 1, 2]),
            Err(SpriteResourceRemapError::TruncatedFrames {
                frame_count: 2,
                needed: 16,
                actual: 3,
            })
        ));
        assert!(matches!(
            decode_sprite_resource_remap(&[0, 9]),
            Err(SpriteResourceRemapError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn rejects_oversized_frame_lists_when_writing() {
        let remap = SpriteResourceRemap {
            frames: vec![frame(IDENTITY_RESOURCE_INDICES); MAX_SPRITE_REMAP_FRAMES + 1],
        };

        assert!(matches!(
            write_sprite_resource_remap_bytes(&remap),
            Err(SpriteResourceRemapError::TooManyFrames {
                count: 256,
                maximum: 255,
            })
        ));
    }

    #[test]
    fn counts_identity_frames_and_preserves_out_of_range_skip_values() {
        let remap = SpriteResourceRemap {
            frames: vec![
                frame(IDENTITY_RESOURCE_INDICES),
                frame([0, 1, 2, 3, 4, 5, 8, u8::MAX]),
            ],
        };

        assert_eq!(remap.identity_frame_count(), 1);
        assert_eq!(remap.skipped_slot_count(), 2);

        let bytes = write_sprite_resource_remap_bytes(&remap).unwrap();
        assert_eq!(decode_sprite_resource_remap(&bytes).unwrap(), remap);
    }
}
