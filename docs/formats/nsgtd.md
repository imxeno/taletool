# NSgtdData Record Formats

`NSgtdData.NOS` stores game-data tables and scripts as named records. The
records share an archive container and text codecs, but their row grammars are
otherwise independent.

## Record Inventory

| Record                 | Payload | Reader boundary or framing                        |
| ---------------------- | ------- | ------------------------------------------------- |
| `act_desc.dat`         | DAT     | Independent `Data`/`A` rows; observed `end`, `~`  |
| `BCard.dat`            | DAT     | Next `VNUM` or end of payload                     |
| `Card.dat`             | DAT     | Global indexed rows plus `VNUM`-started entries   |
| `Item.dat`             | DAT     | Next `VNUM` or end; `END` stops description scan  |
| `monster.dat`          | DAT     | Next `VNUM` or end of payload                     |
| `npctalk.dat`          | DAT     | `%` selects a key; `s` appends a state            |
| `Skill.dat`            | DAT     | Next `VNUM` or end; leading `#` ends descriptions |
| `quest.dat`            | DAT     | `BEGIN` starts the next entry                     |
| `qstprize.dat`         | DAT     | `BEGIN` starts the next entry                     |
| `tutorial.dat`         | DAT     | `script` starts the next entry; `end` is a no-op  |
| `shoptype.dat`         | DAT     | Every nonblank, non-comment row is data           |
| `MapIDData.dat`        | DAT     | Any non-`D` row starts the next entry             |
| `MapPointData.dat`     | DAT     | `S` sections; observed trailing `E` is a no-op    |
| `qstnpc.dat`           | DAT     | Independent discriminated bare rows               |
| `team.dat`             | DAT     | Next `VNUM` or end of payload                     |
| `fish.dat`             | DAT     | Next `VNUM` or end of payload                     |
| `<locale>_nosmall.dat` | DAT     | Next `VNUM` or end; `DSTART`/`DEND` detail region |
| `<locale>_abuse.lst`   | LST     | Counted strings or a zero-byte payload            |

## Common Conventions

DAT and LST storage are described in [Text](text.md). Fixed, non-localized DAT
records use EUC-KR. Localized records use the encoding associated with their
filename prefix.

The grammar examples below use these placeholders:

| Placeholder | Meaning                                              |
| ----------- | ---------------------------------------------------- |
| `<i32>`     | Signed 32-bit integer                                |
| `<text>`    | Text extending to the end of the physical source row |
| `...`       | A repeated row or field sequence                     |

Square brackets mark optional fields or rows; they are not literal source
characters.

Most readers split rows the same way. They trim each row, removing spaces and
control characters from both ends, then split off its first token at the first
tab, or at the first space when the row has no tab. Later tokens are split from
the trimmed rest of the row the same way, so a space before a tab stays inside a
token. Sections note readers that split rows differently.

An integer token may start with spaces and a sign, followed by a decimal number
or by a hexadecimal number after a `$`, `x`/`X`, or `0x`/`0X` prefix, so `-$1F`
reads as `-31`. A decimal number must fit in a signed 32-bit integer, while a
hexadecimal number may use all 32 bits. Conversion stops at a NUL character. A
missing or non-numeric token reads as a default value, usually `-1`. Observed
records write integers in signed decimal, and fields which permit negative
values frequently use them as sentinels. Formats which constrain declared counts
say so explicitly. Text such as `zts1e` is an opaque key; its apparent structure
does not change how it is stored.

Unless a format says otherwise, blank rows and rows whose first non-whitespace
character is `#` are ignored. Singleton tagged rows are positional source
fields: repeating one normally replaces the earlier loaded value. Rows marked as
repeated below retain their physical order and may contain duplicates.

There is no shared text terminator. The archive loader passes every decoded row
to the selected record reader. Tokens such as `~`, `END`, `end`, and `E` may be
ignored, interpreted as data, or control a local scan depending on that reader.

Several rows changed width across game versions. These docs describe behavior
and values observed between 2008 and 2026.

## `act_desc.dat`

Current records contain two sequential tables:

