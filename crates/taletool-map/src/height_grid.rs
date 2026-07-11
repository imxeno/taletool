//! Typed support for map height-grid payloads stored in `NSgrdData` archives.
//!
//! Height grids partition a map's collision triangles into X/Z cells. Three
//! compatible encodings exist: an untagged layout and two explicitly tagged
//! layouts. The second tagged layout widens triangle and cell indices from 16
//! to 32 bits.

use serde::{Deserialize, Serialize};
use taletool_core::{ByteReadError, ByteReader};
use thiserror::Error;

/// Explicit version tag for the 16-bit index layout.
pub const HEIGHT_GRID_VERSION_1: u32 = 0x0BF8_2311;
/// Explicit version tag for the 32-bit index layout.
pub const HEIGHT_GRID_VERSION_2: u32 = 0x0BF8_2312;

#[derive(Debug, Error)]
pub enum HeightGridError {
    #[error(transparent)]
    Truncated(#[from] ByteReadError),
    #[error("height-grid payload declares {declared} bytes but contains {actual}")]
    SizeMismatch { declared: u64, actual: usize },
    #[error("height-grid payload has {count} trailing bytes")]
    TrailingBytes { count: usize },
    #[error("height-grid field {field} contains a non-finite floating-point value")]
    NonFiniteFloat { field: &'static str },
    #[error(
        "height-grid bounds are reversed on axis {axis}: minimum {minimum} exceeds maximum {maximum}"
    )]
    ReversedBounds {
        axis: usize,
        minimum: f32,
        maximum: f32,
    },
    #[error("height-grid cell size on axis {axis} must be positive, got {value}")]
    InvalidCellSize { axis: usize, value: f32 },
    #[error("height-grid dimensions must be non-zero, got {width}x{depth}")]
    InvalidDimensions { width: u16, depth: u16 },
    #[error(
        "height-grid dimensions {width}x{depth} require {expected} cells, but the payload contains {actual}"
    )]
    CellCountMismatch {
        width: u16,
        depth: u16,
        expected: usize,
        actual: usize,
    },
    #[error("height-grid {field} has {count} items; maximum is {maximum}")]
    CountOverflow {
        field: &'static str,
        count: usize,
        maximum: usize,
    },
    #[error("height-grid {field} byte count overflows usize")]
    ByteCountOverflow { field: &'static str },
    #[error("height-grid {field} declares {count} items but only {remaining} payload bytes remain")]
    ImpossibleCount {
        field: &'static str,
        count: usize,
        remaining: usize,
    },
    #[error("height-grid {kind} index {index} is negative: {value}")]
    NegativeIndex {
        kind: &'static str,
        index: usize,
        value: i32,
    },
    #[error(
        "height-grid triangle {triangle} corner {corner} references vertex {vertex_index}, but only {vertex_count} vertices exist"
    )]
    InvalidVertexIndex {
        triangle: usize,
        corner: usize,
        vertex_index: u32,
        vertex_count: usize,
    },
    #[error(
        "height-grid cell {cell} reference {reference} names triangle {triangle_index}, but only {triangle_count} triangles exist"
    )]
    InvalidTriangleIndex {
        cell: usize,
        reference: usize,
        triangle_index: u32,
        triangle_count: usize,
    },
    #[error(
        "height-grid {field} index {index} value {value} exceeds the {maximum}-value encoding limit"
    )]
    IndexOverflow {
        field: &'static str,
        index: usize,
        value: u32,
        maximum: u32,
    },
    #[error("height-grid payload is too large to encode: {size} bytes")]
    PayloadTooLarge { size: usize },
}

pub type HeightGridResult<T> = std::result::Result<T, HeightGridError>;

/// Native index representation selected by the payload preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeightGridEncoding {
    /// Omits a version tag and uses the version-1, 16-bit index layout.
    #[serde(rename = "implicit_version_1")]
    ImplicitVersion1,
    /// Stores [`HEIGHT_GRID_VERSION_1`] and uses 16-bit indices.
    #[serde(rename = "version_1")]
    Version1,
    /// Stores [`HEIGHT_GRID_VERSION_2`] and uses signed 32-bit indices.
    #[serde(rename = "version_2")]
    Version2,
}

