//! Known binary archive presets and related packing policy.
//!
//! Presets are CLI policy rather than archive-library behavior because they
//! depend on user-facing names, output patterns, and default command options.

use std::path::Path;

use taletool_archive::{BinaryCompression, BinaryNosArchive};
use taletool_effect::EffectAssetKind;
use taletool_zlib::ZlibProfile;

use crate::cli::ChunkingArg;

/// Semantic payload type stored by a recognized binary archive family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryAssetKind {
    Geometry,
    Texture,
    Effect(EffectAssetKind),
    CellFlag,
    Map,
    MapObjectSprite,
    SpriteAnimation,
    SpriteRemap,
    MapNeighborhood,
    HeightGrid,
    FreeSizeSprite,
}

/// CLI defaults for a known binary archive family.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BinaryPreset {
    /// Archive family name used for preset lookup.
    pub(crate) name: &'static str,
    /// Sixteen-byte archive header written to packed archives.
    pub(crate) header: [u8; 16],
    /// Direct index byte from the binary archive header.
    pub(crate) direct_index: u8,
    /// Default compression for entries in this archive family.
    pub(crate) compression: BinaryCompression,
    /// zlib 1.1.2 profile required for byte-compatible zlib output.
    pub(crate) zlib_profile: Option<ZlibProfile>,
    /// Default chunk routing strategy.
    pub(crate) chunking: ChunkingArg,
    /// Default number of archive chunks.
    pub(crate) chunk_count: usize,
    /// Default output filename or chunk filename pattern.
    pub(crate) chunk_format: &'static str,
    /// Semantic payload decoder selected for converted archive unpacking.
    pub(crate) asset_kind: BinaryAssetKind,
}

