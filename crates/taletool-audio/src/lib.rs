//! Read, edit, write, and resolve NosTale `sndinfo.lst` sound metadata.

use std::fs;
use std::path::{Path, PathBuf};

use encoding_rs::EUC_KR;
use serde::{Deserialize, Serialize};
use taletool_core::AssetId;
use thiserror::Error;

pub const SOUND_INFO_HEADER_LEN: usize = 4;
pub const SOUND_INFO_ENTRY_LEN: usize = 0x7c;
pub const SOUND_INFO_FILENAME_CAPACITY: usize = 50;
pub const SOUND_INFO_UNKNOWN_47_LEN: usize = 53;

const KEY_OFFSET: usize = 0x00;
const SOUND_ID_OFFSET: usize = 0x0c;
const UNKNOWN_10_OFFSET: usize = 0x10;
const FILENAME_OFFSET: usize = 0x14;
const UNKNOWN_47_OFFSET: usize = 0x47;

#[derive(Debug, Error)]
pub enum SoundInfoError {
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sndinfo.lst is shorter than its {needed}-byte header: got {actual} bytes")]
    TruncatedHeader { needed: usize, actual: usize },
    #[error("sndinfo.lst has invalid negative entry count {count}")]
    InvalidEntryCount { count: i32 },
    #[error("sndinfo.lst table size overflows for {count} entries")]
    TableSizeOverflow { count: usize },
    #[error("sndinfo.lst table is truncated: need {needed} bytes, got {actual}")]
    TruncatedTable { needed: usize, actual: usize },
    #[error("sndinfo.lst entry {entry} filename length {length} exceeds the {capacity}-byte field")]
    InvalidFilenameLength {
        entry: usize,
        length: usize,
        capacity: usize,
    },
    #[error("sound filename is {length} bytes; maximum is {capacity}")]
    FilenameTooLong { length: usize, capacity: usize },
    #[error("sound filename padding has {actual} bytes; expected {expected} for this filename")]
    InvalidFilenamePadding { expected: usize, actual: usize },
    #[error("sndinfo.lst has too many entries to write: {count}")]
    TooManyEntries { count: usize },
}

pub type Result<T> = std::result::Result<T, SoundInfoError>;

/// The three-part logical key used by the client sound table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SoundKey {
    pub group: i32,
    pub primary: i32,
    pub secondary: i32,
}

impl SoundKey {
    pub const fn new(group: i32, primary: i32, secondary: i32) -> Self {
        Self {
            group,
            primary,
            secondary,
        }
    }
}

/// A validated Delphi `string[50]` value, including its unused storage bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundFilename {
    bytes: Vec<u8>,
    padding: Vec<u8>,
}

impl SoundFilename {
    /// Create a filename and zero-fill its remaining fixed-width storage.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let bytes = bytes.into();
        validate_filename_length(bytes.len())?;
        let padding = vec![0; SOUND_INFO_FILENAME_CAPACITY - bytes.len()];
        Ok(Self { bytes, padding })
    }

    /// Create a filename while preserving all unused fixed-width bytes.
    pub fn from_parts(bytes: Vec<u8>, padding: Vec<u8>) -> Result<Self> {
        validate_filename_length(bytes.len())?;
        let expected = SOUND_INFO_FILENAME_CAPACITY - bytes.len();
        if padding.len() != expected {
            return Err(SoundInfoError::InvalidFilenamePadding {
                expected,
                actual: padding.len(),
            });
        }
        Ok(Self { bytes, padding })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn padding(&self) -> &[u8] {
        &self.padding
    }

    /// Decode the byte filename using the non-localized client code page.
    pub fn display_name(&self) -> String {
        EUC_KR.decode(&self.bytes).0.into_owned()
    }

    /// Replace the filename and canonicalize unused storage to zeroes.
    pub fn set_bytes(&mut self, bytes: impl Into<Vec<u8>>) -> Result<()> {
        *self = Self::new(bytes)?;
        Ok(())
    }
}

/// One ordered row in `sndinfo.lst`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundInfoEntry {
    pub key: SoundKey,
    pub sound_id: AssetId,
    pub unknown_10: i32,
    pub filename: SoundFilename,
    pub unknown_47: [u8; SOUND_INFO_UNKNOWN_47_LEN],
}

