# NosTale client compatibility audit

Audit date: 2026-09-29.

Taletool revision: `5a7078e54472f0f1141955300a57b50ac8329cda`.

## Scope and conclusion

This compares taletool's Rust implementations, CLI conversion policies, and
format documentation against NosTale’s archive and payload handling. Every
archive and asset family in the README support table is covered below, including
each documented game-data record grammar.

The most consequential discrepancies are:

- Split archives use the wrong resource-ID routing rules.
- Height grids have an extra purported ID before the version/map-ID field.
- Binary archive lookup does not implement the client's direct-index or unsigned
  binary-search behavior.
- Text archive rebuilding loses metadata and can change the decoding mode;
  repeated filenames overwrite each other during extraction.
- Several structured game-data converters discard client-readable data or model
  different field widths, indexing, or entry boundaries.

The graphics payload layouts otherwise agree substantially with the client
readers. Some restrictions are intentional validation policies, rather than
incorrect byte layouts. Conversely, accepting a payload in taletool does not
establish that the client can use it: compression choices and some runtime
limits are not represented by the generic serializers.

This compatibility audit includes synthetic taletool CLI reproductions. It does
not claim to have executed NosTale, validated every shipped asset, or proved
that one client version defines all historical and future formats. Taletool
references link to the relevant implementation and documentation.

Severity terms:

- **High:** can produce unusable archives or lose meaningful client data.
- **Medium:** changes edge-case behavior, metadata, or documented semantics.
- **Policy/gap:** stricter acceptance, deliberately source-oriented data, or
  claims that cannot be verified from this client.

## Archive and asset coverage

| Family/file                                                    | Client consumer                                                   | Result                                                                                                                                                                                  |
| -------------------------------------------------------------- | ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Binary `.NOS` envelope                                         | `FindMultiFileEntryIndex`, stream/cache helpers                   | Header/table sizes agree; lookup, routing, compression policy, and rebuild metadata differ: A1–A5.                                                                                      |
| `NSgtdData.NOS`                                                | `LoadPackedDataFile`, `LoadMainPackedDataRecord`                  | Envelope agrees; flag dispatch and preservation differ: A6–A8. Record audit below.                                                                                                      |
| `NSlangData*.NOS`                                              | `LoadPackedDataRecordByName`, `LoadGTDLangDataListFromLines`      | DAT and tab-separated normal rows agree; malformed-row handling differs: T7.                                                                                                            |
| `NScliData*.NOS`                                               | `LoadPackedConstStringDataFile`, `TNTConstStringList`             | Vertical-tab delimiter agrees; numeric defaults and malformed rows differ: T7.                                                                                                          |
| `NSetcData.NOS`                                                | `LoadMiniGame6WordAndTabooRecord`                                 | Word-list and binary filter consumers confirmed; source strings versus runtime normalization distinguished in T10.                                                                      |
| `snd.pck`                                                      | `NTSoundsCore.TNOSLSCFilePack.LoadFromFile`                       | 28-byte header and 76-byte disk rows agree; stricter magic checks and lookup differences: S1, V1.                                                                                       |
| `sndinfo.lst`                                                  | `NTSoundsCore.TSndTableList.LoadFromFile`                         | Count and 124-byte rows agree; duplicate resolution differs: S1.                                                                                                                        |
| `*.PKG`                                                        | Updater/package behavior not verified                             | Package/delta/mutation/relaunch semantics remain unverified: U1.                                                                                                                        |
| `NSmnData.NOS`, `NSpnData.NOS`                                 | `TGBFCIndexList.Create`                                           | 25-byte prefix skip, four dwords, seven counted six-byte cell lists, unsigned key ordering agree. Prefix validation is taletool policy: V1.                                             |
| `NSmcData`, `NSpcData`                                         | Animation descriptor and playback paths                           | Count/flags/two-byte frames, 60-tick frames, loop bit `0x80`, and nonzero event flags agree. Container lookup/compression caveats apply.                                                |
| `NSpmData`                                                     | Player resource ordering                                          | Count plus eight bytes per frame agrees, including identity fallback and skipped resource slots above 7. Container caveats apply.                                                       |
| `NSmpData`, `NSppData`, `NSipData`                             | Compact sprite texture readers                                    | Count, 12-byte descriptors, absolute pixel offsets, signed placement, A4R4G4B4, and 512-pixel limit agree. A1 affects monster/player archives.                                          |
| `NS4BbData`                                                    | `TFreeSizeSpriteTextureCache.HandlePrimaryMiss`                   | Width/height and BGRA pixels in 256-pixel column-major blocks agree, including partial blocks. Additional split routing exists: A1.                                                     |
| `NStpData`, `NStpeData`, `NStpuData` and localized UI variants | Texture cache                                                     | Eight-byte header, five formats, square/nonzero dimensions, mip-count-zero behavior, quartered byte sizes, filter flag and opaque byte agree. A1/A4/V1 apply.                           |
| `NStgData`, `NStgeData`                                        | `TLBSGeometryItem.LoadFromStream`                                 | Header, vertex arrays, low-byte count slots, recursive nodes, and batches agree. Runtime index-count overflow and ignored bytes: G1/V1.                                                 |
| `NSedData`, `NSesData`                                         | `ResolveTextureAnimationColor`, `ResolveTextureAnimationFrameKey` | Timing header and six-byte keys agree. Same wire shape correctly requires an external kind distinction. A4/V1 apply.                                                                    |
| `NSemData`                                                     | `ResolveTextureAnimationTransform`                                | Independent payload-relative offsets, 2D translation, packed rotation, and 3D scale agree. Canonical re-layout loses padding by documented design.                                      |
| `NSeffData`                                                    | Effect loading and playback                                       | 24-byte root, 192-byte fixed components, eight tracks and their value widths agree. Workspace overwrites and custom packed-float conversion agree. Unknown-kind acceptance differs: V1. |
| `NStuData`                                                     | `TLBSBulkItem.LoadFromCacheStream`                                | 133-byte header, geometry-key table and four recursive node layouts agree. Client unconditionally inflates; raw overrides are unsafe: A4.                                               |
| `NStcData`                                                     | `TUnit281MaskedTileGrid`, grid consumers                          | Signed 16-bit dimensions and row-major bytes agree. Only client-evidenced flag meanings can be verified: U2.                                                                            |
| `NStkData`                                                     | `TLBSBkRsInfo.LoadResourceFileId`                                 | Eight-byte skipped prefix, 81-byte neighbor records and point-sequence layout agree. The intended transition system remains a theory: U2.                                               |
| `NSgrdData`                                                    | `TNT_HKHGridData`                                                 | Fundamental preamble mismatch: H1. Routing: A1.                                                                                                                                         |
| `NStsData`                                                     | `SoundDataFileName` declaration only                              | Unused declaration confirmed; no payload reader identified. No sound/map payload layout can be inferred from the name alone.                                                            |
| Loose `BGM*`, `.ntm`, `.nam`                                   | Miles audio and DirectShow wrappers                               | Media are handed to external decoders; exact codec restrictions are not established by this audit: U1.                                                                                  |

