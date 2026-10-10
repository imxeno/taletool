use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::row_tokens::{client_int, split_token, tokens, trim};
use super::{GtdLocale, GtdWarning, ParsedGtd, warning};
use crate::{
    Result, TextEncoding, TextError, TextPayloadKind, decode_legacy_text, decode_text_rows,
    encode_dat_payload, encode_legacy_text,
};

/// Most rows the client's `DSTART` scan reads as description text.
const DESCRIPTION_SCAN_ROWS: usize = 20;

/// Contents of one localized `*_nosmall.dat` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NosMallDocument {
    pub locale: GtdLocale,
    pub entries: Vec<NosMallEntry>,
}

/// One native NosMall entry.
///
/// Only `VNUM` is required. An absent row is omitted, and the client keeps the
/// item's zero values or empty text for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NosMallEntry {
    pub vnum: NosMallValue,
    #[serde(default)]
    pub vnum_fields: Vec<NosMallValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Vec<NosMallValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title2: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Vec<NosMallValue>>,
    /// The linked-item count followed by the row's linked item IDs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Vec<NosMallValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_lines: Option<Vec<String>>,
}

/// One NosMall row value: a decimal integer, or the source token itself when it
/// is not one, such as a `True` VNUM flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NosMallValue {
    Integer(i32),
    Text(String),
}

impl NosMallValue {
    fn from_token(token: &str) -> Self {
        match client_int(token) {
            Some(value) if value.to_string() == token => Self::Integer(value),
            _ => Self::Text(token.to_owned()),
        }
    }
}

impl From<i32> for NosMallValue {
    fn from(value: i32) -> Self {
        Self::Integer(value)
    }
}

/// Decode a NosMall DAT payload with the client's item, row and description rules.
pub fn decode_nos_mall(
    data: &[u8],
    kind: TextPayloadKind,
    locale: GtdLocale,
    encoding: TextEncoding,
) -> Result<ParsedGtd<NosMallDocument>> {
    let decoded = decode_text_rows(data, kind)?;
    let text = decode_legacy_text(&decoded, encoding)?;
    let lines = text
        .split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    // The client applies rows before the first VNUM to an item it discards.
    let mut current: Option<EntryBuilder> = None;
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let row = index + 1;
        index += 1;
        let (token, rest) = split_token(line);
        if token.is_empty() || token.starts_with('#') {
            continue;
        }
        let tag = trim(token).to_ascii_uppercase();
        if tag == "VNUM" {
            entries.extend(current.take().map(|builder| builder.entry));
            current = Some(EntryBuilder::new(rest, row, &mut warnings));
            continue;
        }
        let description = if tag == "DSTART" {
            let (description, next) = scan_description(&lines, index, &mut warnings);
            index = next;
            Some(description)
        } else {
            None
        };
        // The client applies every LINK row, even one that a later row
        // replaces or that precedes the first VNUM.
        if tag == "LINK" && !is_client_link_count(&row_values(rest)) {
            warnings.push(warning(
                row,
                "NosMall LINK count reads as a negative 16-bit value, so the client raises a range error",
            ));
        }
        let Some(builder) = current.as_mut() else {
            if !matches!(tag.as_str(), "DEND" | "END" | "~") {
                warnings.push(warning(row, "expected VNUM entry start"));
            }
            continue;
        };

        let entry = &mut builder.entry;
        match tag.as_str() {
            "ITEM" => entry.item = Some(row_values(rest)),
            "ID" => entry.id = Some(row_text(line).to_owned()),
            "TITLE1" => entry.title1 = Some(row_text(line).to_owned()),
            "TITLE2" => entry.title2 = Some(row_text(line).to_owned()),
            "COST" => entry.cost = Some(row_values(rest)),
            "LINK" => entry.link = Some(row_values(rest)),
            "DSTART" => entry.description_lines = description,
            "DEND" | "END" | "~" => continue,
            _ => {
                warnings.push(warning(
                    row,
                    format!("ignored unknown NosMall row {}", trim(token)),
                ));
                continue;
            }
        }
        if let Some(earlier) = builder.rows.insert(tag.clone(), row) {
            warnings.push(warning(
                earlier,
                format!("NosMall {tag} row replaced by row {row}"),
            ));
        }
    }
    entries.extend(current.map(|builder| builder.entry));

    Ok(ParsedGtd {
        document: NosMallDocument { locale, entries },
        warnings,
    })
}

