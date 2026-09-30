//! Lossless raw text archive directories, with metadata separate from payload names.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_archive::{TextNosArchive, TextNosRecord, write_text_nos_archive_records};

use crate::archive_convert::write_output_transactionally;

pub(crate) const TEXT_ARCHIVE_MANIFEST_FILE: &str = "text-archive.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextArchiveManifest {
    format: String,
    version: u32,
    trailer_hex: String,
    entries: Vec<TextArchiveEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextArchiveEntry {
    id: i32,
    name_hex: String,
    packed_flag: i32,
    file: String,
}

pub(crate) fn text_archive_manifest_exists(dir: &Path) -> bool {
    fs::symlink_metadata(dir.join(TEXT_ARCHIVE_MANIFEST_FILE)).is_ok()
}

/// Write unchanged payload bytes to unique files and preserve all envelope metadata.
pub(crate) fn unpack_text_archive(
    archive: &TextNosArchive,
    out: &Path,
) -> anyhow::Result<Vec<(i32, PathBuf)>> {
    write_output_transactionally(out, |staging| {
        let mut entries = Vec::with_capacity(archive.records().len());
        let mut written = Vec::with_capacity(archive.records().len());
        let mut files = BTreeSet::from([TEXT_ARCHIVE_MANIFEST_FILE.to_owned()]);
        for record in archive.records() {
            let file = payload_file_name(&record.name_bytes)?;
            if !files.insert(file.to_ascii_lowercase()) {
                bail!("text archive output filename collision: {file:?}");
            }
            fs::write(staging.join(&file), &record.payload)?;
            written.push((record.id, PathBuf::from(&file)));
            entries.push(TextArchiveEntry {
                id: record.id,
                name_hex: hex::encode(&record.name_bytes),
                packed_flag: record.packed_flag,
                file,
            });
        }
        let manifest = TextArchiveManifest {
            format: "text".to_owned(),
            version: 1,
            trailer_hex: hex::encode(archive.trailer()),
            entries,
        };
        fs::write(
            staging.join(TEXT_ARCHIVE_MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        Ok(written)
    })
}

fn payload_file_name(name: &[u8]) -> anyhow::Result<String> {
    let mut file = String::new();
    for &byte in name {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            file.push(byte as char);
        } else {
            file.push_str(&format!("%{byte:02X}"));
        }
    }
    // Reject names that cannot be represented portably as a single local file.
    let stem = file
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| matches!(suffix.as_bytes(), [b'1'..=b'9']));
    if file.is_empty() || file.len() > 255 || file.ends_with('.') || reserved {
        bail!("text archive name cannot be represented as a portable payload filename: {file:?}");
    }
    Ok(file)
}

/// Build bytes only after validating the entire manifest and loading every payload.
pub(crate) fn pack_text_archive_manifest(dir: &Path) -> anyhow::Result<(Vec<u8>, usize)> {
    let manifest_path = dir.join(TEXT_ARCHIVE_MANIFEST_FILE);
    let manifest: TextArchiveManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("reading {}", manifest_path.display()))?,
    )
    .with_context(|| format!("parsing {}", manifest_path.display()))?;
    if manifest.format != "text" || manifest.version != 1 {
        bail!(
            "unsupported text archive manifest format/version: {:?}/{}; expected text/1",
            manifest.format,
            manifest.version
        );
    }
    let trailer = hex::decode(&manifest.trailer_hex).context("decoding trailer_hex")?;
    let mut files = BTreeSet::new();
    let mut records = Vec::with_capacity(manifest.entries.len());
    for (index, entry) in manifest.entries.iter().enumerate() {
        let path = safe_payload_path(dir, &entry.file)?;
        if !files.insert(entry.file.to_ascii_lowercase()) {
            bail!(
                "text archive entries reference the same payload filename: {}",
                entry.file
            );
        }
        let name_bytes = hex::decode(&entry.name_hex)
            .with_context(|| format!("decoding entry {index} name_hex"))?;
        records.push(TextNosRecord {
            id: entry.id,
            name: String::from_utf8_lossy(&name_bytes).into_owned(),
            name_bytes,
            packed_flag: entry.packed_flag,
            payload: fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
        });
    }
    Ok((
        write_text_nos_archive_records(&records, &trailer)?,
        records.len(),
    ))
}

