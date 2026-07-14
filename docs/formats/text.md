# Text

Text files are stored inside `NSgtdData`, `NSlangData`, `NScliData`, and
`NSetcData` archives. Their record payloads use a few simple text-oriented
formats.

| Archive                   | Contents                                                              |
| ------------------------- | --------------------------------------------------------------------- |
| `NSgtdData.NOS`           | Game data text records, such as item, quest, skill, and monster data. |
| `NSlangData_<locale>.NOS` | Localized language text records (`_code_<locale>_*.txt` files).       |
| `NScliData.NOS`           | Client const strings (`conststring.dat`).                             |
| `NScliData_<locale>.NOS`  | Localized client const string (`conststring.dat`).                    |
| `NSetcData.NOS`           | Typewriter mini-game and unused “taboo” word lists.                   |

| Payload kind     | Typical extension | Contents                                               |
| ---------------- | ----------------- | ------------------------------------------------------ |
| DAT compact text | `.dat`            | Line text compressed with a small control-byte format. |
| List text        | `.lst`            | Counted strings with a one-byte XOR transform.         |
| Plain text       | `.txt`            | Locale text stored as bytes.                           |

Plain `.txt` records do not declare their character encoding. The encoding
depends on the locale/archive the record came from, so localized files should be
decoded with the matching client locale in mind. For `_code_<locale>_*.txt`
records, the two-letter locale code in the record name identifies the expected
encoding.

Known locale encodings:

| Suffix | Locale    | Encoding     |
| ------ | --------- | ------------ |
| `CZ`   | Czech     | Windows-1250 |
| `DE`   | German    | Windows-1250 |
| `ES`   | Spanish   | Windows-1252 |
| `FR`   | French    | Windows-1252 |
| `IT`   | Italian   | Windows-1250 |
| `PL`   | Polish    | Windows-1250 |
| `RU`   | Russian   | Windows-1251 |
| `TR`   | Turkish   | Windows-1254 |
| `UK`   | English   | Windows-1252 |
| `HK`   | Hong Kong | Big5         |
| `TW`   | Taiwan    | Big5         |

Non-localized text records should be treated as EUC-KR (they may contain
comments in Korean)

## NScli Constant Strings

The `conststring.dat` record stored in `NScliData` is a compact-DAT sequence of
numeric key/value rows. It contains, well, const strings that are not related to
the game data like items or monsters, used pretty much everywhere in the game.
Each nonblank row has this form:

```text
<signed decimal key><vertical tab><text>
```

The separator is byte `0x0B`. The client splits at the first separator, parses
the prefix as a signed integer, and retains the complete suffix as text. Literal
`#13#10` sequences are expanded to CRLF before the row is split.

A value containing `<NEW_TYPE>` causes the client to sort the loaded list by its
integer keys after parsing. Without that marker, stored row order is retained.

## NSlang Language Tables

Records named `_code_<locale>_*.txt` in `NSlangData` are compact-DAT sequences
of string key/value rows. A normal row has this form:

```text
<key><tab><text>
```

The client ignores blank rows and rows whose first byte is `#`. It expands
literal `#13#10` sequences to CRLF, splits at the first tab, and retains any
additional tabs in the value. It then expands every `[n]` token in the value to
CRLF. A nonblank, non-comment row without a tab produces a row whose key and
value are both `0`.

## NSetc String Lists

`NSetcData.NOS` contains two ordered string lists:

| Record                  | Payload kind | Contents                                  |
| ----------------------- | ------------ | ----------------------------------------- |
| `MiniGame6WordData.dat` | DAT          | Typewriter mini-game words and phrases.   |
| `TabooStr.lst`          | LST          | An apparently unused blocked-string list. |

Each stored row is one logical string. JSON conversion exposes both records as a
plain ordered string array:

```json
["wolly", "sheep"]
```

Order, duplicate strings, and empty strings are preserved. A logical string
cannot contain a carriage return or line feed. These records are not localized;
structured conversion defaults to EUC-KR while permitting an explicit encoding
override.

## DAT Compact Text

DAT payloads are line-based. The byte `0xFF` ends a line. Other bytes describe
either packed characters from a small table or raw bytes XORed with `0x33`.

| Control byte   | Meaning                                                                                 |
| -------------- | --------------------------------------------------------------------------------------- |
| `0xFF`         | End of line.                                                                            |
| High bit set   | Packed run. The low seven bits give the number of packed characters.                    |
| High bit clear | Raw run. The low seven bits give the number of following bytes, each XORed with `0x33`. |

Packed runs store two 4-bit values per byte. The compact character table is:

```text
0, space, -, ., 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, newline, 0
```

## List Text

List payloads start with a line count. Each line then stores a byte length
followed by that many bytes. Line bytes are XORed with `0x01`.

| Field            | Type                    |
| ---------------- | ----------------------- |
| Line count       | `i32`                   |
| Line byte length | `i32`                   |
| Line bytes       | bytes XORed with `0x01` |
