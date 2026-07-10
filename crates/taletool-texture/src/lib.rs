//! NosTale raster asset decoding.
//!
//! The crate supports header-based texture payloads and [`sprite`] payloads 
//! while sharing their low-level pixel codecs internally.

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use taletool_core::ByteReader;
use thiserror::Error;

pub mod sprite;

#[derive(Debug, Error)]
pub enum TextureError {
    #[error("texture payload is too short: {len} bytes")]
    TooShort { len: usize },
    #[error("unsupported texture format kind {format}")]
    UnsupportedFormat { format: u8 },
    #[error("texture dimensions must be non-zero, got {width}x{height}")]
    InvalidDimensions { width: u16, height: u16 },
    #[error("texture payload is truncated: need {needed} bytes, got {actual}")]
    Truncated { needed: usize, actual: usize },
}

pub type Result<T> = std::result::Result<T, TextureError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureFormat {
    A4R4G4B4,
    A1R5G5B5,
    A8R8G8B8,
    L8,
    A8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureHeader {
    pub width: u16,
    pub height: u16,
    pub format_kind: u8,
    pub unknown_05: u8,
    pub unknown_06: u8,
    pub mip_level_count: u8,
}

impl TextureHeader {
    pub fn format(&self) -> Result<TextureFormat> {
        match self.format_kind {
            0 => Ok(TextureFormat::A4R4G4B4),
            1 => Ok(TextureFormat::A1R5G5B5),
            2 => Ok(TextureFormat::A8R8G8B8),
            3 => Ok(TextureFormat::L8),
            4 => Ok(TextureFormat::A8),
            format => Err(TextureError::UnsupportedFormat { format }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedTexture {
    pub header: TextureHeader,
    pub image: RgbaImage,
    pub mip_levels: Vec<RgbaImage>,
}

pub fn decode_texture(data: &[u8]) -> Result<DecodedTexture> {
    if data.len() < 8 {
        return Err(TextureError::TooShort { len: data.len() });
    }

    let mut reader = ByteReader::new(data);
    let header = TextureHeader {
        width: reader
            .read_u16_le("texture.width")
            .expect("texture header length was checked"),
        height: reader
            .read_u16_le("texture.height")
            .expect("texture header length was checked"),
        format_kind: reader
            .read_u8("texture.format_kind")
            .expect("texture header length was checked"),
        unknown_05: reader
            .read_u8("texture.unknown_05")
            .expect("texture header length was checked"),
        unknown_06: reader
            .read_u8("texture.unknown_06")
            .expect("texture header length was checked"),
        mip_level_count: {
            reader
                .read_u8("texture.mip_level_count")
                .expect("texture header length was checked")
        },
    };

    if header.width == 0 || header.height == 0 {
        return Err(TextureError::InvalidDimensions {
            width: header.width,
            height: header.height,
        });
    }

    let format = header.format()?;
    let bytes_per_pixel = match format {
        TextureFormat::A4R4G4B4 | TextureFormat::A1R5G5B5 => 2,
        TextureFormat::A8R8G8B8 => 4,
        TextureFormat::L8 | TextureFormat::A8 => 1,
    };
    let level_count = usize::from(header.mip_level_count.max(1));
    let mut offset = 8;
    let mut mip_levels = Vec::with_capacity(level_count);

    for level in 0..level_count {
        let width = mip_dimension(header.width, level);
        let height = mip_dimension(header.height, level);
        let pixel_count = width as usize * height as usize;
        let level_bytes = pixel_count * bytes_per_pixel;
        let needed = offset + level_bytes;
        if data.len() < needed {
            return Err(TextureError::Truncated {
                needed,
                actual: data.len(),
            });
        }

        let pixels = &data[offset..needed];
        mip_levels.push(decode_texture_level(format, width, height, pixels));
        offset = needed;
    }

    let image = mip_levels[0].clone();
    Ok(DecodedTexture {
        header,
        image,
        mip_levels,
    })
}

fn decode_texture_level(
    format: TextureFormat,
    width: u16,
    height: u16,
    pixels: &[u8],
) -> RgbaImage {
    let pixel_count = width as usize * height as usize;
    let mut image = RgbaImage::new(width as u32, height as u32);

    for index in 0..pixel_count {
        let rgba = match format {
            TextureFormat::A4R4G4B4 => decode_a4r4g4b4(u16::from_le_bytes([
                pixels[index * 2],
                pixels[index * 2 + 1],
            ])),
            TextureFormat::A1R5G5B5 => decode_a1r5g5b5(u16::from_le_bytes([
                pixels[index * 2],
                pixels[index * 2 + 1],
            ])),
            TextureFormat::A8R8G8B8 => {
                let offset = index * 4;
                Rgba([
                    pixels[offset + 2],
                    pixels[offset + 1],
                    pixels[offset],
                    pixels[offset + 3],
                ])
            }
            TextureFormat::L8 => {
                let value = pixels[index];
                Rgba([value, value, value, 255])
            }
            TextureFormat::A8 => Rgba([255, 255, 255, pixels[index]]),
        };
        let x = (index % width as usize) as u32;
        let y = (index / width as usize) as u32;
        image.put_pixel(x, y, rgba);
    }

    image
}

fn mip_dimension(base: u16, level: usize) -> u16 {
    (base >> level).max(1)
}

pub(crate) fn decode_a4r4g4b4(value: u16) -> Rgba<u8> {
    let a = expand_4((value >> 12) as u8);
    let r = expand_4(((value >> 8) & 0x0f) as u8);
    let g = expand_4(((value >> 4) & 0x0f) as u8);
    let b = expand_4((value & 0x0f) as u8);
    Rgba([r, g, b, a])
}

pub(crate) fn encode_a4r4g4b4(pixel: Rgba<u8>) -> u16 {
    let [r, g, b, a] = pixel.0;
    (quantize_4(a) << 12) | (quantize_4(r) << 8) | (quantize_4(g) << 4) | quantize_4(b)
}

fn decode_a1r5g5b5(value: u16) -> Rgba<u8> {
    let a = if value & 0x8000 != 0 { 255 } else { 0 };
    let r = expand_5(((value >> 10) & 0x1f) as u8);
    let g = expand_5(((value >> 5) & 0x1f) as u8);
    let b = expand_5((value & 0x1f) as u8);
    Rgba([r, g, b, a])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a8r8g8b8_pixel() {
        let data = [1, 0, 1, 0, 2, 0, 0, 0, 10, 20, 30, 40];
        let texture = decode_texture(&data).unwrap();
        assert_eq!(texture.image.get_pixel(0, 0).0, [30, 20, 10, 40]);
        assert_eq!(texture.mip_levels.len(), 1);
    }

    #[test]
    fn decodes_a8r8g8b8_mip_chain() {
        let data = [
            2, 0, 2, 0, 2, 0, 0, 2, //
            10, 20, 30, 40, 11, 21, 31, 41, //
            12, 22, 32, 42, 13, 23, 33, 43, //
            14, 24, 34, 44,
        ];
        let texture = decode_texture(&data).unwrap();

        assert_eq!(texture.mip_levels.len(), 2);
        assert_eq!(texture.mip_levels[0].dimensions(), (2, 2));
        assert_eq!(texture.mip_levels[1].dimensions(), (1, 1));
        assert_eq!(texture.image.get_pixel(1, 1).0, [33, 23, 13, 43]);
        assert_eq!(texture.mip_levels[1].get_pixel(0, 0).0, [34, 24, 14, 44]);
    }
}
