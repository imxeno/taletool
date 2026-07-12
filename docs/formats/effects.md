# Effect Payloads

Four binary `.NOS` archive families store effect definitions and animation
tracks:

| Archive family  | Payload                                        |
| --------------- | ---------------------------------------------- |
| `NSeffData.NOS` | Effect definitions and their component tracks. |
| `NSedData.NOS`  | Animated colors.                               |
| `NSemData.NOS`  | Animated 2D transforms.                        |
| `NSesData.NOS`  | Animated texture resource keys.                |

Technically, they could share one archive. The split reflects the client’s
resource-cache architecture.

The containing archive supplies the lookup id. Effect definitions also store a
resource key in their payload header. All numeric fields are little-endian.

## Effect Definitions

An `NSeffData` payload starts with a 24-byte root header:

| Offset | Type  | Field                        |
| ------ | ----- | ---------------------------- |
| `0x00` | `i32` | Effect resource key.         |
| `0x04` | `i32` | Loaded-tick slot.            |
| `0x08` | `i32` | Source-record slot.          |
| `0x0C` | `u16` | Component count.             |
| `0x0E` | `i32` | Child-records slot.          |
| `0x12` | `i32` | Reference-count slot.        |
| `0x16` | `u16` | Flags loader-workspace slot. |

Client never reads values stored in the slot fields. It looks like those fields
are serialized runtime data, aka Entwell might be snapshotting memory directly
to the cache. The loader overwrites all five slots while constructing its cache.
It writes the current tick count to `0x04`, the allocated source-record address
to `0x08`, the loaded component-array address to `0x0E`, and clears the
reference count and flags at `0x12` and `0x16`.

The header is followed by all fixed component records and then their variable
track data. A fixed component record is 192 bytes:

| Offset | Size   | Field                                 |
| ------ | ------ | ------------------------------------- |
| `0x00` | `0x60` | Typed component properties.           |
| `0x60` | `0x0C` | Object rotation track descriptor.     |
| `0x6C` | `0x0C` | Object scale track descriptor.        |
| `0x78` | `0x0C` | Object translation track descriptor.  |
| `0x84` | `0x0C` | Color track descriptor.               |
| `0x90` | `0x0C` | Texture-resource track descriptor.    |
| `0x9C` | `0x0C` | Texture rotation track descriptor.    |
| `0xA8` | `0x0C` | Texture scale track descriptor.       |
| `0xB4` | `0x0C` | Texture translation track descriptor. |

Each 12-byte descriptor has this layout:

| Descriptor offset | Type      | Field                                                                 |
| ----------------- | --------- | --------------------------------------------------------------------- |
| `0x00`            | `u8`      | Key count.                                                            |
| `0x01`            | `byte[3]` | Probably padding; always zero in client data dated 2008 through 2026. |
| `0x04`            | `i32`     | Key-array loader-workspace slot.                                      |
| `0x08`            | `i32`     | Value-array loader-workspace slot.                                    |

The loader overwrites the two array slots in its working copy after allocating
the corresponding arrays. Both stored array slots are nonzero for every nonempty
track and zero for every empty track. This is consistent with the observed
behavior of effect definitions.

### Component Properties

The first `0x60` bytes combine common rendering and scheduling properties with a
48-byte kind-specific region:

| Offset | Type         | Field                                                       |
| ------ | ------------ | ----------------------------------------------------------- |
| `0x00` | `byte[4]`    | Unknown; unused in the client.                              |
| `0x04` | `u8`         | Component kind: `0` positioned, `1` flying, `2` particle.   |
| `0x05` | `u8`         | Orientation and render-transform mode.                      |
| `0x06` | `u8`         | Timeline-input selection mode.                              |
| `0x07` | `u8`         | Record the resolved timeline value when nonzero.            |
| `0x08` | `i32`        | Source blend factor.                                        |
| `0x0C` | `i32`        | Destination blend factor.                                   |
| `0x10` | `i32`        | Geometry resource key.                                      |
| `0x14` | `i32`        | Callback value slot.                                        |
| `0x18` | `byte[0x30]` | Kind-specific properties, described below.                  |
| `0x48` | `u16[4]`     | First key, last key, rate, and units per key.               |
| `0x50` | `i32`        | Start offset in milliseconds.                               |
| `0x54` | `i32`        | Lifetime or animation-loop duration in milliseconds.        |
| `0x58` | `u8`         | Loop geometry animation when nonzero.                       |
| `0x59` | `u8`         | Animation cursor mode.                                      |
| `0x5A` | `u8`         | External-timing mode.                                       |
| `0x5B` | `u8`         | Enable the temporary depth state when nonzero.              |
| `0x5C` | `u8`         | Enable the object-transform track when nonzero.             |
| `0x5D` | `u8`         | Base effect scale; zero selects the runtime default of `1`. |
| `0x5E` | `u8`         | Unknown; unused in the client.                              |
| `0x5F` | `u8`         | Enable the texture-transform track when nonzero.            |

