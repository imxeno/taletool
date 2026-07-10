Text
====

Text files are stored inside `NSgtdData`, `NSlangData`, `NScliData`, and
`NSetcData` archives.
After a file is extracted from the archive, its payload is one of a
few simple text-oriented formats.

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
decoded with the matching client locale in mind.
For `_code_<locale>_*.txt` records, the two-letter locale code in the record
name identifies the expected encoding.

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


DAT Compact Text
----------------

DAT payloads are line-based. The byte `0xFF` ends a line. Other bytes describe
either packed characters from a small table or raw bytes XORed with `0x33`.

| Control byte   | Meaning                                                                                 |
| -------------- | --------------------------------------------------------------------------------------- |
| `0xFF`         | End of line.                                                                            |
| High bit set   | Packed run. The low seven bits give the number of packed characters.                    |
| High bit clear | Raw run. The low seven bits give the number of following bytes, each XORed with `0x33`. |

Packed runs store two 4-bit values per byte. The compact character table is:

~~~~ text
0, space, -, ., 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, newline, 0
~~~~


List Text
---------

List payloads start with a line count. Each line then stores a byte length
followed by that many bytes. Line bytes are XORed with `0x01`.

| Field            | Type                    |
| ---------------- | ----------------------- |
| Line count       | `i32`                   |
| Line byte length | `i32`                   |
| Line bytes       | bytes XORed with `0x01` |
