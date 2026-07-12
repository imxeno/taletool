//! NosTale map-related asset formats.
//!
//! This crate supports maps stored in `NStuData` archive entries, cell-flag
//! grids stored in `NStcData`, and height grids stored in `NSgrdData`.

pub mod cell_flags;
pub mod height_grid;
pub mod map;

pub use cell_flags::{
    MapCellFlags, MapCellGrid, MapCellGridError, MapCellGridResult, decode_map_cell_grid,
    encode_map_cell_grid,
};
pub use height_grid::{
    HEIGHT_GRID_VERSION_1, HEIGHT_GRID_VERSION_2, HeightGrid, HeightGridBounds,
    HeightGridDimensions, HeightGridEncoding, HeightGridError, HeightGridResult,
    decode_height_grid, write_height_grid_bytes,
};
pub use map::{
    BoundingSphere, Bounds3, CameraAngleLimits, GeometryNode, MAP_HEADER_LEN,
    MAP_HEADER_UNKNOWN_00_LEN, MAP_HEADER_UNKNOWN_79_LEN, MAP_MAX_NODE_DEPTH, Map, MapError,
    MapHeader, MapNode, MapNodeKind, MapResult, Rgba8, decode_map, write_map_bytes,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_map_format_modules() {
        assert_eq!(cell_flags::MapCellFlags::WALKING_DISABLED.bits(), 0x01);
        assert_eq!(height_grid::HEIGHT_GRID_VERSION_1, HEIGHT_GRID_VERSION_1);
        assert_eq!(map::MAP_HEADER_LEN, MAP_HEADER_LEN);
    }
}
