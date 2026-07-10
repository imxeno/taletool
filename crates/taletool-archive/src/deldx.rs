//! DelDX pack file support.
//!
//! NosTale stores sound data in DelDX pack containers such as `snd.pck`.
//! This module owns neutral container parsing, editing, and rebuilding. It also
//! keeps the original patch mutation helper used by `taletool-patch`, but does
//! not know about package ordering or filesystem commit policy.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DELDX_PACK_HEADER_LEN: usize = 0x1c;
pub const DELDX_PACK_ROW_LEN: usize = 0x4c;
pub const DELDX_PACK_ROW_PREFIX_LEN: usize = 0x44;
pub const DELDX_PACK_RESERVED_HEADER_OFFSET: usize = 0x11;
pub const DELDX_PACK_RESERVED_HEADER_LEN: usize = 3;

const PACK_PATCH_PREFIX_LEN: usize = DELDX_PACK_HEADER_LEN + 4;
const PACK_PATCH_INLINE_ROW_LEN: usize = 0x50;
const PACK_DATA_OFFSET: usize = 0x44;
const PACK_DATA_SIZE: usize = 0x48;
const PACK_VERSION_OFFSET: usize = 0x14;
const PACK_COUNT_OFFSET: usize = 0x18;
const PACK_RECORD_COUNT_OFFSET: usize = 0x1c;
const PACK_MAGIC_LEN: u8 = 16;
const PACK_MAGIC: &[u8] = b"DelDX Pack File ";

#[derive(Debug, Error)]
pub enum DelDxPackError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("DelDX pack {path} is invalid: {message}")]
    InvalidArchive { path: PathBuf, message: String },
    #[error("DelDX pack write options are invalid: {message}")]
    InvalidWriteOptions { message: String },
    #[error("DelDX pack write entry {index} is invalid: {message}")]
    InvalidWriteEntry { index: usize, message: String },
    #[error("DelDX pack record index {index} is out of bounds; record count is {count}")]
    RecordIndexOutOfBounds {
        path: PathBuf,
        index: usize,
        count: usize,
    },
    #[error("DelDX pack has too many entries: {count}")]
    TooManyEntries { count: usize },
    #[error("DelDX pack entry {index} payload is too large: {size} bytes")]
    EntryTooLarge { index: usize, size: usize },
}

pub type DelDxPackResult<T> = std::result::Result<T, DelDxPackError>;

/// One row in a parsed DelDX pack table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelDxPackEntry {
    pub index: usize,
    pub name: String,
    pub name_bytes: Vec<u8>,
    pub key: i32,
    pub row_prefix: Vec<u8>,
    pub data_offset: usize,
    pub data_size: usize,
}

/// Decoded DelDX pack record used by the mutation API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelDxPackRecord {
    pub index: usize,
    pub name: String,
    pub name_bytes: Vec<u8>,
    pub key: i32,
    pub row_prefix: Vec<u8>,
    pub data: Vec<u8>,
}

/// Record input for DelDX pack rebuilds and writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelDxPackWriteEntry {
    pub row_prefix: Vec<u8>,
    pub data: Vec<u8>,
}

impl DelDxPackWriteEntry {
    pub fn new(row_prefix: Vec<u8>, data: Vec<u8>) -> Self {
        Self { row_prefix, data }
    }
}

impl From<DelDxPackRecord> for DelDxPackWriteEntry {
    fn from(record: DelDxPackRecord) -> Self {
        Self {
            row_prefix: record.row_prefix,
            data: record.data,
        }
    }
}

/// Archive-wide write settings used when rebuilding DelDX pack bytes.
#[derive(Debug, Clone, Copy)]
pub struct DelDxPackWriteOptions {
    pub header: [u8; DELDX_PACK_HEADER_LEN],
}

impl DelDxPackWriteOptions {
    pub fn new(header: [u8; DELDX_PACK_HEADER_LEN]) -> Self {
        Self {
            header: normalize_deldx_pack_header_for_write(header),
        }
    }
}

/// Parsed DelDX sound pack with neutral record editing methods.
#[derive(Debug, Clone)]
pub struct DelDxPack {
    path: PathBuf,
    data: Vec<u8>,
    entries: Vec<DelDxPackEntry>,
    sorted_unique_indices: Vec<Option<usize>>,
    first_index_by_key: BTreeMap<i32, usize>,
}

impl DelDxPack {
    /// Read and parse a DelDX pack from disk.
    pub fn open(path: impl AsRef<Path>) -> DelDxPackResult<Self> {
        let path = path.as_ref().to_path_buf();
        let data = fs::read(&path).map_err(|source| DelDxPackError::Io {
            path: path.clone(),
            source,
        })?;
        Self::from_bytes(path, data)
    }