struct EntryBuilder {
    entry: NosMallEntry,
    /// Source row of each singleton row the entry holds, keyed by tag.
    rows: HashMap<String, usize>,
}

impl EntryBuilder {
    fn new(rest: &str, row: usize, warnings: &mut Vec<GtdWarning>) -> Self {
        let mut values = row_values(rest).into_iter();
        let vnum = values.next().unwrap_or_else(|| {
            warnings.push(warning(row, "VNUM row without an item ID stored as -1"));
            NosMallValue::Integer(-1)
        });
        Self {
            entry: NosMallEntry {
                vnum,
                vnum_fields: values.collect(),
                item: None,
                id: None,
                title1: None,
                title2: None,
                cost: None,
                link: None,
                description_lines: None,
            },
            rows: HashMap::new(),
        }
    }
}

/// Read the rows after `DSTART` the way the client's description scan does.
///
/// The scan reads at most 20 rows and stops before a column-one `#`, a `DEND`
/// in any letter case, or the end of the payload. The client then parses later
/// rows as tagged rows, so rows past the scan stay description text only while
/// the client ignores them.
fn scan_description(
    lines: &[&str],
    mut index: usize,
    warnings: &mut Vec<GtdWarning>,
) -> (Vec<String>, usize) {
    let mut description = Vec::new();
    while let Some(line) = lines.get(index) {
        if ends_description_scan(line) {
            break;
        }
        if description.len() >= DESCRIPTION_SCAN_ROWS {
            if is_client_tag_row(line) {
                break;
            }
            if description.len() == DESCRIPTION_SCAN_ROWS {
                warnings.push(warning(
                    index + 1,
                    "NosMall description continues past the client's 20-row scan",
                ));
            }
        }
        description.push((*line).to_owned());
        index += 1;
    }
    (description, index)
}

fn ends_description_scan(line: &str) -> bool {
    line.starts_with('#') || trim(line).eq_ignore_ascii_case("DEND")
}

/// Whether the client's row parser loads data from this row.
fn is_client_tag_row(line: &str) -> bool {
    let tag = trim(split_token(line).0).to_ascii_uppercase();
    matches!(
        tag.as_str(),
        "VNUM" | "ITEM" | "TITLE1" | "TITLE2" | "COST" | "LINK" | "DSTART"
    )
}

/// Whether the client can size the linked-ID list from this LINK row's count.
///
/// The client reads a missing or non-numeric count as -1, stores the count in
/// a signed 16-bit field, and raises a range error when that is negative.
fn is_client_link_count(link: &[NosMallValue]) -> bool {
    let count = match link.first() {
        Some(NosMallValue::Integer(value)) => *value,
        Some(NosMallValue::Text(token)) => client_int(token).unwrap_or(-1),
        None => -1,
    };
    count as i16 >= 0
}

/// The values of a row remainder, read one client token at a time.
fn row_values(rest: &str) -> Vec<NosMallValue> {
    tokens(rest).map(NosMallValue::from_token).collect()
}

/// The text after a row's tag, cut where the client cuts it. Trailing
/// whitespace, which the client trims when loading, is kept for rewriting.
fn row_text(line: &str) -> &str {
    let text = trim(split_token(line).1);
    if text.is_empty() {
        return "";
    }
    // The text ends with the trimmed row, so it extends over the row's
    // trailing whitespace.
    let end = line.trim_end_matches(|c: char| c <= ' ').len();
    &line[end - text.len()..]
}

/// Encode a NosMall document using canonical native framing and DAT encoding.
pub fn encode_nos_mall(document: &NosMallDocument, encoding: TextEncoding) -> Result<Vec<u8>> {
    let mut text = String::new();
    for (index, entry) in document.entries.iter().enumerate() {
        if let Some(message) = unwritable_entry(entry) {
            return Err(TextError::InvalidGtdDocument {
                message: format!("NosMall entry {index} {message}"),
            });
        }
        push_value_row(
            &mut text,
            "VNUM",
            std::iter::once(&entry.vnum).chain(&entry.vnum_fields),
        );
        if let Some(item) = &entry.item {
            push_value_row(&mut text, "ITEM", item);
        }
        for (tag, value) in [
            ("ID", &entry.id),
            ("TITLE1", &entry.title1),
            ("TITLE2", &entry.title2),
        ] {
            if let Some(value) = value {
                push_raw_row(&mut text, tag, value);
            }
        }
        for (tag, values) in [("COST", &entry.cost), ("LINK", &entry.link)] {
            if let Some(values) = values {
                push_value_row(&mut text, tag, values);
            }
        }
        if let Some(lines) = &entry.description_lines {
            text.push_str("DSTART\n");
            for line in lines {
                text.push_str(line);
                text.push('\n');
            }
            text.push_str("DEND\n");
        }
        text.push_str("END\n");
    }
    encode_dat_payload(&encode_legacy_text(&text, encoding)?)
}

