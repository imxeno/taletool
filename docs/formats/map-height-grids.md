# Map Height Grids

`NSgrdData*.NOS` archives store optional map height grids. A grid partitions
collision triangles into X/Z cells so ground-height checks only need to test a
small subset of the map geometry.

Each archive payload contains a map ID. The containing archive entry ID is
separate metadata; it is not repeated as a grid ID inside the payload.

All integer and floating-point fields are little-endian.

## Preamble and Versions

Every payload starts with either a map ID or an explicit version tag:

| Offset | Type  | Field                                                                                           |
| ------ | ----- | ----------------------------------------------------------------------------------------------- |
| `0x00` | `u32` | Map ID for the implicit layout, or an explicit version tag.                                     |
| `0x04` | `i32` | Map ID when offset `0x00` contains an explicit version; otherwise the fixed header starts here. |

The recognized explicit tags are:

| Tag          | Triangle indices | Cell triangle references |
| ------------ | ---------------- | ------------------------ |
| `0x0BF82311` | `u16`            | `u16`                    |
| `0x0BF82312` | `i32`            | `i32`                    |

When offset `0x00` is not one of these tags, its bits are the signed map ID, and
the payload uses the 16-bit index layout without storing a version tag. A map ID
equal to either version tag requires an explicit encoding.

## Fixed Grid Header

The fixed header follows the map ID. Offsets below describe the implicit layout;
add four bytes for either explicit-version layout.

| Offset | Type        | Field                                                                 |
| ------ | ----------- | --------------------------------------------------------------------- |
| `0x04` | `u64`       | Declared payload size.                                                |
| `0x0C` | `vec3<f32>` | World-space bounds minimum.                                           |
| `0x18` | `vec3<f32>` | World-space bounds maximum.                                           |
| `0x24` | `u16`       | Grid width in X cells.                                                |
| `0x26` | `u16`       | Grid depth in Z cells.                                                |
| `0x28` | `u32`       | Stored cell count.                                                    |
| `0x2C` | `vec3<f32>` | Cell size. The runtime lookup divides X and Z by the first component. |
| `0x38` | `u32`       | Vertex count.                                                         |
| `0x3C` | `u32`       | Triangle count.                                                       |

Vertex data begins at `0x40` in the implicit layout and `0x44` in either
explicit layout.

NosTale rejects a declared size greater than the decoded size in the archive
record header. Smaller values, including zero, are accepted and do not limit
parsing of the stored arrays.

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

NosTale reads the stored cell count independently of the dimensions. Each row
contains:

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
