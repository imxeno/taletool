//! Free-size sprite payloads used by `NS4BbData` resources.
//!
//! A payload contains one `A8R8G8B8` image split into blocks up to 256 pixels
//! square. Block columns are stored from left to right, blocks within a column
//! are stored from top to bottom, and pixels inside a block are row-major.

use image::RgbaImage;
use thiserror::Error;

use crate::{decode_a8r8g8b8, encode_a8r8g8b8};

pub const FREE_SIZE_SPRITE_HEADER_LEN: usize = 4;
pub const FREE_SIZE_SPRITE_BLOCK_DIMENSION: u32 = 256;
pub const FREE_SIZE_SPRITE_MAX_DIMENSION: u32 = i16::MAX as u32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FreeSizeSpriteError {
    #[error("free-size sprite payload is too short: {len} bytes")]
    TooShort { len: usize },
    #[error("free-size sprite dimensions must be non-zero, got {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("free-size sprite dimensions exceed the client limit of {limit}: {width}x{height}")]
    DimensionsTooLarge { width: u32, height: u32, limit: u32 },
    #[error("free-size sprite pixel byte count overflow for {width}x{height}")]
    PixelSizeOverflow { width: u32, height: u32 },
    #[error("free-size sprite pixels are truncated: need {needed} bytes, got {actual}")]
    TruncatedPixels { needed: usize, actual: usize },
    #[error("free-size sprite has {trailing} trailing bytes")]
    TrailingData { trailing: usize },
    #[error("could not allocate {size} bytes for free-size sprite pixels")]
    AllocationFailed { size: usize },
}

pub type FreeSizeSpriteResult<T> = std::result::Result<T, FreeSizeSpriteError>;

/// One decoded free-size sprite image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeSizeSprite {
    pub image: RgbaImage,
}

impl FreeSizeSprite {
    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn block_columns(&self) -> u32 {
        block_count(self.width())
    }

    pub fn block_rows(&self) -> u32 {
        block_count(self.height())
    }
}

/// Decode a block-interlaced free-size sprite into a normal row-major RGBA image.
pub fn decode_free_size_sprite(data: &[u8]) -> FreeSizeSpriteResult<FreeSizeSprite> {
    if data.len() < FREE_SIZE_SPRITE_HEADER_LEN {
        return Err(FreeSizeSpriteError::TooShort { len: data.len() });
    }

    let width = u32::from(u16::from_le_bytes([data[0], data[1]]));
    let height = u32::from(u16::from_le_bytes([data[2], data[3]]));
    validate_dimensions(width, height)?;
    let pixel_bytes = pixel_byte_count(width, height)?;
    let expected_len = FREE_SIZE_SPRITE_HEADER_LEN
        .checked_add(pixel_bytes)
        .ok_or(FreeSizeSpriteError::PixelSizeOverflow { width, height })?;
    if data.len() < expected_len {
        return Err(FreeSizeSpriteError::TruncatedPixels {
            needed: expected_len,
            actual: data.len(),
        });
    }
    if data.len() > expected_len {
        return Err(FreeSizeSpriteError::TrailingData {
            trailing: data.len() - expected_len,
        });
    }

    let mut rgba = Vec::new();
    rgba.try_reserve_exact(pixel_bytes)
        .map_err(|_| FreeSizeSpriteError::AllocationFailed { size: pixel_bytes })?;
    rgba.resize(pixel_bytes, 0);

    let mut source_offset = FREE_SIZE_SPRITE_HEADER_LEN;
    for block_x in (0..width).step_by(FREE_SIZE_SPRITE_BLOCK_DIMENSION as usize) {
        let block_width = FREE_SIZE_SPRITE_BLOCK_DIMENSION.min(width - block_x);
        for block_y in (0..height).step_by(FREE_SIZE_SPRITE_BLOCK_DIMENSION as usize) {
            let block_height = FREE_SIZE_SPRITE_BLOCK_DIMENSION.min(height - block_y);
            for local_y in 0..block_height {
                for local_x in 0..block_width {
                    let encoded = [
                        data[source_offset],
                        data[source_offset + 1],
                        data[source_offset + 2],
                        data[source_offset + 3],
                    ];
                    source_offset += 4;

                    let destination_pixel =
                        usize::try_from((block_y + local_y) * width + block_x + local_x)
                            .expect("validated free-size sprite dimensions fit usize");
                    let destination_offset = destination_pixel * 4;
                    rgba[destination_offset..destination_offset + 4]
                        .copy_from_slice(&decode_a8r8g8b8(encoded).0);
                }
            }
        }
    }
    debug_assert_eq!(source_offset, expected_len);

    let image = RgbaImage::from_raw(width, height, rgba)
        .expect("decoded pixel buffer length matches the validated dimensions");
    Ok(FreeSizeSprite { image })
}

