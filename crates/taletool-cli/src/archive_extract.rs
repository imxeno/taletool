//! Selected binary archive extraction using NosTale's chunk and table lookup.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::{Digest, Sha256};
use taletool_archive::{BinaryNosArchive, BinaryNosArchiveEntry, BinaryNosChunkRouting};

use crate::archive_convert::{convert_binary_payload, write_output_transactionally};
use crate::binary_preset::resolve_binary_nos_preset_for_archives;
use crate::cli::parse_archive_id;
use crate::paths::binary_family_stem;

/// Read and deduplicate requested IDs, retaining their first-requested order.
pub(crate) fn requested_ids(ids: &[i32], file: Option<&Path>) -> anyhow::Result<Vec<i32>> {
    let mut result = ids.to_vec();
    if let Some(file) = file {
        let contents = fs::read_to_string(file)
            .with_context(|| format!("reading IDs from {}", file.display()))?;
        for (index, line) in contents.lines().enumerate() {
            let line = line.split('#').next().unwrap_or_default();
            for token in line.split_whitespace() {
                result.push(
                    parse_archive_id(token)
                        .map_err(anyhow::Error::msg)
                        .with_context(|| format!("{}:{}", file.display(), index + 1))?,
                );
            }
        }
    }
    let mut seen = BTreeSet::new();
    result.retain(|id| seen.insert(*id));
    if result.is_empty() {
        anyhow::bail!("no IDs requested; supply --id or a non-empty --ids file");
    }
    Ok(result)
}

/// Route only among explicitly supplied archives, retaining missing chunk slots.
struct ArchiveSelection<'a> {
    archives: &'a [BinaryNosArchive],
    single: Option<usize>,
    slots: BTreeMap<usize, usize>,
    routing: BinaryNosChunkRouting,
}

impl<'a> ArchiveSelection<'a> {
    fn new(archives: &'a [BinaryNosArchive]) -> anyhow::Result<Self> {
        let first = archives.first().context("no binary archives supplied")?;
        let family = binary_family_stem(first.path()).context("archive has no UTF-8 filename")?;
        let mut single = None;
        for (index, archive) in archives.iter().enumerate() {
            let actual =
                binary_family_stem(archive.path()).context("archive has no UTF-8 filename")?;
            if !actual.eq_ignore_ascii_case(&family) {
                anyhow::bail!("matched multiple binary archive families: {family}, {actual}");
            }
            if archive.path().file_stem().and_then(|stem| stem.to_str()) == Some(&actual) {
                if single.is_some() {
                    anyhow::bail!("multiple unsuffixed archives supplied for family {family}");
                }
                single = Some(index);
            }
        }

        let mut slots = BTreeMap::new();
        if single.is_none() {
            for (index, archive) in archives.iter().enumerate() {
                let stem = archive
                    .path()
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .context("archive has no UTF-8 filename")?;
                let chunk = usize::from_str_radix(&stem[family.len()..], 16)?;
                if slots.insert(chunk, index).is_some() {
                    anyhow::bail!("multiple archives supplied for {family} chunk {chunk:02X}");
                }
            }
        }
        Ok(Self {
            archives,
            single,
            slots,
            routing: BinaryNosChunkRouting::for_family(&family),
        })
    }

    fn active_archives(&self) -> &[BinaryNosArchive] {
        match self.single {
            Some(index) => std::slice::from_ref(&self.archives[index]),
            None => self.archives,
        }
    }

    fn find(&self, id: i32) -> Option<(&BinaryNosArchive, &BinaryNosArchiveEntry)> {
        let index = self
            .single
            .or_else(|| self.slots.get(&self.routing.chunk_index(id)).copied())?;
        let archive = &self.archives[index];
        archive.find_entry(id).map(|entry| (archive, entry))
    }
}

struct ExtractedPayload {
    archive: PathBuf,
    stored_id: i32,
    stored_size: usize,
    size: usize,
    sha256: String,
    relative_path: PathBuf,
    warnings: Vec<String>,
}

struct ExtractionResult {
    id: i32,
    payload: Option<ExtractedPayload>,
}

