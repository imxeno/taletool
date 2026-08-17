//! Reusable structured JSON conversion for supported text payload families.

use std::path::Path;

use anyhow::bail;
use taletool_text::{
    MalformedConstStringRowKind, TextEncoding, TextPayloadKind, decode_const_string_table,
    decode_language_table, decode_nsetc_string_list,
    gtd::{GtdFileKind, decode_gtd_document},
};

use crate::cli::TextFormatArg;
use crate::text_payload::payload_kind_label;

/// One decoded structured text document and its non-fatal diagnostics.
pub(crate) struct StructuredTextDocument {
    pub(crate) document: Vec<u8>,
    pub(crate) count: usize,
    pub(crate) label: &'static str,
    pub(crate) warnings: Vec<String>,
}

/// Decode a supported DAT/LST payload into its structured JSON representation.
pub(crate) fn decode_structured_text_document(
    path: &Path,
    data: &[u8],
    kind: TextPayloadKind,
    format: TextFormatArg,
    encoding: Option<TextEncoding>,
) -> anyhow::Result<StructuredTextDocument> {
    match format {
        TextFormatArg::Lang => {
            let encoding = resolve_language_encoding(path, encoding)?;
            let parsed = decode_language_table(data, encoding)?;
            let warnings = parsed
                .malformed_rows
                .iter()
                .map(|malformed| malformed_language_warning(path, malformed.row))
                .collect();
            Ok(StructuredTextDocument {
                document: serde_json::to_vec_pretty(&parsed.table)?,
                count: parsed.table.0.len(),
                label: "language",
                warnings,
            })
        }
        TextFormatArg::Cli => {
            let encoding = resolve_cli_encoding(encoding)?;
            let parsed = decode_const_string_table(data, encoding)?;
            let warnings = parsed
                .malformed_rows
                .iter()
                .map(|malformed| {
                    malformed_const_string_warning(path, malformed.row, malformed.kind)
                })
                .collect();
            Ok(StructuredTextDocument {
                document: serde_json::to_vec_pretty(&parsed.table)?,
                count: parsed.table.0.len(),
                label: "constant-string",
                warnings,
            })
        }
        TextFormatArg::Etc => {
            let encoding = resolve_etc_encoding(encoding);
            let list = decode_nsetc_string_list(data, kind, encoding)?;
            Ok(StructuredTextDocument {
                document: serde_json::to_vec_pretty(&list)?,
                count: list.0.len(),
                label: "NSetc string",
                warnings: Vec::new(),
            })
        }
        TextFormatArg::Gtd => {
            let gtd_kind = GtdFileKind::for_path(path).ok_or_else(|| {
                anyhow::anyhow!("cannot infer an NSgtdData grammar from {}", path.display())
            })?;
            let parsed = decode_gtd_document(gtd_kind, data, encoding)?;
            let warnings = parsed
                .warnings
                .iter()
                .map(|warning| {
                    format!(
                        "warning: {}:{}: {}",
                        path.display(),
                        warning.row,
                        warning.message
                    )
                })
                .collect();
            Ok(StructuredTextDocument {
                document: serde_json::to_vec_pretty(&parsed.document)?,
                count: parsed.document.entry_count(),
                label: "NSgtdData",
                warnings,
            })
        }
        TextFormatArg::Auto => anyhow::bail!("structured text format must be resolved"),
    }
}