const BINARY_PRESETS: &[BinaryPreset] = &[
    BinaryPreset {
        name: "NStgData",
        header: *b"NT Data 06\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::LowByte,
        chunk_count: 4,
        chunk_format: "NStgData{chunk:02X}.NOS",
        asset_kind: BinaryAssetKind::Geometry,
    },
    BinaryPreset {
        name: "NStgeData",
        header: *b"NT Data 10\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NStgeData.NOS",
        asset_kind: BinaryAssetKind::Geometry,
    },
    BinaryPreset {
        name: "NStpData",
        header: *b"NT Data 07\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::LowByte,
        chunk_count: 32,
        chunk_format: "NStpData{chunk:02X}.NOS",
        asset_kind: BinaryAssetKind::Texture,
    },
    BinaryPreset {
        name: "NStpeData",
        header: *b"NT Data 11\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::LowByte,
        chunk_count: 8,
        chunk_format: "NStpeData{chunk:02X}.NOS",
        asset_kind: BinaryAssetKind::Texture,
    },
    BinaryPreset {
        name: "NStpuData",
        header: *b"NT Data 12\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::LowByte,
        chunk_count: 4,
        chunk_format: "NStpuData{chunk:02X}.NOS",
        asset_kind: BinaryAssetKind::Texture,
    },
    BinaryPreset {
        name: "NSedData",
        header: *b"NT Data 20\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSedData.NOS",
        asset_kind: BinaryAssetKind::Effect(EffectAssetKind::ColorAnimation),
    },
    BinaryPreset {
        name: "NSemData",
        header: *b"NT Data 21\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSemData.NOS",
        asset_kind: BinaryAssetKind::Effect(EffectAssetKind::TransformAnimation),
    },
    BinaryPreset {
        name: "NSesData",
        header: *b"NT Data 22\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSesData.NOS",
        asset_kind: BinaryAssetKind::Effect(EffectAssetKind::TextureAnimation),
    },
    BinaryPreset {
        name: "NSeffData",
        header: *b"NT Data 23\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSeffData.NOS",
        asset_kind: BinaryAssetKind::Effect(EffectAssetKind::Definition),
    },
    BinaryPreset {
        name: "NStcData",
        header: *b"NT Data 05\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Zlib,
        zlib_profile: Some(ZlibProfile::default_level(9)),
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NStcData.NOS",
        asset_kind: BinaryAssetKind::CellFlag,
    },
    BinaryPreset {
        name: "NStuData",
        header: *b"NT Data 02\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Zlib,
        zlib_profile: Some(ZlibProfile::default_level(9)),
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NStuData.NOS",
        asset_kind: BinaryAssetKind::Map,
    },
    BinaryPreset {
        name: "NSipData",
        header: *b"NT Data 24\0\0\x22\x08\x03 ",
        direct_index: 0,
        compression: BinaryCompression::Zlib,
        zlib_profile: Some(ZlibProfile::default_level(1)),
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSipData.NOS",
        asset_kind: BinaryAssetKind::MapObjectSprite,
    },
    BinaryPreset {
        name: "NSmcData",
        header: *b"NT Data 16\0\0\x15\x07\x04 ",
        direct_index: 1,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSmcData.NOS",
        asset_kind: BinaryAssetKind::SpriteAnimation,
    },
    BinaryPreset {
        name: "NSmpData",
        header: *b"NT Data 17\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Zlib,
        zlib_profile: Some(ZlibProfile::default_level(1)),
        chunking: ChunkingArg::LowByte,
        chunk_count: 16,
        chunk_format: "NSmpData{chunk:02X}.NOS",
        asset_kind: BinaryAssetKind::MapObjectSprite,
    },
    BinaryPreset {
        name: "NSppData",
        header: *b"NT Data 14\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Zlib,
        zlib_profile: Some(ZlibProfile::default_level(1)),
        chunking: ChunkingArg::LowByte,
        chunk_count: 32,
        chunk_format: "NSppData{chunk:02X}.NOS",
        asset_kind: BinaryAssetKind::MapObjectSprite,
    },
    BinaryPreset {
        name: "NSpcData",
        header: *b"NT Data 13\0\0\x15\x07\x04 ",
        direct_index: 1,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSpcData.NOS",
        asset_kind: BinaryAssetKind::SpriteAnimation,
    },
    BinaryPreset {
        name: "NSpmData",
        header: *b"NT Data 15\0\0\x15\x07\x04 ",
        direct_index: 1,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSpmData.NOS",
        asset_kind: BinaryAssetKind::SpriteRemap,
    },
    BinaryPreset {
        name: "NStkData",
        header: *b"NT Data 03\0\0\x15\x07\x04 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NStkData.NOS",
        asset_kind: BinaryAssetKind::MapNeighborhood,
    },
    BinaryPreset {
        name: "NSgrdData",
        header: *b"NT Data 26\0\0\x04\x11\x05 ",
        direct_index: 0,
        compression: BinaryCompression::Raw,
        zlib_profile: None,
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NSgrdData.NOS",
        asset_kind: BinaryAssetKind::HeightGrid,
    },
    BinaryPreset {
        name: "NS4BbData",
        header: *b"32GBS V1.0\x1A\0\x08\x09\x03 ",
        direct_index: 0,
        compression: BinaryCompression::Zlib,
        zlib_profile: Some(ZlibProfile::default_level(9)),
        chunking: ChunkingArg::Single,
        chunk_count: 1,
        chunk_format: "NS4BbData.NOS",
        asset_kind: BinaryAssetKind::FreeSizeSprite,
    },
];

/// Resolve a preset from an explicit `--preset` value or an output path.
pub(crate) fn resolve_binary_preset(out: &str, preset_arg: &str) -> Option<BinaryPreset> {
    if preset_arg.eq_ignore_ascii_case("none") {
        return None;
    }
    if !preset_arg.eq_ignore_ascii_case("auto") {
        return BINARY_PRESETS
            .iter()
            .find(|preset| preset.name.eq_ignore_ascii_case(preset_arg))
            .copied();
    }
    let name = Path::new(out)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(out);
    let name = name
        .replace("{chunk:02X}", "")
        .replace("{chunk:02x}", "")
        .replace("{chunk}", "");
    let stem = Path::new(&name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&name);
    let stem = if stem.len() > 2
        && stem[stem.len() - 2..]
            .chars()
            .all(|c| c.is_ascii_hexdigit())
    {
        &stem[..stem.len() - 2]
    } else {
        stem
    };
    BINARY_PRESETS
        .iter()
        .find(|preset| preset.name.eq_ignore_ascii_case(stem))
        .copied()
}

/// Return the default compression to omit from unpacked payload filenames.
pub(crate) fn binary_nos_archive_default_compression(
    archive: &BinaryNosArchive,
) -> BinaryCompression {
    if let Some(preset) = resolve_binary_nos_preset_for_archive_lenient(archive) {
        return preset.compression;
    }
    infer_default_compression(archive.entries().iter().map(|entry| entry.compression))
}

