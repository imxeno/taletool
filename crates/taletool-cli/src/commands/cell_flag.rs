//! Handlers for `taletool cell-flag` payload commands.

use std::fs;

use anyhow::Context;
#[cfg(test)]
use taletool_map::MapCellGrid;
use taletool_map::decode_map_cell_grid;

use crate::cell_flag_file::unpack_map_cell_grid_png;
#[cfg(test)]
use crate::cell_flag_file::{LegendEntry, color_for_value, render_grid};
#[cfg(test)]
use crate::cli::CellFlagArg;
use crate::cli::CellFlagCommand;

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
            let legend = unpack_map_cell_grid_png(&grid, &out, flag)?;

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

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    use image::{ImageFormat, Rgb};
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
