//! Handlers for `taletool map` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::json;
use taletool_map::decode_map;

use crate::cli::MapCommand;
use crate::map_file::{pack_map_file, unpack_map_file};
use crate::util::fnv1a64;

/// Dispatch a `map` subcommand.
pub(crate) fn run_map(command: MapCommand) -> anyhow::Result<()> {
    match command {
        MapCommand::Inspect {
            input,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let map = decode_map(&data).context("decoding map payload")?;
            let checksum_value = checksum.then(|| format!("{:016x}", fnv1a64(&data)));

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "type": "map",
                        "encoded_size": data.len(),
                        "resource_group": map.header.resource_group,
                        "bounds": map.header.bounds,
                        "ground_bounds": map.header.ground_bounds,
                        "ground_bounding_sphere": map.header.ground_bounding_sphere,
                        "ambient_light": map.header.ambient_light,
                        "diffuse_light": map.header.diffuse_light,
                        "fog_color": map.header.fog_color,
                        "fog_start": map.header.fog_start,
                        "fog_end": map.header.fog_end,
                        "yaw_limits": map.header.yaw_limits,
                        "pitch_limits": map.header.pitch_limits,
                        "reset_yaw": map.header.reset_yaw,
                        "geometry_key_count": map.geometry_keys.len(),
                        "referenced_geometry_keys": map.referenced_geometry_keys(),
                        "root_node_count": map.root_nodes.len(),
                        "node_count": map.node_count(),
                        "geometry_node_count": map.geometry_node_count(),
                        "effect_node_count": map.effect_node_count(),
                        "checksum_fnv1a64": checksum_value,
                    }))?
                );
            } else {
                println!("type: map");
                println!("encoded_size: {}", data.len());
                println!("resource_group: {}", map.header.resource_group);
                println!(
                    "bounds: minimum={:?} maximum={:?}",
                    map.header.bounds.minimum, map.header.bounds.maximum
                );
                println!(
                    "ground_bounds: minimum={:?} maximum={:?}",
                    map.header.ground_bounds.minimum, map.header.ground_bounds.maximum
                );
                println!(
                    "ground_bounding_sphere: center={:?} radius={}",
                    map.header.ground_bounding_sphere.center,
                    map.header.ground_bounding_sphere.radius
                );
                println!(
                    "lighting: ambient={:?} diffuse={:?}",
                    map.header.ambient_light, map.header.diffuse_light
                );
                println!(
                    "fog: color={:#010x} start={} end={}",
                    map.header.fog_color, map.header.fog_start, map.header.fog_end
                );
                println!(
                    "camera: yaw={:?} pitch={:?} reset_yaw={}",
                    map.header.yaw_limits, map.header.pitch_limits, map.header.reset_yaw
                );
                println!(
                    "geometry_keys: {} referenced={:?}",
                    map.geometry_keys.len(),
                    map.referenced_geometry_keys()
                );
                println!(
                    "nodes: {} roots={} geometry={} effects={}",
                    map.node_count(),
                    map.root_nodes.len(),
                    map.geometry_node_count(),
                    map.effect_node_count()
                );
                if let Some(checksum_value) = checksum_value {
                    println!("checksum_fnv1a64: {checksum_value}");
                }
            }
            Ok(())
        }
        MapCommand::Unpack { input, out } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let map = decode_map(&data).context("decoding map payload")?;
            unpack_map_file(&map, &out)?;
            println!(
                "unpacked {} geometry keys and {} nodes into {}",
                map.geometry_keys.len(),
                map.node_count(),
                out.display()
            );
            Ok(())
        }
        MapCommand::Pack { input, out } => {
            let map = pack_map_file(&input, &out)?;
            println!(
                "packed {} geometry keys and {} nodes into {}",
                map.geometry_keys.len(),
                map.node_count(),
                out.display()
            );
            Ok(())
        }
    }
}