/// Resolve the structured format selected by a native payload filename.
pub(crate) fn resolve_structured_format(
    path: &Path,
    kind: TextPayloadKind,
    format: TextFormatArg,
) -> anyhow::Result<TextFormatArg> {
    let format = if format != TextFormatArg::Auto {
        format
    } else if language_locale(path).is_some() {
        TextFormatArg::Lang
    } else if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("conststring.dat"))
    {
        TextFormatArg::Cli
    } else if is_nsetc_payload_name(path) {
        TextFormatArg::Etc
    } else if GtdFileKind::for_path(path).is_some() {
        TextFormatArg::Gtd
    } else {
        bail!(
            "cannot infer a structured text format from {}; pass `--format lang`, `--format cli`, `--format etc`, or `--format gtd`",
            path.display()
        )
    };

    match format {
        TextFormatArg::Lang | TextFormatArg::Cli if kind != TextPayloadKind::Dat => bail!(
            "structured {format:?} JSON requires a DAT payload, got {}",
            path.display()
        ),
        TextFormatArg::Etc if kind == TextPayloadKind::Raw => bail!(
            "structured etc JSON requires a DAT or LST payload, got {}",
            path.display()
        ),
        TextFormatArg::Gtd => {
            let gtd_kind = GtdFileKind::for_path(path).ok_or_else(|| {
                anyhow::anyhow!("unsupported NSgtdData record name: {}", path.display())
            })?;
            match (gtd_kind, kind) {
                (GtdFileKind::Abuse(_), TextPayloadKind::List) => {}
                (GtdFileKind::Abuse(_), _) => bail!(
                    "structured gtd abuse JSON requires an LST payload, got {}",
                    payload_kind_label(kind)
                ),
                (_, TextPayloadKind::Dat) => {}
                (_, _) => bail!(
                    "structured gtd JSON requires a DAT payload, got {}",
                    payload_kind_label(kind)
                ),
            }
        }
        TextFormatArg::Auto => unreachable!("structured format is resolved"),
        _ => {}
    }

    Ok(format)
}

pub(crate) fn parse_optional_encoding(label: Option<&str>) -> anyhow::Result<Option<TextEncoding>> {
    label
        .map(|label| {
            TextEncoding::for_label(label)
                .ok_or_else(|| anyhow::anyhow!("unsupported text encoding: {label}"))
        })
        .transpose()
}

pub(crate) fn resolve_language_encoding(
    path: &Path,
    override_encoding: Option<TextEncoding>,
) -> anyhow::Result<TextEncoding> {
    if let Some(encoding) = override_encoding {
        return Ok(encoding);
    }
    let locale = language_locale(path).ok_or_else(|| {
        anyhow::anyhow!(
            "cannot infer an encoding from {}; pass --encoding explicitly",
            path.display()
        )
    })?;
    encoding_for_locale(&locale).ok_or_else(|| {
        anyhow::anyhow!(
            "cannot infer encoding for NSlang locale {locale:?}; pass --encoding explicitly"
        )
    })
}

pub(crate) fn resolve_cli_encoding(
    override_encoding: Option<TextEncoding>,
) -> anyhow::Result<TextEncoding> {
    override_encoding.ok_or_else(|| anyhow::anyhow!("NScli JSON conversion requires --encoding"))
}

pub(crate) fn resolve_etc_encoding(override_encoding: Option<TextEncoding>) -> TextEncoding {
    override_encoding.unwrap_or(TextEncoding::EucKr)
}

pub(crate) fn encoding_for_locale(locale: &str) -> Option<TextEncoding> {
    match locale.to_ascii_lowercase().as_str() {
        "cz" | "de" | "it" | "pl" => Some(TextEncoding::Windows1250),
        "es" | "fr" | "gsp" | "in" | "my" | "uk" => Some(TextEncoding::Windows1252),
        "ru" => Some(TextEncoding::Windows1251),
        "tr" => Some(TextEncoding::Windows1254),
        "hk" | "tw" => Some(TextEncoding::Big5),
        "jp" => Some(TextEncoding::ShiftJis),
        "kr" => Some(TextEncoding::EucKr),
        _ => None,
    }
}

pub(crate) fn language_locale(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let rest = name.strip_prefix("_code_")?;
    if !rest.ends_with(".txt") {
        return None;
    }
    let (locale, table) = rest.split_once('_')?;
    if locale.is_empty() || table == ".txt" {
        return None;
    }
    Some(locale.to_owned())
}

pub(crate) fn is_nsetc_payload_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("MiniGame6WordData.dat")
                || name.eq_ignore_ascii_case("TabooStr.lst")
        })
}

pub(crate) fn malformed_language_warning(path: &Path, row: usize) -> String {
    format!(
        "warning: {}:{row}: skipping malformed NSlang row: missing tab separator",
        path.display()
    )
}

pub(crate) fn malformed_const_string_warning(
    path: &Path,
    row: usize,
    kind: MalformedConstStringRowKind,
) -> String {
    let reason = match kind {
        MalformedConstStringRowKind::MissingVerticalTab => "missing vertical-tab separator",
        MalformedConstStringRowKind::InvalidIntegerKey => "invalid signed decimal key",
    };
    format!(
        "warning: {}:{row}: skipping malformed NScli row: {reason}",
        path.display()
    )
}
