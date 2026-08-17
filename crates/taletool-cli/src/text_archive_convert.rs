//! Strict family-aware conversion policy for named-record text archives.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use taletool_archive::TextNosArchive;
use taletool_text::{TextEncoding, TextPayloadKind};

use crate::cli::TextFormatArg;
use crate::paths::escape_archive_name;
use crate::structured_text_file::{
    decode_structured_text_document, encoding_for_locale, language_locale, parse_optional_encoding,
    resolve_structured_format,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextArchiveFamily {
    Gtd,
    Lang,
    Cli,
    Etc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextArchiveOutputMode {
    Json,
    PlainText,
}

impl TextArchiveFamily {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Gtd => "NSgtdData",
            Self::Lang => "NSlangData",
            Self::Cli => "NScliData",
            Self::Etc => "NSetcData",
        }
    }

    const fn format(self) -> TextFormatArg {
        match self {
            Self::Gtd => TextFormatArg::Gtd,
            Self::Lang => TextFormatArg::Lang,
            Self::Cli => TextFormatArg::Cli,
            Self::Etc => TextFormatArg::Etc,
        }
    }
}

struct TextArchiveRecordPlan {
    index: usize,
    relative_path: PathBuf,
}

pub(crate) struct TextArchiveConversionPlan {
    family: TextArchiveFamily,
    output_mode: TextArchiveOutputMode,
    encoding: Option<TextEncoding>,
    records: Vec<TextArchiveRecordPlan>,
}

impl TextArchiveConversionPlan {
    pub(crate) const fn family(&self) -> TextArchiveFamily {
        self.family
    }
}

pub(crate) struct ConvertedTextRecord {
    pub(crate) id: i32,
    pub(crate) relative_path: PathBuf,
    pub(crate) description: String,
    pub(crate) warnings: Vec<String>,
}

struct NamedTextArchive {
    family: TextArchiveFamily,
    locale: Option<String>,
    encoding: Option<TextEncoding>,
}

