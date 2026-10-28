//! Icons and sprite payloads used by map-object resources.
//!
//! `NSmpData`, `NSppData`, and `NSipData` archive entries use this format.
//! Each payload contains a counted descriptor table followed by little-endian
//! `A4R4G4B4` pixel blocks addressed by absolute payload offsets.

use image::RgbaImage;
use taletool_core::ByteReader;
use thiserror::Error;

use crate::{decode_a4r4g4b4, encode_a4r4g4b4};

pub mod free_size;

pub const SPRITE_FRAME_DESCRIPTOR_LEN: usize = 12;
pub const SPRITE_MAX_DIMENSION: u32 = u16::MAX as u32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SpriteError {
    #[error("sprite payload is too short: {len} bytes")]
    TooShort { len: usize },
    #[error("zero-frame sprite sentinel has {trailing} trailing bytes")]
    TrailingZeroFrameData { trailing: usize },
    #[error("sprite descriptor table is truncated: need {needed} bytes, got {actual}")]
    TruncatedDescriptorTable { needed: usize, actual: usize },
    #[error("sprite frame {frame} dimensions exceed the format limit of {limit}: {width}x{height}")]
    DimensionsTooLarge {
        frame: usize,
        width: u32,
        height: u32,
        limit: u32,
    },
    #[error(
        "sprite frame {frame} data offset {offset} points into the {table_end}-byte descriptor table"
    )]
    DataOffsetInTable {
        frame: usize,
        offset: usize,
        table_end: usize,
    },
    #[error("sprite frame {frame} pixel byte count overflow for {width}x{height}")]
    PixelSizeOverflow {
        frame: usize,
        width: u32,
        height: u32,
    },
    #[error(
        "sprite frame {frame} pixels are truncated at offset {offset}: need {needed} bytes, got {actual}"
    )]
    TruncatedPixels {
        frame: usize,
        offset: usize,
        needed: usize,
        actual: usize,
    },
    #[error("sprite has too many frames: {count}; maximum is 255")]
    TooManyFrames { count: usize },
    #[error("sprite payload is too large: {size} bytes")]
    PayloadTooLarge { size: usize },
    #[error("could not allocate {size} bytes for sprite data")]
    AllocationFailed { size: usize },
}

pub type SpriteResult<T> = std::result::Result<T, SpriteError>;

/// Editable sprite frame used by the writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFrame {
    pub source_x: i16,
    pub source_y: i16,
    pub image: RgbaImage,
}

impl SpriteFrame {
    pub fn new(source_x: i16, source_y: i16, image: RgbaImage) -> Self {
        Self {
            source_x,
            source_y,
            image,
        }
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }
}

/// One decoded frame plus its original absolute pixel-data offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSpriteFrame {
    pub data_offset: usize,
    pub frame: SpriteFrame,
}

impl DecodedSpriteFrame {
    pub fn width(&self) -> u32 {
        self.frame.width()
    }

    pub fn height(&self) -> u32 {
        self.frame.height()
    }
}

/// Fully decoded compact sprite payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSprite {
    pub frames: Vec<DecodedSpriteFrame>,
}

/// A validated frame borrowing its encoded pixels from the sprite payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteFrameData<'a> {
    width: u32,
    height: u32,
    pub source_x: i16,
    pub source_y: i16,
    pub data_offset: usize,
    frame_index: usize,
    pixels: &'a [u8],
}

impl SpriteFrameData<'_> {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    /// Expand each stored pixel into straight RGBA, without allocating an image.
    pub fn pixels(&self) -> impl ExactSizeIterator<Item = image::Rgba<u8>> + '_ {
        self.pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|encoded| decode_a4r4g4b4(u16::from_le_bytes(*encoded)))
    }

    pub fn decode(&self) -> SpriteResult<DecodedSpriteFrame> {
        let size = self
            .pixels
            .len()
            .checked_mul(2)
            .ok_or(SpriteError::PixelSizeOverflow {
                frame: self.frame_index,
                width: self.width,
                height: self.height,
            })?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(size)
            .map_err(|_| SpriteError::AllocationFailed { size })?;
        for pixel in self.pixels() {
            rgba.extend_from_slice(&pixel.0);
        }
        let image = RgbaImage::from_raw(self.width, self.height, rgba)
            .expect("decoded pixel buffer matches the validated dimensions");
        Ok(DecodedSpriteFrame {
            data_offset: self.data_offset,
            frame: SpriteFrame::new(self.source_x, self.source_y, image),
        })
    }
}

