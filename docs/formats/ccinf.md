# CCINF `.NOS` Files

`NSmnData.NOS` and `NSpnData.NOS` are structured map-object GBFC index assets.
`NSmnData.NOS` indexes the monster/NPC resource family, while `NSpnData.NOS`
indexes the player resource family. The player renderer can explicitly borrow
the NSm family for an icon representation. Both files use the `.NOS` extension,
but they are not multi-entry containers and do not use the numeric-ID binary
`.NOS` archive layout.

The client loads both files with `TGBFCIndexList.Create` rather than
`TEWMultiFileStreamMemory`/`TEWMultiFileStreamSimple`. The file starts with a
single-payload wrapper followed by a compact variable-length table.

## Top-Level Layout

All integer fields are little-endian. The two size fields cover the body from
the entry count at `0x19` through the final cell. Known files store this body
raw, so both sizes equal `file_size - 0x19` and the compression flag is zero.

| Offset | Type     | Field                                        |
| ------ | -------- | -------------------------------------------- |
| `0x00` | `u8[16]` | CCINF header bytes.                          |
| `0x10` | `u32`    | Unpacked body size.                          |
| `0x14` | `u32`    | Stored body size.                            |
| `0x18` | `u8`     | Compression flag.                            |
| `0x19` | `i32`    | Entry count.                                 |
| `0x1D` | variable | Entry records, repeated `entry_count` times. |

The 16-byte header is:

```text
43 43 49 4E 46 20 56 31 2E 32 30 1A 14 11 04 20
```

The wrapper fields mirror the binary `.NOS` container format. The client loader
seeks directly to `0x19`, skipping all 25 wrapper bytes without interpreting
them. Consequently, compressed CCINF bodies are incompatible with the client
even though the wrapper retains the standard unpacked-size, stored-size, and
compression fields.

Entries are read sequentially.

## Entry Layout

Each entry begins with four dwords and then stores seven counted cell lists.

| Offset from entry start | Type     | Field                                      |
| ----------------------- | -------- | ------------------------------------------ |
| `0x00`                  | `i32`    | Entry id / lookup key.                     |
| `0x04`                  | `i32`    | Base resource key.                         |
| `0x08`                  | `i32`    | Remap table file id.                       |
| `0x0C`                  | `i32`    | Animation file id.                         |
| `0x10`                  | variable | Seven cell lists, for list indexes `1..7`. |

The client binary-searches entries by `entry id` and does not sort after
loading, so files are expected to store entries in ascending unsigned `entry id`
order.

## Cell List Layout

Each of the seven lists has this layout:

| Offset | Type               | Field                |
| ------ | ------------------ | -------------------- |
| `0x00` | `u8`               | Cell count.          |
| `0x01` | `cell[Cell count]` | Packed 6-byte cells. |

The raw client type is equivalent to:

```text
struct RawCell {
    i32 value;
    u16 tile;
}
```

The consumers treat the same six bytes as:

```text
struct Cell {
    u16 selector;
    i32 texture_resource_key; // unaligned, starts at byte 2
}
```

`selector` is the low word of `value`; `texture_resource_key` is read with an
unaligned dword load from `cell + 2`. Cell lists are binary-searched by
`selector`, so each list is expected to be sorted by ascending selector.

The nonnegative base and cell resource keys select individual
[`NSmpData` or `NSppData` sprite payloads](sprites.md). Which sprite family
supplies a key depends on the texture cache attached to the rendered map object;
the CCINF file does not encode that distinction.

The animation file id selects a
[`NSmcData` or `NSpcData` sprite-animation payload](sprite-animations.md).
`NSmnData` entries use `NSmcData`, while `NSpnData` entries use `NSpcData`.
