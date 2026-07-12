# Map Payloads

Entries in `NStuData*.NOS` describe a map's environment, geometry resource
table, and recursive object tree. Geometry nodes refer to entries in the local
table, whose values are resource keys in `NStgData*.NOS`.

All integers and floating-point values are little-endian. The archive container
stores these payloads with zlib compression; the layout below starts at the
decoded entry data.

## Map Header

Every payload starts with a 133-byte header.

| Offset | Type        | Field                                                         |
| ------ | ----------- | ------------------------------------------------------------- |
| `0x00` | `u8[30]`    | `unknown_00`; preserved data with no identified runtime use.  |
| `0x1E` | `u8`        | Resource group used with companion scene metadata.            |
| `0x1F` | `aabb<f32>` | Scene bounds: minimum `vec3`, then maximum `vec3`.            |
| `0x37` | `aabb<f32>` | Bounds consulted by the fallback ground-height path.          |
| `0x4F` | sphere      | Center `vec3` and radius used to seed fallback ground height. |
| `0x5F` | color       | Ambient-light RGBA channels, stored in G, B, A, R byte order. |
| `0x63` | color       | Diffuse-light RGBA channels, stored in G, B, A, R byte order. |
| `0x67` | `u32`       | Packed renderer fog color.                                    |
| `0x6B` | `i16[3]`    | Yaw angle, minimum offset, and maximum offset in degrees.     |
| `0x71` | `i16[3]`    | Pitch angle, minimum offset, and maximum offset in degrees.   |
| `0x77` | `u8`        | Normalized fog-start distance.                                |
| `0x78` | `u8`        | Normalized fog-end distance.                                  |
| `0x79` | `u8[10]`    | `unknown_79`; preserved data with no identified runtime use.  |
| `0x83` | `u8`        | Reset-yaw flag (`0` or `1`).                                  |
| `0x84` | `u8`        | `unknown_84`; preserved byte with no identified runtime use.  |

The runtime maps each fog-distance byte from `0..=255` to `0..=150` world units.
When the reset-yaw flag is set, the camera yaw is moved to the stored angle when
the scene loads. Otherwise, the current yaw is clamped to the stored limits.
Pitch is also clamped to its limits.

When optimized height-grid data is unavailable, the fallback query initializes Y
to the ground sphere's bottom point (`center.y - radius`). It traces scene
geometry from there and uses the ground bounds' minimum Y when deciding whether
the result lies below the usable scene area.

The typed API and JSON expose the known fields and retain all unidentified
bytes. Writers require the preserved byte regions to keep their exact native
lengths.

## Geometry Resource Table

The header is followed by a `u16` geometry-key count and that many signed 32-bit
resource keys:

```text
u16 geometry_key_count
i32 geometry_keys[geometry_key_count]
```

Geometry nodes store a `u16` table index, not a resource key directly. A valid
index is less than `geometry_key_count`.

## Recursive Node Lists

The geometry table is followed by the root node list. Every root or child list
starts with a `u16` count, followed immediately by that many node records. Every
node ends with another counted list containing its children.

The first byte of a node selects one of four layouts:

| Kind | Meaning                                                 |
| ---- | ------------------------------------------------------- |
| `0`  | Grouping node used for hierarchical visibility checks.  |
| `1`  | Static geometry node.                                   |
| `2`  | Geometry node submitted with an animation frame offset. |
| `3`  | Effect geometry with animation-driven render state.     |

Grouping nodes contain only a bounding sphere before their child list:

| Type        | Field          |
| ----------- | -------------- |
| `vec3<f32>` | Sphere center. |
| `f32`       | Sphere radius. |

The sphere covers the node's descendant branch and is checked before walking its
children.

## Shared Geometry Fields

Kinds 1, 2, and 3 start with the same 66-byte geometry record:

| Type        | Field                                                        |
| ----------- | ------------------------------------------------------------ |
| `u16`       | Index into the payload's geometry resource table.            |
| `u8[4]`     | Material color in R, G, B, A order.                          |
| `aabb<f32>` | World-space bounds: minimum `vec3`, then maximum `vec3`.     |
| `vec3<f32>` | Bounding-sphere center.                                      |
| `f32`       | Bounding-sphere radius.                                      |
| `vec3<f32>` | Position relative to the scene or companion resource origin. |
| `vec4<i16>` | Packed rotation quaternion in X, Y, Z, W order.              |

The packed quaternion components are converted to floating point with a scale of
`1 / 32768`.

A kind-1 node proceeds directly to its child list. A kind-2 node appends:

| Type  | Field                   |
| ----- | ----------------------- |
| `f32` | Uniform geometry scale. |
| `u16` | Animation frame offset. |

The scale is applied to all three basis vectors of the node's rotation matrix
before the transform is submitted for kinds 2 and 3.

## Effect Geometry

A kind-3 node extends the same kind-2 fields with:

| Type  | Field                                                                  |
| ----- | ---------------------------------------------------------------------- |
| `i16` | Color-animation ID, or a negative value to use the material color.     |
| `i16` | Transform-animation ID, or a negative value for the default transform. |
| `i16` | Texture-animation ID, or a negative value for geometry textures.       |
| `u16` | `unknown_66`; no runtime use has been identified.                      |
| `u16` | Source blend factor.                                                   |
| `u16` | Destination blend factor.                                              |
| `u8`  | Billboard flag (`0` or `1`).                                           |

The animation IDs address the corresponding effect-animation archive families.
The frame offset is supplied when resolving color, transform, and texture
animation data. Billboard nodes additionally apply the camera orientation.

## Validation and Editing

Taletool rejects truncated records, unknown node kinds, invalid booleans,
non-finite floats, reversed bounds, negative sphere radii, out-of-range geometry
indices, trailing bytes, and excessively deep trees. Node-list and geometry-key
counts must fit their native `u16` fields.

Use the archive and map commands together:

```powershell
taletool archive unpack NStuData.NOS --out nstu
taletool map inspect nstu/42.bin --json --checksum
taletool map unpack nstu/42.bin --out 42.json
taletool map pack 42.json --out 42.bin
taletool archive pack nstu --out NStuData.NOS
```

Map JSON documents use format name `map`, with the decoded map stored in the
top-level `map` field. Uninterpreted native fields use `unknown_<hex offset>`
names rather than inferred semantic names.
