use serde::{Deserialize, Serialize};

use super::{GtdLocale, GtdWarning, ParsedGtd, fields, is_ignored_line, values, warning};
use crate::{
    Result, TextEncoding, TextError, decode_dat_payload, decode_legacy_text, encode_dat_payload,
    encode_legacy_text,
};

/// Contents of one localized `*_nosmall.dat` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NosMallDocument {
    pub locale: GtdLocale,
    pub entries: Vec<NosMallEntry>,
}

/// One native NosMall entry
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NosMallEntry {
    pub vnum: i32,
    pub vnum_fields: [i32; 6],
    pub item: [i32; 6],
    pub id: String,
    pub title1: String,
    pub title2: String,
    pub cost: [i32; 6],
    pub link: [i32; 6],
    pub description_lines: Vec<String>,
}

/// Decode a NosMall DAT payload, retaining valid entries in order.
pub fn decode_nos_mall(
    data: &[u8],
    locale: GtdLocale,
    encoding: TextEncoding,
) -> Result<ParsedGtd<NosMallDocument>> {
    let decoded = decode_dat_payload(data)?;
    let text = decode_legacy_text(&decoded, encoding)?;
    let lines = text
        .split_terminator('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if is_ignored_line(line) || line.trim() == "~" {
            index += 1;
            continue;
        }
        if fields(line).first().copied() != Some("VNUM") {
            warnings.push(warning(index + 1, "expected VNUM entry start"));
            index += 1;
            continue;
        }

        let start = index;
        let mut end = index + 1;
        let mut in_description = false;
        while end < lines.len() {
            match lines[end].trim() {
                "DSTART" => in_description = true,
                "DEND" => in_description = false,
                _ if !in_description && fields(lines[end]).first().copied() == Some("VNUM") => {
                    break;
                }
                _ => {}
            }
            end += 1;
        }

        match parse_nos_mall_entry(&lines[start..end], start + 1, &mut warnings) {
            Some(entry) => entries.push(entry),
            None => warnings.push(warning(start + 1, "skipped incomplete NosMall entry")),
        }
        index = end;
    }

    Ok(ParsedGtd {
        document: NosMallDocument { locale, entries },
        warnings,
    })
}

fn parse_nos_mall_entry(
    lines: &[&str],
    first_row: usize,
    warnings: &mut Vec<GtdWarning>,
) -> Option<NosMallEntry> {
    let mut vnum = None;
    let mut vnum_fields = None;
    let mut item = None;
    let mut id = None;
    let mut title1 = None;
    let mut title2 = None;
    let mut cost = None;
    let mut link = None;
    let mut description_lines = Vec::new();
    let mut in_description = false;

    for (offset, line) in lines.iter().enumerate() {
        let row = first_row + offset;
        if in_description {
            if line.trim() == "DEND" {
                in_description = false;
            } else {
                description_lines.push((*line).to_owned());
            }
            continue;
        }
        if is_ignored_line(line) || line.trim() == "~" {
            continue;
        }
        let tokens = fields(line);
        let tag = tokens.first().copied().unwrap_or_default();
        match tag {
            "VNUM" => {
                if let Some(all) = parse_array::<7>(&tokens[1..]) {
                    vnum = Some(all[0]);
                    vnum_fields = all[1..].try_into().ok();
                } else {
                    warnings.push(warning(row, "VNUM must contain seven integers"));
                }
            }
            "ITEM" => parse_fixed_row::<6>(&tokens, row, "ITEM", warnings, &mut item),
            "ID" => id = Some(text_after_tag(line, "ID").to_owned()),
            "TITLE1" => title1 = Some(text_after_tag(line, "TITLE1").to_owned()),
            "TITLE2" => title2 = Some(text_after_tag(line, "TITLE2").to_owned()),
            "COST" => parse_fixed_row::<6>(&tokens, row, "COST", warnings, &mut cost),
            "LINK" => parse_fixed_row::<6>(&tokens, row, "LINK", warnings, &mut link),
            "DSTART" => in_description = true,
            "DEND" | "END" => {}
            _ => warnings.push(warning(row, format!("ignored unknown NosMall row {tag}"))),
        }
    }

    Some(NosMallEntry {
        vnum: vnum?,
        vnum_fields: vnum_fields?,
        item: item?,
        id: id?,
        title1: title1?,
        title2: title2?,
        cost: cost?,
        link: link?,
        description_lines,
    })
}