/// Validated descriptors and borrowed pixels, without decoded frame allocations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpritePayload<'a> {
    pub frames: Vec<SpriteFrameData<'a>>,
}

/// Parse all descriptors and validate populated pixel ranges before decoding.
pub fn parse_sprite(data: &[u8]) -> SpriteResult<SpritePayload<'_>> {
    let Some(&frame_count) = data.first() else {
        return Err(SpriteError::TooShort { len: data.len() });
    };
    let frame_count = usize::from(frame_count);
    if frame_count == 0 && data.len() != 1 {
        return Err(SpriteError::TrailingZeroFrameData {
            trailing: data.len() - 1,
        });
    }
    let table_end = 1 + frame_count * SPRITE_FRAME_DESCRIPTOR_LEN;
    if data.len() < table_end {
        return Err(SpriteError::TruncatedDescriptorTable {
            needed: table_end,
            actual: data.len(),
        });
    }

    let mut reader = ByteReader::new_at(data, 1);
    let mut frames = Vec::with_capacity(frame_count);
    for frame in 0..frame_count {
        let width = reader
            .read_u16_le("sprite.frame.width")
            .expect("sprite descriptor table length was checked");
        let height = reader
            .read_u16_le("sprite.frame.height")
            .expect("sprite descriptor table length was checked");
        let source_x = reader
            .read_i16_le("sprite.frame.source_x")
            .expect("sprite descriptor table length was checked");
        let source_y = reader
            .read_i16_le("sprite.frame.source_y")
            .expect("sprite descriptor table length was checked");
        let data_offset = reader
            .read_u32_le("sprite.frame.data_offset")
            .expect("sprite descriptor table length was checked")
            as usize;
        let pixel_byte_count = pixel_byte_count(frame, width.into(), height.into())?;
        let pixels = if pixel_byte_count == 0 {
            // NosTale does not read pixel data for an empty frame.
            &[][..]
        } else {
            if data_offset < table_end {
                return Err(SpriteError::DataOffsetInTable {
                    frame,
                    offset: data_offset,
                    table_end,
                });
            }
            let end = data_offset.checked_add(pixel_byte_count).ok_or(
                SpriteError::PixelSizeOverflow {
                    frame,
                    width: width.into(),
                    height: height.into(),
                },
            )?;
            let Some(pixels) = data.get(data_offset..end) else {
                return Err(SpriteError::TruncatedPixels {
                    frame,
                    offset: data_offset,
                    needed: pixel_byte_count,
                    actual: data.len().saturating_sub(data_offset),
                });
            };

            pixels
        };
        frames.push(SpriteFrameData {
            width: width.into(),
            height: height.into(),
            source_x,
            source_y,
            data_offset,
            frame_index: frame,
            pixels,
        });
    }

    Ok(SpritePayload { frames })
}

/// Decode one compact sprite payload into editable RGBA frames.
pub fn decode_sprite(data: &[u8]) -> SpriteResult<DecodedSprite> {
    let frames = parse_sprite(data)?
        .frames
        .iter()
        .map(SpriteFrameData::decode)
        .collect::<SpriteResult<_>>()?;
    Ok(DecodedSprite { frames })
}

