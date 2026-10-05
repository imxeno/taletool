//! Manifest-backed raw text archive unpack/pack.
//!
//! Raw text unpacking writes one file per record. `text-archive.json` keeps
//! everything those files cannot: record order, stored IDs, name bytes,
//! packed flags, bytes after the last record, and the data-version date. A
//! directory without a manifest still packs, with regenerated metadata.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_archive::{TextNosArchive, TextNosArchiveWriteOptions, TextNosRecordInput};

use crate::paths::{escape_archive_name, immediate_files, unescape_archive_name};
use crate::text_payload::packed_flag_for_text_record;

pub(crate) const TEXT_ARCHIVE_MANIFEST_FILE: &str = "text-archive.json";
const TEXT_ARCHIVE_FORMAT: &str = "text";
const TEXT_ARCHIVE_MANIFEST_VERSION: u32 = 2;
/// Manifests written before records were tracked only kept the version date.
const VERSION_DATE_ONLY_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextArchiveManifest {
    format: String,
    version: u32,
    /// Delphi `TDateTime` from the data-version trailer, or `null` when the
    /// archive has none.
    version_date: Option<f64>,
    /// Unrecognized bytes between the last record and the trailer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    trailing_hex: String,
    /// Records in stored order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    records: Option<Vec<TextArchiveManifestRecord>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextArchiveManifestRecord {
    /// Payload file in the unpacked directory.
    file: String,
    id: i32,
    /// Record name, when its bytes are valid UTF-8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Record name bytes, when they are not valid UTF-8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name_hex: Option<String>,
    packed_flag: i32,
}

/// Records and archive-level bytes read back from an unpacked directory.
pub(crate) struct UnpackedTextArchive {
    pub(crate) records: Vec<TextNosRecordInput>,
    pub(crate) options: TextNosArchiveWriteOptions,
}

/// Write every record payload and `text-archive.json` into `out`.
///
/// Returns each record's stored ID and payload file in stored order. Records
/// whose names collide, or that would overwrite the manifest, get numbered
/// file names; the manifest keeps their original names.
pub(crate) fn unpack_text_archive_dir(
    archive: &TextNosArchive,
    out: &Path,
) -> anyhow::Result<Vec<(i32, PathBuf)>> {
    let mut version_date = archive.timestamp().map(|timestamp| timestamp.variant);
    if version_date.is_some_and(|value| !value.is_finite()) {
        eprintln!(
            "warning: {} has a non-finite data-version date that JSON cannot store; \
             packing this directory writes no data-version trailer",
            archive.path().display()
        );
        version_date = None;
    }

    fs::create_dir_all(out)?;
    let mut used_files = BTreeSet::from([TEXT_ARCHIVE_MANIFEST_FILE.to_owned()]);
    let mut written = Vec::with_capacity(archive.records().len());
    let mut records = Vec::with_capacity(archive.records().len());
    for record in archive.records() {
        let file = unique_payload_file_name(&record.name, &mut used_files);
        fs::write(out.join(&file), &record.payload)?;
        let (name, name_hex) = match std::str::from_utf8(&record.name_bytes) {
            Ok(name) => (Some(name.to_owned()), None),
            Err(_) => (None, Some(hex::encode(&record.name_bytes))),
        };
        written.push((record.id, PathBuf::from(&file)));
        records.push(TextArchiveManifestRecord {
            file,
            id: record.id,
            name,
            name_hex,
            packed_flag: record.packed_flag,
        });
    }

    let manifest = TextArchiveManifest {
        format: TEXT_ARCHIVE_FORMAT.to_owned(),
        version: TEXT_ARCHIVE_MANIFEST_VERSION,
        version_date,
        trailing_hex: hex::encode(archive.trailing_data()),
        records: Some(records),
    };
    let path = out.join(TEXT_ARCHIVE_MANIFEST_FILE);
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(written)
}

