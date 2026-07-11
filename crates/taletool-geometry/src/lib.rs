//! Typed support for geometry payloads stored in `NStgData` and `NStgeData` archives.
//!
//! The two archive families use the same payload layout. A payload contains
//! bounding metadata, animation timing, parallel vertex attribute arrays,
//! triangle-list index groups, and a recursive transform/render node forest.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// Number of bytes in the fixed geometry header.
pub const GEOMETRY_HEADER_LEN: usize = 0x34;
/// Maximum supported nesting depth for geometry nodes.
pub const GEOMETRY_MAX_NODE_DEPTH: usize = 128;

#[derive(Debug, Error)]
pub enum GeometryError {
    #[error(transparent)]
    Truncated(#[from] ByteReadError),
    #[error("geometry payload has {count} trailing bytes")]
    TrailingBytes { count: usize },
    #[error("geometry field {field} contains a non-finite floating-point value")]
    NonFiniteFloat { field: &'static str },
    #[error(
        "geometry bounds are reversed on axis {axis}: minimum {minimum} exceeds maximum {maximum}"
    )]
    ReversedBounds {
        axis: usize,
        minimum: f32,
        maximum: f32,
    },
    #[error("geometry bounding sphere has invalid negative radius {radius}")]
    NegativeSphereRadius { radius: f32 },
    #[error(
        "geometry animation timing is invalid: first={first_frame}, last={last_frame}, rate={frame_rate}, step={keyframe_step}"
    )]
    InvalidAnimationTiming {
        first_frame: i16,
        last_frame: i16,
        frame_rate: i16,
        keyframe_step: i16,
    },
    #[error("geometry {field} count slot has non-zero reserved byte {value:#04x}")]
    NonZeroReservedByte { field: &'static str, value: u8 },
    #[error("geometry triangle list {triangle_list} is empty")]
    EmptyTriangleList { triangle_list: usize },
    #[error(
        "geometry triangle list {triangle_list} has {count} indices; triangle lists require a multiple of three"
    )]
    InvalidTriangleListIndexCount { triangle_list: usize, count: usize },
    #[error(
        "geometry triangle list {triangle_list} index {index} references vertex {vertex_index}, but only {vertex_count} vertices exist"
    )]
    InvalidVertexIndex {
        triangle_list: usize,
        index: usize,
        vertex_index: u16,
        vertex_count: usize,
    },
    #[error("geometry node {node} batch {batch} has invalid culling flag {value}")]
    InvalidCullingFlag {
        node: usize,
        batch: usize,
        value: u8,
    },
    #[error(
        "geometry node {node} batch {batch} references triangle list {triangle_list_index}, but only {triangle_list_count} lists exist"
    )]
    InvalidBatchTriangleList {
        node: usize,
        batch: usize,
        triangle_list_index: u16,
        triangle_list_count: usize,
    },
    #[error(
        "geometry node {node} {channel} keyframes are not strictly ordered at index {index}: {previous} then {current}"
    )]
    UnorderedKeyframes {
        node: usize,
        channel: &'static str,
        index: usize,
        previous: u16,
        current: u16,
    },
    #[error("geometry node nesting depth {depth} exceeds the supported maximum of {limit}")]
    NodeDepthExceeded { depth: usize, limit: usize },
    #[error("geometry {field} has {count} items; maximum is {maximum}")]
    CountOverflow {
        field: &'static str,
        count: usize,
        maximum: usize,
    },
}

pub type GeometryResult<T> = std::result::Result<T, GeometryError>;

/// Axis-aligned bounds stored in the fixed geometry header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisAlignedBounds {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// Bounding sphere stored after the axis-aligned bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundingSphere {
    pub center: [f32; 3],
    pub radius: f32,
}

/// Fixed metadata that precedes the geometry arrays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryHeader {
    pub bounds: AxisAlignedBounds,
    pub bounding_sphere: BoundingSphere,
    pub first_frame: i16,
    pub last_frame: i16,
    pub frame_rate: i16,
    pub keyframe_step: i16,
    pub texture_coordinate_scale: f32,
}

/// One logical vertex assembled from the payload's three parallel arrays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryVertex {
    pub position: [f32; 3],
    pub texture_coordinates: [i16; 2],
    pub normal: [i8; 3],
}

