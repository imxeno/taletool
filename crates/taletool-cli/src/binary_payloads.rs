//! Filename metadata and ordering rules for unpacked binary payloads.
//!
//! Binary archive unpacking preserves enough metadata in filenames for packing
//! to recreate duplicate IDs, explicit archive indexes, and per-entry
//! compression overrides.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use taletool_archive::{BinaryCompression, BinaryNosArchiveWriteEntry};

/// Metadata parsed from an unpacked binary payload filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BinaryPayloadFilename {
    /// Numeric archive file ID prefix.
    pub(crate) file_id: i32,
    /// Duplicate ordinal from a `__N` filename suffix.
    pub(crate) duplicate_ordinal: Option<usize>,
    /// Explicit archive table index from a `__indexN` suffix.
    pub(crate) explicit_index: Option<usize>,
    /// Per-entry compression override encoded in the filename.
    pub(crate) compression: Option<BinaryCompression>,
}

/// Payload bytes plus filename-derived metadata used while packing.
#[derive(Debug, Clone)]
pub(crate) struct BinaryPayloadInput {
    /// Archive file ID to write for this entry.
    pub(crate) file_id: i32,
    /// Duplicate ordinal used as a sort tie-breaker.
    pub(crate) duplicate_ordinal: Option<usize>,
    /// Explicit table slot requested by the filename.
    pub(crate) explicit_index: Option<usize>,
    /// Optional compression override for this entry.
    pub(crate) compression: Option<BinaryCompression>,
    /// Original filename for deterministic tie-breaking and diagnostics.
    pub(crate) source_name: String,
    /// Raw payload bytes read from disk.
    pub(crate) data: Vec<u8>,
}

/// Parse only the numeric file ID from an unpacked payload filename.
pub(crate) fn parse_id_filename(path: &Path) -> Option<i32> {
    parse_binary_payload_filename(path).map(|parsed| parsed.file_id)
}

/// Parse filename metadata for an unpacked binary payload.
///
/// Recognized suffixes are duplicate ordinals, explicit indexes, and
/// compression markers such as `__raw` or `__zlib`.
pub(crate) fn parse_binary_payload_filename(path: &Path) -> Option<BinaryPayloadFilename> {
    let stem = path.file_stem()?.to_str()?;
    let digits = stem
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    let file_id = digits.parse().ok()?;
    let mut duplicate_ordinal = None;
    let mut explicit_index = None;
    let mut compression = None;
    if let Some(rest) = stem
        .strip_prefix(&digits)
        .and_then(|rest| rest.strip_prefix("__"))
    {
        for token in rest.split("__") {
            let lower_token = token.to_ascii_lowercase();
            if duplicate_ordinal.is_none() && token.chars().all(|ch| ch.is_ascii_digit()) {
                if let Ok(value) = token.parse::<usize>() {
                    if value > 0 {
                        duplicate_ordinal = Some(value);
                    }
                }
            } else if compression.is_none() && lower_token == "raw" {
                compression = Some(BinaryCompression::Raw);
            } else if compression.is_none() && lower_token == "zlib" {
                compression = Some(BinaryCompression::Zlib);
            } else if explicit_index.is_none() {
                if let Some(index) = token.strip_prefix("index") {
                    if !index.is_empty() && index.chars().all(|ch| ch.is_ascii_digit()) {
                        explicit_index = index.parse().ok();
                    }
                }
            }
        }
    }
    Some(BinaryPayloadFilename {
        file_id,
        duplicate_ordinal,
        explicit_index,
        compression,
    })
}

/// Order payloads into the archive table positions used by the writer.
pub(crate) fn order_binary_payload_entries(
    mut entries: Vec<BinaryPayloadInput>,
) -> anyhow::Result<Vec<BinaryNosArchiveWriteEntry>> {
    if entries.iter().all(|entry| entry.explicit_index.is_none()) {
        sort_binary_payload_entries(&mut entries);
        return Ok(entries
            .into_iter()
            .map(BinaryNosArchiveWriteEntry::from)
            .collect());
    }

    let entry_count = entries.len();
    let mut slots = vec![None; entry_count];
    let mut unindexed = Vec::new();
    for entry in entries {
        match entry.explicit_index {
            Some(index) => {
                if index >= entry_count {
                    anyhow::bail!(
                        "{} claims explicit index {}, but this output chunk has {} entries",
                        entry.source_name,
                        index,
                        entry_count
                    );
                }
                if slots[index].is_some() {
                    anyhow::bail!("multiple payload files claim explicit index {index}");
                }
                slots[index] = Some(entry);
            }
            None => unindexed.push(entry),
        }
    }

    sort_binary_payload_entries(&mut unindexed);
    let mut unindexed = unindexed.into_iter();
    for slot in &mut slots {
        if slot.is_none() {
            *slot = unindexed.next();
        }
    }

    Ok(slots
        .into_iter()
        .map(|entry| BinaryNosArchiveWriteEntry::from(entry.expect("all slots are filled")))
        .collect())
}

