use std::borrow::Cow;

use encoding_rs::{
    BIG5, EUC_KR, Encoding, SHIFT_JIS, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1254,
};
use serde::{Deserialize, Serialize};
use taletool_core::ByteReader;
use thiserror::Error;

pub mod gtd;

const COMPACT_DATA_CHARS: [u8; 16] = [
    0, b' ', b'-', b'.', b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'\n', 0,
];

#[derive(Debug, Error)]
pub enum TextError {
    #[error(
        "DAT compact payload is truncated at offset {offset}: need {needed} bytes, got {actual}"
    )]
    TruncatedDatPayload {
        offset: usize,
        needed: usize,
        actual: usize,
    },
    #[error("LST payload is too small: need {needed} bytes, got {actual}")]
    TruncatedListPayload { needed: usize, actual: usize },
    #[error("LST payload has invalid line count {count}")]
    InvalidListLineCount { count: i32 },
    #[error("LST payload line {line} has invalid negative length {value}")]
    InvalidListLineLength { line: usize, value: i32 },
    #[error("LST payload line {line} is truncated: need {needed} bytes, got {actual}")]
    TruncatedListLine {
        line: usize,
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
    #[error("text is not valid {encoding}")]
    InvalidTextEncoding { encoding: &'static str },
    #[error("text cannot be represented as {encoding}")]
    UnrepresentableText { encoding: &'static str },
    #[error("language entry {entry} key starts with '#' or contains a tab or line break")]
    InvalidLanguageKey { entry: usize },
    #[error("{table} entry {entry} contains reserved native newline markup #13#10")]
    ReservedNativeNewlineMarkup { table: &'static str, entry: usize },
    #[error("language entry {entry} value contains a bare carriage return or line feed")]
    InvalidLanguageValueLineBreak { entry: usize },
    #[error("constant-string entry {entry} value contains a bare carriage return or line feed")]
    InvalidConstStringValueLineBreak { entry: usize },
    #[error("NSetc entry {entry} contains a carriage return or line feed")]
    InvalidNSetcEntryLineBreak { entry: usize },
    #[error("NSetc structured strings require a DAT or LST payload")]
    UnsupportedNSetcPayloadKind,
    #[error("invalid NSgtdData document: {message}")]
    InvalidGtdDocument { message: String },
    #[error("unsupported NSgtdData record name: {name}")]
    UnsupportedGtdRecord { name: String },
    #[error("NSgtdData document kind does not match {expected}")]
    GtdDocumentKindMismatch { expected: String },
}

pub type Result<T> = std::result::Result<T, TextError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextPayloadKind {
    Dat,
    List,
    Raw,
}

/// Legacy character encodings used by localized NosTale text payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Big5,
    EucKr,
    ShiftJis,
    Windows1250,
    Windows1251,
    Windows1252,
    Windows1254,
}

impl TextEncoding {
    /// Parse a canonical or commonly used encoding label.
    pub fn for_label(label: &str) -> Option<Self> {
        match label.to_ascii_lowercase().replace('_', "-").as_str() {
            "big5" => Some(Self::Big5),
            "euc-kr" | "euckr" | "windows-949" | "cp949" => Some(Self::EucKr),
            "shift-jis" | "shiftjis" | "sjis" | "windows-932" | "cp932" => Some(Self::ShiftJis),
            "windows-1250" | "cp1250" => Some(Self::Windows1250),
            "windows-1251" | "cp1251" => Some(Self::Windows1251),
            "windows-1252" | "cp1252" => Some(Self::Windows1252),
            "windows-1254" | "cp1254" => Some(Self::Windows1254),
            _ => None,
        }
    }

    /// Return the canonical label written in diagnostics and documentation.
    pub fn label(self) -> &'static str {
        self.encoding().name()
    }

    fn encoding(self) -> &'static Encoding {
        match self {
            Self::Big5 => BIG5,
            Self::EucKr => EUC_KR,
            Self::ShiftJis => SHIFT_JIS,
            Self::Windows1250 => WINDOWS_1250,
            Self::Windows1251 => WINDOWS_1251,
            Self::Windows1252 => WINDOWS_1252,
            Self::Windows1254 => WINDOWS_1254,
        }
    }
}