/// A three-component translation or scale animation keyframe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorKeyframe {
    pub time: u16,
    pub value: [f32; 3],
}

/// A packed quaternion animation keyframe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationKeyframe {
    pub time: u16,
    pub rotation: [i16; 4],
}

/// One texture-bound draw operation owned by a geometry node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryBatch {
    pub texture_resource_key: i32,
    pub disable_culling: bool,
    pub triangle_list_index: u16,
}

/// One transform node and its animation, draw batches, and child nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryNode {
    pub base_translation: [f32; 3],
    pub base_rotation: [i16; 4],
    pub base_scale: [f32; 3],
    pub translation_keyframes: Vec<VectorKeyframe>,
    pub rotation_keyframes: Vec<RotationKeyframe>,
    pub scale_keyframes: Vec<VectorKeyframe>,
    pub batches: Vec<GeometryBatch>,
    pub children: Vec<GeometryNode>,
}

/// Fully decoded geometry payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub header: GeometryHeader,
    pub vertices: Vec<GeometryVertex>,
    pub triangle_lists: Vec<Vec<u16>>,
    pub root_nodes: Vec<GeometryNode>,
}

impl Geometry {
    /// Count all nodes in the root forest.
    pub fn node_count(&self) -> usize {
        self.nodes().count()
    }

    /// Count all draw batches in the node forest.
    pub fn batch_count(&self) -> usize {
        self.nodes().map(|node| node.batches.len()).sum()
    }

    /// Count all triangles across the index groups.
    pub fn triangle_count(&self) -> usize {
        self.triangle_lists.iter().map(|list| list.len() / 3).sum()
    }

    /// Count all transform keyframes in the node forest.
    pub fn keyframe_count(&self) -> usize {
        self.nodes()
            .map(|node| {
                node.translation_keyframes.len()
                    + node.rotation_keyframes.len()
                    + node.scale_keyframes.len()
            })
            .sum()
    }

    /// Return sorted, unique texture resource keys referenced by batches.
    pub fn texture_resource_keys(&self) -> Vec<i32> {
        self.nodes()
            .flat_map(|node| node.batches.iter().map(|batch| batch.texture_resource_key))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn nodes(&self) -> GeometryNodes<'_> {
        GeometryNodes {
            pending: self.root_nodes.iter().rev().collect(),
        }
    }
}

struct GeometryNodes<'a> {
    pending: Vec<&'a GeometryNode>,
}

impl<'a> Iterator for GeometryNodes<'a> {
    type Item = &'a GeometryNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.pending.pop()?;
        self.pending.extend(node.children.iter().rev());
        Some(node)
    }
}

/// Decode one `NStgData` or `NStgeData` payload.
pub fn decode_geometry(data: &[u8]) -> GeometryResult<Geometry> {
    let mut reader = ByteReader::new(data);
    let header = read_header(&mut reader)?;
    validate_header(&header)?;

    let vertex_count = usize::from(reader.read_u16_le("geometry.vertex_count")?);
    let mut positions = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        positions.push(read_vec3(&mut reader, "geometry.vertex.position")?);
    }

    let mut texture_coordinates = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        texture_coordinates.push([
            reader.read_i16_le("geometry.vertex.texture_coordinates.u")?,
            reader.read_i16_le("geometry.vertex.texture_coordinates.v")?,
        ]);
    }

    let mut normals = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        normals.push([
            reader.read_i8("geometry.vertex.normal.x")?,
            reader.read_i8("geometry.vertex.normal.y")?,
            reader.read_i8("geometry.vertex.normal.z")?,
        ]);
    }

    let vertices = positions
        .into_iter()
        .zip(texture_coordinates)
        .zip(normals)
        .map(|((position, texture_coordinates), normal)| GeometryVertex {
            position,
            texture_coordinates,
            normal,
        })
        .collect::<Vec<_>>();
    validate_vertices(&vertices)?;

    let triangle_list_count = usize::from(read_count_slot(
        &mut reader,
        "geometry.triangle_list_count",
    )?);
    let mut triangle_lists = Vec::with_capacity(triangle_list_count);
    for triangle_list in 0..triangle_list_count {
        let index_count = usize::from(reader.read_u16_le("geometry.triangle_list.index_count")?);
        let mut indices = Vec::with_capacity(index_count);
        for index in 0..index_count {
            let vertex_index = reader.read_u16_le("geometry.triangle_list.vertex_index")?;
            if usize::from(vertex_index) >= vertex_count {
                return Err(GeometryError::InvalidVertexIndex {
                    triangle_list,
                    index,
                    vertex_index,
                    vertex_count,
                });
            }
            indices.push(vertex_index);
        }
        validate_triangle_list(triangle_list, &indices)?;
        triangle_lists.push(indices);
    }

    let root_count = usize::from(read_count_slot(&mut reader, "geometry.root_node_count")?);
    let mut root_nodes = Vec::with_capacity(root_count);
    let mut next_node_index = 0;
    for _ in 0..root_count {
        root_nodes.push(read_node(
            &mut reader,
            triangle_list_count,
            1,
            &mut next_node_index,
        )?);
    }

    if reader.remaining() != 0 {
        return Err(GeometryError::TrailingBytes {
            count: reader.remaining(),
        });
    }

    Ok(Geometry {
        header,
        vertices,
        triangle_lists,
        root_nodes,
    })
}

