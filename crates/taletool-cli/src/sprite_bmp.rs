//! Uncompressed, top-down V4 BMP output with explicit straight alpha.

use std::io::Write;

use anyhow::{Context, bail};
use image::Rgba;

const PIXEL_OFFSET: u32 = 14 + 108;

/// Write BGRA rows in their incoming order, with no full-image buffer or flip.
pub(crate) fn write_bmp(
    writer: &mut impl Write,
    width: u32,
    height: u32,
    mut pixels: impl Iterator<Item = Rgba<u8>>,
) -> anyhow::Result<()> {
    if width == 0 || height == 0 {
        bail!("BMP dimensions must be non-zero; use a sprite manifest for empty frames");
    }
    let signed_width = i32::try_from(width).context("BMP width exceeds its signed 32-bit field")?;
    let signed_height =
        i32::try_from(height).context("BMP height exceeds its signed 32-bit field")?;
    let image_size = width
        .checked_mul(height)
        .and_then(|size| size.checked_mul(4));
    let file_size = image_size.and_then(|size| size.checked_add(PIXEL_OFFSET));
    let (Some(image_size), Some(file_size)) = (image_size, file_size) else {
        bail!("{width}x{height} BMP exceeds its 32-bit file-size limit");
    };

    let mut header = [0_u8; PIXEL_OFFSET as usize];
    header[..2].copy_from_slice(b"BM");
    header[2..6].copy_from_slice(&file_size.to_le_bytes());
    header[10..14].copy_from_slice(&PIXEL_OFFSET.to_le_bytes());
    header[14..18].copy_from_slice(&108_u32.to_le_bytes());
    header[18..22].copy_from_slice(&signed_width.to_le_bytes());
    header[22..26].copy_from_slice(&(-signed_height).to_le_bytes());
    header[26..28].copy_from_slice(&1_u16.to_le_bytes());
    header[28..30].copy_from_slice(&32_u16.to_le_bytes());
    header[30..34].copy_from_slice(&3_u32.to_le_bytes()); // BI_BITFIELDS
    header[34..38].copy_from_slice(&image_size.to_le_bytes());
    header[54..58].copy_from_slice(&0x00ff_0000_u32.to_le_bytes());
    header[58..62].copy_from_slice(&0x0000_ff00_u32.to_le_bytes());
    header[62..66].copy_from_slice(&0x0000_00ff_u32.to_le_bytes());
    header[66..70].copy_from_slice(&0xff00_0000_u32.to_le_bytes());
    header[70..74].copy_from_slice(&0x7352_4742_u32.to_le_bytes()); // LCS_sRGB

    let row_size = usize::try_from(u64::from(width) * 4).context("BMP row size exceeds usize")?;
    let mut row = Vec::new();
    row.try_reserve_exact(row_size)
        .context("allocating BMP row")?;
    row.resize(row_size, 0);
    writer.write_all(&header)?;
    for _ in 0..height {
        for bgra in row.as_chunks_mut::<4>().0 {
            let Some(Rgba([red, green, blue, alpha])) = pixels.next() else {
                bail!("BMP pixel iterator ended before {width}x{height} pixels");
            };
            bgra.copy_from_slice(&[blue, green, red, alpha]);
        }
        writer.write_all(&row)?;
    }
    if pixels.next().is_some() {
        bail!("BMP pixel iterator contains more than {width}x{height} pixels");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use image::ImageFormat;
    use taletool_texture::sprite::parse_sprite;

    use super::*;

    #[test]
    fn writes_v4_masks_top_down_bgra_and_preserves_every_alpha_nibble() {
        let mut payload = vec![1, 2, 0, 8, 0, 0, 0, 0, 0, 13, 0, 0, 0];
        for alpha in 0..16_u16 {
            payload.extend_from_slice(&((alpha << 12) | 0x123).to_le_bytes());
        }
        let sprite = parse_sprite(&payload).unwrap();
        let mut bmp = Vec::new();
        write_bmp(&mut bmp, 2, 8, sprite.frames[0].pixels()).unwrap();
        let word = |offset| u32::from_le_bytes(bmp[offset..offset + 4].try_into().unwrap());
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(word(2) as usize, bmp.len());
        assert_eq!(word(10), 122);
        assert_eq!(word(14), 108);
        assert_eq!(word(18), 2);
        assert_eq!(word(22) as i32, -8);
        assert_eq!(&bmp[26..30], &[1, 0, 32, 0]);
        assert_eq!(word(30), 3);
        assert_eq!(word(34), 64);
        assert_eq!(word(54), 0x00ff_0000);
        assert_eq!(word(58), 0x0000_ff00);
        assert_eq!(word(62), 0x0000_00ff);
        assert_eq!(word(66), 0xff00_0000);
        assert_eq!(word(70), 0x7352_4742);
        for (alpha, pixel) in bmp[122..].as_chunks::<4>().0.iter().enumerate() {
            assert_eq!(pixel, &[0x33, 0x22, 0x11, alpha as u8 * 17]);
        }
        let decoded = image::load_from_memory_with_format(&bmp, ImageFormat::Bmp)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded, sprite.frames[0].decode().unwrap().frame.image);
    }

    #[test]
    fn rejects_unrepresentable_dimensions_before_writing() {
        for (width, height) in [(0, 1), (1, 0), (u32::MAX, 1), (65535, 65535)] {
            let mut output = Vec::new();
            assert!(write_bmp(&mut output, width, height, std::iter::empty()).is_err());
            assert!(output.is_empty());
        }
    }
}
