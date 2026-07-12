# Taletool Docs

These pages document NosTale file formats that taletool knows about.

## Archive Formats

| Page                                                     | Covers                                                                    |
| -------------------------------------------------------- | ------------------------------------------------------------------------- |
| [Binary `.NOS` archives](formats/nos-binary-archives.md) | Standard `.NOS` container layout, split archives, filenames, compression. |
| [Text `.NOS` archives](formats/nos-text-archives.md)     | `NSgtdData`, `NSlangData`, `NScliData`, and `NSetcData` layout.           |
| [Sound package](formats/audio.md)                        | `snd.pck` containers and their entry lookup behavior.                     |
| [Patch packages](formats/patch-packages.md)              | `.PKG` packages, opcodes, and apply semantics.                            |

## Asset Formats

| Page                                              | Covers                                                                    |
| ------------------------------------------------- | ------------------------------------------------------------------------- |
| [Text](formats/text.md)                           | `.dat`, `.lst`, `.txt`, and raw text record payloads.                     |
| [CCINF `.NOS` files](formats/ccinf.md)            | `NSmnData.NOS` and `NSpnData.NOS` GBFC indexes and wrapper.               |
| [Textures](formats/textures.md)                   | Texture header and pixel formats.                                         |
| [Sprites](formats/sprites.md)                     | Map-object descriptor sprites and block-interlaced free-size sprites.     |
| [Geometry](formats/geometry.md)                   | `NStgData` and `NStgeData` geometry, animation, and render-node payloads. |
| [Effects](formats/effects.md)                     | `NSedData`, `NSeffData`, `NSemData`, and `NSesData` effect payloads.      |
| [Maps](formats/maps.md)                           | `NStuData` map settings, geometry references, and object trees.           |
| [Map cell flags](formats/map-cell-flags.md)       | Map cell flags.                                                           |
| [Map neighborhoods](formats/map-neighborhoods.md) | `NStkData` map neighbor system data.                                      |
| [Map height grids](formats/map-height-grids.md)   | `NSgrdData` optimized map height grid payloads.                           |
| [Videos](formats/videos.md)                       | `.ntm` and `.nam` intro/act video files.                                  |
| [Audio](formats/audio.md)                         | `snd.pck`, `sndinfo.lst`, and loose `BGM*` audio files in `wave`.         |
| TBD                                               | `NStsData`, `NSmcData`, `NSpcData`, and `NSpmData` payload families.      |
