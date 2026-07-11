//! Handlers for `taletool cell-flag` payload commands.

use std::collections::BTreeMap;
use std::fs;

use anyhow::Context;
use image::{ImageFormat, Rgb, RgbImage};
use taletool_map::{MapCellGrid, decode_map_cell_grid};

use crate::cli::{CellFlagArg, CellFlagCommand};

/// Dispatch a `map` subcommand.
pub(crate) fn run_cell_flag(command: CellFlagCommand) -> anyhow::Result<()> {
    match command {
        CellFlagCommand::ExportPng { payload, out, flag } => {
            let data = fs::read(&payload).with_context(|| {
                format!("failed to read map cell payload {}", payload.display())
            })?;
            let grid = decode_map_cell_grid(&data).with_context(|| {
                format!("failed to decode map cell payload {}", payload.display())
            })?;
            let (image, legend) = render_grid(&grid, flag);

            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create PNG output directory {}", parent.display())
                })?;
            }
            image
                .save_with_format(&out, ImageFormat::Png)
                .with_context(|| format!("failed to write map cell PNG {}", out.display()))?;

            println!("exported {}", out.display());
            for entry in legend {
                println!(
                    "0x{:02X} #{:02X}{:02X}{:02X} {}",
                    entry.value, entry.color[0], entry.color[1], entry.color[2], entry.count
                );
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LegendEntry {
    value: u8,
    color: Rgb<u8>,
    count: usize,
}

fn render_grid(grid: &MapCellGrid, filter: Option<CellFlagArg>) -> (RgbImage, Vec<LegendEntry>) {
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
fn color_for_value(value: u8) -> Rgb<u8> {
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    use taletool_map::MapCellFlags;

    use super::*;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn grid(width: u16, height: u16, values: &[u8]) -> MapCellGrid {
        MapCellGrid::new(
            width,
            height,
            values
                .iter()
                .copied()
                .map(MapCellFlags::from_bits_retain)
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn renders_exact_values_with_stable_distinct_colors_and_legend() {
        let grid = grid(2, 2, &[0x00, 0x01, 0x01, 0xff]);
        let (image, legend) = render_grid(&grid, None);

        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(*image.get_pixel(0, 0), Rgb([255, 255, 255]));
        assert_ne!(image.get_pixel(1, 0), image.get_pixel(1, 1));
        assert_eq!(
            legend,
            vec![
                LegendEntry {
                    value: 0,
                    color: color_for_value(0),
                    count: 1,
                },
                LegendEntry {
                    value: 1,
                    color: color_for_value(1),
                    count: 2,
                },
                LegendEntry {
                    value: 0xff,
                    color: color_for_value(0xff),
                    count: 1,
                },
            ]
        );
    }

    #[test]
    fn palette_is_bijective() {
        let colors = (u8::MIN..=u8::MAX)
            .map(color_for_value)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(colors.len(), 256);
    }

    #[test]
    fn filters_combined_values_by_named_and_numeric_flags() {
        let grid = grid(3, 1, &[0x00, 0x04, 0x05]);
        for filter in ["unknown-04", "0x04", "4"] {
            let flag = CellFlagArg::from_str(filter).unwrap();
            let (image, legend) = render_grid(&grid, Some(flag));
            assert_eq!(*image.get_pixel(0, 0), Rgb([255, 255, 255]));
            assert_eq!(*image.get_pixel(1, 0), Rgb([0, 0, 0]));
            assert_eq!(*image.get_pixel(2, 0), Rgb([0, 0, 0]));
            assert!(legend.is_empty());
        }
    }

    #[test]
    fn accepts_unnamed_single_bits_and_rejects_invalid_masks() {
        assert_eq!(CellFlagArg::from_str("0x20").unwrap().bits(), 0x20);
        for invalid in ["0", "0x03", "256", "wat"] {
            assert!(CellFlagArg::from_str(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn writes_decodable_png() {
        let grid = grid(2, 1, &[0, 1]);
        let (image, _) = render_grid(&grid, None);
        let path = temporary_path("writes-decodable.png");
        image.save_with_format(&path, ImageFormat::Png).unwrap();

        let decoded = image::open(&path).unwrap().to_rgb8();
        assert_eq!(decoded, image);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_payload_and_unwritable_output_fail() {
        assert!(decode_map_cell_grid(&[1, 0, 1]).is_err());

        let grid = grid(1, 1, &[0]);
        let (image, _) = render_grid(&grid, None);
        let directory = temporary_path("output-directory");
        fs::create_dir_all(&directory).unwrap();
        assert!(
            image
                .save_with_format(&directory, ImageFormat::Png)
                .is_err()
        );
        fs::remove_dir(directory).unwrap();
    }

    fn temporary_path(name: &str) -> std::path::PathBuf {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "taletool-map-{}-{sequence}-{name}",
            std::process::id()
        ))
    }
}