/// Resolve and validate every text record before creating transactional output.
pub(crate) fn resolve_text_archive_conversion(
    archive: &TextNosArchive,
    output_mode: TextArchiveOutputMode,
    encoding_label: Option<&str>,
) -> anyhow::Result<TextArchiveConversionPlan> {
    if output_mode == TextArchiveOutputMode::PlainText && encoding_label.is_some() {
        anyhow::bail!("--encoding cannot be used with --plain-text");
    }
    let explicit_encoding = if output_mode == TextArchiveOutputMode::Json {
        parse_optional_encoding(encoding_label)?
    } else {
        None
    };
    let named = named_text_archive(archive.path())?;
    let mut family = named.as_ref().map(|named| named.family);
    let mut records = Vec::with_capacity(archive.records().len());
    let mut output_names = BTreeMap::<String, (usize, i32, String)>::new();
    let mut language_locale_value = None::<(String, usize, i32, String)>;

    for (index, record) in archive.records().iter().enumerate() {
        let logical_path = Path::new(&record.name);
        let format =
            resolve_structured_format(logical_path, record.payload_kind(), TextFormatArg::Auto)
                .with_context(|| record_context(archive, index, record.id, &record.name, family))?;
        let record_family = family_for_format(format);
        if let Some(expected) = family {
            if record_family != expected {
                anyhow::bail!(
                    "{}: record identifies family {}, but the archive identifies {}",
                    record_context(archive, index, record.id, &record.name, family),
                    record_family.name(),
                    expected.name(),
                );
            }
        } else {
            family = Some(record_family);
        }

        if record_family == TextArchiveFamily::Etc {
            validate_nsetc_record_kind(logical_path, record.payload_kind())
                .with_context(|| record_context(archive, index, record.id, &record.name, family))?;
        }

        if record_family == TextArchiveFamily::Lang {
            let locale = language_locale(logical_path).expect("language format has a locale");
            if let Some((expected, ..)) = &language_locale_value {
                if !locale.eq_ignore_ascii_case(expected) {
                    anyhow::bail!(
                        "{}: language locale {locale:?} conflicts with {expected:?}",
                        record_context(archive, index, record.id, &record.name, family),
                    );
                }
            } else {
                language_locale_value = Some((locale, index, record.id, record.name.clone()));
            }
        }

        let relative_path = converted_output_path(&record.name, output_mode)
            .with_context(|| record_context(archive, index, record.id, &record.name, family))?;
        let collision_key = relative_path.to_string_lossy().to_lowercase();
        if let Some((previous_index, previous_id, previous_name)) =
            output_names.insert(collision_key, (index, record.id, record.name.clone()))
        {
            anyhow::bail!(
                "{} and {} both convert to {}",
                record_context(archive, previous_index, previous_id, &previous_name, family,),
                record_context(archive, index, record.id, &record.name, family),
                relative_path.display(),
            );
        }
        records.push(TextArchiveRecordPlan {
            index,
            relative_path,
        });
    }

    let family = family.ok_or_else(|| {
        anyhow::anyhow!(
            "cannot identify empty renamed text archive {}; use a canonical family filename",
            archive.path().display()
        )
    })?;
    if let Some(named) = &named
        && named.family == TextArchiveFamily::Lang
        && let Some(named_locale) = &named.locale
        && let Some((record_locale, index, id, name)) = &language_locale_value
        && !named_locale.eq_ignore_ascii_case(record_locale)
    {
        anyhow::bail!(
            "{}: archive filename identifies NSlang locale {:?}, but its records identify {:?}",
            record_context(archive, *index, *id, name, Some(family)),
            named_locale,
            record_locale,
        );
    }

    if output_mode == TextArchiveOutputMode::Json
        && family == TextArchiveFamily::Gtd
        && explicit_encoding.is_some()
    {
        anyhow::bail!(
            "--encoding cannot be used with NSgtdData because its records may use different encodings"
        );
    }
    let encoding = match (output_mode, family) {
        (TextArchiveOutputMode::PlainText, _) | (_, TextArchiveFamily::Gtd) => None,
        (TextArchiveOutputMode::Json, TextArchiveFamily::Lang) => explicit_encoding
            .or_else(|| named.as_ref().and_then(|named| named.encoding))
            .or_else(|| {
                language_locale_value
                    .as_ref()
                    .and_then(|(locale, ..)| encoding_for_locale(locale))
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot infer NSlang encoding for {}; pass --encoding explicitly",
                    archive.path().display()
                )
            })?
            .into(),
        (TextArchiveOutputMode::Json, TextArchiveFamily::Cli) => explicit_encoding
            .or_else(|| named.as_ref().and_then(|named| named.encoding))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot infer NScli encoding for {}; pass --encoding explicitly",
                    archive.path().display()
                )
            })?
            .into(),
        (TextArchiveOutputMode::Json, TextArchiveFamily::Etc) => {
            Some(explicit_encoding.unwrap_or(TextEncoding::EucKr))
        }
    };

    Ok(TextArchiveConversionPlan {
        family,
        output_mode,
        encoding,
        records,
    })
}

/// Decode and write all records from one preflighted text archive.
pub(crate) fn convert_text_archive(
    archive: &TextNosArchive,
    plan: &TextArchiveConversionPlan,
    out: &Path,
) -> anyhow::Result<Vec<ConvertedTextRecord>> {
    let mut converted = Vec::with_capacity(plan.records.len());
    for record_plan in &plan.records {
        let record = &archive.records()[record_plan.index];
        let context = || {
            record_context(
                archive,
                record_plan.index,
                record.id,
                &record.name,
                Some(plan.family),
            )
        };
        let (document, description, warnings) = match plan.output_mode {
            TextArchiveOutputMode::Json => {
                let decoded = decode_structured_text_document(
                    Path::new(&record.name),
                    &record.payload,
                    record.payload_kind(),
                    plan.family.format(),
                    plan.encoding,
                )
                .with_context(|| format!("converting {}", context()))?;
                (
                    decoded.document,
                    format!("{}, {} entries", decoded.label, decoded.count),
                    decoded.warnings,
                )
            }
            TextArchiveOutputMode::PlainText => {
                // NSgtd abuse lists use a zero-byte payload as a distinct, valid empty state.
                // Other LST families still require their normal counted envelope.
                let decoded = if plan.family == TextArchiveFamily::Gtd
                    && record.payload_kind() == TextPayloadKind::List
                    && record.payload.is_empty()
                {
                    Vec::new()
                } else {
                    record
                        .decoded_payload()
                        .with_context(|| format!("decoding DAT/LST envelope for {}", context()))?
                };
                let description = format!(
                    "decoded {} text, {} bytes",
                    payload_kind_name(record.payload_kind()),
                    decoded.len(),
                );
                (decoded, description, Vec::new())
            }
        };
        let path = out.join(&record_plan.relative_path);
        fs::write(&path, document)
            .with_context(|| format!("writing converted {} to {}", context(), path.display(),))?;
        converted.push(ConvertedTextRecord {
            id: record.id,
            relative_path: record_plan.relative_path.clone(),
            description,
            warnings,
        });
    }
    Ok(converted)
}

