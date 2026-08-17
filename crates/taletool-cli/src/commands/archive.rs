//! Handlers for `taletool archive` commands.
//!
//! This module owns CLI orchestration for full archive containers: resolving
//! inputs, choosing a parser, printing inspection output, and translating
//! unpacked directory layouts back into archive writer inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::json;
use taletool_archive::{
    BinaryCompression, BinaryNosArchive, BinaryNosArchiveWriteOptions, DelDxPack, TextNosArchive,
    TextNosRecordInput, write_text_nos_archive_bytes,
};

use crate::archive_convert::{
    ConvertedPayload, convert_binary_payload, write_output_transactionally,
};
use crate::archive_detect::{DetectedArchive, detect_archive_paths, has_ccinf_header};
use crate::binary_payloads::{
    BinaryPayloadInput, binary_payload_output_name, explicit_indexes_for_binary_ids,
    order_binary_payload_entries, parse_binary_payload_filename, parse_id_filename,
};
use crate::binary_preset::{
    BinaryPreset, binary_nos_archive_default_compression, format_chunk_pattern, output_pattern,
    parse_header_hex, resolve_binary_nos_preset_for_archives, resolve_binary_preset,
    resolve_zlib_profile,
};
use crate::cli::{ArchiveCommand, ArchiveType, ChunkingArg, CompressionArg};
use crate::paths::{escape_archive_name, immediate_files, resolve_inputs, unescape_archive_name};
use crate::sound_pack::{
    pack_sound_pack_dir as build_sound_pack_dir, sound_pack_manifest_exists, unpack_sound_pack,
};
use crate::text_archive_convert::{
    TextArchiveOutputMode, convert_text_archive, resolve_text_archive_conversion,
};
use crate::text_payload::{packed_flag_for_text_record, payload_kind_label};
use crate::util::{duplicate_id_counts, fnv1a64, warn_duplicate_archive_ids};

/// Dispatch an `archive` subcommand.
pub(crate) fn run_archive(command: ArchiveCommand) -> anyhow::Result<()> {
    match command {
        ArchiveCommand::Inspect {
            input,
            archive_type,
            json,
            checksum,
        } => {
            let paths = resolve_inputs(&input)?;
            reject_ccinf_inputs(&paths, "inspect")?;
            let detected = detect_archive_paths(&paths, archive_type)?;
            match detected {
                DetectedArchive::Binary(archives) => {
                    inspect_binary_archives(&archives, json, checksum)
                }
                DetectedArchive::Text(archive) => inspect_text_archive(&archive, json, checksum),
                DetectedArchive::Sound(archive) => inspect_sound_pack(&archive, json, checksum),
            }
        }
        ArchiveCommand::Unpack {
            input,
            out,
            archive_type,
            convert,
            encoding,
            plain_text,
        } => {
            let paths = resolve_inputs(&input)?;
            reject_ccinf_inputs(&paths, "unpack")?;
            let detected = detect_archive_paths(&paths, archive_type)?;
            if convert {
                unpack_converted_archive_with_options(
                    detected,
                    &out,
                    encoding.as_deref(),
                    plain_text,
                )
            } else {
                match detected {
                    DetectedArchive::Binary(archives) => unpack_binary_archives(&archives, &out),
                    DetectedArchive::Text(archive) => unpack_text_archive(&archive, &out),
                    DetectedArchive::Sound(archive) => unpack_sound_pack_archive(&archive, &out),
                }
            }
        }
        ArchiveCommand::Pack {
            dir,
            out,
            archive_type,
            preset,
            header_hex,
            direct_index,
            compression,
            zlib_profile,
            chunking,
            chunk_count,
            chunk_format,
        } => {
            reject_ccinf_pack(&dir, &out)?;
            let pack_type = infer_pack_type(&dir, &out, archive_type, &preset)?;
            match pack_type {
                ArchiveType::Text => pack_text_archive_dir(&dir, Path::new(&out)),
                ArchiveType::Sound => pack_sound_pack_archive_dir(&dir, Path::new(&out)),
                ArchiveType::Binary => pack_binary_archive_dir(
                    &dir,
                    &out,
                    &preset,
                    header_hex.as_deref(),
                    direct_index,
                    compression,
                    &zlib_profile,
                    chunking,
                    chunk_count,
                    chunk_format.as_deref(),
                ),
                ArchiveType::Auto => unreachable!("pack type inference resolves auto"),
            }
        }
    }
}

/// Redirect CCINF files away from archive container commands.
fn reject_ccinf_inputs(paths: &[PathBuf], operation: &str) -> anyhow::Result<()> {
    if let Some(path) = paths.iter().find(|path| has_ccinf_header(path)) {
        let suggestion = match operation {
            "inspect" => format!("taletool ccinf inspect \"{}\"", path.display()),
            "unpack" => format!(
                "taletool ccinf unpack \"{}\" --out <output.json>",
                path.display()
            ),
            _ => unreachable!("archive CCINF redirect operation is known"),
        };
        anyhow::bail!(
            "{} is a CCINF asset, not an archive container; use `{suggestion}`",
            path.display(),
        );
    }
    Ok(())
}

/// Redirect attempts to build a known CCINF target with `archive pack`.
fn reject_ccinf_pack(dir: &Path, out: &str) -> anyhow::Result<()> {
    if output_is_ccinf(out) || dir.join("ccinf.json").is_file() {
        anyhow::bail!(
            "CCINF .NOS files are assets, not archive containers; use `taletool ccinf pack <input.json> --out {out}`"
        );
    }
    Ok(())
}

/// Print summary metadata for binary archive chunks.
fn inspect_binary_archives(
    archives: &[BinaryNosArchive],
    json_output: bool,
    checksum: bool,
) -> anyhow::Result<()> {
    let mut entries = Vec::new();
    let mut raw = 0usize;
    let mut zlib = 0usize;
    for archive in archives {
        for entry in archive.entries() {
            match entry.compression {
                BinaryCompression::Raw => raw += 1,
                BinaryCompression::Zlib => zlib += 1,
            }
            let checksum_value = if checksum {
                Some(fnv1a64(&archive.read_entry_payload(entry)?.data))
            } else {
                None
            };
            entries.push(json!({
                "archive": archive.path(),
                "id": entry.file_id,
                "stored_size": entry.stored_size,
                "unpacked_size": entry.unpacked_size,
                "compression": format!("{:?}", entry.compression).to_ascii_lowercase(),
                "checksum_fnv1a64": checksum_value.map(|value| format!("{value:016x}")),
            }));
        }
    }

    if json_output {
        let output = json!({
            "type": "binary",
            "chunks": archives.len(),
            "entries": entries,
            "compression_counts": {"raw": raw, "zlib": zlib},
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("type: binary");
        println!("chunks: {}", archives.len());
        println!("entries: {}", entries.len());
        println!("compression: raw={raw} zlib={zlib}");
        for entry in entries.iter().take(20) {
            println!(
                "  id={:<12} stored={:<8} unpacked={:<8} compression={}{}",
                entry["id"].as_i64().unwrap_or_default(),
                entry["stored_size"].as_u64().unwrap_or_default(),
                entry["unpacked_size"].as_u64().unwrap_or_default(),
                entry["compression"].as_str().unwrap_or_default(),
                entry["checksum_fnv1a64"]
                    .as_str()
                    .map(|value| format!(" checksum={value}"))
                    .unwrap_or_default()
            );
        }
        if entries.len() > 20 {
            println!("  ... {} more", entries.len() - 20);
        }
    }
    Ok(())
}

/// Print summary metadata for a text archive.
fn inspect_text_archive(
    archive: &TextNosArchive,
    json_output: bool,
    checksum: bool,
) -> anyhow::Result<()> {
    let entries = archive
        .records()
        .iter()
        .map(|record| {
            json!({
                "id": record.id,
                "name": record.name,
                "packed": record.is_packed(),
                "payload_size": record.payload.len(),
                "kind": payload_kind_label(record.payload_kind()),
                "checksum_fnv1a64": checksum.then(|| format!("{:016x}", fnv1a64(&record.payload))),
            })
        })
        .collect::<Vec<_>>();
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "text",
                "records": entries,
                "timestamp": archive.timestamp(),
                "trailing_bytes": archive.trailing_bytes(),
            }))?
        );
    } else {
        println!("type: text");
        println!("records: {}", archive.records().len());
        for record in entries.iter().take(20) {
            println!(
                "  id={:<4} kind={:<4} packed={:<5} payload={:<8} name={}{}",
                record["id"].as_i64().unwrap_or_default(),
                record["kind"].as_str().unwrap_or_default(),
                record["packed"].as_bool().unwrap_or_default(),
                record["payload_size"].as_u64().unwrap_or_default(),
                record["name"].as_str().unwrap_or_default(),
                record["checksum_fnv1a64"]
                    .as_str()
                    .map(|value| format!(" checksum={value}"))
                    .unwrap_or_default()
            );
        }
        if entries.len() > 20 {
            println!("  ... {} more", entries.len() - 20);
        }
    }
    Ok(())
}

