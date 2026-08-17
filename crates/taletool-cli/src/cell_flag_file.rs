//! PNG output helpers for map cell-flag payloads.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Context;
use image::{ImageFormat, Rgb, RgbImage};
use taletool_map::MapCellGrid;

use crate::cli::CellFlagArg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegendEntry {
    pub(crate) value: u8,
    pub(crate) color: Rgb<u8>,
    pub(crate) count: usize,
}

/// Render and write one map cell grid as a PNG.
pub(crate) fn unpack_map_cell_grid_png(
    grid: &MapCellGrid,
    out: &Path,
    filter: Option<CellFlagArg>,
) -> anyhow::Result<Vec<LegendEntry>> {
    let (image, legend) = render_grid(grid, filter);
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create PNG output directory {}", parent.display())
        })?;
    }
    image
        .save_with_format(out, ImageFormat::Png)
        .with_context(|| format!("failed to write map cell PNG {}", out.display()))?;
    Ok(legend)
}

pub(crate) fn render_grid(
    grid: &MapCellGrid,
    filter: Option<CellFlagArg>,
) -> (RgbImage, Vec<LegendEntry>) {
    let mut image = RgbImage::new(u32::from(grid.width()), u32::from(grid.height()));
    let mut counts = BTreeMap::<u8, usize>::new();

    for (index, flags) in grid.cells().iter().enumerate() {
        let value = flags.bits();
        let color = match filter {
            Some(flag) if value & flag.bits() != 0 => Rgb([0, 0, 0]),
            Some(_) => Rgb([255, 255, 255]),
            None => {
                *counts.entry(value).or_default() += 1;
                color_for_value(value)
            }
        };
        let x = (index % usize::from(grid.width())) as u32;
        let y = (index / usize::from(grid.width())) as u32;
        image.put_pixel(x, y, color);
    }

    let legend = counts
        .into_iter()
        .map(|(value, count)| LegendEntry {
            value,
            color: color_for_value(value),
            count,
        })
        .collect();
    (image, legend)
}

/// Map every byte bijectively to RGB332, swapping zero with the RGB332 white
/// entry so empty cells are white without creating a palette collision.
pub(crate) fn color_for_value(value: u8) -> Rgb<u8> {
    let palette_index = match value {
        0 => u8::MAX,
        u8::MAX => 0,
        value => value,
    };
    let red = u8::try_from(u16::from(palette_index & 0x07) * 255 / 7).unwrap();
    let green = u8::try_from(u16::from((palette_index >> 3) & 0x07) * 255 / 7).unwrap();
    let blue = u8::try_from(u16::from((palette_index >> 6) & 0x03) * 255 / 3).unwrap();
    Rgb([red, green, blue])
}
