//! Stable, explicit interfaces for content-addressed data processing.
mod extract;
mod parse;

use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub(crate) enum PipelineCommand {
    /// Print component schemas and reproducible implementation fingerprints.
    Components,
    /// Extract ordered text archive records without parsing their payloads.
    ExtractText {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Parse a raw text payload with an explicit grammar, envelope and encoding.
    ParseText {
        input: PathBuf,
        #[arg(long, value_enum)]
        parser: TextParser,
        #[arg(long, value_enum)]
        kind: PayloadKind,
        #[arg(long)]
        encoding: String,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum TextParser {
    Item,
    Bcard,
    Card,
    Skill,
    Monster,
    Quest,
    QuestPrize,
    QuestNpc,
    MapIds,
    Language,
    Constants,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum PayloadKind {
    Dat,
    Raw,
}

pub(crate) fn run(command: PipelineCommand) -> anyhow::Result<()> {
    match command {
        PipelineCommand::Components => {
            println!(
                "{}",
                include_str!(concat!(env!("OUT_DIR"), "/pipeline-components.json"))
            );
            Ok(())
        }
        PipelineCommand::ExtractText { input, out } => extract::run(&input, &out),
        PipelineCommand::ParseText {
            input,
            parser,
            kind,
            encoding,
            out,
        } => parse::run(&input, parser, kind, &encoding, &out),
    }
}