/// Print summary metadata for a DelDX sound pack.
fn inspect_sound_pack(
    archive: &DelDxPack,
    json_output: bool,
    checksum: bool,
) -> anyhow::Result<()> {
    let mut entries = Vec::new();
    for entry in archive.entries() {
        let checksum_value = if checksum {
            Some(fnv1a64(&archive.read_entry_payload(entry)?))
        } else {
            None
        };
        entries.push(json!({
            "index": entry.index,
            "name": entry.name,
            "key": entry.key,
            "data_offset": entry.data_offset,
            "data_size": entry.data_size,
            "checksum_fnv1a64": checksum_value.map(|value| format!("{value:016x}")),
        }));
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "type": "sound",
                "format": "sound",
                "entries": entries,
                "header_hex": hex::encode(archive.header()),
            }))?
        );
    } else {
        println!("type: sound");
        println!("format: sound");
        println!("entries: {}", archive.entries().len());
        for entry in entries.iter().take(20) {
            println!(
                "  index={:<5} key={:<8} payload={:<8} name={}{}",
                entry["index"].as_u64().unwrap_or_default(),
                entry["key"].as_i64().unwrap_or_default(),
                entry["data_size"].as_u64().unwrap_or_default(),
                entry["name"].as_str().unwrap_or_default(),
                entry["checksum_fnv1a64"]
                    .as_str()
                    .map(|value| format!(" checksum={value}"))
                    .unwrap_or_default()
            );
        }
        if entries.len() > 20 {
            println!("  ... {} more", entries.len() - 20);
        }
    }
    Ok(())
}

struct ConvertedArchiveEntry {
    file_id: i32,
    payload: ConvertedPayload,
}

#[cfg(test)]
fn unpack_converted_archive(detected: DetectedArchive, out: &Path) -> anyhow::Result<()> {
    unpack_converted_archive_with_options(detected, out, None, false)
}

#[cfg(test)]
fn unpack_converted_archive_with_encoding(
    detected: DetectedArchive,
    out: &Path,
    encoding: Option<&str>,
) -> anyhow::Result<()> {
    unpack_converted_archive_with_options(detected, out, encoding, false)
}

#[cfg(test)]
fn unpack_converted_plain_text(detected: DetectedArchive, out: &Path) -> anyhow::Result<()> {
    unpack_converted_archive_with_options(detected, out, None, true)
}

fn unpack_converted_archive_with_options(
    detected: DetectedArchive,
    out: &Path,
    encoding: Option<&str>,
    plain_text: bool,
) -> anyhow::Result<()> {
    match detected {
        DetectedArchive::Binary(archives) => {
            reject_non_text_encoding(encoding, "binary archive")?;
            reject_plain_text(plain_text, "binary archive")?;
            let preset = resolve_binary_nos_preset_for_archives(&archives)?;
            let converted = write_output_transactionally(out, |staging| {
                unpack_converted_binary_archives(&archives, preset, staging)
            })?;
            for entry in &converted {
                println!(
                    "converted {:<12} {} ({})",
                    entry.file_id,
                    out.join(&entry.payload.relative_path).display(),
                    entry.payload.description,
                );
            }
            println!(
                "converted {} {} payloads into {}",
                converted.len(),
                preset.name,
                out.display()
            );
            Ok(())
        }
        DetectedArchive::Text(archive) => {
            let output_mode = if plain_text {
                TextArchiveOutputMode::PlainText
            } else {
                TextArchiveOutputMode::Json
            };
            let plan = resolve_text_archive_conversion(&archive, output_mode, encoding)?;
            let family = plan.family();
            let converted = write_output_transactionally(out, |staging| {
                convert_text_archive(&archive, &plan, staging)
            })?;
            for record in &converted {
                for warning in &record.warnings {
                    eprintln!("{warning}");
                }
                println!(
                    "converted {:<4} {} ({})",
                    record.id,
                    out.join(&record.relative_path).display(),
                    record.description,
                );
            }
            println!(
                "converted {} {} records into {}",
                converted.len(),
                family.name(),
                out.display()
            );
            Ok(())
        }
        DetectedArchive::Sound(archive) => {
            reject_non_text_encoding(encoding, "sound pack")?;
            reject_plain_text(plain_text, "sound pack")?;
            let count =
                write_output_transactionally(out, |staging| unpack_sound_pack(&archive, staging))?;
            println!("unpacked {count} sound pack entries into {}", out.display());
            Ok(())
        }
    }
}

fn reject_plain_text(plain_text: bool, target: &str) -> anyhow::Result<()> {
    if plain_text {
        anyhow::bail!(
            "--plain-text can only be used when converting a text archive, not a {target}"
        );
    }
    Ok(())
}

fn reject_non_text_encoding(encoding: Option<&str>, target: &str) -> anyhow::Result<()> {
    if encoding.is_some() {
        anyhow::bail!("--encoding can only be used when converting a text archive, not a {target}");
    }
    Ok(())
}

fn unpack_converted_binary_archives(
    archives: &[BinaryNosArchive],
    preset: BinaryPreset,
    out: &Path,
) -> anyhow::Result<Vec<ConvertedArchiveEntry>> {
    let duplicates = duplicate_id_counts(
        archives
            .iter()
            .flat_map(|archive| archive.entries().iter().map(|entry| entry.file_id)),
    );
    warn_duplicate_archive_ids(
        "converting",
        preset.name,
        &duplicates,
        "Repeated payloads will be preserved with __N filename suffixes.",
    );

    let mut converted = Vec::new();
    let mut seen_ids = BTreeMap::<i32, usize>::new();
    let mut used_names = BTreeSet::<String>::new();
    for archive in archives {
        let ids = archive
            .entries()
            .iter()
            .map(|entry| entry.file_id)
            .collect::<Vec<_>>();
        let explicit_indexes = explicit_indexes_for_binary_ids(&ids);
        let default_compression = binary_nos_archive_default_compression(archive);
        for (entry_index, entry) in archive.entries().iter().enumerate() {
            let payload = archive.read_entry_payload(entry).with_context(|| {
                format!(
                    "reading {} entry at table index {} with file id {} from {}",
                    preset.name,
                    entry_index,
                    entry.file_id,
                    archive.path().display()
                )
            })?;
            let seen = seen_ids.entry(entry.file_id).or_default();
            *seen += 1;
            let explicit_index = explicit_indexes
                .contains(&entry_index)
                .then_some(entry_index);
            let compression =
                (entry.compression != default_compression).then_some(entry.compression);
            let file_name = binary_payload_output_name(
                entry.file_id,
                *seen,
                explicit_index,
                compression,
                &mut used_names,
            )?;
            let converted_payload =
                convert_binary_payload(&payload.data, preset.asset_kind, out, &file_name)
                    .with_context(|| {
                        format!(
                            "converting {} entry at table index {} with file id {} from {}",
                            preset.name,
                            entry_index,
                            entry.file_id,
                            archive.path().display()
                        )
                    })?;
            converted.push(ConvertedArchiveEntry {
                file_id: entry.file_id,
                payload: converted_payload,
            });
        }
    }
    Ok(converted)
}

