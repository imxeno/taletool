//! Typed support for map payloads stored in `NStuData` archives.
//!
//! A payload combines scene-wide environment and camera metadata, a table of
//! geometry resource keys, and a recursive scene-node forest.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// Number of bytes in the scene-wide payload header.
pub const MAP_HEADER_LEN: usize = 0x85;
/// Length of the unknown header region beginning at offset `0x00`.
pub const MAP_HEADER_UNKNOWN_00_LEN: usize = 0x1e;
/// Length of the unknown header region beginning at offset `0x79`.
pub const MAP_HEADER_UNKNOWN_79_LEN: usize = 0x0a;
/// Maximum supported nesting depth for map nodes.
pub const MAP_MAX_NODE_DEPTH: usize = 128;

#[derive(Debug, Error)]
pub enum MapError {
    #[error(transparent)]
    Truncated(#[from] ByteReadError),
    #[error("map payload has {count} trailing bytes")]
    TrailingBytes { count: usize },
    #[error("map node uses unsupported kind {kind}")]
    UnsupportedNodeKind { kind: u8 },
    #[error("map field {field} uses invalid boolean value {value}")]
    InvalidBoolean { field: &'static str, value: u8 },
    #[error("map {field} has {actual} bytes; expected exactly {expected}")]
    InvalidByteRegionLength {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("map field {field} contains a non-finite floating-point value")]
    NonFiniteFloat { field: &'static str },
    #[error("map bounds are reversed on axis {axis}: minimum {minimum} exceeds maximum {maximum}")]
    ReversedBounds {
        axis: usize,
        minimum: f32,
        maximum: f32,
    },
    #[error("map bounding sphere has invalid negative radius {radius}")]
    NegativeSphereRadius { radius: f32 },
    #[error(
        "map node {node} references geometry-table index {geometry_index}, but only {geometry_count} entries exist"
    )]
    InvalidGeometryIndex {
        node: usize,
        geometry_index: u16,
        geometry_count: usize,
    },
    #[error("map node nesting depth {depth} exceeds the supported maximum of {limit}")]
    NodeDepthExceeded { depth: usize, limit: usize },
    #[error("map {field} has {count} items; maximum is {maximum}")]
    CountOverflow {
        field: &'static str,
        count: usize,
        maximum: usize,
    },
}

pub type MapResult<T> = std::result::Result<T, MapError>;

/// An axis-aligned three-dimensional bounding box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds3 {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// A center point and radius used for hierarchical visibility checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundingSphere {
    pub center: [f32; 3],
    pub radius: f32,
}

/// An eight-bit RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgba8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// Camera angle and the permitted offsets below and above it, in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraAngleLimits {
    pub angle_degrees: i16,
    pub minimum_offset_degrees: i16,
    pub maximum_offset_degrees: i16,
}

/// Scene-wide metadata at the beginning of every map payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapHeader {
    /// Preserved bytes at offsets `0x00..0x1e`.
    pub unknown_00: Vec<u8>,
    /// Group used to associate the payload with companion resource metadata.
    pub resource_group: u8,
    /// World-space bounds used by scene projection and collision paths.
    pub bounds: Bounds3,
    /// Bounds used by the fallback ground-height path.
    pub ground_bounds: Bounds3,
    /// Sphere whose bottom point seeds fallback ground-height queries.
    pub ground_bounding_sphere: BoundingSphere,
    pub ambient_light: Rgba8,
    pub diffuse_light: Rgba8,
    /// Packed fog color passed through to the renderer.
    pub fog_color: u32,
    pub yaw_limits: CameraAngleLimits,
    pub pitch_limits: CameraAngleLimits,
    /// Normalized fog start; the runtime maps `0..=255` to `0..=150` units.
    pub fog_start: u8,
    /// Normalized fog end; the runtime maps `0..=255` to `0..=150` units.
    pub fog_end: u8,
    /// Preserved bytes at offsets `0x79..0x83`.
    pub unknown_79: Vec<u8>,
    /// Whether entering the scene resets yaw to `yaw_limits.angle_degrees`.
    pub reset_yaw: bool,
    /// Preserved final header byte at offset `0x84`.
    pub unknown_84: u8,
}