/// Sort unindexed payloads by ID, duplicate ordinal, then source filename.
fn sort_binary_payload_entries(entries: &mut [BinaryPayloadInput]) {
    entries.sort_by(|left, right| {
        left.file_id
            .cmp(&right.file_id)
            .then_with(|| duplicate_sort_rank(left).cmp(&duplicate_sort_rank(right)))
            .then_with(|| left.source_name.cmp(&right.source_name))
    });
}

/// Return the sort rank for duplicate payload IDs.
fn duplicate_sort_rank(entry: &BinaryPayloadInput) -> usize {
    entry.duplicate_ordinal.unwrap_or(1)
}

impl From<BinaryPayloadInput> for BinaryNosArchiveWriteEntry {
    fn from(entry: BinaryPayloadInput) -> Self {
        Self {
            file_id: entry.file_id,
            compression: entry.compression,
            data: entry.data,
        }
    }
}

/// Find archive table indexes that must be preserved to round-trip duplicates.
pub(crate) fn explicit_indexes_for_binary_ids(ids: &[i32]) -> BTreeSet<usize> {
    let mut occurrences = BTreeMap::<i32, usize>::new();
    let keys = ids
        .iter()
        .enumerate()
        .map(|(index, file_id)| {
            let occurrence = occurrences.entry(*file_id).or_default();
            *occurrence += 1;
            (index, *file_id, *occurrence)
        })
        .collect::<Vec<_>>();
    let mut explicit_indexes = BTreeSet::new();

    loop {
        let simulated = simulate_binary_pack_indexes(&keys, &explicit_indexes);
        let Some((_, moved_index)) = simulated
            .iter()
            .enumerate()
            .find(|(index, original_index)| *index != **original_index)
        else {
            break;
        };
        explicit_indexes.insert(*moved_index);
    }

    explicit_indexes
}

