//! Manifest-backed unpack/pack helpers for DelDX sound packs.
//!
//! The CLI exposes this format as `archive --type sound`. The manifest keeps
//! the canonical container header and ordered payload filenames so `snd.pck`
//! can rebuild without audio decoding.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_archive::{
    DELDX_PACK_HEADER_LEN, DELDX_PACK_ROW_PREFIX_LEN, DelDxPack, DelDxPackWriteEntry,
    DelDxPackWriteOptions, normalize_deldx_pack_header_for_write,
};

use crate::paths::{escape_archive_name, unescape_archive_name};

pub(crate) const SOUND_PACK_MANIFEST_FILE: &str = "sound-pack.json";
const SOUND_PACK_FORMAT: &str = "sound";
const SOUND_PACK_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SoundPackManifest {
    format: String,
    version: u32,
    header_hex: String,
    entries: Vec<String>,
}

/// Unpack a DelDX sound pack into payload files plus `sound-pack.json`.
pub(crate) fn unpack_sound_pack(archive: &DelDxPack, out: &Path) -> anyhow::Result<usize> {
    fs::create_dir_all(out)?;

    let mut entries = Vec::new();
    for entry in archive.entries() {
        let file_name = sound_payload_file_name(entry.index, &entry.name);
        fs::write(out.join(&file_name), archive.read_entry_payload(entry)?)?;
        entries.push(file_name);
    }

    let manifest = SoundPackManifest {
        format: SOUND_PACK_FORMAT.to_owned(),
        version: SOUND_PACK_MANIFEST_VERSION,
        header_hex: hex::encode(normalize_deldx_pack_header_for_write(archive.header())),
        entries,
    };
    fs::write(
        out.join(SOUND_PACK_MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest.entries.len())
}

/// Pack an unpacked sound-pack directory into an output `snd.pck`.
pub(crate) fn pack_sound_pack_dir(dir: &Path, out: &Path) -> anyhow::Result<DelDxPack> {
    let manifest_path = dir.join(SOUND_PACK_MANIFEST_FILE);
    let manifest_bytes =
        fs::read(&manifest_path).with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: SoundPackManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;
    validate_manifest_header(&manifest)?;

    let header = decode_hex_array::<DELDX_PACK_HEADER_LEN>(&manifest.header_hex, "header_hex")?;
    let mut entries = Vec::with_capacity(manifest.entries.len());
    for (position, entry) in manifest.entries.iter().enumerate() {
        entries.push(load_manifest_entry(dir, position, entry)?);
    }

    let archive = DelDxPack::from_entries(
        out.to_path_buf(),
        entries,
        &DelDxPackWriteOptions::new(header),
    )?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    archive.write_to(out)?;
    Ok(archive)
}

pub(crate) fn sound_pack_manifest_exists(dir: &Path) -> bool {
    dir.join(SOUND_PACK_MANIFEST_FILE).is_file()
}

fn validate_manifest_header(manifest: &SoundPackManifest) -> anyhow::Result<()> {
    if manifest.format != SOUND_PACK_FORMAT {
        bail!(
            "sound pack manifest has unsupported format {:?}; expected {:?}",
            manifest.format,
            SOUND_PACK_FORMAT
        );
    }
    if manifest.version != SOUND_PACK_MANIFEST_VERSION {
        bail!(
            "sound pack manifest has unsupported version {}; expected {}",
            manifest.version,
            SOUND_PACK_MANIFEST_VERSION
        );
    }
    Ok(())
}

fn load_manifest_entry(
    dir: &Path,
    position: usize,
    entry: &str,
) -> anyhow::Result<DelDxPackWriteEntry> {
    let name = infer_sound_name_from_entry_file_name(position, entry)?;
    let row_prefix = row_prefix_from_name(&name)
        .with_context(|| format!("building sound pack row prefix for entry {position}"))?;
    let path = safe_manifest_entry_path(dir, entry)?;
    let data = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;

    Ok(DelDxPackWriteEntry::new(row_prefix, data))
}

fn sound_payload_file_name(index: usize, name: &str) -> String {
    format!("{index:06}__{}", escape_archive_name(name))
}

fn decode_hex_array<const N: usize>(value: &str, field: &str) -> anyhow::Result<[u8; N]> {
    let bytes = decode_hex_vec(value, field)?;
    let actual = bytes.len();
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{field} decodes to {actual} bytes; expected {N}"))
}

fn decode_hex_vec(value: &str, field: &str) -> anyhow::Result<Vec<u8>> {
    hex::decode(value).with_context(|| format!("decoding {field}"))
}

fn safe_manifest_entry_path(root: &Path, entry: &str) -> anyhow::Result<PathBuf> {
    if entry == SOUND_PACK_MANIFEST_FILE {
        bail!("sound pack entry cannot reference its manifest file");
    }
    let path = Path::new(entry);
    if path.is_absolute() {
        bail!("sound pack entry path must be relative: {entry}");
    }
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => bail!("sound pack entry path must be a filename: {entry}"),
    }
    Ok(root.join(path))
}