/// Fields shared by every node that submits geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryNode {
    /// Index into [`Map::geometry_keys`].
    pub geometry_index: u16,
    pub color: Rgba8,
    pub bounds: Bounds3,
    pub bounding_sphere: BoundingSphere,
    pub position: [f32; 3],
    /// Packed quaternion components in X, Y, Z, W order.
    pub rotation: [i16; 4],
}

/// Payload specific to one scene-node kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MapNodeKind {
    /// A visibility/collision grouping node with no geometry of its own.
    Group { bounding_sphere: BoundingSphere },
    /// Geometry submitted without an animation time.
    StaticGeometry { geometry: GeometryNode },
    /// Geometry submitted with an animation frame offset.
    AnimatedGeometry {
        geometry: GeometryNode,
        /// Uniform scale applied to the geometry transform basis vectors.
        uniform_scale: f32,
        frame_offset: u16,
    },
    /// Geometry with animation-driven render state and optional billboard mode.
    EffectGeometry {
        geometry: GeometryNode,
        /// Uniform scale applied to the geometry transform basis vectors.
        uniform_scale: f32,
        frame_offset: u16,
        color_animation_id: i16,
        transform_animation_id: i16,
        texture_animation_id: i16,
        /// Value at runtime node offset `0x66`; its purpose is not established.
        unknown_66: u16,
        billboard: bool,
        source_blend: u16,
        destination_blend: u16,
    },
}

/// One recursive scene node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapNode {
    pub kind: MapNodeKind,
    pub children: Vec<MapNode>,
}

/// Fully decoded `NStuData` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Map {
    pub header: MapHeader,
    /// Geometry archive keys referenced by node-local table indices.
    pub geometry_keys: Vec<i32>,
    pub root_nodes: Vec<MapNode>,
}

impl Map {
    /// Count every node in the recursive forest.
    pub fn node_count(&self) -> usize {
        self.nodes().count()
    }

    /// Count nodes that reference geometry.
    pub fn geometry_node_count(&self) -> usize {
        self.nodes()
            .filter(|node| !matches!(node.kind, MapNodeKind::Group { .. }))
            .count()
    }

    /// Count effect-geometry nodes.
    pub fn effect_node_count(&self) -> usize {
        self.nodes()
            .filter(|node| matches!(node.kind, MapNodeKind::EffectGeometry { .. }))
            .count()
    }

    /// Return sorted, unique geometry keys referenced by nodes.
    pub fn referenced_geometry_keys(&self) -> Vec<i32> {
        self.nodes()
            .filter_map(|node| node.kind.geometry())
            .filter_map(|geometry| self.geometry_keys.get(usize::from(geometry.geometry_index)))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn nodes(&self) -> MapNodes<'_> {
        MapNodes {
            pending: self.root_nodes.iter().rev().collect(),
        }
    }
}

impl MapNodeKind {
    fn geometry(&self) -> Option<&GeometryNode> {
        match self {
            Self::Group { .. } => None,
            Self::StaticGeometry { geometry }
            | Self::AnimatedGeometry { geometry, .. }
            | Self::EffectGeometry { geometry, .. } => Some(geometry),
        }
    }
}

struct MapNodes<'a> {
    pending: Vec<&'a MapNode>,
}

impl<'a> Iterator for MapNodes<'a> {
    type Item = &'a MapNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.pending.pop()?;
        self.pending.extend(node.children.iter().rev());
        Some(node)
    }
}

/// Decode one map payload from an `NStuData` archive entry.
pub fn decode_map(data: &[u8]) -> MapResult<Map> {
    let mut reader = ByteReader::new(data);
    let header = read_header(&mut reader)?;

    let geometry_count = usize::from(reader.read_u16_le("map.geometry_key_count")?);
    let mut geometry_keys = Vec::with_capacity(geometry_count);
    for _ in 0..geometry_count {
        geometry_keys.push(reader.read_i32_le("map.geometry_key")?);
    }

    let mut next_node_index = 0;
    let root_nodes = read_node_list(
        &mut reader,
        geometry_count,
        1,
        &mut next_node_index,
        "map.root_node_count",
    )?;

    if reader.remaining() != 0 {
        return Err(MapError::TrailingBytes {
            count: reader.remaining(),
        });
    }

    let map = Map {
        header,
        geometry_keys,
        root_nodes,
    };
    validate_map(&map)?;
    Ok(map)
}