/// Extract binary archive payloads with stable filenames.
fn unpack_binary_archives(archives: &[BinaryNosArchive], out: &Path) -> anyhow::Result<()> {
    let duplicates = duplicate_id_counts(
        archives
            .iter()
            .flat_map(|archive| archive.entries().iter().map(|entry| entry.file_id)),
    );
    warn_duplicate_archive_ids(
        "unpacking",
        "binary",
        &duplicates,
        "Repeated payloads will be preserved with __N filename suffixes.",
    );

    fs::create_dir_all(out)?;
    let mut count = 0usize;
    let mut seen_ids = BTreeMap::<i32, usize>::new();
    let mut used_names = BTreeSet::<String>::new();
    for archive in archives {
        let ids = archive
            .entries()
            .iter()
            .map(|entry| entry.file_id)
            .collect::<Vec<_>>();
        let explicit_indexes = explicit_indexes_for_binary_ids(&ids);
        let default_compression = binary_nos_archive_default_compression(archive);
        for (entry_index, entry) in archive.entries().iter().enumerate() {
            let payload = archive.read_entry_payload(entry)?;
            let seen = seen_ids.entry(entry.file_id).or_default();
            *seen += 1;
            let explicit_index = explicit_indexes
                .contains(&entry_index)
                .then_some(entry_index);
            let compression =
                (entry.compression != default_compression).then_some(entry.compression);
            let file_name = binary_payload_output_name(
                entry.file_id,
                *seen,
                explicit_index,
                compression,
                &mut used_names,
            )?;
            let path = out.join(file_name);
            fs::write(&path, payload.data)?;
            println!("unpacked {:<12} {}", entry.file_id, path.display());
            count += 1;
        }
    }
    println!("unpacked {count} payloads into {}", out.display());
    Ok(())
}

/// Extract text archive records using escaped archive names.
fn unpack_text_archive(archive: &TextNosArchive, out: &Path) -> anyhow::Result<()> {
    let records = write_text_archive_records(archive, out)?;
    for (id, relative_path) in &records {
        println!("unpacked {id:<4} {}", out.join(relative_path).display());
    }
    println!(
        "unpacked {} text records into {}",
        records.len(),
        out.display()
    );
    Ok(())
}

fn write_text_archive_records(
    archive: &TextNosArchive,
    out: &Path,
) -> anyhow::Result<Vec<(i32, PathBuf)>> {
    let duplicates = duplicate_id_counts(archive.records().iter().map(|record| record.id));
    warn_duplicate_archive_ids(
        "unpacking",
        "text",
        &duplicates,
        "IDs are ambiguous for id-based lookup; files are written by archived name.",
    );

    fs::create_dir_all(out)?;
    let mut written = Vec::with_capacity(archive.records().len());
    for record in archive.records() {
        let relative_path = PathBuf::from(escape_archive_name(&record.name));
        let path = out.join(&relative_path);
        fs::write(&path, &record.payload)?;
        written.push((record.id, relative_path));
    }
    Ok(written)
}

/// Extract a DelDX sound pack into a manifest-backed directory.
fn unpack_sound_pack_archive(archive: &DelDxPack, out: &Path) -> anyhow::Result<()> {
    let count = unpack_sound_pack(archive, out)?;
    println!("unpacked {count} sound pack entries into {}", out.display());
    Ok(())
}

/// Pack an unpacked text archive directory into a text archive.
fn pack_text_archive_dir(dir: &Path, out: &Path) -> anyhow::Result<()> {
    let mut records = Vec::new();
    for path in immediate_files(dir)? {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid UTF-8 file name: {}", path.display()))?;
        let name = unescape_archive_name(file_name)?;
        let payload = fs::read(&path)?;
        records.push(TextNosRecordInput {
            packed_flag: packed_flag_for_text_record(&name),
            name_bytes: name.as_bytes().to_vec(),
            name,
            payload,
        });
    }
    records.sort_by_key(|record| record.name.to_lowercase());
    let bytes = write_text_nos_archive_bytes(&records)?;
    fs::write(out, bytes)?;
    println!(
        "packed {} text records into {}",
        records.len(),
        out.display()
    );
    Ok(())
}

/// Pack a manifest-backed sound-pack directory into a DelDX pack.
fn pack_sound_pack_archive_dir(dir: &Path, out: &Path) -> anyhow::Result<()> {
    let archive = build_sound_pack_dir(dir, out)?;
    println!(
        "packed {} sound pack entries into {}",
        archive.entries().len(),
        out.display()
    );
    Ok(())
}

/// Pack numeric payload files into one or more binary archives.
#[allow(clippy::too_many_arguments)]
fn pack_binary_archive_dir(
    dir: &Path,
    out: &str,
    preset_arg: &str,
    header_hex: Option<&str>,
    direct_index: Option<u8>,
    compression_arg: CompressionArg,
    zlib_profile_arg: &str,
    chunking_arg: Option<ChunkingArg>,
    chunk_count_arg: Option<usize>,
    chunk_format_arg: Option<&str>,
) -> anyhow::Result<()> {
    let mut entries = Vec::new();
    for path in immediate_files(dir)? {
        let Some(file_name) = parse_binary_payload_filename(&path) else {
            continue;
        };
        entries.push(BinaryPayloadInput {
            file_id: file_name.file_id,
            duplicate_ordinal: file_name.duplicate_ordinal,
            explicit_index: file_name.explicit_index,
            compression: file_name.compression,
            source_name: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_owned(),
            data: fs::read(&path)?,
        });
    }
    if entries.is_empty() {
        anyhow::bail!("no numeric ID payload files found in {}", dir.display());
    }
    let duplicates = duplicate_id_counts(entries.iter().map(|entry| entry.file_id));
    warn_duplicate_archive_ids(
        "packing",
        "binary",
        &duplicates,
        "Client ID lookup resolves only one matching entry.",
    );

    let preset = resolve_binary_preset(out, preset_arg);
    let header = match header_hex {
        Some(value) => parse_header_hex(value)?,
        None => preset
            .as_ref()
            .map(|preset| preset.header)
            .ok_or_else(|| anyhow::anyhow!("binary archive pack needs --header-hex or a preset"))?,
    };
    let compression = match compression_arg {
        CompressionArg::Raw => BinaryCompression::Raw,
        CompressionArg::Zlib => BinaryCompression::Zlib,
        CompressionArg::Auto => preset
            .as_ref()
            .map(|preset| preset.compression)
            .ok_or_else(|| {
                anyhow::anyhow!("binary archive pack needs --compression or a preset")
            })?,
    };
    let zlib_profile = resolve_zlib_profile(zlib_profile_arg, compression, preset.as_ref())?;
    let chunking = chunking_arg
        .or_else(|| preset.as_ref().map(|preset| preset.chunking))
        .unwrap_or(ChunkingArg::Single);
    let chunk_count = chunk_count_arg
        .or_else(|| preset.as_ref().map(|preset| preset.chunk_count))
        .unwrap_or(1);
    if chunk_count == 0 {
        anyhow::bail!("--chunk-count must be greater than zero");
    }
    let direct_index =
        direct_index.unwrap_or_else(|| preset.as_ref().map_or(0, |preset| preset.direct_index));
    let output_pattern = output_pattern(out, chunk_format_arg, preset.as_ref(), chunk_count)?;

    let mut chunks: BTreeMap<usize, Vec<BinaryPayloadInput>> = BTreeMap::new();
    match chunking {
        ChunkingArg::Single => {
            chunks.insert(0, entries);
        }
        ChunkingArg::LowByte => {
            for entry in entries {
                let chunk = (entry.file_id as u32 & 0xff) as usize;
                if chunk >= chunk_count {
                    anyhow::bail!(
                        "file id {} maps to chunk {}, but chunk count is {}",
                        entry.file_id,
                        chunk,
                        chunk_count
                    );
                }
                chunks.entry(chunk).or_default().push(entry);
            }
            for chunk in 0..chunk_count {
                chunks.entry(chunk).or_default();
            }
        }
    }

    let mut written = 0usize;
    for (chunk, entries) in chunks {
        let entries = order_binary_payload_entries(entries)?;
        let path = if chunk_count == 1 {
            PathBuf::from(&output_pattern)
        } else {
            PathBuf::from(format_chunk_pattern(&output_pattern, chunk))
        };
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let archive = BinaryNosArchive::from_entries(
            path.clone(),
            entries,
            &BinaryNosArchiveWriteOptions {
                header,
                direct_index,
                compression,
                zlib_profile,
            },
        )?;
        archive.write_to(&path)?;
        println!(
            "packed {:<5} entries into {}",
            archive.entries().len(),
            path.display()
        );
        written += 1;
    }
    println!("wrote {written} archive file(s)");
    Ok(())
}