/// Resolve one semantic preset for every chunk in a converted binary archive set.
pub(crate) fn resolve_binary_nos_preset_for_archives(
    archives: &[BinaryNosArchive],
) -> anyhow::Result<BinaryPreset> {
    let Some((first, rest)) = archives.split_first() else {
        anyhow::bail!("cannot identify an empty binary archive set");
    };
    let expected = resolve_binary_nos_preset_for_archive_strict(first)?;
    for archive in rest {
        let actual = resolve_binary_nos_preset_for_archive_strict(archive)?;
        if actual.name != expected.name {
            anyhow::bail!(
                "archive chunk {} identifies family {}, but the archive set started as {}",
                archive.path().display(),
                actual.name,
                expected.name,
            );
        }
    }
    Ok(expected)
}

/// Require a binary archive name and header to identify one consistent family.
fn resolve_binary_nos_preset_for_archive_strict(
    archive: &BinaryNosArchive,
) -> anyhow::Result<BinaryPreset> {
    let by_name = resolve_binary_preset(&archive.path().to_string_lossy(), "auto");
    let by_header = binary_preset_for_header(archive.header(), archive.direct_index());
    match (by_name, by_header) {
        (Some(named), Some(header)) if named.name == header.name => Ok(named),
        (Some(named), Some(header)) => anyhow::bail!(
            "archive {} filename identifies family {}, but its header identifies {}",
            archive.path().display(),
            named.name,
            header.name,
        ),
        (Some(named), None) => anyhow::bail!(
            "archive {} filename identifies family {}, but header {} with direct index {} does not match it",
            archive.path().display(),
            named.name,
            hex::encode(archive.header()),
            archive.direct_index(),
        ),
        (None, Some(header)) => Ok(header),
        (None, None) => anyhow::bail!(
            "archive {} has no recognized binary family; --convert requires a supported family",
            archive.path().display(),
        ),
    }
}

fn binary_preset_for_header(header: [u8; 16], direct_index: u8) -> Option<BinaryPreset> {
    BINARY_PRESETS
        .iter()
        .find(|preset| preset.header == header && preset.direct_index == direct_index)
        .copied()
}

/// Resolve a preset from archive path first, then from header/direct-index data.
fn resolve_binary_nos_preset_for_archive_lenient(
    archive: &BinaryNosArchive,
) -> Option<BinaryPreset> {
    resolve_binary_preset(&archive.path().to_string_lossy(), "auto")
        .or_else(|| binary_preset_for_header(archive.header(), archive.direct_index()))
}

/// Pick a default compression for unknown archives from observed entries.
fn infer_default_compression(
    compressions: impl IntoIterator<Item = BinaryCompression>,
) -> BinaryCompression {
    let mut raw = 0usize;
    let mut zlib = 0usize;
    for compression in compressions {
        match compression {
            BinaryCompression::Raw => raw += 1,
            BinaryCompression::Zlib => zlib += 1,
        }
    }
    if raw > zlib {
        BinaryCompression::Raw
    } else {
        BinaryCompression::Zlib
    }
}

/// Resolve the zlib profile for a binary archive pack operation.
pub(crate) fn resolve_zlib_profile(
    profile_arg: &str,
    compression: BinaryCompression,
    preset: Option<&BinaryPreset>,
) -> anyhow::Result<ZlibProfile> {
    if !matches!(compression, BinaryCompression::Zlib) {
        return Ok(ZlibProfile::default_level(9));
    }
    if profile_arg.eq_ignore_ascii_case("auto") {
        return preset
            .and_then(|preset| preset.zlib_profile)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "zlib archive pack needs --zlib-profile or a preset with a zlib112 profile"
                )
            });
    }
    profile_arg
        .parse::<ZlibProfile>()
        .map_err(|error| anyhow::anyhow!("invalid --zlib-profile {profile_arg:?}: {error}"))
}

