//! Handlers for `taletool patch` package commands.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::json;
use taletool_patch::{
    ParsedPchPkg, PatchChangeSet, PatchSourceFile, PatchSourceLoader, apply_patch_packages,
    normalize_client_path, parse_pch_pkg, sha1_hex,
};

use crate::cli::PatchCommand;
use crate::paths::resolve_inputs;

pub(crate) async fn run_patch(command: PatchCommand) -> Result<()> {
    match command {
        PatchCommand::Inspect { package, json } => inspect_packages(&package, json),
        PatchCommand::Apply {
            root,
            package,
            dry_run,
            backup_dir,
        } => apply_packages(&root, &package, dry_run, backup_dir.as_deref()).await,
    }
}

fn inspect_packages(package_inputs: &[String], json_output: bool) -> Result<()> {
    let packages = read_packages(package_inputs)?;

    if json_output {
        let output = packages
            .iter()
            .map(|(path, parsed)| {
                json!({
                    "package": path,
                    "header": parsed.header,
                    "operations": parsed.operations.iter().map(|operation| {
                        json!({
                            "segment_index": operation.segment.segment_index,
                            "segment_id": operation.segment.segment_id,
                            "op_code": operation.op_code,
                            "op_kind": operation.op_kind.as_str(),
                            "target_path": operation.target_path,
                            "raw_target_path": operation.raw_target_path,
                            "payload_size": operation.payload.len(),
                            "payload_sha1": operation.payload_sha1,
                            "header_json": parsed.header_json(operation),
                        })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for (path, parsed) in &packages {
            println!("package: {path}");
            println!("segments: {}", parsed.operations.len());
            for operation in &parsed.operations {
                println!(
                    "  #{:<3} {:<28} {:<48} payload={} sha1={}",
                    operation.segment.segment_index,
                    operation.op_kind.as_str(),
                    operation.target_path,
                    operation.payload.len(),
                    operation.payload_sha1,
                );
            }
        }
    }

    Ok(())
}

async fn apply_packages(
    root: &Path,
    package_inputs: &[String],
    dry_run: bool,
    backup_dir: Option<&Path>,
) -> Result<()> {
    if !root.is_dir() {
        bail!("client root is not a directory: {}", root.display());
    }

    let packages = read_packages(package_inputs)?;
    let parsed = packages
        .iter()
        .map(|(_, package)| package.clone())
        .collect::<Vec<_>>();
    let loader = FilesystemPatchSourceLoader {
        root: root.to_path_buf(),
    };
    let change_set = apply_patch_packages(&parsed, &loader).await?;

    if dry_run {
        print_change_set("dry-run", &change_set);
        return Ok(());
    }

    let backup_root = backup_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_backup_dir(root));
    commit_change_set(root, &backup_root, &change_set)?;
    print_change_set("applied", &change_set);
    println!("backup_dir: {}", backup_root.display());
    Ok(())
}

fn read_packages(package_inputs: &[String]) -> Result<Vec<(String, ParsedPchPkg)>> {
    let paths = resolve_inputs(package_inputs)?;
    paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path)
                .with_context(|| format!("reading patch package {}", path.display()))?;
            let parsed = parse_pch_pkg(&bytes)
                .with_context(|| format!("parsing patch package {}", path.display()))?;
            Ok((path.display().to_string(), parsed))
        })
        .collect()
}

fn print_change_set(label: &str, change_set: &PatchChangeSet) {
    println!("{label}:");
    println!("writes: {}", change_set.writes.len());
    for file in &change_set.writes {
        println!(
            "  write {:<48} bytes={} sha1={}",
            file.path,
            file.bytes.len(),
            file.sha1
        );
    }
    println!("removals: {}", change_set.removals.len());
    for path in &change_set.removals {
        println!("  remove {path}");
    }
}

struct FilesystemPatchSourceLoader {
    root: PathBuf,
}

#[async_trait]
impl PatchSourceLoader for FilesystemPatchSourceLoader {
    async fn load_source(&self, path: &str) -> Result<Option<PatchSourceFile>> {
        let normalized = normalize_client_path(path)?;
        let disk_path = client_path(&self.root, &normalized)?;
        let bytes = match tokio::fs::read(&disk_path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reading source file {}", disk_path.display()));
            }
        };
        Ok(Some(PatchSourceFile {
            path: normalized,
            sha1: sha1_hex(&bytes),
            bytes,
        }))
    }
}

fn commit_change_set(root: &Path, backup_root: &Path, change_set: &PatchChangeSet) -> Result<()> {
    fs::create_dir_all(backup_root)
        .with_context(|| format!("creating backup dir {}", backup_root.display()))?;
    let temp_root = backup_root.join(".tmp");
    fs::create_dir_all(&temp_root)
        .with_context(|| format!("creating temp dir {}", temp_root.display()))?;

    let mut temp_writes = Vec::new();
    for file in &change_set.writes {
        let temp_path = client_path(&temp_root, &file.path)?;
        if let Some(parent) = temp_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&temp_path, &file.bytes)
            .with_context(|| format!("writing temp file {}", temp_path.display()))?;
        temp_writes.push((file.clone(), temp_path));
    }

    let mut backups = Vec::new();
    let commit_result = (|| -> Result<()> {
        for path in &change_set.removals {
            let target = client_path(root, path)?;
            backup_target(root, backup_root, path, &target, &mut backups)?;
        }

        for (file, temp_path) in &temp_writes {
            let target = client_path(root, &file.path)?;
            backup_target(root, backup_root, &file.path, &target, &mut backups)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(temp_path, &target).with_context(|| {
                format!(
                    "committing temp file {} to {}",
                    temp_path.display(),
                    target.display()
                )
            })?;
        }
        Ok(())
    })();

    if let Err(error) = commit_result {
        let rollback_error = rollback(backups);
        for (_, temp_path) in temp_writes {
            let _ = fs::remove_file(temp_path);
        }
        if let Err(rollback_error) = rollback_error {
            bail!("{error}; rollback also failed: {rollback_error}");
        }
        return Err(error);
    }

    let _ = fs::remove_dir_all(&temp_root);
    Ok(())
}