### Game-data record coverage

This table distinguishes an editable source representation from the actual
runtime record. Preserving tokens that this client ignores can be useful for
other data versions; describing those tokens as client-consumed fields is a
different claim.

| Record                 | Comparison with client reader                                                                                                                                                                                                             |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `act_desc.dat`         | `Data`/`A` recognized, but runtime merges by act ID; first `Data` integer is discarded, repeated `A` replaces text, and children append. Taletool's two source tables are not two runtime tables. General numeric/text differences in T9. |
| `BCard.dat`            | Subject indexing is wrong, and stored slots are not bounded by the number of `DESC` tokens: T1.                                                                                                                                           |
| `Card.dat`             | Core tagged layout and KIT/Z_ETC limits agree. `ICON` is an additional runtime-supported row missing from taletool's schema: T9.                                                                                                          |
| `Item.dat`             | Tagged records and positive-description scan broadly agree. Requiring complete rows/entries and normalizing text differ: T9. Native field coercions are not modeled.                                                                      |
| `monster.dat`          | Main tagged families and grouped arrays agree. Sparse/defaulted rows accepted by the client can be dropped by taletool: T9.                                                                                                               |
| `Skill.dat`            | Frame-independent text layout and leading-`#` description boundary agree. Sparse/defaulted rows and text normalization differ: T9.                                                                                                        |
| `npctalk.dat`          | First-row skip agrees; pending-key/state boundaries and mandatory-title assumption do not: T6.                                                                                                                                            |
| `quest.dat`            | Five-value DATA, two-value TALK, textual O, and ID-dependent VNUM interpretation differ: T2.                                                                                                                                              |
| `qstprize.dat`         | Two-value VNUM/five-value DATA agree; textual O is lost: T2.                                                                                                                                                                              |
| `tutorial.dat`         | SCRIPT/END and default step `-1` mostly agree; prefix matching and 500-command stop differ: T8.                                                                                                                                           |
| `shoptype.dat`         | Bare rows and `~` becoming vnum `-1` agree. Source token lists are broader than the six runtime type slots; numeric defaults differ: T9.                                                                                                  |
| `MapIDData.dat`        | Multiword remainder is legal; DATA writes one scalar, with last-write semantics: T4.                                                                                                                                                      |
| `MapPointData.dat`     | Multiword names are legal; only first 200 points per section are loaded: T4/T8.                                                                                                                                                           |
| `qstnpc.dat`           | Runtime loads only enable value 1; mode-0 rows are source data ignored by this reader: T11.                                                                                                                                               |
| `team.dat`             | Runtime VNUM consumes one integer, not two: T5.                                                                                                                                                                                           |
| `fish.dat`             | Client reader supports a smaller grammar and fixed indexed arrays: T11.                                                                                                                                                                   |
| `<locale>_nosmall.dat` | Counted LINK, boolean fields, ignored ID, and description-scan details differ: T3.                                                                                                                                                        |
| `<locale>_abuse.lst`   | Count/length/XOR-1 layout agrees; runtime trims and converts strings, while source representation preserves bytes: T10.                                                                                                                   |
| `MapTotalData.dat`     | Active dispatch and client parser exist, but taletool has no structured grammar: T12.                                                                                                                                                     |

## Archive discrepancies

### A1. High: split routing is family-specific, not uniformly low-byte

Taletool's packer selects `file_id & 0xff`, then rejects results outside the
preset chunk count. The client installs these selectors:

| Family      | Client selector       | Example                                                                       |
| ----------- | --------------------- | ----------------------------------------------------------------------------- |
| `NStgData`  | `id & 3`              | ID 4 belongs to chunk 00; taletool rejects chunk 4.                           |
| `NStpData`  | `id & 0x1f`           | ID 32 belongs to chunk 00; taletool rejects chunk 32.                         |
| `NStpuData` | `id & 3`              | ID 4 belongs to chunk 00.                                                     |
| `NStpeData` | `id & 7`              | ID 8 belongs to chunk 00.                                                     |
| `NSgrdData` | `id & 7`              | Client supports eight-way routing; taletool's preset only emits one file.     |
| `NSmpData`  | `(id & 0x3c00) >> 10` | ID 1024 belongs to chunk 01; taletool emits it in 00.                         |
| `NSppData`  | `(id & 0xf800) >> 11` | ID 2048 belongs to chunk 01; taletool emits it in 00.                         |
| `NS4BbData` | `id & 3`              | Split loading is supported although taletool documents/presets a single file. |

