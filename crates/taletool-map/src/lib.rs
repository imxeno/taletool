//! NosTale map-related asset formats.
//!
//! This crate supports cell-flag grids stored in `NStcData` archive entries
//! and map height grids stored in `NSgrdData` archive entries.

pub mod cell_flags;
pub mod height_grid;

pub use cell_flags::{
    MapCellFlags, MapCellGrid, MapCellGridError, MapCellGridResult, decode_map_cell_grid,
    encode_map_cell_grid,
};
pub use height_grid::{
    HEIGHT_GRID_VERSION_1, HEIGHT_GRID_VERSION_2, HeightGrid, HeightGridBounds,
    HeightGridDimensions, HeightGridEncoding, HeightGridError, HeightGridResult,
    decode_height_grid, write_height_grid_bytes,
};

/// Compatibility alias for the original cell-grid error name.
pub type MapError = MapCellGridError;
/// Compatibility alias for the original cell-grid result type.
pub type Result<T> = MapCellGridResult<T>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_root_cell_grid_names_and_exposes_format_modules() {
        let legacy: Result<()> = Err(MapError::TooShort { actual: 0 });
        assert_eq!(legacy, Err(MapCellGridError::TooShort { actual: 0 }));
        assert_eq!(cell_flags::MapCellFlags::WALKING_DISABLED.bits(), 0x01);
        assert_eq!(height_grid::HEIGHT_GRID_VERSION_1, HEIGHT_GRID_VERSION_1);
    }
}