fn parse_fixed_row<const N: usize>(
    tokens: &[&str],
    row: usize,
    tag: &str,
    warnings: &mut Vec<GtdWarning>,
    target: &mut Option<[i32; N]>,
) {
    if let Some(parsed) = parse_array::<N>(&tokens[1..]) {
        *target = Some(parsed);
    } else {
        warnings.push(warning(row, format!("{tag} must contain {N} integers")));
    }
}

fn parse_array<const N: usize>(tokens: &[&str]) -> Option<[i32; N]> {
    if tokens.len() != N {
        return None;
    }
    values(tokens)?.try_into().ok()
}

fn text_after_tag<'a>(line: &'a str, tag: &str) -> &'a str {
    line.get(tag.len()..)
        .unwrap_or_default()
        .trim_start_matches([' ', '\t'])
}

/// Encode a NosMall document using canonical native framing and DAT encoding.
pub fn encode_nos_mall(document: &NosMallDocument, encoding: TextEncoding) -> Result<Vec<u8>> {
    let mut text = String::new();
    for (index, entry) in document.entries.iter().enumerate() {
        for (field, value) in [
            ("id", &entry.id),
            ("title1", &entry.title1),
            ("title2", &entry.title2),
        ] {
            if value.contains(['\r', '\n']) {
                return Err(TextError::InvalidGtdDocument {
                    message: format!("NosMall entry {index} {field} contains a line break"),
                });
            }
        }
        for line in &entry.description_lines {
            if line.contains(['\r', '\n']) || line == "DEND" {
                return Err(TextError::InvalidGtdDocument {
                    message: format!(
                        "NosMall entry {index} has an unrepresentable description line"
                    ),
                });
            }
        }
        text.push_str("VNUM");
        for value in std::iter::once(&entry.vnum).chain(entry.vnum_fields.iter()) {
            text.push('\t');
            text.push_str(&value.to_string());
        }
        text.push('\n');
        push_numeric_row(&mut text, "ITEM", &entry.item);
        push_raw_row(&mut text, "ID", &entry.id);
        push_raw_row(&mut text, "TITLE1", &entry.title1);
        push_raw_row(&mut text, "TITLE2", &entry.title2);
        push_numeric_row(&mut text, "COST", &entry.cost);
        push_numeric_row(&mut text, "LINK", &entry.link);
        text.push_str("DSTART\n");
        for line in &entry.description_lines {
            text.push_str(line);
            text.push('\n');
        }
        text.push_str("DEND\nEND\n");
    }
    encode_dat_payload(&encode_legacy_text(&text, encoding)?)
}

fn push_numeric_row<const N: usize>(out: &mut String, tag: &str, row: &[i32; N]) {
    out.push_str(tag);
    for value in row {
        out.push('\t');
        out.push_str(&value.to_string());
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
        let payload = encode_dat_payload(native.as_bytes()).unwrap();
        let parsed = decode_nos_mall(&payload, GtdLocale::Uk, TextEncoding::Windows1252).unwrap();
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries[0].id, "00001  ");
        assert_eq!(
            parsed.document.entries[0].description_lines,
            ["zts3e", "", "literal trailing  "]
        );
        let encoded = encode_nos_mall(&parsed.document, TextEncoding::Windows1252).unwrap();
        assert_eq!(decode_dat_payload(&encoded).unwrap(), native.as_bytes());
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
        let payload = encode_dat_payload(native.as_bytes()).unwrap();
        let parsed = decode_nos_mall(&payload, GtdLocale::Uk, TextEncoding::Windows1252).unwrap();

        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.document.entries.len(), 2);
        assert_eq!(parsed.document.entries[0].id, "one");
        assert_eq!(parsed.document.entries[1].description_lines, ["second"]);
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