/// One logical key/value row exposed by the NSlang JSON converter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageEntry(pub String, pub String);

/// Ordered NSlang key/value rows. Tuple serialization intentionally produces
/// the lightweight `[[key, value], ...]` JSON shape.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LanguageTable(pub Vec<LanguageEntry>);

/// A malformed physical NSlang row skipped while producing a logical table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedLanguageRow {
    pub row: usize,
}

/// Result of parsing an NSlang payload, including rows that require warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLanguageTable {
    pub table: LanguageTable,
    pub malformed_rows: Vec<MalformedLanguageRow>,
}

/// One numeric constant-string lookup row exposed by the NScli JSON converter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstStringEntry(pub i32, pub String);

/// Ordered NScli constant-string rows serialized as `[[key, value], ...]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConstStringTable(pub Vec<ConstStringEntry>);

/// Reason an NScli physical row could not be represented as a valid entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MalformedConstStringRowKind {
    MissingVerticalTab,
    InvalidIntegerKey,
}

/// A malformed physical NScli row skipped while producing a logical table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedConstStringRow {
    pub row: usize,
    pub kind: MalformedConstStringRowKind,
}

/// Result of parsing an NScli payload, including rows that require warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedConstStringTable {
    pub table: ConstStringTable,
    pub malformed_rows: Vec<MalformedConstStringRow>,
}

/// Ordered strings exposed by the structured `NSetcData` JSON converter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NSetcStringList(pub Vec<String>);

/// Decode an NSlang DAT payload using the valid-row behavior of the client.
pub fn decode_language_table(data: &[u8], encoding: TextEncoding) -> Result<ParsedLanguageTable> {
    let decoded = decode_dat_payload(data)?;
    let text = decode_legacy_text(&decoded, encoding)?;
    let mut table = Vec::new();
    let mut malformed_rows = Vec::new();

    for (index, physical_row) in text.split_terminator('\n').enumerate() {
        let row = index + 1;
        if physical_row.trim().is_empty() || physical_row.starts_with('#') {
            continue;
        }

        let normalized = physical_row.replace("#13#10", "\r\n");
        let Some((key, value)) = normalized.split_once('\t') else {
            malformed_rows.push(MalformedLanguageRow { row });
            continue;
        };
        table.push(LanguageEntry(key.to_owned(), value.replace("[n]", "\r\n")));
    }

    Ok(ParsedLanguageTable {
        table: LanguageTable(table),
        malformed_rows,
    })
}

/// Encode an ordered logical NSlang table into a compact DAT payload.
pub fn encode_language_table(table: &LanguageTable, encoding: TextEncoding) -> Result<Vec<u8>> {
    let mut text = String::new();
    for (index, LanguageEntry(key, value)) in table.0.iter().enumerate() {
        if key.starts_with('#') || key.contains(['\t', '\r', '\n']) {
            return Err(TextError::InvalidLanguageKey { entry: index });
        }
        validate_reserved_newline_markup(key, "language", index)?;
        validate_reserved_newline_markup(value, "language", index)?;
        validate_crlf(value, index)?;
        text.push_str(key);
        text.push('\t');
        text.push_str(&value.replace("\r\n", "[n]"));
        text.push('\n');
    }
    let bytes = encode_legacy_text(&text, encoding)?;
    encode_dat_payload(&bytes)
}

/// Decode an NScli constant-string DAT payload into valid numeric rows.
pub fn decode_const_string_table(
    data: &[u8],
    encoding: TextEncoding,
) -> Result<ParsedConstStringTable> {
    let decoded = decode_dat_payload(data)?;
    let text = decode_legacy_text(&decoded, encoding)?;
    let mut table = Vec::new();
    let mut malformed_rows = Vec::new();

    for (index, physical_row) in text.split_terminator('\n').enumerate() {
        let row = index + 1;
        if physical_row.trim().is_empty() {
            continue;
        }

        let normalized = physical_row.replace("#13#10", "\r\n");
        let Some((key, value)) = normalized.split_once('\u{000b}') else {
            malformed_rows.push(MalformedConstStringRow {
                row,
                kind: MalformedConstStringRowKind::MissingVerticalTab,
            });
            continue;
        };
        let Ok(key) = key.parse::<i32>() else {
            malformed_rows.push(MalformedConstStringRow {
                row,
                kind: MalformedConstStringRowKind::InvalidIntegerKey,
            });
            continue;
        };
        table.push(ConstStringEntry(key, value.to_owned()));
    }

    Ok(ParsedConstStringTable {
        table: ConstStringTable(table),
        malformed_rows,
    })
}

