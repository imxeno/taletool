# Text `.NOS` Archives

Text `.NOS` archives are used to store game data and localization strings.

## Known Families

Several unrelated NosTale containers use the `.NOS` extension. These are the
observed text archive families. Text archives are not chunked.

| Family            | Record order    | Record IDs        | Packed flag convention          | Content                                 |
| ----------------- | --------------- | ----------------- | ------------------------------- | --------------------------------------- |
| `NSgtdData.NOS`   | file name order | stored per record | `.dat` and `.txt` records use 1 | [Game data files](nsgtd.md)             |
| `NSlangData*.NOS` | file name order | stored per record | `.dat` and `.txt` records use 1 | [Language files](text.md)               |
| `NScliData*.NOS`  | file name order | stored per record | `.dat` records use 1            | [Client const strings](text.md)         |
| `NSetcData.NOS`   | file name order | stored per record | `.dat` records use 1            | Typewriter word list and `TabooStr.lst` |

## Layout

All integer fields are little-endian.

| Field            | Type  | Meaning                            |
| ---------------- | ----- | ---------------------------------- |
| Record count     | `i32` | Number of records that follow.     |
| Record id        | `i32` | Stored record id.                  |
| Name byte length | `i32` | Byte length of the record name.    |
| Name bytes       | bytes | Stored record name bytes.          |
| Packed flag      | `i32` | Stored per-record packed flag.     |
| Payload length   | `i32` | Byte length of the record payload. |
| Payload bytes    | bytes | Stored record payload bytes.       |

Record IDs are part of each stored record. They should not be treated as a
guaranteed unique archive key.

The packed flag selects how the client reads a payload. A nonzero flag means
compact DAT rows (see [Text](text.md)); a zero flag means plain text, which the
client splits into rows at CR, LF, or CRLF and stops reading at the first NUL
byte. The `.lst` filter-list readers ignore the flag and read the stored bytes
as an LST payload.

When the client loads a whole archive, it processes every record in stored
order, including records that repeat a name. When it looks a record up by name,
it uses the first match.

## Timestamp Trailer

Observed text archives end with a 12-byte data-version trailer after the final
record payload. The client uses the `NSgtdData` and `NScliData` values for the
displayed `GDataVer:` and `CDataVer:` strings when you type `$ver` or equivalent
in the chat.

| Offset from trailer start | Type  | Meaning                                    |
| ------------------------- | ----- | ------------------------------------------ |
| `0x00`                    | `f64` | Delphi `TDateTime` data-version timestamp. |
| `0x08`                    | `u32` | Marker value `$01323EEE`, little-endian.   |

The client reads the trailer from the last 12 bytes of the file, so other bytes
may sit between the final record and the trailer.

The marker bytes at the end of the file are:

```text
EE 3E 32 01
```

Delphi `TDateTime` stores a floating-point day count from `1899-12-30`; the
fractional part is the time of day. It does not encode a timezone. For Unix-time
style conversion, treat it as:

```text
seconds = round((tdatetime - 25569.0) * 86400.0)
```

The client still handles a missing marker: when the trailer is absent, it uses
`2004-12-11 12:00:00` as the fallback data-version date.

At startup, the client also compares the dates with minimum versions stored as
`TDateTime` numbers in the const strings: key `0x0C87` for `NScliData` and key
`0x0C88` for `NSgtdData`. If either archive's date is older than its minimum,
the client shows the data-version mismatch notice and shuts down. An archive
without a trailer counts as `2004-12-11 12:00:00` in this check.
