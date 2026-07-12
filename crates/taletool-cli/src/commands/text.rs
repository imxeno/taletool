//! Handlers for `taletool text` payload commands.
//!
//! These commands operate on individual payload files, not full text archives.
//! Archive-level packing remains in the `archive` command module.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use taletool_text::{
    ConstStringTable, LanguageTable, MalformedConstStringRowKind, TextEncoding, TextPayloadKind,
    decode_const_string_table, decode_language_table, encode_const_string_table,
    encode_language_table,
};

use crate::cli::{TextCommand, TextFormatArg};
use crate::text_payload::{
    decode_text_payload, encode_text_payload, payload_kind_label, preview_text,
    resolve_text_payload_kind, text_output_path,
};

/// Dispatch a `text` subcommand.
pub(crate) fn run_text(command: TextCommand) -> anyhow::Result<()> {
    match command {
        TextCommand::Inspect { payload, kind } => {
            let data = fs::read(&payload)?;
            let kind = resolve_text_payload_kind(&payload, kind);
            println!("kind: {}", payload_kind_label(kind));
            println!("encoded_size: {}", data.len());
            match decode_text_payload(kind, &data) {
                Ok(decoded) => {
                    println!("decoded_size: {}", decoded.len());
                    println!("preview: {}", preview_text(&decoded));
                }
                Err(error) => println!("decode_error: {error}"),
            }
            Ok(())
        }
        TextCommand::Unpack {
            payload,
            out,
            kind,
            format,
            json,
            encoding,
        } => {
            let data = fs::read(&payload)?;
            let kind = resolve_text_payload_kind(&payload, kind);
            if json {
                let format = resolve_structured_format(&payload, kind, format)?;
                let (document, count, label) = match format {
                    TextFormatArg::Lang => {
                        let encoding = resolve_language_encoding(&payload, encoding.as_deref())?;
                        let parsed = decode_language_table(&data, encoding)?;
                        for malformed in &parsed.malformed_rows {
                            eprintln!("{}", malformed_language_warning(&payload, malformed.row));
                        }
                        (
                            serde_json::to_vec_pretty(&parsed.table)?,
                            parsed.table.0.len(),
                            "language",
                        )
                    }
                    TextFormatArg::Cli => {
                        let encoding = resolve_cli_encoding(encoding.as_deref())?;
                        let parsed = decode_const_string_table(&data, encoding)?;
                        for malformed in &parsed.malformed_rows {
                            eprintln!(
                                "{}",
                                malformed_const_string_warning(
                                    &payload,
                                    malformed.row,
                                    malformed.kind,
                                )
                            );
                        }
                        (
                            serde_json::to_vec_pretty(&parsed.table)?,
                            parsed.table.0.len(),
                            "constant-string",
                        )
                    }
                    TextFormatArg::Auto => unreachable!("structured format is resolved"),
                };
                if let Some(parent) = out.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out, document)?;
                println!("unpacked {count} {label} entries into {}", out.display());
                return Ok(());
            }
            require_json_for_explicit_format(format)?;
            let decoded = decode_text_payload(kind, &data)?;
            let path = text_output_path(&payload, &out);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, decoded)?;
            println!("unpacked {}", path.display());
            Ok(())
        }
        TextCommand::Pack {
            input,
            out,
            kind,
            format,
            json,
            encoding,
        } => {
            if json {
                let kind = resolve_text_payload_kind(&out, kind);
                let format = resolve_structured_format(&out, kind, format)?;
                let document = fs::read(&input)?;
                let (encoded, count, label) = match format {
                    TextFormatArg::Lang => {
                        let encoding = resolve_language_encoding(&out, encoding.as_deref())?;
                        let table: LanguageTable = serde_json::from_slice(&document)
                            .with_context(|| format!("parsing {}", input.display()))?;
                        let count = table.0.len();
                        (encode_language_table(&table, encoding)?, count, "language")
                    }
                    TextFormatArg::Cli => {
                        let encoding = resolve_cli_encoding(encoding.as_deref())?;
                        let table: ConstStringTable = serde_json::from_slice(&document)
                            .with_context(|| format!("parsing {}", input.display()))?;
                        let count = table.0.len();
                        (
                            encode_const_string_table(&table, encoding)?,
                            count,
                            "constant-string",
                        )
                    }
                    TextFormatArg::Auto => unreachable!("structured format is resolved"),
                };
                if let Some(parent) = out.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out, encoded)?;
                println!("packed {count} {label} entries into {}", out.display());
                return Ok(());
            }
            require_json_for_explicit_format(format)?;
            let decoded = fs::read(&input)?;
            let kind = resolve_text_payload_kind(&out, kind);
            let encoded = encode_text_payload(kind, &decoded)?;
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out, encoded)?;
            println!("packed {}", out.display());
            Ok(())
        }
    }
}

fn resolve_structured_format(
    path: &Path,
    kind: TextPayloadKind,
    format: TextFormatArg,
) -> anyhow::Result<TextFormatArg> {
    if kind != TextPayloadKind::Dat {
        bail!(
            "structured text JSON requires a DAT payload, got {}",
            path.display()
        );
    }
    if format != TextFormatArg::Auto {
        return Ok(format);
    }
    if language_locale(path).is_some() {
        return Ok(TextFormatArg::Lang);
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("conststring.dat"))
    {
        return Ok(TextFormatArg::Cli);
    }
    bail!(
        "cannot infer a structured text format from {}; pass `--format lang` or `--format cli`",
        path.display()
    )
}