/// Encode an ordered logical NScli constant-string table into a compact DAT payload.
pub fn encode_const_string_table(
    table: &ConstStringTable,
    encoding: TextEncoding,
) -> Result<Vec<u8>> {
    let mut text = String::new();
    for (index, ConstStringEntry(key, value)) in table.0.iter().enumerate() {
        validate_reserved_newline_markup(value, "constant-string", index)?;
        validate_const_string_crlf(value, index)?;
        text.push_str(&key.to_string());
        text.push('\u{000b}');
        text.push_str(&value.replace("\r\n", "#13#10"));
        text.push('\n');
    }
    let bytes = encode_legacy_text(&text, encoding)?;
    encode_dat_payload(&bytes)
}

/// Decode an `NSetcData` DAT or LST payload into its ordered strings.
pub fn decode_nsetc_string_list(
    data: &[u8],
    kind: TextPayloadKind,
    encoding: TextEncoding,
) -> Result<NSetcStringList> {
    let decoded = match kind {
        TextPayloadKind::Dat => decode_dat_payload(data)?,
        TextPayloadKind::List => decode_list_payload(data)?,
        TextPayloadKind::Raw => return Err(TextError::UnsupportedNSetcPayloadKind),
    };
    let text = decode_legacy_text(&decoded, encoding)?;
    if text.is_empty() {
        return Ok(NSetcStringList::default());
    }

    let text = text.strip_suffix('\n').unwrap_or(&text);
    Ok(NSetcStringList(
        text.split('\n').map(str::to_owned).collect(),
    ))
}

/// Encode ordered `NSetcData` strings into a DAT or LST payload.
pub fn encode_nsetc_string_list(
    list: &NSetcStringList,
    kind: TextPayloadKind,
    encoding: TextEncoding,
) -> Result<Vec<u8>> {
    let mut text = String::new();
    for (index, entry) in list.0.iter().enumerate() {
        if entry.contains(['\r', '\n']) {
            return Err(TextError::InvalidNSetcEntryLineBreak { entry: index });
        }
        text.push_str(entry);
        text.push('\n');
    }
    let bytes = encode_legacy_text(&text, encoding)?;
    match kind {
        TextPayloadKind::Dat => encode_dat_payload(&bytes),
        TextPayloadKind::List => encode_list_payload(&bytes),
        TextPayloadKind::Raw => Err(TextError::UnsupportedNSetcPayloadKind),
    }
}

fn decode_legacy_text(data: &[u8], encoding: TextEncoding) -> Result<Cow<'_, str>> {
    encoding
        .encoding()
        .decode_without_bom_handling_and_without_replacement(data)
        .ok_or(TextError::InvalidTextEncoding {
            encoding: encoding.label(),
        })
}

fn encode_legacy_text(text: &str, encoding: TextEncoding) -> Result<Cow<'_, [u8]>> {
    let (bytes, _, had_errors) = encoding.encoding().encode(text);
    if had_errors {
        return Err(TextError::UnrepresentableText {
            encoding: encoding.label(),
        });
    }
    Ok(bytes)
}

fn validate_reserved_newline_markup(value: &str, table: &'static str, entry: usize) -> Result<()> {
    if value.contains("#13#10") {
        return Err(TextError::ReservedNativeNewlineMarkup { table, entry });
    }
    Ok(())
}

fn validate_crlf(value: &str, entry: usize) -> Result<()> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => index += 2,
            b'\r' | b'\n' => return Err(TextError::InvalidLanguageValueLineBreak { entry }),
            _ => index += 1,
        }
    }
    Ok(())
}

fn validate_const_string_crlf(value: &str, entry: usize) -> Result<()> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => index += 2,
            b'\r' | b'\n' => return Err(TextError::InvalidConstStringValueLineBreak { entry }),
            _ => index += 1,
        }
    }
    Ok(())
}

