# NSgtdData Record Formats

`NSgtdData.NOS` stores game-data tables and scripts as named records. The
records share an archive container and text codecs, but their row grammars are
otherwise independent.

## Record Inventory

| Record                 | Payload | Reader boundary or framing                        |
| ---------------------- | ------- | ------------------------------------------------- |
| `act_desc.dat`         | DAT     | Independent `Data`/`A` rows; observed `end`, `~`  |
| `BCard.dat`            | DAT     | Next `V` row or end of payload                    |
| `Card.dat`             | DAT     | Global indexed rows plus `V`-started entries      |
| `Item.dat`             | DAT     | Next `V` row or end; `END` stops description scan |
| `monster.dat`          | DAT     | Next `V` row or end of payload                    |
| `npctalk.dat`          | DAT     | `%` selects a key; `s` appends a state            |
| `Skill.dat`            | DAT     | Next `V` row or end; leading `#` ends description |
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

## Entity Records

`BCard.dat`, `Card.dat`, `Item.dat`, `monster.dat`, and `Skill.dat` share one
reader shape. The client splits each row into tokens as described above, and the
first token is the row's tag. Because the tag ends at the first tab whenever the
row has one, a row such as `DESC x<TAB>sp  y` has the tag `DESC x` and the text
`sp  y`. A text field is the trimmed rest of the row; its inner spaces and tabs
are kept. A text row replaces the field's earlier text, but unless a section
says otherwise, a row with empty text leaves the earlier text in place.

Most rows are selected by the first character of their tag; the sections below
list the rows that need an exact tag. Every row whose tag begins with `V` starts
a new entry, even when its values are missing or malformed, and the following
rows belong to that entry. The next `V` row, or end of payload, closes it. Rows
before the first `V` row fill a placeholder record that the client discards.
Rows whose tag selects nothing, such as `END` and `~` in most of these records,
have no effect.

Every row is optional. A new entry's fields are zero unless a section says
otherwise, and an absent row leaves them unchanged. A present numeric row
assigns every position the client reads. Its values are integer tokens as
described above; a missing or non-numeric token takes that position's default.
The default is -1 unless a section says otherwise, and the value is truncated to
the width of its field. Tokens after the positions the client reads are ignored.

## `BCard.dat`

```text
VNUM <vnum>
ICON <icon>
NAME <text>
DESC <i32> ...
SUBJ<n> <text>
...
LIST<k>-<m> <text>
...
END
```

Every row is selected by the first character of its tag. The client reads one
value of `VNUM` and `ICON`. Each entry has five slots numbered 0 through 4,
whatever the number of `DESC` values:

- `DESC` sets the value formats of slots 0 through 4 from its first five values
  and ignores the rest. Unlike other numeric rows, it assigns only the values
  present, and a non-numeric value reads as 0, so a repeated `DESC` row replaces
  only as many formats as it has values.
- `SUBJ<n>` stores the subject text of slot `n`, for `n` from 0 through 4.
- `LIST<k>-<m>` stores template `m` of slot `k - 1`, for `k` from 1 through 5
  and `m` 1 or 2.

The `SUBJ` index is the part of the tag after its fourth byte. The `LIST`
indexes are the parts between the fourth byte and the first `-`, and after that
`-`. Each index is read as an integer token, so the row `SUBJ 0<TAB>text`, whose
tag is `SUBJ 0`, fills slot 0. The second through fourth bytes are not checked,
so a tag holding a multibyte character can still select a slot, and a row whose
index is missing, non-numeric, or outside these ranges has no effect. Observed
records number subjects `SUBJ1` through `SUBJ5`, so slot 0 has no subject and
`SUBJ5` has no effect. Observed `DESC` rows have one through six values.

An equipment option of slot `i` is displayed with template 1 of that slot for a
non-negative option value and template 2 for a negative one. The slot's format
selects how the value is inserted into the template; format 0 shows the template
unchanged.

