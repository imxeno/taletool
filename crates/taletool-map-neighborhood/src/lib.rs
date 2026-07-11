//! Map-neighborhood payloads stored in `NStkData` archives.
//!
//! Each payload associates an active map with neighboring `NStuData` map object
//! trees and carries their placement, projected-texture bounds, and visibility
//! bounds. A second table stores compact point sequences whose fields are
//! retained without assigning speculative meanings to them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// Number of opaque bytes preceding the two counted tables.
pub const MAP_NEIGHBORHOOD_PREAMBLE_LEN: usize = 8;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MapNeighborhoodError {
    #[error(transparent)]
    Truncated(#[from] ByteReadError),
    #[error("map-neighborhood payload has {count} trailing bytes")]
    TrailingBytes { count: usize },
    #[error("map-neighborhood field {field} contains a non-finite floating-point value")]
    NonFiniteFloat { field: &'static str },
    #[error(
        "neighbor {neighbor} {field} are reversed on axis {axis}: minimum {minimum} exceeds maximum {maximum}"
    )]
    ReversedBounds {
        neighbor: usize,
        field: &'static str,
        axis: usize,
        minimum: f32,
        maximum: f32,
    },
    #[error("neighbor {neighbor} bounding sphere has invalid negative radius {radius}")]
    NegativeSphereRadius { neighbor: usize, radius: f32 },
    #[error("map neighborhood {field} has {count} items; maximum is {maximum}")]
    CountOverflow {
        field: &'static str,
        count: usize,
        maximum: usize,
    },
}

pub type MapNeighborhoodResult<T> = std::result::Result<T, MapNeighborhoodError>;

/// Axis-aligned bounds associated with one neighboring scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapBounds {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// Bounding sphere used to cull one neighboring scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapBoundingSphere {
    pub center: [f32; 3],
    pub radius: f32,
}

/// A reference from the active map to a neighboring `NStuData` map object tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeighborMapReference {
    /// Resource key of the referenced `NStuData` map object tree.
    pub map_resource_key: i32,
    /// Group byte compared with the active map's root group before rendering.
    pub group: u8,
    /// Bounds used to build the neighboring map's projected-texture transform.
    pub projected_texture_bounds: MapBounds,
    /// Bounds used to cull the neighboring map against the camera frustum.
    pub visibility_bounds: MapBounds,
    pub bounding_sphere: MapBoundingSphere,
    /// Translation from neighboring-map coordinates to active-map coordinates.
    pub translation: [f32; 3],
}

/// One compact point sequence from the payload's secondary table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeighborhoodPointSequence {
    /// Persisted leading control values with no established runtime meaning.
    pub leading_values: [u16; 3],
    /// Persisted sequence metadata with no established runtime meaning.
    pub metadata: [u8; 8],
    /// Signed two-dimensional points in stored order.
    pub points: Vec<[i16; 2]>,
    /// Persisted trailing byte with no established runtime meaning.
    pub trailing_byte: u8,
    /// Persisted trailing control value with no established runtime meaning.
    pub trailing_value: u16,
}

/// Fully decoded `NStkData` archive-entry payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapNeighborhood {
    /// Two opaque little-endian words at the start of every payload.
    pub preamble: [u32; 2],
    pub neighbors: Vec<NeighborMapReference>,
    pub point_sequences: Vec<NeighborhoodPointSequence>,
}