/// Read an unpacked text archive directory back into records.
pub(crate) fn read_unpacked_text_archive(dir: &Path) -> anyhow::Result<UnpackedTextArchive> {
    let path = dir.join(TEXT_ARCHIVE_MANIFEST_FILE);
    if !path.is_file() {
        eprintln!(
            "warning: {} has no {TEXT_ARCHIVE_MANIFEST_FILE}; record IDs, order, and packed \
             flags are regenerated, and the packed archive has no data-version trailer, \
             which NosTale reads as 2004-12-11 12:00",
            dir.display()
        );
        return read_unlisted_records(dir, TextNosArchiveWriteOptions::default());
    }

    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let manifest: TextArchiveManifest =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    if manifest.format != TEXT_ARCHIVE_FORMAT {
        bail!(
            "text archive manifest has unsupported format {:?}; expected {:?}",
            manifest.format,
            TEXT_ARCHIVE_FORMAT
        );
    }
    let options = TextNosArchiveWriteOptions {
        trailing_data: hex::decode(&manifest.trailing_hex).context("decoding trailing_hex")?,
        version_date: manifest.version_date,
    };
    match (manifest.version, manifest.records) {
        (VERSION_DATE_ONLY_MANIFEST_VERSION, None) if options.trailing_data.is_empty() => {
            eprintln!(
                "warning: {} is a version {VERSION_DATE_ONLY_MANIFEST_VERSION} manifest; \
                 record IDs, order, and packed flags are regenerated",
                path.display()
            );
            read_unlisted_records(dir, options)
        }
        (TEXT_ARCHIVE_MANIFEST_VERSION, Some(records)) => read_listed_records(dir, &records)
            .map(|records| UnpackedTextArchive { records, options }),
        (TEXT_ARCHIVE_MANIFEST_VERSION, None) => {
            bail!("text archive manifest version {TEXT_ARCHIVE_MANIFEST_VERSION} needs records")
        }
        (version, _) => bail!(
            "text archive manifest has unsupported version {version}; expected {}",
            TEXT_ARCHIVE_MANIFEST_VERSION
        ),
    }
}

pub(crate) fn text_archive_manifest_exists(dir: &Path) -> bool {
    dir.join(TEXT_ARCHIVE_MANIFEST_FILE).is_file()
}

fn read_listed_records(
    dir: &Path,
    records: &[TextArchiveManifestRecord],
) -> anyhow::Result<Vec<TextNosRecordInput>> {
    let mut listed_files = BTreeSet::new();
    let mut inputs = Vec::with_capacity(records.len());
    for (position, record) in records.iter().enumerate() {
        let context = || format!("text archive manifest record {position}");
        if record.file == TEXT_ARCHIVE_MANIFEST_FILE || !is_plain_file_name(&record.file) {
            bail!(
                "{}: file must be a payload filename: {:?}",
                context(),
                record.file
            );
        }
        let name_bytes = match (&record.name, &record.name_hex) {
            (Some(name), None) => name.as_bytes().to_vec(),
            (None, Some(name_hex)) => hex::decode(name_hex)
                .with_context(|| format!("{}: decoding name_hex", context()))?,
            _ => bail!("{}: needs exactly one of name or name_hex", context()),
        };
        let path = dir.join(&record.file);
        let payload = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        listed_files.insert(record.file.clone());
        inputs.push(TextNosRecordInput {
            id: record.id,
            name: String::from_utf8_lossy(&name_bytes).into_owned(),
            name_bytes,
            packed_flag: record.packed_flag,
            payload,
        });
    }

    for path in immediate_files(dir)? {
        let file = path.file_name().and_then(|name| name.to_str());
        if file
            .is_some_and(|file| file == TEXT_ARCHIVE_MANIFEST_FILE || listed_files.contains(file))
        {
            continue;
        }
        bail!(
            "{} is not listed in {TEXT_ARCHIVE_MANIFEST_FILE}; add it to records or remove it",
            path.display()
        );
    }
    Ok(inputs)
}

/// Pack every file as a record, sorted by name with sequential IDs.
fn read_unlisted_records(
    dir: &Path,
    options: TextNosArchiveWriteOptions,
) -> anyhow::Result<UnpackedTextArchive> {
    let mut records = Vec::new();
    for path in immediate_files(dir)? {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid UTF-8 file name: {}", path.display()))?;
        if file_name == TEXT_ARCHIVE_MANIFEST_FILE {
            continue;
        }
        let name = unescape_archive_name(file_name)?;
        let payload = fs::read(&path)?;
        records.push(TextNosRecordInput {
            id: 0,
            packed_flag: packed_flag_for_text_record(&name),
            name_bytes: name.as_bytes().to_vec(),
            name,
            payload,
        });
    }
    records.sort_by_key(|record| record.name.to_lowercase());
    for (index, record) in records.iter_mut().enumerate() {
        record.id = i32::try_from(index + 1).context("too many text records")?;
    }
    Ok(UnpackedTextArchive { records, options })
}

/// Pick a payload file name that no earlier record or the manifest uses.
///
/// Names are compared case-insensitively because common filesystems are.
fn unique_payload_file_name(name: &str, used: &mut BTreeSet<String>) -> String {
    let escaped = match escape_archive_name(name) {
        escaped if escaped.is_empty() || escaped == "." || escaped == ".." => "unnamed".to_owned(),
        escaped => escaped,
    };
    let (stem, extension) = match escaped.rfind('.') {
        Some(dot) if dot > 0 => escaped.split_at(dot),
        _ => (escaped.as_str(), ""),
    };
    let mut candidate = escaped.clone();
    let mut ordinal = 2;
    while !used.insert(candidate.to_ascii_lowercase()) {
        candidate = format!("{stem}__{ordinal}{extension}");
        ordinal += 1;
    }
    candidate
}

fn is_plain_file_name(file: &str) -> bool {
    let mut components = Path::new(file).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}
