Textures
========

NosTale textures are proprietary format payloads stored inside texture archives.
Each payload starts with a small header that describes the dimensions, pixel
format, filtering behavior, and mip level count. The header is followed directly
by pixel data; there is no embedded file name, magic value, or external metadata
block inside the payload itself.

| Archive                                     | Contents                                                                                                                                                     |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `NStpData*.NOS`                             | Textures for the main 3D scene, including world/model textures.                                                                                              |
| `NStpeData*.NOS`                            | Textures for effects, keyed in the `0x4F...` range.                                                                                                          |
| `NStpuData*.NOS`, `NStpuData_<locale>*.NOS` | UI/widget textures. These textures are keyed in the `0x5F...` range and are used by panels, forms, buttons, gauges, list views, and other interface widgets. |


Layout
------

The texture header is 8 bytes:

| Offset | Field           | Type  | Notes                                                               |
| ------ | --------------- | ----- | ------------------------------------------------------------------- |
| `0x00` | Width           | `u16` | Texture width in pixels.                                            |
| `0x02` | Height          | `u16` | Texture height in pixels.                                           |
| `0x04` | Format kind     | `u8`  | Pixel format kind.                                                  |
| `0x05` | Filter flag     | `u8`  | `0` for sharp/nearest filtering, `1` for smoothed/linear filtering. |
| `0x06` | Unknown byte 06 | `u8`  | Unknown                                                             |
| `0x07` | Mip level count | `u8`  | Number of mip levels stored in the payload.                         |

Pixel data follows immediately. The base level is always present; additional
mip levels follow when mip level count is greater than one.

The filter flag controls how the texture is sampled when it is drawn:

 -  `0` keeps pixels sharp when scaled
 -  `1` smooths the texture when scaled.

The client treats any non-zero value like the smoothed mode, however no offical
files ever had a value different than `0` or `1`.


Pixel Formats
-------------

| Format kind | Format     |
| ----------- | ---------- |
| `0`         | `A4R4G4B4` |
| `1`         | `A1R5G5B5` |
| `2`         | `A8R8G8B8` |
| `3`         | `L8`       |
| `4`         | `A8`       |