impl MapNeighborhood {
    /// Return sorted, unique resource keys of neighboring `NStuData` maps.
    pub fn neighbor_map_resource_keys(&self) -> Vec<i32> {
        self.neighbors
            .iter()
            .map(|neighbor| neighbor.map_resource_key)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Return the total number of points in all secondary-table sequences.
    pub fn point_count(&self) -> usize {
        self.point_sequences
            .iter()
            .map(|sequence| sequence.points.len())
            .sum()
    }
}

/// Decode one uncompressed `NStkData` archive-entry payload.
pub fn decode_map_neighborhood(data: &[u8]) -> MapNeighborhoodResult<MapNeighborhood> {
    let mut reader = ByteReader::new(data);
    let preamble = [
        reader.read_u32_le("map_neighborhood.preamble[0]")?,
        reader.read_u32_le("map_neighborhood.preamble[1]")?,
    ];

    let neighbor_count = usize::from(reader.read_u16_le("map_neighborhood.neighbor_count")?);
    let mut neighbors = Vec::with_capacity(neighbor_count);
    for _ in 0..neighbor_count {
        neighbors.push(read_neighbor(&mut reader)?);
    }

    let sequence_count = usize::from(reader.read_u16_le("map_neighborhood.sequence_count")?);
    let mut point_sequences = Vec::with_capacity(sequence_count);
    for _ in 0..sequence_count {
        point_sequences.push(read_point_sequence(&mut reader)?);
    }

    if reader.remaining() != 0 {
        return Err(MapNeighborhoodError::TrailingBytes {
            count: reader.remaining(),
        });
    }

    let neighborhood = MapNeighborhood {
        preamble,
        neighbors,
        point_sequences,
    };
    validate_map_neighborhood(&neighborhood)?;
    Ok(neighborhood)
}

/// Encode one `NStkData` map-neighborhood payload.
pub fn write_map_neighborhood_bytes(
    neighborhood: &MapNeighborhood,
) -> MapNeighborhoodResult<Vec<u8>> {
    validate_map_neighborhood(neighborhood)?;

    let mut output = Vec::new();
    for word in neighborhood.preamble {
        output.extend_from_slice(&word.to_le_bytes());
    }
    output.extend_from_slice(&(neighborhood.neighbors.len() as u16).to_le_bytes());
    for neighbor in &neighborhood.neighbors {
        write_neighbor(&mut output, neighbor);
    }
    output.extend_from_slice(&(neighborhood.point_sequences.len() as u16).to_le_bytes());
    for sequence in &neighborhood.point_sequences {
        write_point_sequence(&mut output, sequence);
    }
    Ok(output)
}

fn read_neighbor(reader: &mut ByteReader<'_>) -> MapNeighborhoodResult<NeighborMapReference> {
    let map_resource_key = reader.read_i32_le("map_neighborhood.neighbor.map_resource_key")?;
    let group = reader.read_u8("map_neighborhood.neighbor.group")?;
    let projected_texture_bounds = MapBounds {
        minimum: read_vec3(
            reader,
            "map_neighborhood.neighbor.projected_texture_bounds.minimum",
        )?,
        maximum: read_vec3(
            reader,
            "map_neighborhood.neighbor.projected_texture_bounds.maximum",
        )?,
    };
    let visibility_bounds = MapBounds {
        minimum: read_vec3(
            reader,
            "map_neighborhood.neighbor.visibility_bounds.minimum",
        )?,
        maximum: read_vec3(
            reader,
            "map_neighborhood.neighbor.visibility_bounds.maximum",
        )?,
    };
    let bounding_sphere = MapBoundingSphere {
        center: read_vec3(reader, "map_neighborhood.neighbor.bounding_sphere.center")?,
        radius: reader.read_f32_le("map_neighborhood.neighbor.bounding_sphere.radius")?,
    };
    let translation = read_vec3(reader, "map_neighborhood.neighbor.translation")?;
    Ok(NeighborMapReference {
        map_resource_key,
        group,
        projected_texture_bounds,
        visibility_bounds,
        bounding_sphere,
        translation,
    })
}

fn read_point_sequence(
    reader: &mut ByteReader<'_>,
) -> MapNeighborhoodResult<NeighborhoodPointSequence> {
    let leading_values = [
        reader.read_u16_le("map_neighborhood.sequence.leading_values[0]")?,
        reader.read_u16_le("map_neighborhood.sequence.leading_values[1]")?,
        reader.read_u16_le("map_neighborhood.sequence.leading_values[2]")?,
    ];
    let metadata = reader.read_array("map_neighborhood.sequence.metadata")?;
    let point_count = usize::from(reader.read_u16_le("map_neighborhood.sequence.point_count")?);
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        points.push([
            reader.read_i16_le("map_neighborhood.sequence.point.x")?,
            reader.read_i16_le("map_neighborhood.sequence.point.y")?,
        ]);
    }
    let trailing_byte = reader.read_u8("map_neighborhood.sequence.trailing_byte")?;
    let trailing_value = reader.read_u16_le("map_neighborhood.sequence.trailing_value")?;
    Ok(NeighborhoodPointSequence {
        leading_values,
        metadata,
        points,
        trailing_byte,
        trailing_value,
    })
}

fn read_vec3(reader: &mut ByteReader<'_>, field: &'static str) -> MapNeighborhoodResult<[f32; 3]> {
    Ok([
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
    ])
}