/// Encode typed map data into the native payload layout.
pub fn write_map_bytes(map: &Map) -> MapResult<Vec<u8>> {
    validate_map(map)?;

    let mut output = Vec::new();
    write_header(&mut output, &map.header);
    output.extend_from_slice(&(map.geometry_keys.len() as u16).to_le_bytes());
    for key in &map.geometry_keys {
        output.extend_from_slice(&key.to_le_bytes());
    }
    write_node_list(&mut output, &map.root_nodes);
    Ok(output)
}

fn read_header(reader: &mut ByteReader<'_>) -> MapResult<MapHeader> {
    let unknown_00 = reader
        .read_bytes("map.header.unknown_00", MAP_HEADER_UNKNOWN_00_LEN)?
        .to_vec();
    let resource_group = reader.read_u8("map.header.resource_group")?;
    let bounds = read_bounds(reader, "map.header.bounds")?;
    let ground_bounds = read_bounds(reader, "map.header.ground_bounds")?;
    let ground_bounding_sphere = read_sphere(reader, "map.header.ground_bounding_sphere")?;
    let ambient_light = read_header_color(reader, "map.header.ambient_light")?;
    let diffuse_light = read_header_color(reader, "map.header.diffuse_light")?;
    let fog_color = reader.read_u32_le("map.header.fog_color")?;
    let yaw_limits = read_angle_limits(reader, "map.header.yaw_limits")?;
    let pitch_limits = read_angle_limits(reader, "map.header.pitch_limits")?;
    let fog_start = reader.read_u8("map.header.fog_start")?;
    let fog_end = reader.read_u8("map.header.fog_end")?;
    let unknown_79 = reader
        .read_bytes("map.header.unknown_79", MAP_HEADER_UNKNOWN_79_LEN)?
        .to_vec();
    let reset_yaw = read_bool(reader, "map.header.reset_yaw")?;
    let unknown_84 = reader.read_u8("map.header.unknown_84")?;

    Ok(MapHeader {
        unknown_00,
        resource_group,
        bounds,
        ground_bounds,
        ground_bounding_sphere,
        ambient_light,
        diffuse_light,
        fog_color,
        yaw_limits,
        pitch_limits,
        fog_start,
        fog_end,
        unknown_79,
        reset_yaw,
        unknown_84,
    })
}

fn read_node_list(
    reader: &mut ByteReader<'_>,
    geometry_count: usize,
    depth: usize,
    next_node_index: &mut usize,
    count_field: &'static str,
) -> MapResult<Vec<MapNode>> {
    let count = usize::from(reader.read_u16_le(count_field)?);
    if count != 0 && depth > MAP_MAX_NODE_DEPTH {
        return Err(MapError::NodeDepthExceeded {
            depth,
            limit: MAP_MAX_NODE_DEPTH,
        });
    }
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        nodes.push(read_node(reader, geometry_count, depth, next_node_index)?);
    }
    Ok(nodes)
}

fn read_node(
    reader: &mut ByteReader<'_>,
    geometry_count: usize,
    depth: usize,
    next_node_index: &mut usize,
) -> MapResult<MapNode> {
    let node_index = *next_node_index;
    *next_node_index += 1;
    let kind = reader.read_u8("map.node.kind")?;
    let kind = match kind {
        0 => MapNodeKind::Group {
            bounding_sphere: read_sphere(reader, "map.node.bounding_sphere")?,
        },
        1 => MapNodeKind::StaticGeometry {
            geometry: read_geometry_node(reader, geometry_count, node_index)?,
        },
        2 => MapNodeKind::AnimatedGeometry {
            geometry: read_geometry_node(reader, geometry_count, node_index)?,
            uniform_scale: reader.read_f32_le("map.node.uniform_scale")?,
            frame_offset: reader.read_u16_le("map.node.frame_offset")?,
        },
        3 => MapNodeKind::EffectGeometry {
            geometry: read_geometry_node(reader, geometry_count, node_index)?,
            uniform_scale: reader.read_f32_le("map.node.uniform_scale")?,
            frame_offset: reader.read_u16_le("map.node.frame_offset")?,
            color_animation_id: reader.read_i16_le("map.node.color_animation_id")?,
            transform_animation_id: reader.read_i16_le("map.node.transform_animation_id")?,
            texture_animation_id: reader.read_i16_le("map.node.texture_animation_id")?,
            unknown_66: reader.read_u16_le("map.node.unknown_66")?,
            source_blend: reader.read_u16_le("map.node.source_blend")?,
            destination_blend: reader.read_u16_le("map.node.destination_blend")?,
            billboard: read_bool(reader, "map.node.billboard")?,
        },
        kind => return Err(MapError::UnsupportedNodeKind { kind }),
    };
    let children = read_node_list(
        reader,
        geometry_count,
        depth + 1,
        next_node_index,
        "map.node.child_count",
    )?;
    Ok(MapNode { kind, children })
}

