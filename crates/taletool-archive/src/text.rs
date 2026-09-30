//! Text `.NOS` record archive support.
//!
//! Text archives store named records and payload bytes. Payload decoding is
//! delegated to `taletool-text`; this module only owns the archive envelope,
//! record table, optional timestamp trailer, and byte-for-byte rebuild support.
//! These files can also use the `.NOS` extension, so their public names are
//! separate from [`crate::binary`] archive types.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use taletool_text::{TextPayloadKind, decode_dat_payload, decode_list_payload};
use thiserror::Error;

const TEXT_ARCHIVE_TRAILER_MARKER: [u8; 4] = [0xee, 0x3e, 0x32, 0x01];

#[derive(Debug, Error)]
pub enum TextNosArchiveError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("text archive has invalid record count {count}")]
    InvalidRecordCount { count: i32 },
    #[error("text archive field {field} has invalid negative length {value}")]
    InvalidLength { field: &'static str, value: i32 },
    #[error(
        "text archive field {field} is truncated at offset {offset}: need {needed} bytes, got {actual}"
    )]
    TruncatedArchive {
        field: &'static str,
        offset: usize,
        needed: usize,
        actual: usize,
    },
    #[error("text archive has too many records: {count}")]
    TooManyRecords { count: usize },
    #[error("text record {name} field {field} is too large: {size} bytes")]
    RecordTooLarge {
        name: String,
        field: &'static str,
        size: usize,
    },
}

pub type TextNosArchiveResult<T> = std::result::Result<T, TextNosArchiveError>;

/// Timestamp trailer decoded from a text archive, when present.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TextNosArchiveTimestamp {
    pub variant: f64,
    pub unix_seconds: i64,
}

/// One parsed named record from a text `.NOS` archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextNosRecord {
    pub id: i32,
    pub name: String,
    pub name_bytes: Vec<u8>,
    pub packed_flag: i32,
    pub payload: Vec<u8>,
}

/// Record input used to rebuild a text `.NOS` archive.
#[derive(Debug, Clone)]
pub struct TextNosRecordInput {
    pub name: String,
    pub name_bytes: Vec<u8>,
    pub packed_flag: i32,
    pub payload: Vec<u8>,
}

impl TextNosRecord {
    pub fn is_packed(&self) -> bool {
        self.packed_flag != 0
    }

    pub fn payload_kind(&self) -> TextPayloadKind {
        let name = self.name.to_ascii_lowercase();
        if self.is_packed() || name.ends_with(".dat") {
            TextPayloadKind::Dat
        } else if name.ends_with(".lst") {
            TextPayloadKind::List
        } else {
            TextPayloadKind::Raw
        }
    }

    pub fn decoded_payload(&self) -> taletool_text::Result<Vec<u8>> {
        match self.payload_kind() {
            TextPayloadKind::Dat => decode_dat_payload(&self.payload),
            TextPayloadKind::List => decode_list_payload(&self.payload),
            TextPayloadKind::Raw => Ok(self.payload.clone()),
        }
    }
}

/// Parsed named-record text `.NOS` archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextNosArchive {
    path: PathBuf,
    records: Vec<TextNosRecord>,
    timestamp: Option<TextNosArchiveTimestamp>,
    trailing_bytes: usize,
    trailer: Vec<u8>,
}

impl TextNosArchive {
    /// Read and parse a text `.NOS` archive from disk.
    pub fn open(path: impl AsRef<Path>) -> TextNosArchiveResult<Self> {
        let path = path.as_ref().to_path_buf();
        let data = fs::read(&path).map_err(|source| TextNosArchiveError::Io {
            path: path.clone(),
            source,
        })?;
        Self::from_bytes(path, data)
    }

    /// Parse a text `.NOS` archive from bytes while preserving its logical path.
    pub fn from_bytes(path: PathBuf, data: Vec<u8>) -> TextNosArchiveResult<Self> {
        let mut reader = ByteReader::new(&data);
        let count = read_text_i32(&mut reader, "record_count")?;
        if count < 0 {
            return Err(TextNosArchiveError::InvalidRecordCount { count });
        }

        let count = count as usize;
        let minimum_size = 4usize.saturating_add(count.saturating_mul(16));
        if minimum_size > data.len() {
            return Err(TextNosArchiveError::TruncatedArchive {
                field: "record_table",
                offset: data.len(),
                needed: minimum_size,
                actual: data.len(),
            });
        }

        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let id = read_text_i32(&mut reader, "record.id")?;
            let name_len = read_text_len(&mut reader, "record.name_len")?;
            let name_bytes = read_text_bytes(&mut reader, "record.name", name_len)?.to_vec();
            let name = String::from_utf8_lossy(&name_bytes).into_owned();
            let packed_flag = read_text_i32(&mut reader, "record.packed_flag")?;
            let payload_len = read_text_len(&mut reader, "record.payload_len")?;
            let payload = read_text_bytes(&mut reader, "record.payload", payload_len)?.to_vec();

            records.push(TextNosRecord {
                id,
                name,
                name_bytes,
                packed_flag,
                payload,
            });
        }

