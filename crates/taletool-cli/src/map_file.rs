//! JSON and file helpers for map assets.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_map::{Map, write_map_bytes};

const MAP_DOCUMENT_FORMAT: &str = "map";
const MAP_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapDocument {
    format: String,
    version: u32,
    map: Map,
}

/// Write decoded map data as a JSON document.
pub(crate) fn unpack_map_file(map: &Map, out: &Path) -> anyhow::Result<()> {
    let document = MapDocument {
        format: MAP_DOCUMENT_FORMAT.to_owned(),
        version: MAP_DOCUMENT_VERSION,
        map: map.clone(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write native map bytes from a JSON document.
pub(crate) fn pack_map_file(input: &Path, out: &Path) -> anyhow::Result<Map> {
    let document_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let document: MapDocument = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&document)?;

    let bytes = write_map_bytes(&document.map)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(document.map)
}

fn validate_document_header(document: &MapDocument) -> anyhow::Result<()> {
    if document.format != MAP_DOCUMENT_FORMAT {
        bail!(
            "map document has unsupported format {:?}; expected {:?}",
            document.format,
            MAP_DOCUMENT_FORMAT
        );
    }
    if document.version != MAP_DOCUMENT_VERSION {
        bail!(
            "map document has unsupported version {}; expected {}",
            document.version,
            MAP_DOCUMENT_VERSION
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
    use taletool_map::{
        BoundingSphere, Bounds3, CameraAngleLimits, MAP_HEADER_UNKNOWN_00_LEN,
        MAP_HEADER_UNKNOWN_79_LEN, MapHeader, MapNode, MapNodeKind, Rgba8, decode_map,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn map() -> Map {
        Map {
            header: MapHeader {
                unknown_00: vec![0; MAP_HEADER_UNKNOWN_00_LEN],
                resource_group: 1,
                bounds: Bounds3 {
                    minimum: [-1.0; 3],
                    maximum: [1.0; 3],
                },
                ground_bounds: Bounds3 {
                    minimum: [-2.0; 3],
                    maximum: [2.0; 3],
                },
                ground_bounding_sphere: BoundingSphere {
                    center: [0.0; 3],
                    radius: 2.0,
                },
                ambient_light: Rgba8 {
                    red: 1,
                    green: 2,
                    blue: 3,
                    alpha: 4,
                },
                diffuse_light: Rgba8 {
                    red: 5,
                    green: 6,
                    blue: 7,
                    alpha: 8,
                },
                fog_color: 0xff00_0000,
                yaw_limits: CameraAngleLimits {
                    angle_degrees: 90,
                    minimum_offset_degrees: 30,
                    maximum_offset_degrees: 30,
                },
                pitch_limits: CameraAngleLimits {
                    angle_degrees: 45,
                    minimum_offset_degrees: 15,
                    maximum_offset_degrees: 5,
                },
                fog_start: 10,
                fog_end: 200,
                unknown_79: vec![0; MAP_HEADER_UNKNOWN_79_LEN],
                reset_yaw: false,
                unknown_84: 0,
            },
            geometry_keys: Vec::new(),
            root_nodes: vec![MapNode {
                kind: MapNodeKind::Group {
                    bounding_sphere: BoundingSphere {
                        center: [0.0; 3],
                        radius: 10.0,
                    },
                },
                children: Vec::new(),
            }],
        }
    }

    #[test]
    fn json_round_trips_native_bytes() {
        let root = temp_dir("map-json-round-trip");
        let json_path = root.join("map.json");
        let payload_path = root.join("map.bin");
        fs::create_dir_all(&root).unwrap();
        let expected = map();

        unpack_map_file(&expected, &json_path).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(document["format"], MAP_DOCUMENT_FORMAT);
        assert_eq!(document["version"], MAP_DOCUMENT_VERSION);
        assert!(document["map"]["header"]["unknown_00"].is_array());
        assert!(document["map"]["header"]["unknown_79"].is_array());
        assert_eq!(document["map"]["header"]["unknown_84"], 0);
        assert_eq!(document["map"]["root_nodes"][0]["kind"]["type"], "group");

        let actual = pack_map_file(&json_path, &payload_path).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            decode_map(&fs::read(&payload_path).unwrap()).unwrap(),
            expected
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_old_schema_wrong_versions_and_unknown_fields() {
        let root = temp_dir("map-invalid-json");
        let json_path = root.join("map.json");
        fs::create_dir_all(&root).unwrap();

        fs::write(
            &json_path,
            json!({
                "format": "scene_bulk",
                "version": 1,
                "bulk": map()
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_map_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            serde_json::to_vec(&MapDocument {
                format: MAP_DOCUMENT_FORMAT.to_owned(),
                version: 999,
                map: map(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(pack_map_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": MAP_DOCUMENT_FORMAT,
                "version": MAP_DOCUMENT_VERSION,
                "map": map(),
                "unexpected": true
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_map_file(&json_path, &root.join("bad.bin")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