/// Encode typed geometry into the native payload layout.
pub fn write_geometry_bytes(geometry: &Geometry) -> GeometryResult<Vec<u8>> {
    validate_geometry(geometry)?;

    let mut output = Vec::new();
    write_vec3(&mut output, geometry.header.bounds.minimum);
    write_vec3(&mut output, geometry.header.bounds.maximum);
    write_vec3(&mut output, geometry.header.bounding_sphere.center);
    output.extend_from_slice(&geometry.header.bounding_sphere.radius.to_le_bytes());
    output.extend_from_slice(&geometry.header.first_frame.to_le_bytes());
    output.extend_from_slice(&geometry.header.last_frame.to_le_bytes());
    output.extend_from_slice(&geometry.header.frame_rate.to_le_bytes());
    output.extend_from_slice(&geometry.header.keyframe_step.to_le_bytes());
    output.extend_from_slice(&geometry.header.texture_coordinate_scale.to_le_bytes());

    output.extend_from_slice(&(geometry.vertices.len() as u16).to_le_bytes());
    for vertex in &geometry.vertices {
        write_vec3(&mut output, vertex.position);
    }
    for vertex in &geometry.vertices {
        for coordinate in vertex.texture_coordinates {
            output.extend_from_slice(&coordinate.to_le_bytes());
        }
    }
    for vertex in &geometry.vertices {
        for normal in vertex.normal {
            output.push(normal as u8);
        }
    }

    write_count_slot(&mut output, geometry.triangle_lists.len());
    for indices in &geometry.triangle_lists {
        output.extend_from_slice(&(indices.len() as u16).to_le_bytes());
        for index in indices {
            output.extend_from_slice(&index.to_le_bytes());
        }
    }

    write_count_slot(&mut output, geometry.root_nodes.len());
    for node in &geometry.root_nodes {
        write_node(&mut output, node);
    }
    Ok(output)
}

fn read_header(reader: &mut ByteReader<'_>) -> GeometryResult<GeometryHeader> {
    Ok(GeometryHeader {
        bounds: AxisAlignedBounds {
            minimum: read_vec3(reader, "geometry.header.bounds.minimum")?,
            maximum: read_vec3(reader, "geometry.header.bounds.maximum")?,
        },
        bounding_sphere: BoundingSphere {
            center: read_vec3(reader, "geometry.header.bounding_sphere.center")?,
            radius: reader.read_f32_le("geometry.header.bounding_sphere.radius")?,
        },
        first_frame: reader.read_i16_le("geometry.header.first_frame")?,
        last_frame: reader.read_i16_le("geometry.header.last_frame")?,
        frame_rate: reader.read_i16_le("geometry.header.frame_rate")?,
        keyframe_step: reader.read_i16_le("geometry.header.keyframe_step")?,
        texture_coordinate_scale: reader.read_f32_le("geometry.header.texture_coordinate_scale")?,
    })
}

