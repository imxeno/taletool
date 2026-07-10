Audio
=====

NosTale stores audio under `wave`. Most sound effects are packed in `snd.pck`
and described by `sndinfo.lst`. Background music may also appear as loose
`BGM*` files in the same directory.


`sndinfo.lst`
-------------

`sndinfo.lst` is a sound lookup table. It maps three logical keys to a sound id
and an associated filename.

The file starts with a little-endian record count:

| Offset | Field        | Type  |
| ------ | ------------ | ----- |
| `0x00` | Record count | `i32` |

Records follow immediately. Each record is `0x7C` bytes:

| Offset | Field    | Type                 | Notes                                                                             |
| ------ | -------- | -------------------- | --------------------------------------------------------------------------------- |
| `0x00` | Key 0    | `i32`                | Sound group.                                                                      |
| `0x04` | Key 1    | `i32`                | Group-specific lookup value.                                                      |
| `0x08` | Key 2    | `i32`                | Group-specific lookup value.                                                      |
| `0x0C` | Sound id | `i32`                | Runtime sound id. `-1` marks an empty or disabled row.                            |
| `0x10` | Unknown  | `i32`                | Varies by row; not used by the known key, sound-id, or filename resolution paths. |
| `0x14` | Filename | Delphi string `[50]` | One length byte followed by up to 50 filename bytes.                              |
| `0x47` | Unknown  | 53 bytes             | Always zero in modern client data; loaded but not used by the client.             |

The client can resolve a row in two directions:

| Lookup      | Behavior                                                                |
| ----------- | ----------------------------------------------------------------------- |
| By keys     | Find the row whose `(Key 0, Key 1, Key 2)` matches the requested tuple. |
| By sound id | Find a row with the matching sound id, then reuse that row's key tuple. |

Once a row is selected, its filename is resolved relative to the `wave`
directory. If the stored filename exists, it is used as-is. If it does not
exist, the client searches the stored filename for the literal `.wav`
substring. When found, it tries the portion before `.wav`. This is how rows
such as `BGM (1).30000.wav` resolve to the shipped loose file
`BGM (1).30000`.

Observed `Key 0` groups in the client:

| Key 0 | Meaning                                                 |
| ----- | ------------------------------------------------------- |
| `0`   | Character/actor sounds.                                 |
| `1`   | Gameplay and UI sounds.                                 |
| `2`   | Map object, monster, player, and effect-related sounds. |
| `3`   | Background music.                                       |
| `4`   | Ambient/environment sounds.                             |
| `5`   | Specialist/vehicle sounds.                              |

For BGM rows, `Key 0` is `3`, `Key 1` is the music slot, and `Key 2` is usually
`0`.

Example BGM rows:

| Key tuple     | Sound id | Stored filename       | Shipped loose filename |
| ------------- | -------- | --------------------- | ---------------------- |
| `(3, 0, 0)`   | `30000`  | `BGM (1).30000.wav`   | `BGM (1).30000`        |
| `(3, 104, 0)` | `30104`  | `BGM (104).30104.wav` | `BGM (104).30104`      |
| `(3, 99, -1)` | `30099`  | `dance.30099.wav`     | `dance.30099`          |

The file extension of loose files matches the sound id, but this is not a rule.


BGM Files
---------

Background music files use names such as `BGM (1).30000` or
`BGM (104).30104`. These files are ordinary MP3 or Ogg audio streams.

The loose filename is resolved through `sndinfo.lst`. The table may store a
`.wav` suffix, but the shipped loose BGM files usually omit it.


`snd.pck`
---------

`snd.pck` is a DelDX packed sound container used for many sound-effect files.
Entries are raw `.wav` payloads, although historical packs can contain
other extensions such as `.av`. In the checked 2011 pack, the `.av` entry still
contains a RIFF/WAVE payload.

The pack header is `0x1C` bytes:

| Offset       | Field          | Notes                                                                                                                                   |
| ------------ | -------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `0x00`       | Magic length   | Expected value is `0x10`.                                                                                                               |
| `0x01..0x10` | Magic text     | Expected value is `DelDX Pack File `.                                                                                                   |
| `0x11..0x13` | Reserved bytes | Unknown. Observed as `f0 fd 7f` in a 2011 pack and `00 00 00` in a 2026 pack. Client loader treats this area as padding and ignores it. |
| `0x14..0x17` | Version        | Little-endian `i32`; NosTale accepts versions up to `10`.                                                                               |
| `0x18..0x1B` | Entry count    | Little-endian `i32`.                                                                                                                    |

The entry table starts immediately after the header. Each row is `0x4C` bytes:

| Offset       | Field       | Notes                                                                  |
| ------------ | ----------- | ---------------------------------------------------------------------- |
| `0x00`       | Name length | `u8`; must be less than `0x44`, so names can use up to 67 bytes.       |
| `0x01..0x43` | Name bytes  | Length-prefixed entry filename bytes followed by unused padding.       |
| `0x44..0x47` | Data offset | Little-endian `u32`; absolute byte offset from the start of `snd.pck`. |
| `0x48..0x4B` | Data size   | Little-endian `u32`; number of payload bytes.                          |

The table size is `0x1C + entry_count * 0x4C`. Non-empty payloads should point
outside the table. Zero-size entries are possible and carry no payload bytes.

Entry names are byte strings, not paths. Older Korean names are stored as
Windows-949/EUC-KR bytes; ASCII names decode identically. Names commonly follow
this shape:

~~~~ text
<label>.<sound-id>.wav
~~~~

The decimal number between the first and second dot is the DelDX entry key used
by patch mutation records when copying existing entries. If that shape is not
present, updater-side lookup falls back to table-position behavior.
