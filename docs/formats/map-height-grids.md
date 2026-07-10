# Map Height Grids

`NSgrdData*.NOS` archives store optional optimized map height grids. Each
payload is keyed by a root bulk/map id and is used to resolve the ground height
for an X/Z world position.

| Archive          | Contents                                          |
| ---------------- | ------------------------------------------------- |
| `NSgrdData*.NOS` | Height grid payloads keyed by root bulk / map id. |

## Layout

The binary archive payload starts with an extra grid id prefix:

| Offset | Field     | Type  | Notes                                |
| ------ | --------- | ----- | ------------------------------------ |
| `0x00` | Grid id   | `i32` | Observed value matches the table id. |
| `0x04` | Grid body | bytes | Grid body.                           |

The grid body has two compatible variants.

| First dword     | Meaning                                                                           |
| --------------- | --------------------------------------------------------------------------------- |
| `0x0BF82311`    | Explicit version.                                                                 |
| `0x0BF82312`    | Explicit version.                                                                 |
| Any other value | No explicit version. This value is Map id; version is assumed to be `0x0BF82311`. |

The only currently observed `NSgrdData06.NOS` entry uses the no-explicit-version
form.

## Grid Body Header

Offsets below are for the no-explicit-version body. Add `4` to each offset for
the explicit-version body.

| Offset | Field          | Type        | Notes                                                    |
| ------ | -------------- | ----------- | -------------------------------------------------------- |
| `0x00` | Map id         | `i32`       | Matches the record ID in the container.                  |
| `0x04` | Data size      | `u64`       | Checked against the archive payload size.                |
| `0x0C` | Origin         | `vec3<f32>` | World-space origin used for X/Z cell lookup.             |
| `0x18` | Bounds vector  | `vec3<f32>` | Loaded by the client; not used by the known lookup path. |
| `0x24` | Grid width     | `u16`       | Cell count in X.                                         |
| `0x26` | Grid depth     | `u16`       | Cell count in Z.                                         |
| `0x28` | Cell count     | `u32`       | Usually `grid_width * grid_depth`.                       |
| `0x2C` | Cell size      | `vec3<f32>` | The client divides by the first scalar component.        |
| `0x38` | Vertex count   | `u32`       | Number of following vertices.                            |
| `0x3C` | Triangle count | `u32`       | Number of following triangle records.                    |

Vertices follow immediately after the header. Each vertex is `vec3<f32>`.

## Triangle Array

Triangle records follow the vertex array.

| Version                 | Stored type           | Client memory type    |
| ----------------------- | --------------------- | --------------------- |
| `0x0BF82311` or assumed | `u16 vertex_index[3]` | `i32 vertex_index[3]` |
| `0x0BF82312`            | `i32 vertex_index[3]` | `i32 vertex_index[3]` |

## Cell Rows

Cell rows follow the triangle array. Rows are stored in X/Z row-major order:

```text
cell_index = grid_width * z + x
```

Each row contains a list of triangle-array indices for that cell.

| Field          | Type               | Notes                                    |
| -------------- | ------------------ | ---------------------------------------- |
| Triangle count | `u16`              | Number of triangle indices in this cell. |
| Indices        | `u16[]` or `i32[]` | Index width follows the grid version.    |

For version `0x0BF82311`, including the assumed form, cell indices are `u16`.
For version `0x0BF82312`, cell indices are `i32`.

## Runtime Use

When map bulk resources are refreshed, the client tries to load a grid whose id
matches the root bulk sort key. If the grid exists, ground-height lookups use
the grid callbacks. If it does not exist, the client uses the default bulk
geometry callbacks.

The HKH lookup:

- subtracts the grid origin from the queried world position
- converts X/Z to a grid cell
- reads that cell's triangle-index list
- casts a ray downward from `Y = 1000`
- writes the hit height back to `position.Y`