fn read_node(
    reader: &mut ByteReader<'_>,
    triangle_list_count: usize,
    depth: usize,
    next_node_index: &mut usize,
) -> GeometryResult<GeometryNode> {
    if depth > GEOMETRY_MAX_NODE_DEPTH {
        return Err(GeometryError::NodeDepthExceeded {
            depth,
            limit: GEOMETRY_MAX_NODE_DEPTH,
        });
    }
    let node_index = *next_node_index;
    *next_node_index += 1;

    let base_translation = read_vec3(reader, "geometry.node.base_translation")?;
    let base_rotation = read_rotation(reader, "geometry.node.base_rotation")?;
    let base_scale = read_vec3(reader, "geometry.node.base_scale")?;
    validate_vec3("geometry.node.base_translation", base_translation)?;
    validate_vec3("geometry.node.base_scale", base_scale)?;

    let translation_keyframes = read_vector_keyframes(reader)?;
    validate_vector_keyframes(node_index, "translation", &translation_keyframes)?;
    let rotation_keyframes = read_rotation_keyframes(reader)?;
    validate_rotation_keyframes(node_index, &rotation_keyframes)?;
    let scale_keyframes = read_vector_keyframes(reader)?;
    validate_vector_keyframes(node_index, "scale", &scale_keyframes)?;

    let batch_count = usize::from(reader.read_u16_le("geometry.node.batch_count")?);
    let mut batches = Vec::with_capacity(batch_count);
    for batch in 0..batch_count {
        let texture_resource_key =
            reader.read_i32_le("geometry.node.batch.texture_resource_key")?;
        let culling_flag = reader.read_u8("geometry.node.batch.disable_culling")?;
        let disable_culling = match culling_flag {
            0 => false,
            1 => true,
            value => {
                return Err(GeometryError::InvalidCullingFlag {
                    node: node_index,
                    batch,
                    value,
                });
            }
        };
        let triangle_list_index = reader.read_u16_le("geometry.node.batch.triangle_list_index")?;
        if usize::from(triangle_list_index) >= triangle_list_count {
            return Err(GeometryError::InvalidBatchTriangleList {
                node: node_index,
                batch,
                triangle_list_index,
                triangle_list_count,
            });
        }
        batches.push(GeometryBatch {
            texture_resource_key,
            disable_culling,
            triangle_list_index,
        });
    }

    let child_count = usize::from(read_count_slot(reader, "geometry.node.child_count")?);
    let mut children = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        children.push(read_node(
            reader,
            triangle_list_count,
            depth + 1,
            next_node_index,
        )?);
    }

    Ok(GeometryNode {
        base_translation,
        base_rotation,
        base_scale,
        translation_keyframes,
        rotation_keyframes,
        scale_keyframes,
        batches,
        children,
    })
}

fn read_vector_keyframes(reader: &mut ByteReader<'_>) -> GeometryResult<Vec<VectorKeyframe>> {
    let count = usize::from(reader.read_u16_le("geometry.node.vector_keyframe_count")?);
    let mut keyframes = Vec::with_capacity(count);
    for _ in 0..count {
        let time = reader.read_u16_le("geometry.node.vector_keyframe.time")?;
        let value = read_vec3(reader, "geometry.node.vector_keyframe.value")?;
        validate_vec3("geometry.node.vector_keyframe.value", value)?;
        keyframes.push(VectorKeyframe { time, value });
    }
    Ok(keyframes)
}

fn read_rotation_keyframes(reader: &mut ByteReader<'_>) -> GeometryResult<Vec<RotationKeyframe>> {
    let count = usize::from(reader.read_u16_le("geometry.node.rotation_keyframe_count")?);
    let mut keyframes = Vec::with_capacity(count);
    for _ in 0..count {
        keyframes.push(RotationKeyframe {
            time: reader.read_u16_le("geometry.node.rotation_keyframe.time")?,
            rotation: read_rotation(reader, "geometry.node.rotation_keyframe.rotation")?,
        });
    }
    Ok(keyframes)
}

fn read_rotation(reader: &mut ByteReader<'_>, field: &'static str) -> GeometryResult<[i16; 4]> {
    Ok([
        reader.read_i16_le(field)?,
        reader.read_i16_le(field)?,
        reader.read_i16_le(field)?,
        reader.read_i16_le(field)?,
    ])
}