```text
Data <vnum> <act_vnum> <part> <max_ts>
...
end
A <act_vnum> <title>
...
~
```

Each `Data` row has exactly four integers. Each `A` row has an act number and a
title extending through the rest of the row. The 2008 record contains only the
`A` table and therefore has no separating `end`; it still ends in `~`. The
reader treats both markers as ignorable framing rows: it recognizes `Data` or
`A` rows on either side. Source order and duplicate rows are significant within
each table.

## `BCard.dat`

Each `BasicCardData` entry is stored on a keyword-prefixed row. The reader
identifies a row by that keyword rather than by its position in the entry. The
slot count `N` is the number of integers on `DESC`:

```text
VNUM <vnum>
ICON <icon>
NAME <text>
DESC <i32> ...                         # N values
SUBJ1 <text>
...
SUBJN <text>
LIST1-1 <text>
LIST1-2 <text>
...
LISTN-1 <text>
LISTN-2 <text>
END
```

Observed records use every `N` from one through five, but the source row accepts
any token count. One `DESC` value, one `SUBJ` row, and two `LIST` rows form each
slot.

The client stores five zero-initialized slots, consumes at most five `DESC`
values, and uses last-write semantics for repeated indexed text rows. Missing
text slots become empty strings. Consequently, a sixth `DESC` token is ignored
and a missing `SUBJ` or `LIST` row has an empty current-layout value. A new
`VNUM`, or end of payload, closes the preceding entry. `END` and the final `~`
are framing rows (not commit points).

## `Card.dat`

The file begins with indexed global text followed by card entries. Observed
files place an `END` row before the global tables.

```text
END
KIT <kit_index> <slot_index> <text>
...
Z_ETC <index> <text>
...

VNUM <vnum>
NAME <text>
GROUP <i32> <i32>
STYLE <i32> ...
EFFECT <i32> <i32> [<i32>]
TIME <i32> <i32>
1ST <i32> ...
2ST <i32> ...
LAST <i32> <i32>
DESC <text>
END
```

`KIT` addresses a 3-by-5 table: kit indices are 0 through 2 inclusive and slot
indices are 0 through 4 inclusive. `Z_ETC` addresses 20 independent text slots
numbered 0 through 19 inclusive. A card's `1ST` row contains 18 integers and
`2ST` contains 12. `STYLE` and `EFFECT` contain arbitrary numbers of integers;
the client consumes their first five and three values respectively into
zero-initialized slots.

`VNUM` starts a card entry. The next `VNUM`, or end of payload, leaves the
current entry loaded. The client does not commit on `END`: both the extra
initial `END` in the current file and the per-entry `END` rows have no effect.
The final `~` is likewise unrecognized and has no effect.

## `Item.dat`

```text
VNUM <vnum> <price>
NAME <text>
INDEX <i32> <i32> <i32> <i32> <i32> <i32>
TYPE <i32> <i32>
FLAG <i32> ...
DATA <20 integers>
BUFF <25 integers>
LINEDESC <declared_count>
[<description>]
END
```

`FLAG` contains an arbitrary number of integers; the client consumes the first
25 into zero-initialized slots. `BUFF` is physically one 25-integer row, viewed
as five groups of five. `LINEDESC` stores its declared source value
independently; it is not derived from the physical description row.

If the declaration is positive, the client consumes the next physical row and
then up to 100 additional rows, stopping at `END`, end of payload, or a
subsequent row beginning with `#` in column one. Blank rows are appended. A
non-positive declaration consumes no description row. Outside the description
scan, `END` and the final `~` have no effect. The next `VNUM`, or end of
payload, is the entry boundary.

## `monster.dat`

A `VNUM` row starts an entry. The next `VNUM`, or end of payload, closes it;
there is no `END` row.

```text
VNUM <vnum>
NAME <text>
LEVEL <1 integer>
RACE <i32> ...
ATTRIB <6 integers>
HP/MP <2 integers>
EXP <2 integers>
PREATT <5 integers>
SETTING <i32> ...
ETC <i32> ...
PETINFO <i32> ...
EFF <3 integers>
ZSKILL <7 integers>
WINFO <3 integers>
WEAPON <7 integers>
AINFO <2 integers>
ARMOR <5 integers>
SKILL <15 integers>
PARTNER <20 integers>
BASIC <50 integers>
CARD <20 integers>
MODE <i32> ...
ITEM <i32> ...
```