fn read_geometry_node(
    reader: &mut ByteReader<'_>,
    geometry_count: usize,
    node: usize,
) -> MapResult<GeometryNode> {
    let geometry_index = reader.read_u16_le("map.node.geometry_index")?;
    if usize::from(geometry_index) >= geometry_count {
        return Err(MapError::InvalidGeometryIndex {
            node,
            geometry_index,
            geometry_count,
        });
    }
    Ok(GeometryNode {
        geometry_index,
        color: read_rgba(reader, "map.node.color")?,
        bounds: read_bounds(reader, "map.node.bounds")?,
        bounding_sphere: read_sphere(reader, "map.node.bounding_sphere")?,
        position: read_vec3(reader, "map.node.position")?,
        rotation: read_rotation(reader, "map.node.rotation")?,
    })
}

fn read_bounds(reader: &mut ByteReader<'_>, field: &'static str) -> MapResult<Bounds3> {
    Ok(Bounds3 {
        minimum: read_vec3(reader, field)?,
        maximum: read_vec3(reader, field)?,
    })
}

fn read_sphere(reader: &mut ByteReader<'_>, field: &'static str) -> MapResult<BoundingSphere> {
    Ok(BoundingSphere {
        center: read_vec3(reader, field)?,
        radius: reader.read_f32_le(field)?,
    })
}

fn read_vec3(reader: &mut ByteReader<'_>, field: &'static str) -> MapResult<[f32; 3]> {
    Ok([
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
    ])
}

fn read_rotation(reader: &mut ByteReader<'_>, field: &'static str) -> MapResult<[i16; 4]> {
    Ok([
        reader.read_i16_le(field)?,
        reader.read_i16_le(field)?,
        reader.read_i16_le(field)?,
        reader.read_i16_le(field)?,
    ])
}

fn read_rgba(reader: &mut ByteReader<'_>, field: &'static str) -> MapResult<Rgba8> {
    let bytes = reader.read_array::<4>(field)?;
    Ok(Rgba8 {
        red: bytes[0],
        green: bytes[1],
        blue: bytes[2],
        alpha: bytes[3],
    })
}

fn read_header_color(reader: &mut ByteReader<'_>, field: &'static str) -> MapResult<Rgba8> {
    let bytes = reader.read_array::<4>(field)?;
    Ok(Rgba8 {
        red: bytes[3],
        green: bytes[0],
        blue: bytes[1],
        alpha: bytes[2],
    })
}

fn read_angle_limits(
    reader: &mut ByteReader<'_>,
    field: &'static str,
) -> MapResult<CameraAngleLimits> {
    Ok(CameraAngleLimits {
        angle_degrees: reader.read_i16_le(field)?,
        minimum_offset_degrees: reader.read_i16_le(field)?,
        maximum_offset_degrees: reader.read_i16_le(field)?,
    })
}

fn read_bool(reader: &mut ByteReader<'_>, field: &'static str) -> MapResult<bool> {
    match reader.read_u8(field)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(MapError::InvalidBoolean { field, value }),
    }
}

fn write_header(output: &mut Vec<u8>, header: &MapHeader) {
    output.extend_from_slice(&header.unknown_00);
    output.push(header.resource_group);
    write_bounds(output, &header.bounds);
    write_bounds(output, &header.ground_bounds);
    write_sphere(output, &header.ground_bounding_sphere);
    write_header_color(output, header.ambient_light);
    write_header_color(output, header.diffuse_light);
    output.extend_from_slice(&header.fog_color.to_le_bytes());
    write_angle_limits(output, header.yaw_limits);
    write_angle_limits(output, header.pitch_limits);
    output.push(header.fog_start);
    output.push(header.fog_end);
    output.extend_from_slice(&header.unknown_79);
    output.push(u8::from(header.reset_yaw));
    output.push(header.unknown_84);
}