pub fn decode_dat_payload(data: &[u8]) -> Result<Vec<u8>> {
    let mut offset = 0;
    let mut decoded = Vec::new();

    while offset < data.len() {
        let control = data[offset];
        offset += 1;

        if control == 0xff {
            decoded.push(b'\n');
            continue;
        }

        let run_len = usize::from(control & 0x7f);
        if control & 0x80 != 0 {
            let needed = run_len.div_ceil(2);
            if offset.saturating_add(needed) > data.len() {
                return Err(TextError::TruncatedDatPayload {
                    offset,
                    needed,
                    actual: data.len().saturating_sub(offset),
                });
            }

            let mut remaining = run_len;
            while remaining > 0 {
                let packed = data[offset];
                offset += 1;

                decoded.push(COMPACT_DATA_CHARS[usize::from(packed >> 4)]);
                remaining -= 1;

                if remaining > 0 {
                    let value = COMPACT_DATA_CHARS[usize::from(packed & 0x0f)];
                    if value != 0 {
                        decoded.push(value);
                    }
                    remaining -= 1;
                }
            }
        } else {
            if offset.saturating_add(run_len) > data.len() {
                return Err(TextError::TruncatedDatPayload {
                    offset,
                    needed: run_len,
                    actual: data.len().saturating_sub(offset),
                });
            }

            for byte in &data[offset..offset + run_len] {
                decoded.push(byte ^ 0x33);
            }
            offset += run_len;
        }
    }

    Ok(decoded)
}

pub fn decode_list_payload(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 4 {
        return Err(TextError::TruncatedListPayload {
            needed: 4,
            actual: data.len(),
        });
    }

    let mut offset = 0;
    let count = read_i32_at(data, offset);
    offset += 4;
    if count < 0 {
        return Err(TextError::InvalidListLineCount { count });
    }

    let mut decoded = Vec::new();
    for line in 0..count as usize {
        if offset.saturating_add(4) > data.len() {
            return Err(TextError::TruncatedListLine {
                line,
                needed: 4,
                actual: data.len().saturating_sub(offset),
            });
        }

        let len = read_i32_at(data, offset);
        offset += 4;
        if len < 0 {
            return Err(TextError::InvalidListLineLength { line, value: len });
        }

        let len = len as usize;
        if offset.saturating_add(len) > data.len() {
            return Err(TextError::TruncatedListLine {
                line,
                needed: len,
                actual: data.len().saturating_sub(offset),
            });
        }

        for byte in &data[offset..offset + len] {
            decoded.push(byte ^ 0x01);
        }
        decoded.push(b'\n');
        offset += len;
    }

    Ok(decoded)
}

pub fn encode_dat_payload(decoded: &[u8]) -> Result<Vec<u8>> {
    if decoded.is_empty() {
        return Ok(Vec::new());
    }

    let mut encoded = Vec::new();
    for line in text_lines(decoded) {
        for chunk in line.chunks(0x7f) {
            encoded.push(chunk.len() as u8);
            encoded.extend(chunk.iter().map(|byte| byte ^ 0x33));
        }
        encoded.push(0xff);
    }
    Ok(encoded)
}

pub fn encode_list_payload(decoded: &[u8]) -> Result<Vec<u8>> {
    let lines = text_lines(decoded);
    let count =
        i32::try_from(lines.len()).map_err(|_| TextError::TooManyRecords { count: lines.len() })?;
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&count.to_le_bytes());
    for (index, line) in lines.iter().enumerate() {
        let len = i32::try_from(line.len()).map_err(|_| TextError::RecordTooLarge {
            name: format!("line {index}"),
            field: "line",
            size: line.len(),
        })?;
        encoded.extend_from_slice(&len.to_le_bytes());
        encoded.extend(line.iter().map(|byte| byte ^ 0x01));
    }
    Ok(encoded)
}

fn text_lines(decoded: &[u8]) -> Vec<&[u8]> {
    if decoded.is_empty() {
        return Vec::new();
    }
    let mut lines = decoded.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    if decoded.ends_with(b"\n") {
        lines.pop();
    }
    lines
}

