//! Binary `.NOS` archive support.
//!
//! A binary `.NOS` archive is a small header and index table followed by raw
//! record chunks. Each record can be stored uncompressed or as a zlib stream.
//! This module exposes a patch-agnostic editor: callers can open archives from
//! disk or memory, inspect and read records, replace/delete/insert records, and
//! rebuild bytes with explicit write options.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};
use taletool_core::{ByteReader, SourceRef};
use taletool_zlib::{ZlibProfile, compress_zlib112_profile};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BinaryNosArchiveError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("archive {path} is too small")]
    TooSmall { path: PathBuf },
    #[error("archive {path} has invalid header count {count}")]
    InvalidCount { path: PathBuf, count: i32 },
    #[error("archive {path} entry table extends past end of file")]
    EntryTableOutOfBounds { path: PathBuf },
    #[error("archive {path} entry {file_id} has invalid data offset {offset}")]
    InvalidDataOffset {
        path: PathBuf,
        file_id: i32,
        offset: i32,
    },
    #[error("archive {path} entry {file_id} payload extends past end of file")]
    PayloadOutOfBounds { path: PathBuf, file_id: i32 },
    #[error("archive {path} entry {file_id} uses unsupported compression flag {flag}")]
    UnsupportedCompression {
        path: PathBuf,
        file_id: i32,
        flag: u8,
    },
    #[error("archive {path} entry {file_id} zlib decode failed: {source}")]
    Zlib {
        path: PathBuf,
        file_id: i32,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "archive {path} entry {file_id} unpacked size mismatch: header={expected}, actual={actual}"
    )]
    SizeMismatch {
        path: PathBuf,
        file_id: i32,
        expected: usize,
        actual: usize,
    },
    #[error("file id {file_id} was not found in archive family {family}")]
    MissingFileId { family: String, file_id: i32 },
    #[error("archive {path} record index {index} is out of bounds; record count is {count}")]
    RecordIndexOutOfBounds {
        path: PathBuf,
        index: usize,
        count: usize,
    },
    #[error("no archives found for family {family} in {data_dir}")]
    MissingFamily { data_dir: PathBuf, family: String },
    #[error("archive has too many entries: {count}")]
    TooManyEntries { count: usize },
    #[error("archive entry {file_id} is too large: {size} bytes")]
    EntryTooLarge { file_id: i32, size: usize },
    #[error("archive data offset for entry {file_id} is too large: {offset}")]
    DataOffsetTooLarge { file_id: i32, offset: usize },
    #[error("failed to zlib encode archive entry {file_id}: {message}")]
    ZlibEncode { file_id: i32, message: String },
}

pub type BinaryNosArchiveResult<T> = std::result::Result<T, BinaryNosArchiveError>;

/// Record compression used by binary `.NOS` archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryCompression {
    Raw,
    Zlib,
}

/// One entry in a parsed archive index table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryNosArchiveEntry {
    pub file_id: i32,
    pub data_offset: usize,
    pub unpacked_size: usize,
    pub stored_size: usize,
    pub compression: BinaryCompression,
}

/// Decoded archive record used by the mutation API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryNosArchiveRecord {
    pub file_id: i32,
    pub compression: BinaryCompression,
    pub data: Vec<u8>,
}

/// Decoded payload returned with its source archive/file ID.
#[derive(Debug, Clone)]
pub struct BinaryEntryPayload {
    pub source: SourceRef,
    pub data: Vec<u8>,
}

/// Record input for archive rebuilds and writes.
#[derive(Debug, Clone)]
pub struct BinaryNosArchiveWriteEntry {
    pub file_id: i32,
    pub compression: Option<BinaryCompression>,
    pub data: Vec<u8>,
}

impl BinaryNosArchiveWriteEntry {
    pub fn new(file_id: i32, data: Vec<u8>) -> Self {
        Self {
            file_id,
            compression: None,
            data,
        }
    }

    pub fn with_compression(file_id: i32, compression: BinaryCompression, data: Vec<u8>) -> Self {
        Self {
            file_id,
            compression: Some(compression),
            data,
        }
    }
}

impl From<BinaryNosArchiveRecord> for BinaryNosArchiveWriteEntry {
    fn from(record: BinaryNosArchiveRecord) -> Self {
        Self {
            file_id: record.file_id,
            compression: Some(record.compression),
            data: record.data,
        }
    }
}

