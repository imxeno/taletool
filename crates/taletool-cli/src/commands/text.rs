//! Handlers for `taletool text` payload commands.
//!
//! These commands operate on individual payload files, not full text archives.
//! Archive-level packing remains in the `archive` command module.

use anyhow::{Context, bail};
use std::fs;
#[cfg(test)]
use std::path::Path;
use taletool_text::{
    ConstStringTable, LanguageTable, NSetcStringList, encode_const_string_table,
    encode_language_table, encode_nsetc_string_list,
    gtd::{GtdDocument, GtdFileKind, encode_gtd_document},
};
#[cfg(test)]
use taletool_text::{MalformedConstStringRowKind, TextEncoding, TextPayloadKind};

use crate::cli::{TextCommand, TextFormatArg};
use crate::structured_text_file::{
    decode_structured_text_document, parse_optional_encoding, resolve_cli_encoding,
    resolve_etc_encoding, resolve_language_encoding, resolve_structured_format,
};
#[cfg(test)]
use crate::structured_text_file::{
    is_nsetc_payload_name, language_locale, malformed_const_string_warning,
    malformed_language_warning,
};
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
                let encoding = parse_optional_encoding(encoding.as_deref())?;
                let decoded =
                    decode_structured_text_document(&payload, &data, kind, format, encoding)?;
                for warning in decoded.warnings {
                    eprintln!("{warning}");
                }
                if let Some(parent) = out.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&out, decoded.document)?;
                println!(
                    "unpacked {} {} entries into {}",
                    decoded.count,
                    decoded.label,
                    out.display()
                );
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
                let encoding = parse_optional_encoding(encoding.as_deref())?;
                let document = fs::read(&input)?;
                let (encoded, count, label) = match format {
                    TextFormatArg::Lang => {
                        let encoding = resolve_language_encoding(&out, encoding)?;
                        let table: LanguageTable = serde_json::from_slice(&document)
                            .with_context(|| format!("parsing {}", input.display()))?;
                        let count = table.0.len();
                        (encode_language_table(&table, encoding)?, count, "language")
                    }
                    TextFormatArg::Cli => {
                        let encoding = resolve_cli_encoding(encoding)?;
                        let table: ConstStringTable = serde_json::from_slice(&document)
                            .with_context(|| format!("parsing {}", input.display()))?;
                        let count = table.0.len();
                        (
                            encode_const_string_table(&table, encoding)?,
                            count,
                            "constant-string",
                        )
                    }
                    TextFormatArg::Etc => {
                        let encoding = resolve_etc_encoding(encoding);
                        let list: NSetcStringList = serde_json::from_slice(&document)
                            .with_context(|| format!("parsing {}", input.display()))?;
                        let count = list.0.len();
                        (
                            encode_nsetc_string_list(&list, kind, encoding)?,
                            count,
                            "NSetc string",
                        )
                    }
                    TextFormatArg::Gtd => {
                        let gtd_kind = GtdFileKind::for_path(&out).ok_or_else(|| {
                            anyhow::anyhow!(
                                "cannot infer an NSgtdData grammar from {}",
                                out.display()
                            )
                        })?;
                        let document: GtdDocument = serde_json::from_slice(&document)
                            .with_context(|| format!("parsing {}", input.display()))?;
                        let count = document.entry_count();
                        (
                            encode_gtd_document(gtd_kind, &document, encoding)?,
                            count,
                            "NSgtdData",
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

fn require_json_for_explicit_format(format: TextFormatArg) -> anyhow::Result<()> {
    if format != TextFormatArg::Auto {
        bail!("--format requires --json");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_supported_language_encodings() {
        let cases = [
            ("_code_cz_Item.txt", TextEncoding::Windows1250),
            ("_code_gsp_Item.txt", TextEncoding::Windows1252),
            ("_code_in_Item.txt", TextEncoding::Windows1252),
            ("_code_jp_Item.txt", TextEncoding::ShiftJis),
            ("_code_kr_Item.txt", TextEncoding::EucKr),
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
            resolve_language_encoding(
                Path::new("_code_zz_Item.txt"),
                Some(TextEncoding::Windows1252),
            )
            .unwrap(),
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
        assert!(
            resolve_language_encoding(Path::new("Item.txt"), Some(TextEncoding::Windows1252))
                .is_ok()
        );
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
            resolve_cli_encoding(Some(TextEncoding::Windows1250)).unwrap(),
            TextEncoding::Windows1250
        );
    }

    #[test]
    fn resolves_nsetc_formats_and_encoding() {
        for (name, kind) in [
            ("MiniGame6WordData.dat", TextPayloadKind::Dat),
            ("TABOOSTR.LST", TextPayloadKind::List),
        ] {
            assert_eq!(
                resolve_structured_format(Path::new(name), kind, TextFormatArg::Auto).unwrap(),
                TextFormatArg::Etc
            );
            assert!(is_nsetc_payload_name(Path::new(name)));
        }

        for (name, kind) in [
            ("renamed.dat", TextPayloadKind::Dat),
            ("renamed.lst", TextPayloadKind::List),
        ] {
            assert_eq!(
                resolve_structured_format(Path::new(name), kind, TextFormatArg::Etc).unwrap(),
                TextFormatArg::Etc
            );
        }

        assert!(
            resolve_structured_format(
                Path::new("renamed.bin"),
                TextPayloadKind::Raw,
                TextFormatArg::Etc,
            )
            .is_err()
        );
        assert!(!is_nsetc_payload_name(Path::new("other.dat")));
        assert_eq!(resolve_etc_encoding(None), TextEncoding::EucKr);
        assert_eq!(
            resolve_etc_encoding(Some(TextEncoding::Windows1252)),
            TextEncoding::Windows1252
        );
        assert!(parse_optional_encoding(Some("utf-8")).is_err());
    }

    #[test]
    fn resolves_gtd_formats_from_native_names() {
        for (name, kind) in [
            ("Item.dat", TextPayloadKind::Dat),
            ("MAPPOINTDATA.DAT", TextPayloadKind::Dat),
            ("uk_nosmall.dat", TextPayloadKind::Dat),
            ("HK_ABUSE.LST", TextPayloadKind::List),
        ] {
            assert_eq!(
                resolve_structured_format(Path::new(name), kind, TextFormatArg::Auto).unwrap(),
                TextFormatArg::Gtd
            );
        }

        assert!(
            resolve_structured_format(
                Path::new("uk_abuse.dat"),
                TextPayloadKind::Dat,
                TextFormatArg::Gtd,
            )
            .is_err()
        );
        assert!(
            resolve_structured_format(
                Path::new("renamed.dat"),
                TextPayloadKind::Dat,
                TextFormatArg::Gtd,
            )
            .is_err()
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
