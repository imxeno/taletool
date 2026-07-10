//! Archive type detection for CLI inputs.
//!
//! Detection stays in the CLI crate because it combines parser attempts with
//! command-line policy, such as whether text archives with trailing bytes are
//! accepted during auto-detection.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use taletool_archive::{BinaryNosArchive, DelDxPack, TextNosArchive};
use taletool_ccinf::{CCINF_HEADER, has_ccinf_header as bytes_have_ccinf_header};

use crate::cli::ArchiveType;
use crate::paths::binary_family_stem;

/// A successfully detected archive input.
pub(crate) enum DetectedArchive {
    /// One or more binary archive chunks from the same archive family.
    Binary(Vec<BinaryNosArchive>),
    /// A single text archive file.
    Text(TextNosArchive),
    /// A single DelDX sound pack.
    Sound(DelDxPack),
}

/// Return whether a file begins with the canonical CCINF signature.
///
/// Dedicated CCINF commands perform complete validation. Archive commands use
/// this shallow check only to redirect the asset before container detection.
pub(crate) fn has_ccinf_header(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0; CCINF_HEADER.len()];
    file.read_exact(&mut header).is_ok() && bytes_have_ccinf_header(&header)
}

/// Detect the archive parser to use for a set of input paths.
///
/// Explicit `ArchiveType` values require that parser to succeed. `Auto` tries
/// all supported container families and fails when both or neither match.
pub(crate) fn detect_archive_paths(
    paths: &[PathBuf],
    archive_type: ArchiveType,
) -> anyhow::Result<DetectedArchive> {
    let binary = if matches!(archive_type, ArchiveType::Auto | ArchiveType::Binary) {
        open_binary_archive_set(paths).ok()
    } else {
        None
    };
    let text = if matches!(archive_type, ArchiveType::Auto | ArchiveType::Text) && paths.len() == 1
    {
        TextNosArchive::open(&paths[0])
            .ok()
            .filter(|archive| archive_type == ArchiveType::Text || archive.trailing_bytes() == 0)
    } else {
        None
    };
    let sound =
        if matches!(archive_type, ArchiveType::Auto | ArchiveType::Sound) && paths.len() == 1 {
            DelDxPack::open(&paths[0]).ok()
        } else {
            None
        };

    match archive_type {
        ArchiveType::Binary => binary
            .map(DetectedArchive::Binary)
            .ok_or_else(|| anyhow::anyhow!("input did not parse as a binary archive")),
        ArchiveType::Text => text
            .map(DetectedArchive::Text)
            .ok_or_else(|| anyhow::anyhow!("input did not parse as a text archive")),
        ArchiveType::Sound => sound
            .map(DetectedArchive::Sound)
            .ok_or_else(|| anyhow::anyhow!("input did not parse as a sound pack")),
        ArchiveType::Auto => {
            let matches = usize::from(binary.is_some())
                + usize::from(text.is_some())
                + usize::from(sound.is_some());
            match (matches, binary, text, sound) {
                (1, Some(binary), _, _) => Ok(DetectedArchive::Binary(binary)),
                (1, _, Some(text), _) => Ok(DetectedArchive::Text(text)),
                (1, _, _, Some(sound)) => Ok(DetectedArchive::Sound(sound)),
                (0, _, _, _) => anyhow::bail!("input did not match a supported archive type"),
                _ => anyhow::bail!("input matched multiple supported archive parsers"),
            }
        }
    }
}

/// Open a set of binary archive chunks and verify they belong to one family.
fn open_binary_archive_set(paths: &[PathBuf]) -> anyhow::Result<Vec<BinaryNosArchive>> {
    if paths.is_empty() {
        anyhow::bail!("no input files matched");
    }
    let mut paths = paths.to_vec();
    paths.sort();
    let mut stems = BTreeSet::new();
    for path in &paths {
        if let Some(stem) = binary_family_stem(path) {
            stems.insert(stem);
        }
    }
    if stems.len() > 1 {
        anyhow::bail!("matched multiple binary archive families: {:?}", stems);
    }

    let mut archives = Vec::with_capacity(paths.len());
    for path in paths {
        let archive = BinaryNosArchive::open(&path)?;
        archives.push(archive);
    }
    Ok(archives)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use taletool_archive::{
        BinaryCompression, BinaryNosArchiveWriteEntry, BinaryNosArchiveWriteOptions,
        DELDX_PACK_HEADER_LEN, DELDX_PACK_ROW_PREFIX_LEN, DelDxPackWriteEntry,
        DelDxPackWriteOptions, write_binary_nos_archive_bytes, write_deldx_pack_bytes,
    };
    use taletool_zlib::ZlibProfile;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn deldx_header() -> [u8; DELDX_PACK_HEADER_LEN] {
        let mut out = [0; DELDX_PACK_HEADER_LEN];
        out[0] = 16;
        out[1..17].copy_from_slice(b"DelDX Pack File ");
        out[0x14..0x18].copy_from_slice(&10i32.to_le_bytes());
        out
    }

    fn deldx_row_prefix(name: &[u8]) -> Vec<u8> {
        let mut out = vec![0; DELDX_PACK_ROW_PREFIX_LEN];
        out[0] = name.len() as u8;
        out[1..1 + name.len()].copy_from_slice(name);
        out
    }

    #[test]
    fn auto_detects_sound_pack_without_breaking_binary_nos() {
        let root = temp_dir("archive-detect");
        fs::create_dir_all(&root).unwrap();
        let sound_path = root.join("snd.pck");
        let sound_bytes = write_deldx_pack_bytes(
            &[DelDxPackWriteEntry::new(
                deldx_row_prefix(b"base.10.wav"),
                b"sound".to_vec(),
            )],
            &DelDxPackWriteOptions::new(deldx_header()),
        )
        .unwrap();
        fs::write(&sound_path, sound_bytes).unwrap();

        let binary_path = root.join("NStgData.NOS");
        let binary_bytes = write_binary_nos_archive_bytes(
            &[BinaryNosArchiveWriteEntry::new(7, b"payload".to_vec())],
            &BinaryNosArchiveWriteOptions::new(
                *b"NT Data 06\0\0\x15\x07\x04 ",
                0,
                BinaryCompression::Raw,
                ZlibProfile::default_level(9),
            ),
        )
        .unwrap();
        fs::write(&binary_path, binary_bytes).unwrap();

        assert!(matches!(
            detect_archive_paths(&[sound_path], ArchiveType::Auto).unwrap(),
            DetectedArchive::Sound(_)
        ));
        assert!(matches!(
            detect_archive_paths(&[binary_path], ArchiveType::Auto).unwrap(),
            DetectedArchive::Binary(_)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
