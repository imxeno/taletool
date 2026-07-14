<div align="center">
<img src=".github/logo.png" alt="Taletool hammer logo" width="128" hspace="16">

<h1>taletool</h1>

<p>
  A CLI and Rust library for inspecting, unpacking, and packing NosTale client
  data formats.
</p>

<p>
  <a href="LICENSE"><img alt="License: AGPL-3.0-or-later" src="https://img.shields.io/badge/license-AGPL--3.0--or--later-2563eb?style=flat-square"></a>
  <a href="Cargo.toml"><img alt="Rust 1.97 or newer" src="https://img.shields.io/badge/rust-1.97%2B-000000?style=flat-square&logo=rust"></a>
  <a href="https://github.com/imxeno/taletool/commits"><img alt="Last commit" src="https://img.shields.io/github/last-commit/imxeno/taletool?style=flat-square&logo=github"></a>
</p>

</div>

## Installation

Building requires Rust 1.97.0 or newer and a C compiler for the vendored zlib
implementation.

```console
git clone https://github.com/imxeno/taletool.git
cd taletool
cargo install --path crates/taletool-cli
taletool --help
```

To run the CLI from a checkout without installing it, put arguments after `--`:

```console
cargo run -p taletool -- scan --data-dir "C:\GameforgeLive\Nostale\NostaleData"
```

All examples below assume that `taletool` is installed and available on `PATH`.

## Support

| Name                      | Contains                                           | Container support | Asset support |
| ------------------------- | -------------------------------------------------- | ----------------- | ------------- |
| `NS4BbData.NOS`           | Free-size sprite resources                         | ✅                | ✅            |
| `NScliData*.NOS`          | Client const strings                               | ✅                | ✅            |
| `NSedData.NOS`            | Effect color animation keyframes                   | ✅                | ✅            |
| `NSeffData.NOS`           | Effect definitions                                 | ✅                | ✅            |
| `NSemData.NOS`            | Effect transform animation keyframes               | ✅                | ✅            |
| `NSesData.NOS`            | Effect texture animation frame keys                | ✅                | ✅            |
| `NSetcData.NOS`           | Typewriter mini-game and unused “taboo” word lists | ✅                | ✅            |
| `NSgrdData*.NOS`          | Optimized map height grid data                     | ✅                | ✅            |
| `NSgtdData.NOS`           | Game data files                                    | ✅                | ⚠️             |
| `NSipData.NOS`            | Map-item sprite resources                          | ✅                | ✅            |
| `NSlangData_<locale>.NOS` | Language files                                     | ✅                | ✅            |
| `NSmcData.NOS`            | Monster/NPC animation definitions                  | ✅                | ✅            |
| `NSmnData.NOS`            | Monster/NPC GBFC index                             | N/A               | ✅            |
| `NSmpData*.NOS`           | Monster/NPC sprites                                | ✅                | ✅            |
| `NSpcData.NOS`            | Player animation definitions                       | ✅                | ✅            |
| `NSpmData.NOS`            | Player frame/resource remap tables                 | ✅                | ✅            |
| `NSpnData.NOS`            | Player GBFC index                                  | N/A               | ✅            |
| `NSppData*.NOS`           | Player sprites                                     | ✅                | ✅            |
| `NStcData.NOS`            | Map cell flags                                     | ✅                | ✅            |
| `NStgData*.NOS`           | Geometry                                           | ✅                | ✅            |
| `NStgeData.NOS`           | Effect geometry                                    | ✅                | ✅            |
| `NStkData.NOS`            | Map neighborhood data                              | ✅                | ⚠️             |
| `NStpData*.NOS`           | Textures                                           | ✅                | ✅            |
| `NStpeData*.NOS`          | Effect textures                                    | ✅                | ✅            |
| `NStpuData*.NOS`          | UI/widget textures                                 | ✅                | ✅            |
| `NStsData.NOS`            | Unknown and unused map-related data                | ✅                | ❌            |
| `NStuData*.NOS`           | Map settings and geometry object trees             | ✅                | ✅            |
| `BGM*`                    | BGM audio files                                    | N/A               | N/A           |
| `*.ntm`, `*.nam`          | Intro/act videos                                   | N/A               | N/A           |
| `snd.pck`                 | Audio files                                        | ✅                | N/A           |
| `sndinfo.lst`             | Audio metadata                                     | N/A               | ✅            |
| `*.PKG`                   | NosTale patch packages                             | ⚠️                 | ⚠️             |

