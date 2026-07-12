//! Lossless JSON manifest helpers for `sndinfo.lst`.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_audio::{
    SOUND_INFO_FILENAME_CAPACITY, SOUND_INFO_UNKNOWN_47_LEN, SoundFilename, SoundInfoEntry,
    SoundInfoTable, SoundKey,
};
use taletool_core::AssetId;

const SOUND_INFO_FORMAT: &str = "sndinfo";
const SOUND_INFO_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SoundInfoManifest {
    format: String,
    version: u32,
    trailing_hex: String,
    entries: Vec<SoundInfoManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SoundInfoManifestEntry {
    key: SoundKey,
    sound_id: AssetId,
    unknown_10: i32,
    filename: String,
    filename_padding_hex: String,
    unknown_47_hex: String,
}

pub(crate) fn unpack_sound_info(table: &SoundInfoTable, out: &Path) -> anyhow::Result<usize> {
    let manifest = SoundInfoManifest::from_table(table);
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(manifest.entries.len())
}

pub(crate) fn pack_sound_info(input: &Path, out: &Path) -> anyhow::Result<SoundInfoTable> {
    let manifest_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let manifest: SoundInfoManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    let table = manifest.into_table()?;
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    table.write_to(out)?;
    Ok(table)
}

impl SoundInfoManifest {
    fn from_table(table: &SoundInfoTable) -> Self {
        Self {
            format: SOUND_INFO_FORMAT.to_owned(),
            version: SOUND_INFO_MANIFEST_VERSION,
            trailing_hex: hex::encode(table.trailing_bytes()),
            entries: table
                .entries()
                .iter()
                .map(SoundInfoManifestEntry::from_entry)
                .collect(),
        }
    }

    fn into_table(self) -> anyhow::Result<SoundInfoTable> {
        if self.format != SOUND_INFO_FORMAT {
            bail!(
                "sound info manifest has unsupported format {:?}; expected {:?}",
                self.format,
                SOUND_INFO_FORMAT
            );
        }
        if self.version != SOUND_INFO_MANIFEST_VERSION {
            bail!(
                "sound info manifest has unsupported version {}; expected {}",
                self.version,
                SOUND_INFO_MANIFEST_VERSION
            );
        }

        let mut entries = Vec::with_capacity(self.entries.len());
        for (index, entry) in self.entries.into_iter().enumerate() {
            entries.push(
                entry
                    .into_entry()
                    .with_context(|| format!("decoding sound info manifest entry {index}"))?,
            );
        }
        let mut table = SoundInfoTable::new(entries);
        table.set_trailing_bytes(
            hex::decode(&self.trailing_hex).context("decoding manifest trailing_hex")?,
        );
        Ok(table)
    }
}

impl SoundInfoManifestEntry {
    fn from_entry(entry: &SoundInfoEntry) -> Self {
        Self {
            key: entry.key,
            sound_id: entry.sound_id,
            unknown_10: entry.unknown_10,
            filename: escape_filename(entry.filename.as_bytes()),
            filename_padding_hex: hex::encode(entry.filename.padding()),
            unknown_47_hex: hex::encode(entry.unknown_47),
        }
    }

    fn into_entry(self) -> anyhow::Result<SoundInfoEntry> {
        let filename = unescape_filename(&self.filename)?;
        if filename.len() > SOUND_INFO_FILENAME_CAPACITY {
            bail!(
                "filename is {} bytes; maximum is {SOUND_INFO_FILENAME_CAPACITY}",
                filename.len()
            );
        }
        let padding =
            hex::decode(&self.filename_padding_hex).context("decoding filename_padding_hex")?;
        let expected_padding = SOUND_INFO_FILENAME_CAPACITY - filename.len();
        if padding.len() != expected_padding {
            bail!(
                "filename_padding_hex decodes to {} bytes; expected {expected_padding}",
                padding.len()
            );
        }
        let unknown_47 =
            decode_hex_array::<SOUND_INFO_UNKNOWN_47_LEN>(&self.unknown_47_hex, "unknown_47_hex")?;

        Ok(SoundInfoEntry {
            key: self.key,
            sound_id: self.sound_id,
            unknown_10: self.unknown_10,
            filename: SoundFilename::from_parts(filename, padding)?,
            unknown_47,
        })
    }
}

/// Escape arbitrary legacy filename bytes into readable ASCII.
///
/// Printable ASCII except `%` remains literal; all other bytes use `%HH`.
fn escape_filename(bytes: &[u8]) -> String {
    let mut out = String::new();
    for byte in bytes {
        if (0x20..=0x7e).contains(byte) && *byte != b'%' {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn unescape_filename(value: &str) -> anyhow::Result<Vec<u8>> {
    if !value.is_ascii() {
        bail!("filename must use ASCII and %HH escapes for legacy bytes");
    }

    let input = value.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let Some(hex_bytes) = input.get(index + 1..index + 3) else {
                bail!("incomplete percent escape at byte {index}");
            };
            let digits = std::str::from_utf8(hex_bytes).expect("manifest filename is ASCII");
            let byte = u8::from_str_radix(digits, 16)
                .with_context(|| format!("invalid percent escape %{digits}"))?;
            out.push(byte);
            index += 3;
        } else {
            out.push(input[index]);
            index += 1;
        }
    }
    Ok(out)
}

fn decode_hex_array<const N: usize>(value: &str, field: &str) -> anyhow::Result<[u8; N]> {
    let bytes = hex::decode(value).with_context(|| format!("decoding {field}"))?;
    let actual = bytes.len();
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{field} decodes to {actual} bytes; expected {N}"))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn manifest_round_trip_preserves_every_byte() {
        let filename = SoundFilename::from_parts(
            vec![b'a', b'%', 0x81, 0x00],
            vec![0xa5; SOUND_INFO_FILENAME_CAPACITY - 4],
        )
        .unwrap();
        let mut entry = SoundInfoEntry::new(SoundKey::new(4, -1, 2), AssetId(-1), filename);
        entry.unknown_10 = -77;
        entry.unknown_47 = [0x5a; SOUND_INFO_UNKNOWN_47_LEN];
        let mut table = SoundInfoTable::new(vec![entry]);
        table.set_trailing_bytes(vec![0xde, 0xad]);

        let manifest = SoundInfoManifest::from_table(&table);
        assert_eq!(manifest.entries[0].filename, "a%25%81%00");
        assert_eq!(manifest.into_table().unwrap(), table);
    }

    #[test]
    fn file_manifest_round_trip_is_byte_identical() {
        let root = temp_dir("manifest-round-trip");
        fs::create_dir_all(&root).unwrap();
        let json = root.join("sndinfo.json");
        let output = root.join("sndinfo.lst");

        let mut entry = SoundInfoEntry::new(
            SoundKey::new(3, 0, 0),
            AssetId(30000),
            SoundFilename::new(b"BGM (1).30000.wav".to_vec()).unwrap(),
        );
        entry.unknown_47[7] = 9;
        let table = SoundInfoTable::new(vec![entry]);

        unpack_sound_info(&table, &json).unwrap();
        let rebuilt = pack_sound_info(&json, &output).unwrap();
        assert_eq!(rebuilt.to_bytes().unwrap(), table.to_bytes().unwrap());
        assert_eq!(fs::read(output).unwrap(), table.to_bytes().unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_rejects_unknown_fields_and_invalid_storage_lengths() {
        let unknown_field = br#"{
            "format":"sndinfo","version":1,"trailing_hex":"","entries":[],"extra":1
        }"#;
        assert!(serde_json::from_slice::<SoundInfoManifest>(unknown_field).is_err());

        let manifest = SoundInfoManifest {
            format: SOUND_INFO_FORMAT.to_owned(),
            version: SOUND_INFO_MANIFEST_VERSION,
            trailing_hex: String::new(),
            entries: vec![SoundInfoManifestEntry {
                key: SoundKey::new(1, 0, 1),
                sound_id: AssetId(1),
                unknown_10: 0,
                filename: "abc".to_owned(),
                filename_padding_hex: String::new(),
                unknown_47_hex: hex::encode([0; SOUND_INFO_UNKNOWN_47_LEN]),
            }],
        };
        assert!(manifest.into_table().is_err());
    }

    #[test]
    fn filename_escape_is_reversible_and_strict() {
        let bytes = [b'A', b' ', b'%', 0, 0xff];
        assert_eq!(unescape_filename(&escape_filename(&bytes)).unwrap(), bytes);
        assert!(unescape_filename("bad%0").is_err());
        assert!(unescape_filename("bad%GG").is_err());
        assert!(unescape_filename("Korean: 한").is_err());
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-cli-{label}-{nonce}"))
    }
}
