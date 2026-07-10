//! Clap definitions for the `taletool` command tree.
//!
//! This module is deliberately declarative. Command behavior belongs in
//! `commands`, while this file defines the argument surface and value enums
//! that Clap parses.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Top-level CLI parser for the `taletool` binary.
#[derive(Debug, Parser)]
#[command(name = "taletool")]
#[command(about = "NosTale data swiss army knife")]
pub(crate) struct Cli {
    /// Global verbosity count, where repeated `-v` increases tracing detail.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    /// The requested command branch.
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Top-level command groups.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Scan a NosTale data directory and classify supported data files.
    Scan {
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect, unpack, or pack full archive containers.
    Archive {
        #[command(subcommand)]
        command: ArchiveCommand,
    },
    /// Inspect, decode, or encode CCINF GBFC index files.
    Ccinf {
        #[command(subcommand)]
        command: CcinfCommand,
    },
    /// Inspect, decode, or encode extracted sprite payloads.
    Sprite {
        #[command(subcommand)]
        command: SpriteCommand,
    },
    /// Inspect or apply original NosTale patch packages.
    #[command(visible_alias = "pak")]
    Patch {
        #[command(subcommand)]
        command: PatchCommand,
    },
    /// Inspect, decode, or encode extracted text payload files.
    Text {
        #[command(subcommand)]
        command: TextCommand,
    },
}

/// Operations for full archive containers.
#[derive(Debug, Subcommand)]
pub(crate) enum ArchiveCommand {
    /// Print archive metadata and optional checksums.
    Inspect {
        #[arg(required = true)]
        input: Vec<String>,
        #[arg(long = "type", value_enum, default_value_t = ArchiveType::Auto)]
        archive_type: ArchiveType,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Extract archive records or payloads to a directory.
    Unpack {
        #[arg(required = true)]
        input: Vec<String>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "type", value_enum, default_value_t = ArchiveType::Auto)]
        archive_type: ArchiveType,
    },
    /// Build a binary, text, or sound archive from an unpacked directory.
    Pack {
        dir: PathBuf,
        #[arg(long)]
        out: String,
        #[arg(long = "type", value_enum, default_value_t = ArchiveType::Auto)]
        archive_type: ArchiveType,
        #[arg(long, default_value = "auto")]
        preset: String,
        #[arg(long)]
        header_hex: Option<String>,
        #[arg(long)]
        direct_index: Option<u8>,
        #[arg(long, value_enum, default_value_t = CompressionArg::Auto)]
        compression: CompressionArg,
        #[arg(long, default_value = "auto")]
        zlib_profile: String,
        #[arg(long, value_enum)]
        chunking: Option<ChunkingArg>,
        #[arg(long)]
        chunk_count: Option<usize>,
        #[arg(long)]
        chunk_format: Option<String>,
    },
}

/// Operations for structured CCINF GBFC index files.
#[derive(Debug, Subcommand)]
pub(crate) enum CcinfCommand {
    /// Print wrapper metadata and a typed entry summary.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a CCINF file into a versioned JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a versioned JSON document into a canonical raw CCINF file.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for multi-frame sprite payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum SpriteCommand {
    /// Print frame dimensions, source coordinates, and data offsets.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a sprite payload into PNG frames and `sprite.json`.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Write a single-frame sprite directly to the output PNG without metadata.
        #[arg(long)]
        png_only: bool,
    },
    /// Encode a manifest-backed sprite directory into a payload file.
    Pack {
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for original NosTale patch packages.
#[derive(Debug, Subcommand)]
pub(crate) enum PatchCommand {
    /// Print package operations and payload checksums.
    Inspect {
        #[arg(required = true)]
        package: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Apply packages to a client root in the exact order provided.
    Apply {
        #[arg(long)]
        root: PathBuf,
        #[arg(required = true)]
        package: Vec<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        backup_dir: Option<PathBuf>,
    },
}

/// Operations for individual text payload files extracted from archives.
#[derive(Debug, Subcommand)]
pub(crate) enum TextCommand {
    /// Print payload size, inferred kind, and a decoded preview.
    Inspect {
        payload: PathBuf,
        #[arg(long, value_enum, default_value_t = TextPayloadKindArg::Auto)]
        kind: TextPayloadKindArg,
    },
    /// Decode a packed text payload into a plain text file.
    Unpack {
        payload: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = TextPayloadKindArg::Auto)]
        kind: TextPayloadKindArg,
    },
    /// Encode a plain text file into the selected payload format.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = TextPayloadKindArg::Auto)]
        kind: TextPayloadKindArg,
    },
}

/// Archive parser/writer selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ArchiveType {
    /// Try supported archive parsers and require an unambiguous match.
    Auto,
    /// Treat inputs as binary table/chunk archives.
    Binary,
    /// Treat inputs as text record archives.
    Text,
    /// Treat inputs as DelDX sound packs such as snd.pck.
    Sound,
}

/// Compression override for binary archive packing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CompressionArg {
    /// Use the selected preset's compression policy.
    Auto,
    /// Store payloads without zlib compression.
    Raw,
    /// Compress payloads as zlib streams.
    Zlib,
}

/// Chunk distribution strategy for binary archive packing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ChunkingArg {
    /// Write every payload to one archive file.
    Single,
    /// Route payloads by the low byte of the file ID.
    LowByte,
}

/// Text payload encoding selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum TextPayloadKindArg {
    /// Infer the payload kind from the file name.
    Auto,
    /// Use the compact DAT text encoding.
    Dat,
    /// Use the LST line-table encoding.
    List,
    /// Leave bytes unchanged.
    Raw,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dedicated_ccinf_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "ccinf",
            "inspect",
            "NSmnData.NOS",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Ccinf {
                command: CcinfCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "ccinf",
            "unpack",
            "NSmnData.NOS",
            "--out",
            "NSmnData.json",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Ccinf {
                command: CcinfCommand::Unpack { .. }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "ccinf",
            "pack",
            "NSmnData.json",
            "--out",
            "NSmnData.NOS",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::Ccinf {
                command: CcinfCommand::Pack { .. }
            }
        ));
    }

    #[test]
    fn archive_type_no_longer_accepts_ccinf() {
        let error = Cli::try_parse_from([
            "taletool",
            "archive",
            "inspect",
            "NSmnData.NOS",
            "--type",
            "ccinf",
        ])
        .unwrap_err();
        assert!(error.to_string().contains("invalid value 'ccinf'"));
    }

    #[test]
    fn parses_dedicated_sprite_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "sprite",
            "inspect",
            "2662.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Sprite {
                command: SpriteCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "sprite",
            "unpack",
            "2662.bin",
            "--out",
            "2662.png",
            "--png-only",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Sprite {
                command: SpriteCommand::Unpack { png_only: true, .. }
            }
        ));

        let pack = Cli::try_parse_from(["taletool", "sprite", "pack", "2662", "--out", "2662.bin"])
            .unwrap();
        assert!(matches!(
            pack.command,
            Command::Sprite {
                command: SpriteCommand::Pack { .. }
            }
        ));
    }
}