fn validate_map_neighborhood(neighborhood: &MapNeighborhood) -> MapNeighborhoodResult<()> {
    validate_count("neighbors", neighborhood.neighbors.len())?;
    validate_count("point sequences", neighborhood.point_sequences.len())?;

    for (neighbor_index, neighbor) in neighborhood.neighbors.iter().enumerate() {
        validate_bounds(
            neighbor_index,
            "projected-texture bounds",
            &neighbor.projected_texture_bounds,
        )?;
        validate_bounds(
            neighbor_index,
            "visibility bounds",
            &neighbor.visibility_bounds,
        )?;
        validate_vec3(
            "neighbor.bounding_sphere.center",
            neighbor.bounding_sphere.center,
        )?;
        validate_float(
            "neighbor.bounding_sphere.radius",
            neighbor.bounding_sphere.radius,
        )?;
        validate_vec3("neighbor.translation", neighbor.translation)?;

        if neighbor.bounding_sphere.radius < 0.0 {
            return Err(MapNeighborhoodError::NegativeSphereRadius {
                neighbor: neighbor_index,
                radius: neighbor.bounding_sphere.radius,
            });
        }
    }

    for sequence in &neighborhood.point_sequences {
        validate_count("sequence points", sequence.points.len())?;
    }
    Ok(())
}

fn validate_bounds(
    neighbor: usize,
    field: &'static str,
    bounds: &MapBounds,
) -> MapNeighborhoodResult<()> {
    validate_vec3(field, bounds.minimum)?;
    validate_vec3(field, bounds.maximum)?;
    for axis in 0..3 {
        let minimum = bounds.minimum[axis];
        let maximum = bounds.maximum[axis];
        if minimum > maximum {
            return Err(MapNeighborhoodError::ReversedBounds {
                neighbor,
                field,
                axis,
                minimum,
                maximum,
            });
        }
    }
    Ok(())
}

fn validate_vec3(field: &'static str, value: [f32; 3]) -> MapNeighborhoodResult<()> {
    for component in value {
        validate_float(field, component)?;
    }
    Ok(())
}

fn validate_float(field: &'static str, value: f32) -> MapNeighborhoodResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(MapNeighborhoodError::NonFiniteFloat { field })
    }
}

fn validate_count(field: &'static str, count: usize) -> MapNeighborhoodResult<()> {
    if count > usize::from(u16::MAX) {
        Err(MapNeighborhoodError::CountOverflow {
            field,
            count,
            maximum: usize::from(u16::MAX),
        })
    } else {
        Ok(())
    }
}

fn write_neighbor(output: &mut Vec<u8>, neighbor: &NeighborMapReference) {
    output.extend_from_slice(&neighbor.map_resource_key.to_le_bytes());
    output.push(neighbor.group);
    write_vec3(output, neighbor.projected_texture_bounds.minimum);
    write_vec3(output, neighbor.projected_texture_bounds.maximum);
    write_vec3(output, neighbor.visibility_bounds.minimum);
    write_vec3(output, neighbor.visibility_bounds.maximum);
    write_vec3(output, neighbor.bounding_sphere.center);
    output.extend_from_slice(&neighbor.bounding_sphere.radius.to_le_bytes());
    write_vec3(output, neighbor.translation);
}

fn write_point_sequence(output: &mut Vec<u8>, sequence: &NeighborhoodPointSequence) {
    for value in sequence.leading_values {
        output.extend_from_slice(&value.to_le_bytes());
    }
    output.extend_from_slice(&sequence.metadata);
    output.extend_from_slice(&(sequence.points.len() as u16).to_le_bytes());
    for point in &sequence.points {
        output.extend_from_slice(&point[0].to_le_bytes());
        output.extend_from_slice(&point[1].to_le_bytes());
    }
    output.push(sequence.trailing_byte);
    output.extend_from_slice(&sequence.trailing_value.to_le_bytes());
}