/// Encode a row-major RGBA image into the block-interlaced layout.
pub fn write_free_size_sprite_bytes(image: &RgbaImage) -> FreeSizeSpriteResult<Vec<u8>> {
    let width = image.width();
    let height = image.height();
    validate_dimensions(width, height)?;
    let pixel_bytes = pixel_byte_count(width, height)?;
    let payload_size = FREE_SIZE_SPRITE_HEADER_LEN
        .checked_add(pixel_bytes)
        .ok_or(FreeSizeSpriteError::PixelSizeOverflow { width, height })?;

    let width_u16 = u16::try_from(width).map_err(|_| FreeSizeSpriteError::DimensionsTooLarge {
        width,
        height,
        limit: FREE_SIZE_SPRITE_MAX_DIMENSION,
    })?;
    let height_u16 =
        u16::try_from(height).map_err(|_| FreeSizeSpriteError::DimensionsTooLarge {
            width,
            height,
            limit: FREE_SIZE_SPRITE_MAX_DIMENSION,
        })?;

    let mut output = Vec::new();
    output
        .try_reserve_exact(payload_size)
        .map_err(|_| FreeSizeSpriteError::AllocationFailed { size: payload_size })?;
    output.extend_from_slice(&width_u16.to_le_bytes());
    output.extend_from_slice(&height_u16.to_le_bytes());

    for block_x in (0..width).step_by(FREE_SIZE_SPRITE_BLOCK_DIMENSION as usize) {
        let block_width = FREE_SIZE_SPRITE_BLOCK_DIMENSION.min(width - block_x);
        for block_y in (0..height).step_by(FREE_SIZE_SPRITE_BLOCK_DIMENSION as usize) {
            let block_height = FREE_SIZE_SPRITE_BLOCK_DIMENSION.min(height - block_y);
            for local_y in 0..block_height {
                for local_x in 0..block_width {
                    output.extend_from_slice(&encode_a8r8g8b8(
                        *image.get_pixel(block_x + local_x, block_y + local_y),
                    ));
                }
            }
        }
    }
    debug_assert_eq!(output.len(), payload_size);
    Ok(output)
}

fn validate_dimensions(width: u32, height: u32) -> FreeSizeSpriteResult<()> {
    if width == 0 || height == 0 {
        return Err(FreeSizeSpriteError::InvalidDimensions { width, height });
    }
    if width > FREE_SIZE_SPRITE_MAX_DIMENSION || height > FREE_SIZE_SPRITE_MAX_DIMENSION {
        return Err(FreeSizeSpriteError::DimensionsTooLarge {
            width,
            height,
            limit: FREE_SIZE_SPRITE_MAX_DIMENSION,
        });
    }
    Ok(())
}

fn pixel_byte_count(width: u32, height: u32) -> FreeSizeSpriteResult<usize> {
    usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(FreeSizeSpriteError::PixelSizeOverflow { width, height })
}

