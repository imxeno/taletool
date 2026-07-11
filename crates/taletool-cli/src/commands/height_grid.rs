//! Handlers for `taletool height-grid` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::json;
use taletool_map::{HeightGrid, HeightGridEncoding, decode_height_grid};

use crate::cli::HeightGridCommand;
use crate::height_grid_file::{pack_height_grid_file, unpack_height_grid_file};
use crate::util::fnv1a64;

/// Dispatch a `height-grid` subcommand.
pub(crate) fn run_height_grid(command: HeightGridCommand) -> anyhow::Result<()> {
    match command {
        HeightGridCommand::Inspect {
            input,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let grid = decode_height_grid(&data)
                .with_context(|| format!("decoding height grid {}", input.display()))?;
            inspect_height_grid(
                &grid,
                data.len(),
                checksum.then(|| fnv1a64(&data)),
                json_output,
            )
        }
        HeightGridCommand::Unpack { input, out } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let grid = decode_height_grid(&data)
                .with_context(|| format!("decoding height grid {}", input.display()))?;
            unpack_height_grid_file(&grid, &out)?;
            println!(
                "unpacked {} vertices, {} triangles, and {} cells into {}",
                grid.vertex_count(),
                grid.triangle_count(),
                grid.cell_count(),
                out.display()
            );
            Ok(())
        }
        HeightGridCommand::Pack { input, out } => {
            let grid = pack_height_grid_file(&input, &out)?;
            println!(
                "packed {} vertices, {} triangles, and {} cells into {}",
                grid.vertex_count(),
                grid.triangle_count(),
                grid.cell_count(),
                out.display()
            );
            Ok(())
        }
    }
}

fn inspect_height_grid(
    grid: &HeightGrid,
    encoded_size: usize,
    checksum: Option<u64>,
    json_output: bool,
) -> anyhow::Result<()> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "height-grid",
                "encoded_size": encoded_size,
                "encoding": encoding_label(grid.encoding),
                "grid_id": grid.grid_id,
                "map_id": grid.map_id,
                "bounds": grid.bounds,
                "dimensions": grid.dimensions,
                "cell_size": grid.cell_size,
                "vertex_count": grid.vertex_count(),
                "triangle_count": grid.triangle_count(),
                "cell_count": grid.cell_count(),
                "non_empty_cell_count": grid.non_empty_cell_count(),
                "triangle_reference_count": grid.triangle_reference_count(),
                "checksum_fnv1a64": checksum.map(|value| format!("{value:016x}")),
            }))?
        );
    } else {
        println!("type: height-grid");
        println!("encoded_size: {encoded_size}");
        println!("encoding: {}", encoding_label(grid.encoding));
        println!("grid_id: {}", grid.grid_id);
        println!("map_id: {}", grid.map_id);
        println!(
            "bounds: minimum={:?} maximum={:?}",
            grid.bounds.minimum, grid.bounds.maximum
        );
        println!(
            "dimensions: {}x{}",
            grid.dimensions.width, grid.dimensions.depth
        );
        println!("cell_size: {:?}", grid.cell_size);
        println!("vertices: {}", grid.vertex_count());
        println!("triangles: {}", grid.triangle_count());
        println!(
            "cells: {} ({} non-empty, {} triangle references)",
            grid.cell_count(),
            grid.non_empty_cell_count(),
            grid.triangle_reference_count()
        );
        if let Some(checksum) = checksum {
            println!("checksum_fnv1a64: {checksum:016x}");
        }
    }
    Ok(())
}

fn encoding_label(encoding: HeightGridEncoding) -> &'static str {
    match encoding {
        HeightGridEncoding::ImplicitVersion1 => "implicit-version-1",
        HeightGridEncoding::Version1 => "version-1",
        HeightGridEncoding::Version2 => "version-2",
    }
}
