//! NosTale raster asset decoding and encoding.
//!
//! Texture records from `NStpData`, `NStpeData`, and `NStpuData` contain an
//! eight-byte header followed by a tightly packed mip chain. The client uses
//! the same payload format for all three archive families. The crate also
//! supports [`sprite`] payloads and shares their low-level pixel codecs.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use taletool_core::ByteReader;
use thiserror::Error;

pub mod sprite;

const TEXTURE_HEADER_LEN: usize = 8;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TextureError {
    #[error("texture payload is too short: {len} bytes")]
    TooShort { len: usize },
    #[error("unsupported texture format kind {format}")]
    UnsupportedFormat { format: u8 },
    #[error("texture dimensions must be non-zero, got {width}x{height}")]
    InvalidDimensions { width: u16, height: u16 },
    #[error("texture must be square, got {width}x{height}")]
    NonSquareDimensions { width: u16, height: u16 },
    #[error(
        "texture mip level {level} has a client byte count of {client_bytes}, but {width}x{height} {format} pixels need {expected_bytes} bytes"
    )]
    InvalidMipChain {
        level: usize,
        width: u16,
        height: u16,
        format: TextureFormat,
        client_bytes: usize,
        expected_bytes: usize,
    },
    #[error("texture pixel byte count overflow for {width}x{height} {format}")]
    PixelSizeOverflow {
        width: u16,
        height: u16,
        format: TextureFormat,
    },
    #[error("texture payload is truncated: need {needed} bytes, got {actual}")]
    Truncated { needed: usize, actual: usize },
    #[error("texture payload has trailing data: expected {expected} bytes, got {actual}")]
    TrailingData { expected: usize, actual: usize },
    #[error("texture header declares {expected} effective mip levels, but {actual} were supplied")]
    MipLevelCountMismatch { expected: usize, actual: usize },
    #[error(
        "texture mip level {level} dimensions must be {expected_width}x{expected_height}, got {actual_width}x{actual_height}"
    )]
    MipDimensionsMismatch {
        level: usize,
        expected_width: u16,
        expected_height: u16,
        actual_width: u32,
        actual_height: u32,
    },
}

pub type Result<T> = std::result::Result<T, TextureError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureFormat {
    A4R4G4B4,
    A1R5G5B5,
    A8R8G8B8,
    L8,
    A8,
}

impl TextureFormat {
    pub const fn kind(self) -> u8 {
        match self {
            Self::A4R4G4B4 => 0,
            Self::A1R5G5B5 => 1,
            Self::A8R8G8B8 => 2,
            Self::L8 => 3,
            Self::A8 => 4,
        }
    }

    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::A4R4G4B4 | Self::A1R5G5B5 => 2,
            Self::A8R8G8B8 => 4,
            Self::L8 | Self::A8 => 1,
        }
    }
}

impl std::fmt::Display for TextureFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::A4R4G4B4 => "A4R4G4B4",
            Self::A1R5G5B5 => "A1R5G5B5",
            Self::A8R8G8B8 => "A8R8G8B8",
            Self::L8 => "L8",
            Self::A8 => "A8",
        };
        f.write_str(name)
    }
}

impl TryFrom<u8> for TextureFormat {
    type Error = TextureError;