fn block_count(dimension: u32) -> u32 {
    dimension.div_ceil(FREE_SIZE_SPRITE_BLOCK_DIMENSION)
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn decodes_and_encodes_a8r8g8b8_losslessly() {
        let bytes = [1, 0, 1, 0, 10, 20, 30, 40];
        let sprite = decode_free_size_sprite(&bytes).unwrap();
        assert_eq!(sprite.image.get_pixel(0, 0).0, [30, 20, 10, 40]);
        assert_eq!(write_free_size_sprite_bytes(&sprite.image).unwrap(), bytes);
    }

    #[test]
    fn serializes_partial_blocks_in_column_major_order() {
        let mut image = RgbaImage::new(257, 257);
        let markers = [
            ((0, 0), Rgba([1, 2, 3, 4])),
            ((255, 255), Rgba([5, 6, 7, 8])),
            ((0, 256), Rgba([9, 10, 11, 12])),
            ((256, 0), Rgba([13, 14, 15, 16])),
            ((256, 256), Rgba([17, 18, 19, 20])),
        ];
        for ((x, y), pixel) in markers {
            image.put_pixel(x, y, pixel);
        }

        let bytes = write_free_size_sprite_bytes(&image).unwrap();
        let stored_pixel = |index: usize| {
            let offset = FREE_SIZE_SPRITE_HEADER_LEN + index * 4;
            &bytes[offset..offset + 4]
        };
        assert_eq!(stored_pixel(0), [3, 2, 1, 4]);
        assert_eq!(stored_pixel(256 * 256 - 1), [7, 6, 5, 8]);
        assert_eq!(stored_pixel(256 * 256), [11, 10, 9, 12]);
        assert_eq!(stored_pixel(256 * 256 + 256), [15, 14, 13, 16]);
        assert_eq!(stored_pixel(257 * 257 - 1), [19, 18, 17, 20]);

        assert_eq!(decode_free_size_sprite(&bytes).unwrap().image, image);
    }

    #[test]
    fn round_trips_representative_block_grids() {
        for (width, height) in [(256, 256), (512, 768), (609, 744)] {
            let mut image = RgbaImage::new(width, height);
            for (x, y, pixel) in image.enumerate_pixels_mut() {
                *pixel = Rgba([
                    x.wrapping_add(y) as u8,
                    x.wrapping_mul(3) as u8,
                    y.wrapping_mul(5) as u8,
                    x.wrapping_add(y.wrapping_mul(7)) as u8,
                ]);
            }
            let encoded = write_free_size_sprite_bytes(&image).unwrap();
            let decoded = decode_free_size_sprite(&encoded).unwrap();
            assert_eq!(decoded.image, image);
        }
    }

    #[test]
    fn rejects_invalid_dimensions_and_lengths() {
        assert_eq!(
            decode_free_size_sprite(&[]),
            Err(FreeSizeSpriteError::TooShort { len: 0 })
        );
        assert_eq!(
            decode_free_size_sprite(&[0, 0, 1, 0]),
            Err(FreeSizeSpriteError::InvalidDimensions {
                width: 0,
                height: 1,
            })
        );
        assert!(matches!(
            decode_free_size_sprite(&[0, 128, 1, 0]),
            Err(FreeSizeSpriteError::DimensionsTooLarge { .. })
        ));
        assert_eq!(
            decode_free_size_sprite(&[1, 0, 1, 0]),
            Err(FreeSizeSpriteError::TruncatedPixels {
                needed: 8,
                actual: 4,
            })
        );
        assert_eq!(
            decode_free_size_sprite(&[1, 0, 1, 0, 0, 0, 0, 0, 1]),
            Err(FreeSizeSpriteError::TrailingData { trailing: 1 })
        );

        let too_wide = RgbaImage::new(FREE_SIZE_SPRITE_MAX_DIMENSION + 1, 1);
        assert!(matches!(
            write_free_size_sprite_bytes(&too_wide),
            Err(FreeSizeSpriteError::DimensionsTooLarge { .. })
        ));
    }
}