/// Resolve the final output filename pattern for binary archive packing.
pub(crate) fn output_pattern(
    out: &str,
    chunk_format: Option<&str>,
    preset: Option<&BinaryPreset>,
    chunk_count: usize,
) -> anyhow::Result<String> {
    if let Some(format) = chunk_format {
        let base = Path::new(out);
        return Ok(base.join(format).to_string_lossy().into_owned());
    }
    if out.contains("{chunk") {
        return Ok(out.to_owned());
    }
    let path = Path::new(out);
    let looks_like_dir = path.extension().is_none();
    if let (true, Some(preset)) = (looks_like_dir, preset) {
        return Ok(path
            .join(preset.chunk_format)
            .to_string_lossy()
            .into_owned());
    }
    if chunk_count > 1 && !out.contains("{chunk") {
        anyhow::bail!(
            "split archive output needs --chunk-format or an --out pattern with {{chunk:02X}}"
        );
    }
    Ok(out.to_owned())
}

/// Substitute one chunk index into a chunk filename pattern.
pub(crate) fn format_chunk_pattern(pattern: &str, chunk: usize) -> String {
    pattern
        .replace("{chunk:02X}", &format!("{chunk:02X}"))
        .replace("{chunk:02x}", &format!("{chunk:02x}"))
        .replace("{chunk}", &chunk.to_string())
}

