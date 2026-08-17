//! Family-aware semantic conversion for binary archive payloads.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use taletool_animation::sprite::decode_sprite_animation;
use taletool_animation::sprite_remap::decode_sprite_resource_remap;
use taletool_effect::decode_effect_asset;
use taletool_geometry::decode_geometry;
use taletool_map::{decode_height_grid, decode_map, decode_map_cell_grid};
use taletool_map_neighborhood::decode_map_neighborhood;
use taletool_texture::decode_texture;
use taletool_texture::sprite::decode_sprite;
use taletool_texture::sprite::free_size::decode_free_size_sprite;

use crate::animation_file::unpack_sprite_animation_file;
use crate::binary_preset::BinaryAssetKind;
use crate::cell_flag_file::unpack_map_cell_grid_png;
use crate::effect_file::unpack_effect_file;
use crate::geometry_file::unpack_geometry_file;
use crate::height_grid_file::unpack_height_grid_file;
use crate::map_file::unpack_map_file;
use crate::map_neighborhood_file::unpack_map_neighborhood_file;
use crate::sprite_file::{unpack_free_size_sprite_png, unpack_sprite_file};
use crate::sprite_remap_file::unpack_sprite_resource_remap_file;
use crate::texture_file::unpack_texture_file;

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

/// One converted payload's output path relative to the archive output root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConvertedPayload {
    pub(crate) relative_path: PathBuf,
    pub(crate) description: String,
}

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
                "converted archive output already exists: {}; choose a new --out path",
                out.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("checking converted output {}", out.display()));
            }
        }

        let parent = out
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let file_name = out.file_name().ok_or_else(|| {
            anyhow::anyhow!(
                "converted archive --out must name a directory: {}",
                out.display()
            )
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
        fs::rename(&self.staging_path, &self.final_path).with_context(|| {
            format!(
                "publishing converted archive output {}",
                self.final_path.display()
            )
        })?;
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

/// Decode and write one payload using its archive family's exact asset type.
pub(crate) fn convert_binary_payload(
    data: &[u8],
    kind: BinaryAssetKind,
    out: &Path,
    stable_file_name: &str,
) -> anyhow::Result<ConvertedPayload> {
    let stem = stable_output_stem(stable_file_name)?;
    match kind {
        BinaryAssetKind::Geometry => {
            let geometry = decode_geometry(data).context("decoding geometry payload")?;
            let relative_path = PathBuf::from(format!("{stem}.json"));
            unpack_geometry_file(&geometry, &out.join(&relative_path))?;
            converted(relative_path, "geometry")
        }
        BinaryAssetKind::Texture => {
            let texture = decode_texture(data).context("decoding texture payload")?;
            let relative_path = PathBuf::from(stem);
            let count = unpack_texture_file(&texture, &out.join(&relative_path))?;
            converted(relative_path, format!("texture, {count} mip levels"))
        }
        BinaryAssetKind::Effect(effect_kind) => {
            let effect = decode_effect_asset(effect_kind, data)
                .with_context(|| format!("decoding {effect_kind:?} effect payload"))?;
            let relative_path = PathBuf::from(format!("{stem}.json"));
            unpack_effect_file(&effect, &out.join(&relative_path))?;
            converted(relative_path, format!("{effect_kind:?} effect"))
        }
        BinaryAssetKind::CellFlag => {
            let grid = decode_map_cell_grid(data).context("decoding map cell-flag payload")?;
            let relative_path = PathBuf::from(format!("{stem}.png"));
            unpack_map_cell_grid_png(&grid, &out.join(&relative_path), None)?;
            converted(
                relative_path,
                format!("cell flags, {}x{}", grid.width(), grid.height()),
            )
        }
        BinaryAssetKind::Map => {
            let map = decode_map(data).context("decoding map payload")?;
            let relative_path = PathBuf::from(format!("{stem}.json"));
            unpack_map_file(&map, &out.join(&relative_path))?;
            converted(relative_path, "map")
        }
        BinaryAssetKind::MapObjectSprite => {
            let sprite = decode_sprite(data).context("decoding map-object sprite payload")?;
            let relative_path = PathBuf::from(stem);
            let count = unpack_sprite_file(&sprite, &out.join(&relative_path))?;
            converted(relative_path, format!("map-object sprite, {count} frames"))
        }
        BinaryAssetKind::SpriteAnimation => {
            let animation =
                decode_sprite_animation(data).context("decoding sprite animation payload")?;
            let relative_path = PathBuf::from(format!("{stem}.json"));
            unpack_sprite_animation_file(&animation, &out.join(&relative_path))?;
            converted(
                relative_path,
                format!("sprite animation, {} frames", animation.frames.len()),
            )
        }
        BinaryAssetKind::SpriteRemap => {
            let remap = decode_sprite_resource_remap(data)
                .context("decoding sprite-resource remap payload")?;
            let relative_path = PathBuf::from(format!("{stem}.json"));
            unpack_sprite_resource_remap_file(&remap, &out.join(&relative_path))?;
            converted(
                relative_path,
                format!("sprite-resource remap, {} frames", remap.frames.len()),
            )
        }
        BinaryAssetKind::MapNeighborhood => {
            let neighborhood =
                decode_map_neighborhood(data).context("decoding map-neighborhood payload")?;
            let relative_path = PathBuf::from(format!("{stem}.json"));
            unpack_map_neighborhood_file(&neighborhood, &out.join(&relative_path))?;
            converted(relative_path, "map neighborhood")
        }
        BinaryAssetKind::HeightGrid => {
            let grid = decode_height_grid(data).context("decoding height-grid payload")?;
            let relative_path = PathBuf::from(format!("{stem}.json"));
            unpack_height_grid_file(&grid, &out.join(&relative_path))?;
            converted(relative_path, "height grid")
        }
        BinaryAssetKind::FreeSizeSprite => {
            let sprite =
                decode_free_size_sprite(data).context("decoding free-size sprite payload")?;
            let relative_path = PathBuf::from(format!("{stem}.png"));
            unpack_free_size_sprite_png(&sprite, &out.join(&relative_path))?;
            converted(
                relative_path,
                format!("free-size sprite, {}x{}", sprite.width(), sprite.height()),
            )
        }
    }
}

fn stable_output_stem(file_name: &str) -> anyhow::Result<&str> {
    if Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(file_name)
    {
        anyhow::bail!("generated output name is not a filename: {file_name:?}");
    }
    let stem = file_name.strip_suffix(".bin").ok_or_else(|| {
        anyhow::anyhow!("generated payload filename does not end in .bin: {file_name:?}")
    })?;
    if stem.is_empty() {
        anyhow::bail!("generated payload filename has an empty stem");
    }
    Ok(stem)
}

fn converted(
    relative_path: PathBuf,
    description: impl Into<String>,
) -> anyhow::Result<ConvertedPayload> {
    Ok(ConvertedPayload {
        relative_path,
        description: description.into(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn publication_failure_cleans_staging_and_preserves_destination() {
        let root = temp_dir("conversion-publish-failure");
        let out = root.join("out");
        fs::create_dir_all(&root).unwrap();

        let error = write_output_transactionally(&out, |staging| {
            fs::write(staging.join("generated.txt"), b"generated")?;
            fs::create_dir(&out)?;
            fs::write(out.join("keep.txt"), b"keep")?;
            Ok(())
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("publishing converted archive output"));
        assert_eq!(fs::read(out.join("keep.txt")).unwrap(), b"keep");
        let staging = fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".staging"))
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert!(
            staging.is_empty(),
            "staging directories remain: {staging:?}"
        );

        fs::remove_dir_all(root).unwrap();
    }
}
