Binary `.NOS` Archives
======================

Binary `.NOS` archives are the standard NosTale table/chunk container format.


Known Families
--------------

Several unrelated NosTale containers use the `.NOS` extension. These are the
observed binary archive families.

| Family           | Header       | Compression          | Split layout       | Content                                           |
| ---------------- | ------------ | -------------------- | ------------------ | ------------------------------------------------- |
| `NStgData*.NOS`  | `NT Data 06` | raw                  | low-byte, 4 files  | TBD                                               |
| `NStgeData*.NOS` | `NT Data 10` | raw                  | single file        | TBD                                               |
| `NStpData*.NOS`  | `NT Data 07` | raw                  | low-byte, 32 files | [Primary texture payloads](textures.md)           |
| `NStpeData*.NOS` | `NT Data 11` | raw                  | low-byte, 8 files  | [Effect texture payloads](textures.md)            |
| `NStpuData*.NOS` | `NT Data 12` | raw                  | low-byte, 4 files  | [UI/widget texture payloads](textures.md)         |
| `NSedData.NOS`   | `NT Data 20` | raw                  | single file        | TBD                                               |
| `NSeffData.NOS`  | `NT Data 23` | raw                  | single file        | TBD                                               |
| `NSemData.NOS`   | `NT Data 21` | raw                  | single file        | TBD                                               |
| `NSesData.NOS`   | `NT Data 22` | raw                  | single file        | TBD                                               |
| `NStcData*.NOS`  | `NT Data 05` | zlib 1.1.2 (level 9) | single file        | TBD                                               |
| `NStuData*.NOS`  | `NT Data 02` | zlib 1.1.2 (level 9) | single file        | TBD                                               |
| `NStkData*.NOS`  | `NT Data 03` | raw                  | single file        | TBD                                               |
| `NStsData.NOS`   | `NT Data 09` | raw                  | single file        | Unknown and unused map-related data               |
| `NSgrdData*.NOS` | `NT Data 26` | raw                  | `file_id & 7`      | [Optimized map height grids](map-height-grids.md) |
| `NSmcData.NOS`   | `NT Data 16` | raw                  | single file        | TBD                                               |
| `NSmpData*.NOS`  | `NT Data 17` | zlib 1.1.2 (level 1) | low-byte, 16 files | [Sprite payloads](sprites.md)             |
| `NSpcData.NOS`   | `NT Data 13` | raw                  | single file        | TBD                                               |
| `NSpmData.NOS`   | `NT Data 15` | raw                  | single file        | TBD                                               |
| `NSppData*.NOS`  | `NT Data 14` | zlib 1.1.2 (level 1) | low-byte, 32 files | [Sprite payloads](sprites.md)             |
| `NSipData*.NOS`  | `NT Data 24` | zlib 1.1.2 (level 1) | single file        | [Sprite payloads](sprites.md)             |
| `NS4BbData*.NOS` | `32GBS V1.0` | zlib 1.1.2 (level 9) | single file        | TBD                                               |

`*` means the archive family may appear as an older single archive name such as
`NStgData.NOS` or as a chunked name such as `NStgData00.NOS`.


Layout
------

All integer fields are little-endian.

| Offset | Field                                                                            |
| ------ | -------------------------------------------------------------------------------- |
| `0x00` | 16-byte archive header bytes. Known archive families use different fixed values. |
| `0x10` | `i32` entry count.                                                               |
| `0x14` | `u8` direct index byte.                                                          |
| `0x15` | Entry table starts. Each row is `i32 file_id`, `i32 data_offset`.                |

The table `file_id` is not necessarily a unique archive key. The table can
contain multiple entries with the same `file_id`, and same-id entries can appear
at very different table indexes. The table index/order is therefore meaningful
and separate from the table `file_id`.

Each `data_offset` points to a stored payload record:

| Offset from payload record | Field                                     |
| -------------------------- | ----------------------------------------- |
| `0x00`                     | `u32 record_tag`                          |
| `0x04`                     | `u32 unpacked_size`                       |
| `0x08`                     | `u32 stored_size`                         |
| `0x0C`                     | `u8 compression_flag` (`0` raw, `1` zlib) |
| `0x0D`                     | Stored payload bytes                      |

In known archives the `record_tag` is often a date-shaped hexadecimal value
such as `0x20030415` or `0x20051104` but it should be treated as opaque
per-record metadata, likely a tooling/exporter or data-format version tag
rather than a record timestamp. Some archive families mix multiple `record_tag`
values in the same container.

NosTale's generic multi-file stream helpers read this 13-byte header before
seeking to payload bytes, but no client code is currently known to branch
on `record_tag` itself. Format checks seen so far are payload-internal:
for example, `NSgrdData` payloads contain the tag value inside and the game
logic branches on it. Read [Map Height Grids docs](./map-height-grids.md)
to learn more.


Compression
-----------

Raw entries are stored unchanged with `stored_size` = `unpacked_size`.
Compressed entries are zlib streams with `compression_flag = 1`. The archive
does not encode the compression level; it differs by archive family. Known
compressed families use level 1 or level 9, as listed in the family table. The
original Delphi 7 tooling used zlib 1.1.2.


Split Archives
--------------

Some archive families are split into several files. Low-byte split families
route entries by the low byte of the table `file_id`; single-file families keep
all entries in one archive. `NSgrdData*.NOS` is a special case, it's split with
`file_id & 7`.