/// Archive-wide write settings used when rebuilding bytes.
#[derive(Debug, Clone, Copy)]
pub struct BinaryNosArchiveWriteOptions {
    pub header: [u8; 16],
    pub direct_index: u8,
    pub compression: BinaryCompression,
    pub zlib_profile: ZlibProfile,
}

impl BinaryNosArchiveWriteOptions {
    pub fn new(
        header: [u8; 16],
        direct_index: u8,
        compression: BinaryCompression,
        zlib_profile: ZlibProfile,
    ) -> Self {
        Self {
            header,
            direct_index,
            compression,
            zlib_profile,
        }
    }
}

/// Parsed numeric-ID binary `.NOS` archive with neutral record editing methods.
///
/// This is the format used by files such as `NStgData.NOS`, `NStpData.NOS`,
/// and their split archive families. It is intentionally unaware of PCHPKG
/// patch opcodes; callers edit records and rebuild the container explicitly.
#[derive(Debug, Clone)]
pub struct BinaryNosArchive {
    path: PathBuf,
    data: Vec<u8>,
    direct_index: u8,
    entries: Vec<BinaryNosArchiveEntry>,
}

impl BinaryNosArchive {
    /// Read and parse a binary `.NOS` archive from disk.
    pub fn open(path: impl AsRef<Path>) -> BinaryNosArchiveResult<Self> {
        let path = path.as_ref().to_path_buf();
        let data = fs::read(&path).map_err(|source| BinaryNosArchiveError::Io {
            path: path.clone(),
            source,
        })?;
        Self::from_bytes(path, data)
    }

    /// Parse a binary `.NOS` archive from bytes without an on-disk path.
    pub fn from_memory(data: Vec<u8>) -> BinaryNosArchiveResult<Self> {
        Self::from_bytes(PathBuf::from("<memory>"), data)
    }