fn read_vec3(reader: &mut ByteReader<'_>, field: &'static str) -> GeometryResult<[f32; 3]> {
    Ok([
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
    ])
}

fn read_count_slot(reader: &mut ByteReader<'_>, field: &'static str) -> GeometryResult<u8> {
    let count = reader.read_u8(field)?;
    let reserved = reader.read_u8(field)?;
    if reserved != 0 {
        return Err(GeometryError::NonZeroReservedByte {
            field,
            value: reserved,
        });
    }
    Ok(count)
}

fn validate_geometry(geometry: &Geometry) -> GeometryResult<()> {
    validate_header(&geometry.header)?;
    validate_vertices(&geometry.vertices)?;
    check_count("vertex", geometry.vertices.len(), u16::MAX as usize)?;
    check_count(
        "triangle list",
        geometry.triangle_lists.len(),
        u8::MAX as usize,
    )?;
    for (triangle_list, indices) in geometry.triangle_lists.iter().enumerate() {
        check_count("triangle-list index", indices.len(), u16::MAX as usize)?;
        validate_triangle_list(triangle_list, indices)?;
        for (index, &vertex_index) in indices.iter().enumerate() {
            if usize::from(vertex_index) >= geometry.vertices.len() {
                return Err(GeometryError::InvalidVertexIndex {
                    triangle_list,
                    index,
                    vertex_index,
                    vertex_count: geometry.vertices.len(),
                });
            }
        }
    }
    check_count("root node", geometry.root_nodes.len(), u8::MAX as usize)?;

    let mut next_node_index = 0;
    for node in &geometry.root_nodes {
        validate_node(node, geometry.triangle_lists.len(), 1, &mut next_node_index)?;
    }
    Ok(())
}

fn validate_header(header: &GeometryHeader) -> GeometryResult<()> {
    validate_vec3("geometry.header.bounds.minimum", header.bounds.minimum)?;
    validate_vec3("geometry.header.bounds.maximum", header.bounds.maximum)?;
    validate_vec3(
        "geometry.header.bounding_sphere.center",
        header.bounding_sphere.center,
    )?;
    validate_float(
        "geometry.header.bounding_sphere.radius",
        header.bounding_sphere.radius,
    )?;
    validate_float(
        "geometry.header.texture_coordinate_scale",
        header.texture_coordinate_scale,
    )?;
    for axis in 0..3 {
        if header.bounds.minimum[axis] > header.bounds.maximum[axis] {
            return Err(GeometryError::ReversedBounds {
                axis,
                minimum: header.bounds.minimum[axis],
                maximum: header.bounds.maximum[axis],
            });
        }
    }
    if header.bounding_sphere.radius < 0.0 {
        return Err(GeometryError::NegativeSphereRadius {
            radius: header.bounding_sphere.radius,
        });
    }
    if header.first_frame < 0
        || header.last_frame < header.first_frame
        || header.frame_rate <= 0
        || header.keyframe_step <= 0
    {
        return Err(GeometryError::InvalidAnimationTiming {
            first_frame: header.first_frame,
            last_frame: header.last_frame,
            frame_rate: header.frame_rate,
            keyframe_step: header.keyframe_step,
        });
    }
    Ok(())
}

fn validate_vertices(vertices: &[GeometryVertex]) -> GeometryResult<()> {
    for vertex in vertices {
        validate_vec3("geometry.vertex.position", vertex.position)?;
    }
    Ok(())
}

fn validate_triangle_list(triangle_list: usize, indices: &[u16]) -> GeometryResult<()> {
    if indices.is_empty() {
        return Err(GeometryError::EmptyTriangleList { triangle_list });
    }
    if !indices.len().is_multiple_of(3) {
        return Err(GeometryError::InvalidTriangleListIndexCount {
            triangle_list,
            count: indices.len(),
        });
    }
    Ok(())
}

