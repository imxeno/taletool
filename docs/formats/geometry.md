# Geometry Payloads

Entries in `NStgData*.NOS` and `NStgeData*.NOS` use the same geometry format.
`NStgData` contains scene and model geometry; `NStgeData` contains effect
geometry. The containing binary `.NOS` archive supplies the resource id. The
payload itself has no magic value or embedded id.

All numeric fields are little-endian. Counts are unsigned unless noted.

## Fixed Header

Every payload starts with a 52-byte header.

| Offset | Type        | Field                     |
| ------ | ----------- | ------------------------- |
| `0x00` | `vec3<f32>` | Bounds minimum.           |
| `0x0C` | `vec3<f32>` | Bounds maximum.           |
| `0x18` | `vec3<f32>` | Bounding-sphere center.   |
| `0x24` | `f32`       | Bounding-sphere radius.   |
| `0x28` | `i16`       | First animation frame.    |
| `0x2A` | `i16`       | Last animation frame.     |
| `0x2C` | `i16`       | Animation frame rate.     |
| `0x2E` | `i16`       | Keyframe step.            |
| `0x30` | `f32`       | Texture-coordinate scale. |

The stored bounds are not necessarily the component-wise bounds of the raw
position array because nodes carry their own transforms.

For the ordinary animated submission path, the client derives a `u16` key time
from the frame range, frame rate, and keyframe step. Effect submission can
instead supply explicit timing values. Key times stored on nodes use the same
scaled timeline.

## Vertex Arrays

The header is followed by a `u16` vertex count and three parallel arrays. Each
array contains exactly that many entries.

| Array               | Entry type  | Bytes per vertex |
| ------------------- | ----------- | ---------------- |
| Positions           | `vec3<f32>` | 12               |
| Texture coordinates | `vec2<i16>` | 4                |
| Normals             | `vec3<i8>`  | 3                |

The runtime converts the signed texture coordinates to floats. Render paths that
request geometry texture scaling apply the header scale to both texture axes.
Normals are stored as signed packed components and are converted directly to
runtime floats.

## Triangle Lists

After the vertex arrays, a two-byte count slot stores a `u8` triangle-list count
followed by one zero reserved byte. Each triangle list contains:

| Field       | Type         | Meaning                               |
| ----------- | ------------ | ------------------------------------- |
| Index count | `u16`        | Number of following vertex indices.   |
| Indices     | `u16[count]` | Indexes into the shared vertex table. |

The client submits them as `D3DPT_TRIANGLELIST`. Every three consecutive indices
form one triangle. Lists are concatenated into the runtime index buffer, while
node batches retain an index identifying their source list.

The sum of all list lengths is not a field in the payload and can exceed 65,535
in shipped data even though each individual list uses a 16-bit count.

## Node Forest

A second two-byte count slot stores the root-node count as a `u8` followed by a
zero reserved byte. Root nodes and children use the same recursive record:

| Field                 | Type                   |
| --------------------- | ---------------------- |
| Base translation      | `vec3<f32>`            |
| Base rotation         | `vec4<i16>`            |
| Base scale            | `vec3<f32>`            |
| Translation keyframes | counted vector table   |
| Rotation keyframes    | counted rotation table |
| Scale keyframes       | counted vector table   |
| Render batches        | counted batch table    |
| Child count           | `u8`, then zero byte   |
| Children              | recursive node records |

Each counted keyframe or batch table starts with a `u16` count.

A vector keyframe is a `u16` key time followed by `vec3<f32>`. A rotation
keyframe is a `u16` key time followed by `vec4<i16>`. The packed quaternion
components are ordered X, Y, Z, W. The client converts each component to float
by multiplying it by `1 / 32768`.

Keyframe times are strictly ascending within each channel.

## Render Batches

Each seven-byte batch has this layout:

| Offset | Type  | Field                 |
| ------ | ----- | --------------------- |
| `0x00` | `i32` | Texture resource key. |
| `0x04` | `u8`  | Disable-culling flag. |
| `0x05` | `u16` | Triangle-list index.  |

The flag is `0` or `1`. In the animated and caller-timed submission paths, `1`
selects no culling and `0` selects clockwise culling. It is not an alpha flag.
The static submission path establishes clockwise culling for the whole item and
does not consult the per-batch byte.
