Patch Packages
==============

Original NosTale patch packages distributed as `.PKG` files. All integer fields
described below are little-endian.


Package Container
-----------------

Each package is a sequence of independently encoded operation segments.

| Offset |        Size | Field                 | Notes                                                                                                                    |
| ------ | ----------: | --------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `0x00` |          12 | Magic                 | ASCII `PCHPKG DATA` followed by `0x1A`.                                                                                  |
| `0x0C` |           4 | Package datetime code | DOS-style packed date/time with a `2000` year base. See below.                                                           |
| `0x10` |           4 | Segment count         | `u32`; must be non-zero.                                                                                                 |
| `0x14` |           1 | Segment lookup flag   | Non-zero means use the requested segment id directly as a table index. Zero means binary-search the table by segment id. |
| `0x15` | `8 * count` | Segment table         | One record per segment.                                                                                                  |

Package datetime code:

| Bits     | Field      | Notes                       |
| -------- | ---------- | --------------------------- |
| `31..25` | Year       | Stored as `year - 2000`.    |
| `24..21` | Month      | `1..12`.                    |
| `20..16` | Day        | `1..31`.                    |
| `15..11` | Hour       | `0..23`.                    |
| `10..5`  | Minute     | `0..59`.                    |
| `4..0`   | Second / 2 | Stored in two-second units. |

Segment table records:

| Offset from record | Size | Field               |
| ------------------ | ---: | ------------------- |
| `0x00`             |    4 | Segment id          |
| `0x04`             |    4 | Segment file offset |

The updater applies requested segment ids in numeric order from `0` to
`count - 1`. In all known packages, the lookup flag is `1` and segment
ids match table indexes, so this is also table order. For lookup flag `0`,
the table must be ordered by segment id because the updater uses a binary
search.


Segment Encoding
----------------

Each segment starts at the offset listed in the package table:

| Offset |         Size | Field                 | Notes                                                                                                                                                                                            |
| ------ | -----------: | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `0x00` |            4 | Segment datetime code | Same packed datetime layout as the package datetime code. All locally scanned segment datetime codes match their package datetime code. The updater apply path appears to skip over these bytes. |
| `0x04` |            4 | Decoded body size     | Size after decompression.                                                                                                                                                                        |
| `0x08` |            4 | Encoded body size     | Stored byte count following this header.                                                                                                                                                         |
| `0x0C` |            1 | Compression flag      | `0` raw, `1` zlib.                                                                                                                                                                               |
| `0x0D` | encoded size | Encoded body          | Raw operation bytes or zlib stream.                                                                                                                                                              |

For raw segments, decoded size must equal encoded size. For zlib segments, the
decompressed byte count must match the one in the header.


Operation Body
--------------

The decoded segment body is one operation:

| Offset     |            Size | Field                   | Notes                                                                                                             |
| ---------- | --------------: | ----------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `0x00`     |               1 | Opcode                  | See opcode table below.                                                                                           |
| `0x01`     |               1 | Target path byte length | Maximum encoded path length is 255 bytes.                                                                         |
| `0x02`     |     path length | Target path             | Byte string. ASCII-only in most observed operations; some old music packages use Korean EUC-KR/Windows-949 bytes. |
| after path | remaining bytes | Payload                 | Opcode-specific.                                                                                                  |