✅ supported, ⚠️ partial support, ❌ not supported, N/A not applicable as an
independent container or asset layer

`*` groups the observed names for a family. Exact single, split, locale, and
locale+chunk filename patterns are listed in the format docs.

See [the docs](docs/README.md) for file format notes.

## Usage

The CLI is intentionally low-level. It primarily showcases the functionality
provided by the Taletool libraries and exposes the underlying file-format
operations without imposing a higher-level modding workflow.

You are welcome to use the Taletool libraries in your own projects, including
modding tools, graphical editors, and other NosTale utilities.

**The CLI and library APIs are not stable yet.** This is also why the project
has not been published to crates.io. Expect breaking changes and pin an exact
version or revision when consuming Taletool from another project.

The command tree follows the data hierarchy: `archive` handles full `.NOS` and
`.pck` containers, while commands such as `map`, `texture`, and `text` handle
individual payloads extracted from those containers.

```text
taletool [OPTIONS] <COMMAND>
```

Use `taletool --help`, `taletool <COMMAND> --help`, or
`taletool <COMMAND> <SUBCOMMAND> --help` for built-in help.

### Global Options and Input Paths

- `-v` or `--verbose` enables informational diagnostics. Repeat it (`-vv`) for
  debug diagnostics. `RUST_LOG` overrides this verbosity-derived log filter.
- Options marked as global, such as `-v`, can appear before or after a
  subcommand.
- `archive inspect`, `archive unpack`, `patch inspect`, and `patch apply` accept
  multiple paths. They also expand quoted `*` and `?` filename patterns
  case-insensitively within one directory. You should quote wildcard inputs,
  especially in shells that perform their own wildcard expansion:
  `"NStpData*.NOS"`.
- `--json` writes machine-readable inspection output to stdout. `--checksum`
  adds a 64-bit FNV-1a checksum where the command supports it.

### Command Overview

| Command            | Purpose                                                       |
| ------------------ | ------------------------------------------------------------- |
| `scan`             | Classify supported files in a client data directory.          |
| `archive`          | Inspect, unpack, and pack binary, text, and sound containers. |
| `animation`        | Work with `NSmcData` and `NSpcData` animation payloads.       |
| `map`              | Work with `NStuData` map payloads.                            |
| `ccinf`            | Work with `NSmnData` and `NSpnData` GBFC index files.         |
| `effect`           | Work with definitions and animation payloads for effects.     |
| `geometry`         | Work with `NStgData` and `NStgeData` geometry payloads.       |
| `height-grid`      | Work with `NSgrdData` optimized height grids.                 |
| `map-neighborhood` | Work with `NStkData` map-neighborhood payloads.               |
| `sprite`           | Convert map-object or free-size sprites to and from PNG.      |
| `sprite-remap`     | Work with `NSpmData` frame/resource remap payloads.           |
| `patch`            | Inspect or apply original `.PKG` patch packages.              |
| `text`             | Decode or encode individual DAT, LST, and raw text payloads.  |
| `audio`            | Convert `sndinfo.lst` audio metadata to and from JSON.        |
| `texture`          | Convert texture payloads and mip levels to and from PNG.      |
| `cell-flag`        | Export an extracted `NStcData` grid as PNG.                   |

### Scanning a Data Directory

```text
taletool scan --data-dir <DATA_DIR> [--no-recursive] [--show-unsupported]
  [--json] [-v]
```

`scan` recursively examines a client data directory and reports paths relative
to that directory. By default, it shows recognized `.NOS` and `.pck` files and
identifies Ogg, MP3, RIFF/WAVE, and MPEG media from their header bytes,
regardless of the filename or extension. Media files are reported as
`type=audio` or `type=video`. Add `--no-recursive` to examine only immediate
files. Add `--show-unsupported` to include every other regular file with
`type=unsupported`. Add `-v` to include archive counts and media format details.
Combine `-v` with `--json` to include those details in each JSON result.

```console
taletool scan --data-dir "C:\NosTale\NostaleData"
taletool scan --data-dir "C:\NosTale\NostaleData" --show-unsupported -v --json
```

