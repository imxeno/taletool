//! Handlers for `taletool archive` commands.
//!
//! This module owns CLI orchestration for full archive containers: resolving
//! inputs, choosing a parser, printing inspection output, and translating
//! unpacked directory layouts back into archive writer inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use taletool_archive::{
    BinaryCompression, BinaryNosArchive, BinaryNosArchiveWriteOptions, DelDxPack, TextNosArchive,
    TextNosRecordInput, write_text_nos_archive_bytes,
};

use anyhow::Context;
use taletool_texture::decode_texture;
use taletool_texture::sprite::decode_sprite;
use taletool_texture::sprite::free_size::decode_free_size_sprite;

use crate::archive_detect::{DetectedArchive, detect_archive_paths, has_ccinf_header};
use crate::binary_payloads::{
    BinaryPayloadInput, binary_payload_output_name, explicit_indexes_for_binary_ids,
    order_binary_payload_entries, parse_binary_payload_filename, parse_id_filename,
};
use crate::binary_preset::{
    binary_nos_archive_default_compression, format_chunk_pattern, output_pattern, parse_header_hex,
    resolve_binary_preset, resolve_zlib_profile,
};
use crate::cli::{ArchiveCommand, ArchiveType, ChunkingArg, CompressionArg, ConvertKind};
use crate::paths::{escape_archive_name, immediate_files, resolve_inputs, unescape_archive_name};
use crate::sound_pack::{
    pack_sound_pack_dir as build_sound_pack_dir, sound_pack_manifest_exists, unpack_sound_pack,
};
use crate::sprite_file::{unpack_free_size_sprite_png, unpack_sprite_file};
use crate::text_payload::{packed_flag_for_text_record, payload_kind_label};
use crate::texture_file::unpack_texture_file;
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
        } => {
            let paths = resolve_inputs(&input)?;
            reject_ccinf_inputs(&paths, "unpack")?;
            let detected = detect_archive_paths(&paths, archive_type)?;
            match detected {
                DetectedArchive::Binary(archives) => {
                    unpack_binary_archives(&archives, &out, convert)
                }
                DetectedArchive::Text(archive) => {
                    if let Some(kind) = convert {
                        eprintln!("warning: --convert {kind} is not supported for text archives, extracting raw records");
                    }
                    unpack_text_archive(&archive, &out)
                }
                DetectedArchive::Sound(archive) => {
                    if let Some(kind) = convert {
                        eprintln!("warning: --convert {kind} is not supported for sound pack archives, extracting raw entries");
                    }
                    unpack_sound_pack_archive(&archive, &out)
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

/// Extract binary archive payloads with stable filenames.
fn unpack_binary_archives(
    archives: &[BinaryNosArchive],
    out: &Path,
    convert: Option<ConvertKind>,
) -> anyhow::Result<()> {
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

            if let Some(convert_kind) = convert {
                let result = convert_payload(&payload.data, convert_kind, out, &file_name)
                    .with_context(|| format!("converting file id {}", entry.file_id))?;
                println!("{}", result.log_line(entry.file_id));
            } else {
                let path = out.join(&file_name);
                fs::write(&path, payload.data)?;
                println!("unpacked {:<12} {}", entry.file_id, path.display());
            }
            count += 1;
        }
    }
    if convert.is_some() {
        println!("converted {count} payloads into {}", out.display());
    } else {
        println!("unpacked {count} payloads into {}", out.display());
    }
    Ok(())
}

/// Outcome of converting a single binary archive entry.
enum ConvertedPayload {
    /// Entry decoded as a texture; mip PNGs written inside `dir`.
    Texture {
        dir: PathBuf,
        mip_count: usize,
    },
    /// Entry decoded as a map-object sprite; frame PNGs written inside `dir`.
    MapObjectSprite {
        dir: PathBuf,
        frame_count: usize,
    },
    /// Entry decoded as a free-size sprite; single PNG written to `path`.
    FreeSizeSprite {
        path: PathBuf,
        width: u32,
        height: u32,
    },
    /// Entry could not be decoded; raw bytes written to `path`.
    Raw {
        path: PathBuf,
    },
}

impl ConvertedPayload {
    fn log_line(&self, file_id: i32) -> String {
        match self {
            Self::Texture { dir, mip_count } => {
                format!(
                    "unpacked {:<12} {} (texture, {mip_count} mips)",
                    file_id,
                    dir.display()
                )
            }
            Self::MapObjectSprite { dir, frame_count } => {
                format!(
                    "unpacked {:<12} {} (sprite, {frame_count} frames)",
                    file_id,
                    dir.display()
                )
            }
            Self::FreeSizeSprite {
                path,
                width,
                height,
            } => {
                format!(
                    "unpacked {:<12} {} (free-size sprite, {width}x{height})",
                    file_id,
                    path.display()
                )
            }
            Self::Raw { path } => {
                format!("unpacked {:<12} {} (raw)", file_id, path.display())
            }
        }
    }
}

/// Reject filenames that could escape the output directory.
fn sanitize_file_name(name: &str) -> anyhow::Result<&str> {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        anyhow::bail!("invalid file name component in output name: {name:?}");
    }
    Ok(name)
}

