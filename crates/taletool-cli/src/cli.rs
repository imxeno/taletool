//! Clap definitions for the `taletool` command tree.
//!
//! This module is deliberately declarative. Command behavior belongs in
//! `commands`, while this file defines the argument surface and value enums
//! that Clap parses.

use std::path::PathBuf;
use std::str::FromStr;

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
        /// Include files that do not match a supported data format.
        #[arg(long)]
        show_unsupported: bool,
        /// Scan only the immediate files in the data directory.
        #[arg(long)]
        no_recursive: bool,
    },
    /// Inspect, unpack, or pack full archive containers.
    Archive {
        #[command(subcommand)]
        command: ArchiveCommand,
    },
    /// Inspect, decode, or encode sprite-animation payloads.
    Animation {
        #[command(subcommand)]
        command: AnimationCommand,
    },
    /// Inspect, decode, or encode map payloads.
    Map {
        #[command(subcommand)]
        command: MapCommand,
    },
    /// Inspect, decode, or encode CCINF GBFC index files.
    Ccinf {
        #[command(subcommand)]
        command: CcinfCommand,
    },
    /// Inspect, decode, or encode effect payloads.
    Effect {
        #[command(subcommand)]
        command: EffectCommand,
    },
    /// Inspect, decode, or encode geometry payloads.
    Geometry {
        #[command(subcommand)]
        command: GeometryCommand,
    },
    /// Inspect, decode, or encode map height-grid payloads.
    HeightGrid {
        #[command(subcommand)]
        command: HeightGridCommand,
    },
    /// Inspect, decode, or encode map-neighborhood payloads.
    MapNeighborhood {
        #[command(subcommand)]
        command: MapNeighborhoodCommand,
    },
    /// Inspect, decode, or encode sprite payloads.
    Sprite {
        #[command(subcommand)]
        command: SpriteCommand,
    },
    /// Inspect, decode, or encode sprite-resource remap payloads.
    SpriteRemap {
        #[command(subcommand)]
        command: SpriteRemapCommand,
    },
    /// Inspect or apply original NosTale patch packages.
    #[command(visible_alias = "pak")]
    Patch {
        #[command(subcommand)]
        command: PatchCommand,
    },
    /// Inspect, decode, or encode text payload files.
    Text {
        #[command(subcommand)]
        command: TextCommand,
    },
    /// Inspect, unpack, or pack sndinfo.lst audio metadata.
    Audio {
        #[command(subcommand)]
        command: AudioCommand,
    },
    /// Inspect, decode, or encode texture payloads.
    Texture {
        #[command(subcommand)]
        command: TextureCommand,
    },
    /// Export extracted map cell-flag payloads.
    CellFlag {
        #[command(subcommand)]
        command: CellFlagCommand,
    },
}

/// Operations for `NSmcData` and `NSpcData` sprite-animation payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum AnimationCommand {
    /// Print playback flags, timing, and frame counts.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a sprite-animation payload into a versioned JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a versioned JSON document into a sprite-animation payload.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for individual `NSpmData` sprite-resource remap payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum SpriteRemapCommand {
    /// Print frame, identity-ordering, and skipped-slot counts.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a sprite-resource remap payload into a JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a JSON document into a sprite-resource remap payload.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for `sndinfo.lst` audio metadata.
#[derive(Debug, Subcommand)]
pub(crate) enum AudioCommand {
    /// Print sound-table entries and optionally resolve files in a wave directory.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        wave_dir: Option<PathBuf>,
    },
    /// Convert sndinfo.lst into an editable JSON manifest.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build sndinfo.lst from a JSON manifest.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for `NStuData` map payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum MapCommand {
    /// Print scene settings, geometry references, and structural counts.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a map payload into a JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a JSON document into a map payload.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for individual map cell-flag payloads extracted from archives.