        let trailing = &data[reader.offset()..];
        let (timestamp, trailing_bytes) =
            if trailing.len() >= 12 && trailing.ends_with(&TEXT_ARCHIVE_TRAILER_MARKER) {
                let mut variant_bytes = [0_u8; 8];
                variant_bytes.copy_from_slice(&trailing[trailing.len() - 12..trailing.len() - 4]);
                let variant = f64::from_le_bytes(variant_bytes);
                let unix_seconds = ((variant - 25569.0) * 86400.0).round() as i64;
                (
                    Some(TextNosArchiveTimestamp {
                        variant,
                        unix_seconds,
                    }),
                    trailing.len() - 12,
                )
            } else {
                (None, trailing.len())
            };

        Ok(Self {
            path,
            records,
            timestamp,
            trailing_bytes,
            trailer: trailing.to_vec(),
        })
    }

    /// Return the logical path associated with this archive.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return parsed records in stored order.
    pub fn records(&self) -> &[TextNosRecord] {
        &self.records
    }

    /// Return the decoded trailer timestamp when the archive has one.
    pub fn timestamp(&self) -> Option<TextNosArchiveTimestamp> {
        self.timestamp
    }

    /// Return all bytes after the records, including any timestamp footer.
    pub fn trailer(&self) -> &[u8] {
        &self.trailer
    }

    /// Rebuild the archive without changing record metadata or trailer bytes.
    pub fn to_bytes(&self) -> TextNosArchiveResult<Vec<u8>> {
        write_text_nos_archive_records(&self.records, &self.trailer)
    }

    /// Return the number of unrecognized bytes left after the parsed records.
    pub fn trailing_bytes(&self) -> usize {
        self.trailing_bytes
    }
}

/// Build a new text archive with sequential IDs starting at one and no trailer.
pub fn write_text_nos_archive_bytes(
    records: &[TextNosRecordInput],
) -> TextNosArchiveResult<Vec<u8>> {
    let count = record_count(records.len())?;
    write_records(
        count,
        records.iter().enumerate().map(|(index, record)| {
            (
                index as i32 + 1,
                record.name.as_str(),
                record.name_bytes.as_slice(),
                record.packed_flag,
                record.payload.as_slice(),
            )
        }),
        &[],
    )
}

/// Write records in supplied order with explicit IDs, flags, name bytes, and trailer.
/// Display names are used only in errors; stored names come from `name_bytes`.
pub fn write_text_nos_archive_records(
    records: &[TextNosRecord],
    trailer: &[u8],
) -> TextNosArchiveResult<Vec<u8>> {
    write_records(
        record_count(records.len())?,
        records.iter().map(|record| {
            (
                record.id,
                record.name.as_str(),
                record.name_bytes.as_slice(),
                record.packed_flag,
                record.payload.as_slice(),
            )
        }),
        trailer,
    )
}

fn record_count(count: usize) -> TextNosArchiveResult<i32> {
    i32::try_from(count).map_err(|_| TextNosArchiveError::TooManyRecords { count })
}

fn write_records<'a>(
    count: i32,
    records: impl Iterator<Item = (i32, &'a str, &'a [u8], i32, &'a [u8])>,
    trailer: &[u8],
) -> TextNosArchiveResult<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    for (id, name, name_bytes, packed_flag, payload) in records {
        let name_len =
            i32::try_from(name_bytes.len()).map_err(|_| TextNosArchiveError::RecordTooLarge {
                name: name.to_owned(),
                field: "name",
                size: name_bytes.len(),
            })?;
        let payload_len =
            i32::try_from(payload.len()).map_err(|_| TextNosArchiveError::RecordTooLarge {
                name: name.to_owned(),
                field: "payload",
                size: payload.len(),
            })?;
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&packed_flag.to_le_bytes());
        out.extend_from_slice(&payload_len.to_le_bytes());
        out.extend_from_slice(payload);
    }
    out.extend_from_slice(trailer);
    Ok(out)
}

fn read_text_i32(reader: &mut ByteReader<'_>, field: &'static str) -> TextNosArchiveResult<i32> {
    reader
        .read_i32_le(field)
        .map_err(text_archive_truncated_error)
}

fn read_text_len(reader: &mut ByteReader<'_>, field: &'static str) -> TextNosArchiveResult<usize> {
    let value = read_text_i32(reader, field)?;
    if value < 0 {
        return Err(TextNosArchiveError::InvalidLength { field, value });
    }
    Ok(value as usize)
}

