# Sprite Animations

Entries in `NSmcData.NOS` and `NSpcData.NOS` describe ordered sprite animation
sequences. Both archive families use the same payload layout. The containing
binary `.NOS` archive supplies the animation file id.

`NSmcData` animations are paired with map-object sprite resources, while
`NSpcData` animations are paired with player and descriptor sprite resources.
The corresponding CCINF entry identifies the animation payload and the sprite
resource keys used by the sequence.

## Layout

Each payload begins with a two-byte header followed by one two-byte record per
animation frame:

| Offset | Type                     | Field                            |
| ------ | ------------------------ | -------------------------------- |
| `0x00` | `u8`                     | Animation frame count.           |
| `0x01` | `u8`                     | Playback flags.                  |
| `0x02` | `frame[animation_count]` | Ordered animation frame records. |

Each frame record has this layout:

| Offset from frame | Type | Field                                     |
| ----------------- | ---- | ----------------------------------------- |
| `0x00`            | `u8` | Sprite frame index.                       |
| `0x01`            | `u8` | Event marker; zero means no marked event. |

The sprite frame index selects the corresponding frame from the active layered
sprite resources. A nonzero event marker schedules an event at the end of that
animation frame. Marker values are retained as bytes; no distinct meanings for
individual nonzero values are known.

## Playback

Each animation frame lasts 60 game ticks. Playback advances through the records
in order.

Playback flag bit `0x80` enables looping. When it is set, playback wraps to the
first record after the last frame. Otherwise playback remains on the final
record. Other playback flag bits are retained but have no known runtime meaning.

A zero frame count is representable and is treated as an unavailable animation
by rendering paths.
