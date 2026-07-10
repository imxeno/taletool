//! Typed support for CCINF `.NOS` files.
//!
//! `NSmnData.NOS` and `NSpnData.NOS` store a compact GBFC index behind a
//! 25-byte wrapper. Even though they are `.NOS` files, they are modeled as single
//! structured files, not multi-entry containers. Known files are always raw:
//! the wrapper's unpacked and stored sizes are equal and its compression flag is zero.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// 16-byte header used by known CCINF `.NOS` files.
pub const CCINF_HEADER: [u8; 16] = *b"CCINF V1.20\x1a\x14\x11\x04 ";
/// Total wrapper length before the entry count and entry records.
pub const CCINF_PREFIX_LEN: usize = 0x19;
/// Number of counted cell lists stored in every CCINF entry.
pub const CCINF_CELL_LIST_COUNT: usize = 7;

/// Return whether a byte slice begins with the CCINF signature.
///
/// This is intentionally a shallow check. Use [`Ccinf::from_memory`] or
/// [`Ccinf::from_bytes`] to validate the complete file.
pub fn has_ccinf_header(data: &[u8]) -> bool {
    data.starts_with(&CCINF_HEADER)
}

#[derive(Debug, Error)]
pub enum CcinfError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("CCINF file {path} is too small")]
    TooSmall { path: PathBuf },
    #[error("CCINF file {path} has an invalid header")]
    InvalidHeader { path: PathBuf },
    #[error("CCINF file {path} uses unsupported compression flag {flag}")]
    UnsupportedCompression { path: PathBuf, flag: u8 },
    #[error("CCINF file {path} {field} mismatch: header={declared}, actual={actual}")]
    SizeMismatch {
        path: PathBuf,
        field: &'static str,
        declared: usize,
        actual: usize,
    },
    #[error("CCINF file {path} has invalid entry count {count}")]
    InvalidEntryCount { path: PathBuf, count: i32 },
    #[error(
        "CCINF file {path} field {field} is truncated at offset {offset}: need {needed} bytes, got {actual}"
    )]
    TruncatedFile {
        path: PathBuf,
        field: &'static str,
        offset: usize,
        needed: usize,
        actual: usize,
    },
    #[error("CCINF file {path} has {count} trailing bytes")]
    TrailingBytes { path: PathBuf, count: usize },
    #[error(
        "CCINF entries are not sorted by unsigned entry id at index {index}: {previous} before {current}"
    )]
    UnsortedEntries {
        index: usize,
        previous: i32,
        current: i32,
    },
    #[error(
        "CCINF entry {entry_id} cell list {list_index} is not sorted by selector at index {index}: {previous} before {current}"
    )]
    UnsortedCellList {
        entry_id: i32,
        list_index: usize,
        index: usize,
        previous: u16,
        current: u16,
    },
    #[error("CCINF file has too many entries: {count}")]
    TooManyEntries { count: usize },
    #[error("CCINF file body is too large: {size} bytes")]
    BodyTooLarge { size: usize },
    #[error("CCINF entry {entry_id} cell list {list_index} has too many cells: {count}")]
    TooManyCells {
        entry_id: i32,
        list_index: usize,
        count: usize,
    },
}

pub type CcinfResult<T> = std::result::Result<T, CcinfError>;

/// One packed six-byte cell from a CCINF entry's selector list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CcinfCell {
    pub selector: u16,
    pub texture_resource_key: i32,
}

/// One typed GBFC index entry from a CCINF file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CcinfEntry {
    pub entry_id: i32,
    pub base_resource_key: i32,
    pub remap_table_file_id: i32,
    pub animation_file_id: i32,
    pub cell_lists: [Vec<CcinfCell>; CCINF_CELL_LIST_COUNT],
}

/// CCINF `.NOS` file.
#[derive(Debug, Clone)]
pub struct Ccinf {
    path: PathBuf,
    data: Vec<u8>,
    unpacked_size: usize,
    stored_size: usize,
    entries: Vec<CcinfEntry>,
}

impl Ccinf {
    /// Read and parse a CCINF file from disk.
    pub fn open(path: impl AsRef<Path>) -> CcinfResult<Self> {
        let path = path.as_ref().to_path_buf();
        let data = fs::read(&path).map_err(|source| CcinfError::Io {
            path: path.clone(),
            source,
        })?;
        Self::from_bytes(path, data)
    }

    /// Parse a CCINF file from bytes without an on-disk path.
    pub fn from_memory(data: Vec<u8>) -> CcinfResult<Self> {
        Self::from_bytes(PathBuf::from("<memory>"), data)
    }

