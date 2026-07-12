//! Command handlers for the top-level CLI command groups.

/// Sprite-animation payload command handlers.
pub(crate) mod animation;
/// Archive container command handlers.
pub(crate) mod archive;
/// Audio metadata command handlers.
pub(crate) mod audio;
/// CCINF asset command handlers.
pub(crate) mod ccinf;
/// Map cell-flag payload command handlers.
pub(crate) mod cell_flag;
/// Effect asset command handlers.
pub(crate) mod effect;
/// Geometry payload command handlers.
pub(crate) mod geometry;
/// Map height-grid payload command handlers.
pub(crate) mod height_grid;
/// Map payload command handlers.
pub(crate) mod map;
/// Map-neighborhood payload command handlers.
pub(crate) mod map_neighborhood;
/// Patch package command handlers.
pub(crate) mod patch;
/// Directory scan command handler.
pub(crate) mod scan;
/// Sprite payload command handlers.
pub(crate) mod sprite;
/// Text payload command handler.
pub(crate) mod text;
/// Texture payload command handlers.
pub(crate) mod texture;
