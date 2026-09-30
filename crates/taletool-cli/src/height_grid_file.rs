//! JSON and file helpers for map height-grid assets.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_map::{HeightGrid, write_height_grid_bytes};

const HEIGHT_GRID_DOCUMENT_FORMAT: &str = "height-grid";
const HEIGHT_GRID_DOCUMENT_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeightGridDocument<T = HeightGrid> {
    format: String,
    version: u32,
    grid: T,
}

/// Write a decoded height grid as a JSON document.
pub(crate) fn unpack_height_grid_file(grid: &HeightGrid, out: &Path) -> anyhow::Result<()> {
    let document = HeightGridDocument {
        format: HEIGHT_GRID_DOCUMENT_FORMAT.to_owned(),
        version: HEIGHT_GRID_DOCUMENT_VERSION,
        grid: grid.clone(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write native height-grid bytes from a JSON document.
pub(crate) fn pack_height_grid_file(input: &Path, out: &Path) -> anyhow::Result<HeightGrid> {
    let document_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    // Check the document version before interpreting fields from older layouts.
    let header: HeightGridDocument<serde::de::IgnoredAny> = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&header)?;

    let document: HeightGridDocument = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing height grid in {}", input.display()))?;
    let bytes = write_height_grid_bytes(&document.grid)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(document.grid)
}

fn validate_document_header<T>(document: &HeightGridDocument<T>) -> anyhow::Result<()> {
    if document.format != HEIGHT_GRID_DOCUMENT_FORMAT {
        bail!(
            "height-grid document has unsupported format {:?}; expected {:?}",
            document.format,
            HEIGHT_GRID_DOCUMENT_FORMAT
        );
    }
    if document.version != HEIGHT_GRID_DOCUMENT_VERSION {
        if document.version == 1 {
            bail!(
                "height-grid JSON version 1 uses an obsolete payload layout; unpack the original NosTale payload again to produce version 2 JSON"
            );
        }
        bail!(
            "height-grid document has unsupported version {}; expected {}",
            document.version,
            HEIGHT_GRID_DOCUMENT_VERSION
        );
    }
    Ok(())
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use taletool_map::{
        HeightGridBounds, HeightGridDimensions, HeightGridEncoding, decode_height_grid,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn sample_grid(encoding: HeightGridEncoding) -> HeightGrid {
        HeightGrid {
            encoding,
            map_id: 43,
            bounds: HeightGridBounds {
                minimum: [-1.0, -2.0, -3.0],
                maximum: [4.0, 5.0, 6.0],
            },
            dimensions: HeightGridDimensions { width: 1, depth: 1 },
            cell_size: [0.5, 0.5, 0.5],
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            triangles: vec![[0, 1, 2]],
            cells: vec![vec![0]],
        }
    }

    #[test]
    fn strict_json_round_trips_all_native_encodings() {
        let root = temp_dir("height-grid-json-round-trip");
        fs::create_dir_all(&root).unwrap();

        for encoding in [
            HeightGridEncoding::ImplicitVersion1,
            HeightGridEncoding::Version1,
            HeightGridEncoding::Version2,
        ] {
            let json_path = root.join(format!("{encoding:?}.json"));
            let payload_path = root.join(format!("{encoding:?}.bin"));
            let expected = sample_grid(encoding);
            unpack_height_grid_file(&expected, &json_path).unwrap();

            let document: serde_json::Value =
                serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
            assert_eq!(document["format"], HEIGHT_GRID_DOCUMENT_FORMAT);
            assert_eq!(document["version"], HEIGHT_GRID_DOCUMENT_VERSION);
            assert!(document["grid"].get("grid_id").is_none());

            let actual = pack_height_grid_file(&json_path, &payload_path).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(
                decode_height_grid(&fs::read(&payload_path).unwrap()).unwrap(),
                expected
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_legacy_json_before_writing_output() {
        let root = temp_dir("height-grid-legacy-json");
        fs::create_dir_all(&root).unwrap();
        let json_path = root.join("grid.json");
        let output = root.join("grid.bin");
        fs::write(&output, b"existing payload").unwrap();
        let mut grid =
            serde_json::to_value(sample_grid(HeightGridEncoding::ImplicitVersion1)).unwrap();
        grid["grid_id"] = json!(200811281);
        let mut document = json!({ "format": "height-grid", "version": 1, "grid": grid });
        fs::write(&json_path, document.to_string()).unwrap();
        let error = pack_height_grid_file(&json_path, &output).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unpack the original NosTale payload again")
        );
        assert_eq!(fs::read(&output).unwrap(), b"existing payload");

        document["version"] = json!(2);
        fs::write(&json_path, document.to_string()).unwrap();
        let error = pack_height_grid_file(&json_path, &output).unwrap_err();
        assert!(format!("{error:#}").contains("unknown field `grid_id`"));
        assert_eq!(fs::read(&output).unwrap(), b"existing payload");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_versions_and_unknown_fields() {
        let root = temp_dir("height-grid-invalid-json");
        let json_path = root.join("grid.json");
        fs::create_dir_all(&root).unwrap();

        fs::write(
            &json_path,
            serde_json::to_vec(&HeightGridDocument {
                format: HEIGHT_GRID_DOCUMENT_FORMAT.to_owned(),
                version: 999,
                grid: sample_grid(HeightGridEncoding::ImplicitVersion1),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(pack_height_grid_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": HEIGHT_GRID_DOCUMENT_FORMAT,
                "version": HEIGHT_GRID_DOCUMENT_VERSION,
                "grid": sample_grid(HeightGridEncoding::ImplicitVersion1),
                "unexpected": true
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_height_grid_file(&json_path, &root.join("bad.bin")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