fn write_vec3(output: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        output.extend_from_slice(&component.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_neighborhood() -> MapNeighborhood {
        MapNeighborhood {
            preamble: [8, 10],
            neighbors: vec![NeighborMapReference {
                map_resource_key: 0x1020_3040,
                group: 7,
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
                    radius: 9.0,
                },
                translation: [10.0, 20.0, 30.0],
            }],
            point_sequences: vec![NeighborhoodPointSequence {
                leading_values: [1, 0x203, 0x405],
                metadata: [0xa5; 8],
                points: vec![[-2, 3], [i16::MIN, i16::MAX]],
                trailing_byte: 0xc3,
                trailing_value: 0x607,
            }],
        }
    }

    #[test]
    fn decodes_and_encodes_every_field_in_wire_order() {
        let expected = sample_neighborhood();
        let bytes = write_map_neighborhood_bytes(&expected).unwrap();

        assert_eq!(&bytes[0..8], &[8, 0, 0, 0, 10, 0, 0, 0]);
        assert_eq!(&bytes[8..10], &[1, 0]);
        assert_eq!(&bytes[10..14], &0x1020_3040_i32.to_le_bytes());
        assert_eq!(bytes[14], 7);
        assert_eq!(&bytes[15..19], &(-10.0_f32).to_le_bytes());
        assert_eq!(&bytes[27..31], &40.0_f32.to_le_bytes());
        assert_eq!(&bytes[39..43], &(-1.0_f32).to_le_bytes());
        assert_eq!(bytes.len(), 8 + 2 + 81 + 2 + 19 + 8);
        assert_eq!(decode_map_neighborhood(&bytes).unwrap(), expected);
    }

    #[test]
    fn accepts_the_observed_empty_payload_shape() {
        let bytes = [8, 0, 0, 0, 10, 0, 0, 0, 0, 0, 0, 0];
        let neighborhood = decode_map_neighborhood(&bytes).unwrap();

        assert_eq!(neighborhood.preamble, [8, 10]);
        assert!(neighborhood.neighbors.is_empty());
        assert!(neighborhood.point_sequences.is_empty());
        assert_eq!(write_map_neighborhood_bytes(&neighborhood).unwrap(), bytes);
    }

    #[test]
    fn exposes_resource_and_point_summaries() {
        let mut neighborhood = sample_neighborhood();
        neighborhood
            .neighbors
            .push(neighborhood.neighbors[0].clone());
        neighborhood.neighbors.push(NeighborMapReference {
            map_resource_key: -4,
            ..neighborhood.neighbors[0].clone()
        });

        assert_eq!(neighborhood.neighbor_map_resource_keys(), [-4, 0x1020_3040]);
        assert_eq!(neighborhood.point_count(), 2);
    }

    #[test]
    fn rejects_truncation_and_trailing_bytes() {
        let bytes = write_map_neighborhood_bytes(&sample_neighborhood()).unwrap();
        assert!(matches!(
            decode_map_neighborhood(&bytes[..bytes.len() - 1]),
            Err(MapNeighborhoodError::Truncated(_))
        ));

        let mut trailing = bytes;
        trailing.push(0);
        assert_eq!(
            decode_map_neighborhood(&trailing),
            Err(MapNeighborhoodError::TrailingBytes { count: 1 })
        );
    }

    #[test]
    fn rejects_invalid_spatial_values() {
        let mut neighborhood = sample_neighborhood();
        neighborhood.neighbors[0].translation[1] = f32::NAN;
        assert!(matches!(
            write_map_neighborhood_bytes(&neighborhood),
            Err(MapNeighborhoodError::NonFiniteFloat { .. })
        ));

        let mut neighborhood = sample_neighborhood();
        neighborhood.neighbors[0].visibility_bounds.minimum[2] = 7.0;
        assert!(matches!(
            write_map_neighborhood_bytes(&neighborhood),
            Err(MapNeighborhoodError::ReversedBounds {
                field: "visibility bounds",
                axis: 2,
                ..
            })
        ));

        let mut neighborhood = sample_neighborhood();
        neighborhood.neighbors[0].projected_texture_bounds.minimum[0] = 41.0;
        assert!(matches!(
            write_map_neighborhood_bytes(&neighborhood),
            Err(MapNeighborhoodError::ReversedBounds {
                field: "projected-texture bounds",
                axis: 0,
                ..
            })
        ));

        let mut neighborhood = sample_neighborhood();
        neighborhood.neighbors[0].bounding_sphere.radius = -0.5;
        assert!(matches!(
            write_map_neighborhood_bytes(&neighborhood),
            Err(MapNeighborhoodError::NegativeSphereRadius { .. })
        ));
    }

    #[test]
    fn rejects_counts_that_do_not_fit_the_format() {
        let mut neighborhood = sample_neighborhood();
        neighborhood.point_sequences[0].points = vec![[0, 0]; usize::from(u16::MAX) + 1];
        assert!(matches!(
            write_map_neighborhood_bytes(&neighborhood),
            Err(MapNeighborhoodError::CountOverflow {
                field: "sequence points",
                ..
            })
        ));
    }
}