fn safe_payload_path(root: &Path, file: &str) -> anyhow::Result<PathBuf> {
    let mut components = Path::new(file).components();
    if file.eq_ignore_ascii_case(TEXT_ARCHIVE_MANIFEST_FILE)
        || file.contains(['/', '\\', ':'])
        || file.ends_with(['.', ' '])
        || !matches!(
            (components.next(), components.next()),
            (Some(Component::Normal(_)), None)
        )
    {
        bail!("text archive payload must be a filename other than its manifest: {file:?}");
    }
    let path = root.join(file);
    let metadata =
        fs::symlink_metadata(&path).with_context(|| format!("checking {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!(
            "text archive payload must be a regular file, not a directory or symlink: {}",
            path.display()
        );
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "taletool-text-fidelity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    // Construct wire bytes independently of the archive writer.
    fn fixture() -> Vec<u8> {
        fixture_with_names(&[
            b"Item.dat",
            b"other.dat",
            b"third.lst",
            b"\xff.dat",
            b"\xfe.dat",
            b"a/b\\c",
        ])
    }

    fn fixture_with_names(names: &[&[u8]]) -> Vec<u8> {
        let mut bytes = (names.len() as i32).to_le_bytes().to_vec();
        for (index, name) in names.iter().enumerate() {
            let id = if index < 3 { -7 } else { index as i32 + 100 };
            let flag = [0i32, -9, i32::MAX][index % 3];
            let payload = vec![index as u8; index];
            bytes.extend(id.to_le_bytes());
            bytes.extend((name.len() as i32).to_le_bytes());
            bytes.extend_from_slice(name);
            bytes.extend(flag.to_le_bytes());
            bytes.extend((payload.len() as i32).to_le_bytes());
            bytes.extend(payload);
        }
        bytes.extend(b"extra trailer bytes");
        bytes.extend(45000.123456789_f64.to_le_bytes());
        bytes.extend([0xee, 0x3e, 0x32, 0x01]);
        bytes
    }

    #[test]
    fn raw_archive_round_trip_preserves_every_byte_and_native_names() {
        let root = TestDir::new();
        let out = root.0.join("unpacked");
        let original = fixture();
        let archive = TextNosArchive::from_bytes("input.NOS".into(), original.clone()).unwrap();
        let files = unpack_text_archive(&archive, &out).unwrap();
        assert_eq!(files.len(), archive.records().len());
        assert_eq!(files[0].1, Path::new("Item.dat"));
        assert_eq!(files[3].1, Path::new("%FF.dat"));
        assert_eq!(files[4].1, Path::new("%FE.dat"));
        assert_eq!(fs::read_dir(&out).unwrap().count(), files.len() + 1);
        for ((id, file), record) in files.iter().zip(archive.records()) {
            assert_eq!(*id, record.id);
            assert_eq!(fs::read(out.join(file)).unwrap(), record.payload);
        }
        // Unlisted files do not accidentally become records.
        fs::write(out.join("notes.txt"), b"notes").unwrap();
        let (rebuilt, count) = pack_text_archive_manifest(&out).unwrap();
        assert_eq!(count, files.len());
        assert_eq!(rebuilt, original);

        fs::write(out.join(&files[1].1), b"edited payload").unwrap();
        let (edited, _) = pack_text_archive_manifest(&out).unwrap();
        let edited = TextNosArchive::from_bytes("edited.NOS".into(), edited).unwrap();
        assert_eq!(edited.trailer(), archive.trailer());
        for (index, (before, after)) in archive.records().iter().zip(edited.records()).enumerate() {
            assert_eq!(before.id, after.id);
            assert_eq!(before.name_bytes, after.name_bytes);
            assert_eq!(before.packed_flag, after.packed_flag);
            assert_eq!(
                after.payload,
                if index == 1 {
                    b"edited payload".as_slice()
                } else {
                    &before.payload
                }
            );
        }
    }

    #[test]
    fn rejects_output_collisions_without_publishing_partial_files() {
        let root = TestDir::new();
        for names in [
            vec![b"Item.dat".as_slice(), b"Item.dat"],
            vec![b"Item.dat".as_slice(), b"ITEM.DAT"],
            vec![b"Item.dat".as_slice(), b"text-archive.json"],
        ] {
            let out = root.0.join("unpacked");
            let archive =
                TextNosArchive::from_bytes("input.NOS".into(), fixture_with_names(&names)).unwrap();
            let error = unpack_text_archive(&archive, &out).unwrap_err();
            assert!(error.to_string().contains("filename collision"));
            assert!(!out.exists());
            assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
        }
    }

    #[test]
    fn rejects_unrepresentable_payload_names() {
        for name in [
            b"".as_slice(),
            b".",
            b"..",
            b"CON",
            b"nul.txt",
            b"LPT1.dat",
            b"tail.",
            &[b'x'; 256],
        ] {
            assert!(payload_file_name(name).is_err(), "accepted {name:?}");
        }
        assert_eq!(payload_file_name(b"console.dat").unwrap(), "console.dat");
        assert_eq!(payload_file_name(b"COM10.dat").unwrap(), "COM10.dat");
    }

    #[test]
    fn empty_archives_preserve_absent_unknown_and_nonfinite_trailers() {
        let root = TestDir::new();
        let mut nan_footer = 0x7ff8_0000_0000_0042u64.to_le_bytes().to_vec();
        nan_footer.extend([0xee, 0x3e, 0x32, 0x01]);
        for (index, trailer) in [vec![], vec![1, 2, 3, 4], nan_footer]
            .into_iter()
            .enumerate()
        {
            let mut bytes = 0i32.to_le_bytes().to_vec();
            bytes.extend(&trailer);
            let archive = TextNosArchive::from_bytes("empty.NOS".into(), bytes.clone()).unwrap();
            let out = root.0.join(index.to_string());
            unpack_text_archive(&archive, &out).unwrap();
            assert_eq!(pack_text_archive_manifest(&out).unwrap(), (bytes, 0));
        }
    }

    fn valid_manifest() -> serde_json::Value {
        serde_json::json!({
            "format": "text", "version": 1, "trailer_hex": "",
            "entries": [{"id": -1, "name_hex": "ff", "packed_flag": -2, "file": "payload.bin"}]
        })
    }

    #[test]
    fn rejects_invalid_metadata_missing_files_and_unsafe_paths() {
        let root = TestDir::new();
        fs::write(root.0.join("payload.bin"), b"test").unwrap();
        let mut invalid = Vec::new();
        for (field, value) in [
            ("format", serde_json::json!("sound")),
            ("version", serde_json::json!(2)),
            ("trailer_hex", serde_json::json!("0")),
            ("extra", serde_json::json!(true)),
        ] {
            let mut manifest = valid_manifest();
            manifest[field] = value;
            invalid.push(manifest);
        }
        for (field, value) in [
            ("name_hex", serde_json::json!("xx")),
            ("id", serde_json::json!(2147483648u64)),
            ("packed_flag", serde_json::json!(-2147483649i64)),
            ("extra", serde_json::json!(true)),
        ] {
            let mut manifest = valid_manifest();
            manifest["entries"][0][field] = value;
            invalid.push(manifest);
        }
        for file in [
            "missing.bin",
            "",
            ".",
            "..",
            "../payload.bin",
            "/payload.bin",
            "a/b",
            "a\\b",
            "C:payload.bin",
            "payload.bin.",
            "payload.bin ",
            "TEXT-ARCHIVE.JSON",
        ] {
            let mut manifest = valid_manifest();
            manifest["entries"][0]["file"] = file.into();
            invalid.push(manifest);
        }
        let mut duplicate = valid_manifest();
        let second = duplicate["entries"][0].clone();
        duplicate["entries"].as_array_mut().unwrap().push(second);
        invalid.push(duplicate);
        for manifest in invalid {
            fs::write(
                root.0.join(TEXT_ARCHIVE_MANIFEST_FILE),
                manifest.to_string(),
            )
            .unwrap();
            assert!(
                pack_text_archive_manifest(&root.0).is_err(),
                "accepted {manifest}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_payloads() {
        let root = TestDir::new();
        fs::write(root.0.join("target.bin"), b"target").unwrap();
        std::os::unix::fs::symlink(root.0.join("target.bin"), root.0.join("payload.bin")).unwrap();
        fs::write(
            root.0.join(TEXT_ARCHIVE_MANIFEST_FILE),
            valid_manifest().to_string(),
        )
        .unwrap();
        let error = pack_text_archive_manifest(&root.0).unwrap_err();
        assert!(error.to_string().contains("symlink"));
    }

    #[test]
    fn refuses_existing_unpack_directory_without_modifying_it() {
        let root = TestDir::new();
        let sentinel = root.0.join("Item.dat");
        fs::write(&sentinel, b"keep").unwrap();
        let archive = TextNosArchive::from_bytes("input.NOS".into(), fixture()).unwrap();
        assert!(unpack_text_archive(&archive, &root.0).is_err());
        assert_eq!(fs::read(sentinel).unwrap(), b"keep");
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 1);
    }
}
