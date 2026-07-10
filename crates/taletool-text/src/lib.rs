use serde::{Deserialize, Serialize};
use taletool_core::ByteReader;
use thiserror::Error;

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
}

pub type Result<T> = std::result::Result<T, TextError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextPayloadKind {
    Dat,
    List,
    Raw,
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

    fn push_list_line(out: &mut Vec<u8>, line: &[u8]) {
        out.extend_from_slice(&(line.len() as i32).to_le_bytes());
        out.extend(line.iter().map(|byte| byte ^ 0x01));
    }
}
