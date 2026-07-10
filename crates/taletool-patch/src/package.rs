use std::io::Read;

use anyhow::{Context, Result, bail};
use encoding_rs::EUC_KR;
use flate2::read::ZlibDecoder;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{checksum::sha1_hex, paths::normalize_client_path};

const MAGIC: &[u8] = b"PCHPKG DATA\x1a";
const DATETIME_CODE_OFFSET: usize = 12;
const COUNT_OFFSET: usize = 16;
const LOOKUP_FLAG_OFFSET: usize = 20;
const TABLE_OFFSET: usize = 21;
const TABLE_RECORD_LEN: usize = 8;
const SEGMENT_HEADER_LEN: usize = 13;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchPackageDateTimeCode {
    pub raw: u32,
    pub raw_hex: String,
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub time_raw: u16,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl PchPackageDateTimeCode {
    pub fn from_raw(raw: u32) -> Self {
        let time_raw = (raw & 0xffff) as u16;
        Self {
            raw,
            raw_hex: format!("{raw:08x}"),
            year: ((raw >> 25) as u16) + 2000,
            month: ((raw & 0x01e0_0000) >> 21) as u8,
            day: ((raw & 0x001f_0000) >> 16) as u8,
            time_raw,
            hour: ((time_raw & 0xf800) >> 11) as u8,
            minute: ((time_raw & 0x07e0) >> 5) as u8,
            second: ((time_raw & 0x001f) * 2) as u8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchPkgHeader {
    pub package_count: u32,
    pub body_offset: usize,
    pub package_datetime: PchPackageDateTimeCode,
    pub segment_lookup_flag: u8,
    pub direct_segment_lookup: bool,
    pub segment_table_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchSegmentHeader {
    pub segment_index: usize,
    pub segment_id: u32,
    pub segment_offset: usize,
    pub body_offset: usize,
    pub segment_datetime: PchPackageDateTimeCode,
    pub decoded_size: usize,
    pub encoded_size: usize,
    pub compressed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PchOperationKind {
    DeleteFile,
    ReplaceFile,
    BinaryDelta,
    ReplaceAndRelaunch,
    PatchInPlace,
    PackedArchiveMutation,
    ReplaceAndRun,
    Unknown,
}

impl PchOperationKind {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::DeleteFile,
            1 => Self::ReplaceFile,
            2 => Self::BinaryDelta,
            3 => Self::ReplaceAndRelaunch,
            4 => Self::PatchInPlace,
            5 => Self::PackedArchiveMutation,
            6 => Self::ReplaceAndRun,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeleteFile => "delete_file",
            Self::ReplaceFile => "replace_file",
            Self::BinaryDelta => "binary_delta",
            Self::ReplaceAndRelaunch => "replace_and_relaunch",
            Self::PatchInPlace => "patch_in_place",
            Self::PackedArchiveMutation => "packed_archive_mutation",
            Self::ReplaceAndRun => "replace_and_run",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_direct_apply(self, _target_path: &str, _payload: &[u8]) -> bool {
        match self {
            Self::ReplaceFile | Self::ReplaceAndRelaunch => true,
            Self::DeleteFile
            | Self::BinaryDelta
            | Self::PatchInPlace
            | Self::PackedArchiveMutation
            | Self::ReplaceAndRun
            | Self::Unknown => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PchOperation {
    pub segment: PchSegmentHeader,
    pub op_code: u8,
    pub op_kind: PchOperationKind,
    pub raw_target_path: String,
    pub target_path: String,
    pub payload: Vec<u8>,
    pub payload_sha1: String,
}

#[derive(Debug, Clone)]
pub struct ParsedPchPkg {
    pub header: PchPkgHeader,
    pub operations: Vec<PchOperation>,
}

impl ParsedPchPkg {
    pub fn header_json(&self, operation: &PchOperation) -> serde_json::Value {
        json!({
            "package_count": self.header.package_count,
            "body_offset": self.header.body_offset,
            "package_datetime": self.header.package_datetime,
            "segment_lookup_flag": self.header.segment_lookup_flag,
            "direct_segment_lookup": self.header.direct_segment_lookup,
            "segment_table_hex": self.header.segment_table_hex,
            "operation_count": self.operations.len(),
            "segment": operation.segment,
            "raw_target_path": operation.raw_target_path,
        })
    }
}

pub fn parse_pch_pkg(bytes: &[u8]) -> Result<ParsedPchPkg> {
    if bytes.len() < TABLE_OFFSET {
        bail!("package is too short");
    }

    if !bytes.starts_with(MAGIC) {
        bail!("package does not start with PCHPKG DATA magic");
    }

    let package_count = read_u32_at(bytes, COUNT_OFFSET, "package segment count")?;
    if package_count == 0 {
        bail!("package has no segments");
    }

    let segment_count = package_count as usize;
    let table_end = TABLE_OFFSET
        .checked_add(
            segment_count
                .checked_mul(TABLE_RECORD_LEN)
                .context("package segment table size overflow")?,
        )
        .context("package segment table end overflow")?;
    if table_end > bytes.len() {
        bail!(
            "package segment table ends at {table_end}, beyond file size {}",
            bytes.len()
        );
    }

    let package_datetime_raw = read_u32_at(bytes, DATETIME_CODE_OFFSET, "package datetime code")?;
    let first_segment_offset =
        read_u32_at(bytes, TABLE_OFFSET + 4, "first segment offset")? as usize;
    let body_offset = first_segment_offset
        .checked_add(SEGMENT_HEADER_LEN)
        .context("first segment body offset overflow")?;
    if body_offset > bytes.len() {
        bail!("first segment body offset {body_offset} is outside package");
    }

    let mut segment_table = Vec::with_capacity(segment_count);
    for segment_index in 0..segment_count {
        let table_offset = TABLE_OFFSET + segment_index * TABLE_RECORD_LEN;
        let segment_id = read_u32_at(bytes, table_offset, "segment id")?;
        let segment_offset = read_u32_at(bytes, table_offset + 4, "segment offset")? as usize;
        segment_table.push(SegmentTableRecord {
            segment_id,
            segment_offset,
        });
    }

    let direct_segment_lookup = bytes[LOOKUP_FLAG_OFFSET] != 0;
    let segment_order = resolve_segment_order(&segment_table, direct_segment_lookup)?;

    let mut operations = Vec::with_capacity(segment_count);
    for (segment_index, record) in segment_order.into_iter().enumerate() {
        let segment = parse_segment_header(
            bytes,
            segment_index,
            record.segment_id,
            record.segment_offset,
        )
        .with_context(|| format!("parsing package segment {segment_index}"))?;
        let body = decode_segment_body(bytes, &segment)
            .with_context(|| format!("decoding package segment {segment_index}"))?;
        let operation = parse_operation_body(body, segment)
            .with_context(|| format!("parsing package segment {segment_index} operation"))?;
        operations.push(operation);
    }

    Ok(ParsedPchPkg {
        header: PchPkgHeader {
            package_count,
            body_offset,
            package_datetime: PchPackageDateTimeCode::from_raw(package_datetime_raw),
            segment_lookup_flag: bytes[LOOKUP_FLAG_OFFSET],
            direct_segment_lookup,
            segment_table_hex: hex::encode(&bytes[TABLE_OFFSET..table_end]),
        },
        operations,
    })
}

#[derive(Debug, Clone, Copy)]
struct SegmentTableRecord {
    segment_id: u32,
    segment_offset: usize,
}

fn resolve_segment_order(
    segment_table: &[SegmentTableRecord],
    direct_segment_lookup: bool,
) -> Result<Vec<SegmentTableRecord>> {
    if direct_segment_lookup {
        return Ok(segment_table.to_vec());
    }

    let mut ordered = Vec::with_capacity(segment_table.len());
    for requested_id in 0..segment_table.len() {
        let requested_id =
            u32::try_from(requested_id).context("package segment id does not fit in u32")?;
        let index = segment_table
            .binary_search_by_key(&requested_id, |record| record.segment_id)
            .map_err(|_| anyhow::anyhow!("package segment table is missing id {requested_id}"))?;
        ordered.push(segment_table[index]);
    }
    Ok(ordered)
}

fn parse_segment_header(
    bytes: &[u8],
    segment_index: usize,
    segment_id: u32,
    segment_offset: usize,
) -> Result<PchSegmentHeader> {
    let header_end = segment_offset
        .checked_add(SEGMENT_HEADER_LEN)
        .context("segment header offset overflow")?;
    let header = bytes
        .get(segment_offset..header_end)
        .with_context(|| format!("segment header at offset {segment_offset} is truncated"))?;
    let segment_datetime_raw = read_u32_at(header, 0, "segment datetime code")?;

    let decoded_size = read_u32_at(header, 4, "segment decoded size")? as usize;
    let encoded_size = read_u32_at(header, 8, "segment encoded size")? as usize;
    let compressed = match header[12] {
        0 => false,
        1 => true,
        other => bail!("segment has unknown compression flag {other}"),
    };
    let body_offset = header_end;
    let body_end = body_offset
        .checked_add(encoded_size)
        .context("segment body offset overflow")?;
    if body_end > bytes.len() {
        bail!(
            "segment body ends at {body_end}, beyond file size {}",
            bytes.len()
        );
    }
    if !compressed && decoded_size != encoded_size {
        bail!("raw segment size mismatch: decoded={decoded_size}, encoded={encoded_size}");
    }

    Ok(PchSegmentHeader {
        segment_index,
        segment_id,
        segment_offset,
        body_offset,
        segment_datetime: PchPackageDateTimeCode::from_raw(segment_datetime_raw),
        decoded_size,
        encoded_size,
        compressed,
    })
}

fn decode_segment_body(bytes: &[u8], segment: &PchSegmentHeader) -> Result<Vec<u8>> {
    let body_end = segment
        .body_offset
        .checked_add(segment.encoded_size)
        .context("segment body end overflow")?;
    let encoded = bytes
        .get(segment.body_offset..body_end)
        .context("segment body is outside package")?;

    let body = if segment.compressed {
        let mut decoder = ZlibDecoder::new(encoded);
        let mut body = Vec::with_capacity(segment.decoded_size);
        decoder
            .read_to_end(&mut body)
            .context("decompressing PCHPKG segment body")?;
        body
    } else {
        encoded.to_vec()
    };

    if body.len() != segment.decoded_size {
        bail!(
            "segment decoded size mismatch: header={}, actual={}",
            segment.decoded_size,
            body.len()
        );
    }
    Ok(body)
}

fn parse_operation_body(body: Vec<u8>, segment: PchSegmentHeader) -> Result<PchOperation> {
    if body.len() < 2 {
        bail!("package operation body is too short");
    }

    let op_code = body[0];
    let path_len = body[1] as usize;
    let path_end = 2usize
        .checked_add(path_len)
        .context("target path length overflow")?;
    if body.len() < path_end {
        bail!("package operation body ended before target path");
    }

    let raw_target_path = decode_target_path(&body[2..path_end])?;
    let target_path = normalize_client_path(&raw_target_path)?;
    let payload = body[path_end..].to_vec();
    let payload_sha1 = sha1_hex(&payload);

    Ok(PchOperation {
        segment,
        op_code,
        op_kind: PchOperationKind::from_code(op_code),
        raw_target_path,
        target_path,
        payload,
        payload_sha1,
    })
}

fn decode_target_path(bytes: &[u8]) -> Result<String> {
    let (path, _, had_errors) = EUC_KR.decode(bytes);
    if had_errors {
        bail!("target path is not EUC-KR/Windows-949");
    }
    Ok(path.into_owned())
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

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    type PackageFixtureSegment<'a> = (u32, &'a [u8], Option<&'a [u8]>);

    fn fixture(op: u8, path: &str, payload: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.push(op);
        body.push(path.len() as u8);
        body.extend_from_slice(path.as_bytes());
        body.extend_from_slice(payload);

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&body).unwrap();
        let compressed = encoder.finish().unwrap();

        package_fixture(&[(0, &body, Some(&compressed))])
    }

    fn package_fixture(segments: &[PackageFixtureSegment<'_>]) -> Vec<u8> {
        package_fixture_with_lookup_flag(segments, 1)
    }

    fn package_fixture_with_lookup_flag(
        segments: &[PackageFixtureSegment<'_>],
        lookup_flag: u8,
    ) -> Vec<u8> {
        let package_datetime_code = ((24_u32) << 25) | (1 << 21) | (2 << 16);
        let package_count = segments.len() as u32;
        let table_end = TABLE_OFFSET + segments.len() * TABLE_RECORD_LEN;
        let mut segment_offset = table_end;
        let mut offsets = Vec::with_capacity(segments.len());
        for (_, body, compressed) in segments {
            offsets.push(segment_offset);
            segment_offset += SEGMENT_HEADER_LEN + compressed.unwrap_or(body).len();
        }

        let mut out = Vec::with_capacity(segment_offset);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&package_datetime_code.to_le_bytes());
        out.extend_from_slice(&package_count.to_le_bytes());
        out.push(lookup_flag);
        for ((segment_id, _, _), offset) in segments.iter().zip(&offsets) {
            out.extend_from_slice(&segment_id.to_le_bytes());
            out.extend_from_slice(&(*offset as u32).to_le_bytes());
        }
        for ((_, body, compressed), _) in segments.iter().zip(&offsets) {
            let encoded = compressed.unwrap_or(body);
            out.extend_from_slice(&package_datetime_code.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
            out.push(u8::from(compressed.is_some()));
            out.extend_from_slice(encoded);
        }
        out
    }

    #[test]
    fn decodes_package_datetime_code_as_packed_datetime() {
        let raw = (24_u32 << 25) | (1 << 21) | (2 << 16) | (3 << 11) | (4 << 5) | 3;
        let date = PchPackageDateTimeCode::from_raw(raw);

        assert_eq!(date.year, 2024);
        assert_eq!(date.month, 1);
        assert_eq!(date.day, 2);
        assert_eq!(date.hour, 3);
        assert_eq!(date.minute, 4);
        assert_eq!(date.second, 6);
    }

    #[test]
    fn parses_synthetic_package() {
        let bytes = fixture(1, r"$(INSTALLED)\Nostale.exe", b"MZpayload");
        let parsed = parse_pch_pkg(&bytes).unwrap();
        assert_eq!(parsed.operations.len(), 1);
        assert_eq!(parsed.header.package_datetime.year, 2024);
        assert_eq!(parsed.header.package_datetime.month, 1);
        assert_eq!(parsed.header.package_datetime.day, 2);
        assert_eq!(parsed.header.package_datetime.hour, 0);
        assert_eq!(parsed.header.package_datetime.minute, 0);
        assert_eq!(parsed.header.package_datetime.second, 0);
        assert!(parsed.header.direct_segment_lookup);
        let operation = &parsed.operations[0];
        assert_eq!(operation.target_path, "Nostale.exe");
        assert_eq!(operation.payload, b"MZpayload");
        assert_eq!(operation.op_kind, PchOperationKind::ReplaceFile);
    }

    #[test]
    fn parses_segment_datetime_independently_from_package_datetime() {
        let mut bytes = fixture(1, r"$(INSTALLED)\Nostale.exe", b"MZpayload");
        let segment_offset = read_u32_at(&bytes, TABLE_OFFSET + 4, "first segment offset").unwrap();
        let segment_datetime_code =
            (24_u32 << 25) | (1 << 21) | (3 << 16) | (4 << 11) | (5 << 5) | 6;
        bytes[segment_offset as usize..segment_offset as usize + 4]
            .copy_from_slice(&segment_datetime_code.to_le_bytes());

        let parsed = parse_pch_pkg(&bytes).unwrap();
        let segment_datetime = &parsed.operations[0].segment.segment_datetime;

        assert_eq!(parsed.header.package_datetime.day, 2);
        assert_eq!(segment_datetime.day, 3);
        assert_eq!(segment_datetime.hour, 4);
        assert_eq!(segment_datetime.minute, 5);
        assert_eq!(segment_datetime.second, 12);
    }

    #[test]
    fn decodes_euc_kr_target_paths() {
        let path_bytes = hex::decode(
            "2428494e5354414c4c4544295c4e6f7374616c65446174615c776176655c\
             b3ebbdbac5d7c0cf284163743120bab8bdba292e3330303930",
        )
        .unwrap();
        let raw_path = decode_target_path(&path_bytes).unwrap();

        assert_eq!(
            raw_path,
            "$(INSTALLED)\\NostaleData\\wave\\\u{b178}\u{c2a4}\u{d14c}\u{c77c}(Act1 \u{bcf4}\u{c2a4}).30090"
        );
    }

    #[test]
    fn rejects_target_paths_with_unknown_encoding() {
        let error = decode_target_path(&[0x80]).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("target path is not EUC-KR/Windows-949")
        );
    }

    #[test]
    fn classifies_opcode_three_as_replace_and_relaunch() {
        let bytes = fixture(3, r"$(INSTALLED)\Nostale.exe", b"MZpayload");
        let parsed = parse_pch_pkg(&bytes).unwrap();
        let operation = &parsed.operations[0];
        assert_eq!(operation.op_kind, PchOperationKind::ReplaceAndRelaunch);
        assert!(
            operation
                .op_kind
                .is_direct_apply(&operation.target_path, &operation.payload)
        );
    }

    #[test]
    fn classifies_opcode_two_as_binary_delta() {
        let bytes = fixture(2, r"$(INSTALLED)\Nostale.dat", b"delta");
        let parsed = parse_pch_pkg(&bytes).unwrap();
        let operation = &parsed.operations[0];
        assert_eq!(operation.op_kind, PchOperationKind::BinaryDelta);
        assert!(
            !operation
                .op_kind
                .is_direct_apply(&operation.target_path, &operation.payload)
        );
    }

    #[test]
    fn classifies_opcode_six_as_replace_and_run() {
        let bytes = fixture(
            6,
            r"$(INSTALLED)\NostaleData\ExtractUIEff.dat",
            b"MZpayload",
        );
        let parsed = parse_pch_pkg(&bytes).unwrap();
        let operation = &parsed.operations[0];
        assert_eq!(operation.op_kind, PchOperationKind::ReplaceAndRun);
        assert_eq!(operation.op_kind.as_str(), "replace_and_run");
        assert!(
            !operation
                .op_kind
                .is_direct_apply(&operation.target_path, &operation.payload)
        );
    }

    #[test]
    fn parses_synthetic_multi_segment_package() {
        let mut first = Vec::new();
        first.push(0);
        first.push(24);
        first.extend_from_slice(br"$(INSTALLED)\Nostale.dat");

        let mut second_body = Vec::new();
        second_body.push(1);
        second_body.push(24);
        second_body.extend_from_slice(br"$(INSTALLED)\Nostale.exe");
        second_body.extend_from_slice(b"MZpayload");
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&second_body).unwrap();
        let second = encoder.finish().unwrap();

        let bytes = package_fixture(&[(0, &first, None), (1, &second_body, Some(&second))]);

        let parsed = parse_pch_pkg(&bytes).unwrap();
        assert_eq!(parsed.operations.len(), 2);
        assert_eq!(parsed.operations[0].op_kind, PchOperationKind::DeleteFile);
        assert_eq!(parsed.operations[0].target_path, "Nostale.dat");
        assert_eq!(parsed.operations[1].op_kind, PchOperationKind::ReplaceFile);
        assert_eq!(parsed.operations[1].target_path, "Nostale.exe");
        assert_eq!(parsed.operations[1].payload, b"MZpayload");
    }

    #[test]
    fn zero_lookup_flag_resolves_sorted_segments_by_id() {
        let mut first = Vec::new();
        first.push(0);
        first.push(24);
        first.extend_from_slice(br"$(INSTALLED)\Nostale.dat");

        let mut second = Vec::new();
        second.push(1);
        second.push(24);
        second.extend_from_slice(br"$(INSTALLED)\Nostale.exe");
        second.extend_from_slice(b"MZpayload");

        let bytes = package_fixture_with_lookup_flag(&[(0, &first, None), (1, &second, None)], 0);

        let parsed = parse_pch_pkg(&bytes).unwrap();
        assert!(!parsed.header.direct_segment_lookup);
        assert_eq!(parsed.operations.len(), 2);
        assert_eq!(parsed.operations[0].segment.segment_id, 0);
        assert_eq!(parsed.operations[0].op_kind, PchOperationKind::DeleteFile);
        assert_eq!(parsed.operations[1].segment.segment_id, 1);
        assert_eq!(parsed.operations[1].op_kind, PchOperationKind::ReplaceFile);
    }

    #[test]
    fn zero_lookup_flag_rejects_unsorted_segment_table() {
        let mut first = Vec::new();
        first.push(0);
        first.push(24);
        first.extend_from_slice(br"$(INSTALLED)\Nostale.dat");

        let mut second = Vec::new();
        second.push(1);
        second.push(24);
        second.extend_from_slice(br"$(INSTALLED)\Nostale.exe");
        second.extend_from_slice(b"MZpayload");

        let bytes = package_fixture_with_lookup_flag(&[(1, &second, None), (0, &first, None)], 0);
        let error = parse_pch_pkg(&bytes).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("package segment table is missing id")
        );
    }
}
