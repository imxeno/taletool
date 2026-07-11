//! Handlers for `taletool map-neighborhood` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::json;
use taletool_map_neighborhood::{MapNeighborhood, decode_map_neighborhood};

use crate::cli::MapNeighborhoodCommand;
use crate::map_neighborhood_file::{pack_map_neighborhood_file, unpack_map_neighborhood_file};
use crate::util::fnv1a64;

/// Dispatch a `map-neighborhood` subcommand.
pub(crate) fn run_map_neighborhood(command: MapNeighborhoodCommand) -> anyhow::Result<()> {
    match command {
        MapNeighborhoodCommand::Inspect {
            input,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let neighborhood = decode_map_neighborhood(&data)
                .with_context(|| format!("decoding map neighborhood {}", input.display()))?;
            inspect_map_neighborhood(
                &neighborhood,
                data.len(),
                checksum.then(|| fnv1a64(&data)),
                json_output,
            )
        }
        MapNeighborhoodCommand::Unpack { input, out } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let neighborhood = decode_map_neighborhood(&data)
                .with_context(|| format!("decoding map neighborhood {}", input.display()))?;
            unpack_map_neighborhood_file(&neighborhood, &out)?;
            println!(
                "unpacked {} neighbor references and {} point sequences into {}",
                neighborhood.neighbors.len(),
                neighborhood.point_sequences.len(),
                out.display()
            );
            Ok(())
        }
        MapNeighborhoodCommand::Pack { input, out } => {
            let neighborhood = pack_map_neighborhood_file(&input, &out)?;
            println!(
                "packed {} neighbor references and {} point sequences into {}",
                neighborhood.neighbors.len(),
                neighborhood.point_sequences.len(),
                out.display()
            );
            Ok(())
        }
    }
}

fn inspect_map_neighborhood(
    neighborhood: &MapNeighborhood,
    encoded_size: usize,
    checksum: Option<u64>,
    json_output: bool,
) -> anyhow::Result<()> {
    let neighbor_map_resource_keys = neighborhood.neighbor_map_resource_keys();
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "map-neighborhood",
                "encoded_size": encoded_size,
                "preamble": neighborhood.preamble,
                "neighbor_count": neighborhood.neighbors.len(),
                "neighbor_map_resource_keys": neighbor_map_resource_keys,
                "point_sequence_count": neighborhood.point_sequences.len(),
                "point_count": neighborhood.point_count(),
                "checksum_fnv1a64": checksum.map(|value| format!("{value:016x}")),
            }))?
        );
    } else {
        println!("type: map-neighborhood");
        println!("encoded_size: {encoded_size}");
        println!("preamble: {:?}", neighborhood.preamble);
        println!(
            "neighbors: {} ({} unique map resource keys)",
            neighborhood.neighbors.len(),
            neighbor_map_resource_keys.len()
        );
        println!("neighbor_map_resource_keys: {neighbor_map_resource_keys:?}");
        println!(
            "point_sequences: {} ({} points)",
            neighborhood.point_sequences.len(),
            neighborhood.point_count()
        );
        if let Some(checksum) = checksum {
            println!("checksum_fnv1a64: {checksum:016x}");
        }
    }
    Ok(())
}