fn write_node_list(output: &mut Vec<u8>, nodes: &[MapNode]) {
    output.extend_from_slice(&(nodes.len() as u16).to_le_bytes());
    for node in nodes {
        write_node(output, node);
    }
}

fn write_node(output: &mut Vec<u8>, node: &MapNode) {
    match &node.kind {
        MapNodeKind::Group { bounding_sphere } => {
            output.push(0);
            write_sphere(output, bounding_sphere);
        }
        MapNodeKind::StaticGeometry { geometry } => {
            output.push(1);
            write_geometry_node(output, geometry);
        }
        MapNodeKind::AnimatedGeometry {
            geometry,
            uniform_scale,
            frame_offset,
        } => {
            output.push(2);
            write_geometry_node(output, geometry);
            output.extend_from_slice(&uniform_scale.to_le_bytes());
            output.extend_from_slice(&frame_offset.to_le_bytes());
        }
        MapNodeKind::EffectGeometry {
            geometry,
            uniform_scale,
            frame_offset,
            color_animation_id,
            transform_animation_id,
            texture_animation_id,
            unknown_66,
            billboard,
            source_blend,
            destination_blend,
        } => {
            output.push(3);
            write_geometry_node(output, geometry);
            output.extend_from_slice(&uniform_scale.to_le_bytes());
            output.extend_from_slice(&frame_offset.to_le_bytes());
            output.extend_from_slice(&color_animation_id.to_le_bytes());
            output.extend_from_slice(&transform_animation_id.to_le_bytes());
            output.extend_from_slice(&texture_animation_id.to_le_bytes());
            output.extend_from_slice(&unknown_66.to_le_bytes());
            output.extend_from_slice(&source_blend.to_le_bytes());
            output.extend_from_slice(&destination_blend.to_le_bytes());
            output.push(u8::from(*billboard));
        }
    }
    write_node_list(output, &node.children);
}

fn write_geometry_node(output: &mut Vec<u8>, node: &GeometryNode) {
    output.extend_from_slice(&node.geometry_index.to_le_bytes());
    write_rgba(output, node.color);
    write_bounds(output, &node.bounds);
    write_sphere(output, &node.bounding_sphere);
    write_vec3(output, node.position);
    for value in node.rotation {
        output.extend_from_slice(&value.to_le_bytes());
    }
}

fn write_bounds(output: &mut Vec<u8>, bounds: &Bounds3) {
    write_vec3(output, bounds.minimum);
    write_vec3(output, bounds.maximum);
}

fn write_sphere(output: &mut Vec<u8>, sphere: &BoundingSphere) {
    write_vec3(output, sphere.center);
    output.extend_from_slice(&sphere.radius.to_le_bytes());
}

fn write_vec3(output: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        output.extend_from_slice(&component.to_le_bytes());
    }
}

fn write_rgba(output: &mut Vec<u8>, color: Rgba8) {
    output.extend_from_slice(&[color.red, color.green, color.blue, color.alpha]);
}

fn write_header_color(output: &mut Vec<u8>, color: Rgba8) {
    output.extend_from_slice(&[color.green, color.blue, color.alpha, color.red]);
}

fn write_angle_limits(output: &mut Vec<u8>, limits: CameraAngleLimits) {
    output.extend_from_slice(&limits.angle_degrees.to_le_bytes());
    output.extend_from_slice(&limits.minimum_offset_degrees.to_le_bytes());
    output.extend_from_slice(&limits.maximum_offset_degrees.to_le_bytes());
}