/// Describe the first value the client would read differently once written.
fn unwritable_entry(entry: &NosMallEntry) -> Option<String> {
    for (field, value) in [
        ("id", &entry.id),
        ("title1", &entry.title1),
        ("title2", &entry.title2),
    ] {
        let Some(value) = value else {
            continue;
        };
        if value.contains(['\r', '\n']) {
            return Some(format!("{field} contains a line break"));
        }
        // The row trim and the client's title trim drop leading whitespace.
        // Trailing whitespace is accepted because decoding keeps it from rows.
        if value.starts_with(|c: char| c <= ' ') {
            return Some(format!(
                "{field} starts with whitespace that the row's trim would remove"
            ));
        }
    }
    if let Some(text) = unwritable_value(std::iter::once(&entry.vnum).chain(&entry.vnum_fields)) {
        return Some(format!("VNUM value {text:?} is not one client token"));
    }
    for (tag, values) in [
        ("ITEM", &entry.item),
        ("COST", &entry.cost),
        ("LINK", &entry.link),
    ] {
        if let Some(text) = values.iter().find_map(unwritable_value) {
            return Some(format!("{tag} value {text:?} is not one client token"));
        }
    }
    if entry
        .link
        .as_ref()
        .is_some_and(|link| !is_client_link_count(link))
    {
        return Some(
            "LINK count reads as a negative 16-bit value, so the client would raise a range error"
                .into(),
        );
    }
    for (position, line) in entry.description_lines.iter().flatten().enumerate() {
        if line.contains(['\r', '\n']) {
            return Some("description line contains a line break".into());
        }
        if ends_description_scan(line) {
            return Some(format!(
                "description line {line:?} would end the client's description scan"
            ));
        }
        if position >= DESCRIPTION_SCAN_ROWS && is_client_tag_row(line) {
            return Some(format!(
                "description line {line:?} is past the client's 20-row scan and would be read as a tagged row"
            ));
        }
    }
    None
}

/// Find a text value that the client would not read back as one token from a
/// tab-separated row. Only the last value of a row is also cut at spaces.
fn unwritable_value<'a>(values: impl IntoIterator<Item = &'a NosMallValue>) -> Option<&'a str> {
    let mut values = values.into_iter().peekable();
    while let Some(value) = values.next() {
        let NosMallValue::Text(text) = value else {
            continue;
        };
        let last = values.peek().is_none();
        let separators: &[char] = if last {
            &['\t', '\r', '\n', ' ']
        } else {
            &['\t', '\r', '\n']
        };
        let readable = !text.contains(separators)
            && text.starts_with(|c: char| c > ' ')
            && (!last || text.ends_with(|c: char| c > ' '));
        if !readable {
            return Some(text);
        }
    }
    None
}

fn push_value_row<'a>(
    out: &mut String,
    tag: &str,
    values: impl IntoIterator<Item = &'a NosMallValue>,
) {
    out.push_str(tag);
    for value in values {
        out.push('\t');
        match value {
            NosMallValue::Integer(value) => out.push_str(&value.to_string()),
            NosMallValue::Text(text) => out.push_str(text),
        }
    }
    out.push('\n');
}

fn push_raw_row(out: &mut String, tag: &str, value: &str) {
    out.push_str(tag);
    out.push('\t');
    out.push_str(value);
    out.push('\n');
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbusePayloadState {
    Counted,
    ZeroLength,
}

/// Contents of one localized `*_abuse.lst` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbuseDocument {
    pub locale: GtdLocale,
    pub payload_state: AbusePayloadState,
    pub entries: Vec<AbuseEntry>,
}

/// An abuse-list entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AbuseEntry {
    Text { text: String },
    Bytes { bytes_base64: String },
}

