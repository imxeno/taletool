//! Atomic publication of generated output directories.
use anyhow::Context;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Publish a newly generated output directory only after every write succeeds.
pub(crate) fn write_output_transactionally<T>(
    out: &Path,
    operation: impl FnOnce(&Path) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let mut transaction = OutputTransaction::begin(out)?;
    match operation(transaction.staging_path()) {
        Ok(value) => match transaction.commit() {
            Ok(()) => Ok(value),
            Err(error) => transaction.abort(error),
        },
        Err(error) => transaction.abort(error),
    }
}

struct OutputTransaction {
    final_path: PathBuf,
    staging_path: PathBuf,
    active: bool,
}

impl OutputTransaction {
    fn begin(out: &Path) -> anyhow::Result<Self> {
        match fs::symlink_metadata(out) {
            Ok(_) => anyhow::bail!(
                "archive output already exists: {}; choose a new --out path",
                out.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("checking archive output {}", out.display()));
            }
        }

        let parent = out
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let file_name = out.file_name().ok_or_else(|| {
            anyhow::anyhow!("archive --out must name a directory: {}", out.display())
        })?;
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output parent directory {}", parent.display()))?;

        for _ in 0..1024 {
            let sequence = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
            let staging_path = parent.join(format!(
                ".taletool-{}-{}-{sequence}.staging",
                file_name.to_string_lossy(),
                std::process::id(),
            ));
            match fs::create_dir(&staging_path) {
                Ok(()) => {
                    return Ok(Self {
                        final_path: out.to_path_buf(),
                        staging_path,
                        active: true,
                    });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("creating staging directory {}", staging_path.display())
                    });
                }
            }
        }
        anyhow::bail!(
            "could not allocate a staging directory beside {}",
            out.display()
        )
    }

    fn staging_path(&self) -> &Path {
        &self.staging_path
    }

    fn commit(&mut self) -> anyhow::Result<()> {
        fs::rename(&self.staging_path, &self.final_path)
            .with_context(|| format!("publishing archive output {}", self.final_path.display()))?;
        self.active = false;
        Ok(())
    }

    fn clean_up(&mut self) -> std::io::Result<()> {
        if self.active {
            match fs::remove_dir_all(&self.staging_path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            self.active = false;
        }
        Ok(())
    }

    fn abort<T>(&mut self, error: anyhow::Error) -> anyhow::Result<T> {
        match self.clean_up() {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(error.context(format!(
                "also failed to remove staging directory {}: {cleanup_error}",
                self.staging_path.display()
            ))),
        }
    }
}

impl Drop for OutputTransaction {
    fn drop(&mut self) {
        let _ = self.clean_up();
    }
}