fn read_i32_at(data: &[u8], offset: usize) -> i32 {
    ByteReader::new_at(data, offset)
        .read_i32_le("i32")
        .expect("caller validates i32 offset")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_jis_labels_and_text_round_trip() {
        for label in ["shift-jis", "shiftjis", "sjis", "windows-932", "cp932"] {
            assert_eq!(TextEncoding::for_label(label), Some(TextEncoding::ShiftJis));
        }
        let encoded = encode_legacy_text("ノーステイル", TextEncoding::ShiftJis).unwrap();
        assert_eq!(
            decode_legacy_text(&encoded, TextEncoding::ShiftJis).unwrap(),
            "ノーステイル"
        );
    }

    #[test]
    fn decodes_dat_raw_runs() {
        let encoded = vec![2, b'A' ^ 0x33, b'B' ^ 0x33, 0xff];
        assert_eq!(decode_dat_payload(&encoded).unwrap(), b"AB\n");
    }

    #[test]
    fn decodes_dat_packed_runs() {
        let encoded = vec![0x80 | 4, 0x56, 0x17, 0xff];
        assert_eq!(decode_dat_payload(&encoded).unwrap(), b"12 3\n");
    }

    #[test]
    fn decodes_dat_multiple_lines() {
        let encoded = vec![1, b'A' ^ 0x33, 0xff, 1, b'B' ^ 0x33, 0xff];
        assert_eq!(decode_dat_payload(&encoded).unwrap(), b"A\nB\n");
    }

    #[test]
    fn rejects_truncated_dat_raw_runs() {
        let error = decode_dat_payload(&[3, b'A' ^ 0x33]).unwrap_err();
        assert!(matches!(error, TextError::TruncatedDatPayload { .. }));
    }

    #[test]
    fn rejects_truncated_dat_packed_runs() {
        let error = decode_dat_payload(&[0x80 | 3, 0x56]).unwrap_err();
        assert!(matches!(error, TextError::TruncatedDatPayload { .. }));
    }

    #[test]
    fn decodes_list_payload() {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&2_i32.to_le_bytes());
        push_list_line(&mut encoded, b"abc");
        push_list_line(&mut encoded, b"de");

        assert_eq!(decode_list_payload(&encoded).unwrap(), b"abc\nde\n");
    }

    #[test]
    fn rejects_truncated_list_payload() {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&1_i32.to_le_bytes());
        encoded.extend_from_slice(&4_i32.to_le_bytes());
        encoded.extend_from_slice(&[b'a' ^ 0x01]);

        let error = decode_list_payload(&encoded).unwrap_err();
        assert!(matches!(error, TextError::TruncatedListLine { .. }));
    }

    #[test]
    fn encodes_dat_payload_as_raw_runs() {
        let encoded = encode_dat_payload(b"AB\nCD\n").unwrap();
        assert_eq!(decode_dat_payload(&encoded).unwrap(), b"AB\nCD\n");
    }

    #[test]
    fn encodes_list_payload() {
        let encoded = encode_list_payload(b"abc\nde\n").unwrap();
        assert_eq!(decode_list_payload(&encoded).unwrap(), b"abc\nde\n");
    }

    #[test]
    fn language_table_parses_only_valid_rows_like_the_client() {
        let decoded = concat!(
            "zts1e\tSpecial Attack[n]More\n",
            "\n",
            "# ignored comment\n",
            "  # not a first-byte comment\n",
            "orphan continuation\n",
            "custom\tvalue\textra\n",
            "zts2e\tBefore#13#10After\n",
        );
        let encoded = encode_dat_payload(decoded.as_bytes()).unwrap();
        let parsed = decode_language_table(&encoded, TextEncoding::Windows1252).unwrap();

        assert_eq!(
            parsed.table,
            LanguageTable(vec![
                LanguageEntry("zts1e".into(), "Special Attack\r\nMore".into()),
                LanguageEntry("custom".into(), "value\textra".into()),
                LanguageEntry("zts2e".into(), "Before\r\nAfter".into()),
            ])
        );
        assert_eq!(
            parsed.malformed_rows,
            vec![
                MalformedLanguageRow { row: 4 },
                MalformedLanguageRow { row: 5 }
            ]
        );
    }

    #[test]
    fn language_table_round_trips_logical_entries() {
        let expected = LanguageTable(vec![
            LanguageEntry("zts1e".into(), "Line one\r\nLine two".into()),
            LanguageEntry("arbitrary".into(), "value\textra".into()),
            LanguageEntry("arbitrary".into(), "duplicate".into()),
        ]);

        let encoded = encode_language_table(&expected, TextEncoding::Windows1252).unwrap();
        let parsed = decode_language_table(&encoded, TextEncoding::Windows1252).unwrap();
        assert_eq!(parsed.table, expected);
        assert!(parsed.malformed_rows.is_empty());
    }

    #[test]
    fn language_table_rejects_invalid_json_text_structure() {
        let bad_key = LanguageTable(vec![LanguageEntry("bad\tkey".into(), "value".into())]);
        assert!(matches!(
            encode_language_table(&bad_key, TextEncoding::Windows1252),
            Err(TextError::InvalidLanguageKey { entry: 0 })
        ));

        let comment_key = LanguageTable(vec![LanguageEntry("#key".into(), "value".into())]);
        assert!(matches!(
            encode_language_table(&comment_key, TextEncoding::Windows1252),
            Err(TextError::InvalidLanguageKey { entry: 0 })
        ));

        for table in [
            LanguageTable(vec![LanguageEntry(
                "key#13#10suffix".into(),
                "value".into(),
            )]),
            LanguageTable(vec![LanguageEntry(
                "key".into(),
                "literal#13#10value".into(),
            )]),
        ] {
            assert!(matches!(
                encode_language_table(&table, TextEncoding::Windows1252),
                Err(TextError::ReservedNativeNewlineMarkup {
                    table: "language",
                    entry: 0
                })
            ));
        }

        let bare_lf = LanguageTable(vec![LanguageEntry("key".into(), "bad\nvalue".into())]);
        assert!(matches!(
            encode_language_table(&bare_lf, TextEncoding::Windows1252),
            Err(TextError::InvalidLanguageValueLineBreak { entry: 0 })
        ));
    }

    #[test]
    fn language_table_uses_strict_legacy_encoding() {
        let encoded = encode_dat_payload(&[0x81, b'\t', b'x', b'\n']).unwrap();
        assert!(matches!(
            decode_language_table(&encoded, TextEncoding::Big5),
            Err(TextError::InvalidTextEncoding { .. })
        ));

        let table = LanguageTable(vec![LanguageEntry("key".into(), "Ж".into())]);
        assert!(matches!(
            encode_language_table(&table, TextEncoding::Windows1252),
            Err(TextError::UnrepresentableText { .. })
        ));
    }

    #[test]
    fn language_table_json_shape_is_an_array_of_pairs() {
        let table = LanguageTable(vec![LanguageEntry("key".into(), "value".into())]);
        assert_eq!(
            serde_json::to_string(&table).unwrap(),
            r#"[["key","value"]]"#
        );
        assert!(serde_json::from_str::<LanguageTable>(r#"[["key"]]"#).is_err());
        assert!(serde_json::from_str::<LanguageTable>(r#"[["key","value","extra"]]"#).is_err());
    }

    #[test]
    fn const_string_table_parses_numeric_rows_and_reports_malformed_rows() {
        let decoded = concat!(
            "0\u{000b}Message\n",
            "\n",
            "bad row\n",
            "not-a-number\u{000b}text\n",
            "7\u{000b}value\u{000b}extra\n",
            "8\u{000b}Before#13#10After[n]<NEW_TYPE>\n",
        );
        let encoded = encode_dat_payload(decoded.as_bytes()).unwrap();
        let parsed = decode_const_string_table(&encoded, TextEncoding::Windows1252).unwrap();

        assert_eq!(
            parsed.table,
            ConstStringTable(vec![
                ConstStringEntry(0, "Message".into()),
                ConstStringEntry(7, "value\u{000b}extra".into()),
                ConstStringEntry(8, "Before\r\nAfter[n]<NEW_TYPE>".into()),
            ])
        );
        assert_eq!(
            parsed.malformed_rows,
            vec![
                MalformedConstStringRow {
                    row: 3,
                    kind: MalformedConstStringRowKind::MissingVerticalTab,
                },
                MalformedConstStringRow {
                    row: 4,
                    kind: MalformedConstStringRowKind::InvalidIntegerKey,
                },
            ]
        );
    }

    #[test]
    fn const_string_table_round_trips_logical_entries() {
        let expected = ConstStringTable(vec![
            ConstStringEntry(1, "Line one\r\nLine two".into()),
            ConstStringEntry(-1, "[n]<NEW_TYPE>".into()),
            ConstStringEntry(1, "duplicate key".into()),
        ]);
        let encoded = encode_const_string_table(&expected, TextEncoding::Windows1252).unwrap();
        let parsed = decode_const_string_table(&encoded, TextEncoding::Windows1252).unwrap();
        assert_eq!(parsed.table, expected);
        assert!(parsed.malformed_rows.is_empty());
    }

    #[test]
    fn const_string_table_rejects_bare_line_breaks_and_bad_json_pairs() {
        let table = ConstStringTable(vec![ConstStringEntry(1, "bad\nvalue".into())]);
        assert!(matches!(
            encode_const_string_table(&table, TextEncoding::Windows1252),
            Err(TextError::InvalidConstStringValueLineBreak { entry: 0 })
        ));

        let reserved = ConstStringTable(vec![ConstStringEntry(1, "literal#13#10value".into())]);
        assert!(matches!(
            encode_const_string_table(&reserved, TextEncoding::Windows1252),
            Err(TextError::ReservedNativeNewlineMarkup {
                table: "constant-string",
                entry: 0
            })
        ));

        assert_eq!(
            serde_json::to_string(&ConstStringTable(vec![ConstStringEntry(7, "text".into())]))
                .unwrap(),
            r#"[[7,"text"]]"#
        );
        assert!(serde_json::from_str::<ConstStringTable>(r#"[["7","text"]]"#).is_err());
        assert!(serde_json::from_str::<ConstStringTable>(r#"[[7]]"#).is_err());
        assert!(serde_json::from_str::<ConstStringTable>(r#"[[2147483648,"text"]]"#).is_err());
    }

    #[test]
    fn nsetc_string_list_round_trips_dat_and_list_payloads() {
        let expected = NSetcStringList(vec![
            "wolly".into(),
            "금지어".into(),
            "wolly".into(),
            String::new(),
        ]);

        for kind in [TextPayloadKind::Dat, TextPayloadKind::List] {
            let encoded = encode_nsetc_string_list(&expected, kind, TextEncoding::EucKr).unwrap();
            assert_eq!(
                decode_nsetc_string_list(&encoded, kind, TextEncoding::EucKr).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn nsetc_string_list_json_shape_is_an_array_of_strings() {
        let list = NSetcStringList(vec!["wolly".into(), "sheep".into()]);
        assert_eq!(
            serde_json::to_string(&list).unwrap(),
            r#"["wolly","sheep"]"#
        );
        assert!(serde_json::from_str::<NSetcStringList>(r#"["wolly",7]"#).is_err());
    }

    #[test]
    fn nsetc_string_list_rejects_multiline_unrepresentable_and_raw_entries() {
        for value in ["bad\nvalue", "bad\r\nvalue"] {
            let list = NSetcStringList(vec![value.into()]);
            assert!(matches!(
                encode_nsetc_string_list(&list, TextPayloadKind::Dat, TextEncoding::EucKr),
                Err(TextError::InvalidNSetcEntryLineBreak { entry: 0 })
            ));
        }

        let unrepresentable = NSetcStringList(vec!["😀".into()]);
        assert!(matches!(
            encode_nsetc_string_list(&unrepresentable, TextPayloadKind::Dat, TextEncoding::EucKr,),
            Err(TextError::UnrepresentableText { .. })
        ));

        assert!(matches!(
            encode_nsetc_string_list(
                &NSetcStringList::default(),
                TextPayloadKind::Raw,
                TextEncoding::EucKr,
            ),
            Err(TextError::UnsupportedNSetcPayloadKind)
        ));
        assert!(matches!(
            decode_nsetc_string_list(&[], TextPayloadKind::Raw, TextEncoding::EucKr),
            Err(TextError::UnsupportedNSetcPayloadKind)
        ));
    }

    fn push_list_line(out: &mut Vec<u8>, line: &[u8]) {
        out.extend_from_slice(&(line.len() as i32).to_le_bytes());
        out.extend(line.iter().map(|byte| byte ^ 0x01));
    }
}