impl HeightGridEncoding {
    fn version_tag(self) -> Option<u32> {
        match self {
            Self::ImplicitVersion1 => None,
            Self::Version1 => Some(HEIGHT_GRID_VERSION_1),
            Self::Version2 => Some(HEIGHT_GRID_VERSION_2),
        }
    }

    fn index_width(self) -> usize {
        match self {
            Self::ImplicitVersion1 | Self::Version1 => 2,
            Self::Version2 => 4,
        }
    }

    fn maximum_index(self) -> u32 {
        match self {
            Self::ImplicitVersion1 | Self::Version1 => u32::from(u16::MAX),
            Self::Version2 => i32::MAX as u32,
        }
    }
}

/// World-space bounds covered by the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightGridBounds {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
}

/// X/Z cell dimensions stored in the fixed header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightGridDimensions {
    pub width: u16,
    pub depth: u16,
}

/// Fully decoded map height grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeightGrid {
    pub encoding: HeightGridEncoding,
    pub grid_id: i32,
    pub map_id: i32,
    pub bounds: HeightGridBounds,
    pub dimensions: HeightGridDimensions,
    pub cell_size: [f32; 3],
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub cells: Vec<Vec<u32>>,
}

impl HeightGrid {
    /// Return one cell's triangle references using X/Z row-major addressing.
    pub fn cell(&self, x: u16, z: u16) -> Option<&[u32]> {
        if x >= self.dimensions.width || z >= self.dimensions.depth {
            return None;
        }
        let index = usize::from(z) * usize::from(self.dimensions.width) + usize::from(x);
        self.cells.get(index).map(Vec::as_slice)
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn non_empty_cell_count(&self) -> usize {
        self.cells.iter().filter(|cell| !cell.is_empty()).count()
    }

    pub fn triangle_reference_count(&self) -> usize {
        self.cells.iter().map(Vec::len).sum()
    }
}

/// Decode one `NSgrdData` archive payload.
pub fn decode_height_grid(data: &[u8]) -> HeightGridResult<HeightGrid> {
    let mut reader = ByteReader::new(data);
    let grid_id = reader.read_i32_le("height_grid.grid_id")?;
    let marker = reader.read_u32_le("height_grid.version_or_map_id")?;
    let (encoding, map_id) = match marker {
        HEIGHT_GRID_VERSION_1 => (
            HeightGridEncoding::Version1,
            reader.read_i32_le("height_grid.map_id")?,
        ),
        HEIGHT_GRID_VERSION_2 => (
            HeightGridEncoding::Version2,
            reader.read_i32_le("height_grid.map_id")?,
        ),
        value => (HeightGridEncoding::ImplicitVersion1, value as i32),
    };

    let declared_size = reader.read_u64_le("height_grid.declared_size")?;
    if declared_size != data.len() as u64 {
        return Err(HeightGridError::SizeMismatch {
            declared: declared_size,
            actual: data.len(),
        });
    }

    let bounds = HeightGridBounds {
        minimum: read_vec3(&mut reader, "height_grid.bounds.minimum")?,
        maximum: read_vec3(&mut reader, "height_grid.bounds.maximum")?,
    };
    let dimensions = HeightGridDimensions {
        width: reader.read_u16_le("height_grid.dimensions.width")?,
        depth: reader.read_u16_le("height_grid.dimensions.depth")?,
    };
    let stored_cell_count = count_from_u32(reader.read_u32_le("height_grid.cell_count")?);
    let cell_size = read_vec3(&mut reader, "height_grid.cell_size")?;
    let vertex_count = count_from_u32(reader.read_u32_le("height_grid.vertex_count")?);
    let triangle_count = count_from_u32(reader.read_u32_le("height_grid.triangle_count")?);

    validate_header(&bounds, dimensions, cell_size, stored_cell_count)?;
    ensure_fixed_items(&reader, "vertices", vertex_count, 12)?;
    let mut vertices = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        vertices.push(read_vec3(&mut reader, "height_grid.vertex")?);
    }

    let triangle_bytes = encoding
        .index_width()
        .checked_mul(3)
        .ok_or(HeightGridError::ByteCountOverflow { field: "triangles" })?;
    ensure_fixed_items(&reader, "triangles", triangle_count, triangle_bytes)?;
    let mut triangles = Vec::with_capacity(triangle_count);
    for triangle in 0..triangle_count {
        let mut indices = [0_u32; 3];
        for (corner, value) in indices.iter_mut().enumerate() {
            *value = read_index(
                &mut reader,
                encoding,
                "triangle vertex",
                triangle * 3 + corner,
            )?;
        }
        triangles.push(indices);
    }