Text fields are the trimmed rest of their row. An empty `SUBJ` or `LIST` row
keeps its slot's earlier text, so a slot without a non-empty row has empty text.
An entry without a non-empty `NAME` row has an empty name. A `NAME` row frees
the earlier name before reallocating that buffer for its own text, so a repeated
`NAME` row after a non-empty name is unreliable. `END` and the final `~` have no
effect.

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
[ICON <icon>]
TIME <i32> <i32>
1ST <i32> ...
2ST <i32> ...
LAST <i32> <i32>
DESC <text>
END
```

`KIT` addresses a 3-by-5 table: kit indices are 0 through 2 inclusive and slot
indices are 0 through 4 inclusive. `Z_ETC` addresses 20 independent text slots
numbered 0 through 19 inclusive. Their indices default to 0, and a row outside
the table has no effect. A `KIT` or `Z_ETC` row without text clears its slot.
These global rows may appear anywhere in the file.

`EFFECT` is the only row that needs its exact tag; every other row is selected
by its first character. The client reads 2 values of `GROUP`, `TIME`, and
`LAST`, 5 of `STYLE`, 3 of `EFFECT`, 18 of `1ST`, and 12 of `2ST`. Observed
`STYLE` rows hold five values and `EFFECT` rows two or three. The second
`EFFECT` value and the `ICON` value both set the card's icon, so whichever of
the two rows comes later wins.

The client does not commit on `END`: both the extra initial `END` in the current
file and the per-entry `END` rows have no effect. The final `~` is likewise
unrecognized and has no effect.

## `Item.dat`

```text
VNUM <vnum> <price>
NAME <text>
INDEX <i32> <i32> <i32> <i32> <i32> <i32>
TYPE <i32> <i32>
FLAG <i32> ...
DATA <20 integers>
BUFF <25 integers>
LINEDESC <declared_count> [<text>]
[<description>]
...
END
```

Every row is selected by its first character. A missing price reads as 0. The
client reads 6 values of `INDEX`, 2 of `TYPE`, and 20 of `DATA`. The first
`INDEX` value is the item type, kept in 16 bits; types 8, 9, and 10 read as 0,
1, and 2. Each `INDEX` row whose type is then 0 through 3 adds the item to that
type's list, so a repeated row adds it again. The client reads 25 `FLAG` values:
the first defaults to -1, and the other 24 are flags that default to 0. Each
`FLAG` row with a non-zero 23rd value appends the signed-item label to the name
loaded so far; a later `NAME` row with text replaces the labeled name. `BUFF` is
physically one 25-integer row, viewed as five groups of five; the client reads
the first four values of each group and skips the fifth.

The `LINEDESC` count is kept in a 16-bit word, and the client reads description
rows only when its signed 16-bit value is positive. For example, `65536` reads
as 0, `32768` through `65535` read as negative, and `-65535` reads as 1.

A positive count makes the client consume the next physical row as the first
description line, then up to 100 additional rows, stopping at `END`, end of
payload, or a row beginning with `#` in column one. The `END` row is consumed;
the `#` row is not. Rows are trimmed, and blank rows are appended. Text after a
positive count on the `LINEDESC` row is ignored. If the first consumed row is
`END`, the description starts empty and the additional scan reuses the limit of
an earlier description: none before any positive description whose first row was
not `END`, and 100 afterwards. Without a following `#` row, that scan can
consume the next item's rows.

A non-positive count consumes no row. The description is the text after the
count on the `LINEDESC` row itself; its leading whitespace is kept. An empty
description clears the item's description and resets the count to 0. Outside the
description scan, `END` and the final `~` have no effect.

## `monster.dat`

A `V` row starts an entry. The next `V` row, or end of payload, closes it; there
is no `END` row.

