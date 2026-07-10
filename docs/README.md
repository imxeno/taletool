Taletool Docs
=============

These pages document NosTale file formats that taletool knows about.


Archive Formats
---------------

| Page                                                     | Covers                                                                    |
| -------------------------------------------------------- | ------------------------------------------------------------------------- |
| [Binary `.NOS` archives](formats/nos-binary-archives.md) | Standard `.NOS` container layout, split archives, filenames, compression. |
| [CCINF `.NOS` archives](formats/nos-ccinf-archives.md)   | `NSmnData.NOS` and `NSpnData.NOS` layout.                                 |
| [Text `.NOS` archives](formats/nos-text-archives.md)     | `NSgtdData`, `NSlangData`, `NScliData`, and `NSetcData` layout.           |
| [Sound package](formats/audio.md)                        | `snd.pck` containers.                                                     |
| [Patch packages](formats/patch-packages.md)              | `.PKG` packages, opcodes, and apply semantics.                            |


Asset Formats
-------------

| Page                                            | Covers                                                                           |
| ----------------------------------------------- | -------------------------------------------------------------------------------- |
| [Text](formats/text.md)                         | Extracted `.dat`, `.lst`, `.txt`, and raw text record payloads.                  |
| [Textures](formats/textures.md)                 | Texture header and pixel formats.                                                |
| [Videos](formats/videos.md)                     | `.ntm` and `.nam` intro/act video files.                                         |
| [Audio](formats/audio.md)                       | `snd.pck`, `sndinfo.lst`, and loose `BGM*` audio files in `wave`.                |
| TBD                                             | `NStgData` and `NStgeData` geometry cache payload families.                      |
| TBD                                             | `NSedData`, `NSeffData`, `NSemData`, and `NSesData` payload families.            |
| TBD                                             | `NStcData`, `NStuData`, and `NStkData` cache/resource metadata payload families. |
| TBD                                             | `NStsData`, `NSmcData`, `NSpcData`, and `NSpmData` payload families.             |
| [Map height grids](formats/map-height-grids.md) | `NSgrdData` optimized map height grid payloads.                                  |
| TBD                                             | `NSmpData`, `NSppData`, `NSipData`, and `NS4BbData` payload families.            |
| TBD                                             | `NSmnData` and `NSpnData` `CCINF V1.20` files.                                   |