/// Decode the native counted/XOR abuse format, distinguishing it from a zero-byte record.
pub fn decode_abuse(
    data: &[u8],
    locale: GtdLocale,
    encoding: TextEncoding,
) -> Result<ParsedGtd<AbuseDocument>> {
    if data.is_empty() {
        return Ok(ParsedGtd {
            document: AbuseDocument {
                locale,
                payload_state: AbusePayloadState::ZeroLength,
                entries: Vec::new(),
            },
            warnings: Vec::new(),
        });
    }
    if data.len() < 4 {
        return Err(TextError::TruncatedListPayload {
            needed: 4,
            actual: data.len(),
        });
    }
    let count = i32::from_le_bytes(data[..4].try_into().expect("four bytes checked"));
    if count < 0 {
        return Err(TextError::InvalidListLineCount { count });
    }
    let mut offset = 4;
    let mut entries = Vec::with_capacity(count as usize);
    for line in 0..count as usize {
        if data.len().saturating_sub(offset) < 4 {
            return Err(TextError::TruncatedListLine {
                line,
                needed: 4,
                actual: data.len().saturating_sub(offset),
            });
        }
        let len = i32::from_le_bytes(data[offset..offset + 4].try_into().expect("range checked"));
        offset += 4;
        if len < 0 {
            return Err(TextError::InvalidListLineLength { line, value: len });
        }
        let len = len as usize;
        if data.len().saturating_sub(offset) < len {
            return Err(TextError::TruncatedListLine {
                line,
                needed: len,
                actual: data.len().saturating_sub(offset),
            });
        }
        let bytes = data[offset..offset + len]
            .iter()
            .map(|byte| byte ^ 1)
            .collect::<Vec<_>>();
        offset += len;
        let entry = match decode_legacy_text(&bytes, encoding) {
            Ok(text)
                if encode_legacy_text(&text, encoding).is_ok_and(|encoded| encoded == bytes) =>
            {
                AbuseEntry::Text {
                    text: text.into_owned(),
                }
            }
            _ => AbuseEntry::Bytes {
                bytes_base64: base64_encode(&bytes),
            },
        };
        entries.push(entry);
    }
    if offset != data.len() {
        return Err(TextError::InvalidGtdDocument {
            message: format!(
                "abuse payload has {} trailing bytes after its declared entries",
                data.len() - offset
            ),
        });
    }
    Ok(ParsedGtd {
        document: AbuseDocument {
            locale,
            payload_state: AbusePayloadState::Counted,
            entries,
        },
        warnings: Vec::new(),
    })
}

/// Encode an abuse document to its exact zero-length or canonical counted representation.
pub fn encode_abuse(document: &AbuseDocument, encoding: TextEncoding) -> Result<Vec<u8>> {
    if document.payload_state == AbusePayloadState::ZeroLength {
        if !document.entries.is_empty() {
            return Err(TextError::InvalidGtdDocument {
                message: "zero-length abuse payload cannot contain entries".into(),
            });
        }
        return Ok(Vec::new());
    }
    let count = i32::try_from(document.entries.len()).map_err(|_| TextError::TooManyRecords {
        count: document.entries.len(),
    })?;
    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    for (index, entry) in document.entries.iter().enumerate() {
        let bytes = match entry {
            AbuseEntry::Text { text } => encode_legacy_text(text, encoding)?.into_owned(),
            AbuseEntry::Bytes { bytes_base64 } => base64_decode(bytes_base64)?,
        };
        let len = i32::try_from(bytes.len()).map_err(|_| TextError::RecordTooLarge {
            name: format!("abuse entry {index}"),
            field: "entry",
            size: bytes.len(),
        })?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend(bytes.iter().map(|byte| byte ^ 1));
    }
    Ok(out)
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(4) {
        return Err(invalid_base64());
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for (chunk_index, chunk) in text.as_bytes().chunks(4).enumerate() {
        let last = chunk_index + 1 == text.len() / 4;
        let a = base64_value(chunk[0])?;
        let b = base64_value(chunk[1])?;
        let c_pad = chunk[2] == b'=';
        let d_pad = chunk[3] == b'=';
        if c_pad && !d_pad || (!last && (c_pad || d_pad)) {
            return Err(invalid_base64());
        }
        let c = if c_pad { 0 } else { base64_value(chunk[2])? };
        let d = if d_pad { 0 } else { base64_value(chunk[3])? };
        let value =
            (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        out.push((value >> 16) as u8);
        if !c_pad {
            out.push((value >> 8) as u8);
        }
        if !d_pad {
            out.push(value as u8);
        }
    }
    Ok(out)
}

fn base64_value(byte: u8) -> Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(invalid_base64()),
    }
}