### Archive Containers

The archive commands support binary `.NOS` tables, text `.NOS` record archives,
and `snd.pck`.

```text
taletool archive inspect <INPUT>... [--type <TYPE>] [--json] [--checksum]
taletool archive unpack <INPUT>... --out <DIR> [--type <TYPE>]
taletool archive pack <DIR> --out <OUT> [OPTIONS]
```

`--type` accepts `auto` (the default), `binary`, `text`, or `sound`. Automatic
detection requires exactly one parser to match. Use an explicit type for a
renamed or ambiguous file. Text and sound archives take one input; binary
archives may take multiple chunks from the same family. Automatic detection only
accepts text archives without trailing bytes, while `--type text` also accepts
parsed text archives that contain trailing data.

CCINF files (`NSmnData.NOS` and `NSpnData.NOS`) are standalone assets rather
than archive containers. Use the dedicated `ccinf` command for them.

#### Inspecting and Unpacking

```console
taletool archive inspect "NStpData*.NOS"
taletool archive inspect snd.pck --type sound --json --checksum
taletool archive unpack "NSlangData_UK*.NOS" --out work/lang
```

The unpacked layout depends on the container:

| Type     | Output layout                                                     |
| -------- | ----------------------------------------------------------------- |
| `binary` | Raw payloads named by numeric ID, for example `42.bin`.           |
| `text`   | Still-encoded record payloads named after escaped archive names.  |
| `sound`  | Ordered payload files plus a required `sound-pack.json` manifest. |

Binary filenames preserve metadata needed for a stable round trip. A filename
can contain a duplicate ordinal (`42__2.bin`), an explicit table slot
(`42__index7.bin`), and a per-entry compression override (`42__raw.bin` or
`42__zlib.bin`). `archive pack` reads immediate files whose names begin with a
decimal ID and ignores other files.

Text archive filenames use `%HH` escapes for characters that are not ASCII
letters, digits, `.`, `-`, or `_`. Packing reverses these escapes. Archive
unpacking does not decode DAT or LST contents; use `taletool text unpack` on an
extracted record.

#### Packing

For known binary archive names, the default `--preset auto` infers the archive
header, direct-index byte, compression profile, chunk count, routing strategy,
and filename pattern from `--out`. An explicit preset is useful when `--out` is
a directory or a renamed file:

```console
taletool archive pack work/nstp --out rebuilt --preset NStpData
taletool archive pack work/lang --out NSlangData_UK.NOS --type text
taletool archive pack work/sound --out snd.pck --type sound
```

With `--type auto`, a `sound-pack.json` manifest or `.pck` output selects
`sound`; an `NSgtdData`/`NSlangData` preset or output name selects `text`; a
directory containing only numeric payload filenames selects `binary`; and any
other directory selects `text`.

Binary packing options are:

| Option                     | Meaning                                                      |
| -------------------------- | ------------------------------------------------------------ |
| `--preset <NAME>`          | Known family, `auto` (default), or `none`.                   |
| `--header-hex <HEX>`       | Exact 16-byte header as 32 hex digits.                       |
| `--direct-index <0..255>`  | Override the header's direct-index byte.                     |
| `--compression <MODE>`     | `auto`, `raw`, or `zlib`; per-file suffixes can override it. |
| `--zlib-profile <PROFILE>` | `auto` or `zlib112-levelN-STRATEGY`.                         |
| `--chunking <MODE>`        | `single` or `low-byte`.                                      |
| `--chunk-count <COUNT>`    | Number of output chunks; must be greater than zero.          |
| `--chunk-format <FORMAT>`  | Chunk filename pattern below the `--out` directory.          |

`--header-hex` may contain whitespace and underscores. A zlib profile level is
`0` through `9`, and its strategy is `default`, `filtered`, or `huffman`.
Low-byte chunking routes each file ID using its low byte, which must be smaller
than `--chunk-count`.

Split output patterns may contain `{chunk}`, `{chunk:02x}`, or `{chunk:02X}`.
Use the token directly in `--out`, or pass a base directory in `--out` and the
filename through `--chunk-format`:

