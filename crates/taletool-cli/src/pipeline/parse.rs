use super::{PayloadKind, TextParser};
use crate::{
    cli::TextFormatArg,
    structured_text_file::{decode_structured_text_document, parse_optional_encoding},
};
use std::{fs, io::Write, path::Path};
use taletool_text::TextPayloadKind;

pub(super) fn run(
    input: &Path,
    parser: TextParser,
    kind: PayloadKind,
    encoding: &str,
    out: &Path,
) -> anyhow::Result<()> {
    let (name, format) = match parser {
        TextParser::Item => ("Item.dat", TextFormatArg::Gtd),
        TextParser::Bcard => ("BCard.dat", TextFormatArg::Gtd),
        TextParser::Card => ("Card.dat", TextFormatArg::Gtd),
        TextParser::Skill => ("Skill.dat", TextFormatArg::Gtd),
        TextParser::Monster => ("monster.dat", TextFormatArg::Gtd),
        TextParser::Quest => ("quest.dat", TextFormatArg::Gtd),
        TextParser::QuestPrize => ("qstprize.dat", TextFormatArg::Gtd),
        TextParser::QuestNpc => ("qstnpc.dat", TextFormatArg::Gtd),
        TextParser::MapIds => ("MapIDData.dat", TextFormatArg::Gtd),
        TextParser::Language => ("language.txt", TextFormatArg::Lang),
        TextParser::Constants => ("conststring.dat", TextFormatArg::Cli),
    };
    let kind = match kind {
        PayloadKind::Dat => TextPayloadKind::Dat,
        PayloadKind::Raw => TextPayloadKind::Raw,
    };
    let parsed = decode_structured_text_document(
        Path::new(name),
        &fs::read(input)?,
        kind,
        format,
        parse_optional_encoding(Some(encoding))?,
    )?;
    for warning in parsed.warnings {
        eprintln!("{warning}");
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = out
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("--out must name a file"))?;
    let part = out.with_file_name(format!(
        ".taletool-{}-{}.part",
        name.to_string_lossy(),
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)?;
    let result = (|| -> anyhow::Result<()> {
        file.write_all(&parsed.document)?;
        drop(file);
        fs::rename(&part, out)?;
        Ok(())
    })();
    if result.is_err() {
        fs::remove_file(&part)?;
    }
    result
}
