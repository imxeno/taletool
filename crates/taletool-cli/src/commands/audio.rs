//! Handlers for `sndinfo.lst` audio metadata commands.

use std::path::Path;

use serde_json::json;
use taletool_audio::{SoundFileResolver, SoundInfoTable};

use crate::cli::AudioCommand;
use crate::sound_info::{pack_sound_info, unpack_sound_info};

pub(crate) fn run_audio(command: AudioCommand) -> anyhow::Result<()> {
    match command {
        AudioCommand::Inspect {
            input,
            json,
            wave_dir,
        } => {
            let table = SoundInfoTable::open(&input)?;
            inspect_sound_info(&table, json, wave_dir.as_deref())
        }
        AudioCommand::Unpack { input, out } => {
            let table = SoundInfoTable::open(&input)?;
            let count = unpack_sound_info(&table, &out)?;
            println!("unpacked {count} sound info entries into {}", out.display());
            Ok(())
        }
        AudioCommand::Pack { input, out } => {
            let table = pack_sound_info(&input, &out)?;
            println!(
                "packed {} sound info entries into {}",
                table.entries().len(),
                out.display()
            );
            Ok(())
        }
    }
}

fn inspect_sound_info(
    table: &SoundInfoTable,
    json_output: bool,
    wave_dir: Option<&Path>,
) -> anyhow::Result<()> {
    let mut resolver = wave_dir.map(SoundFileResolver::new);
    if json_output {
        let entries = table
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let resolved_path = match resolver.as_mut() {
                    Some(resolver) => resolver.resolve_entry(entry)?,
                    None => None,
                };
                Ok(json!({
                    "index": index,
                    "key": entry.key,
                    "sound_id": entry.sound_id,
                    "enabled": entry.is_enabled(),
                    "unknown_10": entry.unknown_10,
                    "filename": entry.filename.display_name(),
                    "filename_bytes_hex": hex::encode(entry.filename.as_bytes()),
                    "resolved_path": resolved_path,
                }))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "sndinfo",
                "entry_count": table.entries().len(),
                "trailing_bytes": table.trailing_bytes().len(),
                "entries": entries,
            }))?
        );
        return Ok(());
    }

    println!("type: sndinfo");
    println!("entries: {}", table.entries().len());
    println!("trailing_bytes: {}", table.trailing_bytes().len());
    for (index, entry) in table.entries().iter().enumerate() {
        print!(
            "  {index}: key=({},{},{}) sound_id={} filename={:?}",
            entry.key.group,
            entry.key.primary,
            entry.key.secondary,
            entry.sound_id,
            entry.filename.display_name()
        );
        if let Some(resolver) = resolver.as_mut() {
            match resolver.resolve_entry(entry)? {
                Some(path) => print!(" resolved={}", path.display()),
                None => print!(" resolved=<missing>"),
            }
        }
        println!();
    }
    Ok(())
}
