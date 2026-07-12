//! JSON and file helpers for geometry assets.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_geometry::{Geometry, write_geometry_bytes};

const GEOMETRY_DOCUMENT_FORMAT: &str = "geometry";
const GEOMETRY_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeometryDocument {
    format: String,
    version: u32,
    geometry: Geometry,
}

/// Write decoded geometry as a JSON document.
pub(crate) fn unpack_geometry_file(geometry: &Geometry, out: &Path) -> anyhow::Result<()> {
    let document = GeometryDocument {
        format: GEOMETRY_DOCUMENT_FORMAT.to_owned(),
        version: GEOMETRY_DOCUMENT_VERSION,
        geometry: geometry.clone(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write native geometry bytes from a JSON document.
pub(crate) fn pack_geometry_file(input: &Path, out: &Path) -> anyhow::Result<Geometry> {
    let document_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let document: GeometryDocument = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&document)?;

    let bytes = write_geometry_bytes(&document.geometry)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(document.geometry)
}

fn validate_document_header(document: &GeometryDocument) -> anyhow::Result<()> {
    if document.format != GEOMETRY_DOCUMENT_FORMAT {
        bail!(
            "geometry document has unsupported format {:?}; expected {:?}",
            document.format,
            GEOMETRY_DOCUMENT_FORMAT
        );
    }
    if document.version != GEOMETRY_DOCUMENT_VERSION {
        bail!(
            "geometry document has unsupported version {}; expected {}",
            document.version,
            GEOMETRY_DOCUMENT_VERSION
        );
    }
    Ok(())
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use taletool_geometry::{
        AxisAlignedBounds, BoundingSphere, GeometryHeader, GeometryNode, GeometryVertex,
        decode_geometry,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn geometry() -> Geometry {
        Geometry {
            header: GeometryHeader {
                bounds: AxisAlignedBounds {
                    minimum: [-1.0, -1.0, -1.0],
                    maximum: [1.0, 1.0, 1.0],
                },
                bounding_sphere: BoundingSphere {
                    center: [0.0, 0.0, 0.0],
                    radius: 2.0,
                },
                first_frame: 0,
                last_frame: 100,
                frame_rate: 30,
                keyframe_step: 160,
                texture_coordinate_scale: 1.0 / 32767.0,
            },
            vertices: vec![
                GeometryVertex {
                    position: [0.0, 0.0, 0.0],
                    texture_coordinates: [0, 0],
                    normal: [0, 127, 0],
                },
                GeometryVertex {
                    position: [1.0, 0.0, 0.0],
                    texture_coordinates: [1, 0],
                    normal: [0, 127, 0],
                },
                GeometryVertex {
                    position: [0.0, 0.0, 1.0],
                    texture_coordinates: [0, 1],
                    normal: [0, 127, 0],
                },
            ],
            triangle_lists: vec![vec![0, 1, 2]],
            root_nodes: vec![GeometryNode {
                base_translation: [0.0, 0.0, 0.0],
                base_rotation: [0, 0, 0, 32767],
                base_scale: [1.0, 1.0, 1.0],
                translation_keyframes: Vec::new(),
                rotation_keyframes: Vec::new(),
                scale_keyframes: Vec::new(),
                batches: Vec::new(),
                children: Vec::new(),
            }],
        }
    }

    #[test]
    fn strict_json_round_trips_native_bytes() {
        let root = temp_dir("geometry-json-round-trip");
        let json_path = root.join("geometry.json");
        let payload_path = root.join("geometry.bin");
        fs::create_dir_all(&root).unwrap();
        let expected = geometry();

        unpack_geometry_file(&expected, &json_path).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(document["format"], GEOMETRY_DOCUMENT_FORMAT);
        assert_eq!(document["version"], GEOMETRY_DOCUMENT_VERSION);

        let actual = pack_geometry_file(&json_path, &payload_path).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            decode_geometry(&fs::read(&payload_path).unwrap()).unwrap(),
            expected
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_versions_and_unknown_fields() {
        let root = temp_dir("geometry-invalid-json");
        let json_path = root.join("geometry.json");
        fs::create_dir_all(&root).unwrap();

        fs::write(
            &json_path,
            serde_json::to_vec(&GeometryDocument {
                format: GEOMETRY_DOCUMENT_FORMAT.to_owned(),
                version: 999,
                geometry: geometry(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(pack_geometry_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": GEOMETRY_DOCUMENT_FORMAT,
                "version": GEOMETRY_DOCUMENT_VERSION,
                "geometry": geometry(),
                "unexpected": true
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_geometry_file(&json_path, &root.join("bad.bin")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