```text
VNUM <vnum>
NAME <text>
LEVEL <1 integer>
RACE <3 integers>
ATTRIB <6 integers>
HP/MP <2 integers>
EXP <2 integers>
PREATT <5 integers>
SETTING <6 integers>
ETC <8 integers>
PETINFO <5 integers>
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
MODE <32 integers>
ITEM <60 integers>
```

The widths above are those of observed records. The wider rows contain repeated
groups: `SKILL` is five groups of three, `BASIC` is ten groups of five, `CARD`
is four groups of five, and `ITEM` is 20 groups of three. The client reads the
rows as follows:

| Row       | Selected by | Values read                                     |
| --------- | ----------- | ----------------------------------------------- |
| `LEVEL`   | `L`         | 1                                               |
| `RACE`    | `R`         | 2                                               |
| `ATTRIB`  | Exact tag   | 1                                               |
| `HP/MP`   | `H`         | 2                                               |
| `EXP`     | Exact tag   | 2                                               |
| `PREATT`  | Exact tag   | 3                                               |
| `SETTING` | Exact tag   | 5; the fourth defaults to 1 and the fifth to 0  |
| `ETC`     | Exact tag   | 2 integers, then 4 booleans defaulting to false |
| `PETINFO` | Exact tag   | None                                            |
| `EFF`     | Never read  | None                                            |
| `ZSKILL`  | `Z`         | 3 after skipping 2; defaults are 0              |
| `WINFO`   | Exact tag   | 3; the third defaults to 0                      |
| `WEAPON`  | Exact tag   | 7                                               |
| `AINFO`   | Exact tag   | 2; the second defaults to 0                     |
| `ARMOR`   | Exact tag   | 5                                               |
| `SKILL`   | Exact tag   | 15                                              |
| `PARTNER` | Never read  | None                                            |
| `BASIC`   | `B`         | 50                                              |
| `CARD`    | `C`         | 20                                              |
| `MODE`    | `M`         | The 31st value only                             |
| `ITEM`    | Never read  | None                                            |

A present `ETC` row also sets another field of the monster to 600, and a present
`AINFO` row sets two armor fields to 1; without these rows those fields stay 0.

An `ETC` boolean is true when its token is a non-zero number or `True`, and
false when it is zero, `False`, or other text, in any letter case. Such a number
may be surrounded by spaces and is decimal, with an optional sign, `.` fraction,
and `E` exponent, so `0.5` reads as true and `$1` as false.

Editing `EFF`, `PARTNER`, `ITEM`, or `PETINFO` therefore has no effect in the
client. `NAME` is selected by `N`; the client displays `^` in a name as a space.
A standalone final `~` is ignored.

## `Skill.dat`

Like monsters, skills are delimited by the next `V` row or end of payload.

```text
VNUM <vnum>
NAME <text>
TYPE <6 integers>
COST <33 integers>
LEVEL <5 integers>
EFFECT <9 integers>
TARGET <5 integers>
DATA <15 integers>
BASIC <slot> <i32> <i32> <i32> <i32> [<i32>]
...
FCOMBO <16 integers>
CELL <93 integers>
Z_DESC <declared_count> [<text>]
[<description row>]
...
<blank row>
```

`TYPE`, `TARGET`, `COST`, and `CELL` need their exact tags; the other rows are
selected by their first character. Any tag beginning with `E`, including `END`,
is read as `EFFECT`. The client never reads `FCOMBO`. It reads 6 values of
`TYPE`, 5 of `TARGET` and `LEVEL`, 15 of `DATA`, and 9 of `EFFECT`, whose last
three default to 0. It reads 33 `COST` values, all after the third defaulting to
0, and skips two `CELL` values before reading 91 that default to 0. A new skill
is not entirely zero: until a `TYPE` row is read, its first `TYPE` value is
65535, the 16-bit form of -1, and its fifth is -1.

`BASIC` is a repeated row. Its first value selects slot 0 through 4 and defaults
to 0; the next four values fill that slot. A row naming another slot has no
effect, and a later row for the same slot replaces the earlier one. Observed
records contain five rows per skill, one for each slot, with a sixth value the
client ignores.

