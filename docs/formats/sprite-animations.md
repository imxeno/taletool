# Sprite Animations

Entries in `NSmcData.NOS` and `NSpcData.NOS` describe ordered sprite animation
sequences. Both archive families use the same payload layout. The containing
binary `.NOS` archive supplies the animation file id.

`NSmcData` animations are paired with map-object sprite resources, while
`NSpcData` animations are paired with player sprite resources. The corresponding
CCINF entry identifies the animation payload and the sprite resource keys used
by the sequence.

## Layout

Each payload begins with a two-byte header followed by one two-byte record per
animation frame:

| Offset | Type                     | Field                            |
| ------ | ------------------------ | -------------------------------- |
| `0x00` | `u8`                     | Animation frame count.           |
| `0x01` | `u8`                     | Playback flags.                  |
| `0x02` | `frame[animation_count]` | Ordered animation frame records. |

Each frame record has this layout:

| Offset from frame | Type | Field                                   |
| ----------------- | ---- | --------------------------------------- |
| `0x00`            | `u8` | Sprite frame index.                     |
| `0x01`            | `u8` | Event-timing flag; zero means unmarked. |

The sprite frame index selects the corresponding frame from the active layered
sprite resources. The event-timing byte is treated as a boolean flag: zero
leaves the frame unmarked, while any nonzero value marks its end as an event
time. All nonzero values have the same behavior. The marked time is
`(frame_index + 1) * 60` game ticks from the start of the animation. The JSON
representation exposes the raw byte as `event_timing_flag`.

## Playback

Each animation frame lasts 60 game ticks. Playback advances through the records
in order.

Playback flag bit `0x80` enables looping. When it is set, playback wraps to the
first record after the last frame. Otherwise playback remains on the final
record. Other playback flag bits have no known runtime meaning.

A zero frame count is representable and is treated as an unavailable animation
by rendering paths.
