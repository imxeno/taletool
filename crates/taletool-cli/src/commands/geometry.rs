//! Handlers for `taletool geometry` asset commands.

use std::fs;

use anyhow::Context;
use serde_json::json;
use taletool_geometry::decode_geometry;

use crate::cli::GeometryCommand;
use crate::geometry_file::{pack_geometry_file, unpack_geometry_file};
use crate::util::fnv1a64;

/// Dispatch a `geometry` subcommand.
pub(crate) fn run_geometry(command: GeometryCommand) -> anyhow::Result<()> {
    match command {
        GeometryCommand::Inspect {
            input,
            json: json_output,
            checksum,
        } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let geometry = decode_geometry(&data).context("decoding geometry payload")?;
            let checksum_value = checksum.then(|| format!("{:016x}", fnv1a64(&data)));

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "type": "geometry",
                        "encoded_size": data.len(),
                        "bounds": geometry.header.bounds,
                        "bounding_sphere": geometry.header.bounding_sphere,
                        "animation": {
                            "first_frame": geometry.header.first_frame,
                            "last_frame": geometry.header.last_frame,
                            "frame_rate": geometry.header.frame_rate,
                            "keyframe_step": geometry.header.keyframe_step,
                        },
                        "texture_coordinate_scale": geometry.header.texture_coordinate_scale,
                        "vertex_count": geometry.vertices.len(),
                        "triangle_list_count": geometry.triangle_lists.len(),
                        "triangle_count": geometry.triangle_count(),
                        "root_node_count": geometry.root_nodes.len(),
                        "node_count": geometry.node_count(),
                        "batch_count": geometry.batch_count(),
                        "keyframe_count": geometry.keyframe_count(),
                        "texture_resource_keys": geometry.texture_resource_keys(),
                        "checksum_fnv1a64": checksum_value,
                    }))?
                );
            } else {
                println!("type: geometry");
                println!("encoded_size: {}", data.len());
                println!(
                    "bounds: minimum={:?} maximum={:?}",
                    geometry.header.bounds.minimum, geometry.header.bounds.maximum
                );
                println!(
                    "bounding_sphere: center={:?} radius={}",
                    geometry.header.bounding_sphere.center, geometry.header.bounding_sphere.radius
                );
                println!(
                    "animation: first={} last={} rate={} step={}",
                    geometry.header.first_frame,
                    geometry.header.last_frame,
                    geometry.header.frame_rate,
                    geometry.header.keyframe_step
                );
                println!(
                    "texture_coordinate_scale: {}",
                    geometry.header.texture_coordinate_scale
                );
                println!("vertices: {}", geometry.vertices.len());
                println!(
                    "triangle_lists: {} ({} triangles)",
                    geometry.triangle_lists.len(),
                    geometry.triangle_count()
                );
                println!(
                    "nodes: {} roots={} batches={} keyframes={}",
                    geometry.node_count(),
                    geometry.root_nodes.len(),
                    geometry.batch_count(),
                    geometry.keyframe_count()
                );
                println!(
                    "texture_resource_keys: {:?}",
                    geometry.texture_resource_keys()
                );
                if let Some(checksum_value) = checksum_value {
                    println!("checksum_fnv1a64: {checksum_value}");
                }
            }
            Ok(())
        }
        GeometryCommand::Unpack { input, out } => {
            let data = fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let geometry = decode_geometry(&data).context("decoding geometry payload")?;
            unpack_geometry_file(&geometry, &out)?;
            println!(
                "unpacked {} vertices and {} nodes into {}",
                geometry.vertices.len(),
                geometry.node_count(),
                out.display()
            );
            Ok(())
        }
        GeometryCommand::Pack { input, out } => {
            let geometry = pack_geometry_file(&input, &out)?;
            println!(
                "packed {} vertices and {} nodes into {}",
                geometry.vertices.len(),
                geometry.node_count(),
                out.display()
            );
            Ok(())
        }
    }
}