/// Resolve the archive type for `archive pack` when the user selects `auto`.
fn infer_pack_type(
    dir: &Path,
    out: &str,
    archive_type: ArchiveType,
    preset: &str,
) -> anyhow::Result<ArchiveType> {
    if archive_type != ArchiveType::Auto {
        return Ok(archive_type);
    }
    if sound_pack_manifest_exists(dir) || output_is_sound(out) {
        return Ok(ArchiveType::Sound);
    }
    let preset_lower = preset.to_ascii_lowercase();
    if matches!(preset_lower.as_str(), "nsgtddata" | "nslangdata") || output_is_text(out) {
        return Ok(ArchiveType::Text);
    }
    let files = immediate_files(dir)?;
    if files.iter().all(|path| parse_id_filename(path).is_some()) {
        Ok(ArchiveType::Binary)
    } else {
        Ok(ArchiveType::Text)
    }
}

/// Return whether an output path names a sound pack.
fn output_is_sound(out: &str) -> bool {
    Path::new(out)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|name| {
            name.eq_ignore_ascii_case("snd.pck") || name.to_ascii_lowercase().ends_with(".pck")
        })
        .unwrap_or(false)
}

/// Return whether an output path names a known CCINF file.
fn output_is_ccinf(out: &str) -> bool {
    Path::new(out)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|name| {
            name.eq_ignore_ascii_case("NSmnData.NOS") || name.eq_ignore_ascii_case("NSpnData.NOS")
        })
        .unwrap_or(false)
}