/// Encode ordered RGBA frames into the compact sprite layout.
pub fn write_sprite_bytes(frames: &[SpriteFrame]) -> SpriteResult<Vec<u8>> {
    if frames.len() > usize::from(u8::MAX) {
        return Err(SpriteError::TooManyFrames {
            count: frames.len(),
        });
    }

    let table_end = 1 + frames.len() * SPRITE_FRAME_DESCRIPTOR_LEN;
    let mut payload_size = table_end;
    for (frame_index, frame) in frames.iter().enumerate() {
        validate_dimensions(frame_index, frame.width(), frame.height())?;
        let byte_count = pixel_byte_count(frame_index, frame.width(), frame.height())?;
        payload_size = payload_size
            .checked_add(byte_count)
            .ok_or(SpriteError::PayloadTooLarge { size: usize::MAX })?;
    }
    if payload_size > u32::MAX as usize {
        return Err(SpriteError::PayloadTooLarge { size: payload_size });
    }

    let mut output = Vec::new();
    output
        .try_reserve_exact(payload_size)
        .map_err(|_| SpriteError::AllocationFailed { size: payload_size })?;
    output.push(frames.len() as u8);
    let mut data_offset = table_end;
    for (frame_index, frame) in frames.iter().enumerate() {
        let width = u16::try_from(frame.width()).map_err(|_| SpriteError::DimensionsTooLarge {
            frame: frame_index,
            width: frame.width(),
            height: frame.height(),
            limit: SPRITE_MAX_DIMENSION,
        })?;
        let height =
            u16::try_from(frame.height()).map_err(|_| SpriteError::DimensionsTooLarge {
                frame: frame_index,
                width: frame.width(),
                height: frame.height(),
                limit: SPRITE_MAX_DIMENSION,
            })?;
        output.extend_from_slice(&width.to_le_bytes());
        output.extend_from_slice(&height.to_le_bytes());
        output.extend_from_slice(&frame.source_x.to_le_bytes());
        output.extend_from_slice(&frame.source_y.to_le_bytes());
        output.extend_from_slice(&(data_offset as u32).to_le_bytes());
        data_offset += pixel_byte_count(frame_index, frame.width(), frame.height())?;
    }

    for frame in frames {
        for pixel in frame.image.pixels() {
            output.extend_from_slice(&encode_a4r4g4b4(*pixel).to_le_bytes());
        }
    }
    debug_assert_eq!(output.len(), payload_size);
    Ok(output)
}

fn validate_dimensions(frame: usize, width: u32, height: u32) -> SpriteResult<()> {
    if width > SPRITE_MAX_DIMENSION || height > SPRITE_MAX_DIMENSION {
        return Err(SpriteError::DimensionsTooLarge {
            frame,
            width,
            height,
            limit: SPRITE_MAX_DIMENSION,
        });
    }
    Ok(())
}

