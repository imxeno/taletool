# CCINF `.NOS` Archives

`NSmnData.NOS` and `NSpnData.NOS` are map-object GBFC index files. They use the
`.NOS` extension, but they are not the standard numeric-ID binary `.NOS` archive
layout used by other files.

The client loads both files with `TGBFCIndexList.Create` rather than
`TEWMultiFileStreamMemory`/`TEWMultiFileStreamSimple`. The loader opens the file
directly, skips a fixed `0x19` byte prefix, and then reads a compact variable
length table.

## Top-Level Layout

All integer fields are little-endian. The client does not validate the prefix
contents; it only seeks past it. Observed files have a `CCINF V1.20` signature
inside this prefix.

| Offset | Type       | Field                                        |
| ------ | ---------- | -------------------------------------------- |
| `0x00` | `u8[0x19]` | Prefix/header bytes.                         |
| `0x19` | `i32`      | Entry count.                                 |
| `0x1D` | variable   | Entry records, repeated `entry_count` times. |

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