fn validate_map(map: &Map) -> MapResult<()> {
    check_byte_region(
        "header.unknown_00",
        &map.header.unknown_00,
        MAP_HEADER_UNKNOWN_00_LEN,
    )?;
    check_byte_region(
        "header.unknown_79",
        &map.header.unknown_79,
        MAP_HEADER_UNKNOWN_79_LEN,
    )?;
    validate_bounds(&map.header.bounds)?;
    validate_bounds(&map.header.ground_bounds)?;
    validate_sphere(&map.header.ground_bounding_sphere)?;
    check_count(
        "geometry key table",
        map.geometry_keys.len(),
        u16::MAX as usize,
    )?;
    let mut next_node_index = 0;
    validate_node_list(
        &map.root_nodes,
        map.geometry_keys.len(),
        1,
        &mut next_node_index,
    )
}

fn validate_node_list(
    nodes: &[MapNode],
    geometry_count: usize,
    depth: usize,
    next_node_index: &mut usize,
) -> MapResult<()> {
    check_count("node list", nodes.len(), u16::MAX as usize)?;
    if !nodes.is_empty() && depth > MAP_MAX_NODE_DEPTH {
        return Err(MapError::NodeDepthExceeded {
            depth,
            limit: MAP_MAX_NODE_DEPTH,
        });
    }
    for node in nodes {
        let node_index = *next_node_index;
        *next_node_index += 1;
        match &node.kind {
            MapNodeKind::Group { bounding_sphere } => validate_sphere(bounding_sphere)?,
            MapNodeKind::StaticGeometry { geometry } => {
                validate_geometry_node(geometry, geometry_count, node_index)?;
            }
            MapNodeKind::AnimatedGeometry {
                geometry,
                uniform_scale,
                ..
            }
            | MapNodeKind::EffectGeometry {
                geometry,
                uniform_scale,
                ..
            } => {
                validate_geometry_node(geometry, geometry_count, node_index)?;
                validate_float("node.uniform_scale", *uniform_scale)?;
            }
        }
        validate_node_list(&node.children, geometry_count, depth + 1, next_node_index)?;
    }
    Ok(())
}

fn validate_geometry_node(
    geometry: &GeometryNode,
    geometry_count: usize,
    node: usize,
) -> MapResult<()> {
    if usize::from(geometry.geometry_index) >= geometry_count {
        return Err(MapError::InvalidGeometryIndex {
            node,
            geometry_index: geometry.geometry_index,
            geometry_count,
        });
    }
    validate_bounds(&geometry.bounds)?;
    validate_sphere(&geometry.bounding_sphere)?;
    validate_vec3("node.position", geometry.position)
}

fn validate_bounds(bounds: &Bounds3) -> MapResult<()> {
    validate_vec3("bounds.minimum", bounds.minimum)?;
    validate_vec3("bounds.maximum", bounds.maximum)?;
    for axis in 0..3 {
        if bounds.minimum[axis] > bounds.maximum[axis] {
            return Err(MapError::ReversedBounds {
                axis,
                minimum: bounds.minimum[axis],
                maximum: bounds.maximum[axis],
            });
        }
    }
    Ok(())
}

fn validate_sphere(sphere: &BoundingSphere) -> MapResult<()> {
    validate_vec3("bounding_sphere.center", sphere.center)?;
    validate_float("bounding_sphere.radius", sphere.radius)?;
    if sphere.radius < 0.0 {
        return Err(MapError::NegativeSphereRadius {
            radius: sphere.radius,
        });
    }
    Ok(())
}

fn validate_vec3(field: &'static str, value: [f32; 3]) -> MapResult<()> {
    for component in value {
        validate_float(field, component)?;
    }
    Ok(())
}

fn validate_float(field: &'static str, value: f32) -> MapResult<()> {
    if !value.is_finite() {
        return Err(MapError::NonFiniteFloat { field });
    }
    Ok(())
}

fn check_byte_region(field: &'static str, bytes: &[u8], expected: usize) -> MapResult<()> {
    if bytes.len() != expected {
        return Err(MapError::InvalidByteRegionLength {
            field,
            expected,
            actual: bytes.len(),
        });
    }
    Ok(())
}