impl SoundInfoEntry {
    pub fn new(key: SoundKey, sound_id: AssetId, filename: SoundFilename) -> Self {
        Self {
            key,
            sound_id,
            unknown_10: 0,
            filename,
            unknown_47: [0; SOUND_INFO_UNKNOWN_47_LEN],
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.sound_id.0 != -1
    }
}

/// An editable, lossless `sndinfo.lst` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundInfoTable {
    entries: Vec<SoundInfoEntry>,
    trailing_bytes: Vec<u8>,
}

/// Resolves table entries against a `wave` directory.
///
/// Direct and stripped-name checks always inspect the current filesystem. The
/// sorted `.wav` listing used by the sound-ID fallback is loaded on demand and
/// retained as a snapshot for the resolver's lifetime.
#[derive(Debug, Clone)]
pub struct SoundFileResolver {
    wave_dir: PathBuf,
    wav_files: Option<Vec<PathBuf>>,
}

impl SoundFileResolver {
    pub fn new(wave_dir: impl AsRef<Path>) -> Self {
        Self {
            wave_dir: wave_dir.as_ref().to_path_buf(),
            wav_files: None,
        }
    }

    /// Resolve the filename belonging to this exact source entry.
    pub fn resolve_entry(&mut self, entry: &SoundInfoEntry) -> Result<Option<PathBuf>> {
        let stored = self.wave_dir.join(entry.filename.display_name());
        if stored.is_file() {
            return Ok(Some(stored));
        }

        if let Some(wav_offset) = entry
            .filename
            .as_bytes()
            .windows(4)
            .position(|window| window == b".wav")
        {
            let stripped_name = EUC_KR
                .decode(&entry.filename.as_bytes()[..wav_offset])
                .0
                .into_owned();
            let stripped = self.wave_dir.join(stripped_name);
            if stripped.is_file() {
                return Ok(Some(stripped));
            }
        }

        let sound_id = entry.sound_id.0.to_string();
        Ok(self
            .wav_files()?
            .iter()
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(&sound_id))
            })
            .cloned())
    }

    fn wav_files(&mut self) -> Result<&[PathBuf]> {
        if self.wav_files.is_none() {
            self.wav_files = Some(read_wav_files(&self.wave_dir)?);
        }
        Ok(self
            .wav_files
            .as_deref()
            .expect("wave file cache was populated"))
    }
}

