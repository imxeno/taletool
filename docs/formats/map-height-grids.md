# Map Height Grids

`NSgrdData*.NOS` archives store optional map height grids. A grid partitions
collision triangles into X/Z cells so ground-height checks only need to test a
small subset of the map geometry.

Each archive payload contains its own grid and map identifiers. In observed
files both identifiers also match the containing archive entry ID, but the
fields are independent and are preserved separately.

All integer and floating-point fields are little-endian.

## Preamble and Versions

Every payload starts with a grid ID followed by either a map ID or an explicit
version tag:

| Offset | Type  | Field                                                                     |
| ------ | ----- | ------------------------------------------------------------------------- |
| `0x00` | `i32` | Grid ID.                                                                  |
| `0x04` | `u32` | Map ID for the implicit layout, or an explicit version tag.               |
| `0x08` | `i32` | Map ID when offset `0x04` contains an explicit version; otherwise absent. |

The recognized explicit tags are:

| Tag          | Triangle indices | Cell triangle references |
| ------------ | ---------------- | ------------------------ |
| `0x0BF82311` | `u16`            | `u16`                    |
| `0x0BF82312` | `i32`            | `i32`                    |

When offset `0x04` is not one of these tags, uses the `0x0BF82311` index layout
without storing a version tag.

## Fixed Grid Header

The fixed header follows the map ID. Offsets below describe the implicit layout;
add four bytes for either explicit-version layout.

| Offset | Type        | Field                                                                 |
| ------ | ----------- | --------------------------------------------------------------------- |
| `0x08` | `u64`       | Total payload size, including the grid ID and optional version tag.   |
| `0x10` | `vec3<f32>` | World-space bounds minimum.                                           |
| `0x1C` | `vec3<f32>` | World-space bounds maximum.                                           |
| `0x28` | `u16`       | Grid width in X cells.                                                |
| `0x2A` | `u16`       | Grid depth in Z cells.                                                |
| `0x2C` | `u32`       | Cell count; valid grids use `width * depth`.                          |
| `0x30` | `vec3<f32>` | Cell size. The runtime lookup divides X and Z by the first component. |
| `0x3C` | `u32`       | Vertex count.                                                         |
| `0x40` | `u32`       | Triangle count.                                                       |

The declared payload size includes every byte from the grid ID through the last
cell row. Bounds and cell-size components must be finite. Dimensions and
cell-size components are positive.

## Vertices and Triangles

Vertices follow the fixed header. Each vertex is a world-space `vec3<f32>`.

The triangle array follows the vertices. Each triangle stores three indices into
the vertex array. The implicit layout and explicit `0x0BF82311` layout store
`u16` indices. The `0x0BF82312` layout stores signed `i32` values; valid indices
are non-negative.

The runtime intersection path tests triangle vertices in stored order `0, 2,
1`.
The payload itself retains the original three-index order.

## Cell Rows

Cell rows follow the triangle array in X/Z row-major order:

```text
cell_index = grid_width * z + x
```

There are exactly `grid_width * grid_depth` rows. Each row contains:

| Field                    | Type               | Meaning                               |
| ------------------------ | ------------------ | ------------------------------------- |
| Triangle reference count | `u16`              | Number of following triangle indices. |
| Triangle references      | `u16[]` or `i32[]` | Indices into the triangle array.      |

Reference width follows the version table above. Empty rows store a zero count
and no references.

## Runtime Use

When map resources are refreshed, the game looks for a grid whose ID matches the
active root map resource. If found, a ground-height lookup:

- subtracts the grid bounds minimum from the queried position
- converts X/Z to a grid cell using the cell size
- reads that cell's triangle references
- casts a ray downward from `Y = 1000`
- updates the queried Y coordinate with the nearest hit

If no matching grid exists, the normal map geometry path handles the query.
