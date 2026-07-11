//! Cell-flag grids stored in `NStcData` archive entries.

use bitflags::bitflags;
use serde::{Deserialize, Deserializer, Serialize};
use taletool_core::ByteReader;
use thiserror::Error;

bitflags! {
    /// Flags attached to one map cell in an `NStcData` payload.
    ///
    /// Only bits observed in current client data have named constants. Unnamed
    /// bits are retained when payloads are decoded and encoded.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct MapCellFlags: u8 {
        /// Players, NPCs, and monsters cannot walk through this cell.
        const WALKING_DISABLED = 0x01;
        /// Attacks cannot pass through this cell.
        const ATTACK_THROUGH_DISABLED = 0x02;
        /// Observed cell property whose runtime meaning is not yet known.
        const UNKNOWN_04 = 0x04;
        /// Monster pathfinding treats this cell as blocked when using its
        /// observed aggro-movement mask.
        const MONSTER_AGGRO_DISABLED = 0x08;
        /// Player-versus-player combat is disabled in this cell.
        const PVP_DISABLED = 0x10;
    }
}

/// Errors produced while reading, constructing, or writing a map cell grid.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MapCellGridError {
    #[error("map cell payload is too short: need at least 4 bytes, got {actual}")]
    TooShort { actual: usize },
    #[error("map cell dimensions must be positive signed 16-bit values, got {width}x{height}")]
    InvalidDimensions { width: i32, height: i32 },
    #[error("map cell count does not match {width}x{height}: expected {expected}, got {actual}")]
    CellCountMismatch {
        width: u16,
        height: u16,
        expected: usize,
        actual: usize,
    },
    #[error("map cell payload has {actual} bytes after its header, expected exactly {expected}")]
    InvalidPayloadLength { expected: usize, actual: usize },
}

pub type MapCellGridResult<T> = std::result::Result<T, MapCellGridError>;

/// A rectangular, row-major grid of map cell flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapCellGrid {
    width: u16,
    height: u16,
    cells: Vec<MapCellFlags>,
}