    /// Build a new archive from decoded record inputs and parse the result.
    pub fn from_entries(
        path: impl Into<PathBuf>,
        entries: Vec<BinaryNosArchiveWriteEntry>,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<Self> {
        let path = path.into();
        let data = write_binary_nos_archive_bytes(&entries, options)?;
        Self::from_bytes(path, data)
    }

    /// Build an empty archive with the supplied write options.
    pub fn empty(
        path: impl Into<PathBuf>,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<Self> {
        Self::from_entries(path, Vec::new(), options)
    }

    /// Parse a binary `.NOS` archive from bytes while preserving its logical path.
    pub fn from_bytes(path: PathBuf, data: Vec<u8>) -> BinaryNosArchiveResult<Self> {
        if data.len() < 21 {
            return Err(BinaryNosArchiveError::TooSmall { path });
        }

        let count = read_i32(&data, 16);
        if count < 0 {
            return Err(BinaryNosArchiveError::InvalidCount { path, count });
        }

        let count = count as usize;
        let table_end = 21usize.saturating_add(count.saturating_mul(8));
        if table_end > data.len() {
            return Err(BinaryNosArchiveError::EntryTableOutOfBounds { path });
        }

        let direct_index = data[20];
        let mut entries = Vec::with_capacity(count);
        let mut table_offset = 21;

        for _ in 0..count {
            let file_id = read_i32(&data, table_offset);
            let data_offset = read_i32(&data, table_offset + 4);
            table_offset += 8;

            if data_offset < 0 {
                return Err(BinaryNosArchiveError::InvalidDataOffset {
                    path,
                    file_id,
                    offset: data_offset,
                });
            }

            let data_offset = data_offset as usize;
            if data_offset.saturating_add(13) > data.len() {
                return Err(BinaryNosArchiveError::InvalidDataOffset {
                    path,
                    file_id,
                    offset: data_offset as i32,
                });
            }

            let unpacked_size = read_u32(&data, data_offset + 4) as usize;
            let stored_size = read_u32(&data, data_offset + 8) as usize;
            let flag = data[data_offset + 12];
            let compression = match flag {
                0 => BinaryCompression::Raw,
                1 => BinaryCompression::Zlib,
                _ => {
                    return Err(BinaryNosArchiveError::UnsupportedCompression {
                        path,
                        file_id,
                        flag,
                    });
                }
            };

            if data_offset.saturating_add(13).saturating_add(stored_size) > data.len() {
                return Err(BinaryNosArchiveError::PayloadOutOfBounds { path, file_id });
            }

            entries.push(BinaryNosArchiveEntry {
                file_id,
                data_offset,
                unpacked_size,
                stored_size,
                compression,
            });
        }

        Ok(Self {
            path,
            data,
            direct_index,
            entries,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    /// Write the current archive bytes to an explicit path.
    pub fn write_to(&self, path: impl AsRef<Path>) -> BinaryNosArchiveResult<()> {
        let path = path.as_ref();
        fs::write(path, &self.data).map_err(|source| BinaryNosArchiveError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Save the current archive bytes back to the path it was opened with.
    pub fn save(&self) -> BinaryNosArchiveResult<()> {
        self.write_to(&self.path)
    }

    /// Return the archive header bytes that are reused by rebuild operations.
    pub fn header(&self) -> [u8; 16] {
        let mut header = [0_u8; 16];
        header.copy_from_slice(&self.data[..16]);
        header
    }

    /// Return the direct-index flag byte from the archive header.
    pub fn direct_index(&self) -> u8 {
        self.direct_index
    }

    /// Return parsed entry-table metadata without decoding payloads.
    pub fn entries(&self) -> &[BinaryNosArchiveEntry] {
        &self.entries
    }

    /// Decode every record into editable payload bytes.
    pub fn records(&self) -> BinaryNosArchiveResult<Vec<BinaryNosArchiveRecord>> {
        self.entries
            .iter()
            .map(|entry| self.record_from_entry(entry))
            .collect()
    }

    /// Decode every record into writer inputs that preserve per-entry compression.
    pub fn write_entries(&self) -> BinaryNosArchiveResult<Vec<BinaryNosArchiveWriteEntry>> {
        Ok(self
            .records()?
            .into_iter()
            .map(BinaryNosArchiveWriteEntry::from)
            .collect())
    }

    /// Find the first entry-table index with the supplied file ID.
    pub fn find_entry_index(&self, file_id: i32) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.file_id == file_id)
    }

    /// Find the first entry-table entry with the supplied file ID.
    pub fn find_entry(&self, file_id: i32) -> Option<&BinaryNosArchiveEntry> {
        self.find_entry_index(file_id)
            .and_then(|index| self.entries.get(index))
    }

    /// Read and decode the first payload with the supplied file ID.
    pub fn read_entry(&self, file_id: i32) -> BinaryNosArchiveResult<Option<BinaryEntryPayload>> {
        let Some(entry) = self.find_entry(file_id) else {
            return Ok(None);
        };
        Ok(Some(self.read_entry_payload(entry)?))
    }

    /// Decode the payload for a specific parsed entry-table entry.
    pub fn read_entry_payload(
        &self,
        entry: &BinaryNosArchiveEntry,
    ) -> BinaryNosArchiveResult<BinaryEntryPayload> {
        let stored = self.read_entry_stored_bytes(entry)?;
        let data = match entry.compression {
            BinaryCompression::Raw => stored.to_vec(),
            BinaryCompression::Zlib => {
                let mut decoder = ZlibDecoder::new(stored);
                let mut decoded = Vec::new();
                decoder.read_to_end(&mut decoded).map_err(|source| {
                    BinaryNosArchiveError::Zlib {
                        path: self.path.clone(),
                        file_id: entry.file_id,
                        source,
                    }
                })?;
                decoded
            }
        };

        if entry.unpacked_size != 0 && data.len() != entry.unpacked_size {
            return Err(BinaryNosArchiveError::SizeMismatch {
                path: self.path.clone(),
                file_id: entry.file_id,
                expected: entry.unpacked_size,
                actual: data.len(),
            });
        }

        Ok(BinaryEntryPayload {
            source: SourceRef {
                archive: self.path.clone(),
                file_id: entry.file_id,
            },
            data,
        })
    }

    /// Return stored bytes for an entry without applying decompression.
    pub fn read_entry_stored_bytes(
        &self,
        entry: &BinaryNosArchiveEntry,
    ) -> BinaryNosArchiveResult<&[u8]> {
        let start = entry.data_offset + 13;
        let end = start + entry.stored_size;
        self.data
            .get(start..end)
            .ok_or_else(|| BinaryNosArchiveError::PayloadOutOfBounds {
                path: self.path.clone(),
                file_id: entry.file_id,
            })
    }

    /// Rebuild archive bytes using the supplied write options.
    pub fn to_bytes_with_options(
        &self,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<Vec<u8>> {
        write_binary_nos_archive_bytes(&self.write_entries()?, options)
    }

    /// Replace the whole archive with a new set of decoded record inputs.
    pub fn rebuild_from_entries(
        &mut self,
        entries: Vec<BinaryNosArchiveWriteEntry>,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<()> {
        let path = self.path.clone();
        *self = Self::from_entries(path, entries, options)?;
        Ok(())
    }

    /// Append a decoded record and rebuild the archive.
    pub fn push_record(
        &mut self,
        record: BinaryNosArchiveWriteEntry,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<usize> {
        let mut records = self.write_entries()?;
        let index = records.len();
        records.push(record);
        self.rebuild_from_entries(records, options)?;
        Ok(index)
    }

    /// Insert a decoded record at an entry-table index and rebuild the archive.
    pub fn insert_record(
        &mut self,
        index: usize,
        record: BinaryNosArchiveWriteEntry,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<()> {
        let mut records = self.write_entries()?;
        if index > records.len() {
            return Err(BinaryNosArchiveError::RecordIndexOutOfBounds {
                path: self.path.clone(),
                index,
                count: records.len(),
            });
        }
        records.insert(index, record);
        self.rebuild_from_entries(records, options)
    }

    /// Replace the first record with the supplied file ID and rebuild the archive.
    pub fn replace_record(
        &mut self,
        file_id: i32,
        data: Vec<u8>,
        compression: Option<BinaryCompression>,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<Option<BinaryNosArchiveRecord>> {
        let Some(index) = self.find_entry_index(file_id) else {
            return Ok(None);
        };
        self.replace_record_at(
            index,
            BinaryNosArchiveWriteEntry {
                file_id,
                compression,
                data,
            },
            options,
        )
        .map(Some)
    }

    /// Replace a record by entry-table index and rebuild the archive.
    pub fn replace_record_at(
        &mut self,
        index: usize,
        record: BinaryNosArchiveWriteEntry,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<BinaryNosArchiveRecord> {
        let mut records = self.write_entries()?;
        if index >= records.len() {
            return Err(BinaryNosArchiveError::RecordIndexOutOfBounds {
                path: self.path.clone(),
                index,
                count: records.len(),
            });
        }
        let old = self.record_at(index)?;
        records[index] = record;
        self.rebuild_from_entries(records, options)?;
        Ok(old)
    }

    /// Remove the first record with the supplied file ID and rebuild the archive.
    pub fn remove_record(
        &mut self,
        file_id: i32,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<Option<BinaryNosArchiveRecord>> {
        let Some(index) = self.find_entry_index(file_id) else {
            return Ok(None);
        };
        self.remove_record_at(index, options).map(Some)
    }

    /// Remove a record by entry-table index and rebuild the archive.
    pub fn remove_record_at(
        &mut self,
        index: usize,
        options: &BinaryNosArchiveWriteOptions,
    ) -> BinaryNosArchiveResult<BinaryNosArchiveRecord> {
        let mut records = self.write_entries()?;
        if index >= records.len() {
            return Err(BinaryNosArchiveError::RecordIndexOutOfBounds {
                path: self.path.clone(),
                index,
                count: records.len(),
            });
        }
        let old = self.record_at(index)?;
        records.remove(index);
        self.rebuild_from_entries(records, options)?;
        Ok(old)
    }

    fn record_at(&self, index: usize) -> BinaryNosArchiveResult<BinaryNosArchiveRecord> {
        let entry = self.entries.get(index).ok_or_else(|| {
            BinaryNosArchiveError::RecordIndexOutOfBounds {
                path: self.path.clone(),
                index,
                count: self.entries.len(),
            }
        })?;
        self.record_from_entry(entry)
    }

    fn record_from_entry(
        &self,
        entry: &BinaryNosArchiveEntry,
    ) -> BinaryNosArchiveResult<BinaryNosArchiveRecord> {
        Ok(BinaryNosArchiveRecord {
            file_id: entry.file_id,
            compression: entry.compression,
            data: self.read_entry_payload(entry)?.data,
        })
    }
}

/// Rebuild binary `.NOS` archive bytes from decoded record inputs.
pub fn write_binary_nos_archive_bytes(
    entries: &[BinaryNosArchiveWriteEntry],
    options: &BinaryNosArchiveWriteOptions,
) -> BinaryNosArchiveResult<Vec<u8>> {
    let count =
        i32::try_from(entries.len()).map_err(|_| BinaryNosArchiveError::TooManyEntries {
            count: entries.len(),
        })?;
    let table_len = entries
        .len()
        .checked_mul(8)
        .and_then(|value| 21usize.checked_add(value))
        .ok_or(BinaryNosArchiveError::TooManyEntries {
            count: entries.len(),
        })?;

    let mut table = Vec::with_capacity(table_len);
    table.extend_from_slice(&options.header);
    table.extend_from_slice(&count.to_le_bytes());
    table.push(options.direct_index);

    let mut chunks = Vec::new();
    let mut next_offset = table_len;
    for entry in entries {
        let data_offset =
            i32::try_from(next_offset).map_err(|_| BinaryNosArchiveError::DataOffsetTooLarge {
                file_id: entry.file_id,
                offset: next_offset,
            })?;
        table.extend_from_slice(&entry.file_id.to_le_bytes());
        table.extend_from_slice(&data_offset.to_le_bytes());

        let unpacked_size =
            u32::try_from(entry.data.len()).map_err(|_| BinaryNosArchiveError::EntryTooLarge {
                file_id: entry.file_id,
                size: entry.data.len(),
            })?;
        let compression = entry.compression.unwrap_or(options.compression);
        let (stored, flag) = match compression {
            BinaryCompression::Raw => (entry.data.clone(), 0_u8),
            BinaryCompression::Zlib => (
                compress_zlib112_profile(&entry.data, options.zlib_profile).map_err(|source| {
                    BinaryNosArchiveError::ZlibEncode {
                        file_id: entry.file_id,
                        message: source.to_string(),
                    }
                })?,
                1_u8,
            ),
        };
        let stored_size =
            u32::try_from(stored.len()).map_err(|_| BinaryNosArchiveError::EntryTooLarge {
                file_id: entry.file_id,
                size: stored.len(),
            })?;

        chunks.extend_from_slice(&entry.file_id.to_le_bytes());
        chunks.extend_from_slice(&unpacked_size.to_le_bytes());
        chunks.extend_from_slice(&stored_size.to_le_bytes());
        chunks.push(flag);
        chunks.extend_from_slice(&stored);
        next_offset = next_offset
            .checked_add(13)
            .and_then(|value| value.checked_add(stored.len()))
            .ok_or(BinaryNosArchiveError::DataOffsetTooLarge {
                file_id: entry.file_id,
                offset: usize::MAX,
            })?;
    }

    table.extend_from_slice(&chunks);
    Ok(table)
}

/// A family of split `.NOS` archives loaded together.
#[derive(Debug, Clone)]
pub struct BinaryNosSplitArchive {
    family: String,
    archives: Vec<BinaryNosArchive>,
}

impl BinaryNosSplitArchive {
    /// Open either a single archive family file or all split chunks for a family.
    pub fn open_family(data_dir: impl AsRef<Path>, family: &str) -> BinaryNosArchiveResult<Self> {
        let data_dir = data_dir.as_ref();
        let exact = data_dir.join(format!("{family}.NOS"));
        let mut paths = Vec::new();

        if exact.exists() {
            paths.push(exact);
        } else {
            let entries = fs::read_dir(data_dir).map_err(|source| BinaryNosArchiveError::Io {
                path: data_dir.to_path_buf(),
                source,
            })?;
            for entry in entries {
                let entry = entry.map_err(|source| BinaryNosArchiveError::Io {
                    path: data_dir.to_path_buf(),
                    source,
                })?;
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if name.starts_with(family) && name.ends_with(".NOS") {
                    paths.push(path);
                }
            }
            paths.sort();
        }

        if paths.is_empty() {
            return Err(BinaryNosArchiveError::MissingFamily {
                data_dir: data_dir.to_path_buf(),
                family: family.to_owned(),
            });
        }

        let mut archives = Vec::with_capacity(paths.len());
        for path in paths {
            archives.push(BinaryNosArchive::open(path)?);
        }

        Ok(Self {
            family: family.to_owned(),
            archives,
        })
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    pub fn archives(&self) -> &[BinaryNosArchive] {
        &self.archives
    }

    pub fn entries(&self) -> impl Iterator<Item = (&BinaryNosArchive, &BinaryNosArchiveEntry)> {
        self.archives
            .iter()
            .flat_map(|archive| archive.entries().iter().map(move |entry| (archive, entry)))
    }

    pub fn read_entry(&self, file_id: i32) -> BinaryNosArchiveResult<BinaryEntryPayload> {
        if self.archives.len() > 1 {
            let selected = (file_id as u32 & 0xff) as usize;
            if let Some(archive) = self.archives.get(selected)
                && let Some(payload) = archive.read_entry(file_id)?
            {
                return Ok(payload);
            }
        }

        for archive in &self.archives {
            if let Some(payload) = archive.read_entry(file_id)? {
                return Ok(payload);
            }
        }

        Err(BinaryNosArchiveError::MissingFileId {
            family: self.family.clone(),
            file_id,
        })
    }
}

fn read_i32(data: &[u8], offset: usize) -> i32 {
    ByteReader::new_at(data, offset)
        .read_i32_le("i32")
        .expect("caller validates i32 offset")
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    ByteReader::new_at(data, offset)
        .read_u32_le("u32")
        .expect("caller validates u32 offset")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_archive_entry() {
        let mut data = vec![0_u8; 21 + 8];
        data[16..20].copy_from_slice(&1_i32.to_le_bytes());
        data[21..25].copy_from_slice(&7_i32.to_le_bytes());
        data[25..29].copy_from_slice(&29_i32.to_le_bytes());
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&3_u32.to_le_bytes());
        data.extend_from_slice(&3_u32.to_le_bytes());
        data.push(0);
        data.extend_from_slice(&[1, 2, 3]);

        let archive = BinaryNosArchive::from_bytes(PathBuf::from("fixture.NOS"), data).unwrap();
        let payload = archive.read_entry(7).unwrap().unwrap();
        assert_eq!(payload.data, vec![1, 2, 3]);
    }

    #[test]
    fn reads_duplicate_ids_by_specific_entry() {
        let mut data = vec![0_u8; 21 + 16];
        data[16..20].copy_from_slice(&2_i32.to_le_bytes());
        data[21..25].copy_from_slice(&7_i32.to_le_bytes());
        data[25..29].copy_from_slice(&37_i32.to_le_bytes());
        data[29..33].copy_from_slice(&7_i32.to_le_bytes());
        data[33..37].copy_from_slice(&51_i32.to_le_bytes());

        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.push(0);
        data.push(1);

        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.extend_from_slice(&1_u32.to_le_bytes());
        data.push(0);
        data.push(2);

        let archive = BinaryNosArchive::from_bytes(PathBuf::from("fixture.NOS"), data).unwrap();
        assert_eq!(archive.read_entry(7).unwrap().unwrap().data, vec![1]);
        assert_eq!(
            archive
                .read_entry_payload(&archive.entries()[1])
                .unwrap()
                .data,
            vec![2]
        );
    }

    #[test]
    fn writes_raw_archive_entry() {
        let options = BinaryNosArchiveWriteOptions {
            header: *b"NT Data 06\0\0\x15\x07\x04 ",
            direct_index: 0,
            compression: BinaryCompression::Raw,
            zlib_profile: ZlibProfile::default_level(9),
        };
        let data = write_binary_nos_archive_bytes(
            &[BinaryNosArchiveWriteEntry {
                file_id: 7,
                compression: None,
                data: vec![1, 2, 3],
            }],
            &options,
        )
        .unwrap();
        let archive = BinaryNosArchive::from_bytes(PathBuf::from("fixture.NOS"), data).unwrap();
        assert_eq!(archive.entries()[0].compression, BinaryCompression::Raw);
        assert_eq!(archive.read_entry(7).unwrap().unwrap().data, vec![1, 2, 3]);
    }

    #[test]
    fn writes_zlib_archive_entry() {
        let options = BinaryNosArchiveWriteOptions {
            header: *b"NT Data 02\0\0\x15\x07\x04 ",
            direct_index: 0,
            compression: BinaryCompression::Zlib,
            zlib_profile: ZlibProfile::default_level(1),
        };
        let data = write_binary_nos_archive_bytes(
            &[BinaryNosArchiveWriteEntry {
                file_id: 7,
                compression: None,
                data: vec![1, 2, 3, 4, 5, 6],
            }],
            &options,
        )
        .unwrap();
        let archive = BinaryNosArchive::from_bytes(PathBuf::from("fixture.NOS"), data).unwrap();
        assert_eq!(archive.entries()[0].compression, BinaryCompression::Zlib);
        assert_eq!(
            archive
                .read_entry_stored_bytes(&archive.entries()[0])
                .unwrap(),
            compress_zlib112_profile(&[1, 2, 3, 4, 5, 6], ZlibProfile::default_level(1)).unwrap()
        );
        assert_eq!(
            archive.read_entry(7).unwrap().unwrap().data,
            vec![1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn writes_entry_compression_overrides() {
        let options = BinaryNosArchiveWriteOptions {
            header: *b"NT Data 02\0\0\x15\x07\x04 ",
            direct_index: 0,
            compression: BinaryCompression::Zlib,
            zlib_profile: ZlibProfile::default_level(9),
        };
        let data = write_binary_nos_archive_bytes(
            &[
                BinaryNosArchiveWriteEntry {
                    file_id: 1,
                    compression: None,
                    data: b"default zlib".to_vec(),
                },
                BinaryNosArchiveWriteEntry {
                    file_id: 2,
                    compression: Some(BinaryCompression::Raw),
                    data: b"forced raw".to_vec(),
                },
            ],
            &options,
        )
        .unwrap();
        let archive = BinaryNosArchive::from_bytes(PathBuf::from("fixture.NOS"), data).unwrap();

        assert_eq!(archive.entries()[0].compression, BinaryCompression::Zlib);
        assert_eq!(archive.entries()[1].compression, BinaryCompression::Raw);
        assert_eq!(
            archive.read_entry(1).unwrap().unwrap().data,
            b"default zlib"
        );
        assert_eq!(archive.read_entry(2).unwrap().unwrap().data, b"forced raw");
    }

    #[test]
    fn builds_archive_from_scratch_and_reads_from_memory() {
        let options = BinaryNosArchiveWriteOptions::new(
            *b"NT Data 06\0\0\x15\x07\x04 ",
            0,
            BinaryCompression::Raw,
            ZlibProfile::default_level(9),
        );
        let archive = BinaryNosArchive::from_entries(
            PathBuf::from("scratch.NOS"),
            vec![
                BinaryNosArchiveWriteEntry::new(7, b"seven".to_vec()),
                BinaryNosArchiveWriteEntry::with_compression(
                    8,
                    BinaryCompression::Raw,
                    b"eight".to_vec(),
                ),
            ],
            &options,
        )
        .unwrap();

        assert_eq!(archive.entries().len(), 2);
        assert_eq!(archive.records().unwrap()[0].data, b"seven");

        let memory = BinaryNosArchive::from_memory(archive.as_bytes().to_vec()).unwrap();
        assert_eq!(memory.read_entry(8).unwrap().unwrap().data, b"eight");
    }

    #[test]
    fn mutates_archive_records_and_rebuilds_metadata() {
        let options = BinaryNosArchiveWriteOptions::new(
            *b"NT Data 06\0\0\x15\x07\x04 ",
            0,
            BinaryCompression::Raw,
            ZlibProfile::default_level(9),
        );
        let mut archive = BinaryNosArchive::empty(PathBuf::from("mutable.NOS"), &options).unwrap();

        let inserted_index = archive
            .push_record(
                BinaryNosArchiveWriteEntry::new(10, b"ten".to_vec()),
                &options,
            )
            .unwrap();
        assert_eq!(inserted_index, 0);
        archive
            .insert_record(
                0,
                BinaryNosArchiveWriteEntry::new(5, b"five".to_vec()),
                &options,
            )
            .unwrap();

        let old = archive
            .replace_record(
                10,
                b"updated".to_vec(),
                Some(BinaryCompression::Raw),
                &options,
            )
            .unwrap()
            .unwrap();
        assert_eq!(old.data, b"ten");

        let removed = archive.remove_record(5, &options).unwrap().unwrap();
        assert_eq!(removed.data, b"five");

        assert_eq!(archive.entries().len(), 1);
        assert_eq!(archive.entries()[0].file_id, 10);
        assert_eq!(archive.read_entry(10).unwrap().unwrap().data, b"updated");
        assert_eq!(archive.entries()[0].data_offset, 21 + 8);
    }
}