The wider rows contain fixed repeated groups: `SKILL` is five groups of three,
`BASIC` is ten groups of five, `CARD` is four groups of five, and `ITEM` is 20
groups of three. All tagged rows are required for a complete entry.

Those six rows accept arbitrary token counts. The client consumes prefixes of 3,
6, 8, 5, 32, and 60 tokens respectively. `ITEM` is interpreted in groups of
three; a source row may end with a partial group, and that partial group remains
part of the row. All other monster rows have one fixed width. A standalone final
`~` is ignored.

## `Skill.dat`

Like monsters, skills are delimited by the next `VNUM` or end of payload.

```text
VNUM <vnum>
NAME <text>
TYPE <6 integers>
COST <i32> ...
LEVEL <5 integers>
EFFECT <i32> ...
TARGET <5 integers>
DATA <15 integers>
BASIC <6 integers>
BASIC <6 integers>
BASIC <6 integers>
BASIC <6 integers>
BASIC <6 integers>
FCOMBO <16 integers>
CELL <i32> ...
Z_DESC <declared_count>
<description row>
...
<blank row>
```

`COST`, `EFFECT`, and `CELL` accept arbitrary token counts. The client consumes
prefixes of 33, 9, and 93 integers respectively into zero-initialized slots.
`BASIC` is a repeated physical row; audited records contain five rows per skill.
`Z_DESC` stores an independent declared count. A positive count causes the
client to consume the immediately following row and then up to 100 more rows,
stopping only at end of payload or a subsequent row whose first physical
character is `#`. Blank rows, `VNUM`, `END`, and `~` are description data while
that scan is active. In both audited layouts, a leading-`#` divider ultimately
ends every positive description; intervening blank rows become trailing line
breaks in the loaded text. A non-positive declaration consumes no following row.
A final `~` has no effect only when it reaches the outer tagged-row reader.

## `npctalk.dat`

The first physical row is a header and is skipped. Entries and states then use
single-character commands:

```text
<header>
% <vnum>
t <title>
s <state_vnum>
c <text>
b <text>
f <text>
...
```

The client splits each row once at its first space, so a single space must
separate the command from its text. A tab is part of the command token.

`%` updates the pending NPC key, while `s` appends a state using that key. The
`c`, `b`, and `f` rows are ordered commands belonging to the most recently
created state and may be freely interleaved. The client ignores `t`; the title
remains part of the source grammar used by other readers. There is no explicit
entry terminator. A malformed `%` sets the pending client key to `0` without
creating or closing a state. Following commands continue to modify the most
recent state, while a following `s` creates a state under key `0`.

## `quest.dat`

Observed quest entries are written between case-insensitive `BEGIN` and `END`
rows:

```text
BEGIN
VNUM <i32> ...
LEVEL <i32> ...
TITLE <text>
DESC <text>
TALK <4 integers>
TARGET <3 integers>
DATA <4 integers>
...
PRIZE <4 integers>
LINK <i32>
[O <i32> ...]
END
```

`VNUM` and `LEVEL` accept arbitrary numbers of integers. The client consumes
their first six and three values respectively; missing `VNUM` slots use `-1`.
`DATA` is ordered and repeatable. `O` is optional and has a variable number of
integer fields. All other rows occur once in a complete block. `VNUM` and
fixed-width numeric rows accept an inline `//` suffix after their values;
`LEVEL` and `O` are variable-width rows and do not use that suffix rule.

The client uses only `BEGIN` as an entry boundary. The next `BEGIN`, or end of
payload, leaves the current quest loaded. `END` and the final `~` have no
effect, and rows physically following `END` still modify the current quest until
another `BEGIN`.

## `qstprize.dat`

Observed quest-prize blocks use the same wrapper rows as quests but a different
body:

```text
BEGIN
VNUM <i32> <i32>
DATA <i32> <i32> <i32> <i32> <i32>
END
```