Single unsuffixed archives are supported, so single-file presets for grids or
free-size sprites are not inherently invalid. The missing split behavior and
claims that all other split families use the low byte are the discrepancies.

Taletool references:
[packer](../crates/taletool-cli/src/commands/archive.rs#L726),
[presets](../crates/taletool-cli/src/binary_preset.rs#L50).

Recommendation: use explicit per-family routing functions shared by reading and
packing. Keep generic low-byte mode as a separate custom-container option.

### A2. Medium: split reading masks misplaced or missing chunks

`BinaryNosSplitArchive::open_family` collects any matching prefix/suffix into a
dense sorted vector. `read_entry` indexes that vector by the low byte, then
searches every archive if the first attempt fails. The client keeps numbered
slots, including nil entries for missing files, and uses only the selected
stream. Taletool can therefore find an entry the client cannot load, and a
missing 00 chunk makes vector index 0 refer to a different physical chunk. The
filename prefix match can also include unrelated suffix variants.

Taletool references: [split API](../crates/taletool-archive/src/binary.rs#L674).

Recommendation: preserve parsed hexadecimal chunk numbers and provide an
explicit forensic search separately from client-equivalent lookup.

### A3. High: binary entry lookup and new-file sorting differ

The client’s `FindMultiFileEntryIndex` (`0x00461830`) uses the requested ID as a
table index when `DirectIndex != 0` and the unsigned ID is below the count.
Otherwise it binary-searches the table using unsigned comparisons. A direct
index outside the count still takes that search path. Taletool always returns
the first row whose stored ID matches.

Consequences:

- With direct indexing enabled, table IDs `[42, 99]` and lookup ID 0 select the
  first payload in the client; taletool reports no matching ID.
- With duplicate sorted IDs, the client's first binary-search hit need not be
  the first stored occurrence. Taletool promises the first occurrence.
- New CLI payloads are sorted as signed `i32`, while client search requires
  unsigned ordering. For example, signed order `[-1, 1]` cannot be searched
  correctly as unsigned keys.
- The public writer accepts arbitrary order without validating the search
  contract. Existing explicit-index filenames help preserve table order on
  extraction/repacking, but do not make the lookup API client-equivalent.

Taletool references:
[Rust lookup](../crates/taletool-archive/src/binary.rs#L367),
[signed sort](../crates/taletool-cli/src/binary_payloads.rs#L147).

Recommendation: distinguish physical-table enumeration, stored-ID search and
client lookup. Validate or deliberately construct index-mode output.

### A4. High: generic compression support overstates client compatibility

Taletool can decode and write raw or zlib records in every binary family. The
client does not universally honor the record compression byte:

- Sprite/cell zip caches branch on **any nonzero** compression byte, rather than
  requiring exactly 1.
- `NStuData` always constructs a zlib decompression stream after the 13-byte
  record header. A raw override creates an unusable map record.
- Geometry copies stored bytes without decompressing them. Memory-backed
  animation/remap streams return a pointer after the record header without
  decompressing. Texture, neighborhood, height-grid, and effect-definition
  readers also consume the expected raw payload directly.

The normal presets mostly choose the right representation, but generic
`--compression` and per-file overrides can produce archives taletool accepts and
the client misreads. The documentation should distinguish generic container
capability from family-compatible storage. Compression levels and exact zlib
1.1.2 exporter profiles cannot be inferred from these readers.

Taletool references:
[Rust compression branch](../crates/taletool-archive/src/binary.rs#L269).

### A5. Medium: binary rebuilding overwrites opaque record tags

The docs correctly call the first record dword opaque `record_tag`. The parsed
entry and editable record types do not retain it; the writer puts `file_id`
there instead. A synthetic record tagged `0x20051104` with ID 0 became tag 0
after raw extraction and repacking.

No reviewed client reader branches on this tag, so this is a metadata/lossless
rebuild defect rather than a demonstrated playback failure. Unmodified
`as_bytes`/save paths retain original bytes; decoded rebuild paths do not.

Taletool references:
[entry models](../crates/taletool-archive/src/binary.rs#L109),
[writer](../crates/taletool-archive/src/binary.rs#L650),
[documented record tag](formats/nos-binary-archives.md).

### A6. High: text decoding must honor the packed flag

`TextNosRecord::payload_kind` treats a `.dat` record as compact DAT even when
its packed flag is zero. NosTale's shared loaders use compact decoding only when
the flag is nonzero; otherwise they call `TStrings.LoadFromStream`. A plain-text
`conststring.dat` with flag 0 is client-readable but fails taletool conversion.

The `.lst` binary filter is a specialized exception: abuse/Taboo callbacks read
the original stream themselves. Extension-based standalone CLI inference is
reasonable when no envelope exists; overriding an available envelope flag is not
equivalent to the client.

Taletool references:
[payload-kind inference](../crates/taletool-archive/src/text.rs#L92).

### A7. High: text archive extraction/repacking is not lossless

The ordinary archive workflow loses:

- Stored record IDs, regenerated as 1 through N.
- Original order, replaced by a case-insensitive filename sort.
- Packed flags, regenerated from extensions.
- The timestamp trailer and any other trailing bytes.
- Duplicate names, which overwrite the same output file.
- Non-UTF-8 name bytes, which are decoded with replacement before filenames are
  escaped and cannot be reconstructed from those filenames.

These are behaviorally relevant. NosTale has numeric-ID lookup; name lookup
returns the first matching record; batch loading processes records in order;
flags select decoding; the timestamp affects the displayed data version.

A fixture with ID 77, flag 0 and a timestamp rebuilt as ID 1, flag 1 and no
timestamp, without transforming the raw payload into compact DAT. Two records
named `same.dat` extracted successfully into one file containing the second
payload. `--convert` detects output-name collisions, but raw extraction does
not.

Taletool references:
[raw extraction and packing](../crates/taletool-cli/src/commands/archive.rs#L577),
[text writer](../crates/taletool-archive/src/text.rs#L210),
[name decoding](../crates/taletool-archive/src/text.rs#L155).

Recommendation: add an ordered archive manifest with raw names, IDs, flags and
trailer bytes, and unique physical payload filenames, as sound packs already
use. Correct the README's archive-level losslessness claim meanwhile.

### A8. Medium: timestamp conversion is one second early at integral seconds

The implementation computes `round((variant - 2.00001) * 86400 - 2208988800)`,
equivalent to subtracting 25569.00001 days. The documented epoch uses 25569.0. A
timestamp of 25569.0 therefore reports Unix time **-1**, rather than 0.

There is also an acceptance difference: taletool recognizes the footer only when
exactly 12 bytes remain after parsed records. NosTale reads the marker from EOF
independently of record parsing, so extra data before a valid final footer does
not prevent its recognition. The client's missing-footer fallback date is
documented correctly, but taletool reports an absent timestamp rather than
calculating that runtime fallback.

Taletool references: [conversion](../crates/taletool-archive/src/text.rs#L167).

## Height grids

### H1. High: the extra grid ID is not in the client wire layout

`LoadFromMultiFileStream` reads exactly the common 13-byte record header, then
calls `LoadFromStream` at the payload start. That routine immediately reads the
version-or-map-ID dword. There is no preceding serialized grid ID.

| Field              | Client implicit layout | Client explicit layout | Taletool implicit layout | Taletool explicit layout |
| ------------------ | ---------------------: | ---------------------: | -----------------------: | -----------------------: |
| Version tag        |                 absent |                      0 |                   absent |                        4 |
| Map ID             |                      0 |                      4 |                        4 |                        8 |
| Declared size, u64 |                      4 |                      8 |                        8 |                       12 |
| Bounds minimum     |                     12 |                     16 |                       16 |                       20 |

Taletool reads/writes an extra `grid_id` at offset 0. This has three different
effects, which should not be conflated:

1. A native implicit-version payload is misaligned and generally rejected.
2. A native explicit-v1 payload can parse accidentally: its version becomes
   `grid_id`, its map ID is mistaken for the implicit-layout discriminator, and
   its remaining fields align. It is reported as implicit v1.
3. Explicit v2 is likewise mistaken for implicit v1, so its 32-bit indices are
   decoded as 16-bit indices. Empty fixtures can pass; populated ones fail or
   are misinterpreted.

Reproductions: a 66-byte native implicit grid failed with "declares 0 bytes"; a
70-byte native explicit-v1 grid was reported as `implicit-version-1` with
`grid_id = 200811281` (`0x0BF82311`); a populated native explicit-v2 grid failed
with ten trailing bytes.

The declared-size interpretation also differs. NosTale only rejects when the
stored unsigned value exceeds the outer record's decoded-size field. Taletool
requires equality with the input slice length. Equality may describe surveyed
assets, but is not the client's acceptance rule. Cell count, bounds and cell
size validations are also stricter than the loader; it consumes the stored cell
count independently of width/depth.

Taletool references:
[Rust decoder](../crates/taletool-map/src/height_grid.rs#L202),
[Rust writer](../crates/taletool-map/src/height_grid.rs#L310).

Recommendation: correct the preamble and JSON model, then add fixtures for all
three native encodings with nonempty vertices, triangles and cell references.
Investigate existing surveyed files before migrating any previously exported
JSON: a supposed grid ID may actually be the native version tag.

## Text payload and record discrepancies

### T1. High: BCard SUBJ is zero-based; LIST is one-based

NosTale parses the suffix of `SUBJ` directly and accepts 0 through 4. It
subtracts one for both `LIST` indices. Taletool subtracts one for SUBJ too,
drops SUBJ0, and writes subjects back beginning at SUBJ1.

Additionally, taletool constructs only as many subject/list slots as there are
DESC tokens. NosTale always has five independent slots. A SUBJ/LIST outside the
DESC token count can still matter and should not disappear.

Fixture: `DESC 0 0`, `SUBJ0 first`, `SUBJ1 second` became JSON subjects
`["second", ""]`. The client loads `first` into slot 0 and `second` into slot

1. The documentation's SUBJ1…SUBJN convention is therefore wrong for this
   client.

Taletool references:
[builder and parser](../crates/taletool-text/src/gtd/entity.rs#L164).

### T2. High: quest schemas disagree with the client fields

For `quest.dat`:

- DATA reads **five** integers per reward row; taletool uses `[i32; 4]`.
- TALK reads **two** integers; taletool requires four.
- O is objective **text**, not a vector of integers. Nonnumeric text disappears
  from taletool's optional objective field.
- LEVEL consumes two integers, not the documented three.
- VNUM is interpreted differently for several quest-ID ranges, rather than
  unconditionally assigning the same six fields. Keeping all source tokens is
  useful, but does not describe those runtime semantics.

For `qstprize.dat`, an O text row is supported by NosTale but absent from
taletool's model. It is warned about and dropped.

A complete taletool-compatible quest with a five-value DATA row and
`O objective text` exported successfully with `data: []` and no objective. A
quest-prize fixture likewise lost its O text. Other client versions might
consume different widths, but this is a concrete incompatibility with the client
version examined, not proof that all surveyed data must be rewritten.

Taletool references:
[quest schema/parser](../crates/taletool-text/src/gtd/structured.rs#L197),
[quest-prize parser](../crates/taletool-text/src/gtd/structured.rs#L351).

### T3. High: NosMall LINK is counted, and VNUM contains booleans

NosTale reads LINK's first integer as a count, allocates that many linked IDs,
and reads that many following integers. Taletool requires exactly six integers
for the entire row. `LINK 0` and `LINK 1 99` are client-readable but cause a
NosMall entry to be discarded as incomplete. Larger lists are also excluded.

VNUM contains integer fields and four `StrToBoolDef` fields, with distinct
defaults. Taletool requires seven decimal integers, excluding textual boolean
forms. NosTale has no ID-tag branch; taletool nevertheless requires ID to retain
an entry.

The source-oriented description array intentionally preserves more than the
runtime's first 20 rows, but its parser only recognizes exact uppercase DEND and
does not model the client's case-insensitive DEND/leading-`#` termination. A
tagged field after such a runtime terminator may be absorbed as description data
by taletool.

Taletool references:
[Rust parser](../crates/taletool-text/src/gtd/localized.rs#L88).

### T4. High: map names may contain spaces

`MapIDData.dat` stores four numeric fields, then the **remaining text** as the
name. `MapPointData.dat` D rows store three numeric fields, then the remaining
text. Both Rust parsers insist on exactly five whitespace-separated tokens, so
`Two Words` causes the entire row to be skipped.

MapID DATA also updates one scalar in the current entry; subsequent DATA rows
replace it. Taletool's ordered vectors preserve source rows, but the docs should
not imply that all their values are consumed by this client.

Taletool references:
[map-ID parser](../crates/taletool-text/src/gtd/structured.rs#L548),
[map-point parser](../crates/taletool-text/src/gtd/structured.rs#L617).

### T5. High: team VNUM does not require two integers

NosTale consumes one integer after VNUM. Taletool's `[i32; 2]` prevents a
one-integer row from even starting an entry. A fixture with VNUM 1, TITLE, four
TARGET values and four BUFF values exported an empty entry list. A second source
token can be preserved for another version, but cannot be mandatory for
compatibility with this reader.

Taletool references:
[Rust team parser](../crates/taletool-text/src/gtd/structured.rs#L752).

### T6. High: NPC dialogue writer delimiters and parser state differ

The writer uses the shared `push_text` helper, inserting a **tab** after each
command. NosTale's NPC reader specifically calls `ExtractDelimitedToken` with
the single delimiter **space**, unlike the other game-data readers that accept
tabs. A normal JSON round trip produced `%\t42`, `s\t7`, and `c\thello`. NosTale
sees no space in those rows, leaves the remainder empty, loads key/state 0
instead of 42/7, and does not retain the display text. This affects ordinary
valid dialogue, not only irregular inputs. The writer needs NPC-specific space
separators.

The client skips the first row, lets `%` update a pending key, and creates a
state only on `s`. Commands still target the last created state until another
`s`, even if a new `%` appeared. An invalid `%` sets the pending key to 0.

Taletool closes the state and entry immediately on a valid `%`, leaves the old
pending entry unchanged on an invalid `%`, and removes every entry without a
nonempty `t` title. NosTale ignores `t` entirely. Thus titleless but functional
dialogue is lost, commands between `%` and `s` can be lost, and invalid-key
fallback behavior differs. The existing documentation claims the correct
invalid-`%` behavior, but the implementation does not implement it generally.

Other runtime details missing from the grammar explanation: `#` inside the
remainder begins a comment; `f` changes persistent line-flag state; certain
image-only `c` rows are filtered. Preserving these as source commands is useful
provided conversion retains their ordering and state effects.

Taletool references:
[Rust parser and title filter](../crates/taletool-text/src/gtd/structured.rs#L82),
[Rust writer](../crates/taletool-text/src/gtd/structured.rs#L167),
[tab-emitting helper](../crates/taletool-text/src/gtd/mod.rs#L499).

### T7. Medium: language and constant-string malformed rows are not skipped

For a nonblank, noncomment NSlang row without a tab, NosTale creates an entry
whose key and value are both `"0"`. Taletool emits a warning and removes it.

For NScli, NosTale appends a row before checking for the vertical-tab delimiter.
A row without the delimiter remains a zero-initialized record. With a delimiter
and an invalid integer key, the key becomes -1 and the text remains. Taletool
removes both cases. Delphi integer parsing also accepts forms such as `$10`;
Rust's decimal-only `parse::<i32>` does not.

These are differences for irregular input, not a reason to weaken truncation
safety. Document the deliberate filtering or provide a runtime-compatible mode;
avoid calling it the client's exact row behavior.

Taletool references:
[Rust language/constant parsers](../crates/taletool-text/src/lib.rs#L180).

### T8. Medium: missing script limits and prefix semantics

The tutorial loader recognizes any token whose first six characters are SCRIPT
case-insensitively, whereas taletool requires the whole token to equal `script`.
At 500 commands in the current tutorial, the client breaks out of the entire
read loop when another command is encountered, potentially leaving later scripts
unread. Taletool accepts an unlimited vector.

MapPoint sections retain only their first 200 D rows in the client. Taletool
accepts more without identifying the unread suffix.

Recommendation: preserve source rows if desired, but expose/warn about runtime
limits when editing, and avoid describing every exported row as client-active.

Taletool references:
[tutorial parser](../crates/taletool-text/src/gtd/structured.rs#L409).

### T9. High/medium: structured text is stricter and less source-preserving

Several cross-cutting assumptions need qualification:

- NosTale allocates zeroed rows at entry boundaries and supplies per-field
  `StrToIntDef` defaults. Taletool often requires every named field before its
  `finish` function retains an entry. A sparse or malformed numeric row can
  therefore remove a whole client-loaded entry from a successful JSON export.
- Some Delphi dispatch is by first character; other readers compare full tokens
  case-insensitively. Rust commonly matches exact uppercase words. The docs'
  case-insensitive quest BEGIN claim is not the same as NosTale's uppercase
  first-character B branch. Fish and team do explicitly uppercase their tokens.
- Integer storage in the client frequently narrows to signed/unsigned words or
  bytes. Preserving signed 32-bit source values is valid, but is not the final
  runtime value. Decimal-only parsing and exact token-count rules also exclude
  Delphi-accepted defaults and hexadecimal forms.
- The client's multi-delimiter helper tries delimiters in order: with
  `[#9, ' ']`, it uses a tab anywhere in the remaining string before trying a
  space. Rust splits on either at each position. Mixed-delimiter rows are
  therefore not generally equivalent; normal uniformly tabbed rows agree.
- Several entity text fields are rebuilt with `fields(...).join(" ")`. NosTale
  uses the trimmed remainder, preserving internal spaces/tabs. A name such as
  `NAME A  B` can change to `A B` after conversion.
- `Card.dat` supports ICON (writing `IconGraphic`) independently of EFFECT;
  taletool has no ICON field and warns/drops the row. Relative ordering between
  these assignments can affect the client result.
- Act records merge by act ID; a later A replaces the title and Data appends
  children. Their source rows should not be described as independent runtime
  entries merely because JSON keeps two arrays.

Taletool references:
[shared Rust token/numeric helpers](../crates/taletool-text/src/gtd/mod.rs#L473),
[Skill required-field finalization](../crates/taletool-text/src/gtd/entity.rs#L910),
[entity text rebuilding](../crates/taletool-text/src/gtd/entity.rs#L228).

Recommendation: distinguish a normalized supported-schema export from a
source-preserving or client-equivalent export. Preserve unmodeled rows and
physical text, or fail explicitly instead of producing a partially populated
document that can later overwrite the source.

### T10. Medium/policy: DAT character and row fidelity; LST normalization

For a compact DAT run, NosTale appends both nibble-table characters when the run
has at least two characters left, including NUL. Taletool suppresses a
zero-valued low nibble. Bytes `82 40 FF` therefore produce `30 0A` in taletool,
where the client decoder's string before row framing contains `30 00`. Whether a
later PChar consumer truncates that string depends on that consumer; the codec
itself does not discard the byte.

Compact nibble 14 also decodes to LF inside a string. NosTale adds each
FF-terminated decoded string as one `TStrings` row; taletool flattens both that
embedded LF and the FF row boundary to the same byte. Later line splitting and
re-encoding can turn one client row into two. Preserving decoded row boundaries
separately from their content is needed for exact codec/structured fidelity.

Do not imitate the client's unsafe `Offset <= BufferLength` boundary accesses;
safe truncation errors are desirable. They are separate from this well-bounded
two-character discrepancy.

The counted LST XOR-1 layout agrees, but the filter loader converts to
WideString, trims, and converts back to ANSI. Taletool's preserved whitespace
and binary fallback are source fidelity, not a runtime filter-list snapshot. The
minigame word reader also omits whitespace-only rows. The generic plain-text LST
bridge flattens entries with newline delimiters, so embedded newlines do not
have a byte-preserving round trip through that representation; the structured
abuse format is the appropriate lossless representation.

Taletool references:
[Rust DAT and LST codecs](../crates/taletool-text/src/lib.rs#L378).

### T11. Version/policy: fish and qstnpc source grammar exceeds runtime use

This NosTale fish reader recognizes only VNUM, LEVEL, ITEMT, MAPT, ITEM and MAP.
ITEM consumes a slot and item ID, without the documented weight. It does not
consume POST, POS, BASICT or BASIC at all. Those fields may be meaningful to
other client revisions or authoring tools, but their claimed runtime role cannot
be verified here. Missing fields retain allocation defaults, while a present
LEVEL/count with a bad token uses -1.

The qstnpc reader only processes rows whose second numeric token equals 1. Mode
0 is ignored, not a second complete client-loaded row type. In enabled rows it
discards the fourth token and keeps the quest ID and required level.

Taletool references:
[Rust fish grammar](../crates/taletool-text/src/gtd/structured.rs#L866),
[Rust qstnpc grammar](../crates/taletool-text/src/gtd/structured.rs#L682).

### T12. Coverage gap: MapTotalData.dat is missing

`LoadMainPackedDataRecord` actively recognizes `MapTotalData.dat`. Its reader
handles V-prefixed rows with four numeric values, T-prefixed text, and a
D-prefixed numeric value, then sorts for map-range lookup. Taletool has no
corresponding `GtdFileKind`. Since archive conversion plans require every record
to have a recognized structured format, this can block conversion of an entire
otherwise valid NSgtd archive containing the record, including the
`--plain-text` conversion path.

`NSgtdData2.NOS` and NPC-specific NSnn/NSnc/NSnp filenames also exist as NosTale
constants. The reviewed initialization uses the shared monster resources;
constant presence alone does not establish an additional active archive format.
They should be listed as evidence-limited names, not silently promoted to
supported families.

Taletool references:
[kind inventory](../crates/taletool-text/src/gtd/mod.rs#L65),
[strict conversion planning](../crates/taletool-cli/src/text_archive_convert.rs#L103).

## Audio and graphics details

### S1. Medium: audio lookup is deterministic but not client-equivalent

Sound metadata layouts and ordinary filename resolution agree. Differences:

- Taletool chooses the first source-order duplicate key or sound ID. NosTale
  sorts pointer indexes and uses binary search; it does not guarantee that
  duplicate choice.
- NosTale sound-ID filename lookup resolves the matched row's key tuple back
  through the key index. Taletool resolves that exact source row directly.
  Duplicate tuples can therefore select different filenames.
- Taletool sorts the fallback `.wav` directory listing; NosTale retains Windows
  enumeration order. This intentional deterministic difference is documented.
- NosTale derives pack keys with `StrToIntDef` between the first two dots;
  taletool uses decimal-only Rust parsing. For example, `x.$10.wav` means key 16
  to Delphi and -1 to taletool. If either dot is absent, both fall back to the
  row index; with two dots and an invalid number, both use -1. The docs' broad
  fallback wording should make that distinction explicit.
- Taletool filename decoding assumes EUC-KR; NosTale passes ANSI filenames to
  Windows APIs. Case sensitivity and code-page behavior on a non-Windows host
  are not an exact emulation of the original runtime.

Taletool references:
[Rust metadata lookup](../crates/taletool-audio/src/lib.rs#L326),
[Rust ID resolution](../crates/taletool-audio/src/lib.rs#L397),
[Rust pack key derivation](../crates/taletool-archive/src/deldx.rs#L805).

### G1. Medium: geometry index totals have a 16-bit runtime limit

The geometry docs correctly observe that the serialized lists can collectively
contain more than 65,535 indices. But NosTale accumulates their lengths into a
`Word TotalIndexCount`, stores per-list starts from it, and sizes the Direct3D
index buffer from that wrapped total. It subsequently copies every list's full
data. A large serialized total is representable, but is not safely supported by
this client runtime.

Taletool accepts those totals without flagging the client limitation. A sum of
65,538 wraps to 2 in that loader. Do not infer runtime support from individual
u16 list counts or from successful taletool parsing.

Taletool references:
[Rust geometry decode](../crates/taletool-geometry/src/lib.rs#L244),
[current geometry docs](formats/geometry.md).

### V1. Acceptance restrictions should be labeled as taletool policy

These are not all bugs. A safe editor should reject truncation and dangerous
references even if an old client fails to check them. The distinction matters
when the docs describe a restriction as a native-format requirement.

| Area                                                               | Taletool restriction                                                                                  | Client behavior                                                                                                                                                       |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Binary conversion                                                  | Exact 16-byte preset header plus direct-index byte                                                    | Generic readers copy the prefix without validating family magic. Any nonzero direct-index byte enables that mode. Raw inspection is less strict than conversion.      |
| CCINF                                                              | Exact prefix, raw flag, equal stored/decoded/body sizes, EOF                                          | Client skips all 25 prefix bytes, then reads the count and cells. Rejection of an actually compressed body is appropriate; the client does not decompress one either. |
| DelDX pack                                                         | Exact magic text and length                                                                           | Client checks version > 10, but does not test the signature. Magic checking is a useful detection policy.                                                             |
| Geometry count slots                                               | High/reserved byte must be zero                                                                       | Client reads the low byte and advances two bytes, ignoring the other byte.                                                                                            |
| Geometry/map booleans                                              | Canonical 0 or 1                                                                                      | Serialized bytes are read directly; relevant flag consumers use byte/Boolean tests rather than a parser-side canonicality check.                                      |
| Geometry/effect timing                                             | Strictly increasing keys and restricted timing                                                        | Loaders do not enforce all of these checks. Some invalid timelines can cause division/modulo problems, so keep safety checks while documenting them.                  |
| Effect definitions                                                 | Only kinds 0, 1, 2                                                                                    | Factory skips unsupported kinds after the loader has retained the fixed records/tracks. Taletool cannot losslessly represent such a record.                           |
| Maps/neighborhoods/grids                                           | Finite bounds, ordered bounds, nonnegative radii, positive dimensions, valid references, depth limits | Readers largely trust serialized data. These checks are editor policy, not proof of a different wire layout.                                                          |
| Textures, animation/remap, geometry, maps and several other assets | Complete consumption; trailing data rejected                                                          | Client paths generally consume expected data or follow pointers, without a corresponding EOF assertion.                                                               |
| Text archive conversion                                            | All records from one recognized family, one locale, known native names                                | Generic archive reader dispatches names and skips unrecognized records; it does not impose that whole-archive schema rule.                                            |

Taletool references:
[binary conversion policy](../crates/taletool-cli/src/binary_preset.rs#L347),
[CCINF parser](../crates/taletool-ccinf/src/lib.rs#L173),
[DelDX validation](../crates/taletool-archive/src/deldx.rs#L767),
[geometry count validation](../crates/taletool-geometry/src/lib.rs#L522).

## Claims this audit cannot settle

### U1. Patch packages, exporter profiles and media codecs

This audit does not verify PCHPKG package loading, binary delta application,
`.NOS` mutation opcodes, DelDX patch mutation, or `ExtractUIEff`. These belong
to the separate updater executable described in taletool’s patch docs.

Consequently the `.PKG` magic, packed timestamp, segment lookup, opcodes 0–6,
CRC rules, cursor semantics, the 0x50-byte inline DelDX rows, parent-archive
fallback and helper SHA1 remain **unverified against NosTale**. The ordinary
binary/DelDX readers can corroborate output envelope layouts, not patch
application rules. Resolving these claims requires independent verification of
the updater and helper executables. No patch behavior was changed during this
audit.

Likewise, a client decoder cannot establish the exact historical zlib encoder
version, strategy and level, fixed exporter header/date bytes, or statements
about every surveyed release. Keep those as corpus/exporter evidence rather than
attributing them to the client reader.

The video wrapper hands a filename to DirectShow `RenderFile`; it does not
validate MPEG Program Stream magic or restrict the file to that codec. Miles
handles audio bytes. Taletool's Ogg/MP3/WAVE/MPEG sniffing is a useful
classification heuristic, not an enumeration of everything those external
decoders can play. `.ntm`/`.nam` and BGM codec claims require media samples.

Taletool references:
[patch implementation](../crates/taletool-patch/src/package.rs),
[patch assumptions](formats/patch-packages.md),
[media sniffing](../crates/taletool-cli/src/commands/scan.rs#L215).

### U2. Runtime interpretation versus historical/design theories

- Cell-flag layout is confirmed. The docs already distinguish walk/aggro checks
  from server-inferred attack-through/PvP labels; server semantics cannot be
  proven by NosTale alone.
- Neighborhood loading and positioning are confirmed. Empty surveyed tables, an
  unfinished system, portal boundaries and a server-authorized handoff are
  corpus observations or theories, not additional wire-format requirements.
- No NSts payload consumer was identified; `SoundDataFileName` is only a
  declaration. This neither proves a sound format nor gives a parser to audit.
- Locale-to-encoding defaults come from taletool's filename policy. The generic
  Delphi archive decoder manipulates ANSI bytes and does not implement that
  locale mapping. A single regional client cannot verify all locale mappings,
  especially Windows ANSI conversion behavior.

Taletool references:
[encoding defaults](../crates/taletool-text/src/gtd/mod.rs#L101).

## Reproduction and validation

The CLI was rebuilt with `cargo build -p taletool`. Temporary fixtures were
created outside the repository and passed to the built executable. They encode
the field sequences discussed above; they are not claimed to be captured
shipping assets or executions of the Delphi runtime.

| Probe                                                   | Observed taletool result                                                  |
| ------------------------------------------------------- | ------------------------------------------------------------------------- |
| Native implicit grid, one empty cell, 66 bytes          | Rejected: declared size 0 versus input 66.                                |
| Native explicit-v1/v2 empty grids, 70 bytes             | Both reported as implicit v1; version tag exposed as grid ID.             |
| Native explicit-v2 grid, one triangle and one reference | Rejected: ten trailing bytes.                                             |
| Pack NStg ID 4 / NStp ID 32 with presets                | Rejected as out-of-range chunks 4 / 32.                                   |
| Pack NSmp ID 1024 / NSpp ID 2048 with presets           | Succeeded, but placed payloads in chunk 00 instead of client-selected 01. |
| Text timestamp `TDateTime = 25569.0`                    | JSON `unix_seconds: -1`.                                                  |
| Two text records with the same name                     | Successful extraction; only second payload survives.                      |
| Binary record tag `0x20051104`, ID 0                    | Repacked tag becomes 0.                                                   |
| Text record ID 77, flag 0, footer                       | Repacked ID 1, flag 1, no footer.                                         |
| Flag-0 plain conststring.dat conversion                 | Rejected as truncated compact DAT.                                        |
| DAT bytes `82 40 FF`                                    | Plain output hex `300a`; low NUL omitted.                                 |
| BCard SUBJ0/SUBJ1 example                               | Subjects become `["second", ""]`.                                         |
| MapID/MapPoint name `Two Words`                         | Warned and skipped.                                                       |
| Team one-integer VNUM                                   | Successful JSON export with zero entries.                                 |
| Quest five-integer DATA and textual O                   | Successful export with empty DATA and absent objective.                   |
| Quest-prize O text                                      | Warning; text absent from JSON.                                           |
| NosMall `LINK 1 99`                                     | Warning; whole entry omitted.                                             |
| NSlang row without tab                                  | Empty logical table instead of client fallback `0`/`0` row.               |
| NScli `bad` + vertical tab + `text`                     | Empty logical table instead of client key -1/text row.                    |
| Ordinary NPC dialogue JSON repack                       | Tab separators instead of the client's required spaces.                   |
| NPC dialogue without a title                            | Warning; all dialogue entries omitted.                                    |

A minimal reproducible split test, run from the repository root:

```sh
mkdir -p /tmp/taletool-routing/input
printf '\000' > /tmp/taletool-routing/input/4.bin
target/debug/taletool archive pack /tmp/taletool-routing/input \
  --out /tmp/taletool-routing/output --preset NStgData
```

The payload bytes do not need to be geometry for this test: archive packing does
not decode them, and the failure is exclusively the ID-to-chunk rule.

A compact DAT fixture can be generated without using taletool's encoder:

```python
from pathlib import Path

lines = [b"VNUM 1", b"TITLE title", b"TARGET 1 2 3 4", b"BUFF 1 2 3 4"]
payload = b"".join(
    bytes([len(line)]) + bytes(c ^ 0x33 for c in line) + b"\xff"
    for line in lines
)
Path("/tmp/team.dat").write_bytes(payload)
```

```sh
target/debug/taletool text unpack /tmp/team.dat --out /tmp/team.json --json
```

Repository checks passed: `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`. Documentation was formatted with `dprint fmt` and
passed `dprint check`. Passing tests establish the current implementation's
internal consistency, not agreement with the client. The existing tests include
expectations for several discrepant schemas, so fixes should use independently
constructed fixtures for the documented client behavior rather than only round
trips between taletool's own writer and reader.

## Suggested correction order

1. Correct height-grid framing, family routing, and binary lookup semantics.
2. Add archive manifests and preserve text flags/IDs/order/names/trailers and
   binary record tags; prevent duplicate-name overwrite.
3. Correct the BCard, quest, NosMall, team, map-name and NPC dialogue
   converters; make dropped client-readable data explicit before users repack
   JSON.
4. Add MapTotal support and runtime-limit diagnostics; separate source grammar
   preservation from client interpretation and historical-version differences.
5. Correct timestamp conversion, document audio lookup differences, and label
   validation policies and updater/media evidence limits.

This report does not modify format implementations or rewrite their existing
documentation claims; it records the discrepancies for review and follow-up.