fn backup_target(
    root: &Path,
    backup_root: &Path,
    client_path_text: &str,
    target: &Path,
    backups: &mut Vec<BackupEntry>,
) -> Result<()> {
    if backups.iter().any(|backup| backup.target == target) {
        return Ok(());
    }

    if !target.exists() {
        backups.push(BackupEntry {
            target: target.to_path_buf(),
            backup: None,
        });
        return Ok(());
    }

    if !target.is_file() {
        bail!("patch target is not a file: {}", target.display());
    }

    let backup = client_path(backup_root, client_path_text)?;
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(target, &backup)
        .with_context(|| format!("backing up {} below {}", target.display(), root.display()))?;
    backups.push(BackupEntry {
        target: target.to_path_buf(),
        backup: Some(backup),
    });
    Ok(())
}

fn rollback(mut backups: Vec<BackupEntry>) -> Result<()> {
    let mut first_error = None;
    backups.reverse();
    for entry in backups {
        if entry.target.exists()
            && let Err(error) = fs::remove_file(&entry.target)
            && first_error.is_none()
        {
            first_error = Some(
                anyhow::Error::new(error)
                    .context(format!("removing changed file {}", entry.target.display())),
            );
        }

        if let Some(backup) = entry.backup {
            if let Some(parent) = entry.target.parent()
                && let Err(error) = fs::create_dir_all(parent)
                && first_error.is_none()
            {
                first_error = Some(
                    anyhow::Error::new(error)
                        .context(format!("creating rollback parent {}", parent.display())),
                );
            }
            if let Err(error) = fs::rename(&backup, &entry.target)
                && first_error.is_none()
            {
                first_error = Some(anyhow::Error::new(error).context(format!(
                    "restoring backup {} to {}",
                    backup.display(),
                    entry.target.display()
                )));
            }
        }
    }

    if let Some(error) = first_error {
        Err(error)
    } else {
        Ok(())
    }
}

#[derive(Debug)]
struct BackupEntry {
    target: PathBuf,
    backup: Option<PathBuf>,
}

fn default_backup_dir(root: &Path) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    root.join(".taletool")
        .join("backups")
        .join(format!("run-{millis}-{}", std::process::id()))
}

fn client_path(root: &Path, client_path_text: &str) -> Result<PathBuf> {
    let normalized = normalize_client_path(client_path_text)?;
    let mut path = root.to_path_buf();
    for part in normalized.split('/') {
        path.push(part);
    }
    Ok(path)
}
