//! Small shared helpers for CLI presentation and encoding.

use std::collections::BTreeMap;

/// Return duplicate ID counts sorted by ID.
pub(crate) fn duplicate_id_counts(ids: impl IntoIterator<Item = i32>) -> Vec<(i32, usize)> {
    let mut counts = BTreeMap::<i32, usize>::new();
    for id in ids {
        *counts.entry(id).or_default() += 1;
    }
    counts.into_iter().filter(|(_, count)| *count > 1).collect()
}

/// Emit a warning when an archive operation sees duplicated record IDs.
pub(crate) fn warn_duplicate_archive_ids(
    action: &str,
    archive_kind: &str,
    duplicates: &[(i32, usize)],
    detail: &str,
) {
    if duplicates.is_empty() {
        return;
    }

    eprintln!(
        "warning: {action} {archive_kind} archive with duplicated IDs: {}. {detail}",
        duplicate_id_summary(duplicates)
    );
}

/// Format a bounded duplicate ID summary for terminal warnings.
fn duplicate_id_summary(duplicates: &[(i32, usize)]) -> String {
    const MAX_DUPLICATE_IDS: usize = 8;

    let mut parts = duplicates
        .iter()
        .take(MAX_DUPLICATE_IDS)
        .map(|(id, count)| format!("{id} x{count}"))
        .collect::<Vec<_>>();
    if duplicates.len() > MAX_DUPLICATE_IDS {
        parts.push(format!("{} more", duplicates.len() - MAX_DUPLICATE_IDS));
    }
    parts.join(", ")
}

/// Compute the FNV-1a 64-bit checksum used by `archive inspect --checksum`.
pub(crate) fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_id_counts_returns_only_duplicates_sorted_by_id() {
        assert_eq!(
            duplicate_id_counts([7, 2, 7, 2, 9, 2, 1]),
            vec![(2, 3), (7, 2)]
        );
    }

    #[test]
    fn duplicate_id_summary_limits_large_lists() {
        let duplicates = (0..10).map(|id| (id, 2)).collect::<Vec<_>>();
        assert_eq!(
            duplicate_id_summary(&duplicates),
            "0 x2, 1 x2, 2 x2, 3 x2, 4 x2, 5 x2, 6 x2, 7 x2, 2 more"
        );
    }
}