impl MapCellGrid {
    /// Construct a grid after checking its dimensions and cell count.
    pub fn new(width: u16, height: u16, cells: Vec<MapCellFlags>) -> MapCellGridResult<Self> {
        validate_dimensions(width, height)?;
        let expected = usize::from(width) * usize::from(height);
        if cells.len() != expected {
            return Err(MapCellGridError::CellCountMismatch {
                width,
                height,
                expected,
                actual: cells.len(),
            });
        }
        Ok(Self {
            width,
            height,
            cells,
        })
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn cells(&self) -> &[MapCellFlags] {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut [MapCellFlags] {
        &mut self.cells
    }

    /// Return the cell at `(x, y)`, or `None` when the coordinate is outside
    /// the grid.
    pub fn get(&self, x: u16, y: u16) -> Option<MapCellFlags> {
        self.cell_index(x, y).map(|index| self.cells[index])
    }

    /// Return a mutable reference to the cell at `(x, y)`.
    pub fn get_mut(&mut self, x: u16, y: u16) -> Option<&mut MapCellFlags> {
        let index = self.cell_index(x, y)?;
        self.cells.get_mut(index)
    }

    fn cell_index(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(usize::from(y) * usize::from(self.width) + usize::from(x))
    }
}

impl<'de> Deserialize<'de> for MapCellGrid {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Fields {
            width: u16,
            height: u16,
            cells: Vec<MapCellFlags>,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(fields.width, fields.height, fields.cells).map_err(serde::de::Error::custom)
    }
}

/// Decode one uncompressed `NStcData` archive-entry payload.
pub fn decode_map_cell_grid(data: &[u8]) -> MapCellGridResult<MapCellGrid> {
    if data.len() < 4 {
        return Err(MapCellGridError::TooShort { actual: data.len() });
    }

    let mut reader = ByteReader::new(data);
    let width = reader
        .read_i16_le("map_cell_grid.width")
        .expect("map cell header length was checked");
    let height = reader
        .read_i16_le("map_cell_grid.height")
        .expect("map cell header length was checked");
    if width <= 0 || height <= 0 {
        return Err(MapCellGridError::InvalidDimensions {
            width: i32::from(width),
            height: i32::from(height),
        });
    }

    let width = width as u16;
    let height = height as u16;
    let expected = usize::from(width) * usize::from(height);
    let actual = reader.remaining();
    if actual != expected {
        return Err(MapCellGridError::InvalidPayloadLength { expected, actual });
    }

    let cells = reader
        .read_bytes("map_cell_grid.cells", expected)
        .expect("map cell payload length was checked")
        .iter()
        .copied()
        .map(MapCellFlags::from_bits_retain)
        .collect();
    MapCellGrid::new(width, height, cells)
}

/// Encode one `NStcData` archive-entry payload.
pub fn encode_map_cell_grid(grid: &MapCellGrid) -> MapCellGridResult<Vec<u8>> {
    validate_dimensions(grid.width, grid.height)?;
    let expected = usize::from(grid.width) * usize::from(grid.height);
    if grid.cells.len() != expected {
        return Err(MapCellGridError::CellCountMismatch {
            width: grid.width,
            height: grid.height,
            expected,
            actual: grid.cells.len(),
        });
    }

    let mut encoded = Vec::with_capacity(4 + expected);
    encoded.extend_from_slice(&(grid.width as i16).to_le_bytes());
    encoded.extend_from_slice(&(grid.height as i16).to_le_bytes());
    encoded.extend(grid.cells.iter().map(|flags| flags.bits()));
    Ok(encoded)
}

fn validate_dimensions(width: u16, height: u16) -> MapCellGridResult<()> {
    if width == 0 || height == 0 || width > i16::MAX as u16 || height > i16::MAX as u16 {
        return Err(MapCellGridError::InvalidDimensions {
            width: i32::from(width),
            height: i32::from(height),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_row_major_cells_and_combined_flags() {
        let data = [2, 0, 2, 0, 0x01, 0x06, 0x18, 0x00];
        let grid = decode_map_cell_grid(&data).unwrap();

        assert_eq!((grid.width(), grid.height()), (2, 2));
        assert_eq!(grid.get(0, 0), Some(MapCellFlags::WALKING_DISABLED));
        assert_eq!(grid.get(1, 0).unwrap().bits(), 0x06);
        assert_eq!(grid.get(0, 1).unwrap().bits(), 0x18);
        assert_eq!(grid.get(2, 0), None);
    }

    #[test]
    fn round_trips_named_and_unnamed_bits() {
        let data = [3, 0, 1, 0, 0x01, 0x1f, 0xe0];
        let grid = decode_map_cell_grid(&data).unwrap();
        assert_eq!(grid.get(2, 0).unwrap().bits(), 0xe0);
        assert_eq!(encode_map_cell_grid(&grid).unwrap(), data);
    }

    #[test]
    fn mutates_one_checked_cell() {
        let mut grid = MapCellGrid::new(2, 1, vec![MapCellFlags::empty(); 2]).unwrap();
        grid.get_mut(1, 0)
            .unwrap()
            .insert(MapCellFlags::PVP_DISABLED);
        assert_eq!(encode_map_cell_grid(&grid).unwrap(), [2, 0, 1, 0, 0, 0x10]);
        assert!(grid.get_mut(2, 0).is_none());
    }

    #[test]
    fn rejects_invalid_payloads() {
        assert_eq!(
            decode_map_cell_grid(&[1, 0, 1]),
            Err(MapCellGridError::TooShort { actual: 3 })
        );
        assert!(matches!(
            decode_map_cell_grid(&[0, 0, 1, 0]),
            Err(MapCellGridError::InvalidDimensions { .. })
        ));
        assert_eq!(
            decode_map_cell_grid(&[2, 0, 1, 0, 0x01]),
            Err(MapCellGridError::InvalidPayloadLength {
                expected: 2,
                actual: 1
            })
        );
        assert_eq!(
            decode_map_cell_grid(&[1, 0, 1, 0, 0, 0]),
            Err(MapCellGridError::InvalidPayloadLength {
                expected: 1,
                actual: 2
            })
        );
    }

    #[test]
    fn rejects_invalid_construction() {
        assert!(matches!(
            MapCellGrid::new(0, 1, Vec::new()),
            Err(MapCellGridError::InvalidDimensions { .. })
        ));
        assert!(matches!(
            MapCellGrid::new(2, 2, vec![MapCellFlags::empty(); 3]),
            Err(MapCellGridError::CellCountMismatch { .. })
        ));
        assert!(matches!(
            MapCellGrid::new(0x8000, 1, vec![MapCellFlags::empty(); 0x8000]),
            Err(MapCellGridError::InvalidDimensions { .. })
        ));
    }
}