Both tagged rows are required. Their fixed-width numeric values may be followed
by an inline `//` comment. As with quests, `BEGIN` starts an entry while `END`
and `~` have no effect. The next `BEGIN`, or end of payload, leaves the current
prize entry loaded. Both audited records serialize an `END` after each body and
finish with `~`.

## `tutorial.dat`

```text
script <vnum>
<step> <text>
...
end

script <vnum>
...
end
[~]
```

Each `script` starts a tutorial entry. The next `script`, or end of payload,
leaves the current entry loaded. Command rows begin with a signed step number
and retain the remaining text. The client ignores tokens beginning with `END`,
so every observed lowercase `end` row is decorative and does not close an entry.
If a command's first token is not numeric, the client retains it with step `-1`.

The 2008-era file has no final `~`. In the current file, the final `~` is not a
terminator and is not ignored: it loads into the last script as a command with
step `-1`, zero-valued kind, and empty text.

## `shoptype.dat`

The reader attempts to parse every non-comment row as bare numeric data:

```text
<vnum> [<type> ...]
...
~
```

A reader row is created for every nonblank physical row not beginning with `#`.
It has one shop number followed by zero to six type values. The 2008 record has
no `~`; the current record ends with one. That `~` is neither a terminator nor
ignored: failed numeric conversion creates a shop record with vnum `-1` and no
type values. Valid rows physically following it would still be consumed.

## `MapIDData.dat`

```text
<min_map_vnum> <max_map_vnum> <map_point_vnum> <point_kind> [<name>]
DATA <i32> ...
DATA <i32> ...
...
```

Every nonblank, non-comment row whose first character is not `D` starts a
map-range entry. The reader takes four integers from it and keeps the rest of
the row, after the delimiter that follows the fourth field, as the name. The
name may contain spaces or be empty; it keeps any further leading whitespace,
but cannot end in whitespace because the row is trimmed. A missing or
non-numeric field reads as `-1`, so rows such as `~` or `data 5` also start
entries.

Any row whose first character is `D`, conventionally `DATA`, stores its first
integer in a single value of the most recent entry, and a later `D` row replaces
it. The reader ignores the remaining integers, uses `-1` for a missing or
non-numeric value, and discards a `D` row before the first entry. On entering a
map, the client checks this value for 1, 2, or 3 to choose a quest or NPC marker
mode. Every 2008 entry has no `DATA` row, while every current entry has one with
one value. The record has no global terminator.

## `MapPointData.dat`

```text
S <vnum>
D <kind> <x> <y> [<name>]
D <kind> <x> <y> [<name>]
...
S <vnum>
...
E
```

The reader dispatches on the first character of each trimmed row. `S` starts a
section and parses the whole rest of the row as its number, so `S 2 // comment`
reads as section `-1`. Each `D` appends a point to the most recent section: the
reader takes three integers and keeps the rest of the row as the point name,
which may contain spaces or be empty. A missing or non-numeric value reads as
`-1`. A section holds at most 200 points, and the reader ignores later `D` rows
in it. A `D` row before the first `S` row is invalid, because the reader has no
section to store it in. Rows starting with any other character are ignored.

The current record has one global trailing `E`, not one per section; the 2008
record has no `E`. The reader ignores `E` rather than stopping, so later `S` and
`D` rows are still consumed.

## `qstnpc.dat`

The second integer selects one of two complete bare-row shapes:

```text
<npc_vnum> 0 <i32> <i32> <i32> <i32>
<npc_vnum> 1 <quest_vnum> <unknown> <level>
...
~
```

Mode 0 has six integers in total, while mode 1 has five. Other mode values and
other row widths are not valid records. The observed final `~` parses with
default numeric values, but its default enable value is not `1`, so it appends
no NPC or quest row. A valid row physically following it is still accepted.

## `team.dat`

```text
VNUM <i32> <i32>
TITLE <text>
[DESC <text>]
TARGET <i32> <i32> <i32> <i32>
BUFF <i32> <i32> <i32> <i32>
```

