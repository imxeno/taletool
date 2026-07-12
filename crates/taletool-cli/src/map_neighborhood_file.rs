//! JSON and file helpers for map-neighborhood assets.

use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_map_neighborhood::{MapNeighborhood, write_map_neighborhood_bytes};

const MAP_NEIGHBORHOOD_DOCUMENT_FORMAT: &str = "map-neighborhood";
const MAP_NEIGHBORHOOD_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapNeighborhoodDocument {
    format: String,
    version: u32,
    map_neighborhood: MapNeighborhood,
}

/// Write a decoded map neighborhood as a JSON document.
pub(crate) fn unpack_map_neighborhood_file(
    map_neighborhood: &MapNeighborhood,
    out: &Path,
) -> anyhow::Result<()> {
    let document = MapNeighborhoodDocument {
        format: MAP_NEIGHBORHOOD_DOCUMENT_FORMAT.to_owned(),
        version: MAP_NEIGHBORHOOD_DOCUMENT_VERSION,
        map_neighborhood: map_neighborhood.clone(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// Build and write native map-neighborhood bytes from a JSON document.
pub(crate) fn pack_map_neighborhood_file(
    input: &Path,
    out: &Path,
) -> anyhow::Result<MapNeighborhood> {
    let document_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let document: MapNeighborhoodDocument = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&document)?;

    let bytes = write_map_neighborhood_bytes(&document.map_neighborhood)?;
    create_parent_dir(out)?;
    fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    Ok(document.map_neighborhood)
}

fn validate_document_header(document: &MapNeighborhoodDocument) -> anyhow::Result<()> {
    if document.format != MAP_NEIGHBORHOOD_DOCUMENT_FORMAT {
        bail!(
            "map-neighborhood document has unsupported format {:?}; expected {:?}",
            document.format,
            MAP_NEIGHBORHOOD_DOCUMENT_FORMAT
        );
    }
    if document.version != MAP_NEIGHBORHOOD_DOCUMENT_VERSION {
        bail!(
            "map-neighborhood document has unsupported version {}; expected {}",
            document.version,
            MAP_NEIGHBORHOOD_DOCUMENT_VERSION
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
    use taletool_map_neighborhood::{
        MapBoundingSphere, MapBounds, NeighborMapReference, NeighborhoodPointSequence,
        decode_map_neighborhood,
    };

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn map_neighborhood() -> MapNeighborhood {
        MapNeighborhood {
            preamble: [8, 10],
            neighbors: vec![NeighborMapReference {
                map_resource_key: 42,
                group: 3,
                projected_texture_bounds: MapBounds {
                    minimum: [-10.0, -20.0, -30.0],
                    maximum: [40.0, 50.0, 60.0],
                },
                visibility_bounds: MapBounds {
                    minimum: [-1.0, -2.0, -3.0],
                    maximum: [4.0, 5.0, 6.0],
                },
                bounding_sphere: MapBoundingSphere {
                    center: [1.0, 2.0, 3.0],
                    radius: 7.0,
                },
                translation: [10.0, 20.0, 30.0],
            }],
            point_sequences: vec![NeighborhoodPointSequence {
                leading_values: [1, 2, 3],
                metadata: [0xaa; 8],
                points: vec![[-4, 5]],
                trailing_byte: 6,
                trailing_value: 7,
            }],
        }
    }

    #[test]
    fn strict_json_round_trips_native_bytes() {
        let root = temp_dir("map-neighborhood-json-round-trip");
        let json_path = root.join("map-neighborhood.json");
        let payload_path = root.join("map-neighborhood.bin");
        fs::create_dir_all(&root).unwrap();
        let expected = map_neighborhood();

        unpack_map_neighborhood_file(&expected, &json_path).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(document["format"], MAP_NEIGHBORHOOD_DOCUMENT_FORMAT);
        assert_eq!(document["version"], MAP_NEIGHBORHOOD_DOCUMENT_VERSION);
        assert_eq!(
            document["map_neighborhood"]["neighbors"][0]["projected_texture_bounds"]["minimum"],
            json!([-10.0, -20.0, -30.0])
        );
        assert_eq!(
            document["map_neighborhood"]["neighbors"][0]["visibility_bounds"]["maximum"],
            json!([4.0, 5.0, 6.0])
        );
        assert_eq!(
            document["map_neighborhood"]["neighbors"][0]["map_resource_key"],
            42
        );
        assert!(document.get("scene_resource").is_none());
        assert!(document.get("scene_neighborhood").is_none());
        assert!(document["map_neighborhood"].get("chunks").is_none());
        assert!(
            document["map_neighborhood"]["neighbors"][0]
                .get("metadata")
                .is_none()
        );

        let actual = pack_map_neighborhood_file(&json_path, &payload_path).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            decode_map_neighborhood(&fs::read(&payload_path).unwrap()).unwrap(),
            expected
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_versions_legacy_schema_and_unknown_fields() {
        let root = temp_dir("map-neighborhood-invalid-json");
        let json_path = root.join("map-neighborhood.json");
        fs::create_dir_all(&root).unwrap();

        fs::write(
            &json_path,
            serde_json::to_vec(&MapNeighborhoodDocument {
                format: MAP_NEIGHBORHOOD_DOCUMENT_FORMAT.to_owned(),
                version: 999,
                map_neighborhood: map_neighborhood(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(pack_map_neighborhood_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": MAP_NEIGHBORHOOD_DOCUMENT_FORMAT,
                "version": MAP_NEIGHBORHOOD_DOCUMENT_VERSION,
                "map_neighborhood": map_neighborhood(),
                "unexpected": true
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_map_neighborhood_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": "scene-neighborhood",
                "version": 1,
                "scene_neighborhood": map_neighborhood()
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_map_neighborhood_file(&json_path, &root.join("bad.bin")).is_err());

        let mut legacy_field_document = serde_json::to_value(MapNeighborhoodDocument {
            format: MAP_NEIGHBORHOOD_DOCUMENT_FORMAT.to_owned(),
            version: MAP_NEIGHBORHOOD_DOCUMENT_VERSION,
            map_neighborhood: map_neighborhood(),
        })
        .unwrap();
        let neighbor = legacy_field_document["map_neighborhood"]["neighbors"][0]
            .as_object_mut()
            .unwrap();
        let resource_key = neighbor.remove("map_resource_key").unwrap();
        neighbor.insert("scene_resource_key".to_owned(), resource_key);
        fs::write(
            &json_path,
            serde_json::to_vec(&legacy_field_document).unwrap(),
        )
        .unwrap();
        assert!(pack_map_neighborhood_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": "scene-resource-links",
                "version": MAP_NEIGHBORHOOD_DOCUMENT_VERSION,
                "map_neighborhood": map_neighborhood()
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_map_neighborhood_file(&json_path, &root.join("bad.bin")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": "scene-resource-links",
                "version": 1,
                "scene_resource": {
                    "preamble": [8, 10],
                    "chunks": [],
                    "point_sequences": []
                }
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_map_neighborhood_file(&json_path, &root.join("bad.bin")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