fn validate_node(
    node: &GeometryNode,
    triangle_list_count: usize,
    depth: usize,
    next_node_index: &mut usize,
) -> GeometryResult<()> {
    if depth > GEOMETRY_MAX_NODE_DEPTH {
        return Err(GeometryError::NodeDepthExceeded {
            depth,
            limit: GEOMETRY_MAX_NODE_DEPTH,
        });
    }
    let node_index = *next_node_index;
    *next_node_index += 1;

    validate_vec3("geometry.node.base_translation", node.base_translation)?;
    validate_vec3("geometry.node.base_scale", node.base_scale)?;
    check_count(
        "node translation keyframe",
        node.translation_keyframes.len(),
        u16::MAX as usize,
    )?;
    check_count(
        "node rotation keyframe",
        node.rotation_keyframes.len(),
        u16::MAX as usize,
    )?;
    check_count(
        "node scale keyframe",
        node.scale_keyframes.len(),
        u16::MAX as usize,
    )?;
    check_count("node batch", node.batches.len(), u16::MAX as usize)?;
    check_count("node child", node.children.len(), u8::MAX as usize)?;

    validate_vector_keyframes(node_index, "translation", &node.translation_keyframes)?;
    validate_rotation_keyframes(node_index, &node.rotation_keyframes)?;
    validate_vector_keyframes(node_index, "scale", &node.scale_keyframes)?;
    for keyframe in node
        .translation_keyframes
        .iter()
        .chain(&node.scale_keyframes)
    {
        validate_vec3("geometry.node.vector_keyframe.value", keyframe.value)?;
    }
    for (batch, value) in node.batches.iter().enumerate() {
        if usize::from(value.triangle_list_index) >= triangle_list_count {
            return Err(GeometryError::InvalidBatchTriangleList {
                node: node_index,
                batch,
                triangle_list_index: value.triangle_list_index,
                triangle_list_count,
            });
        }
    }
    for child in &node.children {
        validate_node(child, triangle_list_count, depth + 1, next_node_index)?;
    }
    Ok(())
}

fn validate_vector_keyframes(
    node: usize,
    channel: &'static str,
    keyframes: &[VectorKeyframe],
) -> GeometryResult<()> {
    validate_keyframe_order(
        node,
        channel,
        keyframes.iter().map(|keyframe| keyframe.time),
    )
}

fn validate_rotation_keyframes(node: usize, keyframes: &[RotationKeyframe]) -> GeometryResult<()> {
    validate_keyframe_order(
        node,
        "rotation",
        keyframes.iter().map(|keyframe| keyframe.time),
    )
}

fn validate_keyframe_order(
    node: usize,
    channel: &'static str,
    times: impl Iterator<Item = u16>,
) -> GeometryResult<()> {
    let mut previous = None;
    for (index, current) in times.enumerate() {
        if let Some(previous) = previous
            && previous >= current
        {
            return Err(GeometryError::UnorderedKeyframes {
                node,
                channel,
                index,
                previous,
                current,
            });
        }
        previous = Some(current);
    }
    Ok(())
}

fn validate_vec3(field: &'static str, value: [f32; 3]) -> GeometryResult<()> {
    for component in value {
        validate_float(field, component)?;
    }
    Ok(())
}

fn validate_float(field: &'static str, value: f32) -> GeometryResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(GeometryError::NonFiniteFloat { field })
    }
}

