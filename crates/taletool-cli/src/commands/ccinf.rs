//! Handlers for `taletool ccinf` structured asset commands.

use serde_json::json;
use taletool_archive::CcinfNosArchive;

use crate::ccinf_file::{pack_ccinf_file, unpack_ccinf_file};
use crate::cli::CcinfCommand;
use crate::util::fnv1a64;

/// Dispatch a `ccinf` subcommand.
pub(crate) fn run_ccinf(command: CcinfCommand) -> anyhow::Result<()> {
    match command {
        CcinfCommand::Inspect {
            input,
            json,
            checksum,
        } => {
            let file = CcinfNosArchive::open(input)?;
            inspect_ccinf_file(&file, json, checksum)
        }
        CcinfCommand::Unpack { input, out } => {
            let file = CcinfNosArchive::open(input)?;
            let count = unpack_ccinf_file(&file, &out)?;
            println!("unpacked {count} CCINF entries into {}", out.display());
            Ok(())
        }
        CcinfCommand::Pack { input, out } => {
            let file = pack_ccinf_file(&input, &out)?;
            println!(
                "packed {} CCINF entries into {}",
                file.entries().len(),
                out.display()
            );
            Ok(())
        }
    }
}

/// Print wrapper metadata and a typed GBFC entry summary.
fn inspect_ccinf_file(
    file: &CcinfNosArchive,
    json_output: bool,
    checksum: bool,
) -> anyhow::Result<()> {
    let entries = file
        .entries()
        .iter()
        .map(|entry| {
            let cell_counts = entry.cell_lists.iter().map(Vec::len).collect::<Vec<_>>();
            json!({
                "entry_id": entry.entry_id,
                "base_resource_key": entry.base_resource_key,
                "remap_table_file_id": entry.remap_table_file_id,
                "animation_file_id": entry.animation_file_id,
                "cell_counts": cell_counts,
            })
        })
        .collect::<Vec<_>>();
    let cell_count = file
        .entries()
        .iter()
        .flat_map(|entry| entry.cell_lists.iter())
        .map(Vec::len)
        .sum::<usize>();
    let checksum_value = checksum.then(|| format!("{:016x}", fnv1a64(file.as_bytes())));

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "ccinf",
                "header_hex": hex::encode(file.header()),
                "unpacked_size": file.unpacked_size(),
                "stored_size": file.stored_size(),
                "compression": "raw",
                "entry_count": file.entries().len(),
                "cell_count": cell_count,
                "checksum_fnv1a64": checksum_value,
                "entries": entries,
            }))?
        );
    } else {
        println!("type: ccinf");
        println!("header: {}", hex::encode(file.header()));
        println!(
            "body: unpacked={} stored={} compression=raw",
            file.unpacked_size(),
            file.stored_size()
        );
        println!("entries: {}", file.entries().len());
        println!("cells: {cell_count}");
        if let Some(checksum_value) = checksum_value {
            println!("checksum_fnv1a64: {checksum_value}");
        }
        for entry in entries.iter().take(20) {
            let cell_counts = entry["cell_counts"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .map(|value| value.as_u64().unwrap_or_default().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            println!(
                "  id={:<12} base={:<12} remap={:<12} animation={:<12} cells=[{}]",
                entry["entry_id"].as_i64().unwrap_or_default(),
                entry["base_resource_key"].as_i64().unwrap_or_default(),
                entry["remap_table_file_id"].as_i64().unwrap_or_default(),
                entry["animation_file_id"].as_i64().unwrap_or_default(),
                cell_counts,
            );
        }
        if entries.len() > 20 {
            println!("  ... {} more", entries.len() - 20);
        }
    }
    Ok(())
}