The `Z_DESC` count is kept in a 16-bit word like the `Item.dat` count, and only
a positive signed 16-bit value starts a description scan. A positive count
causes the client to consume the immediately following row and then up to 100
more rows, stopping only at end of payload or a subsequent row whose first
physical character is `#`. Blank rows, `VNUM`, `END`, and `~` are description
data while that scan is active. In both observed layouts, a leading-`#` divider
ultimately ends every positive description; intervening blank rows become
trailing line breaks in the loaded text. Text after a positive count on the
`Z_DESC` row is ignored. A non-positive count consumes no following row; the
description is the text after the count on the `Z_DESC` row, with its leading
whitespace kept. Every `Z_DESC` row sets the count, but one whose description is
empty leaves the earlier description text in place. A final `~` has no effect
only when it reaches the outer tagged-row reader.

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
VNUM <item_id> <i32> <flag> <flag> <i32> <flag> <flag>
ITEM <6 integers>
ID <text>
TITLE1 <text>
TITLE2 <text>
COST <6 integers>
LINK <count> <linked_item_id>...
DSTART
<description row>
...
DEND
END
```

Each `VNUM` row starts a new item whose fields are zero and whose texts are
empty. Every other row is optional and sets fields of the current item; rows
before the first `VNUM` are discarded. A repeated row replaces the earlier row's
values, but a `TITLE1`, `TITLE2`, or `DSTART` row that reads no text leaves the
earlier text. Tags are compared after trimming and in any letter case, so `vnum`
also starts an item. The client never reads `ID`, and it ignores `END`, unknown
tags, blank rows, and rows whose first token begins with `#`. `END` is therefore
not an entry boundary.

The client splits rows into tags and values like most readers, so a space before
a tab stays inside the token: `ITEM 1<TAB>2` has the tag `ITEM 1` and is
ignored, and `COST<TAB>1 2<TAB>3` reads `1 2` as one value.

Numeric fields take the row's values in order as integers. A missing or
non-numeric value becomes `-1`, and values after the last field are ignored.
`VNUM` holds three integers and four flags in the order shown. A flag is true
for a nonzero number or `True` and false for zero or `False`, in any letter
case. A missing or other flag value leaves the first three flags false and the
last one true.

`LINK` has no fixed width. Its first value counts the linked item IDs that
follow, which name other items by their `VNUM` item ID. Buying or gifting an
item with a positive count opens a selection of the linked items instead of the
item itself. A count larger than the IDs present reads the missing IDs as `-1`,
and IDs after the count are ignored. The client keeps the low 16 bits of the
count as a signed value, so `65537` and `-$FFFFFFFF` count one ID and `32768` is
negative. A negative count, including the `-1` of a missing or non-numeric
count, makes the client raise a range error while it loads the record. This
applies to every `LINK` row, including one that a later row replaces and one
before the first `VNUM`.

`TITLE1` and `TITLE2` take the trimmed rest of their row after the tag. Because
the row is trimmed first, an indented `TITLE1<TAB>name` row sets the title
`name`.

After `DSTART`, the client reads at most 20 rows as description text. It stops
earlier at a row starting with `#` in column one, a row that trims to `DEND` in
any letter case, or the end of the payload. Read rows are trimmed and joined
with line breaks; blank rows before the first text add nothing. Row parsing then
resumes at the row where the scan stopped, so the closing `DEND` is an ignored
row. Rows after a column-one `#` row or after the 20th row are ordinary tagged
rows: `VNUM` starts the next item, `ITEM`, `COST`, `LINK`, `TITLE1`, `TITLE2`,
and `DSTART` set values, and other rows are ignored. The current archive
contains 62 regions with 21 to 25 rows, all in `kr_nosmall.dat`. Their extra
rows have no tag, so the client ignores them.

The locale prefix selects the text encoding; it does not alter the row grammar.

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
