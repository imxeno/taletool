# Sprite Payloads

NosTale uses two sprite payload families. Map-object sprites contain one or more
descriptor-based `A4R4G4B4` frames. Free-size sprites contain one
block-interlaced `A8R8G8B8` image. Neither format has a magic value or embedded
resource id; the containing binary `.NOS` archive supplies the id.

## Map-Object Sprites

Entries in `NSmpData*.NOS`, `NSppData*.NOS`, and `NSipData.NOS` files use the
map-object format. The first byte is a frame count, followed by one 12-byte
descriptor per frame and the referenced pixel blocks.

All integer fields and pixels are little-endian.

| Offset | Type       | Field                                     |
| ------ | ---------- | ----------------------------------------- |
| `0x00` | `u8`       | Frame count.                              |
| `0x01` | descriptor | First frame descriptor.                   |
| varies | descriptor | Remaining descriptors, 12 bytes each.     |
| varies | pixel data | Frame pixel blocks at descriptor offsets. |

Each frame descriptor is 12 bytes:

| Offset from descriptor | Type  | Field                                              |
| ---------------------- | ----- | -------------------------------------------------- |
| `0x00`                 | `u16` | Width in pixels.                                   |
| `0x02`                 | `u16` | Height in pixels.                                  |
| `0x04`                 | `i16` | Source X placement coordinate.                     |
| `0x06`                 | `i16` | Source Y placement coordinate.                     |
| `0x08`                 | `u32` | Absolute pixel-data offset from the payload start. |

The source coordinates place the frame relative to its logical origin. They are
not crop coordinates. Known payloads place pixel blocks contiguously in frame
order immediately after the descriptor table, but readers must use each
descriptor's absolute offset.

A frame contains `width * height` row-major `A4R4G4B4` pixels. One little-endian
word stores four 4-bit channels:

| Bits     | Channel |
| -------- | ------- |
| `15..12` | Alpha   |
| `11..8`  | Red     |
| `7..4`   | Green   |
| `3..0`   | Blue    |

Decoding expands a nibble by replication, so `0xA` becomes `0xAA`. Packing an
edited PNG quantizes each 8-bit channel to the nearest 4-bit value. PNGs
produced by Taletool therefore rebuild their source pixels exactly.

## Free-Size Sprites

Entries in `NS4BbData.NOS` use the free-size format. These assets are typically
splash screens, login backgrounds, and other large screen images. Each payload
contains one image and begins with a 4-byte header:

| Offset | Type  | Field             |
| ------ | ----- | ----------------- |
| `0x00` | `u16` | Width in pixels.  |
| `0x02` | `u16` | Height in pixels. |
| `0x04` | bytes | Pixel blocks.     |

The image contains `width * height` `A8R8G8B8` pixels. Each little-endian 32-bit
value has this layout:

| Bits     | Channel |
| -------- | ------- |
| `31..24` | Alpha   |
| `23..16` | Red     |
| `15..8`  | Green   |
| `7..0`   | Blue    |

The byte order in the file is blue, green, red, alpha.

Free-size sprite pixels are block-interlaced rather than one row-major image
stream. The image is divided into blocks up to 256 by 256 pixels. Complete block
columns are stored from left to right; blocks within a column are stored from
top to bottom. Pixels inside each block are row-major. Partial right and bottom
blocks use their actual dimensions and have no padding.

For example, a 1024 by 768 image stores blocks in this order:

```text
(0, 0), (0, 256), (0, 512),
(256, 0), (256, 256), (256, 512),
(512, 0), (512, 256), (512, 512),
(768, 0), (768, 256), (768, 512)
```