/// Extract and report selected payloads; missing IDs are successful lookup results.
pub(crate) fn extract_binary_archives(
    archives: &[BinaryNosArchive],
    ids: &[i32],
    out: &Path,
    convert: bool,
) -> anyhow::Result<()> {
    let results = extract_payloads(archives, ids, out, convert)?;
    let mut found = 0;
    for result in &results {
        if let Some(payload) = &result.payload {
            found += 1;
            for warning in &payload.warnings {
                eprintln!("{warning}");
            }
            println!(
                "found id={} size={} sha256={} stored_id={} stored_size={} archive={} out={}",
                result.id,
                payload.size,
                payload.sha256,
                payload.stored_id,
                payload.stored_size,
                payload.archive.display(),
                out.join(&payload.relative_path).display(),
            );
        } else {
            println!("missing id={} size=- sha256=-", result.id);
        }
    }
    println!("found={found} missing={}", results.len() - found);
    Ok(())
}

fn extract_payloads(
    archives: &[BinaryNosArchive],
    ids: &[i32],
    out: &Path,
    convert: bool,
) -> anyhow::Result<Vec<ExtractionResult>> {
    let selection = ArchiveSelection::new(archives)?;
    let preset = convert
        .then(|| resolve_binary_nos_preset_for_archives(selection.active_archives()))
        .transpose()?;
    write_output_transactionally(out, |staging| {
        let mut results = Vec::with_capacity(ids.len());
        for &id in ids {
            let Some((archive, entry)) = selection.find(id) else {
                results.push(ExtractionResult { id, payload: None });
                continue;
            };
            let payload = archive.read_entry_payload(entry).with_context(|| {
                format!(
                    "reading requested ID {id} from {}",
                    archive.path().display()
                )
            })?;
            let file_name = format!("{id}.bin");
            let (relative_path, warnings) = if let Some(preset) = preset {
                let converted =
                    convert_binary_payload(&payload.data, preset.asset_kind, staging, &file_name)
                        .with_context(|| {
                        format!(
                            "converting requested ID {id} from {}",
                            archive.path().display()
                        )
                    })?;
                (converted.relative_path, converted.warnings)
            } else {
                fs::write(staging.join(&file_name), &payload.data)
                    .with_context(|| format!("writing requested ID {id}"))?;
                (PathBuf::from(file_name), Vec::new())
            };
            results.push(ExtractionResult {
                id,
                payload: Some(ExtractedPayload {
                    archive: archive.path().to_path_buf(),
                    stored_id: entry.file_id,
                    stored_size: entry.stored_size,
                    size: payload.data.len(),
                    sha256: hex::encode(Sha256::digest(&payload.data)),
                    relative_path,
                    warnings,
                }),
            });
        }
        Ok(results)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use image::RgbaImage;
    use taletool_archive::{
        BinaryCompression, BinaryNosArchiveWriteEntry, BinaryNosArchiveWriteOptions,
    };
    use taletool_texture::sprite::free_size::write_free_size_sprite_bytes;
    use taletool_zlib::ZlibProfile;

    use crate::binary_preset::resolve_binary_preset;

    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "taletool-extract-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn out(&self) -> PathBuf {
            self.0.join("out")
        }

        fn assert_no_output(&self) {
            assert!(!self.out().exists());
            assert!(fs::read_dir(&self.0).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".staging")
            }));
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn archive(name: &str, ids: &[i32], direct_index: u8) -> BinaryNosArchive {
        archive_with_entries(
            name,
            ids.iter()
                .enumerate()
                .map(|(index, &id)| BinaryNosArchiveWriteEntry::new(id, vec![index as u8]))
                .collect(),
            direct_index,
        )
    }

    fn archive_with_entries(
        name: &str,
        entries: Vec<BinaryNosArchiveWriteEntry>,
        direct_index: u8,
    ) -> BinaryNosArchive {
        let header = resolve_binary_preset(name, "auto").map_or([0; 16], |preset| preset.header);
        BinaryNosArchive::from_entries(
            name,
            entries,
            &BinaryNosArchiveWriteOptions::new(
                header,
                direct_index,
                BinaryCompression::Raw,
                ZlibProfile::default_level(9),
            ),
        )
        .unwrap()
    }

    #[test]
    fn id_file_preserves_order_and_deduplicates_unsigned_aliases() {
        let fixture = Fixture::new();
        let file = fixture.0.join("ids.txt");
        fs::write(
            &file,
            "# selected IDs\r\n\r\n42\t7 42 # duplicate\r\n-1 4294967295 0xFFFFFFFF\n0x80000000\n",
        )
        .unwrap();
        assert_eq!(
            requested_ids(&[], Some(&file)).unwrap(),
            [42, 7, -1, i32::MIN]
        );
        assert_eq!(requested_ids(&[42, 7, 42], None).unwrap(), [42, 7]);

        fs::write(&file, "42\n4294967296\n").unwrap();
        let error = format!("{:#}", requested_ids(&[], Some(&file)).unwrap_err());
        assert!(error.contains("ids.txt:2"));
        assert!(error.contains("4294967296"));
        fs::write(&file, "# no IDs\n \n").unwrap();
        assert!(
            requested_ids(&[], Some(&file))
                .unwrap_err()
                .to_string()
                .contains("no IDs requested")
        );
    }

    #[test]
    fn direct_index_aliases_use_requested_filenames_and_report_stored_ids() {
        let fixture = Fixture::new();
        let archives = [archive("custom.NOS", &[42, 99], 255)];
        let results =
            extract_payloads(&archives, &[0, 1, 42, 99, 2], &fixture.out(), false).unwrap();
        for (id, stored_id, byte) in [(0, 42, 0), (1, 99, 1), (42, 42, 0), (99, 99, 1)] {
            let result = results.iter().find(|result| result.id == id).unwrap();
            assert_eq!(result.payload.as_ref().unwrap().stored_id, stored_id);
            assert_eq!(
                fs::read(fixture.out().join(format!("{id}.bin"))).unwrap(),
                [byte]
            );
        }
        assert!(results[4].payload.is_none());
        assert!(!fixture.out().join("2.bin").exists());
    }

    #[test]
    fn extraction_uses_unsigned_search_and_first_midpoint_duplicate() {
        let fixture = Fixture::new();
        let results = extract_payloads(
            &[archive("custom.NOS", &[7, 7, 7, 7, i32::MIN, -1], 0)],
            &[7, i32::MIN, -1],
            &fixture.out(),
            false,
        )
        .unwrap();
        assert!(results.iter().all(|result| result.payload.is_some()));
        assert_eq!(fs::read(fixture.out().join("7.bin")).unwrap(), [2]);
        assert_eq!(
            fs::read(fixture.out().join("-2147483648.bin")).unwrap(),
            [4]
        );
        assert_eq!(fs::read(fixture.out().join("-1.bin")).unwrap(), [5]);
    }

    #[test]
    fn extraction_does_not_scan_an_unsorted_table() {
        let fixture = Fixture::new();
        let results = extract_payloads(
            &[archive("custom.NOS", &[-1, 1], 0)],
            &[1],
            &fixture.out(),
            false,
        )
        .unwrap();
        assert!(results[0].payload.is_none());
        assert_eq!(fs::read_dir(fixture.out()).unwrap().count(), 0);
    }

    #[test]
    fn split_lookup_retains_holes_and_never_scans_other_chunks() {
        let fixture = Fixture::new();
        let archives = [
            archive("NStgData00.NOS", &[5, 8, 10], 0),
            archive("nstgdata02.nos", &[6, 8], 0),
        ];
        let results = extract_payloads(&archives, &[5, 6, 8, 10], &fixture.out(), false).unwrap();
        assert!(results[0].payload.is_none());
        assert_eq!(
            results[1].payload.as_ref().unwrap().archive,
            Path::new("nstgdata02.nos")
        );
        assert_eq!(fs::read(fixture.out().join("8.bin")).unwrap(), [1]);
        assert!(results[3].payload.is_none());
        assert!(!fixture.out().join("5.bin").exists());
        assert!(!fixture.out().join("10.bin").exists());
    }

    #[test]
    fn lone_numbered_archive_uses_family_bit_selector() {
        for (name, found, missing) in [
            ("NSmpData02.NOS", 0x800, 0x400),
            ("NSppData01.NOS", 0x800, 0x1000),
            ("NStpuData_UK03.NOS", 7, 6),
            ("CustomFF.NOS", -1, -2),
        ] {
            let fixture = Fixture::new();
            let results = extract_payloads(
                &[archive(name, &[found, missing], 1)],
                &[found, missing],
                &fixture.out(),
                false,
            )
            .unwrap();
            assert!(results[0].payload.is_some(), "{name}");
            assert!(results[1].payload.is_none(), "{name}");
        }
    }

    #[test]
    fn unsuffixed_archive_takes_precedence_over_supplied_chunks() {
        let fixture = Fixture::new();
        let results = extract_payloads(
            &[
                archive("NStgData01.NOS", &[5], 0),
                archive("nstgdata.nos", &[6], 0),
            ],
            &[5, 6],
            &fixture.out(),
            false,
        )
        .unwrap();
        assert!(results[0].payload.is_none());
        assert_eq!(
            results[1].payload.as_ref().unwrap().archive,
            Path::new("nstgdata.nos")
        );
    }

    #[test]
    fn hexadecimal_locale_name_is_an_unsuffixed_family() {
        let fixture = Fixture::new();
        let results = extract_payloads(
            &[
                archive("NStpuData_DE.NOS", &[42], 0),
                archive("NStpuData_DE02.NOS", &[43], 0),
            ],
            &[42, 43],
            &fixture.out(),
            false,
        )
        .unwrap();
        assert!(results[0].payload.is_some());
        assert!(results[1].payload.is_none());
        assert_eq!(fs::read(fixture.out().join("42.bin")).unwrap(), [0]);
    }

    #[test]
    fn ambiguous_chunks_and_mixed_families_fail_before_writing() {
        for archives in [
            vec![
                archive("a/NStgData00.NOS", &[0], 0),
                archive("b/NStgData00.NOS", &[0], 0),
            ],
            vec![
                archive("a/NStgData.NOS", &[0], 0),
                archive("b/NStgData.NOS", &[0], 0),
            ],
            vec![
                archive("NStgData.NOS", &[0], 0),
                archive("NStpData.NOS", &[0], 0),
            ],
        ] {
            let fixture = Fixture::new();
            assert!(extract_payloads(&archives, &[0], &fixture.out(), false).is_err());
            fixture.assert_no_output();
        }
    }

    #[test]
    fn compressed_payload_reports_decoded_size_and_sha256() {
        let fixture = Fixture::new();
        let archives = [archive_with_entries(
            "custom.NOS",
            vec![BinaryNosArchiveWriteEntry::with_compression(
                42,
                BinaryCompression::Zlib,
                b"abc".to_vec(),
            )],
            0,
        )];
        let results = extract_payloads(&archives, &[42], &fixture.out(), false).unwrap();
        let payload = results[0].payload.as_ref().unwrap();
        assert_eq!(payload.size, 3);
        assert_eq!(
            payload.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(payload.stored_size, archives[0].entries()[0].stored_size);
        assert_eq!(fs::read(fixture.out().join("42.bin")).unwrap(), b"abc");
    }

    #[test]
    fn conversion_only_decodes_selected_entries_and_keeps_payload_hash() {
        let fixture = Fixture::new();
        let data = write_free_size_sprite_bytes(&RgbaImage::new(2, 3)).unwrap();
        let archives = [archive_with_entries(
            "NS4BbData.NOS",
            vec![
                BinaryNosArchiveWriteEntry::new(42, data.clone()),
                BinaryNosArchiveWriteEntry::new(99, b"invalid unrequested sprite".to_vec()),
            ],
            0,
        )];
        let results = extract_payloads(&archives, &[42, 100], &fixture.out(), true).unwrap();
        let payload = results[0].payload.as_ref().unwrap();
        assert_eq!(payload.relative_path, Path::new("42.png"));
        assert_eq!(payload.size, data.len());
        assert_eq!(payload.sha256, hex::encode(Sha256::digest(&data)));
        assert_eq!(
            image::open(fixture.out().join("42.png")).unwrap().width(),
            2
        );
        assert!(!fixture.out().join("99.png").exists());
        assert!(results[1].payload.is_none());
    }

    #[test]
    fn failed_conversion_rolls_back_every_selected_output() {
        let fixture = Fixture::new();
        let archives = [archive_with_entries(
            "NS4BbData.NOS",
            vec![
                BinaryNosArchiveWriteEntry::new(
                    42,
                    write_free_size_sprite_bytes(&RgbaImage::new(2, 3)).unwrap(),
                ),
                BinaryNosArchiveWriteEntry::new(99, b"invalid sprite".to_vec()),
            ],
            0,
        )];
        let error = extract_payloads(&archives, &[42, 99], &fixture.out(), true)
            .err()
            .unwrap();
        assert!(error.to_string().contains("converting requested ID 99"));
        fixture.assert_no_output();
    }

    #[test]
    fn existing_output_is_preserved_and_unknown_conversion_is_rejected() {
        let fixture = Fixture::new();
        let archives = [archive("custom.NOS", &[42], 0)];
        assert!(extract_payloads(&archives, &[42], &fixture.out(), true).is_err());
        fixture.assert_no_output();
        fs::create_dir(fixture.out()).unwrap();
        fs::write(fixture.out().join("42.bin"), b"keep").unwrap();
        assert!(extract_payloads(&archives, &[42], &fixture.out(), false).is_err());
        assert_eq!(fs::read(fixture.out().join("42.bin")).unwrap(), b"keep");
    }
}