```console
taletool archive pack work/custom --type binary \
  --out "rebuilt/Custom{chunk:02X}.NOS" \
  --header-hex "4e542044617461203939000015070420" \
  --compression zlib --zlib-profile zlib112-level9-default \
  --chunking low-byte --chunk-count 4
```

Known presets are:

| Preset      | Storage | Chunks | Default output             |
| ----------- | ------- | -----: | -------------------------- |
| `NS4BbData` | zlib-9  |      1 | `NS4BbData.NOS`            |
| `NSedData`  | raw     |      1 | `NSedData.NOS`             |
| `NSeffData` | raw     |      1 | `NSeffData.NOS`            |
| `NSemData`  | raw     |      1 | `NSemData.NOS`             |
| `NSesData`  | raw     |      1 | `NSesData.NOS`             |
| `NSgrdData` | raw     |      1 | `NSgrdData.NOS`            |
| `NSipData`  | zlib-1  |      1 | `NSipData.NOS`             |
| `NSmcData`  | raw     |      1 | `NSmcData.NOS`             |
| `NSmpData`  | zlib-1  |     16 | `NSmpData{chunk:02X}.NOS`  |
| `NSpcData`  | raw     |      1 | `NSpcData.NOS`             |
| `NSpmData`  | raw     |      1 | `NSpmData.NOS`             |
| `NSppData`  | zlib-1  |     32 | `NSppData{chunk:02X}.NOS`  |
| `NStcData`  | zlib-9  |      1 | `NStcData.NOS`             |
| `NStgData`  | raw     |      4 | `NStgData{chunk:02X}.NOS`  |
| `NStgeData` | raw     |      1 | `NStgeData.NOS`            |
| `NStkData`  | raw     |      1 | `NStkData.NOS`             |
| `NStpData`  | raw     |     32 | `NStpData{chunk:02X}.NOS`  |
| `NStpeData` | raw     |      8 | `NStpeData{chunk:02X}.NOS` |
| `NStpuData` | raw     |      4 | `NStpuData{chunk:02X}.NOS` |
| `NStuData`  | zlib-9  |      1 | `NStuData.NOS`             |

`zlib-1` and `zlib-9` mean the zlib 1.1.2 default strategy at the indicated
level. All multi-chunk presets use low-byte routing.

### JSON Payload Commands

The following command groups share the same inspect/unpack/pack interface:

```text
taletool <GROUP> inspect <INPUT> [--json] [--checksum]
taletool <GROUP> unpack <INPUT> --out <OUTPUT.json>
taletool <GROUP> pack <INPUT.json> --out <OUTPUT>
```

| Group              | Payload family                                |
| ------------------ | --------------------------------------------- |
| `animation`        | `NSmcData` and `NSpcData` sprite animations.  |
| `map`              | `NStuData` scene settings and object trees.   |
| `ccinf`            | `NSmnData` and `NSpnData` GBFC indexes.       |
| `geometry`         | `NStgData` and `NStgeData` geometry.          |
| `height-grid`      | `NSgrdData` optimized height grids.           |
| `map-neighborhood` | `NStkData` neighbor maps and point sequences. |
| `sprite-remap`     | `NSpmData` frame/resource ordering tables.    |

`inspect` prints a summary without writing files. `unpack` writes an editable
JSON document, and `pack` rebuilds a payload from that document. The round-trip
editing workflow is to unpack, modify, and pack the generated document:

```console
taletool geometry inspect work/nstg/100.bin --json --checksum
taletool geometry unpack work/nstg/100.bin --out work/geometry-100.json
taletool geometry pack work/geometry-100.json --out work/nstg/100.bin
```

Effect payloads use the same pattern, with an additional kind selector on
`inspect` and `unpack`:

```text
taletool effect inspect <INPUT> [--kind <KIND>] [--json] [--checksum]
taletool effect unpack <INPUT> --out <OUTPUT.json> [--kind <KIND>]
taletool effect pack <INPUT.json> --out <OUTPUT>
```

`--kind` accepts `auto`, `color-animation` (`NSedData`), `definition`
(`NSeffData`), `transform-animation` (`NSemData`), or `texture-animation`
(`NSesData`). Automatic detection requires exactly one semantic format to match.
Packing reads the kind from the JSON document.

### Sprites