/// Decode a raw archive payload according to `kind` and write PNG output.
fn convert_payload(
    data: &[u8],
    kind: ConvertKind,
    out_dir: &Path,
    file_name: &str,
) -> anyhow::Result<ConvertedPayload> {
    let stem = sanitize_file_name(file_name.strip_suffix(".bin").unwrap_or(file_name))?;
    fs::create_dir_all(out_dir)?;

    match kind {
        ConvertKind::Texture => {
            let texture = decode_texture(data)?;
            let dir = out_dir.join(stem);
            let mip_count = unpack_texture_file(&texture, &dir)
                .with_context(|| format!("writing texture output to {}", dir.display()))?;
            Ok(ConvertedPayload::Texture { dir, mip_count })
        }
        ConvertKind::Sprite => {
            if let Ok(sprite) = decode_sprite(data) {
                let dir = out_dir.join(stem);
                let frame_count = unpack_sprite_file(&sprite, &dir)
                    .with_context(|| format!("writing sprite output to {}", dir.display()))?;
                Ok(ConvertedPayload::MapObjectSprite { dir, frame_count })
            } else {
                let free_sprite = decode_free_size_sprite(data)
                    .with_context(|| "data is not a valid map-object sprite either")?;
                let path = out_dir.join(format!("{stem}.png"));
                unpack_free_size_sprite_png(&free_sprite, &path)
                    .with_context(|| format!("writing free-size sprite to {}", path.display()))?;
                Ok(ConvertedPayload::FreeSizeSprite {
                    width: free_sprite.width(),
                    height: free_sprite.height(),
                    path,
                })
            }
        }
        ConvertKind::Auto => {
            if let Ok(texture) = decode_texture(data) {
                let dir = out_dir.join(stem);
                let mip_count = unpack_texture_file(&texture, &dir)?;
                return Ok(ConvertedPayload::Texture { dir, mip_count });
            }
            if let Ok(sprite) = decode_sprite(data) {
                let dir = out_dir.join(stem);
                let frame_count = unpack_sprite_file(&sprite, &dir)?;
                return Ok(ConvertedPayload::MapObjectSprite { dir, frame_count });
            }
            if let Ok(free_sprite) = decode_free_size_sprite(data) {
                let path = out_dir.join(format!("{stem}.png"));
                unpack_free_size_sprite_png(&free_sprite, &path)?;
                return Ok(ConvertedPayload::FreeSizeSprite {
                    width: free_sprite.width(),
                    height: free_sprite.height(),
                    path,
                });
            }
            let path = out_dir.join(stem);
            fs::write(&path, data)?;
            Ok(ConvertedPayload::Raw { path })
        }
    }
}