The callback value at `0x14` is ordinary serialized data. The callback function
itself is configured by the application and is not stored in this file.

Only kind values `0`, `1`, and `2` are valid. The client skips records with
other kind values.

The `0x18..0x47` kind-specific region is a fixed-size union. Positioned
components use only `0x18..0x22`, while flying and particle components use the
full 48 bytes. Keeping every component record at `0xC0` bytes lets the loader
copy records in bulk, locate a component by multiplication, and interpret the
same region according to the kind discriminator without separate property
allocations or variable-length parsing. This was done likely to optimize
performance.

The inactive `0x23..0x47` tail of positioned components commonly contains values
matching flying-property defaults. Client leaves this tail unread for positioned
components. The values likely remain because the producing tool initialized a
shared property structure and did not clear fields irrelevant to the selected
kind; retaining those values is incidental rather than part of the layout
optimization.

#### Positioned Components (`kind = 0`)

| Offset | Type       | Field                                                   |
| ------ | ---------- | ------------------------------------------------------- |
| `0x18` | `u8`       | Source/target placement mode.                           |
| `0x19` | `f32`      | Source actor-height scale.                              |
| `0x1D` | `u8`       | Refresh source height while updating when nonzero.      |
| `0x1E` | `u8`       | Aim at the target's height-adjusted point when nonzero. |
| `0x1F` | `f32`      | Target actor-height scale.                              |
| `0x23` | `byte[37]` | Inactive union tail.                                    |

Depending on the placement mode and available source/target actors, the client
creates either an actor-following effect or a fixed map/world effect.

#### Flying Components (`kind = 1`)

| Offset | Type                | Field                                                   |
| ------ | ------------------- | ------------------------------------------------------- |
| `0x18` | `u8`                | Source/target placement mode.                           |
| `0x19` | `f32`               | Target actor-height scale.                              |
| `0x1D` | `vec3<f32>`         | Target offset, rotated into the travel frame.           |
| `0x29` | `f32`               | Source actor-height scale.                              |
| `0x2D` | `vec3<f32>`         | Source offset, rotated into the travel frame.           |
| `0x39` | `f32`               | Distance removed from the path before completion.       |
| `0x3D` | `f32`               | Travel-rate factor used to calculate lifetime.          |
| `0x41` | `f32`               | Vertical scale of the sinusoidal travel offset.         |
| `0x45` | `u8`                | Rotate the sine offset before applying height when set. |
| `0x46` | packed 16-bit float | Horizontal scale of the sinusoidal travel offset.       |

#### Particle Components (`kind = 2`)

| Offset | Type                        | Field                                           |
| ------ | --------------------------- | ----------------------------------------------- |
| `0x18` | `u8`                        | Particle flags.                                 |
| `0x19` | `vec3<packed 16-bit float>` | Spawn offset.                                   |
| `0x1F` | `vec4<i16>`                 | Packed X, Y, Z, W rotation quaternion.          |
| `0x27` | `vec3<packed 16-bit float>` | Per-axis random range.                          |
| `0x2D` | `vec3<packed 16-bit float>` | Particle size.                                  |
| `0x33` | `byte[3]`                   | Per-axis randomization values.                  |
| `0x36` | `byte[3]`                   | Rotation randomization values.                  |
| `0x39` | `byte[3]`                   | Scale randomization values.                     |
| `0x3C` | `u8`                        | Gravity factor.                                 |
| `0x3D` | `u8`                        | Initial spawn count.                            |
| `0x3E` | `u8`                        | Subsequent spawn-count base.                    |
| `0x3F` | `u8`                        | Subsequent spawn-count random range.            |
| `0x40` | `u16`                       | Spawn-delay base in milliseconds.               |
| `0x42` | `u16`                       | Spawn-delay random range in milliseconds.       |
| `0x44` | `u16`                       | Particle-lifetime base in milliseconds.         |
| `0x46` | `u16`                       | Particle-lifetime random range in milliseconds. |