fn named_text_archive(path: &Path) -> anyhow::Result<Option<NamedTextArchive>> {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return Ok(None);
    };
    let lower = stem.to_ascii_lowercase();
    let named = match lower.as_str() {
        "nsgtddata" => Some(NamedTextArchive {
            family: TextArchiveFamily::Gtd,
            locale: None,
            encoding: None,
        }),
        "nslangdata" => Some(NamedTextArchive {
            family: TextArchiveFamily::Lang,
            locale: None,
            encoding: None,
        }),
        "nsclidata" => Some(NamedTextArchive {
            family: TextArchiveFamily::Cli,
            locale: None,
            encoding: Some(TextEncoding::EucKr),
        }),
        "nsetcdata" => Some(NamedTextArchive {
            family: TextArchiveFamily::Etc,
            locale: None,
            encoding: Some(TextEncoding::EucKr),
        }),
        _ => {
            if let Some(locale) = lower.strip_prefix("nslangdata_") {
                Some(locale_named_archive(TextArchiveFamily::Lang, locale, path)?)
            } else if let Some(locale) = lower.strip_prefix("nsclidata_") {
                Some(locale_named_archive(TextArchiveFamily::Cli, locale, path)?)
            } else {
                None
            }
        }
    };
    Ok(named)
}

fn locale_named_archive(
    family: TextArchiveFamily,
    locale: &str,
    path: &Path,
) -> anyhow::Result<NamedTextArchive> {
    if locale.is_empty() {
        anyhow::bail!("text archive locale is empty: {}", path.display());
    }
    Ok(NamedTextArchive {
        family,
        locale: Some(locale.to_owned()),
        encoding: encoding_for_locale(locale),
    })
}

fn family_for_format(format: TextFormatArg) -> TextArchiveFamily {
    match format {
        TextFormatArg::Gtd => TextArchiveFamily::Gtd,
        TextFormatArg::Lang => TextArchiveFamily::Lang,
        TextFormatArg::Cli => TextArchiveFamily::Cli,
        TextFormatArg::Etc => TextArchiveFamily::Etc,
        TextFormatArg::Auto => unreachable!("structured format is resolved"),
    }
}

fn validate_nsetc_record_kind(path: &Path, kind: TextPayloadKind) -> anyhow::Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let expected = if name.eq_ignore_ascii_case("MiniGame6WordData.dat") {
        TextPayloadKind::Dat
    } else if name.eq_ignore_ascii_case("TabooStr.lst") {
        TextPayloadKind::List
    } else {
        unreachable!("NSetc family is resolved only from supported native names")
    };
    if kind != expected {
        anyhow::bail!(
            "{} requires a {} payload, got {}",
            path.display(),
            payload_kind_name(expected),
            payload_kind_name(kind),
        );
    }
    Ok(())
}

const fn payload_kind_name(kind: TextPayloadKind) -> &'static str {
    match kind {
        TextPayloadKind::Dat => "DAT",
        TextPayloadKind::List => "LST",
        TextPayloadKind::Raw => "raw",
    }
}

fn converted_output_path(
    name: &str,
    output_mode: TextArchiveOutputMode,
) -> anyhow::Result<PathBuf> {
    let escaped = escape_archive_name(name);
    if output_mode == TextArchiveOutputMode::PlainText {
        return Ok(PathBuf::from(escaped));
    }
    let stem = Path::new(&escaped)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| anyhow::anyhow!("text record has no usable output stem: {name:?}"))?;
    Ok(PathBuf::from(format!("{stem}.json")))
}

fn record_context(
    archive: &TextNosArchive,
    index: usize,
    id: i32,
    name: &str,
    family: Option<TextArchiveFamily>,
) -> String {
    format!(
        "{} record at index {index} with id {id} and name {name:?} from {}",
        family.map_or("text", TextArchiveFamily::name),
        archive.path().display()
    )
}