/// Extract text archive records using escaped archive names.
fn unpack_text_archive(archive: &TextNosArchive, out: &Path) -> anyhow::Result<()> {
    let duplicates = duplicate_id_counts(archive.records().iter().map(|record| record.id));
    warn_duplicate_archive_ids(
        "unpacking",
        "text",
        &duplicates,
        "IDs are ambiguous for id-based lookup; files are written by archived name.",
    );

    fs::create_dir_all(out)?;
    for record in archive.records() {
        let path = out.join(escape_archive_name(&record.name));
        fs::write(&path, &record.payload)?;
        println!("unpacked {:<4} {}", record.id, path.display());
    }
    println!(
        "unpacked {} text records into {}",
        archive.records().len(),
        out.display()
    );
    Ok(())
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

    use image::GenericImageView;
    use image::RgbaImage;
    use taletool_archive::BinaryNosArchiveWriteEntry;
    use taletool_ccinf::CCINF_HEADER;
    use taletool_texture::{TextureFormat, TextureHeader, write_texture_bytes};
    use taletool_zlib::{ZlibProfile, ZlibStrategy};

    use crate::cli::ConvertKind;
    use crate::texture_file::TEXTURE_MANIFEST_FILE;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn make_texture_payload(width: u16, height: u16) -> Vec<u8> {
        let header = TextureHeader {
            width,
            height,
            format: TextureFormat::A8R8G8B8,
            filter_flag: 0,
            unknown_06: 0,
            mip_level_count: 1,
        };
        let mip_levels = vec![RgbaImage::new(width.into(), height.into())];
        write_texture_bytes(&header, &mip_levels).unwrap()
    }

    fn make_binary_archive(entries: Vec<BinaryNosArchiveWriteEntry>) -> BinaryNosArchive {
        BinaryNosArchive::from_entries(
            PathBuf::from("<test>"),
            entries,
            &BinaryNosArchiveWriteOptions::new(
                [0; 16],
                0,
                BinaryCompression::Raw,
                ZlibProfile {
                    level: 0,
                    strategy: ZlibStrategy::Default,
                },
            ),
        )
        .unwrap()
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
    fn unpack_binary_archive_with_texture_convert() {
        let root = temp_dir("binary-texture-convert");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            42,
            make_texture_payload(4, 4),
        )]);
        unpack_binary_archives(&[archive], &output, Some(ConvertKind::Texture)).unwrap();

        let tex_dir = output.join("42");
        assert!(tex_dir.is_dir(), "texture output directory should exist");
        assert!(
            tex_dir.join("mip-000.png").is_file(),
            "mip-000.png should exist"
        );
        assert!(
            tex_dir.join(TEXTURE_MANIFEST_FILE).is_file(),
            "texture.json should exist"
        );

        let img = image::open(tex_dir.join("mip-000.png")).unwrap();
        assert_eq!(img.dimensions(), (4, 4));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_with_texture_auto_convert() {
        let root = temp_dir("binary-texture-auto");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            7,
            make_texture_payload(2, 2),
        )]);
        unpack_binary_archives(&[archive], &output, Some(ConvertKind::Auto)).unwrap();

        let tex_dir = output.join("7");
        assert!(tex_dir.is_dir());
        assert!(tex_dir.join("mip-000.png").is_file());
        assert!(tex_dir.join(TEXTURE_MANIFEST_FILE).is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_without_convert_writes_raw() {
        let root = temp_dir("binary-no-convert");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            99,
            b"hello world".to_vec(),
        )]);
        unpack_binary_archives(&[archive], &output, None).unwrap();

        let raw = output.join("99.bin");
        assert!(raw.is_file());
        assert_eq!(fs::read(&raw).unwrap(), b"hello world");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_auto_convert_falls_back_to_raw() {
        let root = temp_dir("binary-auto-fallback");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            1,
            b"not a valid texture or sprite".to_vec(),
        )]);
        unpack_binary_archives(&[archive], &output, Some(ConvertKind::Auto)).unwrap();

        let raw = output.join("1");
        assert!(raw.is_file(), "raw fallback should use stem, not .bin extension");
        assert_eq!(fs::read(&raw).unwrap(), b"not a valid texture or sprite");

        fs::remove_dir_all(root).unwrap();
    }

    fn make_map_object_sprite_payload() -> Vec<u8> {
        use taletool_texture::sprite::{SpriteFrame, write_sprite_bytes};
        let image = RgbaImage::new(4, 4);
        write_sprite_bytes(&[SpriteFrame::new(0, 0, image)]).unwrap()
    }

    fn make_free_size_sprite_payload() -> Vec<u8> {
        use taletool_texture::sprite::free_size::write_free_size_sprite_bytes;
        write_free_size_sprite_bytes(&RgbaImage::new(8, 8)).unwrap()
    }

    #[test]
    fn unpack_binary_archive_with_sprite_map_object_convert() {
        let root = temp_dir("binary-sprite-map-object");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            5,
            make_map_object_sprite_payload(),
        )]);
        unpack_binary_archives(&[archive], &output, Some(ConvertKind::Sprite)).unwrap();

        let dir = output.join("5");
        assert!(dir.is_dir());
        assert!(dir.join("frame-000.png").is_file());
        assert!(dir.join("sprite.json").is_file());
        let img = image::open(dir.join("frame-000.png")).unwrap();
        assert_eq!(img.dimensions(), (4, 4));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_with_sprite_free_size_convert() {
        let root = temp_dir("binary-sprite-free-size");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            3,
            make_free_size_sprite_payload(),
        )]);
        unpack_binary_archives(&[archive], &output, Some(ConvertKind::Sprite)).unwrap();

        let png = output.join("3.png");
        assert!(png.is_file());
        let img = image::open(&png).unwrap();
        assert_eq!(img.dimensions(), (8, 8));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_with_auto_sprite_convert() {
        let root = temp_dir("binary-auto-sprite");
        let output = root.join("output");

        let archive = make_binary_archive(vec![
            BinaryNosArchiveWriteEntry::new(1, make_map_object_sprite_payload()),
            BinaryNosArchiveWriteEntry::new(2, make_free_size_sprite_payload()),
        ]);
        unpack_binary_archives(&[archive], &output, Some(ConvertKind::Auto)).unwrap();

        assert!(output.join("1").join("frame-000.png").is_file());
        assert!(output.join("2.png").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_with_texture_convert_error() {
        let root = temp_dir("binary-texture-convert-error");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            1,
            b"not a texture".to_vec(),
        )]);
        let result = unpack_binary_archives(&[archive], &output, Some(ConvertKind::Texture));
        assert!(result.is_err(), "texture convert should error on invalid data");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_with_sprite_convert_error() {
        let root = temp_dir("binary-sprite-convert-error");
        let output = root.join("output");

        let archive = make_binary_archive(vec![BinaryNosArchiveWriteEntry::new(
            1,
            b"not a sprite".to_vec(),
        )]);
        let result = unpack_binary_archives(&[archive], &output, Some(ConvertKind::Sprite));
        assert!(result.is_err(), "sprite convert should error on invalid data");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unpack_binary_archive_with_mixed_entries() {
        let root = temp_dir("binary-mixed-entries");
        let output = root.join("output");

        let archive = make_binary_archive(vec![
            BinaryNosArchiveWriteEntry::new(10, make_texture_payload(2, 2)),
            BinaryNosArchiveWriteEntry::new(20, make_free_size_sprite_payload()),
            BinaryNosArchiveWriteEntry::new(30, b"raw data".to_vec()),
        ]);
        unpack_binary_archives(&[archive], &output, Some(ConvertKind::Auto)).unwrap();

        assert!(output.join("10").join("mip-000.png").is_file());
        assert!(output.join("20.png").is_file());
        assert!(output.join("30").is_file());
        assert_eq!(fs::read(output.join("30")).unwrap(), b"raw data");

        fs::remove_dir_all(root).unwrap();
    }
}