The packed 16-bit floats are not IEEE half-floats. Zero represents `0.0`; other
values use bit 15 for the sign, bits 14..11 for a four-bit exponent with bias 7,
and bits 10..0 for an eleven-bit mantissa. Taletool exposes the stored `u16`
value and a conversion to `f32`.

For each axis, half the value at `0x33` selects the centered randomization
bucket count. Rotation and scale randomization use the corresponding values as
exclusive random upper bounds. Gravity is the byte at `0x3C` multiplied by the
engine's fixed gravity increment. The first particle batch uses the count at
`0x3D`; later batches use `base + Random(range)` from `0x3E..0x3F`. Their next
spawn time is the current tick plus the base delay and `Random(delay_range)`.
Each particle expires after the base lifetime plus `Random(lifetime_range)`,
clamped to the containing effect's lifetime.

### Variable Track Data

Variable data is stored component by component in descriptor order. Within one
track, all `u16` times are stored first, followed by the parallel value array:

| Track value         | Stored type             |
| ------------------- | ----------------------- |
| Object rotation     | `vec4<f32>` quaternion. |
| Object scale        | `vec3<f32>`.            |
| Object translation  | `vec3<f32>`.            |
| Color               | Four BGRA bytes.        |
| Texture resource    | `i32` resource key.     |
| Texture rotation    | `vec4<f32>` quaternion. |
| Texture scale       | `vec3<f32>`.            |
| Texture translation | `vec3<f32>`.            |

The fixed records therefore carry track counts, while the variable section
carries no additional counts.

## Standalone Animation Timing

Color, transform, and texture animations begin with the same eight-byte timing
block:

| Offset | Type  | Field          |
| ------ | ----- | -------------- |
| `0x00` | `i16` | First frame.   |
| `0x02` | `i16` | Last frame.    |
| `0x04` | `i16` | Frame rate.    |
| `0x06` | `i16` | Keyframe step. |

At runtime, elapsed milliseconds are converted to a scaled `u16` key:

```text
((elapsed_ms * frame_rate * keyframe_step / 1000)
  % ((last_frame - first_frame + 1) * keyframe_step))
  + first_frame * keyframe_step
```

Key times within each track are strictly increasing.

## Color Animation

An `NSedData` payload stores the timing block, a `u16` key count, and six-byte
keys:

| Offset in key | Type       | Field                          |
| ------------- | ---------- | ------------------------------ |
| `0x00`        | `u16`      | Key time.                      |
| `0x02`        | `BGRA8888` | Blue, green, red, alpha bytes. |

The active value is interpolated independently in each channel between
neighboring keys. The final key is used directly when there is no following key.

## Texture Animation

An `NSesData` payload has the same ten-byte header as a color animation. Each
six-byte key contains a `u16` time followed by an `i32` texture resource key.
The active resource is the value at the greatest key time not exceeding the
current animation key.

Color and texture animations have identical structural layouts.

## Transform Animation

An `NSemData` payload adds three signed 32-bit offsets after the timing block:

| Offset | Type  | Field                                    |
| ------ | ----- | ---------------------------------------- |
| `0x08` | `i32` | Translation table offset.                |
| `0x0C` | `i32` | Packed-quaternion rotation table offset. |
| `0x10` | `i32` | Scale table offset.                      |

Offsets are relative to the start of the payload. Each target begins with a
`u16` count:

| Table       | Entry                                      |
| ----------- | ------------------------------------------ |
| Translation | `u16` time, `vec2<f32>` value.             |
| Rotation    | `u16` time, `vec4<i16>` packed quaternion. |
| Scale       | `u16` time, `vec3<f32>` value.             |

Readers resolve the three offsets independently. Tables may appear in any
physical order, multiple empty channels may share one zero-count table, and
padding or unreferenced bytes may occur between or after tables. Taletool does
not retain those layout-only bytes when editing an animation; packed output uses
contiguous translation, rotation, and scale tables in that order.

Quaternion components are ordered X, Y, Z, W. Rotation uses spherical
interpolation; translation and scale use component-wise linear interpolation.
