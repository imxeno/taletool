//! Filesystem path helpers used by CLI commands.
//!
//! These helpers keep command modules focused on domain orchestration while
//! centralizing wildcard expansion, directory listing, and archive-name
//! escaping rules.

use std::fs;
use std::path::{Path, PathBuf};

/// Resolve command input arguments into sorted, deduplicated paths.
pub(crate) fn resolve_inputs(inputs: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for input in inputs {
        if has_wildcard(input) {
            paths.extend(resolve_glob(input)?);
        } else {
            paths.push(PathBuf::from(input));
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        anyhow::bail!("no input files matched");
    }
    Ok(paths)
}

/// Return whether an input path contains CLI wildcard characters.
fn has_wildcard(input: &str) -> bool {
    input.contains('*') || input.contains('?')
}

/// Resolve a single-level wildcard pattern against its parent directory.
fn resolve_glob(pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
    let wildcard_at = pattern
        .find(['*', '?'])
        .ok_or_else(|| anyhow::anyhow!("not a glob pattern"))?;
    let separator_at = pattern[..wildcard_at]
        .rfind(['\\', '/'])
        .map(|index| index + 1)
        .unwrap_or(0);
    let (dir, file_pattern) = pattern.split_at(separator_at);
    let dir = if dir.is_empty() { "." } else { dir };
    let mut matches = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if wildcard_match(file_pattern, name) {
            matches.push(path);
        }
    }
    matches.sort();
    if matches.is_empty() {
        anyhow::bail!("glob did not match any files: {pattern}");
    }
    Ok(matches)
}

/// Case-insensitive `*` and `?` wildcard matcher for filenames.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    fn inner(pattern: &[u8], text: &[u8]) -> bool {
        match pattern {
            [] => text.is_empty(),
            [b'*', rest @ ..] => {
                inner(rest, text) || (!text.is_empty() && inner(pattern, &text[1..]))
            }
            [b'?', rest @ ..] => !text.is_empty() && inner(rest, &text[1..]),
            [first, rest @ ..] => {
                !text.is_empty() && first.eq_ignore_ascii_case(&text[0]) && inner(rest, &text[1..])
            }
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}

/// Return a binary archive family stem with a trailing chunk suffix removed.
pub(crate) fn binary_family_stem(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let trimmed = if stem.len() > 2
        && stem[stem.len() - 2..]
            .chars()
            .all(|c| c.is_ascii_hexdigit())
    {
        &stem[..stem.len() - 2]
    } else {
        stem
    };
    Some(trimmed.to_owned())
}

/// Return immediate files in a directory, sorted by path.
pub(crate) fn immediate_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Percent-escape an archived text record name for use as a local filename.
pub(crate) fn escape_archive_name(name: &str) -> String {
    let mut out = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    if out.is_empty() {
        "unnamed".to_owned()
    } else {
        out
    }
}

/// Decode a filename produced by `escape_archive_name`.
pub(crate) fn unescape_archive_name(name: &str) -> anyhow::Result<String> {
    let bytes = name.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                anyhow::bail!("invalid percent escape in {name}");
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])?;
            out.push(u8::from_str_radix(hex, 16)?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}