    ensure_fixed_items(&reader, "cells", stored_cell_count, 2)?;
    let mut cells = Vec::with_capacity(stored_cell_count);
    for cell in 0..stored_cell_count {
        let reference_count = usize::from(reader.read_u16_le("height_grid.cell.reference_count")?);
        ensure_fixed_items(
            &reader,
            "cell references",
            reference_count,
            encoding.index_width(),
        )?;
        let mut references = Vec::with_capacity(reference_count);
        for reference in 0..reference_count {
            references.push(read_index(
                &mut reader,
                encoding,
                "cell triangle",
                reference,
            )?);
        }
        debug_assert_eq!(cells.len(), cell);
        cells.push(references);
    }

    if reader.remaining() != 0 {
        return Err(HeightGridError::TrailingBytes {
            count: reader.remaining(),
        });
    }

    let grid = HeightGrid {
        encoding,
        grid_id,
        map_id,
        bounds,
        dimensions,
        cell_size,
        vertices,
        triangles,
        cells,
    };
    validate_height_grid(&grid)?;
    Ok(grid)
}

/// Encode typed height-grid data into the native payload layout.
pub fn write_height_grid_bytes(grid: &HeightGrid) -> HeightGridResult<Vec<u8>> {
    validate_height_grid(grid)?;

    let mut output = Vec::new();
    output.extend_from_slice(&grid.grid_id.to_le_bytes());
    if let Some(version) = grid.encoding.version_tag() {
        output.extend_from_slice(&version.to_le_bytes());
    }
    output.extend_from_slice(&grid.map_id.to_le_bytes());
    let size_offset = output.len();
    output.extend_from_slice(&0_u64.to_le_bytes());
    write_vec3(&mut output, grid.bounds.minimum);
    write_vec3(&mut output, grid.bounds.maximum);
    output.extend_from_slice(&grid.dimensions.width.to_le_bytes());
    output.extend_from_slice(&grid.dimensions.depth.to_le_bytes());
    output.extend_from_slice(&(grid.cells.len() as u32).to_le_bytes());
    write_vec3(&mut output, grid.cell_size);
    output.extend_from_slice(&(grid.vertices.len() as u32).to_le_bytes());
    output.extend_from_slice(&(grid.triangles.len() as u32).to_le_bytes());

    for vertex in &grid.vertices {
        write_vec3(&mut output, *vertex);
    }
    for triangle in &grid.triangles {
        for index in triangle {
            write_index(&mut output, grid.encoding, *index);
        }
    }
    for cell in &grid.cells {
        output.extend_from_slice(&(cell.len() as u16).to_le_bytes());
        for index in cell {
            write_index(&mut output, grid.encoding, *index);
        }
    }

    let size = u64::try_from(output.len())
        .map_err(|_| HeightGridError::PayloadTooLarge { size: output.len() })?;
    output[size_offset..size_offset + 8].copy_from_slice(&size.to_le_bytes());
    Ok(output)
}

fn validate_height_grid(grid: &HeightGrid) -> HeightGridResult<()> {
    validate_header(
        &grid.bounds,
        grid.dimensions,
        grid.cell_size,
        grid.cells.len(),
    )?;
    validate_count("vertices", grid.vertices.len(), u32::MAX as usize)?;
    validate_count("triangles", grid.triangles.len(), u32::MAX as usize)?;
    validate_count("cells", grid.cells.len(), u32::MAX as usize)?;
    for vertex in &grid.vertices {
        validate_vec3("height_grid.vertex", *vertex)?;
    }

    let maximum_index = grid.encoding.maximum_index();
    for (triangle, indices) in grid.triangles.iter().enumerate() {
        for (corner, &vertex_index) in indices.iter().enumerate() {
            validate_index_width(
                "triangle vertex",
                triangle * 3 + corner,
                vertex_index,
                maximum_index,
            )?;
            if vertex_index as usize >= grid.vertices.len() {
                return Err(HeightGridError::InvalidVertexIndex {
                    triangle,
                    corner,
                    vertex_index,
                    vertex_count: grid.vertices.len(),
                });
            }
        }
    }

    for (cell, references) in grid.cells.iter().enumerate() {
        validate_count("cell references", references.len(), u16::MAX as usize)?;
        for (reference, &triangle_index) in references.iter().enumerate() {
            validate_index_width("cell triangle", reference, triangle_index, maximum_index)?;
            if triangle_index as usize >= grid.triangles.len() {
                return Err(HeightGridError::InvalidTriangleIndex {
                    cell,
                    reference,
                    triangle_index,
                    triangle_count: grid.triangles.len(),
                });
            }
        }
    }
    Ok(())
}