fn pixel_byte_count(frame: usize, width: u32, height: u32) -> SpriteResult<usize> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(2));
    pixels.ok_or(SpriteError::PixelSizeOverflow {
        frame,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn frame(source_x: i16, source_y: i16, pixels: &[[u8; 4]]) -> SpriteFrame {
        let mut image = RgbaImage::new(pixels.len() as u32, 1);
        for (pixel, value) in image.pixels_mut().zip(pixels) {
            *pixel = Rgba(*value);
        }
        SpriteFrame::new(source_x, source_y, image)
    }

    #[test]
    fn empty_sprite_round_trips() {
        let decoded = decode_sprite(&[0]).unwrap();
        assert!(decoded.frames.is_empty());
        assert_eq!(write_sprite_bytes(&[]).unwrap(), [0]);
        assert_eq!(
            decode_sprite(&[0, 1]),
            Err(SpriteError::TrailingZeroFrameData { trailing: 1 })
        );
    }

    #[test]
    fn decodes_and_rebuilds_multiple_frames_byte_for_byte() {
        let bytes = [
            2, // frame count
            2, 0, 1, 0, 0xfe, 0xff, 7, 0, 25, 0, 0, 0, // frame 0
            1, 0, 1, 0, 4, 0, 5, 0, 29, 0, 0, 0, // frame 1
            0x23, 0xf1, 0x68, 0x84, // frame 0 pixels
            0xff, 0x0f, // frame 1 pixel
        ];
        let decoded = decode_sprite(&bytes).unwrap();
        assert_eq!(decoded.frames.len(), 2);
        assert_eq!(decoded.frames[0].data_offset, 25);
        assert_eq!(decoded.frames[0].frame.source_x, -2);
        assert_eq!(decoded.frames[0].frame.source_y, 7);
        assert_eq!(
            decoded.frames[0].frame.image.get_pixel(0, 0).0,
            [17, 34, 51, 255]
        );
        assert_eq!(
            decoded.frames[0].frame.image.get_pixel(1, 0).0,
            [68, 102, 136, 136]
        );

        let frames = decoded
            .frames
            .iter()
            .map(|decoded| decoded.frame.clone())
            .collect::<Vec<_>>();
        assert_eq!(write_sprite_bytes(&frames).unwrap(), bytes);
    }

    #[test]
    fn quantizes_edited_channels_to_nearest_nibble() {
        let bytes = write_sprite_bytes(&[frame(0, 0, &[[8, 25, 42, 247]])]).unwrap();
        assert_eq!(&bytes[13..], &0xf012_u16.to_le_bytes());
    }

    #[test]
    fn rejects_truncated_or_invalid_frames() {
        assert_eq!(decode_sprite(&[]), Err(SpriteError::TooShort { len: 0 }));
        assert_eq!(
            decode_sprite(&[1]),
            Err(SpriteError::TruncatedDescriptorTable {
                needed: 13,
                actual: 1,
            })
        );

        let data_in_table = [1, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0];
        assert!(matches!(
            decode_sprite(&data_in_table),
            Err(SpriteError::DataOffsetInTable { frame: 0, .. })
        ));

        let truncated_pixels = [1, 1, 0, 1, 0, 0, 0, 0, 0, 13, 0, 0, 0];
        assert!(matches!(
            decode_sprite(&truncated_pixels),
            Err(SpriteError::TruncatedPixels { frame: 0, .. })
        ));
    }

    #[test]
    fn rejects_dimensions_and_counts_that_do_not_fit_the_format() {
        let too_wide = SpriteFrame::new(0, 0, RgbaImage::new(SPRITE_MAX_DIMENSION + 1, 1));
        assert!(matches!(
            write_sprite_bytes(&[too_wide]),
            Err(SpriteError::DimensionsTooLarge { frame: 0, .. })
        ));

        let frames = vec![frame(0, 0, &[[0, 0, 0, 0]]); 256];
        assert_eq!(
            write_sprite_bytes(&frames),
            Err(SpriteError::TooManyFrames { count: 256 })
        );
    }

    #[test]
    fn round_trips_empty_and_oversized_frames_in_order() {
        let frames = [
            (0, 0),
            (0, 65535),
            (65535, 0),
            (513, 2),
            (2, 513),
            (65535, 1),
            (1, 65535),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (width, height))| {
            SpriteFrame::new(
                index as i16,
                -(index as i16),
                RgbaImage::from_pixel(width, height, Rgba([17, 34, 51, 136])),
            )
        })
        .collect::<Vec<_>>();
        let bytes = write_sprite_bytes(&frames).unwrap();
        let decoded = decode_sprite(&bytes).unwrap();
        let actual = decoded
            .frames
            .into_iter()
            .map(|frame| frame.frame)
            .collect::<Vec<_>>();
        assert_eq!(actual, frames);
        assert_eq!(write_sprite_bytes(&actual).unwrap(), bytes);
    }

    #[test]
    fn empty_frames_ignore_offsets_but_populated_frames_require_pixels() {
        for (width, height) in [(0_u16, 0_u16), (0, 65535), (65535, 0)] {
            for offset in [0_u32, 1, 13, 14, u32::MAX] {
                let mut bytes = vec![1];
                bytes.extend_from_slice(&width.to_le_bytes());
                bytes.extend_from_slice(&height.to_le_bytes());
                bytes.extend_from_slice(&(-7_i16).to_le_bytes());
                bytes.extend_from_slice(&12_i16.to_le_bytes());
                bytes.extend_from_slice(&offset.to_le_bytes());
                let decoded = decode_sprite(&bytes).unwrap();
                assert_eq!(decoded.frames[0].data_offset, offset as usize);
                assert_eq!(
                    decoded.frames[0].frame.image.dimensions(),
                    (width.into(), height.into())
                );
                assert!(decoded.frames[0].frame.image.as_raw().is_empty());
                bytes[1..3].copy_from_slice(&65535_u16.to_le_bytes());
                bytes[3..5].copy_from_slice(&65535_u16.to_le_bytes());
                assert!(parse_sprite(&bytes).is_err());
            }
        }
    }
}
