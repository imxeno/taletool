use anyhow::{Context, Result, bail};

const ARCHIVE_HEADER_LEN: usize = 0x15;
const ARCHIVE_ENTRY_LEN: usize = 8;
const ENTRY_HEADER_LEN: usize = 0x0d;
const UPDATE_PREFIX_LEN: usize = ARCHIVE_HEADER_LEN + 4;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveEntry {
    file_id: i32,
    data_offset: u32,
}

#[derive(Debug, Clone)]
struct ArchiveView<'a> {
    bytes: &'a [u8],
    entries: Vec<ArchiveEntry>,
    direct_index: bool,
}

impl<'a> ArchiveView<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < ARCHIVE_HEADER_LEN {
            bail!("NOS archive is shorter than its header");
        }

        let count = read_i32_at(bytes, 0x10, "archive entry count")?;
        if count < 0 {
            bail!("NOS archive has negative entry count: {count}");
        }
        let count = count as usize;
        let table_end = archive_table_end(count)?;
        if table_end > bytes.len() {
            bail!(
                "NOS archive index table ends at {table_end}, beyond file size {}",
                bytes.len()
            );
        }

        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let offset = ARCHIVE_HEADER_LEN + index * ARCHIVE_ENTRY_LEN;
            let file_id = read_i32_at(bytes, offset, "archive entry file id")?;
            let data_offset = read_u32_at(bytes, offset + 4, "archive entry data offset")?;
            let data_offset_usize = data_offset as usize;
            if data_offset_usize < table_end {
                bail!("NOS archive entry {index} points into the index table: {data_offset_usize}");
            }
            let entry_end = checked_add(data_offset_usize, ENTRY_HEADER_LEN, "entry header end")?;
            if entry_end > bytes.len() {
                bail!("NOS archive entry {index} header ends beyond file size");
            }
            let data_len = stored_payload_len(&bytes[data_offset_usize..entry_end])?;
            let entry_end = checked_add(entry_end, data_len, "entry payload end")?;
            if entry_end > bytes.len() {
                bail!("NOS archive entry {index} payload ends beyond file size");
            }
            entries.push(ArchiveEntry {
                file_id,
                data_offset,
            });
        }

        Ok(Self {
            bytes,
            entries,
            direct_index: bytes[0x14] != 0,
        })
    }

    fn entry_count(&self) -> usize {
        self.entries.len()
    }

    fn entry_id(&self, index: usize) -> Result<i32> {
        self.entries
            .get(index)
            .map(|entry| entry.file_id)
            .with_context(|| format!("NOS archive entry index {index} is out of bounds"))
    }

    fn find_entry_index(&self, file_id: i32) -> Option<usize> {
        if self.direct_index && file_id >= 0 {
            let index = file_id as usize;
            return (index < self.entries.len()).then_some(index);
        }

        self.entries
            .binary_search_by_key(&file_id, |entry| entry.file_id)
            .ok()
    }

    fn entry_bytes(&self, index: usize) -> Result<&'a [u8]> {
        let entry = self
            .entries
            .get(index)
            .with_context(|| format!("NOS archive entry index {index} is out of bounds"))?;
        let start = entry.data_offset as usize;
        let header_end = checked_add(start, ENTRY_HEADER_LEN, "entry header end")?;
        let payload_len = stored_payload_len(&self.bytes[start..header_end])?;
        let end = checked_add(header_end, payload_len, "entry payload end")?;
        self.bytes
            .get(start..end)
            .with_context(|| format!("NOS archive entry {index} is outside the file"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveUpdate {
    header: Vec<u8>,
    output_count: usize,
    records: Vec<UpdateRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateRecord {
    Skip {
        target_id: i32,
        source_id: i32,
        source_index: i32,
    },
    Inline {
        tag: u8,
        target_id: i32,
        bytes: Vec<u8>,
    },
    Copy {
        tag: u8,
        target_id: i32,
        source_id: i32,
        source_index: i32,
    },
}

impl ArchiveUpdate {
    fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < UPDATE_PREFIX_LEN {
            bail!("NOS archive update is shorter than its header");
        }

        let output_count = read_i32_at(bytes, 0x10, "update output entry count")?;
        if output_count < 0 {
            bail!("NOS archive update has negative output entry count: {output_count}");
        }
        let output_count = output_count as usize;

        let record_count = read_i32_at(bytes, ARCHIVE_HEADER_LEN, "update record count")?;
        if record_count < 0 {
            bail!("NOS archive update has negative record count: {record_count}");
        }
        let record_count = record_count as usize;

        let mut pos = UPDATE_PREFIX_LEN;
        let mut records = Vec::with_capacity(record_count);
        let mut output_record_count = 0usize;

        for index in 0..record_count {
            let record_start = pos;
            let header = bytes
                .get(pos..pos + 9)
                .with_context(|| format!("update record {index} is truncated"))?;
            let tag = header[0];
            let first = read_i32_at(header, 1, "update record first field")?;
            let second = read_i32_at(header, 5, "update record second field")?;
            pos += 9;

            match tag {
                0 => {
                    let source_index = read_i32_at(bytes, pos, "update skip source index")?;
                    pos += 4;
                    records.push(UpdateRecord::Skip {
                        target_id: first,
                        source_id: second,
                        source_index,
                    });
                }
                1 | 2 => {
                    if second < 0 {
                        bail!("update record {index} has negative inline size: {second}");
                    }
                    let inline_size = second as usize;
                    let end = checked_add(pos, inline_size, "update inline record end")?;
                    let inline = bytes
                        .get(pos..end)
                        .with_context(|| format!("update inline record {index} is truncated"))?;
                    validate_entry_bytes(inline).with_context(|| {
                        format!("update inline record {index} does not contain a complete entry")
                    })?;
                    pos = end;
                    output_record_count += 1;
                    records.push(UpdateRecord::Inline {
                        tag,
                        target_id: first,
                        bytes: inline.to_vec(),
                    });
                }
                3..=5 => {
                    let source_index = read_i32_at(bytes, pos, "update copy source index")?;
                    pos += 4;
                    output_record_count += 1;
                    records.push(UpdateRecord::Copy {
                        tag,
                        target_id: first,
                        source_id: second,
                        source_index,
                    });
                }
                _ => bail!("update record {index} has unknown tag {tag} at offset {record_start}"),
            }
        }

        if pos != bytes.len() {
            bail!(
                "NOS archive update has {} trailing bytes after its records",
                bytes.len() - pos
            );
        }
        if output_record_count != output_count {
            bail!(
                "NOS archive update output count mismatch: header={output_count}, records={output_record_count}"
            );
        }

        Ok(Self {
            header: bytes[..ARCHIVE_HEADER_LEN].to_vec(),
            output_count,
            records,
        })
    }
}

pub fn apply_binary_nos_archive_update(base_bytes: &[u8], update_bytes: &[u8]) -> Result<Vec<u8>> {
    let base = ArchiveView::parse(base_bytes).context("parsing base binary NOS archive")?;
    let update = ArchiveUpdate::parse(update_bytes).context("parsing binary NOS archive update")?;
    let table_end = archive_table_end(update.output_count)?;

    let mut out = vec![0; table_end];
    out[..ARCHIVE_HEADER_LEN].copy_from_slice(&update.header);

    let mut output_index = 0usize;
    let mut source_cursor = 0usize;

    for record in &update.records {
        match record {
            UpdateRecord::Skip { .. } => {}
            UpdateRecord::Inline {
                tag,
                target_id,
                bytes,
            } => {
                if *tag == 2 {
                    advance_source_cursor(&base, &mut source_cursor, *target_id)?;
                }
                append_entry(&mut out, output_index, *target_id, bytes)?;
                output_index += 1;
            }
            UpdateRecord::Copy {
                tag,
                target_id,
                source_id,
                source_index,
            } => {
                let index = resolve_copy_index(&base, *source_id, *source_index, source_cursor)?;
                if matches!(*tag, 4 | 5) {
                    advance_source_cursor(&base, &mut source_cursor, *target_id)?;
                }
                let bytes = base.entry_bytes(index)?;
                append_entry(&mut out, output_index, *target_id, bytes)?;
                output_index += 1;
            }
        }
    }

    if output_index != update.output_count {
        bail!(
            "NOS archive update wrote {output_index} entries, expected {}",
            update.output_count
        );
    }
    ArchiveView::parse(&out).context("validating reconstructed binary NOS archive")?;
    Ok(out)
}

pub fn split_archive_parent_path(target_path: &str) -> Option<String> {
    let dot = target_path.rfind('.')?;
    let (stem, ext) = target_path.split_at(dot);
    if !ext.eq_ignore_ascii_case(".nos") || stem.len() < 2 {
        return None;
    }

    let stem_bytes = stem.as_bytes();
    let shard = &stem_bytes[stem_bytes.len() - 2..];
    if !shard.iter().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }

    Some(format!("{}{}", &stem[..stem.len() - 2], ext))
}

pub fn rebuild_binary_nos_archive_from_entries_like(
    source_bytes: &[u8],
    entries: &[(i32, &[u8])],
) -> Result<Vec<u8>> {
    if source_bytes.len() < ARCHIVE_HEADER_LEN {
        bail!("binary NOS archive is shorter than its header");
    }

    let table_end = archive_table_end(entries.len())?;
    let mut out = vec![0; table_end];
    out[..ARCHIVE_HEADER_LEN].copy_from_slice(&source_bytes[..ARCHIVE_HEADER_LEN]);
    out[0x10..0x14].copy_from_slice(&(entries.len() as i32).to_le_bytes());

    for (index, (file_id, entry)) in entries.iter().enumerate() {
        append_entry(&mut out, index, *file_id, entry)?;
    }

    ArchiveView::parse(&out).context("validating rebuilt binary NOS archive")?;
    Ok(out)
}

pub fn binary_nos_archive_entry_ids(bytes: &[u8]) -> Result<Vec<i32>> {
    let archive = ArchiveView::parse(bytes)?;
    (0..archive.entry_count())
        .map(|index| archive.entry_id(index))
        .collect()
}

fn append_entry(out: &mut Vec<u8>, output_index: usize, file_id: i32, entry: &[u8]) -> Result<()> {
    validate_entry_bytes(entry)?;
    let table_offset = ARCHIVE_HEADER_LEN + output_index * ARCHIVE_ENTRY_LEN;
    let data_offset =
        u32::try_from(out.len()).context("reconstructed NOS archive exceeds 4 GiB")?;
    out[table_offset..table_offset + 4].copy_from_slice(&file_id.to_le_bytes());
    out[table_offset + 4..table_offset + 8].copy_from_slice(&data_offset.to_le_bytes());
    out.extend_from_slice(entry);
    Ok(())
}

fn resolve_copy_index(
    base: &ArchiveView<'_>,
    source_id: i32,
    expected_index: i32,
    source_cursor: usize,
) -> Result<usize> {
    if let Some(index) = base.find_entry_index(source_id) {
        return Ok(index);
    }

    if expected_index >= 0 {
        let index = expected_index as usize;
        if index < base.entry_count() {
            return Ok(index);
        }
    }

    if source_cursor < base.entry_count() {
        return Ok(source_cursor);
    }

    base.entry_count()
        .checked_sub(1)
        .context("cannot copy from an empty NOS archive")
}

fn advance_source_cursor(
    base: &ArchiveView<'_>,
    source_cursor: &mut usize,
    target_id: i32,
) -> Result<()> {
    while *source_cursor + 1 < base.entry_count() && base.entry_id(*source_cursor)? <= target_id {
        *source_cursor += 1;
    }
    Ok(())
}

fn archive_table_end(count: usize) -> Result<usize> {
    checked_add(
        ARCHIVE_HEADER_LEN,
        count
            .checked_mul(ARCHIVE_ENTRY_LEN)
            .context("NOS archive index table size overflow")?,
        "NOS archive index table end",
    )
}

fn validate_entry_bytes(bytes: &[u8]) -> Result<()> {
    if bytes.len() < ENTRY_HEADER_LEN {
        bail!("NOS archive entry is shorter than its 13-byte header");
    }
    let payload_len = stored_payload_len(&bytes[..ENTRY_HEADER_LEN])?;
    let expected_len = checked_add(ENTRY_HEADER_LEN, payload_len, "NOS archive entry length")?;
    if bytes.len() != expected_len {
        bail!(
            "NOS archive entry length mismatch: header expects {expected_len}, got {}",
            bytes.len()
        );
    }
    Ok(())
}

fn stored_payload_len(header: &[u8]) -> Result<usize> {
    if header.len() < ENTRY_HEADER_LEN {
        bail!("NOS archive entry header is truncated");
    }
    let offset = if header[12] != 0 { 8 } else { 4 };
    let len = read_u32_at(header, offset, "NOS archive stored payload size")?;
    Ok(len as usize)
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

    fn entry(stored_len_offset: usize, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0x12, 0x07, 0x04, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        out[stored_len_offset..stored_len_offset + 4]
            .copy_from_slice(&(payload.len() as u32).to_le_bytes());
        if stored_len_offset == 8 {
            out[12] = 1;
        }
        out.extend_from_slice(payload);
        out
    }

    fn archive(ids: &[i32], entries: &[Vec<u8>]) -> Vec<u8> {
        assert_eq!(ids.len(), entries.len());
        let mut out = vec![0; ARCHIVE_HEADER_LEN + ids.len() * ARCHIVE_ENTRY_LEN];
        out[..8].copy_from_slice(b"NT Data ");
        out[8..10].copy_from_slice(b"00");
        out[0x10..0x14].copy_from_slice(&(ids.len() as i32).to_le_bytes());
        for (index, (id, entry)) in ids.iter().zip(entries).enumerate() {
            let table_offset = ARCHIVE_HEADER_LEN + index * ARCHIVE_ENTRY_LEN;
            let data_offset = out.len() as u32;
            out[table_offset..table_offset + 4].copy_from_slice(&id.to_le_bytes());
            out[table_offset + 4..table_offset + 8].copy_from_slice(&data_offset.to_le_bytes());
            out.extend_from_slice(entry);
        }
        out
    }

    fn update(output_count: i32, records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0; UPDATE_PREFIX_LEN];
        out[..8].copy_from_slice(b"NT Data ");
        out[8..10].copy_from_slice(b"00");
        out[0x10..0x14].copy_from_slice(&output_count.to_le_bytes());
        out[ARCHIVE_HEADER_LEN..UPDATE_PREFIX_LEN]
            .copy_from_slice(&(records.len() as i32).to_le_bytes());
        for record in records {
            out.extend_from_slice(record);
        }
        out
    }

    fn inline_record(tag: u8, target_id: i32, entry: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        out.extend_from_slice(&target_id.to_le_bytes());
        out.extend_from_slice(&(entry.len() as i32).to_le_bytes());
        out.extend_from_slice(entry);
        out
    }

    fn copy_record(tag: u8, target_id: i32, source_id: i32, source_index: i32) -> Vec<u8> {
        let mut out = vec![tag];
        out.extend_from_slice(&target_id.to_le_bytes());
        out.extend_from_slice(&source_id.to_le_bytes());
        out.extend_from_slice(&source_index.to_le_bytes());
        out
    }

    #[test]
    fn parses_valid_archive() {
        let first = entry(8, b"abc");
        let second = entry(4, b"defg");
        let bytes = archive(&[10, 20], &[first, second]);
        let parsed = ArchiveView::parse(&bytes).unwrap();
        assert_eq!(parsed.entry_count(), 2);
        assert_eq!(parsed.entry_id(1).unwrap(), 20);
        assert_eq!(parsed.find_entry_index(20), Some(1));
    }

    #[test]
    fn rejects_truncated_archive_entry() {
        let first = entry(8, b"abc");
        let mut bytes = archive(&[10], &[first]);
        bytes.pop();
        assert!(ArchiveView::parse(&bytes).is_err());
    }

    #[test]
    fn parses_update_records_without_target_path_context() {
        let inserted = entry(8, b"new");
        let bytes = update(
            2,
            &[
                inline_record(1, 5, &inserted),
                copy_record(5, 10, 10, 0),
                copy_record(0, 0, 0, 0),
            ],
        );
        let parsed = ArchiveUpdate::parse(&bytes).unwrap();
        assert_eq!(parsed.output_count, 2);
        assert_eq!(parsed.records.len(), 3);
    }

    #[test]
    fn rejects_update_with_output_count_mismatch() {
        let inserted = entry(8, b"new");
        let bytes = update(2, &[inline_record(1, 5, &inserted)]);
        assert!(ArchiveUpdate::parse(&bytes).is_err());
    }

    #[test]
    fn applies_insert_replace_copy_and_skip_records() {
        let existing_a = entry(8, b"existing-a");
        let existing_b = entry(8, b"existing-b");
        let existing_c = entry(4, b"existing-c");
        let base = archive(
            &[10, 20, 30],
            &[existing_a.clone(), existing_b.clone(), existing_c.clone()],
        );
        let inserted = entry(8, b"inserted");
        let replaced = entry(8, b"replaced");
        let update = update(
            4,
            &[
                inline_record(1, 5, &inserted),
                inline_record(2, 10, &replaced),
                copy_record(3, 25, 20, 1),
                copy_record(0, 99, 99, 99),
                copy_record(5, 30, 30, 2),
            ],
        );

        let out = apply_binary_nos_archive_update(&base, &update).unwrap();
        let parsed = ArchiveView::parse(&out).unwrap();
        assert_eq!(parsed.entry_count(), 4);
        assert_eq!(parsed.entry_id(0).unwrap(), 5);
        assert_eq!(parsed.entry_id(1).unwrap(), 10);
        assert_eq!(parsed.entry_id(2).unwrap(), 25);
        assert_eq!(parsed.entry_id(3).unwrap(), 30);
        assert_eq!(parsed.entry_bytes(0).unwrap(), inserted.as_slice());
        assert_eq!(parsed.entry_bytes(1).unwrap(), replaced.as_slice());
        assert_eq!(parsed.entry_bytes(2).unwrap(), existing_b.as_slice());
        assert_eq!(parsed.entry_bytes(3).unwrap(), existing_c.as_slice());
    }

    #[test]
    fn tag_four_copies_with_expected_index_fallback() {
        let existing_a = entry(8, b"existing-a");
        let base = archive(&[10], std::slice::from_ref(&existing_a));
        let update = update(1, &[copy_record(4, 15, 999, 0)]);

        let out = apply_binary_nos_archive_update(&base, &update).unwrap();
        let parsed = ArchiveView::parse(&out).unwrap();
        assert_eq!(parsed.entry_id(0).unwrap(), 15);
        assert_eq!(parsed.entry_bytes(0).unwrap(), existing_a.as_slice());
    }

    #[test]
    fn derives_split_archive_parent_paths() {
        assert_eq!(
            split_archive_parent_path("NostaleData/NSppData08.NOS").as_deref(),
            Some("NostaleData/NSppData.NOS")
        );
        assert_eq!(
            split_archive_parent_path("NostaleData/NSmpData0f.nos").as_deref(),
            Some("NostaleData/NSmpData.nos")
        );
        assert_eq!(split_archive_parent_path("NostaleData/NSppData.NOS"), None);
        assert_eq!(
            split_archive_parent_path("NostaleData/NSppDataZZ.NOS"),
            None
        );
        assert_eq!(
            split_archive_parent_path("NostaleData/NSppData08.dat"),
            None
        );
    }
}