fn validate_header(
    bounds: &HeightGridBounds,
    dimensions: HeightGridDimensions,
    cell_size: [f32; 3],
    cell_count: usize,
) -> HeightGridResult<()> {
    validate_vec3("height_grid.bounds.minimum", bounds.minimum)?;
    validate_vec3("height_grid.bounds.maximum", bounds.maximum)?;
    for axis in 0..3 {
        if bounds.minimum[axis] > bounds.maximum[axis] {
            return Err(HeightGridError::ReversedBounds {
                axis,
                minimum: bounds.minimum[axis],
                maximum: bounds.maximum[axis],
            });
        }
    }
    validate_vec3("height_grid.cell_size", cell_size)?;
    for (axis, value) in cell_size.into_iter().enumerate() {
        if value <= 0.0 {
            return Err(HeightGridError::InvalidCellSize { axis, value });
        }
    }
    if dimensions.width == 0 || dimensions.depth == 0 {
        return Err(HeightGridError::InvalidDimensions {
            width: dimensions.width,
            depth: dimensions.depth,
        });
    }
    let expected = usize::from(dimensions.width) * usize::from(dimensions.depth);
    if expected != cell_count {
        return Err(HeightGridError::CellCountMismatch {
            width: dimensions.width,
            depth: dimensions.depth,
            expected,
            actual: cell_count,
        });
    }
    Ok(())
}

fn validate_vec3(field: &'static str, value: [f32; 3]) -> HeightGridResult<()> {
    if value.into_iter().any(|component| !component.is_finite()) {
        return Err(HeightGridError::NonFiniteFloat { field });
    }
    Ok(())
}

fn validate_count(field: &'static str, count: usize, maximum: usize) -> HeightGridResult<()> {
    if count > maximum {
        return Err(HeightGridError::CountOverflow {
            field,
            count,
            maximum,
        });
    }
    Ok(())
}

fn validate_index_width(
    field: &'static str,
    index: usize,
    value: u32,
    maximum: u32,
) -> HeightGridResult<()> {
    if value > maximum {
        return Err(HeightGridError::IndexOverflow {
            field,
            index,
            value,
            maximum,
        });
    }
    Ok(())
}

fn ensure_fixed_items(
    reader: &ByteReader<'_>,
    field: &'static str,
    count: usize,
    bytes_per_item: usize,
) -> HeightGridResult<()> {
    let needed = count
        .checked_mul(bytes_per_item)
        .ok_or(HeightGridError::ByteCountOverflow { field })?;
    if needed > reader.remaining() {
        return Err(HeightGridError::ImpossibleCount {
            field,
            count,
            remaining: reader.remaining(),
        });
    }
    Ok(())
}

fn count_from_u32(value: u32) -> usize {
    value as usize
}

fn read_vec3(reader: &mut ByteReader<'_>, field: &'static str) -> HeightGridResult<[f32; 3]> {
    Ok([
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
        reader.read_f32_le(field)?,
    ])
}

fn read_index(
    reader: &mut ByteReader<'_>,
    encoding: HeightGridEncoding,
    kind: &'static str,
    index: usize,
) -> HeightGridResult<u32> {
    match encoding {
        HeightGridEncoding::ImplicitVersion1 | HeightGridEncoding::Version1 => {
            Ok(u32::from(reader.read_u16_le("height_grid.index")?))
        }
        HeightGridEncoding::Version2 => {
            let value = reader.read_i32_le("height_grid.index")?;
            u32::try_from(value).map_err(|_| HeightGridError::NegativeIndex { kind, index, value })
        }
    }
}

