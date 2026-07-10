# Text `.NOS` Archives

Text `.NOS` archives are used to store game data and localization strings.

## Known Families

Several unrelated NosTale containers use the `.NOS` extension. These are the
observed text archive families. Text archives are not chunked.

| Family                    | Record order    | Record IDs        | Packed flag convention          | Content                                 |
| ------------------------- | --------------- | ----------------- | ------------------------------- | --------------------------------------- |
| `NSgtdData.NOS`           | file name order | stored per record | `.dat` and `.txt` records use 1 | [Game data files](text.md)              |
| `NSlangData_<locale>.NOS` | file name order | stored per record | `.dat` and `.txt` records use 1 | [Language files](text.md)               |
| `NScliData.NOS`           | file name order | stored per record | `.dat` records use 1            | Client const strings                    |
| `NScliData_<locale>.NOS`  | file name order | stored per record | `.dat` records use 1            | Localized const strings                 |
| `NSetcData.NOS`           | file name order | stored per record | `.dat` records use 1            | Typewriter word list and `TabooStr.lst` |

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

## Timestamp Trailer

Observed text archives end with a 12-byte data-version trailer after the final
record payload. The client uses the `NSgtdData` and `NSlangData` values for the
displayed `GDataVer:` and `CDataVer:` strings when you type `$ver` or equivalent
in the chat.

| Offset from trailer start | Type  | Meaning                                    |
| ------------------------- | ----- | ------------------------------------------ |
| `0x00`                    | `f64` | Delphi `TDateTime` data-version timestamp. |
| `0x08`                    | `u32` | Marker value `$01323EEE`, little-endian.   |

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