fn read_text_bytes<'a>(
    reader: &mut ByteReader<'a>,
    field: &'static str,
    len: usize,
) -> TextNosArchiveResult<&'a [u8]> {
    reader
        .read_bytes(field, len)
        .map_err(text_archive_truncated_error)
}

fn text_archive_truncated_error(error: ByteReadError) -> TextNosArchiveError {
    TextNosArchiveError::TruncatedArchive {
        field: error.field,
        offset: error.offset,
        needed: error.needed,
        actual: error.actual,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn preserves_records_and_trailer_and_reads_timestamp_at_eof() {
        let mut bytes = 2i32.to_le_bytes().to_vec();
        push_text_record(&mut bytes, -7, "same.dat", 0, b"plain");
        push_text_record(&mut bytes, -7, "same.dat", -42, b"encoded");
        bytes.extend_from_slice(b"opaque");
        bytes.extend_from_slice(&(25569.0 + 1.0 / 86400.0_f64).to_le_bytes());
        bytes.extend_from_slice(&TEXT_ARCHIVE_TRAILER_MARKER);
        let archive = TextNosArchive::from_bytes("fixture.NOS".into(), bytes.clone()).unwrap();
        assert_eq!(archive.timestamp().unwrap().unix_seconds, 1);
        assert_eq!(archive.trailing_bytes(), 6);
        assert_eq!(archive.trailer().len(), 18);
        assert_eq!(archive.to_bytes().unwrap(), bytes);
        assert_eq!(
            write_text_nos_archive_records(archive.records(), archive.trailer()).unwrap(),
            bytes
        );
    }

    #[test]
    fn parses_text_archive_fixture_with_timestamp() {
        let mut archive = Vec::new();
        archive.extend_from_slice(&2_i32.to_le_bytes());
        push_text_record(&mut archive, 10, "foo.dat", 1, &[1, b'X' ^ 0x33, 0xff]);

        let mut list_payload = Vec::new();
        list_payload.extend_from_slice(&1_i32.to_le_bytes());
        push_list_line(&mut list_payload, b"row");
        push_text_record(&mut archive, 20, "bar.lst", 0, &list_payload);

        archive.extend_from_slice(&2.00001_f64.to_le_bytes());
        archive.extend_from_slice(&TEXT_ARCHIVE_TRAILER_MARKER);

        let parsed = TextNosArchive::from_bytes(PathBuf::from("fixture.NOS"), archive).unwrap();
        assert_eq!(parsed.records().len(), 2);
        assert_eq!(parsed.records()[0].payload_kind(), TextPayloadKind::Dat);
        assert_eq!(parsed.records()[0].decoded_payload().unwrap(), b"X\n");
        assert_eq!(parsed.records()[1].payload_kind(), TextPayloadKind::List);
        assert_eq!(parsed.records()[1].decoded_payload().unwrap(), b"row\n");
        assert!(parsed.timestamp().is_some());
        assert_eq!(parsed.trailing_bytes(), 0);
    }

    #[test]
    fn ignores_non_matching_text_archive_trailing_bytes() {
        let mut archive = Vec::new();
        archive.extend_from_slice(&0_i32.to_le_bytes());
        archive.extend_from_slice(&[0_u8; 12]);

        let parsed = TextNosArchive::from_bytes(PathBuf::from("fixture.NOS"), archive).unwrap();
        assert!(parsed.timestamp().is_none());
        assert_eq!(parsed.trailing_bytes(), 12);
    }

    #[test]
    fn writes_text_archive_with_sequential_ids() {
        let bytes = write_text_nos_archive_bytes(&[
            TextNosRecordInput {
                name: "a.dat".to_owned(),
                name_bytes: b"a.dat".to_vec(),
                packed_flag: 1,
                payload: taletool_text::encode_dat_payload(b"a\n").unwrap(),
            },
            TextNosRecordInput {
                name: "b.lst".to_owned(),
                name_bytes: b"b.lst".to_vec(),
                packed_flag: 0,
                payload: taletool_text::encode_list_payload(b"b\n").unwrap(),
            },
        ])
        .unwrap();
        let archive = TextNosArchive::from_bytes(PathBuf::from("fixture.NOS"), bytes).unwrap();
        assert_eq!(archive.records()[0].id, 1);
        assert_eq!(archive.records()[1].id, 2);
        assert_eq!(archive.records()[0].decoded_payload().unwrap(), b"a\n");
        assert_eq!(archive.records()[1].decoded_payload().unwrap(), b"b\n");
    }

    fn push_text_record(out: &mut Vec<u8>, id: i32, name: &str, packed_flag: i32, payload: &[u8]) {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(name.len() as i32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed_flag.to_le_bytes());
        out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        out.extend_from_slice(payload);
    }

    fn push_list_line(out: &mut Vec<u8>, line: &[u8]) {
        out.extend_from_slice(&(line.len() as i32).to_le_bytes());
        out.extend(line.iter().map(|byte| byte ^ 0x01));
    }
}