impl SoundInfoTable {
    pub fn new(entries: Vec<SoundInfoEntry>) -> Self {
        Self {
            entries,
            trailing_bytes: Vec::new(),
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(|source| SoundInfoError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_bytes(&data)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < SOUND_INFO_HEADER_LEN {
            return Err(SoundInfoError::TruncatedHeader {
                needed: SOUND_INFO_HEADER_LEN,
                actual: data.len(),
            });
        }

        let count = i32::from_le_bytes(data[..4].try_into().expect("header length checked"));
        if count < 0 {
            return Err(SoundInfoError::InvalidEntryCount { count });
        }
        let count = count as usize;
        let rows_len = count
            .checked_mul(SOUND_INFO_ENTRY_LEN)
            .ok_or(SoundInfoError::TableSizeOverflow { count })?;
        let table_end = SOUND_INFO_HEADER_LEN
            .checked_add(rows_len)
            .ok_or(SoundInfoError::TableSizeOverflow { count })?;
        if data.len() < table_end {
            return Err(SoundInfoError::TruncatedTable {
                needed: table_end,
                actual: data.len(),
            });
        }

        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let start = SOUND_INFO_HEADER_LEN + index * SOUND_INFO_ENTRY_LEN;
            let row = &data[start..start + SOUND_INFO_ENTRY_LEN];
            let filename_len = usize::from(row[FILENAME_OFFSET]);
            if filename_len > SOUND_INFO_FILENAME_CAPACITY {
                return Err(SoundInfoError::InvalidFilenameLength {
                    entry: index,
                    length: filename_len,
                    capacity: SOUND_INFO_FILENAME_CAPACITY,
                });
            }

            let filename_data_start = FILENAME_OFFSET + 1;
            let filename_data_end = filename_data_start + filename_len;
            let filename_field_end = filename_data_start + SOUND_INFO_FILENAME_CAPACITY;
            entries.push(SoundInfoEntry {
                key: SoundKey::new(
                    read_i32(row, KEY_OFFSET),
                    read_i32(row, KEY_OFFSET + 4),
                    read_i32(row, KEY_OFFSET + 8),
                ),
                sound_id: AssetId(read_i32(row, SOUND_ID_OFFSET)),
                unknown_10: read_i32(row, UNKNOWN_10_OFFSET),
                filename: SoundFilename::from_parts(
                    row[filename_data_start..filename_data_end].to_vec(),
                    row[filename_data_end..filename_field_end].to_vec(),
                )?,
                unknown_47: row[UNKNOWN_47_OFFSET..]
                    .try_into()
                    .expect("sound row tail has a fixed length"),
            });
        }

        Ok(Self {
            entries,
            trailing_bytes: data[table_end..].to_vec(),
        })
    }

    pub fn entries(&self) -> &[SoundInfoEntry] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut [SoundInfoEntry] {
        &mut self.entries
    }

    pub fn push(&mut self, entry: SoundInfoEntry) {
        self.entries.push(entry);
    }

    pub fn into_entries(self) -> Vec<SoundInfoEntry> {
        self.entries
    }

    pub fn trailing_bytes(&self) -> &[u8] {
        &self.trailing_bytes
    }

    pub fn set_trailing_bytes(&mut self, trailing_bytes: Vec<u8>) {
        self.trailing_bytes = trailing_bytes;
    }

    /// Return the first source-order entry matching a logical key.
    pub fn entry_by_key(&self, key: SoundKey) -> Option<&SoundInfoEntry> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    /// Return the first source-order entry matching a sound ID.
    pub fn entry_by_sound_id(&self, sound_id: AssetId) -> Option<&SoundInfoEntry> {
        self.entries.iter().find(|entry| entry.sound_id == sound_id)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let count =
            i32::try_from(self.entries.len()).map_err(|_| SoundInfoError::TooManyEntries {
                count: self.entries.len(),
            })?;
        let rows_len = self.entries.len().checked_mul(SOUND_INFO_ENTRY_LEN).ok_or(
            SoundInfoError::TableSizeOverflow {
                count: self.entries.len(),
            },
        )?;
        let capacity = SOUND_INFO_HEADER_LEN
            .checked_add(rows_len)
            .and_then(|size| size.checked_add(self.trailing_bytes.len()))
            .ok_or(SoundInfoError::TableSizeOverflow {
                count: self.entries.len(),
            })?;
        let mut out = Vec::with_capacity(capacity);
        out.extend_from_slice(&count.to_le_bytes());

        for entry in &self.entries {
            validate_filename_length(entry.filename.bytes.len())?;
            let expected_padding = SOUND_INFO_FILENAME_CAPACITY - entry.filename.bytes.len();
            if entry.filename.padding.len() != expected_padding {
                return Err(SoundInfoError::InvalidFilenamePadding {
                    expected: expected_padding,
                    actual: entry.filename.padding.len(),
                });
            }
            out.extend_from_slice(&entry.key.group.to_le_bytes());
            out.extend_from_slice(&entry.key.primary.to_le_bytes());
            out.extend_from_slice(&entry.key.secondary.to_le_bytes());
            out.extend_from_slice(&entry.sound_id.0.to_le_bytes());
            out.extend_from_slice(&entry.unknown_10.to_le_bytes());
            out.push(entry.filename.bytes.len() as u8);
            out.extend_from_slice(&entry.filename.bytes);
            out.extend_from_slice(&entry.filename.padding);
            out.extend_from_slice(&entry.unknown_47);
        }
        out.extend_from_slice(&self.trailing_bytes);
        Ok(out)
    }

    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let data = self.to_bytes()?;
        fs::write(path, data).map_err(|source| SoundInfoError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn resolve_path_by_key(
        &self,
        key: SoundKey,
        wave_dir: impl AsRef<Path>,
    ) -> Result<Option<PathBuf>> {
        match self.entry_by_key(key) {
            Some(entry) => SoundFileResolver::new(wave_dir).resolve_entry(entry),
            None => Ok(None),
        }
    }

    pub fn resolve_path_by_sound_id(
        &self,
        sound_id: AssetId,
        wave_dir: impl AsRef<Path>,
    ) -> Result<Option<PathBuf>> {
        match self.entry_by_sound_id(sound_id) {
            Some(entry) => SoundFileResolver::new(wave_dir).resolve_entry(entry),
            None => Ok(None),
        }
    }
}

fn read_wav_files(wave_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut wav_files = Vec::new();
    let directory = fs::read_dir(wave_dir).map_err(|source| SoundInfoError::Io {
        path: wave_dir.to_path_buf(),
        source,
    })?;
    for item in directory {
        let item = item.map_err(|source| SoundInfoError::Io {
            path: wave_dir.to_path_buf(),
            source,
        })?;
        let file_type = item.file_type().map_err(|source| SoundInfoError::Io {
            path: item.path(),
            source,
        })?;
        if file_type.is_file()
            && item
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
        {
            wav_files.push(item.path());
        }
    }
    wav_files.sort();
    Ok(wav_files)
}

fn validate_filename_length(length: usize) -> Result<()> {
    if length > SOUND_INFO_FILENAME_CAPACITY {
        return Err(SoundInfoError::FilenameTooLong {
            length,
            capacity: SOUND_INFO_FILENAME_CAPACITY,
        });
    }
    Ok(())
}

fn read_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("sound row has a fixed length"),
    )
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn parses_and_exactly_reserializes_every_stored_byte() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2_i32.to_le_bytes());
        push_row(
            &mut bytes,
            SoundKey::new(4, 12, -3),
            -1,
            0x1234_5678,
            &[0x81, 0x41],
            0xa0,
            0xb0,
        );
        push_row(
            &mut bytes,
            SoundKey::new(1, 0, 7),
            1007,
            -99,
            b"click.1007.wav",
            0xc0,
            0xd0,
        );
        bytes.extend_from_slice(&[0xee, 0xff]);

        let table = SoundInfoTable::from_bytes(&bytes).unwrap();
        assert_eq!(table.entries().len(), 2);
        assert_eq!(table.entries()[0].key, SoundKey::new(4, 12, -3));
        assert_eq!(table.entries()[0].sound_id, AssetId(-1));
        assert!(!table.entries()[0].is_enabled());
        assert_eq!(table.entries()[0].filename.as_bytes(), [0x81, 0x41]);
        assert_eq!(table.trailing_bytes(), [0xee, 0xff]);
        assert_eq!(table.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn accepts_an_empty_table() {
        let table = SoundInfoTable::from_bytes(&0_i32.to_le_bytes()).unwrap();
        assert!(table.entries().is_empty());
        assert_eq!(table.to_bytes().unwrap(), 0_i32.to_le_bytes());
    }

    #[test]
    fn rejects_negative_count_and_truncated_rows() {
        for length in 0..SOUND_INFO_HEADER_LEN {
            assert!(matches!(
                SoundInfoTable::from_bytes(&[0; SOUND_INFO_HEADER_LEN][..length]),
                Err(SoundInfoError::TruncatedHeader { .. })
            ));
        }

        assert!(matches!(
            SoundInfoTable::from_bytes(&(-1_i32).to_le_bytes()),
            Err(SoundInfoError::InvalidEntryCount { count: -1 })
        ));

        for length in SOUND_INFO_HEADER_LEN..SOUND_INFO_HEADER_LEN + SOUND_INFO_ENTRY_LEN {
            let mut truncated = 1_i32.to_le_bytes().to_vec();
            truncated.resize(length, 0);
            assert!(matches!(
                SoundInfoTable::from_bytes(&truncated),
                Err(SoundInfoError::TruncatedTable { .. })
            ));
        }

        assert!(matches!(
            SoundInfoTable::from_bytes(&i32::MAX.to_le_bytes()),
            Err(SoundInfoError::TruncatedTable { .. })
        ));
    }

    #[test]
    fn rejects_invalid_filename_lengths_and_padding() {
        let mut bytes = 1_i32.to_le_bytes().to_vec();
        bytes.resize(SOUND_INFO_HEADER_LEN + SOUND_INFO_ENTRY_LEN, 0);
        bytes[SOUND_INFO_HEADER_LEN + FILENAME_OFFSET] = 51;
        assert!(matches!(
            SoundInfoTable::from_bytes(&bytes),
            Err(SoundInfoError::InvalidFilenameLength { .. })
        ));

        assert!(matches!(
            SoundFilename::new(vec![0; 51]),
            Err(SoundInfoError::FilenameTooLong { .. })
        ));
        assert!(matches!(
            SoundFilename::from_parts(b"abc".to_vec(), vec![0; 46]),
            Err(SoundInfoError::InvalidFilenamePadding { .. })
        ));
    }

    #[test]
    fn lookups_return_the_first_source_order_match() {
        let key = SoundKey::new(3, 7, 0);
        let table = SoundInfoTable::new(vec![
            entry(key, 30007, b"first.wav"),
            entry(SoundKey::new(1, 0, 1), 10, b"other.wav"),
            entry(key, 30008, b"second.wav"),
            entry(SoundKey::new(5, 2, 1), 10, b"duplicate.wav"),
        ]);

        assert_eq!(table.entry_by_key(key).unwrap().sound_id, AssetId(30007));
        assert_eq!(
            table
                .entry_by_sound_id(AssetId(10))
                .unwrap()
                .filename
                .as_bytes(),
            b"other.wav"
        );
        assert!(table.entry_by_sound_id(AssetId(999)).is_none());
    }

    #[test]
    fn resolves_stored_stripped_and_sound_id_fallback_names() {
        let root = temp_dir("resolve");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("exact.wav"), b"").unwrap();
        fs::write(root.join("BGM (1).30000"), b"").unwrap();
        fs::write(root.join("alternate-42.wav"), b"").unwrap();

        let table = SoundInfoTable::new(vec![
            entry(SoundKey::new(1, 0, 1), 1, b"exact.wav"),
            entry(SoundKey::new(3, 0, 0), 30000, b"BGM (1).30000.wav"),
            entry(SoundKey::new(2, 1, 0), 42, b"missing.wav"),
            entry(SoundKey::new(2, 2, 0), 999, b"absent.wav"),
        ]);

        assert_eq!(
            table.resolve_path_by_sound_id(AssetId(1), &root).unwrap(),
            Some(root.join("exact.wav"))
        );
        assert_eq!(
            table
                .resolve_path_by_key(SoundKey::new(3, 0, 0), &root)
                .unwrap(),
            Some(root.join("BGM (1).30000"))
        );
        assert_eq!(
            table.resolve_path_by_sound_id(AssetId(42), &root).unwrap(),
            Some(root.join("alternate-42.wav"))
        );
        assert_eq!(
            table.resolve_path_by_sound_id(AssetId(999), &root).unwrap(),
            None
        );
        assert_eq!(
            table.resolve_path_by_sound_id(AssetId(123), &root).unwrap(),
            None
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolver_uses_the_exact_entry_when_sound_ids_are_duplicated() {
        let root = temp_dir("duplicate-id-resolution");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("first.wav"), b"").unwrap();
        fs::write(root.join("second.wav"), b"").unwrap();
        let first = entry(SoundKey::new(1, 0, 1), 10, b"first.wav");
        let second = entry(SoundKey::new(2, 0, 1), 10, b"second.wav");
        let mut resolver = SoundFileResolver::new(&root);

        assert_eq!(
            resolver.resolve_entry(&first).unwrap(),
            Some(root.join("first.wav"))
        );
        assert_eq!(
            resolver.resolve_entry(&second).unwrap(),
            Some(root.join("second.wav"))
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolver_reuses_its_fallback_directory_snapshot() {
        let root = temp_dir("resolver-cache");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("fallback-41.wav"), b"").unwrap();
        let first = entry(SoundKey::new(2, 0, 1), 41, b"missing-first.wav");
        let second = entry(SoundKey::new(2, 0, 2), 42, b"missing-second.wav");
        let mut resolver = SoundFileResolver::new(&root);

        assert_eq!(
            resolver.resolve_entry(&first).unwrap(),
            Some(root.join("fallback-41.wav"))
        );
        assert!(resolver.wav_files.is_some());
        fs::write(root.join("fallback-42.wav"), b"").unwrap();
        assert_eq!(resolver.resolve_entry(&second).unwrap(), None);

        fs::remove_dir_all(root).unwrap();
    }

    fn entry(key: SoundKey, sound_id: i32, filename: &[u8]) -> SoundInfoEntry {
        SoundInfoEntry::new(
            key,
            AssetId(sound_id),
            SoundFilename::new(filename.to_vec()).unwrap(),
        )
    }

    fn push_row(
        out: &mut Vec<u8>,
        key: SoundKey,
        sound_id: i32,
        unknown_10: i32,
        filename: &[u8],
        padding_byte: u8,
        unknown_byte: u8,
    ) {
        out.extend_from_slice(&key.group.to_le_bytes());
        out.extend_from_slice(&key.primary.to_le_bytes());
        out.extend_from_slice(&key.secondary.to_le_bytes());
        out.extend_from_slice(&sound_id.to_le_bytes());
        out.extend_from_slice(&unknown_10.to_le_bytes());
        out.push(filename.len() as u8);
        out.extend_from_slice(filename);
        out.resize(
            out.len() + SOUND_INFO_FILENAME_CAPACITY - filename.len(),
            padding_byte,
        );
        out.resize(out.len() + SOUND_INFO_UNKNOWN_47_LEN, unknown_byte);
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-audio-{label}-{nonce}"))
    }
}