```text
taletool sprite inspect <INPUT> [--kind <KIND>] [--json] [--checksum]
taletool sprite unpack <INPUT> --out <OUTPUT> [--kind <KIND>] [--png-only]
taletool sprite pack <INPUT> --out <OUTPUT> [--kind <KIND>]
```

`--kind` accepts `auto`, `map-object`, or `free-size`:

- Map-object sprites are counted `A4R4G4B4` frame sets used by `NSmpData`,
  `NSppData`, and `NSipData`. Unpacking normally creates a directory containing
  `sprite.json` and `frame-NNN.png` files. Packing takes that directory.
  `--png-only` writes directly to one `.png` file and therefore requires a
  single-frame payload.
- Free-size sprites are single block-interlaced `A8R8G8B8` images used by
  `NS4BbData`. Unpacking writes directly to the `.png` path supplied by `--out`,
  and packing takes a PNG file.

For packing, `auto` selects map-object for a directory and free-size for a PNG
file. For inspection and unpacking, `auto` tries both decoders and requires
exactly one to match.

```console
taletool sprite unpack work/nsip/17.bin --out work/sprite-17
taletool sprite pack work/sprite-17 --out work/nsip/17.bin
taletool sprite unpack work/ns4bb/5.bin --kind free-size --out work/5.png
taletool sprite pack work/5.png --kind free-size --out work/ns4bb/5.bin
```

### Text Payloads

```text
taletool text inspect <PAYLOAD> [--kind <KIND>]
taletool text unpack <PAYLOAD> --out <OUTPUT> [--kind <KIND>]
  [--json] [--format <FORMAT>] [--encoding <ENCODING>]
taletool text pack <INPUT> --out <OUTPUT> [--kind <KIND>]
  [--json] [--format <FORMAT>] [--encoding <ENCODING>]
```

The payload kind controls the byte-level codec:

| Kind   | Automatic filename match | Behavior                        |
| ------ | ------------------------ | ------------------------------- |
| `dat`  | `.dat`, `.txt`           | NosTale compact DAT encoding.   |
| `list` | `.lst`                   | Length-prefixed LST line table. |
| `raw`  | Any other extension      | Copy bytes unchanged.           |

`--kind auto` is the default and uses the input filename for inspect/unpack or
the output filename for pack. For plain-text unpacking, an `--out` path with an
extension is used directly. A path without an extension is treated as a
directory and receives `<payload-stem>.txt`.

```console
taletool text inspect work/lang/_code_uk_Item.txt
taletool text unpack work/lang/_code_uk_Item.txt --out work/text/Item.txt
taletool text pack work/text/Item.txt --out work/lang/_code_uk_Item.txt
```

Add `--json` for structured language, constant-string, and NSetc string data.
`--format` accepts `auto`, `lang`, `cli`, or `etc` and may only be used with
`--json`. Language and constant-string documents require DAT payloads and use an
ordered `[[key, value], ...]` JSON shape; language keys are strings and
constant-string keys are signed integers. NSetc documents accept DAT or LST
payloads and use an ordered `[value, ...]` string array.

```console
taletool text unpack work/lang/_code_uk_Item.txt --out work/Item.json --json
taletool text pack work/Item.json --out work/lang/_code_uk_Item.txt --json

taletool text unpack work/cli/conststring.dat --out work/conststring.json \
  --json --encoding windows-1252
taletool text pack work/conststring.json --out work/cli/conststring.dat \
  --json --encoding windows-1252

taletool text unpack work/etc/MiniGame6WordData.dat \
  --out work/MiniGame6WordData.json --json
taletool text pack work/MiniGame6WordData.json \
  --out work/etc/MiniGame6WordData.dat --json

taletool text unpack work/etc/TabooStr.lst --out work/TabooStr.json --json
taletool text pack work/TabooStr.json --out work/etc/TabooStr.lst --json
```

`lang` is inferred from native `_code_<locale>_<table>.txt` names. Its encoding
is inferred as follows:

| Locales                 | Encoding     |
| ----------------------- | ------------ |
| `cz`, `de`, `it`, `pl`  | Windows-1250 |
| `ru`                    | Windows-1251 |
| `es`, `fr`, `uk`, `gsp` | Windows-1252 |
| `tr`                    | Windows-1254 |
| `hk`, `tw`              | Big5         |