#[derive(Debug, Subcommand)]
pub(crate) enum CellFlagCommand {
    /// Export an NStc cell grid as a color-keyed or filtered PNG.
    ExportPng {
        payload: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Render cells containing this single flag as black and all others as white.
        #[arg(long)]
        flag: Option<CellFlagArg>,
    },
}

/// A validated single-bit map cell flag supplied on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellFlagArg(u8);

impl CellFlagArg {
    pub(crate) fn bits(self) -> u8 {
        self.0
    }
}

impl FromStr for CellFlagArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = match input.to_ascii_lowercase().as_str() {
            "walking-disabled" => 0x01,
            "attack-through-disabled" => 0x02,
            "unknown-04" => 0x04,
            "monster-aggro-disabled" => 0x08,
            "pvp-disabled" => 0x10,
            _ => parse_cell_flag_number(input)?,
        };

        if value == 0 || !value.is_power_of_two() {
            return Err(format!(
                "cell flag must be one non-zero bit in the range 0x01..=0x80, got {input}"
            ));
        }
        Ok(Self(value))
    }
}

fn parse_cell_flag_number(input: &str) -> Result<u8, String> {
    if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        u8::from_str_radix(hex, 16)
            .map_err(|_| format!("unknown cell flag name or invalid hexadecimal mask: {input}"))
    } else {
        input
            .parse::<u8>()
            .map_err(|_| format!("unknown cell flag name or invalid decimal mask: {input}"))
    }
}

/// Operations for effect definitions and effect animation payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum EffectCommand {
    /// Print effect timing, component, and keyframe metadata.
    Inspect {
        input: PathBuf,
        #[arg(long, value_enum, default_value_t = EffectKindArg::Auto)]
        kind: EffectKindArg,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode an effect payload into a JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = EffectKindArg::Auto)]
        kind: EffectKindArg,
    },
    /// Encode a JSON document into an effect payload.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for `NStgData` and `NStgeData` geometry payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum GeometryCommand {
    /// Print geometry bounds, animation timing, and structural counts.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a geometry payload into a JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a JSON document into a geometry payload.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for `NSgrdData` map height-grid payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum HeightGridCommand {
    /// Print grid identifiers, bounds, dimensions, and structural counts.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a height-grid payload into a JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a JSON document into a height-grid payload.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for `NStkData` map-neighborhood payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum MapNeighborhoodCommand {
    /// Print neighbor-map keys and point-sequence structural counts.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a map-neighborhood payload into a JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a JSON document into a map-neighborhood payload.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
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

/// Operations for CCINF GBFC index files.
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
    /// Decode a CCINF file into a JSON document.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a JSON document into a CCINF file.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

/// Operations for map-object and free-size sprite payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum SpriteCommand {
    /// Print sprite dimensions and format-specific metadata.
    Inspect {
        input: PathBuf,
        #[arg(long, value_enum, default_value_t = SpriteKindArg::Auto)]
        kind: SpriteKindArg,
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
        #[arg(long, value_enum, default_value_t = SpriteKindArg::Auto)]
        kind: SpriteKindArg,
        /// Write a single-frame sprite directly to the output PNG without metadata.
        #[arg(long)]
        png_only: bool,
    },
    /// Encode a map-object sprite directory or free-size sprite PNG.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = SpriteKindArg::Auto)]
        kind: SpriteKindArg,
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

/// Operations for individual text payload files.
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
        /// Select the logical structured-text format instead of inferring it.
        #[arg(long, value_enum, default_value_t = TextFormatArg::Auto)]
        format: TextFormatArg,
        /// Write a structured text JSON document.
        #[arg(long)]
        json: bool,
        /// Override the character encoding inferred from the structured format.
        #[arg(long, requires = "json")]
        encoding: Option<String>,
    },
    /// Encode a plain text file into the selected payload format.
    Pack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = TextPayloadKindArg::Auto)]
        kind: TextPayloadKindArg,
        /// Select the logical structured-text format instead of inferring it.
        #[arg(long, value_enum, default_value_t = TextFormatArg::Auto)]
        format: TextFormatArg,
        /// Read a structured text JSON document.
        #[arg(long)]
        json: bool,
        /// Override the character encoding inferred from the structured format.
        #[arg(long, requires = "json")]
        encoding: Option<String>,
    },
}