fn infer_sound_name_from_entry_file_name(position: usize, entry: &str) -> anyhow::Result<String> {
    let expected_prefix = format!("{position:06}__");
    let Some(encoded_name) = entry.strip_prefix(&expected_prefix) else {
        bail!("sound pack entry {position} filename must start with {expected_prefix:?}");
    };
    if encoded_name.is_empty() {
        bail!("sound pack entry {position} filename is missing the archived name");
    }
    unescape_archive_name(encoded_name)
        .with_context(|| format!("decoding sound pack entry {position} filename"))
}

fn row_prefix_from_name(name: &str) -> anyhow::Result<Vec<u8>> {
    let name = name.as_bytes();
    let name_len = name.len();
    if name_len >= DELDX_PACK_ROW_PREFIX_LEN {
        bail!("row name length exceeds field capacity");
    }
    let mut row_prefix = vec![0; DELDX_PACK_ROW_PREFIX_LEN];
    row_prefix[0] = name_len as u8;
    row_prefix[1..1 + name_len].copy_from_slice(name);
    Ok(row_prefix)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use taletool_archive::{
        DELDX_PACK_HEADER_LEN, DELDX_PACK_RESERVED_HEADER_LEN, DELDX_PACK_RESERVED_HEADER_OFFSET,
        DELDX_PACK_ROW_PREFIX_LEN, DelDxPackWriteOptions, write_deldx_pack_bytes,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn header() -> [u8; DELDX_PACK_HEADER_LEN] {
        let mut out = [0; DELDX_PACK_HEADER_LEN];
        out[0] = 16;
        out[1..17].copy_from_slice(b"DelDX Pack File ");
        out[0x14..0x18].copy_from_slice(&10i32.to_le_bytes());
        out
    }

    fn row_prefix(name: &[u8]) -> Vec<u8> {
        let mut out = vec![0; DELDX_PACK_ROW_PREFIX_LEN];
        out[0] = name.len() as u8;
        out[1..1 + name.len()].copy_from_slice(name);
        out
    }

    #[test]
    fn sound_pack_manifest_round_trips() {
        let root = temp_dir("sound-round-trip");
        let unpacked = root.join("unpacked");
        let input = root.join("snd.pck");
        let output = root.join("repacked.pck");
        let entries = vec![
            DelDxPackWriteEntry::new(row_prefix(b"base.10.wav"), b"sound".to_vec()),
            DelDxPackWriteEntry::new(row_prefix(b"blank.20.wav"), Vec::new()),
        ];
        let mut bytes =
            write_deldx_pack_bytes(&entries, &DelDxPackWriteOptions::new(header())).unwrap();
        bytes[DELDX_PACK_RESERVED_HEADER_OFFSET
            ..DELDX_PACK_RESERVED_HEADER_OFFSET + DELDX_PACK_RESERVED_HEADER_LEN]
            .copy_from_slice(&[0xf0, 0xfd, 0x7f]);
        fs::create_dir_all(&root).unwrap();
        fs::write(&input, bytes).unwrap();
        let archive = DelDxPack::open(&input).unwrap();

        assert_eq!(unpack_sound_pack(&archive, &unpacked).unwrap(), 2);
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(unpacked.join(SOUND_PACK_MANIFEST_FILE)).unwrap())
                .unwrap();
        assert_eq!(manifest["format"], "sound");
        assert_eq!(
            manifest["header_hex"],
            hex::encode(normalize_deldx_pack_header_for_write(archive.header()))
        );
        assert_eq!(manifest["entries"][0], "000000__base.10.wav");
        assert_eq!(manifest["entries"][1], "000001__blank.20.wav");
        assert_eq!(
            fs::read(unpacked.join("000000__base.10.wav")).unwrap(),
            b"sound"
        );
        assert_eq!(
            fs::read(unpacked.join("000001__blank.20.wav")).unwrap(),
            b""
        );
        let repacked = pack_sound_pack_dir(&unpacked, &output).unwrap();

        assert_eq!(
            repacked.write_entries().unwrap(),
            archive.write_entries().unwrap()
        );
        assert_eq!(
            &repacked.header()[DELDX_PACK_RESERVED_HEADER_OFFSET
                ..DELDX_PACK_RESERVED_HEADER_OFFSET + DELDX_PACK_RESERVED_HEADER_LEN],
            &[0, 0, 0]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_sound_payload_fails() {
        let root = temp_dir("sound-missing-payload");
        let unpacked = root.join("unpacked");
        let payload = unpacked.join("000000__base.10.wav");
        fs::create_dir_all(&unpacked).unwrap();
        fs::write(
            unpacked.join(SOUND_PACK_MANIFEST_FILE),
            serde_json::json!({
                "format": SOUND_PACK_FORMAT,
                "version": SOUND_PACK_MANIFEST_VERSION,
                "header_hex": hex::encode(header()),
                "entries": ["000000__base.10.wav"]
            })
            .to_string(),
        )
        .unwrap();

        assert!(pack_sound_pack_dir(&unpacked, &root.join("snd.pck")).is_err());
        assert!(!payload.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_sound_manifest_version_fails() {
        let root = temp_dir("sound-invalid-version");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(SOUND_PACK_MANIFEST_FILE),
            serde_json::json!({
                "format": SOUND_PACK_FORMAT,
                "version": 999,
                "header_hex": hex::encode(header()),
                "entries": []
            })
            .to_string(),
        )
        .unwrap();

        assert!(pack_sound_pack_dir(&root, &root.join("snd.pck")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_sound_manifest_fields_fail() {
        let root = temp_dir("sound-unknown-field");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(SOUND_PACK_MANIFEST_FILE),
            serde_json::json!({
                "format": SOUND_PACK_FORMAT,
                "version": SOUND_PACK_MANIFEST_VERSION,
                "header_hex": hex::encode(header()),
                "source": "snd.pck",
                "entries": []
            })
            .to_string(),
        )
        .unwrap();

        assert!(pack_sound_pack_dir(&root, &root.join("snd.pck")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sound_manifest_entry_file_name_must_match_position() {
        let root = temp_dir("sound-entry-name-position");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("000001__base.10.wav"), b"sound").unwrap();
        fs::write(
            root.join(SOUND_PACK_MANIFEST_FILE),
            serde_json::json!({
                "format": SOUND_PACK_FORMAT,
                "version": SOUND_PACK_MANIFEST_VERSION,
                "header_hex": hex::encode(header()),
                "entries": ["000001__base.10.wav"]
            })
            .to_string(),
        )
        .unwrap();

        assert!(pack_sound_pack_dir(&root, &root.join("snd.pck")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
