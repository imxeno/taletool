use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use taletool_archive::deldx::apply_packed_archive_mutation;

use crate::{
    binary_delta::apply_binary_delta,
    binary_nos_update::{apply_binary_nos_archive_update, split_archive_parent_path},
    checksum::sha1_hex,
    extract_ui_eff::{
        EXTRACT_UI_EFF_SHA1, EXTRACT_UI_EFF_TARGET_PATH, ExtractUiEffInput, NSTG_DATA_PATH,
        NSTP_DATA_PATH, apply_extract_ui_eff,
    },
    package::{ParsedPchPkg, PchOperation, PchOperationKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchSourceFile {
    pub path: String,
    pub sha1: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchFile {
    pub path: String,
    pub sha1: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatchChangeSet {
    pub writes: Vec<PatchFile>,
    pub removals: Vec<String>,
}

pub type PatchApplyResult = PatchChangeSet;

#[async_trait]
pub trait PatchSourceLoader {
    async fn load_source(&self, path: &str) -> Result<Option<PatchSourceFile>>;
}

pub async fn apply_patch_operation(
    operation: &PchOperation,
    loader: &impl PatchSourceLoader,
) -> Result<PatchApplyResult> {
    let overlay = ChangeOverlay::default();
    apply_operation_with_overlay(operation, loader, &overlay).await
}

pub async fn apply_patch_package(
    package: &ParsedPchPkg,
    loader: &impl PatchSourceLoader,
) -> Result<PatchApplyResult> {
    let mut overlay = ChangeOverlay::default();
    for operation in &package.operations {
        let changes = apply_operation_with_overlay(operation, loader, &overlay)
            .await
            .with_context(|| {
                format!(
                    "applying package operation {} ({}) for {}",
                    operation.segment.segment_index,
                    operation.op_kind.as_str(),
                    operation.target_path
                )
            })?;
        overlay.apply_change_set(changes);
    }
    Ok(overlay.into_change_set())
}

pub async fn apply_patch_packages(
    packages: &[ParsedPchPkg],
    loader: &impl PatchSourceLoader,
) -> Result<PatchApplyResult> {
    let mut overlay = ChangeOverlay::default();
    for (package_index, package) in packages.iter().enumerate() {
        for operation in &package.operations {
            let changes = apply_operation_with_overlay(operation, loader, &overlay)
                .await
                .with_context(|| {
                    format!(
                        "applying package {package_index} operation {} ({}) for {}",
                        operation.segment.segment_index,
                        operation.op_kind.as_str(),
                        operation.target_path
                    )
                })?;
            overlay.apply_change_set(changes);
        }
    }
    Ok(overlay.into_change_set())
}

async fn apply_operation_with_overlay(
    operation: &PchOperation,
    loader: &impl PatchSourceLoader,
    overlay: &ChangeOverlay,
) -> Result<PatchChangeSet> {
    match operation.op_kind {
        PchOperationKind::DeleteFile => {
            if !operation.payload.is_empty() {
                bail!(
                    "delete operation for {} has unexpected payload of {} bytes",
                    operation.target_path,
                    operation.payload.len()
                );
            }
            Ok(PatchChangeSet::remove(operation.target_path.clone()))
        }
        PchOperationKind::ReplaceFile => Ok(PatchChangeSet::write(PatchFile::from_bytes(
            &operation.target_path,
            operation.payload.clone(),
        ))),
        PchOperationKind::ReplaceAndRun => apply_replace_and_run(operation, loader, overlay).await,
        PchOperationKind::BinaryDelta => {
            let base = require_verified_source(loader, overlay, &operation.target_path)
                .await
                .with_context(|| {
                    format!("loading binary delta base file {}", operation.target_path)
                })?;
            let reconstructed = apply_binary_delta(&base.bytes, &operation.payload)?;
            Ok(PatchChangeSet::write(PatchFile::from_bytes(
                &operation.target_path,
                reconstructed,
            )))
        }
        PchOperationKind::ReplaceAndRelaunch => Ok(PatchChangeSet::write(PatchFile::from_bytes(
            &operation.target_path,
            operation.payload.clone(),
        ))),
        PchOperationKind::PatchInPlace => {
            let candidates = patch_in_place_base_candidates(&operation.target_path);
            let base = require_first_verified_source(loader, overlay, &candidates)
                .await
                .with_context(|| {
                    format!(
                        "loading NOS archive update base from {}",
                        candidates.join(", ")
                    )
                })?;
            let reconstructed = apply_binary_nos_archive_update(&base.bytes, &operation.payload)?;
            Ok(PatchChangeSet::write(PatchFile::from_bytes(
                &operation.target_path,
                reconstructed,
            )))
        }
        PchOperationKind::PackedArchiveMutation => {
            let base = require_verified_source(loader, overlay, &operation.target_path)
                .await
                .with_context(|| {
                    format!("loading packed archive base file {}", operation.target_path)
                })?;
            let reconstructed = apply_packed_archive_mutation(&base.bytes, &operation.payload)?;
            Ok(PatchChangeSet::write(PatchFile::from_bytes(
                &operation.target_path,
                reconstructed,
            )))
        }
        PchOperationKind::Unknown => bail!("unknown operation code {}", operation.op_code),
    }
}

async fn apply_replace_and_run(
    operation: &PchOperation,
    loader: &impl PatchSourceLoader,
    overlay: &ChangeOverlay,
) -> Result<PatchChangeSet> {
    if !operation
        .target_path
        .eq_ignore_ascii_case(EXTRACT_UI_EFF_TARGET_PATH)
        || operation.payload_sha1 != EXTRACT_UI_EFF_SHA1
    {
        bail!("unsupported replace-and-run operation; expected known ExtractUIEff.dat helper");
    }

    let nstg_data = require_verified_source(loader, overlay, NSTG_DATA_PATH)
        .await
        .context("loading NStgData.NOS for ExtractUIEff emulation")?;
    let nstp_data = require_verified_source(loader, overlay, NSTP_DATA_PATH)
        .await
        .context("loading NStpData.NOS for ExtractUIEff emulation")?;

    let output = apply_extract_ui_eff(ExtractUiEffInput {
        nstg_data: &nstg_data.bytes,
        nstp_data: &nstp_data.bytes,
    })?;

    let mut changes = PatchChangeSet::write(PatchFile::from_bytes(
        &operation.target_path,
        operation.payload.clone(),
    ));
    for file in output.files {
        changes.add_write(PatchFile::from_bytes(file.path, file.bytes));
    }
    Ok(changes)
}

async fn require_first_verified_source(
    loader: &impl PatchSourceLoader,
    overlay: &ChangeOverlay,
    candidates: &[String],
) -> Result<PatchSourceFile> {
    for candidate in candidates {
        if let Some(source) = load_verified_source(loader, overlay, candidate).await? {
            return Ok(source);
        }
    }
    bail!(
        "missing source file; expected one of {}",
        candidates.join(", ")
    )
}

async fn require_verified_source(
    loader: &impl PatchSourceLoader,
    overlay: &ChangeOverlay,
    path: &str,
) -> Result<PatchSourceFile> {
    load_verified_source(loader, overlay, path)
        .await?
        .with_context(|| format!("missing source file {path}"))
}

async fn load_verified_source(
    loader: &impl PatchSourceLoader,
    overlay: &ChangeOverlay,
    path: &str,
) -> Result<Option<PatchSourceFile>> {
    if let Some(source) = overlay.source_for_path(path) {
        let Some(source) = source else {
            return Ok(None);
        };
        verify_source(&source)?;
        return Ok(Some(source));
    }

    let Some(source) = loader.load_source(path).await? else {
        return Ok(None);
    };
    verify_source(&source)?;
    Ok(Some(source))
}

fn verify_source(source: &PatchSourceFile) -> Result<()> {
    let actual = sha1_hex(&source.bytes);
    if actual != source.sha1 {
        bail!(
            "base blob checksum mismatch for {}: expected {}, got {}",
            source.path,
            source.sha1,
            actual
        );
    }
    Ok(())
}

fn patch_in_place_base_candidates(target_path: &str) -> Vec<String> {
    let mut candidates = vec![target_path.to_string()];
    if let Some(parent_path) = split_archive_parent_path(target_path)
        && parent_path != target_path
    {
        candidates.push(parent_path);
    }
    candidates
}

fn change_key(path: &str) -> String {
    path.to_ascii_lowercase()
}

#[derive(Debug, Clone)]
enum PendingChange {
    Write(PatchFile),
    Remove(String),
}

#[derive(Debug, Clone, Default)]
struct ChangeOverlay {
    changes: BTreeMap<String, PendingChange>,
}

impl ChangeOverlay {
    fn apply_change_set(&mut self, change_set: PatchChangeSet) {
        for removal in change_set.removals {
            self.remove(removal);
        }
        for write in change_set.writes {
            self.write(write);
        }
    }

    fn source_for_path(&self, path: &str) -> Option<Option<PatchSourceFile>> {
        match self.changes.get(&change_key(path))? {
            PendingChange::Write(file) => Some(Some(PatchSourceFile {
                path: file.path.clone(),
                sha1: file.sha1.clone(),
                bytes: file.bytes.clone(),
            })),
            PendingChange::Remove(_) => Some(None),
        }
    }

    fn write(&mut self, file: PatchFile) {
        self.changes
            .insert(change_key(&file.path), PendingChange::Write(file));
    }

    fn remove(&mut self, path: String) {
        self.changes
            .insert(change_key(&path), PendingChange::Remove(path));
    }

    fn into_change_set(self) -> PatchChangeSet {
        let mut writes = Vec::new();
        let mut removals = Vec::new();
        for change in self.changes.into_values() {
            match change {
                PendingChange::Write(file) => writes.push(file),
                PendingChange::Remove(path) => removals.push(path),
            }
        }
        PatchChangeSet { writes, removals }
    }
}

impl PatchFile {
    pub fn from_bytes(path: &str, bytes: Vec<u8>) -> Self {
        Self {
            path: path.to_string(),
            sha1: sha1_hex(&bytes),
            bytes,
        }
    }
}

impl PatchChangeSet {
    pub fn write(file: PatchFile) -> Self {
        Self {
            writes: vec![file],
            removals: Vec::new(),
        }
    }

    pub fn remove(path: String) -> Self {
        Self {
            writes: Vec::new(),
            removals: vec![path],
        }
    }

    pub fn add_write(&mut self, file: PatchFile) {
        self.removals
            .retain(|path| !path.eq_ignore_ascii_case(&file.path));
        if let Some(existing) = self
            .writes
            .iter_mut()
            .find(|existing| existing.path.eq_ignore_ascii_case(&file.path))
        {
            *existing = file;
        } else {
            self.writes.push(file);
        }
    }

    pub fn add_removal(&mut self, path: String) {
        self.writes
            .retain(|file| !file.path.eq_ignore_ascii_case(&path));
        if !self
            .removals
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&path))
        {
            self.removals.push(path);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.removals.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::package::{PchOperationKind, PchPackageDateTimeCode, PchSegmentHeader};

    #[derive(Default)]
    struct MemoryLoader {
        files: BTreeMap<String, PatchSourceFile>,
    }

    impl MemoryLoader {
        fn insert(&mut self, path: &str, bytes: Vec<u8>) {
            self.files.insert(
                path.to_ascii_lowercase(),
                PatchSourceFile {
                    path: path.to_string(),
                    sha1: sha1_hex(&bytes),
                    bytes,
                },
            );
        }
    }

    #[async_trait]
    impl PatchSourceLoader for MemoryLoader {
        async fn load_source(&self, path: &str) -> Result<Option<PatchSourceFile>> {
            Ok(self.files.get(&path.to_ascii_lowercase()).cloned())
        }
    }

    fn operation(
        op_code: u8,
        op_kind: PchOperationKind,
        target_path: &str,
        payload: &[u8],
        payload_sha1: &str,
    ) -> PchOperation {
        PchOperation {
            segment: PchSegmentHeader {
                segment_index: 0,
                segment_id: 0,
                segment_offset: 0,
                body_offset: 0,
                segment_datetime: PchPackageDateTimeCode::from_raw(0),
                decoded_size: payload.len(),
                encoded_size: payload.len(),
                compressed: false,
            },
            op_code,
            op_kind,
            raw_target_path: target_path.to_string(),
            target_path: target_path.to_string(),
            payload: payload.to_vec(),
            payload_sha1: payload_sha1.to_string(),
        }
    }

    fn archive_entry(stored_len_offset: usize, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0x12, 0x07, 0x04, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        out[stored_len_offset..stored_len_offset + 4]
            .copy_from_slice(&(payload.len() as u32).to_le_bytes());
        if stored_len_offset == 8 {
            out[12] = 1;
        }
        out.extend_from_slice(payload);
        out
    }

    fn archive(ids: &[i32], entries: &[Vec<u8>]) -> Vec<u8> {
        assert_eq!(ids.len(), entries.len());
        let mut out = vec![0; 0x15 + ids.len() * 8];
        out[..8].copy_from_slice(b"NT Data ");
        out[8..10].copy_from_slice(b"10");
        out[0x10..0x14].copy_from_slice(&(ids.len() as i32).to_le_bytes());
        for (index, (id, entry)) in ids.iter().zip(entries).enumerate() {
            let table_offset = 0x15 + index * 8;
            let data_offset = out.len() as u32;
            out[table_offset..table_offset + 4].copy_from_slice(&id.to_le_bytes());
            out[table_offset + 4..table_offset + 8].copy_from_slice(&data_offset.to_le_bytes());
            out.extend_from_slice(entry);
        }
        out
    }

    fn file_for_path<'a>(files: &'a [PatchFile], path: &str) -> &'a PatchFile {
        files
            .iter()
            .find(|file| file.path.eq_ignore_ascii_case(path))
            .expect("expected output file")
    }

    #[test]
    fn patch_in_place_candidates_include_split_archive_parent() {
        assert_eq!(
            patch_in_place_base_candidates("NostaleData/NSppData08.NOS"),
            vec![
                "NostaleData/NSppData08.NOS".to_string(),
                "NostaleData/NSppData.NOS".to_string(),
            ]
        );
        assert_eq!(
            patch_in_place_base_candidates("NostaleData/NSppData.NOS"),
            vec!["NostaleData/NSppData.NOS".to_string()]
        );
        assert_eq!(
            patch_in_place_base_candidates("NostaleData/NSppDataZZ.NOS"),
            vec!["NostaleData/NSppDataZZ.NOS".to_string()]
        );
    }

    #[tokio::test]
    async fn applies_individual_replace_operation_in_memory() -> Result<()> {
        let operation = operation(
            1,
            PchOperationKind::ReplaceFile,
            "Nostale.exe",
            b"MZpayload",
            &sha1_hex(b"MZpayload"),
        );

        let result = apply_patch_operation(&operation, &MemoryLoader::default()).await?;

        assert_eq!(result.removals, Vec::<String>::new());
        assert_eq!(result.writes.len(), 1);
        assert_eq!(result.writes[0].path, "Nostale.exe");
        assert_eq!(result.writes[0].bytes, b"MZpayload");
        Ok(())
    }

    #[tokio::test]
    async fn opcode_three_does_not_validate_target_or_payload_magic() -> Result<()> {
        let operation = operation(
            3,
            PchOperationKind::ReplaceAndRelaunch,
            "data.bin",
            b"payload",
            &sha1_hex(b"payload"),
        );

        let result = apply_patch_operation(&operation, &MemoryLoader::default()).await?;

        assert_eq!(result.removals, Vec::<String>::new());
        assert_eq!(result.writes.len(), 1);
        assert_eq!(result.writes[0].path, "data.bin");
        assert_eq!(result.writes[0].bytes, b"payload");
        Ok(())
    }

    #[tokio::test]
    async fn package_apply_returns_atomic_change_set() -> Result<()> {
        let operations = vec![
            operation(
                1,
                PchOperationKind::ReplaceFile,
                "a.dat",
                b"a-new",
                &sha1_hex(b"a-new"),
            ),
            operation(
                0,
                PchOperationKind::DeleteFile,
                "b.dat",
                b"",
                &sha1_hex(b""),
            ),
        ];
        let package = ParsedPchPkg {
            header: crate::package::PchPkgHeader {
                package_count: 2,
                body_offset: 0,
                package_datetime: PchPackageDateTimeCode::from_raw(0),
                segment_lookup_flag: 1,
                direct_segment_lookup: true,
                segment_table_hex: String::new(),
            },
            operations,
        };

        let result = apply_patch_package(&package, &MemoryLoader::default()).await?;

        assert_eq!(result.writes[0].path, "a.dat");
        assert_eq!(result.writes[0].bytes, b"a-new");
        assert_eq!(result.removals, vec!["b.dat"]);
        Ok(())
    }

    #[tokio::test]
    async fn package_apply_uses_previous_operation_output_as_source() -> Result<()> {
        let mut literal_delta = Vec::new();
        literal_delta.extend_from_slice(&0_u32.to_le_bytes());
        literal_delta.push(0);
        literal_delta.extend_from_slice(&3_u32.to_le_bytes());
        literal_delta.extend_from_slice(&crc32fast::hash(b"two").to_le_bytes());
        literal_delta.extend_from_slice(b"two");
        literal_delta.extend_from_slice(&0_u32.to_le_bytes());
        literal_delta.push(0);
        literal_delta.extend_from_slice(&0_u32.to_le_bytes());

        let operations = vec![
            operation(
                1,
                PchOperationKind::ReplaceFile,
                "a.dat",
                b"one",
                &sha1_hex(b"one"),
            ),
            operation(
                2,
                PchOperationKind::BinaryDelta,
                "a.dat",
                &literal_delta,
                &sha1_hex(&literal_delta),
            ),
        ];
        let package = ParsedPchPkg {
            header: crate::package::PchPkgHeader {
                package_count: 2,
                body_offset: 0,
                package_datetime: PchPackageDateTimeCode::from_raw(0),
                segment_lookup_flag: 1,
                direct_segment_lookup: true,
                segment_table_hex: String::new(),
            },
            operations,
        };

        let result = apply_patch_package(&package, &MemoryLoader::default()).await?;

        assert_eq!(result.writes.len(), 1);
        assert_eq!(result.writes[0].path, "a.dat");
        assert_eq!(result.writes[0].bytes, b"two");
        Ok(())
    }

    #[tokio::test]
    async fn known_extract_ui_eff_updates_archives_without_partial_state() -> Result<()> {
        let mut loader = MemoryLoader::default();
        let nstg = archive(
            &[0x100, 0x4f000001],
            &[archive_entry(8, b"keep-g"), archive_entry(8, b"extract-g")],
        );
        let nstp = archive(
            &[0x4f000002, 0x200, 0x5f000003],
            &[
                archive_entry(8, b"extract-pe"),
                archive_entry(8, b"keep-p"),
                archive_entry(8, b"extract-pu"),
            ],
        );
        loader.insert(NSTG_DATA_PATH, nstg);
        loader.insert(NSTP_DATA_PATH, nstp);
        let operation = operation(
            6,
            PchOperationKind::ReplaceAndRun,
            EXTRACT_UI_EFF_TARGET_PATH,
            b"MZhelper",
            EXTRACT_UI_EFF_SHA1,
        );

        let result = apply_patch_operation(&operation, &loader).await?;

        assert!(
            !file_for_path(&result.writes, EXTRACT_UI_EFF_TARGET_PATH)
                .bytes
                .is_empty()
        );
        assert_eq!(
            crate::binary_nos_update::binary_nos_archive_entry_ids(
                &file_for_path(&result.writes, NSTG_DATA_PATH).bytes
            )?,
            vec![0x100]
        );
        assert_eq!(
            crate::binary_nos_update::binary_nos_archive_entry_ids(
                &file_for_path(&result.writes, "NostaleData/NStgeData.NOS").bytes
            )?,
            vec![0x4f000001]
        );
        assert_eq!(
            crate::binary_nos_update::binary_nos_archive_entry_ids(
                &file_for_path(&result.writes, NSTP_DATA_PATH).bytes
            )?,
            vec![0x200]
        );
        assert_eq!(
            crate::binary_nos_update::binary_nos_archive_entry_ids(
                &file_for_path(&result.writes, "NostaleData/NStpeData.NOS").bytes
            )?,
            vec![0x4f000002]
        );
        assert_eq!(
            crate::binary_nos_update::binary_nos_archive_entry_ids(
                &file_for_path(&result.writes, "NostaleData/NStpuData.NOS").bytes
            )?,
            vec![0x5f000003]
        );
        Ok(())
    }

    #[tokio::test]
    async fn known_extract_ui_eff_without_sources_fails() -> Result<()> {
        let operation = operation(
            6,
            PchOperationKind::ReplaceAndRun,
            EXTRACT_UI_EFF_TARGET_PATH,
            b"MZhelper",
            EXTRACT_UI_EFF_SHA1,
        );

        let err = apply_patch_operation(&operation, &MemoryLoader::default())
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("loading NStgData.NOS"));
        Ok(())
    }

    #[tokio::test]
    async fn unknown_opcode_six_payload_fails() -> Result<()> {
        let operation = operation(
            6,
            PchOperationKind::ReplaceAndRun,
            EXTRACT_UI_EFF_TARGET_PATH,
            b"MZother",
            "0123456789012345678901234567890123456789",
        );

        let err = apply_patch_operation(&operation, &MemoryLoader::default())
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("unsupported replace-and-run operation"));
        Ok(())
    }

    #[tokio::test]
    async fn opcode_six_non_extract_ui_eff_target_fails() -> Result<()> {
        let operation = operation(
            6,
            PchOperationKind::ReplaceAndRun,
            "NostaleData/OtherHelper.dat",
            b"MZhelper",
            EXTRACT_UI_EFF_SHA1,
        );

        let err = apply_patch_operation(&operation, &MemoryLoader::default())
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("unsupported replace-and-run operation"));
        Ok(())
    }
}