    /// Build a raw CCINF file from typed entries.
    pub fn from_entries(path: impl Into<PathBuf>, entries: Vec<CcinfEntry>) -> CcinfResult<Self> {
        let path = path.into();
        let data = write_ccinf_bytes(&entries)?;
        Self::from_bytes(path, data)
    }

    /// Parse CCINF bytes while preserving their logical path.
    pub fn from_bytes(path: PathBuf, data: Vec<u8>) -> CcinfResult<Self> {
        if data.len() < CCINF_PREFIX_LEN {
            return Err(CcinfError::TooSmall { path });
        }
        if !has_ccinf_header(&data) {
            return Err(CcinfError::InvalidHeader { path });
        }

        let unpacked_size = read_u32_at(&data, 0x10) as usize;
        let stored_size = read_u32_at(&data, 0x14) as usize;
        let compression_flag = data[0x18];
        if compression_flag != 0 {
            return Err(CcinfError::UnsupportedCompression {
                path,
                flag: compression_flag,
            });
        }

        let body_size = data.len() - CCINF_PREFIX_LEN;
        if stored_size != body_size {
            return Err(CcinfError::SizeMismatch {
                path,
                field: "stored_size",
                declared: stored_size,
                actual: body_size,
            });
        }
        if unpacked_size != body_size {
            return Err(CcinfError::SizeMismatch {
                path,
                field: "unpacked_size",
                declared: unpacked_size,
                actual: body_size,
            });
        }

        let mut reader = ByteReader::new_at(&data, CCINF_PREFIX_LEN);
        let count = read_i32(&mut reader, &path, "entry_count")?;
        if count < 0 {
            return Err(CcinfError::InvalidEntryCount { path, count });
        }
        let count = count as usize;
        let minimum_records_size =
            count
                .checked_mul(16 + CCINF_CELL_LIST_COUNT)
                .ok_or_else(|| CcinfError::InvalidEntryCount {
                    path: path.clone(),
                    count: i32::MAX,
                })?;
        if minimum_records_size > reader.remaining() {
            return Err(CcinfError::TruncatedFile {
                path,
                field: "entry_records",
                offset: reader.offset(),
                needed: minimum_records_size,
                actual: reader.remaining(),
            });
        }

        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let entry_id = read_i32(&mut reader, &path, "entry.id")?;
            let base_resource_key = read_i32(&mut reader, &path, "entry.base_resource_key")?;
            let remap_table_file_id = read_i32(&mut reader, &path, "entry.remap_table_file_id")?;
            let animation_file_id = read_i32(&mut reader, &path, "entry.animation_file_id")?;
            let mut cell_lists = std::array::from_fn(|_| Vec::new());

            for (list_index, cells) in cell_lists.iter_mut().enumerate() {
                let cell_count = read_u8(&mut reader, &path, "cell_list.count")? as usize;
                cells.reserve(cell_count);
                for _ in 0..cell_count {
                    cells.push(CcinfCell {
                        selector: read_u16(&mut reader, &path, "cell.selector")?,
                        texture_resource_key: read_i32(
                            &mut reader,
                            &path,
                            "cell.texture_resource_key",
                        )?,
                    });
                }
                validate_cell_order(entry_id, list_index, cells)?;
            }

            entries.push(CcinfEntry {
                entry_id,
                base_resource_key,
                remap_table_file_id,
                animation_file_id,
                cell_lists,
            });
        }

        if reader.remaining() != 0 {
            return Err(CcinfError::TrailingBytes {
                path,
                count: reader.remaining(),
            });
        }
        validate_entry_order(&entries)?;