/// Parse the 16-byte `--header-hex` option.
pub(crate) fn parse_header_hex(value: &str) -> anyhow::Result<[u8; 16]> {
    let clean = value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_')
        .collect::<String>();
    if clean.len() != 32 {
        anyhow::bail!("--header-hex must contain exactly 16 bytes");
    }
    let mut bytes = [0_u8; 16];
    for index in 0..16 {
        bytes[index] = u8::from_str_radix(&clean[index * 2..index * 2 + 2], 16)?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use taletool_archive::{
        BinaryNosArchiveWriteEntry, BinaryNosArchiveWriteOptions, write_binary_nos_archive_bytes,
    };

    use super::*;

    fn binary_nos_archive_with_compressions(
        path: &str,
        compressions: &[BinaryCompression],
    ) -> BinaryNosArchive {
        binary_nos_archive_with_header(path, *b"Unknown NOS fmt!", compressions)
    }

    fn binary_nos_archive_with_header(
        path: &str,
        header: [u8; 16],
        compressions: &[BinaryCompression],
    ) -> BinaryNosArchive {
        let entries = compressions
            .iter()
            .enumerate()
            .map(|(index, compression)| BinaryNosArchiveWriteEntry {
                file_id: index as i32,
                compression: Some(*compression),
                data: vec![index as u8],
            })
            .collect::<Vec<_>>();
        let data = write_binary_nos_archive_bytes(
            &entries,
            &BinaryNosArchiveWriteOptions {
                header,
                direct_index: 0,
                compression: BinaryCompression::Zlib,
                zlib_profile: ZlibProfile::default_level(9),
            },
        )
        .unwrap();
        BinaryNosArchive::from_bytes(PathBuf::from(path), data).unwrap()
    }

    fn empty_binary_nos_archive(
        path: &str,
        header: [u8; 16],
        direct_index: u8,
    ) -> BinaryNosArchive {
        let data = write_binary_nos_archive_bytes(
            &[],
            &BinaryNosArchiveWriteOptions {
                header,
                direct_index,
                compression: BinaryCompression::Raw,
                zlib_profile: ZlibProfile::default_level(9),
            },
        )
        .unwrap();
        BinaryNosArchive::from_bytes(PathBuf::from(path), data).unwrap()
    }

    #[test]
    fn strict_conversion_resolver_maps_every_supported_family() {
        let expected = [
            ("NStgData", BinaryAssetKind::Geometry),
            ("NStgeData", BinaryAssetKind::Geometry),
            ("NStpData", BinaryAssetKind::Texture),
            ("NStpeData", BinaryAssetKind::Texture),
            ("NStpuData", BinaryAssetKind::Texture),
            (
                "NSedData",
                BinaryAssetKind::Effect(EffectAssetKind::ColorAnimation),
            ),
            (
                "NSemData",
                BinaryAssetKind::Effect(EffectAssetKind::TransformAnimation),
            ),
            (
                "NSesData",
                BinaryAssetKind::Effect(EffectAssetKind::TextureAnimation),
            ),
            (
                "NSeffData",
                BinaryAssetKind::Effect(EffectAssetKind::Definition),
            ),
            ("NStcData", BinaryAssetKind::CellFlag),
            ("NStuData", BinaryAssetKind::Map),
            ("NSipData", BinaryAssetKind::MapObjectSprite),
            ("NSmcData", BinaryAssetKind::SpriteAnimation),
            ("NSmpData", BinaryAssetKind::MapObjectSprite),
            ("NSppData", BinaryAssetKind::MapObjectSprite),
            ("NSpcData", BinaryAssetKind::SpriteAnimation),
            ("NSpmData", BinaryAssetKind::SpriteRemap),
            ("NStkData", BinaryAssetKind::MapNeighborhood),
            ("NSgrdData", BinaryAssetKind::HeightGrid),
            ("NS4BbData", BinaryAssetKind::FreeSizeSprite),
        ];

        assert_eq!(expected.len(), BINARY_PRESETS.len());
        for (name, asset_kind) in expected {
            let preset = resolve_binary_preset(&format!("{name}.NOS"), "auto").unwrap();
            assert_eq!(preset.asset_kind, asset_kind, "{name}");
            let named = empty_binary_nos_archive(
                &format!("{name}.NOS"),
                preset.header,
                preset.direct_index,
            );
            let renamed =
                empty_binary_nos_archive("renamed.NOS", preset.header, preset.direct_index);
            assert_eq!(
                resolve_binary_nos_preset_for_archives(&[named])
                    .unwrap()
                    .name,
                name
            );
            assert_eq!(
                resolve_binary_nos_preset_for_archives(&[renamed])
                    .unwrap()
                    .name,
                name
            );
        }
    }

    #[test]
    fn strict_conversion_resolver_rejects_conflicts_and_unknown_families() {
        let nstg = resolve_binary_preset("NStgData.NOS", "auto").unwrap();
        let nstp = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let conflicting = empty_binary_nos_archive("NStpData.NOS", nstg.header, nstg.direct_index);
        let error = resolve_binary_nos_preset_for_archives(&[conflicting])
            .unwrap_err()
            .to_string();
        assert!(error.contains("filename identifies family NStpData"));
        assert!(error.contains("header identifies NStgData"));

        let wrong_header =
            empty_binary_nos_archive("NStpData.NOS", *b"Unknown NOS fmt!", nstp.direct_index);
        assert!(
            resolve_binary_nos_preset_for_archives(&[wrong_header])
                .unwrap_err()
                .to_string()
                .contains("does not match it")
        );

        for (path, header) in [
            ("NStsData.NOS", *b"NT Data 09\0\0\x15\x07\x04 "),
            ("custom.NOS", *b"Unknown NOS fmt!"),
        ] {
            let archive = empty_binary_nos_archive(path, header, 0);
            assert!(
                resolve_binary_nos_preset_for_archives(&[archive])
                    .unwrap_err()
                    .to_string()
                    .contains("no recognized binary family"),
                "{path}"
            );
        }
    }

    #[test]
    fn strict_conversion_resolver_rejects_inconsistent_chunks() {
        let nstg = resolve_binary_preset("NStgData.NOS", "auto").unwrap();
        let nstp = resolve_binary_preset("NStpData.NOS", "auto").unwrap();
        let archives = [
            empty_binary_nos_archive("renamed-a.NOS", nstg.header, nstg.direct_index),
            empty_binary_nos_archive("renamed-b.NOS", nstp.header, nstp.direct_index),
        ];
        let error = resolve_binary_nos_preset_for_archives(&archives)
            .unwrap_err()
            .to_string();
        assert!(error.contains("identifies family NStpData"));
        assert!(error.contains("started as NStgData"));
    }

    #[test]
    fn binary_nos_archive_default_compression_prefers_known_profile() {
        let archive = binary_nos_archive_with_compressions(
            "NStpData00.NOS",
            &[
                BinaryCompression::Zlib,
                BinaryCompression::Zlib,
                BinaryCompression::Raw,
            ],
        );

        assert_eq!(
            binary_nos_archive_default_compression(&archive),
            BinaryCompression::Raw
        );
    }

    #[test]
    fn binary_nos_archive_default_compression_prefers_known_header_profile() {
        let archive = binary_nos_archive_with_header(
            "renamed.NOS",
            *b"NT Data 02\0\0\x15\x07\x04 ",
            &[BinaryCompression::Raw, BinaryCompression::Raw],
        );

        assert_eq!(
            binary_nos_archive_default_compression(&archive),
            BinaryCompression::Zlib
        );
    }

    #[test]
    fn binary_presets_define_zlib112_profiles_for_known_zlib_archives() {
        for (name, level, chunk_count) in [
            ("NSipData", 1, 1),
            ("NSmpData", 1, 16),
            ("NSppData", 1, 32),
            ("NStuData", 9, 1),
            ("NStcData", 9, 1),
            ("NS4BbData", 9, 1),
        ] {
            let preset = resolve_binary_preset(&format!("{name}.NOS"), "auto").unwrap();
            assert_eq!(preset.compression, BinaryCompression::Zlib);
            assert_eq!(preset.zlib_profile, Some(ZlibProfile::default_level(level)));
            assert_eq!(preset.chunk_count, chunk_count);
        }
    }

    #[test]
    fn effect_archive_presets_are_raw_single_file_archives() {
        for (name, data_number) in [
            ("NSedData", b"20"),
            ("NSemData", b"21"),
            ("NSesData", b"22"),
            ("NSeffData", b"23"),
        ] {
            let preset = resolve_binary_preset(&format!("{name}.NOS"), "auto").unwrap();
            assert_eq!(preset.compression, BinaryCompression::Raw);
            assert_eq!(preset.chunking, ChunkingArg::Single);
            assert_eq!(preset.chunk_count, 1);
            assert_eq!(&preset.header[8..10], data_number);
        }
    }

    #[test]
    fn animation_and_remap_presets_define_raw_single_file_archives() {
        for (name, header) in [
            ("NSmcData", *b"NT Data 16\0\0\x15\x07\x04 "),
            ("NSpcData", *b"NT Data 13\0\0\x15\x07\x04 "),
            ("NSpmData", *b"NT Data 15\0\0\x15\x07\x04 "),
        ] {
            let preset = resolve_binary_preset(&format!("{name}.NOS"), "auto").unwrap();
            assert_eq!(preset.header, header);
            assert_eq!(preset.direct_index, 1);
            assert_eq!(preset.compression, BinaryCompression::Raw);
            assert_eq!(preset.zlib_profile, None);
            assert_eq!(preset.chunking, ChunkingArg::Single);
            assert_eq!(preset.chunk_count, 1);
            assert_eq!(preset.chunk_format, format!("{name}.NOS"));
        }
    }

    #[test]
    fn resolves_zlib_profile_from_preset_or_explicit_value() {
        let preset = resolve_binary_preset("NSipData.NOS", "auto").unwrap();

        assert_eq!(
            resolve_zlib_profile("auto", BinaryCompression::Zlib, Some(&preset)).unwrap(),
            ZlibProfile::default_level(1)
        );
        assert_eq!(
            resolve_zlib_profile(
                "zlib112-level9-huffman",
                BinaryCompression::Zlib,
                Some(&preset)
            )
            .unwrap(),
            "zlib112-level9-huffman".parse::<ZlibProfile>().unwrap()
        );
        assert!(
            resolve_zlib_profile("miniz-level9-fixed", BinaryCompression::Zlib, Some(&preset))
                .is_err()
        );
        assert!(resolve_zlib_profile("auto", BinaryCompression::Zlib, None).is_err());
        assert_eq!(
            resolve_zlib_profile("auto", BinaryCompression::Raw, None).unwrap(),
            ZlibProfile::default_level(9)
        );
    }

    #[test]
    fn binary_nos_archive_default_compression_uses_uniform_unknown_archive() {
        let archive = binary_nos_archive_with_compressions(
            "fixture.NOS",
            &[BinaryCompression::Raw, BinaryCompression::Raw],
        );

        assert_eq!(
            binary_nos_archive_default_compression(&archive),
            BinaryCompression::Raw
        );
    }

    #[test]
    fn binary_nos_archive_default_compression_uses_majority_or_zlib_tie_for_unknown_archive() {
        let majority = binary_nos_archive_with_compressions(
            "fixture.NOS",
            &[
                BinaryCompression::Zlib,
                BinaryCompression::Zlib,
                BinaryCompression::Raw,
            ],
        );
        let tie = binary_nos_archive_with_compressions(
            "fixture.NOS",
            &[BinaryCompression::Raw, BinaryCompression::Zlib],
        );

        assert_eq!(
            binary_nos_archive_default_compression(&majority),
            BinaryCompression::Zlib
        );
        assert_eq!(
            binary_nos_archive_default_compression(&tie),
            BinaryCompression::Zlib
        );
    }
}
