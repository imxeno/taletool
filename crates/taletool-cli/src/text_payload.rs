//! CLI glue for individual text payload encoding and decoding.
//!
//! The domain crate implements the DAT and LST codecs. This module provides
//! command-line kind inference, labels, preview formatting, and output path
//! selection.

use std::path::{Path, PathBuf};

use taletool_text::{
    TextPayloadKind, decode_dat_payload, decode_list_payload, encode_dat_payload,
    encode_list_payload,
};

use crate::cli::TextPayloadKindArg;

/// Resolve the effective text payload kind from `--kind` and the path.
pub(crate) fn resolve_text_payload_kind(path: &Path, kind: TextPayloadKindArg) -> TextPayloadKind {
    match kind {
        TextPayloadKindArg::Dat => TextPayloadKind::Dat,
        TextPayloadKindArg::List => TextPayloadKind::List,
        TextPayloadKindArg::Raw => TextPayloadKind::Raw,
        TextPayloadKindArg::Auto => {
            let lower = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.ends_with(".lst") {
                TextPayloadKind::List
            } else if lower.ends_with(".dat") || lower.ends_with(".txt") {
                TextPayloadKind::Dat
            } else {
                TextPayloadKind::Raw
            }
        }
    }
}

/// Decode bytes according to the selected text payload kind.
pub(crate) fn decode_text_payload(kind: TextPayloadKind, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(match kind {
        TextPayloadKind::Dat => decode_dat_payload(data)?,
        TextPayloadKind::List => decode_list_payload(data)?,
        TextPayloadKind::Raw => data.to_vec(),
    })
}

/// Encode bytes according to the selected text payload kind.
pub(crate) fn encode_text_payload(kind: TextPayloadKind, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(match kind {
        TextPayloadKind::Dat => encode_dat_payload(data)?,
        TextPayloadKind::List => encode_list_payload(data)?,
        TextPayloadKind::Raw => data.to_vec(),
    })
}

/// Return the packed flag used when writing a text archive record.
pub(crate) fn packed_flag_for_text_record(name: &str) -> i32 {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".dat") || lower.ends_with(".txt") {
        1
    } else {
        0
    }
}

/// Resolve the output file path for a decoded text payload.
pub(crate) fn text_output_path(payload: &Path, out: &Path) -> PathBuf {
    if out.extension().is_none() || out.is_dir() {
        let stem = payload
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("payload");
        out.join(format!("{stem}.txt"))
    } else {
        out.to_path_buf()
    }
}

/// Return the user-facing label for a payload kind.
pub(crate) fn payload_kind_label(kind: TextPayloadKind) -> &'static str {
    match kind {
        TextPayloadKind::Dat => "dat",
        TextPayloadKind::List => "list",
        TextPayloadKind::Raw => "raw",
    }
}

/// Build a one-line decoded text preview for terminal output.
pub(crate) fn preview_text(decoded: &[u8]) -> String {
    let mut preview = String::from_utf8_lossy(decoded).into_owned();
    preview = preview.replace('\r', "\\r").replace('\n', "\\n");
    const MAX_PREVIEW_CHARS: usize = 96;
    if preview.chars().count() > MAX_PREVIEW_CHARS {
        let truncated = preview.chars().take(MAX_PREVIEW_CHARS).collect::<String>();
        format!("{truncated}...")
    } else {
        preview
    }
}