        Ok(Self {
            path,
            data,
            unpacked_size,
            stored_size,
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

    pub fn header(&self) -> [u8; 16] {
        CCINF_HEADER
    }

    pub fn unpacked_size(&self) -> usize {
        self.unpacked_size
    }

    pub fn stored_size(&self) -> usize {
        self.stored_size
    }

    pub fn compression_flag(&self) -> u8 {
        0
    }

    pub fn entries(&self) -> &[CcinfEntry] {
        &self.entries
    }

    /// Find an entry using the client's unsigned-ID ordering.
    pub fn find_entry(&self, entry_id: i32) -> Option<&CcinfEntry> {
        self.entries
            .binary_search_by_key(&(entry_id as u32), |entry| entry.entry_id as u32)
            .ok()
            .and_then(|index| self.entries.get(index))
    }

    /// Return bytes rebuilt from entries.
    pub fn to_bytes(&self) -> CcinfResult<Vec<u8>> {
        write_ccinf_bytes(&self.entries)
    }

    /// Write the current file bytes to an explicit path.
    pub fn write_to(&self, path: impl AsRef<Path>) -> CcinfResult<()> {
        let path = path.as_ref();
        fs::write(path, self.to_bytes()?).map_err(|source| CcinfError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Save the current file back to the path it was opened with.
    pub fn save(&self) -> CcinfResult<()> {
        self.write_to(&self.path)
    }
}

/// Rebuild raw CCINF bytes from typed entries.
pub fn write_ccinf_bytes(entries: &[CcinfEntry]) -> CcinfResult<Vec<u8>> {
    validate_entry_order(entries)?;
    let count = i32::try_from(entries.len()).map_err(|_| CcinfError::TooManyEntries {
        count: entries.len(),
    })?;

    let mut body = Vec::new();
    body.extend_from_slice(&count.to_le_bytes());
    for entry in entries {
        body.extend_from_slice(&entry.entry_id.to_le_bytes());
        body.extend_from_slice(&entry.base_resource_key.to_le_bytes());
        body.extend_from_slice(&entry.remap_table_file_id.to_le_bytes());
        body.extend_from_slice(&entry.animation_file_id.to_le_bytes());

        for (list_index, cells) in entry.cell_lists.iter().enumerate() {
            validate_cell_order(entry.entry_id, list_index, cells)?;
            let cell_count = u8::try_from(cells.len()).map_err(|_| CcinfError::TooManyCells {
                entry_id: entry.entry_id,
                list_index: list_index + 1,
                count: cells.len(),
            })?;
            body.push(cell_count);
            for cell in cells {
                body.extend_from_slice(&cell.selector.to_le_bytes());
                body.extend_from_slice(&cell.texture_resource_key.to_le_bytes());
            }
        }
    }

    let body_size =
        u32::try_from(body.len()).map_err(|_| CcinfError::BodyTooLarge { size: body.len() })?;
    let mut out = Vec::with_capacity(CCINF_PREFIX_LEN + body.len());
    out.extend_from_slice(&CCINF_HEADER);
    out.extend_from_slice(&body_size.to_le_bytes());
    out.extend_from_slice(&body_size.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&body);
    Ok(out)
}

fn validate_entry_order(entries: &[CcinfEntry]) -> CcinfResult<()> {
    for (index, pair) in entries.windows(2).enumerate() {
        if (pair[0].entry_id as u32) > (pair[1].entry_id as u32) {
            return Err(CcinfError::UnsortedEntries {
                index: index + 1,
                previous: pair[0].entry_id,
                current: pair[1].entry_id,
            });
        }
    }
    Ok(())
}

fn validate_cell_order(entry_id: i32, list_index: usize, cells: &[CcinfCell]) -> CcinfResult<()> {
    for (index, pair) in cells.windows(2).enumerate() {
        if pair[0].selector > pair[1].selector {
            return Err(CcinfError::UnsortedCellList {
                entry_id,
                list_index: list_index + 1,
                index: index + 1,
                previous: pair[0].selector,
                current: pair[1].selector,
            });
        }
    }
    Ok(())
}

fn read_i32(reader: &mut ByteReader<'_>, path: &Path, field: &'static str) -> CcinfResult<i32> {
    reader
        .read_i32_le(field)
        .map_err(|error| truncated_error(path, error))
}

fn read_u16(reader: &mut ByteReader<'_>, path: &Path, field: &'static str) -> CcinfResult<u16> {
    reader
        .read_u16_le(field)
        .map_err(|error| truncated_error(path, error))
}

fn read_u8(reader: &mut ByteReader<'_>, path: &Path, field: &'static str) -> CcinfResult<u8> {
    reader
        .read_u8(field)
        .map_err(|error| truncated_error(path, error))
}

fn truncated_error(path: &Path, error: ByteReadError) -> CcinfError {
    CcinfError::TruncatedFile {
        path: path.to_path_buf(),
        field: error.field,
        offset: error.offset,
        needed: error.needed,
        actual: error.actual,
    }
}

fn read_u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("offset checked"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_lists() -> [Vec<CcinfCell>; CCINF_CELL_LIST_COUNT] {
        std::array::from_fn(|_| Vec::new())
    }

    fn entry(entry_id: i32) -> CcinfEntry {
        CcinfEntry {
            entry_id,
            base_resource_key: entry_id + 10,
            remap_table_file_id: entry_id + 20,
            animation_file_id: entry_id + 30,
            cell_lists: empty_lists(),
        }
    }

    fn wrap_body(body: &[u8]) -> Vec<u8> {
        let size = body.len() as u32;
        let mut out = CCINF_HEADER.to_vec();
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.push(0);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn detects_only_the_canonical_header() {
        assert!(has_ccinf_header(&CCINF_HEADER));
        assert!(has_ccinf_header(&write_ccinf_bytes(&[]).unwrap()));
        assert!(!has_ccinf_header(b"CCINF V1.20"));
        assert!(!has_ccinf_header(b"not ccinf"));

        assert!(matches!(
            Ccinf::from_memory(CCINF_HEADER.to_vec()),
            Err(CcinfError::TooSmall { .. })
        ));
    }

    #[test]
    fn parses_and_rebuilds_all_seven_cell_lists() {
        let mut first = entry(7);
        for (list_index, cells) in first.cell_lists.iter_mut().enumerate() {
            cells.push(CcinfCell {
                selector: list_index as u16,
                texture_resource_key: 1000 + list_index as i32,
            });
        }
        let entries = vec![first, entry(-1)];
        let bytes = write_ccinf_bytes(&entries).unwrap();
        let ccinf = Ccinf::from_memory(bytes.clone()).unwrap();

        assert_eq!(ccinf.header(), CCINF_HEADER);
        assert_eq!(ccinf.unpacked_size(), bytes.len() - CCINF_PREFIX_LEN);
        assert_eq!(ccinf.unpacked_size(), ccinf.stored_size());
        assert_eq!(ccinf.compression_flag(), 0);
        assert_eq!(ccinf.entries(), entries);
        assert_eq!(ccinf.find_entry(-1), Some(&entries[1]));
        assert_eq!(ccinf.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn writes_dynamic_raw_wrapper_fields() {
        let bytes = write_ccinf_bytes(&[entry(1)]).unwrap();
        let body_size = (bytes.len() - CCINF_PREFIX_LEN) as u32;

        assert_eq!(&bytes[..16], &CCINF_HEADER);
        assert_eq!(read_u32_at(&bytes, 0x10), body_size);
        assert_eq!(read_u32_at(&bytes, 0x14), body_size);
        assert_eq!(bytes[0x18], 0);
    }

    #[test]
    fn rejects_invalid_header_sizes_and_compression() {
        let bytes = write_ccinf_bytes(&[]).unwrap();

        let mut bad_header = bytes.clone();
        bad_header[0] = b'X';
        assert!(matches!(
            Ccinf::from_memory(bad_header),
            Err(CcinfError::InvalidHeader { .. })
        ));

        let mut bad_size = bytes.clone();
        bad_size[0x14..0x18].copy_from_slice(&99_u32.to_le_bytes());
        assert!(matches!(
            Ccinf::from_memory(bad_size),
            Err(CcinfError::SizeMismatch {
                field: "stored_size",
                ..
            })
        ));

        let mut compressed = bytes;
        compressed[0x18] = 1;
        assert!(matches!(
            Ccinf::from_memory(compressed),
            Err(CcinfError::UnsupportedCompression { flag: 1, .. })
        ));
    }

    #[test]
    fn rejects_negative_count_and_truncated_records() {
        let negative = wrap_body(&(-1_i32).to_le_bytes());
        assert!(matches!(
            Ccinf::from_memory(negative),
            Err(CcinfError::InvalidEntryCount { count: -1, .. })
        ));

        let mut truncated_body = Vec::new();
        truncated_body.extend_from_slice(&1_i32.to_le_bytes());
        truncated_body.extend_from_slice(&[0; 10]);
        let truncated = wrap_body(&truncated_body);
        assert!(matches!(
            Ccinf::from_memory(truncated),
            Err(CcinfError::TruncatedFile { .. })
        ));
    }

    #[test]
    fn rejects_trailing_bytes_after_entries() {
        let mut body = 0_i32.to_le_bytes().to_vec();
        body.push(0xaa);
        let bytes = wrap_body(&body);
        assert!(matches!(
            Ccinf::from_memory(bytes),
            Err(CcinfError::TrailingBytes { count: 1, .. })
        ));
    }

    #[test]
    fn rejects_unsorted_entries_and_cells() {
        assert!(matches!(
            write_ccinf_bytes(&[entry(-1), entry(7)]),
            Err(CcinfError::UnsortedEntries { .. })
        ));

        let mut unsorted_cells = entry(7);
        unsorted_cells.cell_lists[0] = vec![
            CcinfCell {
                selector: 2,
                texture_resource_key: 20,
            },
            CcinfCell {
                selector: 1,
                texture_resource_key: 10,
            },
        ];
        assert!(matches!(
            write_ccinf_bytes(&[unsorted_cells]),
            Err(CcinfError::UnsortedCellList { .. })
        ));
    }

    #[test]
    fn rejects_cell_lists_larger_than_u8_count() {
        let mut oversized = entry(1);
        oversized.cell_lists[0] = vec![
            CcinfCell {
                selector: 1,
                texture_resource_key: 2,
            };
            256
        ];

        assert!(matches!(
            write_ccinf_bytes(&[oversized]),
            Err(CcinfError::TooManyCells { count: 256, .. })
        ));
    }
}
