# Map Neighborhood Payloads

`NStkData.NOS` appears to be the remaining data format for an unfinished system
intended to make separate maps look connected at their edges. A record could
place additional `NStuData` map object trees around the active map, while a
second table of two-dimensional point sequences may have described transition
points.

The client contains working code for loading, positioning, culling, and
rendering the additional map object trees. It loads the point sequences but
never uses them. Furthermore, every `NStkData` record found in the surveyed
releases has both tables empty. The format and rendering path therefore
survived, but the larger system was probably never completed or enabled in
public builds of the game.

All numeric fields are little-endian. Counts are unsigned 16-bit integers. The
containing binary `.NOS` archive supplies the map resource ID.

## Layout

Every payload starts with this header:

| Offset | Type     | Field                                                                     |
| ------ | -------- | ------------------------------------------------------------------------- |
| `0x00` | `u32[2]` | Uninterpreted eight-byte prefix. All observed payloads contain `[8, 10]`. |
| `0x08` | `u16`    | Neighbor-map reference count.                                             |
| `0x0A` | variable | Neighbor-map references.                                                  |
| varies | `u16`    | Point-sequence count.                                                     |
| varies | variable | Point sequences.                                                          |

The client seeks eight bytes past the start of the archive-entry payload before
reading the first table count.

## Neighbor-Map References

Each reference is an 81-byte record:

| Record offset | Type        | Field                                                                   |
| ------------- | ----------- | ----------------------------------------------------------------------- |
| `0x00`        | `i32`       | Resource key of a neighboring map object tree in `NStuData.NOS`.        |
| `0x04`        | `u8`        | Group byte, must match the active map's root group to render.           |
| `0x05`        | `vec3<f32>` | Projected-texture bounds minimum.                                       |
| `0x11`        | `vec3<f32>` | Projected-texture bounds maximum.                                       |
| `0x1D`        | `vec3<f32>` | Visibility bounds minimum.                                              |
| `0x29`        | `vec3<f32>` | Visibility bounds maximum.                                              |
| `0x35`        | `vec3<f32>` | Visibility bounding-sphere center.                                      |
| `0x41`        | `f32`       | Visibility bounding-sphere radius.                                      |
| `0x45`        | `vec3<f32>` | Translation from neighboring-map coordinates to active-map coordinates. |

The client class name for this record is `TLBSBkNhInfoItem`; `Nh` plausibly
abbreviates "neighbor" or "neighborhood." Its behavior supports that reading:

- The resource key resolves another `NStuData` object tree and its geometry-key
  table.
- The projected-texture bounds define the orthographic transform used to project
  the neighboring map's map-wide texture.
- The visibility bounds and sphere are used for camera-frustum tests.
- The translation is applied to the linked objects and to the camera frustum
  used while traversing that object tree.
- Visible linked objects are added to the same render lists as objects from the
  active map's root object tree.
- The reference's group byte must match the active map's root group before the
  neighboring tree is considered for rendering.

### Root-Only State

Only `NStuData[R]`, the root entry, supplies the active map's lighting, fog,
camera limits, and other environment state. Environment fields from a
neighboring `NStuData` entry are not applied. The neighboring map is rendered
under the active map's environment. Linking `NStuData[A]` does not activate
`NStcData[A]` or `NSgrdData[A]` either. Consequently, a neighboring map can
provide visible terrain and structures without becoming walkable or interactive.

## Point Sequences

A point-sequence record has a 19-byte fixed portion plus four bytes per point:

| Record offset | Type               | Field                                          |
| ------------- | ------------------ | ---------------------------------------------- |
| `0x00`        | `u16[3]`           | Three leading values; purpose unknown.         |
| `0x06`        | `u8[8]`            | Eight-byte data block; purpose unknown.        |
| `0x0E`        | `u16`              | Point count.                                   |
| `0x10`        | `vec2<i16>[count]` | Signed two-dimensional points in stored order. |
| varies        | `u8`               | Trailing byte; purpose unknown.                |
| varies        | `u16`              | Trailing value; purpose unknown.               |

The client class name for this record is `TLBSBkMvInfoItem`; `Mv` may refer to
movement. The client constructs and fills the list, it appears to not read it
outside loading and list maintenance.

The most coherent theory is that the ordered points were supposed to trace a
map-edge handoff line or portal boundary in map-local cell coordinates. An
ordered polyline would allow a designer to follow an irregular terrain edge
instead of using a rectangular trigger. Under this theory, the surrounding
fields would identify the destination, its linked neighbor record, the permitted
crossing direction, or related transition parameters.

## Likely Intended Map-Edge Flow

Taken together, the two tables suggest the following unfinished design:

1. The server selects map `R`, and the client loads `NStuData[R]` as the active
   root together with its root-only environment and cell data.
2. `NStkData[R]` places one or more neighboring `NStuData` map object trees
   beyond the root's boundaries. The client renders them as non-interactive
   scenery.
3. A point sequence marks the irregular boundary associated with a neighbor.
   Approaching or crossing it prepares a handoff.
4. The server authorizes the map change and supplies the new active map ID.
5. The destination becomes the new root, at which point its own lighting, cell
   flags, height data, and gameplay objects replace those of the previous root.

This would create visual continuity near a map edge without requiring the client
to simulate two interactive maps simultaneously. It also explains why linked map
lighting and cell data are ignored: they would become active only after that map
became the root.

The client never performed steps 3 or 4 from the point-sequence data, and no
populated shipping record has been found. This flow is only a theory.