/// Return whether an output path names a known text archive family.
fn output_is_text(out: &str) -> bool {
    Path::new(out)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|name| {
            let lower = name.to_ascii_lowercase();
            lower.starts_with("nsgtddata") || lower.starts_with("nslangdata")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{GenericImageView, Rgba, RgbaImage};
    use taletool_archive::{
        BinaryNosArchiveWriteEntry, DELDX_PACK_HEADER_LEN, DelDxPackWriteOptions,
    };
    use taletool_ccinf::CCINF_HEADER;
    use taletool_effect::{
        AnimationTiming, ColorAnimation, EffectAsset, EffectAssetKind, EffectDefinition,
        EffectDefinitionLoaderWorkspace, TextureAnimation, TransformAnimation,
        write_effect_asset_bytes,
    };
    use taletool_geometry::{
        AxisAlignedBounds, BoundingSphere as GeometryBoundingSphere, Geometry, GeometryHeader,
        write_geometry_bytes,
    };
    use taletool_map::{
        BoundingSphere as MapBoundingSphere, Bounds3, CameraAngleLimits, HeightGrid,
        HeightGridBounds, HeightGridDimensions, HeightGridEncoding, MAP_HEADER_UNKNOWN_00_LEN,
        MAP_HEADER_UNKNOWN_79_LEN, Map, MapHeader, Rgba8, write_height_grid_bytes, write_map_bytes,
    };
    use taletool_text::{
        ConstStringEntry, ConstStringTable, LanguageEntry, LanguageTable, NSetcStringList,
        TextEncoding, TextPayloadKind, encode_const_string_table, encode_dat_payload,
        encode_language_table, encode_list_payload, encode_nsetc_string_list,
    };
    use taletool_texture::sprite::free_size::{
        decode_free_size_sprite, write_free_size_sprite_bytes,
    };
    use taletool_texture::sprite::{decode_sprite, write_sprite_bytes};
    use taletool_texture::{TextureFormat, TextureHeader, write_texture_bytes};
    use taletool_zlib::ZlibProfile;

    use crate::sound_pack::SOUND_PACK_MANIFEST_FILE;
    use crate::sprite_file::SPRITE_MANIFEST_FILE;
    use crate::texture_file::TEXTURE_MANIFEST_FILE;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn archive_for_preset(
        preset: BinaryPreset,
        entries: Vec<BinaryNosArchiveWriteEntry>,
    ) -> BinaryNosArchive {
        BinaryNosArchive::from_entries(
            PathBuf::from(format!("{}.NOS", preset.name)),
            entries,
            &BinaryNosArchiveWriteOptions::new(
                preset.header,
                preset.direct_index,
                preset.compression,
                preset
                    .zlib_profile
                    .unwrap_or_else(|| ZlibProfile::default_level(9)),
            ),
        )
        .unwrap()
    }

    fn text_archive(archive_name: &str, records: Vec<(&str, i32, Vec<u8>)>) -> TextNosArchive {
        let records = records
            .into_iter()
            .map(|(name, packed_flag, payload)| TextNosRecordInput {
                name: name.to_owned(),
                name_bytes: name.as_bytes().to_vec(),
                packed_flag,
                payload,
            })
            .collect::<Vec<_>>();
        let bytes = write_text_nos_archive_bytes(&records).unwrap();
        TextNosArchive::from_bytes(PathBuf::from(archive_name), bytes).unwrap()
    }

    fn sample_payload(kind: crate::binary_preset::BinaryAssetKind) -> Vec<u8> {
        use crate::binary_preset::BinaryAssetKind;

        match kind {
            BinaryAssetKind::Geometry => write_geometry_bytes(&Geometry {
                header: GeometryHeader {
                    bounds: AxisAlignedBounds {
                        minimum: [0.0; 3],
                        maximum: [1.0; 3],
                    },
                    bounding_sphere: GeometryBoundingSphere {
                        center: [0.0; 3],
                        radius: 1.0,
                    },
                    first_frame: 0,
                    last_frame: 0,
                    frame_rate: 30,
                    keyframe_step: 160,
                    texture_coordinate_scale: 1.0 / 32767.0,
                },
                vertices: Vec::new(),
                triangle_lists: Vec::new(),
                root_nodes: Vec::new(),
            })
            .unwrap(),
            BinaryAssetKind::Texture => write_texture_bytes(
                &TextureHeader {
                    width: 2,
                    height: 2,
                    format: TextureFormat::A8R8G8B8,
                    filter_flag: 0,
                    unknown_06: 0,
                    mip_level_count: 1,
                },
                &[RgbaImage::new(2, 2)],
            )
            .unwrap(),
            BinaryAssetKind::Effect(effect_kind) => {
                let timing = AnimationTiming {
                    first_frame: 0,
                    last_frame: 1,
                    frame_rate: 30,
                    keyframe_step: 160,
                };
                let effect = match effect_kind {
                    EffectAssetKind::ColorAnimation => {
                        EffectAsset::ColorAnimation(ColorAnimation {
                            timing,
                            keyframes: Vec::new(),
                        })
                    }
                    EffectAssetKind::Definition => EffectAsset::Definition(EffectDefinition {
                        resource_key: 7,
                        loader_workspace: EffectDefinitionLoaderWorkspace::default(),
                        components: Vec::new(),
                    }),
                    EffectAssetKind::TransformAnimation => {
                        EffectAsset::TransformAnimation(TransformAnimation {
                            timing,
                            translation_keyframes: Vec::new(),
                            rotation_keyframes: Vec::new(),
                            scale_keyframes: Vec::new(),
                        })
                    }
                    EffectAssetKind::TextureAnimation => {
                        EffectAsset::TextureAnimation(TextureAnimation {
                            timing,
                            keyframes: Vec::new(),
                        })
                    }
                };
                write_effect_asset_bytes(&effect).unwrap()
            }
            BinaryAssetKind::CellFlag => vec![1, 0, 1, 0, 0],
            BinaryAssetKind::Map => write_map_bytes(&Map {
                header: MapHeader {
                    unknown_00: vec![0; MAP_HEADER_UNKNOWN_00_LEN],
                    resource_group: 0,
                    bounds: Bounds3 {
                        minimum: [0.0; 3],
                        maximum: [1.0; 3],
                    },
                    ground_bounds: Bounds3 {
                        minimum: [0.0; 3],
                        maximum: [1.0; 3],
                    },
                    ground_bounding_sphere: MapBoundingSphere {
                        center: [0.0; 3],
                        radius: 1.0,
                    },
                    ambient_light: Rgba8 {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: 0,
                    },
                    diffuse_light: Rgba8 {
                        red: 0,
                        green: 0,
                        blue: 0,
                        alpha: 0,
                    },
                    fog_color: 0,
                    yaw_limits: CameraAngleLimits {
                        angle_degrees: 0,
                        minimum_offset_degrees: 0,
                        maximum_offset_degrees: 0,
                    },
                    pitch_limits: CameraAngleLimits {
                        angle_degrees: 0,
                        minimum_offset_degrees: 0,
                        maximum_offset_degrees: 0,
                    },
                    fog_start: 0,
                    fog_end: 0,
                    unknown_79: vec![0; MAP_HEADER_UNKNOWN_79_LEN],
                    reset_yaw: false,
                    unknown_84: 0,
                },
                geometry_keys: Vec::new(),
                root_nodes: Vec::new(),
            })
            .unwrap(),
            BinaryAssetKind::MapObjectSprite => write_sprite_bytes(&[]).unwrap(),
            BinaryAssetKind::SpriteAnimation => vec![0, 1],
            BinaryAssetKind::SpriteRemap => vec![0],
            BinaryAssetKind::MapNeighborhood => {
                vec![8, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0]
            }
            BinaryAssetKind::HeightGrid => write_height_grid_bytes(&HeightGrid {
                encoding: HeightGridEncoding::Version1,
                grid_id: 1,
                map_id: 2,
                bounds: HeightGridBounds {
                    minimum: [0.0; 3],
                    maximum: [1.0; 3],
                },
                dimensions: HeightGridDimensions { width: 1, depth: 1 },
                cell_size: [1.0; 3],
                vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                triangles: vec![[0, 1, 2]],
                cells: vec![vec![0]],
            })
            .unwrap(),
            BinaryAssetKind::FreeSizeSprite => {
                write_free_size_sprite_bytes(&RgbaImage::new(2, 2)).unwrap()
            }
        }
    }

    fn expected_converted_marker(
        kind: crate::binary_preset::BinaryAssetKind,
        out: &Path,
        stem: &str,
    ) -> PathBuf {
        use crate::binary_preset::BinaryAssetKind;

        match kind {
            BinaryAssetKind::Texture => out.join(stem).join(TEXTURE_MANIFEST_FILE),
            BinaryAssetKind::MapObjectSprite => out.join(stem).join(SPRITE_MANIFEST_FILE),
            BinaryAssetKind::CellFlag | BinaryAssetKind::FreeSizeSprite => {
                out.join(format!("{stem}.png"))
            }
            _ => out.join(format!("{stem}.json")),
        }
    }

    fn assert_no_staging_directories(root: &Path) {
        let staging = fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".staging"))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert!(
            staging.is_empty(),
            "staging directories remain: {staging:?}"
        );
    }

    #[test]
    fn redirects_known_ccinf_pack_targets() {
        let error = reject_ccinf_pack(Path::new("unused"), "NSpnData.NOS").unwrap_err();
        assert!(error.to_string().contains("taletool ccinf pack"));

        let root = temp_dir("ccinf-redirect");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("ccinf.json"), b"{}").unwrap();
        let error = reject_ccinf_pack(&root, "renamed.NOS").unwrap_err();
        assert!(error.to_string().contains("taletool ccinf pack"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn redirects_ccinf_inspect_and_unpack_inputs() {
        let root = temp_dir("ccinf-input-redirect");
        let path = root.join("NSmnData.NOS");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, CCINF_HEADER).unwrap();

        for operation in ["inspect", "unpack"] {
            let error = reject_ccinf_inputs(std::slice::from_ref(&path), operation).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(&format!("taletool ccinf {operation}"))
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn converts_every_supported_binary_archive_family() {
        let root = temp_dir("convert-all-families");
        fs::create_dir_all(&root).unwrap();
        for name in [
            "NStgData",
            "NStgeData",
            "NStpData",
            "NStpeData",
            "NStpuData",
            "NSedData",
            "NSemData",
            "NSesData",
            "NSeffData",
            "NStcData",
            "NStuData",
            "NSipData",
            "NSmcData",
            "NSmpData",
            "NSppData",
            "NSpcData",
            "NSpmData",
            "NStkData",
            "NSgrdData",
            "NS4BbData",
        ] {
            let preset = resolve_binary_preset(&format!("{name}.NOS"), "auto").unwrap();
            let archive = archive_for_preset(
                preset,
                vec![BinaryNosArchiveWriteEntry::new(
                    42,
                    sample_payload(preset.asset_kind),
                )],
            );
            let out = root.join(name);
            unpack_converted_archive(DetectedArchive::Binary(vec![archive]), &out).unwrap();
            assert!(
                expected_converted_marker(preset.asset_kind, &out, "42").is_file(),
                "missing converted marker for {name}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conversion_uses_family_semantics_for_ambiguous_sprites() {
        let root = temp_dir("convert-ambiguous-sprite");
        fs::create_dir_all(&root).unwrap();
        let mut image = RgbaImage::new(257, 256);
        image.put_pixel(1, 0, Rgba([0, 13, 0, 0]));
        let payload = write_free_size_sprite_bytes(&image).unwrap();
        assert!(decode_sprite(&payload).is_ok());
        assert!(decode_free_size_sprite(&payload).is_ok());

        let free_preset = resolve_binary_preset("NS4BbData.NOS", "auto").unwrap();
        let free_out = root.join("free");
        unpack_converted_archive(
            DetectedArchive::Binary(vec![archive_for_preset(
                free_preset,
                vec![BinaryNosArchiveWriteEntry::new(7, payload.clone())],
            )]),
            &free_out,
        )
        .unwrap();
        assert_eq!(
            image::open(free_out.join("7.png")).unwrap().dimensions(),
            (257, 256)
        );
        assert!(!free_out.join("7").exists());

        let map_preset = resolve_binary_preset("NSipData.NOS", "auto").unwrap();
        let map_out = root.join("map-object");
        unpack_converted_archive(
            DetectedArchive::Binary(vec![archive_for_preset(
                map_preset,
                vec![BinaryNosArchiveWriteEntry::new(7, payload)],
            )]),
            &map_out,
        )
        .unwrap();
        assert!(map_out.join("7").join(SPRITE_MANIFEST_FILE).is_file());
        assert!(!map_out.join("7.png").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conversion_preserves_stable_filename_metadata() {
        let root = temp_dir("convert-stable-names");
        fs::create_dir_all(&root).unwrap();
        let preset = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let payload = sample_payload(preset.asset_kind);
        let first = BinaryNosArchiveWriteEntry::new(42, payload.clone());
        let mut second = BinaryNosArchiveWriteEntry::new(42, payload);
        second.compression = Some(BinaryCompression::Zlib);
        let archive = archive_for_preset(preset, vec![first, second]);
        let out = root.join("out");
        unpack_converted_archive(DetectedArchive::Binary(vec![archive]), &out).unwrap();

        assert!(out.join("42").join(TEXTURE_MANIFEST_FILE).is_file());
        assert!(
            out.join("42__2__zlib")
                .join(TEXTURE_MANIFEST_FILE)
                .is_file()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conversion_failures_leave_no_partial_or_staging_output() {
        let root = temp_dir("convert-transaction-failure");
        fs::create_dir_all(&root).unwrap();
        let preset = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let archive = archive_for_preset(
            preset,
            vec![BinaryNosArchiveWriteEntry::new(
                99,
                b"not a texture".to_vec(),
            )],
        );
        let out = root.join("out");
        let error = unpack_converted_archive(DetectedArchive::Binary(vec![archive]), &out)
            .unwrap_err()
            .to_string();
        assert!(error.contains("NStpData entry at table index 0 with file id 99"));
        assert!(!out.exists());
        assert_no_staging_directories(&root);

        let unknown = BinaryNosArchive::from_entries(
            PathBuf::from("custom.NOS"),
            vec![BinaryNosArchiveWriteEntry::new(1, vec![0])],
            &BinaryNosArchiveWriteOptions::new(
                *b"Unknown NOS fmt!",
                0,
                BinaryCompression::Raw,
                ZlibProfile::default_level(9),
            ),
        )
        .unwrap();
        let unknown_out = root.join("unknown");
        assert!(
            unpack_converted_archive(DetectedArchive::Binary(vec![unknown]), &unknown_out).is_err()
        );
        assert!(!unknown_out.exists());
        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conversion_rejects_existing_output_without_modifying_it() {
        let root = temp_dir("convert-existing-output");
        let out = root.join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("keep.txt"), b"keep").unwrap();
        let preset = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let archive = archive_for_preset(
            preset,
            vec![BinaryNosArchiveWriteEntry::new(
                1,
                sample_payload(preset.asset_kind),
            )],
        );
        let error = unpack_converted_archive(DetectedArchive::Binary(vec![archive]), &out)
            .unwrap_err()
            .to_string();
        assert!(error.contains("output already exists"));
        assert_eq!(fs::read(out.join("keep.txt")).unwrap(), b"keep");
        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn converts_every_supported_text_archive_family() {
        let root = temp_dir("convert-text-families");
        fs::create_dir_all(&root).unwrap();

        let gtd = text_archive(
            "NSgtdData.NOS",
            vec![("Item.dat", 1, encode_dat_payload(b"").unwrap())],
        );
        let gtd_out = root.join("gtd");
        unpack_converted_archive(DetectedArchive::Text(gtd), &gtd_out).unwrap();
        let gtd_json: serde_json::Value =
            serde_json::from_slice(&fs::read(gtd_out.join("Item.json")).unwrap()).unwrap();
        assert_eq!(gtd_json["kind"], "item");

        let language = LanguageTable(vec![
            LanguageEntry("FIRST".to_owned(), "First".to_owned()),
            LanguageEntry("SECOND".to_owned(), "Second".to_owned()),
        ]);
        let lang = text_archive(
            "NSlangData_UK.NOS",
            vec![(
                "_code_uk_Item.txt",
                1,
                encode_language_table(&language, TextEncoding::Windows1252).unwrap(),
            )],
        );
        let lang_out = root.join("lang");
        unpack_converted_archive(DetectedArchive::Text(lang), &lang_out).unwrap();
        assert_eq!(
            serde_json::from_slice::<LanguageTable>(
                &fs::read(lang_out.join("_code_uk_Item.json")).unwrap()
            )
            .unwrap(),
            language
        );

        let constants = ConstStringTable(vec![
            ConstStringEntry(2, "Second".to_owned()),
            ConstStringEntry(1, "First".to_owned()),
        ]);
        let cli = text_archive(
            "NScliData.NOS",
            vec![(
                "conststring.dat",
                1,
                encode_const_string_table(&constants, TextEncoding::EucKr).unwrap(),
            )],
        );
        let cli_out = root.join("cli");
        unpack_converted_archive(DetectedArchive::Text(cli), &cli_out).unwrap();
        assert_eq!(
            serde_json::from_slice::<ConstStringTable>(
                &fs::read(cli_out.join("conststring.json")).unwrap()
            )
            .unwrap(),
            constants
        );

        let dat_strings = NSetcStringList(vec!["one".to_owned(), "two".to_owned()]);
        let lst_strings = NSetcStringList(vec!["three".to_owned(), "four".to_owned()]);
        let etc = text_archive(
            "NSetcData.NOS",
            vec![
                (
                    "MiniGame6WordData.dat",
                    1,
                    encode_nsetc_string_list(
                        &dat_strings,
                        TextPayloadKind::Dat,
                        TextEncoding::EucKr,
                    )
                    .unwrap(),
                ),
                (
                    "TabooStr.lst",
                    0,
                    encode_nsetc_string_list(
                        &lst_strings,
                        TextPayloadKind::List,
                        TextEncoding::EucKr,
                    )
                    .unwrap(),
                ),
            ],
        );
        let etc_out = root.join("etc");
        unpack_converted_archive(DetectedArchive::Text(etc), &etc_out).unwrap();
        assert_eq!(
            serde_json::from_slice::<NSetcStringList>(
                &fs::read(etc_out.join("MiniGame6WordData.json")).unwrap()
            )
            .unwrap(),
            dat_strings
        );
        assert_eq!(
            serde_json::from_slice::<NSetcStringList>(
                &fs::read(etc_out.join("TabooStr.json")).unwrap()
            )
            .unwrap(),
            lst_strings
        );

        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plain_text_conversion_decodes_every_supported_text_family_exactly() {
        let root = temp_dir("convert-plain-text-families");
        fs::create_dir_all(&root).unwrap();

        let gtd_text = b"# header\n% not-a-vnum\n~\n".to_vec();
        let gtd = text_archive(
            "NSgtdData.NOS",
            vec![
                ("kr_abuse.lst", 0, Vec::new()),
                ("npctalk.dat", 1, encode_dat_payload(&gtd_text).unwrap()),
            ],
        );
        let gtd_out = root.join("gtd");
        unpack_converted_plain_text(DetectedArchive::Text(gtd), &gtd_out).unwrap();
        assert_eq!(fs::read(gtd_out.join("kr_abuse.lst")).unwrap(), b"");
        assert_eq!(fs::read(gtd_out.join("npctalk.dat")).unwrap(), gtd_text);
        assert!(!gtd_out.join("npctalk.json").exists());

        let mut language_text = b"KEY\tCaf".to_vec();
        language_text.extend_from_slice(&[0xe9, b'\n']);
        let language = text_archive(
            "NSlangData_UK.NOS",
            vec![(
                "_code_uk_Item name.txt",
                1,
                encode_dat_payload(&language_text).unwrap(),
            )],
        );
        let language_out = root.join("language");
        unpack_converted_plain_text(DetectedArchive::Text(language), &language_out).unwrap();
        assert_eq!(
            fs::read(language_out.join("_code_uk_Item%20name.txt")).unwrap(),
            language_text
        );

        let mut cli_text = b"1\x0bCaf".to_vec();
        cli_text.extend_from_slice(&[0xe9, b'\n']);
        let cli = text_archive(
            "renamed-cli.NOS",
            vec![("conststring.dat", 1, encode_dat_payload(&cli_text).unwrap())],
        );
        let cli_out = root.join("cli");
        unpack_converted_plain_text(DetectedArchive::Text(cli), &cli_out).unwrap();
        assert_eq!(fs::read(cli_out.join("conststring.dat")).unwrap(), cli_text);

        let etc_dat_text = b"one\ntwo\n".to_vec();
        let etc_list_text = b"three\nfour\n".to_vec();
        let etc = text_archive(
            "NSetcData.NOS",
            vec![
                (
                    "MiniGame6WordData.dat",
                    1,
                    encode_dat_payload(&etc_dat_text).unwrap(),
                ),
                (
                    "TabooStr.lst",
                    0,
                    encode_list_payload(&etc_list_text).unwrap(),
                ),
            ],
        );
        let etc_out = root.join("etc");
        unpack_converted_plain_text(DetectedArchive::Text(etc), &etc_out).unwrap();
        assert_eq!(
            fs::read(etc_out.join("MiniGame6WordData.dat")).unwrap(),
            etc_dat_text
        );
        assert_eq!(
            fs::read(etc_out.join("TabooStr.lst")).unwrap(),
            etc_list_text
        );

        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plain_text_conversion_has_no_semantic_warnings() {
        let root = temp_dir("convert-plain-text-no-warnings");
        fs::create_dir_all(&root).unwrap();
        let source = b"# header\n% invalid production row\n";
        let archive = text_archive(
            "NSgtdData.NOS",
            vec![("npctalk.dat", 1, encode_dat_payload(source).unwrap())],
        );
        let plan =
            resolve_text_archive_conversion(&archive, TextArchiveOutputMode::PlainText, None)
                .unwrap();
        let converted = convert_text_archive(&archive, &plan, &root).unwrap();
        assert_eq!(converted.len(), 1);
        assert!(converted[0].warnings.is_empty());
        assert_eq!(fs::read(root.join("npctalk.dat")).unwrap(), source);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plain_text_conversion_preserves_strict_and_transactional_failures() {
        let root = temp_dir("convert-plain-text-failures");
        fs::create_dir_all(&root).unwrap();

        let strict = text_archive(
            "NSgtdData.NOS",
            vec![("unknown.dat", 1, encode_dat_payload(b"text").unwrap())],
        );
        let strict_out = root.join("strict");
        assert!(unpack_converted_plain_text(DetectedArchive::Text(strict), &strict_out).is_err());
        assert!(!strict_out.exists());

        let collision = text_archive(
            "NSgtdData.NOS",
            vec![
                ("Item.dat", 1, encode_dat_payload(b"one").unwrap()),
                ("ITEM.DAT", 1, encode_dat_payload(b"two").unwrap()),
            ],
        );
        let collision_out = root.join("collision");
        let error = unpack_converted_plain_text(DetectedArchive::Text(collision), &collision_out)
            .unwrap_err()
            .to_string();
        assert!(error.contains("both convert to"));
        assert!(!collision_out.exists());

        let malformed = text_archive(
            "NSgtdData.NOS",
            vec![("Item.dat", 1, b"not a DAT payload".to_vec())],
        );
        let malformed_out = root.join("malformed");
        let error = unpack_converted_plain_text(DetectedArchive::Text(malformed), &malformed_out)
            .unwrap_err()
            .to_string();
        assert!(error.contains("NSgtdData record at index 0 with id 1"));
        assert!(!malformed_out.exists());

        let malformed_list = text_archive("NSetcData.NOS", vec![("TabooStr.lst", 0, Vec::new())]);
        let malformed_list_out = root.join("malformed-list");
        let error =
            unpack_converted_plain_text(DetectedArchive::Text(malformed_list), &malformed_list_out)
                .unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains("LST payload is too small"));
        assert!(!malformed_list_out.exists());

        let existing = text_archive(
            "NSgtdData.NOS",
            vec![("Item.dat", 1, encode_dat_payload(b"text").unwrap())],
        );
        let existing_out = root.join("existing");
        fs::create_dir_all(&existing_out).unwrap();
        fs::write(existing_out.join("keep.txt"), b"keep").unwrap();
        let error = unpack_converted_plain_text(DetectedArchive::Text(existing), &existing_out)
            .unwrap_err()
            .to_string();
        assert!(error.contains("output already exists"));
        assert_eq!(fs::read(existing_out.join("keep.txt")).unwrap(), b"keep");

        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn text_conversion_infers_renamed_families_and_applies_encoding_policy() {
        let root = temp_dir("convert-text-encoding");
        fs::create_dir_all(&root).unwrap();

        let korean = LanguageTable(vec![LanguageEntry(
            "GREETING".to_owned(),
            "안녕하세요".to_owned(),
        )]);
        let lang = text_archive(
            "renamed.NOS",
            vec![(
                "_code_kr_Item.txt",
                1,
                encode_language_table(&korean, TextEncoding::EucKr).unwrap(),
            )],
        );
        let lang_out = root.join("renamed-lang");
        unpack_converted_archive(DetectedArchive::Text(lang), &lang_out).unwrap();
        assert_eq!(
            serde_json::from_slice::<LanguageTable>(
                &fs::read(lang_out.join("_code_kr_Item.json")).unwrap()
            )
            .unwrap(),
            korean
        );

        let unknown_language = LanguageTable(vec![LanguageEntry(
            "GREETING".to_owned(),
            "Café".to_owned(),
        )]);
        let unknown_lang = text_archive(
            "NSlangData_ZZ.NOS",
            vec![(
                "_code_zz_Item.txt",
                1,
                encode_language_table(&unknown_language, TextEncoding::Windows1252).unwrap(),
            )],
        );
        let unknown_lang_out = root.join("unknown-lang");
        unpack_converted_archive_with_encoding(
            DetectedArchive::Text(unknown_lang),
            &unknown_lang_out,
            Some("windows-1252"),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<LanguageTable>(
                &fs::read(unknown_lang_out.join("_code_zz_Item.json")).unwrap()
            )
            .unwrap(),
            unknown_language
        );

        let constants = ConstStringTable(vec![ConstStringEntry(1, "Café".to_owned())]);
        let cli_payload = encode_const_string_table(&constants, TextEncoding::Windows1252).unwrap();
        let localized_cli = text_archive(
            "NScliData_UK.NOS",
            vec![("conststring.dat", 1, cli_payload.clone())],
        );
        let localized_cli_out = root.join("localized-cli");
        unpack_converted_archive(DetectedArchive::Text(localized_cli), &localized_cli_out).unwrap();
        assert_eq!(
            serde_json::from_slice::<ConstStringTable>(
                &fs::read(localized_cli_out.join("conststring.json")).unwrap()
            )
            .unwrap(),
            constants
        );

        let unknown_cli = text_archive(
            "NScliData_ZZ.NOS",
            vec![("conststring.dat", 1, cli_payload.clone())],
        );
        let unknown_cli_out = root.join("unknown-cli");
        unpack_converted_archive_with_encoding(
            DetectedArchive::Text(unknown_cli),
            &unknown_cli_out,
            Some("windows-1252"),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<ConstStringTable>(
                &fs::read(unknown_cli_out.join("conststring.json")).unwrap()
            )
            .unwrap(),
            constants
        );

        let cli_without_encoding = text_archive(
            "renamed-cli.NOS",
            vec![("conststring.dat", 1, cli_payload.clone())],
        );
        let missing_out = root.join("missing-cli-encoding");
        let error =
            unpack_converted_archive(DetectedArchive::Text(cli_without_encoding), &missing_out)
                .unwrap_err()
                .to_string();
        assert!(error.contains("renamed-cli.NOS"));
        assert!(error.contains("--encoding"));
        assert!(!missing_out.exists());

        let cli = text_archive("renamed-cli.NOS", vec![("conststring.dat", 1, cli_payload)]);
        let cli_out = root.join("renamed-cli");
        unpack_converted_archive_with_encoding(
            DetectedArchive::Text(cli),
            &cli_out,
            Some("windows-1252"),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<ConstStringTable>(
                &fs::read(cli_out.join("conststring.json")).unwrap()
            )
            .unwrap(),
            constants
        );

        let etc_strings = NSetcStringList(vec!["Café".to_owned()]);
        let etc = text_archive(
            "NSetcData.NOS",
            vec![(
                "MiniGame6WordData.dat",
                1,
                encode_nsetc_string_list(
                    &etc_strings,
                    TextPayloadKind::Dat,
                    TextEncoding::Windows1252,
                )
                .unwrap(),
            )],
        );
        let etc_out = root.join("etc-override");
        unpack_converted_archive_with_encoding(
            DetectedArchive::Text(etc),
            &etc_out,
            Some("windows-1252"),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_slice::<NSetcStringList>(
                &fs::read(etc_out.join("MiniGame6WordData.json")).unwrap()
            )
            .unwrap(),
            etc_strings
        );

        let gtd = text_archive(
            "NSgtdData.NOS",
            vec![("Item.dat", 1, encode_dat_payload(b"").unwrap())],
        );
        let gtd_out = root.join("gtd-override");
        let error = unpack_converted_archive_with_encoding(
            DetectedArchive::Text(gtd),
            &gtd_out,
            Some("windows-1252"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("cannot be used with NSgtdData"));
        assert!(!gtd_out.exists());
        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn strict_text_resolution_applies_to_json_and_plain_text() {
        let root = temp_dir("convert-text-resolution-errors");
        fs::create_dir_all(&root).unwrap();
        let language_payload =
            encode_language_table(&LanguageTable::default(), TextEncoding::Windows1252).unwrap();
        let etc_payload = encode_nsetc_string_list(
            &NSetcStringList::default(),
            TextPayloadKind::Dat,
            TextEncoding::EucKr,
        )
        .unwrap();

        let failures = [
            (
                "unknown",
                text_archive(
                    "NSgtdData.NOS",
                    vec![("unknown.dat", 1, encode_dat_payload(b"").unwrap())],
                ),
                "unknown.dat",
            ),
            (
                "mixed",
                text_archive(
                    "renamed.NOS",
                    vec![
                        ("_code_uk_Item.txt", 1, language_payload.clone()),
                        ("MiniGame6WordData.dat", 1, etc_payload.clone()),
                    ],
                ),
                "record identifies family",
            ),
            (
                "family-conflict",
                text_archive(
                    "NSetcData.NOS",
                    vec![("_code_uk_Item.txt", 1, language_payload.clone())],
                ),
                "archive identifies NSetcData",
            ),
            (
                "locale-conflict",
                text_archive(
                    "NSlangData_DE.NOS",
                    vec![("_code_uk_Item.txt", 1, language_payload.clone())],
                ),
                "records identify \"uk\"",
            ),
            (
                "collision",
                text_archive(
                    "NSgtdData.NOS",
                    vec![
                        ("Item.dat", 1, encode_dat_payload(b"").unwrap()),
                        ("ITEM.DAT", 1, encode_dat_payload(b"").unwrap()),
                    ],
                ),
                "both convert to",
            ),
            (
                "nsetc-kind-conflict",
                text_archive("NSetcData.NOS", vec![("TabooStr.lst", 1, etc_payload)]),
                "requires a LST payload",
            ),
            (
                "renamed-empty",
                text_archive("renamed.NOS", Vec::new()),
                "empty renamed text archive",
            ),
        ];

        for (name, archive, expected) in failures {
            for (mode, plain_text) in [("json", false), ("plain", true)] {
                let out = root.join(format!("{name}-{mode}"));
                let error = unpack_converted_archive_with_options(
                    DetectedArchive::Text(archive.clone()),
                    &out,
                    None,
                    plain_text,
                )
                .unwrap_err();
                let error = format!("{error:#}");
                assert!(
                    error.contains(expected),
                    "missing {expected:?} from {error:?}"
                );
                assert!(!out.exists());
                assert_no_staging_directories(&root);
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_text_conversion_is_transactional_and_errors_have_record_context() {
        let root = temp_dir("convert-text-malformed");
        fs::create_dir_all(&root).unwrap();
        let archive = text_archive(
            "NScliData.NOS",
            vec![("conststring.dat", 1, b"not a DAT payload".to_vec())],
        );
        let out = root.join("out");
        let error = unpack_converted_archive(DetectedArchive::Text(archive), &out)
            .unwrap_err()
            .to_string();
        for expected in [
            "NScliData",
            "NScliData.NOS",
            "index 0",
            "id 1",
            "conststring.dat",
        ] {
            assert!(
                error.contains(expected),
                "missing {expected:?} from {error:?}"
            );
        }
        assert!(!out.exists());
        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn text_decoder_warnings_remain_nonfatal() {
        let root = temp_dir("convert-text-warning");
        fs::create_dir_all(&root).unwrap();
        let archive = text_archive(
            "NSlangData_UK.NOS",
            vec![(
                "_code_uk_Item.txt",
                1,
                encode_dat_payload(b"malformed row\nKEY\tvalue\n").unwrap(),
            )],
        );
        let plan =
            resolve_text_archive_conversion(&archive, TextArchiveOutputMode::Json, None).unwrap();
        let converted = convert_text_archive(&archive, &plan, &root).unwrap();
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].warnings.len(), 1);
        assert!(converted[0].warnings[0].contains("skipping malformed NSlang row"));
        assert_eq!(
            serde_json::from_slice::<LanguageTable>(
                &fs::read(root.join("_code_uk_Item.json")).unwrap()
            )
            .unwrap(),
            LanguageTable(vec![LanguageEntry("KEY".to_owned(), "value".to_owned())])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn raw_text_unpack_and_converted_sound_layout_are_unchanged() {
        let root = temp_dir("raw-text-converted-sound");
        fs::create_dir_all(&root).unwrap();
        let payload = encode_dat_payload(b"encoded text").unwrap();
        let text = text_archive("NSgtdData.NOS", vec![("Item.dat", 1, payload.clone())]);
        let text_out = root.join("text");
        unpack_text_archive(&text, &text_out).unwrap();
        assert_eq!(fs::read(text_out.join("Item.dat")).unwrap(), payload);

        let mut sound_header = [0; DELDX_PACK_HEADER_LEN];
        sound_header[0] = 16;
        sound_header[1..17].copy_from_slice(b"DelDX Pack File ");
        sound_header[0x14..0x18].copy_from_slice(&10_i32.to_le_bytes());
        let sound = DelDxPack::empty("snd.pck", &DelDxPackWriteOptions::new(sound_header)).unwrap();
        let sound_out = root.join("sound");
        unpack_converted_archive(DetectedArchive::Sound(sound), &sound_out).unwrap();
        assert!(sound_out.join(SOUND_PACK_MANIFEST_FILE).is_file());
        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn encoding_is_rejected_for_binary_and_sound_conversion() {
        let root = temp_dir("convert-non-text-encoding");
        fs::create_dir_all(&root).unwrap();
        let preset = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let binary = archive_for_preset(
            preset,
            vec![BinaryNosArchiveWriteEntry::new(
                1,
                sample_payload(preset.asset_kind),
            )],
        );
        let binary_out = root.join("binary");
        let error = unpack_converted_archive_with_encoding(
            DetectedArchive::Binary(vec![binary]),
            &binary_out,
            Some("windows-1252"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("not a binary archive"));
        assert!(!binary_out.exists());

        let mut sound_header = [0; DELDX_PACK_HEADER_LEN];
        sound_header[0] = 16;
        sound_header[1..17].copy_from_slice(b"DelDX Pack File ");
        sound_header[0x14..0x18].copy_from_slice(&10_i32.to_le_bytes());
        let sound = DelDxPack::empty("snd.pck", &DelDxPackWriteOptions::new(sound_header)).unwrap();
        let sound_out = root.join("sound");
        let error = unpack_converted_archive_with_encoding(
            DetectedArchive::Sound(sound),
            &sound_out,
            Some("windows-1252"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("not a sound pack"));
        assert!(!sound_out.exists());
        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plain_text_is_rejected_for_binary_and_sound_conversion() {
        let root = temp_dir("convert-non-text-plain");
        fs::create_dir_all(&root).unwrap();
        let preset = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let binary = archive_for_preset(
            preset,
            vec![BinaryNosArchiveWriteEntry::new(
                1,
                sample_payload(preset.asset_kind),
            )],
        );
        let binary_out = root.join("binary");
        let error = unpack_converted_plain_text(DetectedArchive::Binary(vec![binary]), &binary_out)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a binary archive"));
        assert!(!binary_out.exists());

        let mut sound_header = [0; DELDX_PACK_HEADER_LEN];
        sound_header[0] = 16;
        sound_header[1..17].copy_from_slice(b"DelDX Pack File ");
        sound_header[0x14..0x18].copy_from_slice(&10_i32.to_le_bytes());
        let sound = DelDxPack::empty("snd.pck", &DelDxPackWriteOptions::new(sound_header)).unwrap();
        let sound_out = root.join("sound");
        let error = unpack_converted_plain_text(DetectedArchive::Sound(sound), &sound_out)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a sound pack"));
        assert!(!sound_out.exists());
        assert_no_staging_directories(&root);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn raw_binary_unpack_behavior_is_unchanged() {
        let root = temp_dir("raw-unpack-unchanged");
        let preset = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let payload = sample_payload(preset.asset_kind);
        let archive = archive_for_preset(
            preset,
            vec![BinaryNosArchiveWriteEntry::new(42, payload.clone())],
        );
        unpack_binary_archives(&[archive], &root).unwrap();
        assert_eq!(fs::read(root.join("42.bin")).unwrap(), payload);
        assert!(!root.join("42").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
