# Sprite Resource Remaps

Individual entries in `NSpmData.NOS` describe how the eight player rendering
resource slots are reordered for each sprite frame.

An `NSpnData.NOS` [CCINF entry](ccinf.md) supplies the remap table file id. The
active [`NSpcData` animation](sprite-animations.md) supplies a sprite frame
index, which selects a row from that remap payload.

## Payload Layout

Each extracted payload begins with a one-byte frame count followed by eight
resource-index bytes per frame:

| Offset | Type                 | Field                                |
| ------ | -------------------- | ------------------------------------ |
| `0x00` | `u8`                 | Remap frame count.                   |
| `0x01` | `frame[frame_count]` | Ordered eight-byte remap frame rows. |

Frame row `i` begins at `1 + i * 8`. Within a row, byte position `0..7` is the
rendering resource slot and its byte value is the source resource index to use
for that slot.

If the active sprite frame index is greater than or equal to the remap frame
count, the renderer falls back to identity ordering (`0, 1, 2, 3, 4, 5, 6, 7`).
A resource index above `7` causes the corresponding rendering slot to be
skipped. Rows are not required to be permutations; duplicate and out-of-range
bytes are lossless raw data even though currently observed rows commonly are
permutations.

The largest representable payload contains 255 remap frames. A zero-frame
payload consists of the single byte `00`.