    fn try_from(kind: u8) -> Result<Self> {
        match kind {
            0 => Ok(Self::A4R4G4B4),
            1 => Ok(Self::A1R5G5B5),
            2 => Ok(Self::A8R8G8B8),
            3 => Ok(Self::L8),
            4 => Ok(Self::A8),
            format => Err(TextureError::UnsupportedFormat { format }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureHeader {
    pub width: u16,
    pub height: u16,
    pub format: TextureFormat,
    /// `0` selects point filtering; every non-zero value selects linear filtering.
    pub filter_flag: u8,
    /// Opaque header byte that the client retains but does not interpret.
    pub unknown_06: u8,
    /// Stored level count. The client treats zero as a one-level texture.
    pub mip_level_count: u8,
}

impl TextureHeader {
    pub fn effective_mip_level_count(self) -> usize {
        usize::from(self.mip_level_count.max(1))
    }

    pub fn mip_dimensions(self, level: usize) -> (u16, u16) {
        (
            mip_dimension(self.width, level),
            mip_dimension(self.height, level),
        )
    }

    pub fn uses_linear_filtering(self) -> bool {
        self.filter_flag != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTexture {
    pub header: TextureHeader,
    pub mip_levels: Vec<RgbaImage>,
}

impl DecodedTexture {
    pub fn image(&self) -> &RgbaImage {
        &self.mip_levels[0]
    }
}

/// Decode a texture payload, requiring the complete payload to be consumed.
pub fn decode_texture(data: &[u8]) -> Result<DecodedTexture> {
    if data.len() < TEXTURE_HEADER_LEN {
        return Err(TextureError::TooShort { len: data.len() });
    }

    let mut reader = ByteReader::new(data);
    let width = reader
        .read_u16_le("texture.width")
        .expect("texture header length was checked");
    let height = reader
        .read_u16_le("texture.height")
        .expect("texture header length was checked");
    let format = TextureFormat::try_from(
        reader
            .read_u8("texture.format")
            .expect("texture header length was checked"),
    )?;
    let header = TextureHeader {
        width,
        height,
        format,
        filter_flag: reader
            .read_u8("texture.filter_flag")
            .expect("texture header length was checked"),
        unknown_06: reader
            .read_u8("texture.unknown_06")
            .expect("texture header length was checked"),
        mip_level_count: reader
            .read_u8("texture.mip_level_count")
            .expect("texture header length was checked"),
    };

    let layouts = mip_layouts(header)?;
    let expected_len = layouts
        .last()
        .map_or(TEXTURE_HEADER_LEN, |layout| layout.end);
    if data.len() < expected_len {
        return Err(TextureError::Truncated {
            needed: expected_len,
            actual: data.len(),
        });
    }
    if data.len() > expected_len {
        return Err(TextureError::TrailingData {
            expected: expected_len,
            actual: data.len(),
        });
    }

    let mip_levels = layouts
        .into_iter()
        .map(|layout| {
            decode_texture_level(
                header.format,
                layout.width,
                layout.height,
                &data[layout.offset..layout.end],
            )
        })
        .collect();

    Ok(DecodedTexture { header, mip_levels })
}

/// Encode a header and exact RGBA mip chain into a canonical texture payload.
pub fn write_texture_bytes(header: &TextureHeader, mip_levels: &[RgbaImage]) -> Result<Vec<u8>> {
    let layouts = mip_layouts(*header)?;
    if mip_levels.len() != layouts.len() {
        return Err(TextureError::MipLevelCountMismatch {
            expected: layouts.len(),
            actual: mip_levels.len(),
        });
    }

    for (level, (image, layout)) in mip_levels.iter().zip(&layouts).enumerate() {
        if image.dimensions() != (u32::from(layout.width), u32::from(layout.height)) {
            return Err(TextureError::MipDimensionsMismatch {
                level,
                expected_width: layout.width,
                expected_height: layout.height,
                actual_width: image.width(),
                actual_height: image.height(),
            });
        }
    }

    let capacity = layouts
        .last()
        .map_or(TEXTURE_HEADER_LEN, |layout| layout.end);
    let mut output = Vec::with_capacity(capacity);
    output.extend_from_slice(&header.width.to_le_bytes());
    output.extend_from_slice(&header.height.to_le_bytes());
    output.push(header.format.kind());
    output.push(header.filter_flag);
    output.push(header.unknown_06);
    output.push(header.mip_level_count);
    for image in mip_levels {
        encode_texture_level(header.format, image, &mut output);
    }
    debug_assert_eq!(output.len(), capacity);
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
struct MipLayout {
    width: u16,
    height: u16,
    offset: usize,
    end: usize,
}

fn mip_layouts(header: TextureHeader) -> Result<Vec<MipLayout>> {
    validate_dimensions(header.width, header.height)?;
    let level_count = header.effective_mip_level_count();
    let base_bytes = pixel_byte_count(header.width, header.height, header.format)?;
    let mut client_bytes = base_bytes;
    let mut offset = TEXTURE_HEADER_LEN;
    let mut layouts = Vec::with_capacity(level_count);

    for level in 0..level_count {
        let (width, height) = header.mip_dimensions(level);
        let expected_bytes = pixel_byte_count(width, height, header.format)?;
        if client_bytes != expected_bytes {
            return Err(TextureError::InvalidMipChain {
                level,
                width,
                height,
                format: header.format,
                client_bytes,
                expected_bytes,
            });
        }
        let end = offset
            .checked_add(expected_bytes)
            .ok_or(TextureError::PixelSizeOverflow {
                width,
                height,
                format: header.format,
            })?;
        layouts.push(MipLayout {
            width,
            height,
            offset,
            end,
        });
        offset = end;
        client_bytes >>= 2;
    }
    Ok(layouts)
}

fn validate_dimensions(width: u16, height: u16) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(TextureError::InvalidDimensions { width, height });
    }
    if width != height {
        return Err(TextureError::NonSquareDimensions { width, height });
    }
    Ok(())
}

fn pixel_byte_count(width: u16, height: u16, format: TextureFormat) -> Result<usize> {
    usize::from(width)
        .checked_mul(usize::from(height))
        .and_then(|pixels| pixels.checked_mul(format.bytes_per_pixel()))
        .ok_or(TextureError::PixelSizeOverflow {
            width,
            height,
            format,
        })
}

fn decode_texture_level(
    format: TextureFormat,
    width: u16,
    height: u16,
    pixels: &[u8],
) -> RgbaImage {
    let mut image = RgbaImage::new(width.into(), height.into());
    match format {
        TextureFormat::A4R4G4B4 => {
            for (pixel, encoded) in image.pixels_mut().zip(pixels.chunks_exact(2)) {
                *pixel = decode_a4r4g4b4(u16::from_le_bytes([encoded[0], encoded[1]]));
            }
        }
        TextureFormat::A1R5G5B5 => {
            for (pixel, encoded) in image.pixels_mut().zip(pixels.chunks_exact(2)) {
                *pixel = decode_a1r5g5b5(u16::from_le_bytes([encoded[0], encoded[1]]));
            }
        }
        TextureFormat::A8R8G8B8 => {
            for (pixel, encoded) in image.pixels_mut().zip(pixels.chunks_exact(4)) {
                *pixel = Rgba([encoded[2], encoded[1], encoded[0], encoded[3]]);
            }
        }
        TextureFormat::L8 => {
            for (pixel, value) in image.pixels_mut().zip(pixels) {
                *pixel = Rgba([*value, *value, *value, 255]);
            }
        }
        TextureFormat::A8 => {
            for (pixel, alpha) in image.pixels_mut().zip(pixels) {
                *pixel = Rgba([0, 0, 0, *alpha]);
            }
        }
    }
    image
}

fn encode_texture_level(format: TextureFormat, image: &RgbaImage, output: &mut Vec<u8>) {
    for pixel in image.pixels() {
        let [red, green, blue, alpha] = pixel.0;
        match format {
            TextureFormat::A4R4G4B4 => output.extend_from_slice(
                &((quantize_4(alpha) << 12)
                    | (quantize_4(red) << 8)
                    | (quantize_4(green) << 4)
                    | quantize_4(blue))
                .to_le_bytes(),
            ),
            TextureFormat::A1R5G5B5 => output.extend_from_slice(
                &((u16::from(alpha >= 128) << 15)
                    | (quantize_5(red) << 10)
                    | (quantize_5(green) << 5)
                    | quantize_5(blue))
                .to_le_bytes(),
            ),
            TextureFormat::A8R8G8B8 => output.extend_from_slice(&[blue, green, red, alpha]),
            TextureFormat::L8 => output.push(luminance(red, green, blue)),
            TextureFormat::A8 => output.push(alpha),
        }
    }
}

fn mip_dimension(base: u16, level: usize) -> u16 {
    base.checked_shr(level as u32).unwrap_or(0).max(1)
}

pub(crate) fn decode_a4r4g4b4(value: u16) -> Rgba<u8> {
    Rgba([
        expand_4(((value >> 8) & 0x0f) as u8),
        expand_4(((value >> 4) & 0x0f) as u8),
        expand_4((value & 0x0f) as u8),
        expand_4((value >> 12) as u8),
    ])
}

pub(crate) fn encode_a4r4g4b4(pixel: Rgba<u8>) -> u16 {
    let [r, g, b, a] = pixel.0;
    (quantize_4(a) << 12) | (quantize_4(r) << 8) | (quantize_4(g) << 4) | quantize_4(b)
}

pub(crate) fn decode_a8r8g8b8(encoded: [u8; 4]) -> Rgba<u8> {
    let [blue, green, red, alpha] = encoded;
    Rgba([red, green, blue, alpha])
}

pub(crate) fn encode_a8r8g8b8(pixel: Rgba<u8>) -> [u8; 4] {
    let [red, green, blue, alpha] = pixel.0;
    [blue, green, red, alpha]
}

fn decode_a1r5g5b5(value: u16) -> Rgba<u8> {
    Rgba([
        expand_5(((value >> 10) & 0x1f) as u8),
        expand_5(((value >> 5) & 0x1f) as u8),
        expand_5((value & 0x1f) as u8),
        if value & 0x8000 != 0 { 255 } else { 0 },
    ])
}

fn expand_4(value: u8) -> u8 {
    (value << 4) | value
}

fn quantize_4(value: u8) -> u16 {
    (u16::from(value) * 15 + 127) / 255
}

fn expand_5(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

fn quantize_5(value: u8) -> u16 {
    (u16::from(value) * 31 + 127) / 255
}

fn luminance(red: u8, green: u8, blue: u8) -> u8 {
    ((u32::from(red) * 77 + u32::from(green) * 150 + u32::from(blue) * 29 + 128) >> 8) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(format: TextureFormat, pixels: &[u8]) -> Vec<u8> {
        let mut data = vec![1, 0, 1, 0, format.kind(), 2, 7, 0];
        data.extend_from_slice(pixels);
        data
    }

    #[test]
    fn every_format_decodes_and_rebuilds_byte_for_byte() {
        let cases = [
            (TextureFormat::A4R4G4B4, vec![0x34, 0x12]),
            (TextureFormat::A1R5G5B5, vec![0x41, 0x8c]),
            (TextureFormat::A8R8G8B8, vec![10, 20, 30, 40]),
            (TextureFormat::L8, vec![77]),
            (TextureFormat::A8, vec![88]),
        ];

        for (format, pixels) in cases {
            let data = payload(format, &pixels);
            let texture = decode_texture(&data).unwrap();
            assert_eq!(texture.header.format, format);
            assert_eq!(texture.header.filter_flag, 2);
            assert_eq!(texture.header.unknown_06, 7);
            assert_eq!(
                write_texture_bytes(&texture.header, &texture.mip_levels).unwrap(),
                data
            );
        }
    }

    #[test]
    fn decodes_client_channel_semantics() {
        let argb4444 = decode_texture(&payload(TextureFormat::A4R4G4B4, &[0x34, 0x12])).unwrap();
        assert_eq!(argb4444.image().get_pixel(0, 0).0, [34, 51, 68, 17]);

        let bgra = decode_texture(&payload(TextureFormat::A8R8G8B8, &[10, 20, 30, 40])).unwrap();
        assert_eq!(bgra.image().get_pixel(0, 0).0, [30, 20, 10, 40]);

        let alpha = decode_texture(&payload(TextureFormat::A8, &[88])).unwrap();
        assert_eq!(alpha.image().get_pixel(0, 0).0, [0, 0, 0, 88]);
    }

    #[test]
    fn decodes_and_rebuilds_mip_chain() {
        let mut data = vec![4, 0, 4, 0, TextureFormat::L8.kind(), 0, 9, 3];
        data.extend(0_u8..16);
        data.extend([20, 21, 22, 23]);
        data.push(30);

        let texture = decode_texture(&data).unwrap();
        assert_eq!(texture.mip_levels.len(), 3);
        assert_eq!(texture.mip_levels[0].dimensions(), (4, 4));
        assert_eq!(texture.mip_levels[1].dimensions(), (2, 2));
        assert_eq!(texture.mip_levels[2].dimensions(), (1, 1));
        assert_eq!(
            write_texture_bytes(&texture.header, &texture.mip_levels).unwrap(),
            data
        );
    }

    #[test]
    fn stored_zero_and_one_both_mean_one_level() {
        for stored_count in [0, 1] {
            let mut data = payload(TextureFormat::L8, &[42]);
            data[7] = stored_count;
            let texture = decode_texture(&data).unwrap();
            assert_eq!(texture.header.effective_mip_level_count(), 1);
            assert_eq!(texture.mip_levels.len(), 1);
            assert_eq!(
                write_texture_bytes(&texture.header, &texture.mip_levels).unwrap(),
                data
            );
        }
    }

    #[test]
    fn writer_quantizes_and_converts_pixels() {
        let image = RgbaImage::from_pixel(1, 1, Rgba([8, 25, 42, 127]));

        let header = |format| TextureHeader {
            width: 1,
            height: 1,
            format,
            filter_flag: 0,
            unknown_06: 0,
            mip_level_count: 0,
        };
        assert_eq!(
            &write_texture_bytes(
                &header(TextureFormat::A4R4G4B4),
                std::slice::from_ref(&image)
            )
            .unwrap()[8..],
            &0x7012_u16.to_le_bytes()
        );
        assert_eq!(
            &write_texture_bytes(
                &header(TextureFormat::A1R5G5B5),
                std::slice::from_ref(&image)
            )
            .unwrap()[8..],
            &0x0465_u16.to_le_bytes()
        );
        assert_eq!(
            write_texture_bytes(&header(TextureFormat::L8), std::slice::from_ref(&image)).unwrap()
                [8],
            22
        );
        assert_eq!(
            write_texture_bytes(&header(TextureFormat::A8), &[image]).unwrap()[8],
            127
        );
    }

    #[test]
    fn rejects_invalid_headers_and_payload_lengths() {
        assert_eq!(decode_texture(&[]), Err(TextureError::TooShort { len: 0 }));

        let mut invalid = payload(TextureFormat::L8, &[1]);
        invalid[4] = 5;
        assert_eq!(
            decode_texture(&invalid),
            Err(TextureError::UnsupportedFormat { format: 5 })
        );

        let mut rectangular = payload(TextureFormat::L8, &[1, 2]);
        rectangular[0] = 2;
        assert!(matches!(
            decode_texture(&rectangular),
            Err(TextureError::NonSquareDimensions { .. })
        ));

        let mut zero = payload(TextureFormat::L8, &[]);
        zero[0] = 0;
        zero[1] = 0;
        assert!(matches!(
            decode_texture(&zero),
            Err(TextureError::InvalidDimensions { .. })
        ));

        let truncated = payload(TextureFormat::A8R8G8B8, &[1, 2, 3]);
        assert!(matches!(
            decode_texture(&truncated),
            Err(TextureError::Truncated { .. })
        ));

        let trailing = payload(TextureFormat::L8, &[1, 2]);
        assert!(matches!(
            decode_texture(&trailing),
            Err(TextureError::TrailingData { .. })
        ));
    }

    #[test]
    fn rejects_impossible_or_mismatched_mip_chains() {
        let mut excessive = payload(TextureFormat::L8, &[1]);
        excessive[7] = 2;
        assert!(matches!(
            decode_texture(&excessive),
            Err(TextureError::InvalidMipChain { level: 1, .. })
        ));

        let header = TextureHeader {
            width: 2,
            height: 2,
            format: TextureFormat::L8,
            filter_flag: 0,
            unknown_06: 0,
            mip_level_count: 2,
        };
        assert!(matches!(
            write_texture_bytes(&header, &[RgbaImage::new(2, 2)]),
            Err(TextureError::MipLevelCountMismatch { .. })
        ));
        assert!(matches!(
            write_texture_bytes(&header, &[RgbaImage::new(2, 2), RgbaImage::new(2, 2)]),
            Err(TextureError::MipDimensionsMismatch { level: 1, .. })
        ));
    }
}