/// Operations for texture payloads.
#[derive(Debug, Subcommand)]
pub(crate) enum TextureCommand {
    /// Print texture dimensions, format, mip levels, and optional checksum.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        checksum: bool,
    },
    /// Decode a texture payload into PNG mip levels and `texture.json`.
    Unpack {
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Encode a texture manifest directory into one payload.
    Pack {
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
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

/// Sprite payload format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SpriteKindArg {
    /// Require exactly one supported sprite parser to match.
    Auto,
    /// Use the counted map-object descriptor and `A4R4G4B4` frame format.
    MapObject,
    /// Use the single-image, block-interlaced `A8R8G8B8` format.
    FreeSize,
}

/// Effect payload format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum EffectKindArg {
    /// Require exactly one supported semantic format to match.
    Auto,
    /// Color animation keys from `NSedData`.
    ColorAnimation,
    /// Effect component definitions from `NSeffData`.
    Definition,
    /// Transform animation keys from `NSemData`.
    TransformAnimation,
    /// Texture resource animation keys from `NSesData`.
    TextureAnimation,
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

/// Logical structured formats supported by `taletool text --json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum TextFormatArg {
    /// Infer the structured format from the native payload filename.
    Auto,
    /// Parse or write an NSlang ordered key/value table.
    Lang,
    /// Parse or write an NScli numeric constant-string table.
    Cli,
    /// Parse or write an NSetc ordered string list.
    Etc,
    /// Parse or write a source-oriented NSgtdData record.
    Gtd,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scan_options() {
        let default =
            Cli::try_parse_from(["taletool", "scan", "--data-dir", "NostaleData"]).unwrap();
        assert!(matches!(
            default.command,
            Command::Scan {
                json: false,
                show_unsupported: false,
                no_recursive: false,
                ..
            }
        ));

        let expanded = Cli::try_parse_from([
            "taletool",
            "scan",
            "--data-dir",
            "NostaleData",
            "--json",
            "--show-unsupported",
            "--no-recursive",
        ])
        .unwrap();
        assert!(matches!(
            expanded.command,
            Command::Scan {
                json: true,
                show_unsupported: true,
                no_recursive: true,
                ..
            }
        ));
    }

    #[test]
    fn parses_dedicated_map_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "map",
            "inspect",
            "42.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Map {
                command: MapCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack =
            Cli::try_parse_from(["taletool", "map", "unpack", "42.bin", "--out", "42.json"])
                .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Map {
                command: MapCommand::Unpack { .. }
            }
        ));

        let pack =
            Cli::try_parse_from(["taletool", "map", "pack", "42.json", "--out", "42.bin"]).unwrap();
        assert!(matches!(
            pack.command,
            Command::Map {
                command: MapCommand::Pack { .. }
            }
        ));

        assert!(Cli::try_parse_from(["taletool", "bulk", "inspect", "42.bin"]).is_err());
    }

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
    fn parses_dedicated_geometry_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "geometry",
            "inspect",
            "16777578.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Geometry {
                command: GeometryCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "geometry",
            "unpack",
            "16777578.bin",
            "--out",
            "geometry.json",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Geometry {
                command: GeometryCommand::Unpack { .. }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "geometry",
            "pack",
            "geometry.json",
            "--out",
            "16777578.bin",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::Geometry {
                command: GeometryCommand::Pack { .. }
            }
        ));
    }

    #[test]
    fn parses_dedicated_effect_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "effect",
            "inspect",
            "42.bin",
            "--kind",
            "definition",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Effect {
                command: EffectCommand::Inspect {
                    kind: EffectKindArg::Definition,
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "effect",
            "unpack",
            "42.bin",
            "--out",
            "42.json",
            "--kind",
            "transform-animation",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Effect {
                command: EffectCommand::Unpack {
                    kind: EffectKindArg::TransformAnimation,
                    ..
                }
            }
        ));

        let pack =
            Cli::try_parse_from(["taletool", "effect", "pack", "42.json", "--out", "42.bin"])
                .unwrap();
        assert!(matches!(
            pack.command,
            Command::Effect {
                command: EffectCommand::Pack { .. }
            }
        ));
    }

    #[test]
    fn parses_dedicated_animation_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "animation",
            "inspect",
            "42.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Animation {
                command: AnimationCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "animation",
            "unpack",
            "42.bin",
            "--out",
            "42.json",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Animation {
                command: AnimationCommand::Unpack { .. }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "animation",
            "pack",
            "42.json",
            "--out",
            "42.bin",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::Animation {
                command: AnimationCommand::Pack { .. }
            }
        ));
    }

    #[test]
    fn parses_dedicated_sprite_remap_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "sprite-remap",
            "inspect",
            "42.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::SpriteRemap {
                command: SpriteRemapCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "sprite-remap",
            "unpack",
            "42.bin",
            "--out",
            "42.json",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::SpriteRemap {
                command: SpriteRemapCommand::Unpack { .. }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "sprite-remap",
            "pack",
            "42.json",
            "--out",
            "42.bin",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::SpriteRemap {
                command: SpriteRemapCommand::Pack { .. }
            }
        ));
    }

    #[test]
    fn parses_dedicated_sprite_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "sprite",
            "inspect",
            "2662.bin",
            "--kind",
            "map-object",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Sprite {
                command: SpriteCommand::Inspect {
                    kind: SpriteKindArg::MapObject,
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
            "--kind",
            "map-object",
            "--png-only",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Sprite {
                command: SpriteCommand::Unpack { png_only: true, .. }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "sprite",
            "pack",
            "background.png",
            "--out",
            "background.bin",
            "--kind",
            "free-size",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::Sprite {
                command: SpriteCommand::Pack {
                    kind: SpriteKindArg::FreeSize,
                    ..
                }
            }
        ));

        let auto = Cli::try_parse_from(["taletool", "sprite", "inspect", "sprite.bin"]).unwrap();
        assert!(matches!(
            auto.command,
            Command::Sprite {
                command: SpriteCommand::Inspect {
                    kind: SpriteKindArg::Auto,
                    ..
                }
            }
        ));
    }

    #[test]
    fn parses_dedicated_height_grid_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "height-grid",
            "inspect",
            "2006.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::HeightGrid {
                command: HeightGridCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "height-grid",
            "unpack",
            "2006.bin",
            "--out",
            "2006.json",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::HeightGrid {
                command: HeightGridCommand::Unpack { .. }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "height-grid",
            "pack",
            "2006.json",
            "--out",
            "2006.bin",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::HeightGrid {
                command: HeightGridCommand::Pack { .. }
            }
        ));
    }

    #[test]
    fn parses_dedicated_map_neighborhood_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "map-neighborhood",
            "inspect",
            "2006.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::MapNeighborhood {
                command: MapNeighborhoodCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack = Cli::try_parse_from([
            "taletool",
            "map-neighborhood",
            "unpack",
            "2006.bin",
            "--out",
            "2006.json",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::MapNeighborhood {
                command: MapNeighborhoodCommand::Unpack { .. }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "map-neighborhood",
            "pack",
            "2006.json",
            "--out",
            "2006.bin",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::MapNeighborhood {
                command: MapNeighborhoodCommand::Pack { .. }
            }
        ));

        assert!(
            Cli::try_parse_from(["taletool", "scene-neighborhood", "inspect", "2006.bin"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["taletool", "scene-resource", "inspect", "2006.bin"]).is_err()
        );
    }

    #[test]
    fn parses_dedicated_texture_commands() {
        let inspect = Cli::try_parse_from([
            "taletool",
            "texture",
            "inspect",
            "123.bin",
            "--json",
            "--checksum",
        ])
        .unwrap();
        assert!(matches!(
            inspect.command,
            Command::Texture {
                command: TextureCommand::Inspect {
                    json: true,
                    checksum: true,
                    ..
                }
            }
        ));

        let unpack =
            Cli::try_parse_from(["taletool", "texture", "unpack", "123.bin", "--out", "123"])
                .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Texture {
                command: TextureCommand::Unpack { .. }
            }
        ));

        let pack = Cli::try_parse_from(["taletool", "texture", "pack", "123", "--out", "123.bin"])
            .unwrap();
        assert!(matches!(
            pack.command,
            Command::Texture {
                command: TextureCommand::Pack { .. }
            }
        ));
    }

    #[test]
    fn parses_structured_language_text_commands() {
        let unpack = Cli::try_parse_from([
            "taletool",
            "text",
            "unpack",
            "_code_uk_Item.txt",
            "--out",
            "Item.json",
            "--json",
            "--encoding",
            "windows-1252",
        ])
        .unwrap();
        assert!(matches!(
            unpack.command,
            Command::Text {
                command: TextCommand::Unpack {
                    json: true,
                    encoding: Some(_),
                    format: TextFormatArg::Auto,
                    ..
                }
            }
        ));

        let renamed = Cli::try_parse_from([
            "taletool",
            "text",
            "unpack",
            "Item.txt",
            "--out",
            "Item.json",
            "--json",
            "--format",
            "lang",
            "--encoding",
            "windows-1252",
        ])
        .unwrap();
        assert!(matches!(
            renamed.command,
            Command::Text {
                command: TextCommand::Unpack {
                    json: true,
                    format: TextFormatArg::Lang,
                    ..
                }
            }
        ));

        let nscli = Cli::try_parse_from([
            "taletool",
            "text",
            "unpack",
            "strings.txt",
            "--out",
            "strings.json",
            "--json",
            "--format",
            "cli",
            "--encoding",
            "windows-1252",
        ])
        .unwrap();
        assert!(matches!(
            nscli.command,
            Command::Text {
                command: TextCommand::Unpack {
                    json: true,
                    format: TextFormatArg::Cli,
                    ..
                }
            }
        ));

        let nsetc = Cli::try_parse_from([
            "taletool",
            "text",
            "unpack",
            "renamed.lst",
            "--out",
            "strings.json",
            "--json",
            "--format",
            "etc",
        ])
        .unwrap();
        assert!(matches!(
            nsetc.command,
            Command::Text {
                command: TextCommand::Unpack {
                    json: true,
                    format: TextFormatArg::Etc,
                    ..
                }
            }
        ));

        let gtd = Cli::try_parse_from([
            "taletool",
            "text",
            "unpack",
            "Item.dat",
            "--out",
            "Item.json",
            "--json",
            "--format",
            "gtd",
        ])
        .unwrap();
        assert!(matches!(
            gtd.command,
            Command::Text {
                command: TextCommand::Unpack {
                    json: true,
                    format: TextFormatArg::Gtd,
                    ..
                }
            }
        ));

        let pack = Cli::try_parse_from([
            "taletool",
            "text",
            "pack",
            "Item.json",
            "--out",
            "_code_uk_Item.txt",
            "--json",
        ])
        .unwrap();
        assert!(matches!(
            pack.command,
            Command::Text {
                command: TextCommand::Pack { json: true, .. }
            }
        ));

        assert!(
            Cli::try_parse_from([
                "taletool",
                "text",
                "unpack",
                "_code_uk_Item.txt",
                "--out",
                "Item.txt",
                "--encoding",
                "windows-1252",
            ])
            .is_err()
        );
    }
}
