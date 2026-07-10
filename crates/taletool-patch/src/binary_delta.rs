use anyhow::{Context, Result, bail};

const DELTA_RECORD_LEN: usize = 12;

pub fn apply_binary_delta(base_bytes: &[u8], delta_bytes: &[u8]) -> Result<Vec<u8>> {
    let mut reader = DeltaReader::new(delta_bytes);
    let mut base_offset = 0usize;
    let mut output = Vec::new();

    loop {
        let source_len = reader.read_u32("source chunk length")? as usize;
        let source_end = checked_add(base_offset, source_len, "source chunk end")?;
        let source = base_bytes.get(base_offset..source_end).with_context(|| {
            format!(
                "binary delta source chunk [{base_offset}..{source_end}] exceeds base size {}",
                base_bytes.len()
            )
        })?;
        base_offset = source_end;

        match reader.read_u8("delta operation tag")? {
            0 => {
                let literal_len = reader.read_u32("literal length")? as usize;
                if literal_len == 0 {
                    if !reader.is_empty() {
                        bail!("binary delta has trailing bytes after terminator");
                    }
                    return Ok(output);
                }

                let expected_crc = reader.read_u32("literal crc")?;
                let literal = reader.read_slice(literal_len, "literal bytes")?;
                verify_crc(literal, expected_crc, "literal chunk")?;
                output.extend_from_slice(literal);
            }
            1 => {
                let expected_output_crc = reader.read_u32("patched chunk crc")?;
                let expected_source_crc = reader.read_u32("source chunk crc")?;
                verify_crc(source, expected_source_crc, "source chunk")?;

                let chunk = rebuild_delta_chunk(source, &mut reader)?;
                verify_crc(&chunk, expected_output_crc, "patched chunk")?;
                output.extend_from_slice(&chunk);
            }
            tag => bail!("binary delta has unknown operation tag {tag}"),
        }
    }
}

fn rebuild_delta_chunk(source: &[u8], reader: &mut DeltaReader<'_>) -> Result<Vec<u8>> {
    let literal_len = reader.read_u32("delta literal section length")? as usize;
    let table_len = reader.read_u32("delta table section length")? as usize;
    if table_len % DELTA_RECORD_LEN != 0 {
        bail!("binary delta table length {table_len} is not divisible by {DELTA_RECORD_LEN}");
    }

    let literal = reader.read_slice(literal_len, "delta literal section")?;
    let table = reader.read_slice(table_len, "delta table section")?;
    let mut literal_offset = 0usize;
    let mut output_pos = 1usize;
    let mut output = Vec::new();

    for (index, record) in table.chunks_exact(DELTA_RECORD_LEN).enumerate() {
        let copy_len = read_u16_at(record, 0, "copy length")? as usize;
        let source_pos = read_u32_at(record, 4, "source position")? as usize;
        let target_pos = read_u32_at(record, 8, "target position")? as usize;
        if target_pos < output_pos {
            bail!(
                "binary delta record {index} target position {target_pos} is before current output position {output_pos}"
            );
        }

        let literal_gap = target_pos - output_pos;
        if literal_gap > 0 {
            let literal_end = checked_add(literal_offset, literal_gap, "literal gap end")?;
            output.extend_from_slice(literal.get(literal_offset..literal_end).with_context(
                || {
                    format!(
                        "binary delta record {index} needs literal bytes [{literal_offset}..{literal_end}], but section has {}",
                        literal.len()
                    )
                },
            )?);
            literal_offset = literal_end;
            output_pos = checked_add(output_pos, literal_gap, "output position after literal gap")?;
        }

        if copy_len > 0 {
            if source_pos == 0 {
                bail!("binary delta record {index} has zero source position for non-empty copy");
            }
            let source_offset = source_pos - 1;
            let source_end = checked_add(source_offset, copy_len, "source copy end")?;
            output.extend_from_slice(source.get(source_offset..source_end).with_context(|| {
                format!(
                    "binary delta record {index} needs source bytes [{source_offset}..{source_end}], but source has {}",
                    source.len()
                )
            })?);
            output_pos = checked_add(output_pos, copy_len, "output position after source copy")?;
        }
    }

    if literal_offset != literal.len() {
        bail!(
            "binary delta left {} unused literal bytes",
            literal.len() - literal_offset
        );
    }

    Ok(output)
}

