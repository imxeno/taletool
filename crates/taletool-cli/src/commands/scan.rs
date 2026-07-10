//! Handler for `taletool scan`.
//!
//! Scanning is intentionally shallow: it walks one data directory, attempts
//! format detection for supported data files, and reports a concise
//! classification.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use taletool_ccinf::Ccinf;
use taletool_core::validate_data_dir;

use crate::archive_detect::{DetectedArchive, detect_archive_paths, has_ccinf_header};
use crate::cli::ArchiveType;

/// Scan a data directory and print either human-readable or JSON output.
pub(crate) fn run_scan(data_dir: PathBuf, verbose: bool, json_output: bool) -> anyhow::Result<()> {
    let data_dir = validate_data_dir(data_dir)?;
    let mut files = Vec::new();
    for entry in fs::read_dir(&data_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !is_scannable_data_file(&path) {
            continue;
        }
        files.push(scan_file(&path, verbose));
    }
    files.sort_by(|left, right| left.file.cmp(&right.file));

    if json_output {
        println!("{}", serde_json::to_string_pretty(&files)?);
    } else {
        println!("data_dir: {}", data_dir.display());
        println!("files: {}", files.len());
        for file in &files {
            match &file.details {
                Some(details) => println!(
                    "  {:<24} type={:<7} {}",
                    file.file,
                    file.archive_type.as_deref().unwrap_or("unknown"),
                    details
                ),
                None => println!(
                    "  {:<24} type={}",
                    file.file,
                    file.archive_type.as_deref().unwrap_or("unknown")
                ),
            }
        }
    }
    Ok(())
}

/// Serializable result for one file found during `scan`.
#[derive(Debug, Serialize)]
struct ScanFile {
    file: String,
    archive_type: Option<String>,
    details: Option<String>,
    error: Option<String>,
}

/// Classify a supported data file using the same format policy as commands.
fn scan_file(path: &Path, verbose: bool) -> ScanFile {
    let file = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    if has_ccinf_header(path) {
        return match Ccinf::open(path) {
            Ok(ccinf) => ScanFile {
                file,
                archive_type: Some("ccinf".to_owned()),
                details: verbose.then(|| format!("entries={}", ccinf.entries().len())),
                error: None,
            },
            Err(error) => ScanFile {
                file,
                archive_type: Some("ccinf".to_owned()),
                details: None,
                error: Some(error.to_string()),
            },
        };
    }

    match detect_archive_paths(&[path.to_path_buf()], ArchiveType::Auto) {
        Ok(DetectedArchive::Binary(archives)) => {
            let entry_count: usize = archives.iter().map(|archive| archive.entries().len()).sum();
            ScanFile {
                file,
                archive_type: Some("binary".to_owned()),
                details: verbose
                    .then(|| format!("chunks={} entries={entry_count}", archives.len())),
                error: None,
            }
        }
        Ok(DetectedArchive::Text(archive)) => ScanFile {
            file,
            archive_type: Some("text".to_owned()),
            details: verbose.then(|| format!("records={}", archive.records().len())),
            error: None,
        },
        Ok(DetectedArchive::Sound(archive)) => ScanFile {
            file,
            archive_type: Some("sound".to_owned()),
            details: verbose.then(|| format!("entries={}", archive.entries().len())),
            error: None,
        },
        Err(error) => ScanFile {
            file,
            archive_type: None,
            details: None,
            error: Some(error.to_string()),
        },
    }
}

fn is_scannable_data_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            extension.eq_ignore_ascii_case("nos") || extension.eq_ignore_ascii_case("pck")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use taletool_ccinf::write_ccinf_bytes;

    use super::*;

    #[test]
    fn scan_classifies_ccinf_as_a_supported_data_file() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "taletool-ccinf-scan-{}-{nanos}",
            std::process::id()
        ));
        let path = root.join("NSmnData.NOS");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, write_ccinf_bytes(&[]).unwrap()).unwrap();

        let scanned = scan_file(&path, true);
        assert_eq!(scanned.archive_type.as_deref(), Some("ccinf"));
        assert_eq!(scanned.details.as_deref(), Some("entries=0"));
        assert!(scanned.error.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