fn write_vec3(output: &mut Vec<u8>, value: [f32; 3]) {
    for component in value {
        output.extend_from_slice(&component.to_le_bytes());
    }
}

fn write_index(output: &mut Vec<u8>, encoding: HeightGridEncoding, value: u32) {
    match encoding {
        HeightGridEncoding::ImplicitVersion1 | HeightGridEncoding::Version1 => {
            output.extend_from_slice(&(value as u16).to_le_bytes());
        }
        HeightGridEncoding::Version2 => {
            output.extend_from_slice(&(value as i32).to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(encoding: HeightGridEncoding) -> HeightGrid {
        HeightGrid {
            encoding,
            grid_id: 2006,
            map_id: 2006,
            bounds: HeightGridBounds {
                minimum: [-1.0, -2.0, -3.0],
                maximum: [4.0, 5.0, 6.0],
            },
            dimensions: HeightGridDimensions { width: 2, depth: 1 },
            cell_size: [0.5, 0.5, 0.5],
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
            ],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
            cells: vec![Vec::new(), vec![0, 1]],
        }
    }

    fn fixture_bytes(encoding: HeightGridEncoding) -> Vec<u8> {
        let grid = sample(encoding);
        let mut output = Vec::new();
        output.extend_from_slice(&2006_i32.to_le_bytes());
        match encoding {
            HeightGridEncoding::ImplicitVersion1 => {}
            HeightGridEncoding::Version1 => {
                output.extend_from_slice(&HEIGHT_GRID_VERSION_1.to_le_bytes());
            }
            HeightGridEncoding::Version2 => {
                output.extend_from_slice(&HEIGHT_GRID_VERSION_2.to_le_bytes());
            }
        }
        output.extend_from_slice(&2006_i32.to_le_bytes());
        let size_offset = output.len();
        output.extend_from_slice(&0_u64.to_le_bytes());
        for value in grid.bounds.minimum {
            output.extend_from_slice(&value.to_le_bytes());
        }
        for value in grid.bounds.maximum {
            output.extend_from_slice(&value.to_le_bytes());
        }
        output.extend_from_slice(&2_u16.to_le_bytes());
        output.extend_from_slice(&1_u16.to_le_bytes());
        output.extend_from_slice(&2_u32.to_le_bytes());
        for value in grid.cell_size {
            output.extend_from_slice(&value.to_le_bytes());
        }
        output.extend_from_slice(&4_u32.to_le_bytes());
        output.extend_from_slice(&2_u32.to_le_bytes());
        for vertex in &grid.vertices {
            for value in vertex {
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
        let write_fixture_index = |output: &mut Vec<u8>, value: u32| match encoding {
            HeightGridEncoding::ImplicitVersion1 | HeightGridEncoding::Version1 => {
                output.extend_from_slice(&(value as u16).to_le_bytes());
            }
            HeightGridEncoding::Version2 => {
                output.extend_from_slice(&(value as i32).to_le_bytes());
            }
        };
        for value in [0, 1, 2, 1, 3, 2] {
            write_fixture_index(&mut output, value);
        }
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(&2_u16.to_le_bytes());
        write_fixture_index(&mut output, 0);
        write_fixture_index(&mut output, 1);
        let size = output.len() as u64;
        output[size_offset..size_offset + 8].copy_from_slice(&size.to_le_bytes());
        output
    }

    #[test]
    fn all_encodings_round_trip_byte_for_byte() {
        for encoding in [
            HeightGridEncoding::ImplicitVersion1,
            HeightGridEncoding::Version1,
            HeightGridEncoding::Version2,
        ] {
            let bytes = fixture_bytes(encoding);
            let decoded = decode_height_grid(&bytes).unwrap();
            assert_eq!(decoded, sample(encoding));
            assert_eq!(write_height_grid_bytes(&decoded).unwrap(), bytes);
        }
    }

    #[test]
    fn provides_row_major_cell_helpers_and_counts() {
        let grid = sample(HeightGridEncoding::ImplicitVersion1);
        assert_eq!(grid.cell(0, 0), Some([].as_slice()));
        assert_eq!(grid.cell(1, 0), Some([0, 1].as_slice()));
        assert_eq!(grid.cell(0, 1), None);
        assert_eq!(grid.vertex_count(), 4);
        assert_eq!(grid.triangle_count(), 2);
        assert_eq!(grid.cell_count(), 2);
        assert_eq!(grid.non_empty_cell_count(), 1);
        assert_eq!(grid.triangle_reference_count(), 2);
    }

    #[test]
    fn writer_recomputes_declared_size() {
        let bytes = write_height_grid_bytes(&sample(HeightGridEncoding::Version1)).unwrap();
        assert_eq!(
            u64::from_le_bytes(bytes[12..20].try_into().unwrap()),
            bytes.len() as u64
        );
    }

    #[test]
    fn accepts_maximum_version1_index() {
        let mut grid = sample(HeightGridEncoding::Version1);
        grid.vertices.resize(usize::from(u16::MAX) + 1, [0.0; 3]);
        grid.triangles[0][0] = u32::from(u16::MAX);
        let bytes = write_height_grid_bytes(&grid).unwrap();
        assert_eq!(decode_height_grid(&bytes).unwrap(), grid);
    }

    #[test]
    fn rejects_size_mismatches_and_trailing_data() {
        let mut bytes = fixture_bytes(HeightGridEncoding::ImplicitVersion1);
        bytes[8..16].copy_from_slice(&1_u64.to_le_bytes());
        assert!(matches!(
            decode_height_grid(&bytes),
            Err(HeightGridError::SizeMismatch { .. })
        ));

        let mut bytes = fixture_bytes(HeightGridEncoding::ImplicitVersion1);
        bytes.push(0);
        let size = bytes.len() as u64;
        bytes[8..16].copy_from_slice(&size.to_le_bytes());
        assert!(matches!(
            decode_height_grid(&bytes),
            Err(HeightGridError::TrailingBytes { count: 1 })
        ));
    }

    #[test]
    fn rejects_invalid_header_and_cell_shape() {
        let mut grid = sample(HeightGridEncoding::ImplicitVersion1);
        grid.bounds.minimum[0] = f32::NAN;
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::NonFiniteFloat { .. })
        ));

        let mut grid = sample(HeightGridEncoding::ImplicitVersion1);
        grid.bounds.minimum[0] = 10.0;
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::ReversedBounds { .. })
        ));

        let mut grid = sample(HeightGridEncoding::ImplicitVersion1);
        grid.cell_size[0] = 0.0;
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::InvalidCellSize { .. })
        ));

        let mut grid = sample(HeightGridEncoding::ImplicitVersion1);
        grid.cells.pop();
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::CellCountMismatch { .. })
        ));
    }

    #[test]
    fn rejects_invalid_and_unrepresentable_indices() {
        let mut grid = sample(HeightGridEncoding::Version2);
        grid.triangles[0][0] = i32::MAX as u32 + 1;
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::IndexOverflow { .. })
        ));

        let mut grid = sample(HeightGridEncoding::ImplicitVersion1);
        grid.triangles[0][0] = 99;
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::InvalidVertexIndex { .. })
        ));

        let mut grid = sample(HeightGridEncoding::ImplicitVersion1);
        grid.cells[1][0] = 99;
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::InvalidTriangleIndex { .. })
        ));

        let mut grid = sample(HeightGridEncoding::ImplicitVersion1);
        grid.cells[1].resize(usize::from(u16::MAX) + 1, 0);
        assert!(matches!(
            write_height_grid_bytes(&grid),
            Err(HeightGridError::CountOverflow {
                field: "cell references",
                ..
            })
        ));

        let mut bytes = fixture_bytes(HeightGridEncoding::Version2);
        let triangle_offset = 72 + 4 * 12;
        bytes[triangle_offset..triangle_offset + 4].copy_from_slice(&(-1_i32).to_le_bytes());
        assert!(matches!(
            decode_height_grid(&bytes),
            Err(HeightGridError::NegativeIndex { .. })
        ));
    }

    #[test]
    fn rejects_truncated_and_impossible_counts_without_allocating() {
        let bytes = fixture_bytes(HeightGridEncoding::ImplicitVersion1);
        assert!(decode_height_grid(&bytes[..20]).is_err());

        let mut bytes = fixture_bytes(HeightGridEncoding::ImplicitVersion1);
        bytes[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decode_height_grid(&bytes),
            Err(HeightGridError::ImpossibleCount {
                field: "vertices",
                ..
            })
        ));
    }
}