fn verify_crc(bytes: &[u8], expected: u32, label: &str) -> Result<()> {
    let actual = crc32fast::hash(bytes);
    if actual != expected {
        bail!("{label} CRC mismatch: expected {expected:08x}, got {actual:08x}");
    }
    Ok(())
}

fn checked_add(left: usize, right: usize, label: &str) -> Result<usize> {
    left.checked_add(right)
        .with_context(|| format!("{label} overflow"))
}

fn read_u16_at(bytes: &[u8], offset: usize, label: &str) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
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

struct DeltaReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> DeltaReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn read_u8(&mut self, label: &str) -> Result<u8> {
        let value = *self
            .bytes
            .get(self.offset)
            .with_context(|| format!("missing {label}"))?;
        self.offset += 1;
        Ok(value)
    }

    fn read_u32(&mut self, label: &str) -> Result<u32> {
        let value = read_u32_at(self.bytes, self.offset, label)?;
        self.offset += 4;
        Ok(value)
    }

    fn read_slice(&mut self, len: usize, label: &str) -> Result<&'a [u8]> {
        let end = checked_add(self.offset, len, label)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .with_context(|| format!("{label} extends beyond delta size {}", self.bytes.len()))?;
        self.offset = end;
        Ok(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn finish(out: &mut Vec<u8>) {
        push_u32(out, 0);
        out.push(0);
        push_u32(out, 0);
    }

    #[test]
    fn applies_literal_records() {
        let literal = b"abc";
        let mut delta = Vec::new();
        push_u32(&mut delta, 0);
        delta.push(0);
        push_u32(&mut delta, literal.len() as u32);
        push_u32(&mut delta, crc32fast::hash(literal));
        delta.extend_from_slice(literal);
        finish(&mut delta);

        assert_eq!(apply_binary_delta(b"", &delta).unwrap(), literal);
    }

    #[test]
    fn applies_copy_records_with_literal_gaps() {
        let base = b"abcdef";
        let literal = b"XY";
        let expected = b"abXYef";
        let mut table = Vec::new();
        push_u16(&mut table, 2);
        push_u16(&mut table, 0);
        push_u32(&mut table, 1);
        push_u32(&mut table, 1);
        push_u16(&mut table, 2);
        push_u16(&mut table, 0);
        push_u32(&mut table, 5);
        push_u32(&mut table, 5);

        let mut delta = Vec::new();
        push_u32(&mut delta, base.len() as u32);
        delta.push(1);
        push_u32(&mut delta, crc32fast::hash(expected));
        push_u32(&mut delta, crc32fast::hash(base));
        push_u32(&mut delta, literal.len() as u32);
        push_u32(&mut delta, table.len() as u32);
        delta.extend_from_slice(literal);
        delta.extend_from_slice(&table);
        finish(&mut delta);

        assert_eq!(apply_binary_delta(base, &delta).unwrap(), expected);
    }

    #[test]
    fn rejects_bad_source_crc() {
        let base = b"abcdef";
        let expected = b"abcdef";
        let mut table = Vec::new();
        push_u16(&mut table, 6);
        push_u16(&mut table, 0);
        push_u32(&mut table, 1);
        push_u32(&mut table, 1);

        let mut delta = Vec::new();
        push_u32(&mut delta, base.len() as u32);
        delta.push(1);
        push_u32(&mut delta, crc32fast::hash(expected));
        push_u32(&mut delta, 0);
        push_u32(&mut delta, 0);
        push_u32(&mut delta, table.len() as u32);
        delta.extend_from_slice(&table);
        finish(&mut delta);

        let err = apply_binary_delta(base, &delta).unwrap_err().to_string();
        assert!(err.contains("source chunk CRC mismatch"));
    }
}
