use anyhow::{Context, Result};

use crate::binary_nos_update::{
    binary_nos_archive_entry_ids, rebuild_binary_nos_archive_from_entries_like,
};

pub const EXTRACT_UI_EFF_TARGET_PATH: &str = "NostaleData/ExtractUIEff.dat";
pub const EXTRACT_UI_EFF_SHA1: &str = "8db83a801a27308d6306121a556918cd223c7752";

pub const NSTG_DATA_PATH: &str = "NostaleData/NStgData.NOS";
pub const NSTGE_DATA_PATH: &str = "NostaleData/NStgeData.NOS";
pub const NSTP_DATA_PATH: &str = "NostaleData/NStpData.NOS";
pub const NSTPE_DATA_PATH: &str = "NostaleData/NStpeData.NOS";
pub const NSTPU_DATA_PATH: &str = "NostaleData/NStpuData.NOS";

const UI_EFF_MIN: i32 = 0x4f000000;
const UI_EFF_MAX: i32 = 0x4fffffff;
const UI_EFF_ALT_MIN: i32 = 0x5f000000;
const UI_EFF_ALT_MAX: i32 = 0x5fffffff;

#[derive(Debug, Clone, Copy, Default)]
pub struct ExtractUiEffInput<'a> {
    pub nstg_data: &'a [u8],
    pub nstp_data: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractUiEffOutput {
    pub files: Vec<ExtractUiEffFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractUiEffFile {
    pub path: &'static str,
    pub bytes: Vec<u8>,
}

pub fn apply_extract_ui_eff(input: ExtractUiEffInput<'_>) -> Result<ExtractUiEffOutput> {
    let mut files = Vec::new();
    files.extend(
        split_one_range(
            input.nstg_data,
            NSTG_DATA_PATH,
            NSTGE_DATA_PATH,
            UI_EFF_MIN,
            UI_EFF_MAX,
        )
        .context("splitting NStgData.NOS")?,
    );
    files.extend(
        split_two_ranges(
            input.nstp_data,
            NSTP_DATA_PATH,
            NSTPE_DATA_PATH,
            UI_EFF_MIN,
            UI_EFF_MAX,
            NSTPU_DATA_PATH,
            UI_EFF_ALT_MIN,
            UI_EFF_ALT_MAX,
        )
        .context("splitting NStpData.NOS")?,
    );
    Ok(ExtractUiEffOutput { files })
}

fn split_one_range(
    source: &[u8],
    source_path: &'static str,
    selected_path: &'static str,
    min_id: i32,
    max_id: i32,
) -> Result<Vec<ExtractUiEffFile>> {
    let entries = raw_entries(source)?;
    let mut remainder = Vec::new();
    let mut selected = Vec::new();
    for (file_id, entry) in entries {
        if in_range(file_id, min_id, max_id) {
            selected.push((file_id, entry));
        } else {
            remainder.push((file_id, entry));
        }
    }

    if selected.is_empty() {
        return Ok(Vec::new());
    }

    Ok(vec![
        ExtractUiEffFile {
            path: source_path,
            bytes: rebuild_binary_nos_archive_from_entries_like(
                source,
                &borrow_entries(&remainder),
            )?,
        },
        ExtractUiEffFile {
            path: selected_path,
            bytes: rebuild_binary_nos_archive_from_entries_like(
                source,
                &borrow_entries(&selected),
            )?,
        },
    ])
}

#[allow(clippy::too_many_arguments)]
fn split_two_ranges(
    source: &[u8],
    source_path: &'static str,
    first_path: &'static str,
    first_min_id: i32,
    first_max_id: i32,
    second_path: &'static str,
    second_min_id: i32,
    second_max_id: i32,
) -> Result<Vec<ExtractUiEffFile>> {
    let entries = raw_entries(source)?;
    let mut remainder = Vec::new();
    let mut first = Vec::new();
    let mut second = Vec::new();
    for (file_id, entry) in entries {
        if in_range(file_id, first_min_id, first_max_id) {
            first.push((file_id, entry));
        } else if in_range(file_id, second_min_id, second_max_id) {
            second.push((file_id, entry));
        } else {
            remainder.push((file_id, entry));
        }
    }

    if first.is_empty() && second.is_empty() {
        return Ok(Vec::new());
    }

    let mut files = vec![ExtractUiEffFile {
        path: source_path,
        bytes: rebuild_binary_nos_archive_from_entries_like(source, &borrow_entries(&remainder))?,
    }];
    if !first.is_empty() {
        files.push(ExtractUiEffFile {
            path: first_path,
            bytes: rebuild_binary_nos_archive_from_entries_like(source, &borrow_entries(&first))?,
        });
    }
    if !second.is_empty() {
        files.push(ExtractUiEffFile {
            path: second_path,
            bytes: rebuild_binary_nos_archive_from_entries_like(source, &borrow_entries(&second))?,
        });
    }
    Ok(files)
}

fn raw_entries(source: &[u8]) -> Result<Vec<(i32, Vec<u8>)>> {
    let ids = binary_nos_archive_entry_ids(source)?;
    let mut entries = Vec::with_capacity(ids.len());
    for (index, file_id) in ids.into_iter().enumerate() {
        entries.push((file_id, raw_entry_bytes(source, index)?));
    }
    Ok(entries)
}

fn raw_entry_bytes(source: &[u8], index: usize) -> Result<Vec<u8>> {
    const ARCHIVE_HEADER_LEN: usize = 0x15;
    const ARCHIVE_ENTRY_LEN: usize = 8;
    const ENTRY_HEADER_LEN: usize = 0x0d;

    let table_offset = ARCHIVE_HEADER_LEN + index * ARCHIVE_ENTRY_LEN;
    let data_offset = u32::from_le_bytes(
        source
            .get(table_offset + 4..table_offset + 8)
            .context("archive entry data offset is missing")?
            .try_into()
            .expect("slice length checked"),
    ) as usize;
    let header = source
        .get(data_offset..data_offset + ENTRY_HEADER_LEN)
        .context("archive entry header is truncated")?;
    let size_offset = if header[12] != 0 { 8 } else { 4 };
    let payload_size = u32::from_le_bytes(
        header
            .get(size_offset..size_offset + 4)
            .context("archive entry payload size is missing")?
            .try_into()
            .expect("slice length checked"),
    ) as usize;
    let end = data_offset
        .checked_add(ENTRY_HEADER_LEN)
        .and_then(|value| value.checked_add(payload_size))
        .context("archive entry end overflow")?;
    Ok(source
        .get(data_offset..end)
        .context("archive entry bytes are outside source archive")?
        .to_vec())
}

fn borrow_entries(entries: &[(i32, Vec<u8>)]) -> Vec<(i32, &[u8])> {
    entries
        .iter()
        .map(|(file_id, bytes)| (*file_id, bytes.as_slice()))
        .collect()
}

fn in_range(file_id: i32, min_id: i32, max_id: i32) -> bool {
    (min_id..=max_id).contains(&file_id)
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
        let mut out = vec![0; 0x15 + ids.len() * 8];
        out[..8].copy_from_slice(b"NT Data ");
        out[8..10].copy_from_slice(b"10");
        out[0x10..0x14].copy_from_slice(&(ids.len() as i32).to_le_bytes());
        for (index, (id, entry)) in ids.iter().zip(entries).enumerate() {
            let table_offset = 0x15 + index * 8;
            let data_offset = out.len() as u32;
            out[table_offset..table_offset + 4].copy_from_slice(&id.to_le_bytes());
            out[table_offset + 4..table_offset + 8].copy_from_slice(&data_offset.to_le_bytes());
            out.extend_from_slice(entry);
        }
        out
    }

    fn ids(bytes: &[u8]) -> Vec<i32> {
        binary_nos_archive_entry_ids(bytes).unwrap()
    }

    fn entry_payloads(bytes: &[u8]) -> Vec<Vec<u8>> {
        let entries = raw_entries(bytes).unwrap();
        entries
            .into_iter()
            .map(|(_, entry)| entry[0x0d..].to_vec())
            .collect()
    }

    fn file<'a>(output: &'a ExtractUiEffOutput, path: &str) -> &'a ExtractUiEffFile {
        output
            .files
            .iter()
            .find(|file| file.path == path)
            .expect("expected output file")
    }

    #[test]
    fn splits_nstg_data_into_remainder_and_extracted_archive() {
        let source = archive(
            &[0x100, 0x4f000001, 0x4f000002, 0x200],
            &[
                entry(8, b"keep-a"),
                entry(8, b"extract-a"),
                entry(4, b"extract-b"),
                entry(8, b"keep-b"),
            ],
        );
        let nstp = archive(&[0x100], &[entry(8, b"keep-p")]);

        let output = apply_extract_ui_eff(ExtractUiEffInput {
            nstg_data: &source,
            nstp_data: &nstp,
        })
        .unwrap();

        assert_eq!(
            ids(&file(&output, NSTG_DATA_PATH).bytes),
            vec![0x100, 0x200]
        );
        assert_eq!(
            ids(&file(&output, NSTGE_DATA_PATH).bytes),
            vec![0x4f000001, 0x4f000002]
        );
        assert_eq!(
            entry_payloads(&file(&output, NSTGE_DATA_PATH).bytes),
            vec![b"extract-a".to_vec(), b"extract-b".to_vec()]
        );
    }

    #[test]
    fn splits_nstp_data_into_two_extracted_archives() {
        let nstg = archive(&[0x100], &[entry(8, b"keep-g")]);
        let source = archive(
            &[0x5f000001, 0x100, 0x4f000001, 0x5f000002],
            &[
                entry(8, b"extract-u-a"),
                entry(8, b"keep"),
                entry(8, b"extract-e"),
                entry(4, b"extract-u-b"),
            ],
        );

        let output = apply_extract_ui_eff(ExtractUiEffInput {
            nstg_data: &nstg,
            nstp_data: &source,
        })
        .unwrap();

        assert_eq!(ids(&file(&output, NSTP_DATA_PATH).bytes), vec![0x100]);
        assert_eq!(ids(&file(&output, NSTPE_DATA_PATH).bytes), vec![0x4f000001]);
        assert_eq!(
            ids(&file(&output, NSTPU_DATA_PATH).bytes),
            vec![0x5f000001, 0x5f000002]
        );
        assert_eq!(
            entry_payloads(&file(&output, NSTPU_DATA_PATH).bytes),
            vec![b"extract-u-a".to_vec(), b"extract-u-b".to_vec()]
        );
    }

    #[test]
    fn returns_no_split_outputs_when_no_ranges_match() {
        let nstg = archive(&[0x100, 0x200], &[entry(8, b"a"), entry(8, b"b")]);
        let nstp = archive(&[0x300], &[entry(8, b"c")]);

        let output = apply_extract_ui_eff(ExtractUiEffInput {
            nstg_data: &nstg,
            nstp_data: &nstp,
        })
        .unwrap();

        assert!(output.files.is_empty());
    }

    #[test]
    fn rejects_malformed_source_archive() {
        let valid = archive(&[0x100], &[entry(8, b"ok")]);
        let err = apply_extract_ui_eff(ExtractUiEffInput {
            nstg_data: b"not an archive",
            nstp_data: &valid,
        })
        .unwrap_err()
        .to_string();

        assert!(err.contains("splitting NStgData.NOS"));
    }
}