    /// Parse a DelDX pack from bytes without an on-disk path.
    pub fn from_memory(data: Vec<u8>) -> DelDxPackResult<Self> {
        Self::from_bytes(PathBuf::from("<memory>"), data)
    }

    /// Build a new pack from decoded record inputs and parse the result.
    pub fn from_entries(
        path: impl Into<PathBuf>,
        entries: Vec<DelDxPackWriteEntry>,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<Self> {
        let path = path.into();
        let data = write_deldx_pack_bytes(&entries, options)?;
        Self::from_bytes(path, data)
    }

    /// Build an empty DelDX pack with the supplied write options.
    pub fn empty(
        path: impl Into<PathBuf>,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<Self> {
        Self::from_entries(path, Vec::new(), options)
    }

    /// Parse a DelDX pack from bytes while preserving its logical path.
    pub fn from_bytes(path: PathBuf, data: Vec<u8>) -> DelDxPackResult<Self> {
        if data.len() < DELDX_PACK_HEADER_LEN {
            return Err(archive_invalid(
                &path,
                "file is shorter than the DelDX header",
            ));
        }
        validate_pack_header(&data[..DELDX_PACK_HEADER_LEN])
            .map_err(|error| archive_invalid(&path, error.to_string()))?;

        let count = read_i32_at(&data, PACK_COUNT_OFFSET, "DelDX pack file count")
            .map_err(|error| archive_invalid(&path, error.to_string()))?;
        if count < 0 {
            return Err(archive_invalid(
                &path,
                format!("negative file count: {count}"),
            ));
        }
        let count = count as usize;
        let table_end =
            pack_table_end(count).map_err(|error| archive_invalid(&path, error.to_string()))?;
        if table_end > data.len() {
            return Err(archive_invalid(
                &path,
                format!("table ends at {table_end}, beyond file size {}", data.len()),
            ));
        }

        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let row_start = DELDX_PACK_HEADER_LEN + index * DELDX_PACK_ROW_LEN;
            let row_end = row_start + DELDX_PACK_ROW_LEN;
            let row = data
                .get(row_start..row_end)
                .ok_or_else(|| archive_invalid(&path, format!("row {index} is truncated")))?;
            validate_row_name(row).map_err(|error| archive_invalid(&path, error.to_string()))?;

            let row_prefix = row[..DELDX_PACK_ROW_PREFIX_LEN].to_vec();
            let name_bytes = row_name_bytes(&row_prefix)
                .map_err(|error| archive_invalid(&path, error.to_string()))?;
            let key = derive_row_key(name_bytes, index);
            let data_offset = read_u32_at(row, PACK_DATA_OFFSET, "DelDX pack data offset")
                .map_err(|error| archive_invalid(&path, error.to_string()))?
                as usize;
            let data_size = read_u32_at(row, PACK_DATA_SIZE, "DelDX pack data size")
                .map_err(|error| archive_invalid(&path, error.to_string()))?
                as usize;

            if data_offset > data.len() {
                return Err(archive_invalid(
                    &path,
                    format!("entry {index} data offset {data_offset} is beyond file size"),
                ));
            }
            if data_size > 0 && data_offset < table_end {
                return Err(archive_invalid(
                    &path,
                    format!("entry {index} points into the index table"),
                ));
            }
            let data_end = data_offset.checked_add(data_size).ok_or_else(|| {
                archive_invalid(&path, format!("entry {index} payload end overflow"))
            })?;
            if data_end > data.len() {
                return Err(archive_invalid(
                    &path,
                    format!("entry {index} data ends beyond file size"),
                ));
            }

            entries.push(DelDxPackEntry {
                index,
                name: String::from_utf8_lossy(name_bytes).into_owned(),
                name_bytes: name_bytes.to_vec(),
                key,
                row_prefix,
                data_offset,
                data_size,
            });
        }

        let (sorted_unique_indices, first_index_by_key) = build_key_indexes(&entries);
        Ok(Self {
            path,
            data,
            entries,
            sorted_unique_indices,
            first_index_by_key,
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

    /// Write the current pack bytes to an explicit path.
    pub fn write_to(&self, path: impl AsRef<Path>) -> DelDxPackResult<()> {
        let path = path.as_ref();
        fs::write(path, &self.data).map_err(|source| DelDxPackError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Save the current pack bytes back to the path it was opened with.
    pub fn save(&self) -> DelDxPackResult<()> {
        self.write_to(&self.path)
    }

    /// Return the parsed header bytes.
    ///
    /// Rebuild operations normalize reserved header bytes before writing.
    pub fn header(&self) -> [u8; DELDX_PACK_HEADER_LEN] {
        let mut header = [0_u8; DELDX_PACK_HEADER_LEN];
        header.copy_from_slice(&self.data[..DELDX_PACK_HEADER_LEN]);
        header
    }

    /// Return parsed table metadata without copying payload bytes.
    pub fn entries(&self) -> &[DelDxPackEntry] {
        &self.entries
    }

    /// Decode every record into editable payload bytes.
    pub fn records(&self) -> DelDxPackResult<Vec<DelDxPackRecord>> {
        self.entries
            .iter()
            .map(|entry| self.record_from_entry(entry))
            .collect()
    }

    /// Decode every record into writer inputs that preserve row metadata.
    pub fn write_entries(&self) -> DelDxPackResult<Vec<DelDxPackWriteEntry>> {
        Ok(self
            .records()?
            .into_iter()
            .map(DelDxPackWriteEntry::from)
            .collect())
    }

    /// Read the payload for a specific parsed table entry.
    pub fn read_entry_payload(&self, entry: &DelDxPackEntry) -> DelDxPackResult<Vec<u8>> {
        let start = entry.data_offset;
        let end = start
            .checked_add(entry.data_size)
            .ok_or_else(|| archive_invalid(&self.path, "entry payload end overflow"))?;
        Ok(self
            .data
            .get(start..end)
            .ok_or_else(|| {
                archive_invalid(
                    &self.path,
                    format!("entry {} payload is outside the file", entry.index),
                )
            })?
            .to_vec())
    }

    /// Rebuild pack bytes using the supplied write options.
    pub fn to_bytes_with_options(
        &self,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<Vec<u8>> {
        write_deldx_pack_bytes(&self.write_entries()?, options)
    }

    /// Replace the whole pack with a new set of decoded record inputs.
    pub fn rebuild_from_entries(
        &mut self,
        entries: Vec<DelDxPackWriteEntry>,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<()> {
        let path = self.path.clone();
        *self = Self::from_entries(path, entries, options)?;
        Ok(())
    }

    /// Append a decoded record and rebuild the pack.
    pub fn push_record(
        &mut self,
        record: DelDxPackWriteEntry,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<usize> {
        let mut records = self.write_entries()?;
        let index = records.len();
        records.push(record);
        self.rebuild_from_entries(records, options)?;
        Ok(index)
    }

    /// Insert a decoded record at a table index and rebuild the pack.
    pub fn insert_record(
        &mut self,
        index: usize,
        record: DelDxPackWriteEntry,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<()> {
        let mut records = self.write_entries()?;
        if index > records.len() {
            return Err(DelDxPackError::RecordIndexOutOfBounds {
                path: self.path.clone(),
                index,
                count: records.len(),
            });
        }
        records.insert(index, record);
        self.rebuild_from_entries(records, options)
    }

    /// Replace a record by table index and rebuild the pack.
    pub fn replace_record_at(
        &mut self,
        index: usize,
        record: DelDxPackWriteEntry,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<DelDxPackRecord> {
        let mut records = self.write_entries()?;
        if index >= records.len() {
            return Err(DelDxPackError::RecordIndexOutOfBounds {
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

    /// Remove a record by table index and rebuild the pack.
    pub fn remove_record_at(
        &mut self,
        index: usize,
        options: &DelDxPackWriteOptions,
    ) -> DelDxPackResult<DelDxPackRecord> {
        let mut records = self.write_entries()?;
        if index >= records.len() {
            return Err(DelDxPackError::RecordIndexOutOfBounds {
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

    fn record_at(&self, index: usize) -> DelDxPackResult<DelDxPackRecord> {
        let entry =
            self.entries
                .get(index)
                .ok_or_else(|| DelDxPackError::RecordIndexOutOfBounds {
                    path: self.path.clone(),
                    index,
                    count: self.entries.len(),
                })?;
        self.record_from_entry(entry)
    }

    fn record_from_entry(&self, entry: &DelDxPackEntry) -> DelDxPackResult<DelDxPackRecord> {
        Ok(DelDxPackRecord {
            index: entry.index,
            name: entry.name.clone(),
            name_bytes: entry.name_bytes.clone(),
            key: entry.key,
            row_prefix: entry.row_prefix.clone(),
            data: self.read_entry_payload(entry)?,
        })
    }

    fn find_entry_index_for_mutation(&self, key: i32, expected_sorted_index: i32) -> Option<usize> {
        if expected_sorted_index >= 0 {
            let expected_sorted_index = expected_sorted_index as usize;
            if let Some(Some(index)) = self.sorted_unique_indices.get(expected_sorted_index)
                && self.entries[*index].key == key
            {
                return Some(*index);
            }
        }
        self.first_index_by_key.get(&key).copied()
    }
}

/// Rebuild DelDX pack bytes from decoded record inputs.
pub fn write_deldx_pack_bytes(
    entries: &[DelDxPackWriteEntry],
    options: &DelDxPackWriteOptions,
) -> DelDxPackResult<Vec<u8>> {
    validate_pack_header(&options.header).map_err(|error| DelDxPackError::InvalidWriteOptions {
        message: error.to_string(),
    })?;
    let count = i32::try_from(entries.len()).map_err(|_| DelDxPackError::TooManyEntries {
        count: entries.len(),
    })?;
    let table_end =
        pack_table_end(entries.len()).map_err(|error| DelDxPackError::InvalidWriteOptions {
            message: error.to_string(),
        })?;

    let mut out = normalize_deldx_pack_header_for_write(options.header).to_vec();
    out[PACK_COUNT_OFFSET..PACK_COUNT_OFFSET + 4].copy_from_slice(&count.to_le_bytes());
    out.resize(table_end, 0);
    for (index, entry) in entries.iter().enumerate() {
        validate_write_entry(index, entry)?;
        append_pack_entry(&mut out, index, &entry.row_prefix, &entry.data).map_err(|error| {
            DelDxPackError::InvalidWriteEntry {
                index,
                message: error.to_string(),
            }
        })?;
    }

    DelDxPack::from_memory(out.clone())?;
    Ok(out)
}

/// Return DelDX header bytes in the canonical form.
///
/// Bytes `0x11..0x13` are reserved/ignored by the original client loader. Old
/// packs have been observed with non-zero values there; Taletool almays
/// writes zeros.
pub fn normalize_deldx_pack_header_for_write(
    mut header: [u8; DELDX_PACK_HEADER_LEN],
) -> [u8; DELDX_PACK_HEADER_LEN] {
    header[DELDX_PACK_RESERVED_HEADER_OFFSET
        ..DELDX_PACK_RESERVED_HEADER_OFFSET + DELDX_PACK_RESERVED_HEADER_LEN]
        .fill(0);
    header
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedArchiveMutation {
    header: [u8; DELDX_PACK_HEADER_LEN],
    output_count: usize,
    records: Vec<PackedMutationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackedMutationRecord {
    Skip {
        target_key: i32,
        source_key: i32,
        source_index: i32,
    },
    Inline {
        tag: u8,
        row: Vec<u8>,
        payload: Vec<u8>,
    },
    Copy {
        target_key: i32,
        source_key: i32,
        source_index: i32,
    },
    NoOutput {
        tag: u8,
    },
}

impl PackedArchiveMutation {
    /// Parse a DelDX mutation stream without applying it.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < PACK_PATCH_PREFIX_LEN {
            bail!("DelDX pack mutation is shorter than its header");
        }
        validate_pack_header(&bytes[..DELDX_PACK_HEADER_LEN])?;

        let output_count = read_i32_at(bytes, PACK_COUNT_OFFSET, "DelDX mutation output count")?;
        if output_count < 0 {
            bail!("DelDX mutation has negative output count: {output_count}");
        }
        let output_count = output_count as usize;

        let record_count = read_i32_at(
            bytes,
            PACK_RECORD_COUNT_OFFSET,
            "DelDX mutation record count",
        )?;
        if record_count < 0 {
            bail!("DelDX mutation has negative record count: {record_count}");
        }
        let record_count = record_count as usize;

        let mut pos = PACK_PATCH_PREFIX_LEN;
        let mut records = Vec::with_capacity(record_count);
        let mut output_record_count = 0usize;

        for index in 0..record_count {
            let tag = *bytes
                .get(pos)
                .with_context(|| format!("DelDX mutation record {index} is missing its tag"))?;
            pos += 1;

            match tag {
                0 => {
                    let target_key = read_i32_at(bytes, pos, "DelDX skip target key")?;
                    let source_key = read_i32_at(bytes, pos + 4, "DelDX skip source key")?;
                    let source_index = read_i32_at(bytes, pos + 8, "DelDX skip source index")?;
                    pos += 12;
                    records.push(PackedMutationRecord::Skip {
                        target_key,
                        source_key,
                        source_index,
                    });
                }
                1 | 2 => {
                    let row_end =
                        checked_add(pos, PACK_PATCH_INLINE_ROW_LEN, "DelDX inline row end")?;
                    let row = bytes
                        .get(pos..row_end)
                        .with_context(|| format!("DelDX inline record {index} is truncated"))?;
                    validate_patch_row(row)
                        .with_context(|| format!("validating DelDX inline record {index}"))?;
                    let payload_size =
                        read_i32_at(row, PACK_DATA_SIZE, "DelDX inline payload size")?;
                    if payload_size < 0 {
                        bail!(
                            "DelDX inline record {index} has negative payload size: {payload_size}"
                        );
                    }
                    pos = row_end;
                    let payload_size = payload_size as usize;
                    let payload_end = checked_add(pos, payload_size, "DelDX inline payload end")?;
                    let payload = bytes
                        .get(pos..payload_end)
                        .with_context(|| format!("DelDX inline payload {index} is truncated"))?;
                    pos = payload_end;
                    output_record_count += 1;
                    records.push(PackedMutationRecord::Inline {
                        tag,
                        row: row.to_vec(),
                        payload: payload.to_vec(),
                    });
                }
                5 => {
                    let target_key = read_i32_at(bytes, pos, "DelDX copy target key")?;
                    let source_key = read_i32_at(bytes, pos + 4, "DelDX copy source key")?;
                    let source_index = read_i32_at(bytes, pos + 8, "DelDX copy source index")?;
                    pos += 12;
                    output_record_count += 1;
                    records.push(PackedMutationRecord::Copy {
                        target_key,
                        source_key,
                        source_index,
                    });
                }
                _ => records.push(PackedMutationRecord::NoOutput { tag }),
            }
        }

        if pos != bytes.len() {
            bail!(
                "DelDX mutation has {} trailing bytes after its records",
                bytes.len() - pos
            );
        }
        if output_record_count != output_count {
            bail!(
                "DelDX mutation output count mismatch: header={output_count}, records={output_record_count}"
            );
        }

        let mut header = [0_u8; DELDX_PACK_HEADER_LEN];
        header.copy_from_slice(&bytes[..DELDX_PACK_HEADER_LEN]);
        Ok(Self {
            header,
            output_count,
            records,
        })
    }
}

/// Apply a DelDX pack mutation to base pack bytes and return rebuilt bytes.
pub fn apply_packed_archive_mutation(base_bytes: &[u8], mutation_bytes: &[u8]) -> Result<Vec<u8>> {
    let base = DelDxPack::from_memory(base_bytes.to_vec()).context("parsing base DelDX pack")?;
    let mutation =
        PackedArchiveMutation::parse(mutation_bytes).context("parsing DelDX pack mutation")?;

    let mut entries = Vec::with_capacity(mutation.output_count);
    for record in &mutation.records {
        match record {
            PackedMutationRecord::Skip { .. } | PackedMutationRecord::NoOutput { .. } => {}
            PackedMutationRecord::Inline { row, payload, .. } => {
                entries.push(DelDxPackWriteEntry::new(
                    row[..DELDX_PACK_ROW_PREFIX_LEN].to_vec(),
                    payload.clone(),
                ));
            }
            PackedMutationRecord::Copy {
                target_key,
                source_index,
                ..
            } => {
                if let Some(index) = base.find_entry_index_for_mutation(*target_key, *source_index)
                {
                    entries.push(DelDxPackWriteEntry::from(base.record_at(index)?));
                } else {
                    entries.push(DelDxPackWriteEntry::new(
                        vec![0; DELDX_PACK_ROW_PREFIX_LEN],
                        Vec::new(),
                    ));
                }
            }
        }
    }

    if entries.len() != mutation.output_count {
        bail!(
            "DelDX mutation wrote {} entries, expected {}",
            entries.len(),
            mutation.output_count
        );
    }
    write_deldx_pack_bytes(
        &entries,
        &DelDxPackWriteOptions {
            header: mutation.header,
        },
    )
    .context("validating reconstructed DelDX pack")
}

fn archive_invalid(path: &Path, message: impl Into<String>) -> DelDxPackError {
    DelDxPackError::InvalidArchive {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

fn build_key_indexes(entries: &[DelDxPackEntry]) -> (Vec<Option<usize>>, BTreeMap<i32, usize>) {
    let mut sorted_indices: Vec<_> = (0..entries.len()).collect();
    sorted_indices.sort_by_key(|index| entries[*index].key);
    let mut sorted_unique_indices = Vec::with_capacity(sorted_indices.len());
    let mut first_index_by_key = BTreeMap::new();
    let mut previous_key = None;
    for index in sorted_indices {
        let key = entries[index].key;
        if previous_key == Some(key) {
            sorted_unique_indices.push(None);
        } else {
            sorted_unique_indices.push(Some(index));
            first_index_by_key.entry(key).or_insert(index);
            previous_key = Some(key);
        }
    }
    (sorted_unique_indices, first_index_by_key)
}

fn validate_write_entry(index: usize, entry: &DelDxPackWriteEntry) -> DelDxPackResult<()> {
    if entry.row_prefix.len() != DELDX_PACK_ROW_PREFIX_LEN {
        return Err(DelDxPackError::InvalidWriteEntry {
            index,
            message: format!(
                "row_prefix must be {DELDX_PACK_ROW_PREFIX_LEN} bytes, got {}",
                entry.row_prefix.len()
            ),
        });
    }
    validate_row_name(&entry.row_prefix).map_err(|error| DelDxPackError::InvalidWriteEntry {
        index,
        message: error.to_string(),
    })?;
    u32::try_from(entry.data.len()).map_err(|_| DelDxPackError::EntryTooLarge {
        index,
        size: entry.data.len(),
    })?;
    Ok(())
}

fn append_pack_entry(
    out: &mut Vec<u8>,
    output_index: usize,
    row_prefix: &[u8],
    payload: &[u8],
) -> Result<()> {
    if row_prefix.len() < DELDX_PACK_ROW_PREFIX_LEN {
        bail!("DelDX row prefix is too short");
    }
    let table_offset = DELDX_PACK_HEADER_LEN + output_index * DELDX_PACK_ROW_LEN;
    let data_offset = u32::try_from(out.len()).context("DelDX pack exceeds 4 GiB")?;
    let payload_size = u32::try_from(payload.len()).context("DelDX pack entry exceeds 4 GiB")?;
    out[table_offset..table_offset + DELDX_PACK_ROW_PREFIX_LEN]
        .copy_from_slice(&row_prefix[..DELDX_PACK_ROW_PREFIX_LEN]);
    out[table_offset + PACK_DATA_OFFSET..table_offset + PACK_DATA_OFFSET + 4]
        .copy_from_slice(&data_offset.to_le_bytes());
    out[table_offset + PACK_DATA_SIZE..table_offset + PACK_DATA_SIZE + 4]
        .copy_from_slice(&payload_size.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

fn validate_pack_header(header: &[u8]) -> Result<()> {
    if header.len() < DELDX_PACK_HEADER_LEN {
        bail!("DelDX pack header is truncated");
    }
    if header[0] != PACK_MAGIC_LEN || &header[1..1 + PACK_MAGIC.len()] != PACK_MAGIC {
        bail!("DelDX pack header magic is not recognized");
    }
    let version = read_i32_at(header, PACK_VERSION_OFFSET, "DelDX pack version")?;
    if version > 10 {
        bail!("DelDX pack version {version} is newer than supported version 10");
    }
    Ok(())
}

fn validate_patch_row(row: &[u8]) -> Result<()> {
    if row.len() < PACK_PATCH_INLINE_ROW_LEN {
        bail!("DelDX inline row is truncated");
    }
    validate_row_name(row)
}

fn validate_row_name(row: &[u8]) -> Result<()> {
    if row.len() < DELDX_PACK_ROW_PREFIX_LEN {
        bail!("DelDX row prefix is truncated");
    }
    let name_len = row[0] as usize;
    if name_len >= DELDX_PACK_ROW_PREFIX_LEN {
        bail!("DelDX row name length {name_len} exceeds field capacity");
    }
    Ok(())
}

fn row_name_bytes(row: &[u8]) -> Result<&[u8]> {
    validate_row_name(row)?;
    let name_len = row[0] as usize;
    Ok(&row[1..1 + name_len])
}

fn derive_row_key(name: &[u8], fallback_index: usize) -> i32 {
    let Some(first_dot) = name.iter().position(|byte| *byte == b'.') else {
        return fallback_index as i32;
    };
    let Some(second_dot_relative) = name[first_dot + 1..].iter().position(|byte| *byte == b'.')
    else {
        return fallback_index as i32;
    };
    let second_dot = first_dot + 1 + second_dot_relative;
    std::str::from_utf8(&name[first_dot + 1..second_dot])
        .ok()
        .and_then(|key| key.parse::<i32>().ok())
        .unwrap_or(-1)
}

fn pack_table_end(count: usize) -> Result<usize> {
    checked_add(
        DELDX_PACK_HEADER_LEN,
        count
            .checked_mul(DELDX_PACK_ROW_LEN)
            .context("DelDX pack table size overflow")?,
        "DelDX pack table end",
    )
}

fn read_i32_at(bytes: &[u8], offset: usize, label: &str) -> Result<i32> {
    Ok(i32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .with_context(|| format!("missing {label}"))?
            .try_into()
            .expect("slice length checked"),
    ))
}

fn read_u32_at(bytes: &[u8], offset: usize, label: &str) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .with_context(|| format!("missing {label}"))?
            .try_into()
            .expect("slice length checked"),
    ))
}

fn checked_add(lhs: usize, rhs: usize, label: &str) -> Result<usize> {
    lhs.checked_add(rhs)
        .with_context(|| format!("{label} overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(count: usize) -> Vec<u8> {
        let mut out = vec![0; DELDX_PACK_HEADER_LEN];
        out[0] = PACK_MAGIC_LEN;
        out[1..1 + PACK_MAGIC.len()].copy_from_slice(PACK_MAGIC);
        out[PACK_VERSION_OFFSET..PACK_VERSION_OFFSET + 4].copy_from_slice(&10i32.to_le_bytes());
        out[PACK_COUNT_OFFSET..PACK_COUNT_OFFSET + 4]
            .copy_from_slice(&(count as i32).to_le_bytes());
        out
    }

    fn header_array(count: usize) -> [u8; DELDX_PACK_HEADER_LEN] {
        let mut out = [0; DELDX_PACK_HEADER_LEN];
        out.copy_from_slice(&header(count));
        out
    }

    fn row(name: &[u8], payload_len: usize) -> Vec<u8> {
        let mut row = vec![0; PACK_PATCH_INLINE_ROW_LEN];
        row[0] = name.len() as u8;
        row[1..1 + name.len()].copy_from_slice(name);
        row[PACK_DATA_SIZE..PACK_DATA_SIZE + 4]
            .copy_from_slice(&(payload_len as u32).to_le_bytes());
        row
    }

    fn pack(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut out = header(entries.len());
        out.resize(
            DELDX_PACK_HEADER_LEN + entries.len() * DELDX_PACK_ROW_LEN,
            0,
        );
        for (index, (name, payload)) in entries.iter().enumerate() {
            let table_offset = DELDX_PACK_HEADER_LEN + index * DELDX_PACK_ROW_LEN;
            let mut row = row(name, payload.len());
            let data_offset = out.len() as u32;
            row[PACK_DATA_OFFSET..PACK_DATA_OFFSET + 4].copy_from_slice(&data_offset.to_le_bytes());
            out[table_offset..table_offset + DELDX_PACK_ROW_LEN]
                .copy_from_slice(&row[..DELDX_PACK_ROW_LEN]);
            out.extend_from_slice(payload);
        }
        out
    }

    fn mutation(output_count: usize, records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = header(output_count);
        out.extend_from_slice(&(records.len() as i32).to_le_bytes());
        for record in records {
            out.extend_from_slice(record);
        }
        out
    }

    fn copy_record(key: i32, source_index: i32) -> Vec<u8> {
        let mut out = vec![5];
        out.extend_from_slice(&key.to_le_bytes());
        out.extend_from_slice(&key.to_le_bytes());
        out.extend_from_slice(&source_index.to_le_bytes());
        out
    }

    fn inline_record(tag: u8, name: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        out.extend_from_slice(&row(name, payload.len()));
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn parses_valid_pack() {
        let bytes = pack(&[(b"base.10.wav", b"sound")]);
        let parsed = DelDxPack::from_memory(bytes).unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].key, 10);
        assert_eq!(
            parsed.read_entry_payload(&parsed.entries[0]).unwrap(),
            b"sound"
        );
    }

    #[test]
    fn rebuilds_pack_from_entries() {
        let bytes = pack(&[(b"base.10.wav", b"sound"), (b"base.20.wav", b"")]);
        let parsed = DelDxPack::from_memory(bytes.clone()).unwrap();
        let rebuilt = write_deldx_pack_bytes(
            &parsed.write_entries().unwrap(),
            &DelDxPackWriteOptions::new(parsed.header()),
        )
        .unwrap();

        assert_eq!(rebuilt, bytes);
        let reparsed = DelDxPack::from_memory(rebuilt).unwrap();
        assert_eq!(reparsed.entries[0].name, "base.10.wav");
        assert_eq!(reparsed.entries[1].data_size, 0);
    }

    #[test]
    fn rebuild_zeroes_reserved_header_bytes() {
        let mut bytes = pack(&[(b"base.10.wav", b"sound")]);
        bytes[DELDX_PACK_RESERVED_HEADER_OFFSET
            ..DELDX_PACK_RESERVED_HEADER_OFFSET + DELDX_PACK_RESERVED_HEADER_LEN]
            .copy_from_slice(&[0xf0, 0xfd, 0x7f]);
        let parsed = DelDxPack::from_memory(bytes).unwrap();
        let rebuilt = write_deldx_pack_bytes(
            &parsed.write_entries().unwrap(),
            &DelDxPackWriteOptions {
                header: parsed.header(),
            },
        )
        .unwrap();

        assert_eq!(
            &rebuilt[DELDX_PACK_RESERVED_HEADER_OFFSET
                ..DELDX_PACK_RESERVED_HEADER_OFFSET + DELDX_PACK_RESERVED_HEADER_LEN],
            &[0, 0, 0]
        );
        assert_eq!(
            DelDxPack::from_memory(rebuilt)
                .unwrap()
                .write_entries()
                .unwrap(),
            parsed.write_entries().unwrap()
        );
    }

    #[test]
    fn mutates_pack_records_and_rebuilds_metadata() {
        let parsed = DelDxPack::from_memory(pack(&[(b"base.10.wav", b"sound")])).unwrap();
        let options = DelDxPackWriteOptions::new(parsed.header());
        let mut archive = DelDxPack::empty(PathBuf::from("snd.pck"), &options).unwrap();
        archive
            .push_record(
                DelDxPackWriteEntry::new(parsed.entries[0].row_prefix.clone(), b"new".to_vec()),
                &options,
            )
            .unwrap();
        archive
            .replace_record_at(
                0,
                DelDxPackWriteEntry::new(
                    parsed.entries[0].row_prefix.clone(),
                    b"replaced".to_vec(),
                ),
                &options,
            )
            .unwrap();

        assert_eq!(archive.entries()[0].data_size, b"replaced".len());
        assert_eq!(
            archive.read_entry_payload(&archive.entries()[0]).unwrap(),
            b"replaced"
        );
    }

    #[test]
    fn rejects_too_new_pack_version() {
        let mut bytes = header(0);
        bytes[PACK_VERSION_OFFSET..PACK_VERSION_OFFSET + 4].copy_from_slice(&11i32.to_le_bytes());

        assert!(DelDxPack::from_memory(bytes).is_err());
    }

    #[test]
    fn rejects_invalid_write_row_name() {
        let mut prefix = vec![0; DELDX_PACK_ROW_PREFIX_LEN];
        prefix[0] = DELDX_PACK_ROW_PREFIX_LEN as u8;
        let error = write_deldx_pack_bytes(
            &[DelDxPackWriteEntry::new(prefix, Vec::new())],
            &DelDxPackWriteOptions::new(header_array(1)),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("name length"));
    }

    #[test]
    fn applies_copy_inline_and_missing_copy_records() {
        let base = pack(&[(b"base.10.wav", b"base-a"), (b"base.20.wav", b"base-b")]);
        let mutation = mutation(
            3,
            &[
                copy_record(10, 0),
                inline_record(1, b"new.30.wav", b"new"),
                copy_record(999, 100),
            ],
        );

        let out = apply_packed_archive_mutation(&base, &mutation).unwrap();
        let parsed = DelDxPack::from_memory(out).unwrap();
        assert_eq!(parsed.entries.len(), 3);
        assert_eq!(parsed.entries[0].key, 10);
        assert_eq!(parsed.entries[1].key, 30);
        assert_eq!(parsed.entries[2].data_size, 0);
        assert_eq!(
            parsed.read_entry_payload(&parsed.entries[0]).unwrap(),
            b"base-a"
        );
        assert_eq!(
            parsed.read_entry_payload(&parsed.entries[1]).unwrap(),
            b"new"
        );
    }

    #[test]
    fn rejects_truncated_inline_payload() {
        let mut record = inline_record(1, b"new.30.wav", b"new");
        record.pop();
        let mutation = mutation(1, &[record]);
        assert!(PackedArchiveMutation::parse(&mutation).is_err());
    }

    #[test]
    fn parses_updater_no_output_marker_tags() {
        let mutation = mutation(0, &[vec![3], vec![4], vec![6], vec![255]]);
        let parsed = PackedArchiveMutation::parse(&mutation).unwrap();

        assert_eq!(
            parsed.records,
            vec![
                PackedMutationRecord::NoOutput { tag: 3 },
                PackedMutationRecord::NoOutput { tag: 4 },
                PackedMutationRecord::NoOutput { tag: 6 },
                PackedMutationRecord::NoOutput { tag: 255 },
            ]
        );
    }
}