fn invalid_base64() -> TextError {
    TextError::InvalidGtdDocument {
        message: "abuse bytes_base64 is not valid base64".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_dat_payload;

    fn decode_uk(native: &str) -> ParsedGtd<NosMallDocument> {
        let payload = encode_dat_payload(native.as_bytes()).unwrap();
        decode_nos_mall(
            &payload,
            TextPayloadKind::Dat,
            GtdLocale::Uk,
            TextEncoding::Windows1252,
        )
        .unwrap()
    }

    fn encode_uk(document: &NosMallDocument) -> Result<String> {
        let encoded = encode_nos_mall(document, TextEncoding::Windows1252)?;
        Ok(String::from_utf8(decode_dat_payload(&encoded)?).unwrap())
    }

    fn warning_rows(parsed: &ParsedGtd<NosMallDocument>) -> Vec<usize> {
        parsed.warnings.iter().map(|warning| warning.row).collect()
    }

    fn values(values: &[i32]) -> Option<Vec<NosMallValue>> {
        Some(values.iter().copied().map(NosMallValue::from).collect())
    }

    fn text(value: &str) -> NosMallValue {
        NosMallValue::Text(value.into())
    }

    fn lines(lines: &[&str]) -> Option<Vec<String>> {
        Some(lines.iter().map(|line| (*line).to_owned()).collect())
    }

    #[test]
    fn nos_mall_preserves_multiline_and_blank_descriptions() {
        let native = concat!(
            "VNUM\t1\t999999\t0\t0\t0\t1\t1\n",
            "ITEM\t0\t0\t1115\t1115\t1\t1\n",
            "ID\t00001  \n",
            "TITLE1\tzts1e\n",
            "TITLE2\tzts2e\n",
            "COST\t999999\t0\t1\t1\t0\t30\n",
            "LINK\t0\t0\t0\t0\t0\t0\n",
            "DSTART\n",
            "zts3e\n",
            "\n",
            "literal trailing  \n",
            "DEND\n",
            "END\n",
        );
        let parsed = decode_uk(native);
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries[0].id.as_deref(), Some("00001  "));
        assert_eq!(
            parsed.document.entries[0].description_lines,
            lines(&["zts3e", "", "literal trailing  "])
        );
        assert_eq!(encode_uk(&parsed.document).unwrap(), native);
    }

    #[test]
    fn nos_mall_uses_vnum_and_eof_boundaries_without_end_rows() {
        let native = concat!(
            "VNUM 1 0 0 0 0 0 0\n",
            "ITEM 0 0 0 0 0 0\nID one\nTITLE1 a\nTITLE2 b\n",
            "COST 0 0 0 0 0 0\nLINK 0 0 0 0 0 0\nDSTART\nfirst\nDEND\n",
            "VNUM 2 0 0 0 0 0 0\n",
            "ITEM 0 0 0 0 0 0\nID two\nTITLE1 c\nTITLE2 d\n",
            "COST 0 0 0 0 0 0\nLINK 0 0 0 0 0 0\nDSTART\nsecond\nDEND\n",
        );
        let parsed = decode_uk(native);

        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(parsed.document.entries[0].id.as_deref(), Some("one"));
        assert_eq!(
            parsed.document.entries[1].description_lines,
            lines(&["second"])
        );
    }

    #[test]
    fn nos_mall_keeps_items_with_any_row_missing() {
        let parsed = decode_uk("VNUM 1\nVNUM 2 0\nTITLE1 only a title\nDSTART\nDEND\n");

        assert!(parsed.warnings.is_empty());
        let entries = &parsed.document.entries;
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            NosMallEntry {
                vnum: 1.into(),
                vnum_fields: Vec::new(),
                item: None,
                id: None,
                title1: None,
                title2: None,
                cost: None,
                link: None,
                description_lines: None,
            }
        );
        assert_eq!(entries[1].id, None);
        assert_eq!(entries[1].title1.as_deref(), Some("only a title"));
        assert_eq!(entries[1].description_lines, Some(Vec::new()));
        assert_eq!(
            encode_uk(&parsed.document).unwrap(),
            "VNUM\t1\nEND\nVNUM\t2\t0\nTITLE1\tonly a title\nDSTART\nDEND\nEND\n"
        );
    }

    #[test]
    fn nos_mall_link_is_a_counted_list_of_any_length() {
        let native = concat!(
            "VNUM\t1\nLINK\t0\nEND\n",
            "VNUM\t2\nLINK\t1\t99\nEND\n",
            "VNUM\t3\nLINK\t7\t1\t2\t3\t4\t5\t6\t7\nEND\n",
            "VNUM\t4\nLINK\t3\t10\nEND\n",
        );
        let parsed = decode_uk(native);

        assert!(parsed.warnings.is_empty());
        let links = parsed
            .document
            .entries
            .iter()
            .map(|entry| entry.link.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            links,
            [
                values(&[0]),
                values(&[1, 99]),
                values(&[7, 1, 2, 3, 4, 5, 6, 7]),
                values(&[3, 10]),
            ]
        );
        assert_eq!(encode_uk(&parsed.document).unwrap(), native);
    }

    #[test]
    fn nos_mall_link_counts_must_fit_the_client_array() {
        let parsed = decode_uk(concat!(
            "VNUM 1\nLINK -1 5\n",
            "VNUM 2\nLINK\n",
            "VNUM 3\nLINK many\n",
            "VNUM 4\nLINK 32768\n",
            "VNUM 5\nLINK $FFFF 1\n",
            "VNUM 6\nLINK -$3 1\n",
        ));

        assert_eq!(warning_rows(&parsed), [2, 4, 6, 8, 10, 12]);
        assert!(parsed.warnings[0].message.contains("range error"));
        assert_eq!(parsed.document.entries[0].link, values(&[-1, 5]));
        for entry in &parsed.document.entries {
            let document = NosMallDocument {
                locale: GtdLocale::Uk,
                entries: vec![entry.clone()],
            };
            assert!(encode_uk(&document).is_err(), "{:?}", entry.link);
        }

        // The client reads signed and hexadecimal counts and keeps the low 16
        // bits.
        let native = concat!(
            "VNUM\t1\nLINK\t$3\t10\t11\t12\nEND\n",
            "VNUM\t2\nLINK\t65537\t10\nEND\n",
            "VNUM\t3\nLINK\t0x2\t1\t2\nEND\n",
            "VNUM\t4\nLINK\tx1\t1\nEND\n",
            "VNUM\t5\nLINK\t03\t1\t2\t3\nEND\n",
            "VNUM\t6\nLINK\t65536\nEND\n",
            "VNUM\t7\nLINK\t32767\nEND\n",
            "VNUM\t8\nLINK\t+$2\t1\t2\nEND\n",
            "VNUM\t9\nLINK\t-$FFFFFFFF\t1\nEND\n",
        );
        let parsed = decode_uk(native);
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries[1].link, values(&[65537, 10]));
        assert_eq!(encode_uk(&parsed.document).unwrap(), native);
    }

    #[test]
    fn nos_mall_warns_for_every_link_count_the_client_cannot_size() {
        let parsed = decode_uk("LINK -1\nVNUM 1\nLINK many\nLINK 0\n");

        assert_eq!(warning_rows(&parsed), [1, 1, 3, 3]);
        assert!(parsed.warnings[0].message.contains("range error"));
        assert!(parsed.warnings[2].message.contains("range error"));
        assert_eq!(parsed.document.entries[0].link, values(&[0]));
    }

    #[test]
    fn nos_mall_vnum_keeps_flag_text_and_short_rows() {
        let native = concat!(
            "VNUM\t3\t999\tTrue\tfalse\t0\tFalse\tTRUE\nEND\n",
            "VNUM\t5\t5\nEND\n",
            "VNUM\t07\t+1\nEND\n",
        );
        let parsed = decode_uk(native);

        assert!(parsed.warnings.is_empty());
        let entries = &parsed.document.entries;
        assert_eq!(
            entries[0].vnum_fields,
            [
                999.into(),
                text("True"),
                text("false"),
                0.into(),
                text("False"),
                text("TRUE")
            ]
        );
        assert_eq!(entries[1].vnum_fields, [5.into()]);
        assert_eq!(entries[2].vnum, text("07"));
        assert_eq!(
            serde_json::to_string(&entries[0].vnum_fields).unwrap(),
            r#"[999,"True","false",0,"False","TRUE"]"#
        );
        assert_eq!(encode_uk(&parsed.document).unwrap(), native);

        let empty = decode_uk("VNUM\nTITLE1 a\n");
        assert_eq!(warning_rows(&empty), [1]);
        assert_eq!(empty.document.entries[0].vnum, NosMallValue::Integer(-1));
    }

    #[test]
    fn nos_mall_matches_tags_and_terminators_in_any_case() {
        let parsed = decode_uk(concat!(
            "VNUM 1\n",
            "DSTART\nfirst\n dend \n",
            "COST 9 9 9 9 9 9\n",
            "vnum 2\n",
            "id two\n",
            "Dstart\ntext\n#note\nitem 7 7 7 7 7 7\n",
            "  DEND  \nEnd\n",
        ));

        assert!(parsed.warnings.is_empty());
        let entries = &parsed.document.entries;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].description_lines, lines(&["first"]));
        assert_eq!(entries[0].cost, values(&[9; 6]));
        assert_eq!(entries[1].vnum, NosMallValue::Integer(2));
        assert_eq!(entries[1].id.as_deref(), Some("two"));
        assert_eq!(entries[1].description_lines, lines(&["text"]));
        assert_eq!(entries[1].item, values(&[7; 6]));
    }

    #[test]
    fn nos_mall_trims_rows_before_cutting_the_tag() {
        let parsed = decode_uk("VNUM 1\n  TITLE1\tindented\n\tID\t00001\n");

        assert!(parsed.warnings.is_empty());
        let entry = &parsed.document.entries[0];
        assert_eq!(entry.title1.as_deref(), Some("indented"));
        assert_eq!(entry.id.as_deref(), Some("00001"));
        assert_eq!(
            encode_uk(&parsed.document).unwrap(),
            "VNUM\t1\nID\t00001\nTITLE1\tindented\nEND\n"
        );
    }

    #[test]
    fn nos_mall_splits_rows_at_a_tab_before_any_space() {
        let parsed = decode_uk("VNUM 1\nITEM 1\t2\nCOST\t1 2\t3\nTITLE1 Two\twords\n");

        assert_eq!(warning_rows(&parsed), [2, 4]);
        let entry = &parsed.document.entries[0];
        assert_eq!(entry.item, None);
        assert_eq!(entry.cost, Some(vec![text("1 2"), 3.into()]));
        assert_eq!(entry.title1, None);
        assert_eq!(
            encode_uk(&parsed.document).unwrap(),
            "VNUM\t1\nCOST\t1 2\t3\nEND\n"
        );
    }

    #[test]
    fn nos_mall_description_scan_reads_at_most_twenty_rows() {
        let description = (1..=20)
            .map(|row| format!("row {row}\n"))
            .collect::<String>();
        let parsed = decode_uk(&format!(
            "VNUM 1\nITEM 1 1 1 1 1 1\nDSTART\n{description}Item 9 9 9 9 9 9\nextra text\nDEND\n"
        ));

        let entry = &parsed.document.entries[0];
        assert_eq!(entry.description_lines.as_ref().unwrap().len(), 20);
        assert_eq!(entry.item, values(&[9; 6]));
        assert_eq!(warning_rows(&parsed), [2, 25]);

        let parsed = decode_uk(&format!(
            "VNUM 2\nDSTART\n{description}row 21\n  #row 22\nDEND\n"
        ));
        assert_eq!(warning_rows(&parsed), [23]);
        let entry = &parsed.document.entries[0];
        assert_eq!(entry.description_lines.as_ref().unwrap().len(), 22);
        assert_eq!(
            encode_uk(&parsed.document).unwrap(),
            format!("VNUM\t2\nDSTART\n{description}row 21\n  #row 22\nDEND\nEND\n")
        );
    }

    #[test]
    fn nos_mall_writer_rejects_text_the_client_reads_differently() {
        let entry = |description: Vec<String>| NosMallDocument {
            locale: GtdLocale::Uk,
            entries: vec![NosMallEntry {
                vnum: 1.into(),
                vnum_fields: Vec::new(),
                item: None,
                id: None,
                title1: None,
                title2: None,
                cost: None,
                link: None,
                description_lines: Some(description),
            }],
        };
        let twenty = (1..=20).map(|row| format!("row {row}")).collect::<Vec<_>>();

        for terminator in ["dend", "  DEND ", "#note"] {
            let document = entry(vec!["first".into(), terminator.into()]);
            assert!(encode_uk(&document).is_err(), "{terminator:?}");
        }
        let mut overflow = twenty.clone();
        overflow.push("ITEM 7 7 7 7 7 7".into());
        assert!(encode_uk(&entry(overflow)).is_err());
        let mut overflow = twenty.clone();
        overflow.push("plain text".into());
        assert!(encode_uk(&entry(overflow)).is_ok());
        let mut tagged = twenty;
        tagged[19] = "ITEM 7 7 7 7 7 7".into();
        assert!(encode_uk(&entry(tagged)).is_ok());

        let mut document = entry(Vec::new());
        for (title, writable) in [
            (" lead", false),
            ("\tlead", false),
            ("   ", false),
            ("trail  ", true),
            ("", true),
        ] {
            document.entries[0].title1 = Some(title.into());
            assert_eq!(encode_uk(&document).is_ok(), writable, "{title:?}");
        }
        document.entries[0].title1 = None;
        document.entries[0].id = Some(" 00001".into());
        assert!(encode_uk(&document).is_err());
        document.entries[0].id = None;

        for (fields, writable) in [
            (vec![text("a b"), 1.into()], true),
            (vec![1.into(), text("a b")], false),
            (vec![text(""), 1.into()], false),
            (vec![text(" a"), 1.into()], false),
            (vec![text("a\tb")], false),
        ] {
            document.entries[0].vnum_fields = fields.clone();
            assert_eq!(encode_uk(&document).is_ok(), writable, "{fields:?}");
        }
    }

    #[test]
    fn nos_mall_reads_fixed_width_json_documents() {
        let document: NosMallDocument = serde_json::from_str(
            r#"{"locale":"uk","entries":[{"vnum":1,"vnum_fields":[999999,0,0,0,1,1],
            "item":[0,0,1115,1115,1,1],"id":"00001","title1":"zts1e","title2":"zts2e",
            "cost":[999999,0,1,1,0,30],"link":[0,0,0,0,0,0],"description_lines":["zts3e"]}]}"#,
        )
        .unwrap();

        assert_eq!(
            encode_uk(&document).unwrap(),
            concat!(
                "VNUM\t1\t999999\t0\t0\t0\t1\t1\n",
                "ITEM\t0\t0\t1115\t1115\t1\t1\n",
                "ID\t00001\nTITLE1\tzts1e\nTITLE2\tzts2e\n",
                "COST\t999999\t0\t1\t1\t0\t30\n",
                "LINK\t0\t0\t0\t0\t0\t0\n",
                "DSTART\nzts3e\nDEND\nEND\n",
            )
        );
    }

    #[test]
    fn nos_mall_ignores_rows_before_the_first_vnum() {
        let parsed = decode_uk("# header\nITEM 1 1 1 1 1 1\nDSTART\nVNUM 9\nDEND\nVNUM 1\n");

        assert_eq!(warning_rows(&parsed), [2, 3]);
        assert_eq!(parsed.document.entries.len(), 1);
        assert_eq!(parsed.document.entries[0].vnum, NosMallValue::Integer(1));
    }

    #[test]
    fn abuse_distinguishes_zero_length_and_counted_empty() {
        let zero = decode_abuse(&[], GtdLocale::Kr, TextEncoding::EucKr).unwrap();
        assert_eq!(zero.document.payload_state, AbusePayloadState::ZeroLength);
        assert_eq!(
            encode_abuse(&zero.document, TextEncoding::EucKr).unwrap(),
            Vec::<u8>::new()
        );

        let counted = decode_abuse(
            &0_i32.to_le_bytes(),
            GtdLocale::Cz,
            TextEncoding::Windows1250,
        )
        .unwrap();
        assert_eq!(counted.document.payload_state, AbusePayloadState::Counted);
        assert!(counted.document.entries.is_empty());
        assert_eq!(
            encode_abuse(&counted.document, TextEncoding::Windows1250).unwrap(),
            0_i32.to_le_bytes()
        );
    }

    #[test]
    fn abuse_preserves_duplicates_and_falls_back_to_raw_bytes() {
        let document = AbuseDocument {
            locale: GtdLocale::Hk,
            payload_state: AbusePayloadState::Counted,
            entries: vec![
                AbuseEntry::Text {
                    text: "same".into(),
                },
                AbuseEntry::Text {
                    text: "same".into(),
                },
                AbuseEntry::Bytes {
                    bytes_base64: base64_encode(&[0x81]),
                },
            ],
        };
        let encoded = encode_abuse(&document, TextEncoding::Big5).unwrap();
        let decoded = decode_abuse(&encoded, GtdLocale::Hk, TextEncoding::Big5).unwrap();
        assert_eq!(decoded.document, document);
        assert_eq!(
            encode_abuse(&decoded.document, TextEncoding::Big5).unwrap(),
            encoded
        );
    }

    #[test]
    fn abuse_json_entries_are_readable_or_explicitly_binary() {
        let entries = vec![
            AbuseEntry::Text {
                text: "word".into(),
            },
            AbuseEntry::Bytes {
                bytes_base64: "gQ==".into(),
            },
        ];
        assert_eq!(
            serde_json::to_string(&entries).unwrap(),
            r#"[{"text":"word"},{"bytes_base64":"gQ=="}]"#
        );
    }
}