Use `--encoding` for an unknown or renamed locale. `cli` is inferred from
`conststring.dat` but always requires `--encoding`. `etc` is inferred from
`MiniGame6WordData.dat` and `TabooStr.lst`; it defaults to EUC-KR and accepts an
encoding override. Accepted labels are `big5`,
`euc-kr`/`euckr`/`windows-949`/`cp949`, and `windows-1250` through
`windows-1254` (or the corresponding `cp1250`, `cp1251`, `cp1252`, and `cp1254`
aliases).

### Textures

```text
taletool texture inspect <INPUT> [--json] [--checksum]
taletool texture unpack <INPUT> --out <DIR>
taletool texture pack <DIR> --out <OUTPUT>
```

Unpacking creates `texture.json` and ordered `mip-NNN.png` images. Edit the PNG
files and manifest in place, then pass the directory back to `pack`. The
manifest preserves the pixel format, filtering fields, and stored mip count; PNG
dimensions determine the rebuilt dimensions and must form a valid mip chain.

```console
taletool texture unpack work/nstp/100.bin --out work/texture-100
taletool texture pack work/texture-100 --out work/nstp/100.bin
```

### Audio Metadata

The `audio` command handles `sndinfo.lst`. The audio data in `snd.pck` is
handled by `archive --type sound`.

```text
taletool audio inspect <INPUT> [--json] [--wave-dir <DIR>]
taletool audio unpack <INPUT> --out <OUTPUT.json>
taletool audio pack <INPUT.json> --out <OUTPUT>
```

`--wave-dir` resolves each metadata entry against loose files in the client's
wave directory and reports the resolved path or a missing entry. The generated
JSON preserves ordered keys, filename storage, unknown bytes, and trailing bytes
for round trips.

```console
taletool audio inspect sndinfo.lst --wave-dir "C:\NosTale\wave"
taletool audio unpack sndinfo.lst --out work/sndinfo.json
taletool audio pack work/sndinfo.json --out rebuilt/sndinfo.lst
```

### Map Cell Flags

```text
taletool cell-flag export-png <PAYLOAD> --out <OUTPUT.png> [--flag <FLAG>]
```

Without `--flag`, every distinct flag byte receives a stable color and the CLI
prints a value/color/count legend. With `--flag`, cells containing the selected
bit are black and all other cells are white.

Named flags are `walking-disabled`, `attack-through-disabled`, `unknown-04`,
`monster-aggro-disabled`, and `pvp-disabled`. Any single non-zero bit from
`0x01` through `0x80` can also be supplied in decimal or hexadecimal. Combined
masks such as `0x03` are rejected.

```console
taletool cell-flag export-png work/nstc/42.bin --out work/map-42-flags.png
taletool cell-flag export-png work/nstc/42.bin --out work/map-42-walls.png \
  --flag walking-disabled
```

### Patch Packages

`patch` (also available as `pak`) parses original NosTale `.PKG` patch packages.

```text
taletool patch inspect <PACKAGE>... [--json]
taletool patch apply --root <CLIENT_ROOT> <PACKAGE>...
  [--dry-run] [--backup-dir <DIR>]
```

Inspect packages before applying them, then perform a dry run against the target
client:

```console
taletool patch inspect "patches/*.PKG" --json
taletool patch apply --root "C:\NosTale" "patches/*.PKG" --dry-run
taletool patch apply --root "C:\NosTale" "patches/*.PKG" \
  --backup-dir "C:\NosTale-backups\update-1"
```

Package paths are wildcard-expanded, sorted, and deduplicated before parsing.
Because package operations are order-dependent, ensure that lexical path order
is the intended application order.

`--dry-run` resolves all operations and prints planned writes and removals
without changing the client. A real apply backs up replaced or removed files
before committing changes. Without `--backup-dir`, backups go below
`<CLIENT_ROOT>/.taletool/backups/run-<timestamp>-<pid>`. If a commit fails,
`taletool` attempts to roll back files already changed during that run.

## License

Taletool is licensed under the GNU Affero General Public License version 3 or
later (`AGPL-3.0-or-later`). See [LICENSE](LICENSE).

Third-party code keeps its original license. See [NOTICE.md](NOTICE.md).
