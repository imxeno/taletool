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

Only flags observed in current client data have named API constants. The client
stores and copies the complete cell byte, but only WALKING_DISABLED and
MONSTER_AGGRO_DISABLED are checked by the client - other names were implied from
server behavior.

| Mask   | API name                  |
| ------ | ------------------------- |
| `0x01` | `WALKING_DISABLED`        |
| `0x02` | `ATTACK_THROUGH_DISABLED` |
| `0x04` | `UNKNOWN_04`              |
| `0x08` | `MONSTER_AGGRO_DISABLED`  |
| `0x10` | `PVP_DISABLED`            |