Target paths are encoded as EUC-KR/Windows-949. ASCII-only paths decode
identically under that code page. In observed packages, stored paths use Windows
separators and begin with `$(INSTALLED)\`.


Opcodes
-------

| Opcode | Operation             | Payload                      | Updater behavior                                                                                                                     |
| -----: | --------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
|    `0` | Delete file           | Empty                        | Remove the target path.                                                                                                              |
|    `1` | Replace file          | Complete replacement bytes   | Write payload bytes to the target path.                                                                                              |
|    `2` | Binary delta          | Binary delta stream          | Load the current target file, apply the delta, and write the reconstructed bytes.                                                    |
|    `3` | Replace and relaunch  | Complete replacement bytes   | Write payload bytes to the target path and relaunch the updater. This is used to update the updater exe.                             |
|    `4` | `.NOS` archive update | `.NOS` archive update stream | Load the target binary `.NOS` archive, apply record-level archive updates, and write the rebuilt archive.                            |
|    `5` | DelDX pack mutation   | DelDX pack mutation stream   | Load the target DelDX pack such as `snd.pck`, apply record-level pack updates, and write the rebuilt file.                           |
|    `6` | Replace and run       | Complete replacement bytes   | Write payload bytes to the target path and run the written file. Historically this was used only for `NostaleData\ExtractUIEff.dat`. |


Opcode 2 Binary Delta
---------------------

Opcode `2` payloads rebuild a target file from the current source bytes.

The delta is a stream of chunks. Source chunks are consumed from the base file
in order.

Chunk prefix:

| Field               | Type  | Notes                                                       |
| ------------------- | ----- | ----------------------------------------------------------- |
| Source chunk length | `u32` | Number of bytes consumed from the base file for this chunk. |
| Delta tag           | `u8`  | `0` literal or terminator, `1` patched chunk.               |

Tag `0` literal/terminator:

| Field          | Type  | Notes                                         |
| -------------- | ----- | --------------------------------------------- |
| Literal length | `u32` | If zero, this terminates the delta stream.    |
| Literal CRC32  | `u32` | Present only when literal length is non-zero. |
| Literal bytes  | bytes | Appended directly to output.                  |

The zero-length terminator must be the final bytes in the delta stream.

Tag `1` patched chunk:

| Field                  | Type    | Notes                                         |
| ---------------------- | ------- | --------------------------------------------- |
| Patched chunk CRC32    | `u32`   | Checked after reconstruction.                 |
| Source chunk CRC32     | `u32`   | Checked before reconstruction.                |
| Literal section length | `u32`   | Bytes used to fill gaps between copy records. |
| Table section length   | `u32`   | Must be divisible by 12.                      |
| Literal section        | bytes   | Gap bytes.                                    |
| Copy table             | records | Rebuild instructions.                         |

Copy table records are 12 bytes:

| Offset | Type  | Field            | Notes                                            |
| ------ | ----- | ---------------- | ------------------------------------------------ |
| `0x00` | `u16` | Copy length      | Zero is allowed.                                 |
| `0x02` | `u16` | Unknown/reserved | No identified role in the observed updater path. |
| `0x04` | `u32` | Source position  | 1-based position inside the source chunk.        |
| `0x08` | `u32` | Target position  | 1-based position in the patched chunk.           |

The rebuilt chunk starts at target position `1`. Literal bytes fill any gap
before the next target position, then copy bytes are taken from the source
chunk.


Opcode 4 Binary `.NOS` Archive Updates
--------------------------------------

Opcode `4` payloads are patch instructions for binary table/chunk `.NOS`
archives. The payload begins with the output archive header, followed by update
records:

| Offset |     Size | Field                 | Notes                                                                                                        |
| ------ | -------: | --------------------- | ------------------------------------------------------------------------------------------------------------ |
| `0x00` |   `0x15` | Output archive header | Same 21-byte header shape as binary `.NOS` archives. The count at `0x10` is the expected output entry count. |
| `0x15` |        4 | Update record count   | `i32`.                                                                                                       |
| `0x19` | variable | Update records        | See below.                                                                                                   |

Every update record starts with:

| Field  | Type  | Meaning                                  |
| ------ | ----- | ---------------------------------------- |
| Tag    | `u8`  | Record action.                           |
| First  | `i32` | Usually target file id.                  |
| Second | `i32` | Size or source file id depending on tag. |

Record tags:

| Tag | Fields after common header | Output? | Meaning                                                                        |
| --: | -------------------------- | ------- | ------------------------------------------------------------------------------ |
| `0` | `source_index: i32`        | No      | Skip/no-output record.                                                         |
| `1` | inline entry bytes         | Yes     | Insert inline raw archive entry as `target_id = first`; `second` is byte size. |
| `2` | inline entry bytes         | Yes     | Replace-style inline entry; advances the source cursor before writing.         |
| `3` | `source_index: i32`        | Yes     | Copy an entry from the base archive to `target_id = first`.                    |
| `4` | `source_index: i32`        | Yes     | Copy with source-cursor advancement before writing.                            |
| `5` | `source_index: i32`        | Yes     | Copy with source-cursor advancement before writing.                            |

Inline entry bytes are the complete stored binary `.NOS` record: 13-byte record
header plus stored payload bytes.

Copy records refer to entries already present in the base archive. Observed
payloads use `second` as the source file id, and some records also rely on the
stored source index or source cursor position. The reconstructed archive must
contain exactly the output count declared in the update header.

Some archive update packages target split archive names, for example
`NStpData08.NOS`, while using records from the unsuffixed parent archive such
as `NStpData.NOS`. This was historically used to split files into chunks.


Opcode 5 DelDX Pack Mutations
-----------------------------

Opcode `5` payloads update DelDX packs such as `NostaleData/wave/snd.pck`.

Payload prefix:

| Offset |     Size | Field                 | Notes                                                       |
| ------ | -------: | --------------------- | ----------------------------------------------------------- |
| `0x00` |   `0x1C` | DelDX header          | Header count at `0x18` is the expected output record count. |
| `0x1C` |        4 | Mutation record count | `i32`.                                                      |
| `0x20` | variable | Mutation records      | See below.                                                  |

Mutation record tags:

|   Tag | Payload                                                   | Output? | Meaning                      |
| ----: | --------------------------------------------------------- | ------- | ---------------------------- |
|   `0` | `target_key: i32`, `source_key: i32`, `source_index: i32` | No      | Skip/no-output record.       |
|   `1` | 0x50-byte inline row, payload bytes                       | Yes     | Inline output record.        |
|   `2` | 0x50-byte inline row, payload bytes                       | Yes     | Inline output record.        |
|   `5` | `target_key: i32`, `source_key: i32`, `source_index: i32` | Yes     | Copy an existing pack entry. |
| other | none                                                      | No      | One-byte no-output marker.   |

The 2018 updater from `uk/99990476.PKG` dispatches all tag values other than
`0`, `1`, `2`, and `5` as one-byte records that read no extra fields and
produce no output. The local mirror scan found opcode `5` payloads using tags
`0`, `1`, `2`, and `5`.

Inline rows contain the normal DelDX row prefix at `0x00..0x43`, data offset at
`0x44`, and payload size at `0x48`. The mutation payload stores the actual
payload bytes immediately after the inline row; the row's payload size
determines how many bytes follow.

Copy records include both `target_key` and `source_key`; observed mutation
behavior also depends on the expected sorted index. If the referenced entry is
missing, the output contains an empty placeholder record.


Opcode 6 Replace And Run
------------------------

Opcode `6` writes the payload to the target path and runs the written file.
Observed packages use it with a helper executable named
`NostaleData/ExtractUIEff.dat`; the old patch flow used that helper to split
UI/effect records out of broad binary `.NOS` archives.

Observed helper SHA1:

~~~~ text
8db83a801a27308d6306121a556918cd223c7752
~~~~

The observed helper performs these archive splits:

| Source archive             | Extracted id range       | Output archive              |
| -------------------------- | ------------------------ | --------------------------- |
| `NostaleData/NStgData.NOS` | `0x4F000000..0x4FFFFFFF` | `NostaleData/NStgeData.NOS` |
| `NostaleData/NStpData.NOS` | `0x4F000000..0x4FFFFFFF` | `NostaleData/NStpeData.NOS` |
| `NostaleData/NStpData.NOS` | `0x5F000000..0x5FFFFFFF` | `NostaleData/NStpuData.NOS` |

The source archives are rewritten without the extracted records.
