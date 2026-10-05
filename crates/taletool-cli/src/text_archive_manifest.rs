//! Version-date manifest for raw text archive unpack/pack.
//!
//! Raw text unpacking writes one file per record. The archive's data-version
//! trailer belongs to no record, so it is kept in `text-archive.json` beside
//! the payloads and restored when the directory is packed.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_archive::TextNosArchive;

pub(crate) const TEXT_ARCHIVE_MANIFEST_FILE: &str = "text-archive.json";
const TEXT_ARCHIVE_FORMAT: &str = "text";
const TEXT_ARCHIVE_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextArchiveManifest {
    format: String,
    version: u32,
    /// Delphi `TDateTime` from the data-version trailer, or `null` when the
    /// archive has none.
    version_date: Option<f64>,
}

/// Write `text-archive.json` with the archive's data-version date.
pub(crate) fn write_text_archive_manifest(
    archive: &TextNosArchive,
    out: &Path,
) -> anyhow::Result<()> {
    let mut version_date = archive.timestamp().map(|timestamp| timestamp.variant);
    if version_date.is_some_and(|value| !value.is_finite()) {
        eprintln!(
            "warning: {} has a non-finite data-version date that JSON cannot store; \
             packing this directory writes no data-version trailer",
            archive.path().display()
        );
        version_date = None;
    }
    let manifest = TextArchiveManifest {
        format: TEXT_ARCHIVE_FORMAT.to_owned(),
        version: TEXT_ARCHIVE_MANIFEST_VERSION,
        version_date,
    };
    let path = out.join(TEXT_ARCHIVE_MANIFEST_FILE);
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Read the data-version date recorded in an unpacked text archive directory.
///
/// A directory without `text-archive.json` yields `None` with a warning,
/// because the packed archive then has no data-version trailer.
pub(crate) fn read_text_archive_version_date(dir: &Path) -> anyhow::Result<Option<f64>> {
    let path = dir.join(TEXT_ARCHIVE_MANIFEST_FILE);
    if !path.is_file() {
        eprintln!(
            "warning: {} has no {TEXT_ARCHIVE_MANIFEST_FILE}; the packed archive has no \
             data-version trailer, which NosTale reads as 2004-12-11 12:00",
            dir.display()
        );
        return Ok(None);
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
    if manifest.version != TEXT_ARCHIVE_MANIFEST_VERSION {
        bail!(
            "text archive manifest has unsupported version {}; expected {}",
            manifest.version,
            TEXT_ARCHIVE_MANIFEST_VERSION
        );
    }
    Ok(manifest.version_date)
}

pub(crate) fn text_archive_manifest_exists(dir: &Path) -> bool {
    dir.join(TEXT_ARCHIVE_MANIFEST_FILE).is_file()
}