fn check_count(field: &'static str, count: usize, maximum: usize) -> GeometryResult<()> {
    if count > maximum {
        Err(GeometryError::CountOverflow {
            field,
            count,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn write_node(output: &mut Vec<u8>, node: &GeometryNode) {
    write_vec3(output, node.base_translation);
    write_rotation(output, node.base_rotation);
    write_vec3(output, node.base_scale);
    write_vector_keyframes(output, &node.translation_keyframes);
    write_rotation_keyframes(output, &node.rotation_keyframes);
    write_vector_keyframes(output, &node.scale_keyframes);
    output.extend_from_slice(&(node.batches.len() as u16).to_le_bytes());
    for batch in &node.batches {
        output.extend_from_slice(&batch.texture_resource_key.to_le_bytes());
        output.push(u8::from(batch.disable_culling));
        output.extend_from_slice(&batch.triangle_list_index.to_le_bytes());
    }
    write_count_slot(output, node.children.len());
    for child in &node.children {
        write_node(output, child);
    }
}

fn write_vector_keyframes(output: &mut Vec<u8>, keyframes: &[VectorKeyframe]) {
    output.extend_from_slice(&(keyframes.len() as u16).to_le_bytes());
    for keyframe in keyframes {
        output.extend_from_slice(&keyframe.time.to_le_bytes());
        write_vec3(output, keyframe.value);
    }
}

fn write_rotation_keyframes(output: &mut Vec<u8>, keyframes: &[RotationKeyframe]) {
    output.extend_from_slice(&(keyframes.len() as u16).to_le_bytes());
    for keyframe in keyframes {
        output.extend_from_slice(&keyframe.time.to_le_bytes());
        write_rotation(output, keyframe.rotation);
    }
}

fn write_rotation(output: &mut Vec<u8>, rotation: [i16; 4]) {
    for component in rotation {
        output.extend_from_slice(&component.to_le_bytes());
    }
}

fn write_vec3(output: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        output.extend_from_slice(&component.to_le_bytes());
    }
}

fn write_count_slot(output: &mut Vec<u8>, count: usize) {
    output.extend_from_slice(&[count as u8, 0]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> GeometryHeader {
        GeometryHeader {
            bounds: AxisAlignedBounds {
                minimum: [-1.0, -2.0, -3.0],
                maximum: [4.0, 5.0, 6.0],
            },
            bounding_sphere: BoundingSphere {
                center: [1.0, 1.5, 2.0],
                radius: 8.0,
            },
            first_frame: 0,
            last_frame: 100,
            frame_rate: 30,
            keyframe_step: 160,
            texture_coordinate_scale: 1.0 / 32767.0,
        }
    }

    fn vertex(position: [f32; 3], texture_coordinates: [i16; 2]) -> GeometryVertex {
        GeometryVertex {
            position,
            texture_coordinates,
            normal: [0, 127, -128],
        }
    }

    fn node() -> GeometryNode {
        GeometryNode {
            base_translation: [1.0, 2.0, 3.0],
            base_rotation: [0, 0, 0, 32767],
            base_scale: [1.0, 1.0, 1.0],
            translation_keyframes: vec![
                VectorKeyframe {
                    time: 0,
                    value: [1.0, 2.0, 3.0],
                },
                VectorKeyframe {
                    time: 160,
                    value: [4.0, 5.0, 6.0],
                },
            ],
            rotation_keyframes: vec![RotationKeyframe {
                time: 0,
                rotation: [1, 2, 3, 32760],
            }],
            scale_keyframes: Vec::new(),
            batches: vec![GeometryBatch {
                texture_resource_key: 0x0500_0123,
                disable_culling: true,
                triangle_list_index: 0,
            }],
            children: Vec::new(),
        }
    }

    fn geometry_fixture() -> Geometry {
        Geometry {
            header: header(),
            vertices: vec![
                vertex([0.0, 0.0, 0.0], [0, 0]),
                vertex([1.0, 0.0, 0.0], [32767, 0]),
                vertex([0.0, 1.0, 0.0], [0, 32767]),
            ],
            triangle_lists: vec![vec![0, 1, 2]],
            root_nodes: vec![node()],
        }
    }

    #[test]
    fn geometry_round_trips_byte_for_byte() {
        let geometry = geometry_fixture();
        let encoded = write_geometry_bytes(&geometry).unwrap();
        assert_eq!(encoded.len(), 210);
        let decoded = decode_geometry(&encoded).unwrap();
        assert_eq!(decoded, geometry);
        assert_eq!(write_geometry_bytes(&decoded).unwrap(), encoded);
        assert_eq!(decoded.node_count(), 1);
        assert_eq!(decoded.batch_count(), 1);
        assert_eq!(decoded.triangle_count(), 1);
        assert_eq!(decoded.keyframe_count(), 3);
        assert_eq!(decoded.texture_resource_keys(), vec![0x0500_0123]);
    }

    #[test]
    fn nested_and_meshless_geometry_round_trips() {
        let mut root = node();
        root.batches.clear();
        root.children.push(node_without_batches());
        let geometry = Geometry {
            header: header(),
            vertices: Vec::new(),
            triangle_lists: Vec::new(),
            root_nodes: vec![root],
        };
        let encoded = write_geometry_bytes(&geometry).unwrap();
        let decoded = decode_geometry(&encoded).unwrap();
        assert_eq!(decoded, geometry);
        assert_eq!(decoded.node_count(), 2);
    }

    #[test]
    fn rejects_invalid_triangle_and_batch_references() {
        let mut geometry = geometry_fixture();
        geometry.triangle_lists[0].push(0);
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::InvalidTriangleListIndexCount { .. })
        ));

        let mut geometry = geometry_fixture();
        geometry.triangle_lists[0].clear();
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::EmptyTriangleList { .. })
        ));

        let mut geometry = geometry_fixture();
        geometry.triangle_lists[0][2] = 3;
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::InvalidVertexIndex { .. })
        ));

        let mut geometry = geometry_fixture();
        geometry.root_nodes[0].batches[0].triangle_list_index = 1;
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::InvalidBatchTriangleList { .. })
        ));
    }

    #[test]
    fn rejects_unordered_keys_and_invalid_header_values() {
        let mut geometry = geometry_fixture();
        geometry.root_nodes[0].translation_keyframes[1].time = 0;
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::UnorderedKeyframes { .. })
        ));

        let mut geometry = geometry_fixture();
        geometry.header.bounds.maximum[0] = -2.0;
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::ReversedBounds { .. })
        ));

        let mut geometry = geometry_fixture();
        geometry.header.texture_coordinate_scale = f32::NAN;
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::NonFiniteFloat { .. })
        ));

        let mut geometry = geometry_fixture();
        geometry.header.frame_rate = 0;
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::InvalidAnimationTiming { .. })
        ));
    }

    #[test]
    fn rejects_counts_that_do_not_fit_the_payload_fields() {
        let mut geometry = geometry_fixture();
        geometry.vertices = vec![geometry.vertices[0].clone(); usize::from(u16::MAX) + 1];
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::CountOverflow {
                field: "vertex",
                ..
            })
        ));

        let mut geometry = geometry_fixture();
        geometry.root_nodes = vec![node_without_batches(); usize::from(u8::MAX) + 1];
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::CountOverflow {
                field: "root node",
                ..
            })
        ));
    }

    #[test]
    fn rejects_noncanonical_flags_reserved_bytes_and_trailing_data() {
        let encoded = write_geometry_bytes(&geometry_fixture()).unwrap();
        let vertex_count = geometry_fixture().vertices.len();
        let triangle_count_slot = GEOMETRY_HEADER_LEN + 2 + vertex_count * 19;

        let mut nonzero_reserved = encoded.clone();
        nonzero_reserved[triangle_count_slot + 1] = 1;
        assert!(matches!(
            decode_geometry(&nonzero_reserved),
            Err(GeometryError::NonZeroReservedByte { .. })
        ));

        let batch_flag = encoded
            .windows(7)
            .position(|window| window[..4] == 0x0500_0123_i32.to_le_bytes())
            .unwrap()
            + 4;
        let mut invalid_flag = encoded.clone();
        invalid_flag[batch_flag] = 2;
        assert!(matches!(
            decode_geometry(&invalid_flag),
            Err(GeometryError::InvalidCullingFlag { .. })
        ));

        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            decode_geometry(&trailing),
            Err(GeometryError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn rejects_truncated_payloads_and_excessive_nesting() {
        let encoded = write_geometry_bytes(&geometry_fixture()).unwrap();
        for end in [0, GEOMETRY_HEADER_LEN - 1, encoded.len() - 1] {
            assert!(matches!(
                decode_geometry(&encoded[..end]),
                Err(GeometryError::Truncated(_))
            ));
        }

        let mut leaf = node();
        leaf.batches.clear();
        for _ in 0..GEOMETRY_MAX_NODE_DEPTH {
            leaf = GeometryNode {
                children: vec![leaf],
                ..node_without_batches()
            };
        }
        let geometry = Geometry {
            header: header(),
            vertices: Vec::new(),
            triangle_lists: Vec::new(),
            root_nodes: vec![leaf],
        };
        assert!(matches!(
            write_geometry_bytes(&geometry),
            Err(GeometryError::NodeDepthExceeded { .. })
        ));
    }

    fn node_without_batches() -> GeometryNode {
        let mut value = node();
        value.batches.clear();
        value
    }
}
