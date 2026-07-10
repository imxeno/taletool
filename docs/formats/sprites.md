Sprites
=======

Entries extracted from `NSmpData*.NOS`, `NSppData*.NOS`, and `NSipData*.NOS`
are multi-frame sprite payloads. A payload stores an ordered frame descriptor
table followed by 16-bit pixel blocks. It has no magic value or embedded
resource id; the containing binary `.NOS` archive supplies the id.


Layout
------

All integer fields and pixels are little-endian.

| Offset | Type       | Field                                      |
| ------ | ---------- | ------------------------------------------ |
| `0x00` | `u8`       | Frame count.                               |
| `0x01` | descriptor | First frame descriptor.                    |
| varies | descriptor | Remaining descriptors, 12 bytes each.     |
| varies | pixel data | Frame pixel blocks at descriptor offsets.  |

Each frame descriptor is 12 bytes:

| Offset from descriptor | Type  | Field                                              |
| ---------------------- | ----- | -------------------------------------------------- |
| `0x00`                 | `u16` | Width in pixels.                                   |
| `0x02`                 | `u16` | Height in pixels.                                  |
| `0x04`                 | `i16` | Source X placement coordinate.                     |
| `0x06`                 | `i16` | Source Y placement coordinate.                     |
| `0x08`                 | `u32` | Absolute pixel-data offset from the payload start. |

The source coordinates are placement metadata. The renderer uses them to position
the frame relative to the map object's origin; in particular, source Y is also
used by sprite-height calculations.

Known payloads place pixel blocks contiguously in frame order immediately after
the descriptor table. Readers must still use each descriptor's absolute offset.


Pixel Encoding
--------------

Each frame contains `width * height` row-major `A4R4G4B4` pixels. One
little-endian word stores four 4-bit channels:

| Bits     | Channel |
| -------- | ------- |
| `15..12` | Alpha   |
| `11..8`  | Red     |
| `7..4`   | Green   |
| `3..0`   | Blue    |