/// Simulate pack ordering with a candidate set of explicit indexes.
fn simulate_binary_pack_indexes(
    keys: &[(usize, i32, usize)],
    explicit_indexes: &BTreeSet<usize>,
) -> Vec<usize> {
    let mut slots = vec![None; keys.len()];
    for index in explicit_indexes {
        slots[*index] = Some(*index);
    }

    let mut unindexed = keys
        .iter()
        .filter(|(index, _, _)| !explicit_indexes.contains(index))
        .copied()
        .collect::<Vec<_>>();
    unindexed.sort_by(|left, right| {
        left.1
            .cmp(&right.1)
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut unindexed = unindexed.into_iter();
    for slot in &mut slots {
        if slot.is_none() {
            *slot = unindexed.next().map(|(index, _, _)| index);
        }
    }

    slots
        .into_iter()
        .map(|index| index.expect("all slots are filled"))
        .collect()
}

/// Build a unique unpacked payload filename for one archive entry.
pub(crate) fn binary_payload_output_name(
    file_id: i32,
    duplicate_ordinal: usize,
    explicit_index: Option<usize>,
    compression: Option<BinaryCompression>,
    used_names: &mut BTreeSet<String>,
) -> anyhow::Result<String> {
    let primary = binary_payload_output_name_with_duplicate(
        file_id,
        (explicit_index.is_none() && duplicate_ordinal > 1).then_some(duplicate_ordinal),
        explicit_index,
        compression,
    );
    if used_names.insert(primary.clone()) {
        return Ok(primary);
    }

    if let Some(index) = explicit_index {
        let fallback = binary_payload_output_name_with_duplicate(
            file_id,
            Some(duplicate_ordinal),
            Some(index),
            compression,
        );
        if used_names.insert(fallback.clone()) {
            return Ok(fallback);
        }
    }

    anyhow::bail!("generated duplicate output filename for file id {file_id}")
}

/// Format one payload filename from its metadata components.
fn binary_payload_output_name_with_duplicate(
    file_id: i32,
    duplicate_ordinal: Option<usize>,
    explicit_index: Option<usize>,
    compression: Option<BinaryCompression>,
) -> String {
    let mut name = file_id.to_string();
    if let Some(duplicate_ordinal) = duplicate_ordinal {
        name.push_str(&format!("__{duplicate_ordinal}"));
    }
    if let Some(index) = explicit_index {
        name.push_str(&format!("__index{index}"));
    }
    if let Some(compression) = compression {
        name.push_str("__");
        name.push_str(compression_filename_token(compression));
    }
    name.push_str(".bin");
    name
}

/// Filename token for a compression override.
fn compression_filename_token(compression: BinaryCompression) -> &'static str {
    match compression {
        BinaryCompression::Raw => "raw",
        BinaryCompression::Zlib => "zlib",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;

    fn binary_input(
        source_name: &str,
        data_byte: u8,
        explicit_index: Option<usize>,
    ) -> BinaryPayloadInput {
        let parsed = parse_binary_payload_filename(Path::new(source_name)).unwrap();
        BinaryPayloadInput {
            file_id: parsed.file_id,
            duplicate_ordinal: parsed.duplicate_ordinal,
            explicit_index: explicit_index.or(parsed.explicit_index),
            compression: parsed.compression,
            source_name: source_name.to_owned(),
            data: vec![data_byte],
        }
    }

    #[test]
    fn parses_binary_payload_filename_metadata() {
        assert_eq!(
            parse_binary_payload_filename(Path::new("123.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: None,
                compression: None,
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("00123.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: None,
                compression: None,
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__2.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: Some(2),
                explicit_index: None,
                compression: None,
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__002.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: Some(2),
                explicit_index: None,
                compression: None,
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__index124.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: Some(124),
                compression: None,
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__index000124.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: Some(124),
                compression: None,
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__2__index124.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: Some(2),
                explicit_index: Some(124),
                compression: None,
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123_custom.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: None,
                compression: None,
            }
        );
    }

    #[test]
    fn parses_binary_payload_compression_filename_metadata() {
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__raw.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: None,
                compression: Some(BinaryCompression::Raw),
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__zlib.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: None,
                compression: Some(BinaryCompression::Zlib),
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__2__raw.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: Some(2),
                explicit_index: None,
                compression: Some(BinaryCompression::Raw),
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__index124__zlib.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: None,
                explicit_index: Some(124),
                compression: Some(BinaryCompression::Zlib),
            }
        );
        assert_eq!(
            parse_binary_payload_filename(Path::new("123__2__index124__RAW.bin")).unwrap(),
            BinaryPayloadFilename {
                file_id: 123,
                duplicate_ordinal: Some(2),
                explicit_index: Some(124),
                compression: Some(BinaryCompression::Raw),
            }
        );
    }

    #[test]
    fn binary_payload_output_name_appends_compression_marker_last() {
        let mut used_names = BTreeSet::new();

        assert_eq!(
            binary_payload_output_name(105, 1, None, Some(BinaryCompression::Raw), &mut used_names)
                .unwrap(),
            "105__raw.bin"
        );
        assert_eq!(
            binary_payload_output_name(
                0,
                2,
                Some(93),
                Some(BinaryCompression::Raw),
                &mut used_names
            )
            .unwrap(),
            "0__index93__raw.bin"
        );
    }

    #[test]
    fn binary_payload_order_sorts_clean_files_by_id() {
        let ordered = order_binary_payload_entries(vec![
            binary_input("7.bin", 7, None),
            binary_input("2.bin", 2, None),
            binary_input("5.bin", 5, None),
        ])
        .unwrap();

        assert_eq!(
            ordered
                .iter()
                .map(|entry| entry.file_id)
                .collect::<Vec<_>>(),
            vec![2, 5, 7]
        );
    }

    #[test]
    fn binary_payload_order_uses_duplicate_ordinal_as_tie_breaker() {
        let ordered = order_binary_payload_entries(vec![
            binary_input("123__3.bin", 3, None),
            binary_input("123.bin", 1, None),
            binary_input("123__2.bin", 2, None),
        ])
        .unwrap();

        assert_eq!(
            ordered
                .iter()
                .map(|entry| entry.data[0])
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn binary_payload_order_reserves_explicit_indexes() {
        let mut entries = Vec::new();
        entries.push(binary_input("0.bin", 0, None));
        for id in 1..93 {
            entries.push(binary_input(&format!("{id}.bin"), id as u8, None));
        }
        entries.push(binary_input("0__index93.bin", 93, None));
        entries.push(binary_input("94.bin", 94, None));
        entries.push(binary_input("95.bin", 95, None));
        entries.push(binary_input("96.bin", 96, None));
        entries.push(binary_input("0__index97.bin", 97, None));

        let ordered = order_binary_payload_entries(entries).unwrap();
        assert_eq!(ordered[0].file_id, 0);
        assert_eq!(ordered[93].file_id, 0);
        assert_eq!(ordered[97].file_id, 0);
        assert_eq!(ordered[1].file_id, 1);
        assert_eq!(ordered[94].file_id, 94);
    }

    #[test]
    fn binary_payload_order_preserves_compression_overrides() {
        let ordered = order_binary_payload_entries(vec![
            binary_input("2__raw.bin", 2, None),
            binary_input("1.bin", 1, None),
            binary_input("3__index1__zlib.bin", 3, None),
        ])
        .unwrap();

        assert_eq!(ordered[0].file_id, 1);
        assert_eq!(ordered[0].compression, None);
        assert_eq!(ordered[1].file_id, 3);
        assert_eq!(ordered[1].compression, Some(BinaryCompression::Zlib));
        assert_eq!(ordered[2].file_id, 2);
        assert_eq!(ordered[2].compression, Some(BinaryCompression::Raw));
    }

    #[test]
    fn binary_payload_order_rejects_duplicate_explicit_indexes() {
        let error = order_binary_payload_entries(vec![
            binary_input("1__index1.bin", 1, None),
            binary_input("2__index1.bin", 2, None),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("explicit index 1"));
    }

    #[test]
    fn explicit_index_detector_marks_only_displaced_zero_rows() {
        let mut ids = Vec::new();
        ids.push(0);
        ids.extend(1..93);
        ids.push(0);
        ids.extend([94, 95, 96]);
        ids.push(0);

        assert_eq!(
            explicit_indexes_for_binary_ids(&ids),
            BTreeSet::from([93, 97])
        );
    }
}