fn require_json_for_explicit_format(format: TextFormatArg) -> anyhow::Result<()> {
    if format != TextFormatArg::Auto {
        bail!("--format requires --json");
    }
    Ok(())
}

fn resolve_language_encoding(
    path: &Path,
    override_label: Option<&str>,
) -> anyhow::Result<TextEncoding> {
    if let Some(label) = override_label {
        return TextEncoding::for_label(label)
            .ok_or_else(|| anyhow::anyhow!("unsupported text encoding: {label}"));
    }

    let locale = language_locale(path).ok_or_else(|| {
        anyhow::anyhow!(
            "cannot infer an encoding from {}; pass --encoding explicitly",
            path.display()
        )
    })?;
    let encoding = match locale.as_str() {
        "cz" | "de" | "it" | "pl" => TextEncoding::Windows1250,
        "es" | "fr" | "uk" | "gsp" => TextEncoding::Windows1252,
        "ru" => TextEncoding::Windows1251,
        "tr" => TextEncoding::Windows1254,
        "hk" | "tw" => TextEncoding::Big5,
        _ => {
            bail!("cannot infer encoding for NSlang locale {locale:?}; pass --encoding explicitly")
        }
    };
    Ok(encoding)
}

fn resolve_cli_encoding(override_label: Option<&str>) -> anyhow::Result<TextEncoding> {
    let label = override_label
        .ok_or_else(|| anyhow::anyhow!("NScli JSON conversion requires --encoding"))?;
    TextEncoding::for_label(label)
        .ok_or_else(|| anyhow::anyhow!("unsupported text encoding: {label}"))
}

fn language_locale(path: &Path) -> Option<String> {
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

fn malformed_language_warning(path: &Path, row: usize) -> String {
    format!(
        "warning: {}:{row}: skipping malformed NSlang row: missing tab separator",
        path.display()
    )
}

fn malformed_const_string_warning(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_supported_language_encodings() {
        let cases = [
            ("_code_cz_Item.txt", TextEncoding::Windows1250),
            ("_code_gsp_Item.txt", TextEncoding::Windows1252),
            ("_code_ru_Item.txt", TextEncoding::Windows1251),
            ("_code_tr_Item.txt", TextEncoding::Windows1254),
            ("_code_tw_Item.txt", TextEncoding::Big5),
        ];
        for (name, expected) in cases {
            assert_eq!(
                resolve_language_encoding(Path::new(name), None).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn encoding_override_supports_unknown_language_locale() {
        assert_eq!(
            resolve_language_encoding(Path::new("_code_zz_Item.txt"), Some("cp1252")).unwrap(),
            TextEncoding::Windows1252
        );
        assert!(resolve_language_encoding(Path::new("_code_zz_Item.txt"), None).is_err());
    }

    #[test]
    fn recognizes_only_nslang_payload_names() {
        assert_eq!(
            language_locale(Path::new("_code_UK_Item.txt")),
            Some("uk".to_owned())
        );
        assert!(language_locale(Path::new("conststring.dat")).is_none());
        assert!(language_locale(Path::new("_code_uk_.txt")).is_none());
    }

    #[test]
    fn explicit_language_format_accepts_a_renamed_dat_payload() {
        assert_eq!(
            resolve_structured_format(
                Path::new("Item.txt"),
                TextPayloadKind::Dat,
                TextFormatArg::Lang,
            )
            .unwrap(),
            TextFormatArg::Lang
        );
        assert!(
            resolve_structured_format(
                Path::new("Item.txt"),
                TextPayloadKind::Dat,
                TextFormatArg::Auto,
            )
            .is_err()
        );
        assert!(resolve_language_encoding(Path::new("Item.txt"), Some("windows-1252")).is_ok());
        assert!(resolve_language_encoding(Path::new("Item.txt"), None).is_err());
    }

    #[test]
    fn resolves_const_string_format_and_requires_an_encoding() {
        assert_eq!(
            resolve_structured_format(
                Path::new("conststring.dat"),
                TextPayloadKind::Dat,
                TextFormatArg::Auto,
            )
            .unwrap(),
            TextFormatArg::Cli
        );
        assert_eq!(
            resolve_structured_format(
                Path::new("strings.txt"),
                TextPayloadKind::Dat,
                TextFormatArg::Cli,
            )
            .unwrap(),
            TextFormatArg::Cli
        );
        assert!(resolve_cli_encoding(None).is_err());
        assert_eq!(
            resolve_cli_encoding(Some("cp1250")).unwrap(),
            TextEncoding::Windows1250
        );
    }

    #[test]
    fn malformed_warning_contains_path_row_and_reason() {
        assert_eq!(
            malformed_language_warning(Path::new("_code_uk_Item.txt"), 42),
            "warning: _code_uk_Item.txt:42: skipping malformed NSlang row: missing tab separator"
        );
        assert_eq!(
            malformed_const_string_warning(
                Path::new("conststring.dat"),
                7,
                MalformedConstStringRowKind::InvalidIntegerKey,
            ),
            "warning: conststring.dat:7: skipping malformed NScli row: invalid signed decimal key"
        );
    }
}
