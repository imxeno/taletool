# Map Cell Flags

`NStcData.NOS` stores rectangular map cell-flag grids keyed by map id.

## Layout

Each uncompressed archive-entry payload consists of a four-byte header followed
by one byte per cell.

| Offset | Field  | Type   | Notes                   |
| ------ | ------ | ------ | ----------------------- |
| `0x00` | Width  | `i16`  | Positive cell count.    |
| `0x02` | Height | `i16`  | Positive cell count.    |
| `0x04` | Cells  | `u8[]` | Exactly width × height. |

Cells use row-major order:

```text
cell_index = y * width + x
```

## Cell Flags

The client stores and copies the complete cell byte. It checks `0x01` to block
walking and `0x08` to disable monster aggro. Other observed bits have no
identified client-side checks.

| Mask   | Client meaning          |
| ------ | ----------------------- |
| `0x01` | Walking disabled.       |
| `0x02` | Unknown.                |
| `0x04` | Unknown.                |
| `0x08` | Monster aggro disabled. |
| `0x10` | Unknown.                |