The next `VNUM`, or end of payload, closes an entry. `DESC` is an optional
singular row; the other four tags are required. Fixed-width numeric rows accept
an inline `//` suffix after their values. There is no explicit entry or file
terminator.

## `fish.dat`

Fish data is a nested tagged stream:

```text
VNUM <vnum>
[LEVEL <i32> <i32>]
[MAPT <declared_map_count>]
MAP <map_slot> <map_vnum>
[POST <map_slot> <declared_position_count>]
POS <map_slot> <slot> <x> <y> <direction>
...
[ITEMT <declared_item_count>]
ITEM <slot> <vnum> [<weight>]
...
[BASICT <declared_basic_count>]
BASIC <slot> <vnum> [<weight>]
...
~
```

The reader trims each row's first token and compares it with its tags in any
letter case. It reads only `VNUM`, `LEVEL`, `MAPT`, `MAP`, `ITEMT`, and `ITEM`.
Each of these takes a fixed number of integers, two for `LEVEL`, `MAP`, and
`ITEM` and one for the others, and ignores any further tokens, such as a `//`
comment. A missing or non-numeric value reads as `-1`. Every other row,
including the trailing `~`, is ignored.

`VNUM` always starts an entry, whatever its value tokens hold, and the next
`VNUM` closes it. Rows before the first `VNUM` are discarded. `MAP` stores a map
in slot 0 to 2 and `ITEM` stores an item in slot 0 to 61; a later row for the
same slot replaces it. The reader does not check the slot, so a slot outside
those ranges overwrites another field of the entry, such as the item count for
item slot `-1` or the map count for item slot 62, or memory outside it. `MAPT`
and `ITEMT` set how many map and item slots the fish information window lists,
so a zero or negative count lists none. If `LEVEL` or a count row is absent, the
reader uses zero values.

The `ITEM` weight and the `POST`, `POS`, `BASICT`, and `BASIC` rows are source
data that the client never reads. In the source layout, `POST` gives a map slot
and its declared position count, and the `POS` rows that follow list that map's
positions. `BASICT` and `BASIC` list a second item table. `MAPT`, each `POST`
count, `ITEMT`, and `BASICT` are stored independently of the number of rows that
follow them, so a count may differ from its row count.

## `<locale>_nosmall.dat`

All localized NosMall DAT records use this block grammar:

```text
VNUM <vnum> <i32> <i32> <i32> <i32> <i32> <i32>
ITEM <6 integers>
ID <text>
TITLE1 <text>
TITLE2 <text>
COST <6 integers>
LINK <6 integers>
DSTART
<description row>
...
DEND
END
```

All tagged scalar rows are required. `VNUM` contains seven integers in total:
the entry number plus six opaque fields. `DSTART` and `DEND` delimit zero or
more physical description rows. A client detail scan stops at a trimmed `DEND`,
a row beginning with `#` in column one, end of payload, or its 20-row limit.
Consumed rows are trimmed and joined with line breaks. The current archive
contains 62 source regions with 21 to 25 physical rows, so this client uses only
their first 20 rows. `END` is an observed serialization row but is not an entry
boundary; the next `VNUM`, or end of payload, leaves the item loaded.

`ID`, `TITLE1`, and `TITLE2` retain the remainder of their physical rows rather
than tokenizing it into words. The locale prefix selects the text encoding; it
does not alter the row grammar.

## `<locale>_abuse.lst`

An abuse record has one of two physical states:

1. A zero-byte archive payload.
2. A counted LST payload, including a four-byte zero count for a counted empty
   list.

The counted form uses the standard LST layout:

| Field             | Type                    |
| ----------------- | ----------------------- |
| Entry count       | little-endian `i32`     |
| Entry byte length | little-endian `i32`     |
| Entry bytes       | bytes XORed with `0x01` |

The count is followed by one length-and-bytes pair per entry. Negative counts or
lengths, truncation, and bytes after the declared final entry are invalid.
Order, duplicates, and zero-length strings are significant.

A zero-byte payload and a counted empty list both load no strings, but they are
distinct storage states. That distinction matters to readers which inspect the
physical record before enumerating its strings.
