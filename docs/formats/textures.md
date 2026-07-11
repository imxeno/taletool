# Textures

NosTale textures are proprietary raster payloads stored as records in binary
`.NOS` archives. Each payload contains an eight-byte header followed by one or
more tightly packed mip levels. It has no magic value, resource ID, or filename;
the containing archive supplies the resource ID.

| Archive                                     | Contents                                                                                                                                                     |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `NStpData*.NOS`                             | Textures for the main 3D scene, including world and model textures.                                                                                          |
| `NStpeData*.NOS`                            | Textures for effects, keyed in the `0x4F...` range.                                                                                                          |
| `NStpuData*.NOS`, `NStpuData_<locale>*.NOS` | UI/widget textures. These textures are keyed in the `0x5F...` range and are used by panels, forms, buttons, gauges, list views, and other interface widgets. |

All integer fields and multi-byte pixels are little-endian.

## Header

The texture header is eight bytes:

| Offset | Field           | Type  | Notes                                                                          |
| ------ | --------------- | ----- | ------------------------------------------------------------------------------ |
| `0x00` | Width           | `u16` | Texture width in pixels.                                                       |
| `0x02` | Height          | `u16` | Texture height in pixels. The client requires square images.                   |
| `0x04` | Pixel format    | `u8`  | Selects one of the five formats below.                                         |
| `0x05` | Filter flag     | `u8`  | `0` selects point filtering; any non-zero value selects linear filtering.      |
| `0x06` | Opaque byte 06  | `u8`  | Retained in the texture-cache key but not otherwise interpreted by the client. |
| `0x07` | Mip level count | `u8`  | Stored level count; `0` is treated as one level.                               |

The client reads byte `0x06` as part of the complete header and uses the
complete header to group reusable cache entries. No texture creation,
pixel-loading, or sampler-state path reads that byte individually.

Width and height must both be non-zero and equal. A mip level halves each
dimension, with a minimum of one pixel. The client derives the next stored byte
count by dividing the previous level's byte count by four.

## Pixel Formats

| Kind | Format     | Bytes/pixel | Stored channels                               |
| ---- | ---------- | ----------- | --------------------------------------------- |
| `0`  | `A4R4G4B4` | 2           | Four 4-bit channels in one little-endian word |
| `1`  | `A1R5G5B5` | 2           | One alpha bit and three 5-bit color channels  |
| `2`  | `A8R8G8B8` | 4           | Blue, green, red, alpha byte order            |
| `3`  | `L8`       | 1           | Grayscale luminance                           |
| `4`  | `A8`       | 1           | Alpha only                                    |

`A4R4G4B4` and `A1R5G5B5` use these bit layouts:

| Format     | Bits     | Channel |
| ---------- | -------- | ------- |
| `A4R4G4B4` | `15..12` | Alpha   |
|            | `11..8`  | Red     |
|            | `7..4`   | Green   |
|            | `3..0`   | Blue    |
| `A1R5G5B5` | `15`     | Alpha   |
|            | `14..10` | Red     |
|            | `9..5`   | Green   |
|            | `4..0`   | Blue    |

The client expands 4- and 5-bit channels across the full 8-bit range and treats
`L8` as opaque grayscale. Its fallback for unsupported alpha-only Direct3D
textures expands `A8` to black RGB with the stored alpha.