fn check_count(field: &'static str, count: usize, maximum: usize) -> MapResult<()> {
    if count > maximum {
        return Err(MapError::CountOverflow {
            field,
            count,
            maximum,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Bounds3 {
        Bounds3 {
            minimum: [-1.0, -2.0, -3.0],
            maximum: [4.0, 5.0, 6.0],
        }
    }

    fn sphere(radius: f32) -> BoundingSphere {
        BoundingSphere {
            center: [1.0, 2.0, 3.0],
            radius,
        }
    }

    fn geometry(index: u16) -> GeometryNode {
        GeometryNode {
            geometry_index: index,
            color: Rgba8 {
                red: 10,
                green: 20,
                blue: 30,
                alpha: 40,
            },
            bounds: bounds(),
            bounding_sphere: sphere(8.0),
            position: [7.0, 8.0, 9.0],
            rotation: [1, 2, 3, 32767],
        }
    }

    fn fixture() -> Map {
        Map {
            header: MapHeader {
                unknown_00: (0..MAP_HEADER_UNKNOWN_00_LEN as u8).collect(),
                resource_group: 7,
                bounds: bounds(),
                ground_bounds: Bounds3 {
                    minimum: [-10.0, -20.0, -30.0],
                    maximum: [40.0, 50.0, 60.0],
                },
                ground_bounding_sphere: BoundingSphere {
                    center: [10.0, 20.0, 30.0],
                    radius: 25.0,
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
                fog_color: 0xaabb_ccdd,
                yaw_limits: CameraAngleLimits {
                    angle_degrees: 90,
                    minimum_offset_degrees: 30,
                    maximum_offset_degrees: 45,
                },
                pitch_limits: CameraAngleLimits {
                    angle_degrees: 45,
                    minimum_offset_degrees: 15,
                    maximum_offset_degrees: 10,
                },
                fog_start: 50,
                fog_end: 200,
                unknown_79: vec![0x5a; MAP_HEADER_UNKNOWN_79_LEN],
                reset_yaw: true,
                unknown_84: 0xee,
            },
            geometry_keys: vec![100, 200, 300],
            root_nodes: vec![MapNode {
                kind: MapNodeKind::Group {
                    bounding_sphere: sphere(100.0),
                },
                children: vec![
                    MapNode {
                        kind: MapNodeKind::StaticGeometry {
                            geometry: geometry(0),
                        },
                        children: Vec::new(),
                    },
                    MapNode {
                        kind: MapNodeKind::AnimatedGeometry {
                            geometry: geometry(1),
                            uniform_scale: 1.5,
                            frame_offset: 12,
                        },
                        children: Vec::new(),
                    },
                    MapNode {
                        kind: MapNodeKind::EffectGeometry {
                            geometry: geometry(2),
                            uniform_scale: 2.5,
                            frame_offset: 34,
                            color_animation_id: -1,
                            transform_animation_id: 4,
                            texture_animation_id: 5,
                            unknown_66: 6,
                            billboard: true,
                            source_blend: 7,
                            destination_blend: 8,
                        },
                        children: Vec::new(),
                    },
                ],
            }],
        }
    }

    #[test]
    fn round_trips_every_node_variant() {
        let expected = fixture();
        let bytes = write_map_bytes(&expected).unwrap();
        assert_eq!(bytes.len(), 400);
        assert_eq!(decode_map(&bytes).unwrap(), expected);
    }

    #[test]
    fn writes_header_channel_order_and_native_effect_order() {
        let map = fixture();
        let bytes = write_map_bytes(&map).unwrap();
        assert_eq!(&bytes[0x5f..0x63], &[2, 3, 4, 1]);
        assert_eq!(&bytes[0x63..0x67], &[6, 7, 8, 5]);

        let effect_start = 312;
        let tail = &bytes[effect_start + 1 + 66..effect_start + 1 + 66 + 19];
        assert_eq!(&tail[0..4], &2.5_f32.to_le_bytes());
        assert_eq!(&tail[4..6], &34_u16.to_le_bytes());
        assert_eq!(tail[18], 1);
    }

    #[test]
    fn decodes_ground_fields_and_uniform_scale_at_native_offsets() {
        let map = fixture();
        let bytes = write_map_bytes(&map).unwrap();

        assert_eq!(&bytes[0x37..0x3b], &(-10.0_f32).to_le_bytes());
        assert_eq!(&bytes[0x4f..0x53], &10.0_f32.to_le_bytes());
        assert_eq!(&bytes[0x5b..0x5f], &25.0_f32.to_le_bytes());

        let decoded = decode_map(&bytes).unwrap();
        assert_eq!(decoded.header.ground_bounds, map.header.ground_bounds);
        assert_eq!(
            decoded.header.ground_bounding_sphere,
            map.header.ground_bounding_sphere
        );
        let MapNodeKind::AnimatedGeometry { uniform_scale, .. } =
            decoded.root_nodes[0].children[1].kind
        else {
            panic!("fixture node kind changed");
        };
        assert_eq!(uniform_scale, 1.5);
    }

    #[test]
    fn reports_structural_counts_and_references() {
        let map = fixture();
        assert_eq!(map.node_count(), 4);
        assert_eq!(map.geometry_node_count(), 3);
        assert_eq!(map.effect_node_count(), 1);
        assert_eq!(map.referenced_geometry_keys(), vec![100, 200, 300]);
    }

    #[test]
    fn rejects_unknown_node_kinds_and_trailing_bytes() {
        let mut bytes = write_map_bytes(&fixture()).unwrap();
        let root_kind_offset = MAP_HEADER_LEN + 2 + 3 * 4 + 2;
        bytes[root_kind_offset] = 4;
        assert!(matches!(
            decode_map(&bytes),
            Err(MapError::UnsupportedNodeKind { kind: 4 })
        ));

        let mut bytes = write_map_bytes(&fixture()).unwrap();
        bytes.push(0);
        assert!(matches!(
            decode_map(&bytes),
            Err(MapError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn rejects_every_truncated_prefix_and_invalid_booleans() {
        let bytes = write_map_bytes(&fixture()).unwrap();
        for end in 0..bytes.len() {
            assert!(decode_map(&bytes[..end]).is_err(), "accepted {end} bytes");
        }

        let mut bytes = bytes;
        bytes[0x83] = 2;
        assert!(matches!(
            decode_map(&bytes),
            Err(MapError::InvalidBoolean {
                field: "map.header.reset_yaw",
                value: 2
            })
        ));
    }

    #[test]
    fn validates_geometry_indices_and_unknown_region_lengths() {
        let mut map = fixture();
        let MapNodeKind::StaticGeometry { geometry } = &mut map.root_nodes[0].children[0].kind
        else {
            panic!("fixture node kind changed");
        };
        geometry.geometry_index = 3;
        assert!(matches!(
            write_map_bytes(&map),
            Err(MapError::InvalidGeometryIndex { .. })
        ));

        let mut map = fixture();
        map.header.unknown_79.pop();
        assert!(matches!(
            write_map_bytes(&map),
            Err(MapError::InvalidByteRegionLength { .. })
        ));
    }

    #[test]
    fn rejects_reversed_bounds_negative_radii_and_non_finite_values() {
        let mut map = fixture();
        map.header.bounds.minimum[0] = 10.0;
        assert!(matches!(
            write_map_bytes(&map),
            Err(MapError::ReversedBounds { axis: 0, .. })
        ));

        let mut map = fixture();
        let MapNodeKind::Group { bounding_sphere } = &mut map.root_nodes[0].kind else {
            panic!("fixture node kind changed");
        };
        bounding_sphere.radius = -1.0;
        assert!(matches!(
            write_map_bytes(&map),
            Err(MapError::NegativeSphereRadius { .. })
        ));

        let mut map = fixture();
        let MapNodeKind::AnimatedGeometry { uniform_scale, .. } =
            &mut map.root_nodes[0].children[1].kind
        else {
            panic!("fixture node kind changed");
        };
        *uniform_scale = f32::NAN;
        assert!(matches!(
            write_map_bytes(&map),
            Err(MapError::NonFiniteFloat { .. })
        ));
    }

    #[test]
    fn rejects_excessive_node_depth() {
        let mut map = fixture();
        map.root_nodes.clear();
        let mut node = MapNode {
            kind: MapNodeKind::Group {
                bounding_sphere: sphere(1.0),
            },
            children: Vec::new(),
        };
        for _ in 0..MAP_MAX_NODE_DEPTH {
            node = MapNode {
                kind: MapNodeKind::Group {
                    bounding_sphere: sphere(1.0),
                },
                children: vec![node],
            };
        }
        map.root_nodes.push(node);
        assert!(matches!(
            write_map_bytes(&map),
            Err(MapError::NodeDepthExceeded { .. })
        ));
    }
}
