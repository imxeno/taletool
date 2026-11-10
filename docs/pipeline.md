# Content-addressed pipeline commands

The `pipeline` command group separates text archive extraction from typed record
parsing. Callers can cache these operations independently of the CLI binary
version.

## Component identities

`pipeline components` writes a versioned JSON report to standard output. Each
component contains an output schema version, SHA-256 implementation fingerprint,
named source dependencies, relevant locked external dependencies and dependency
settings. IDs are `extract.text` and `parse.<grammar>`.

Fingerprints use repository-relative source names and bytes, so the checkout's
location does not affect them. `build.rs` declares each component's source
closure. Shared modules belong to every component that calls them; unrelated
archive and rendering implementations are excluded. The GTD grammars currently
share source modules and therefore invalidate together when those modules
change. Text extraction excludes those parsing modules.

When adding implementation modules or dependencies, update these closures and
check the component report. Change the fingerprint algorithm's domain version
when its meaning changes. Update the component schema when its output contract
changes. The source list describes an audited closure; it is not a Rust call
graph inferred by the compiler.

## Text archive extraction

`pipeline extract-text` takes an input archive path and a required `--out`
directory. The output directory must not exist. Publication is transactional:
errors do not leave a completed output directory.

The directory contains `manifest.json` and `payloads/<sha256>` files. Equal
payloads are written once. The version 1 manifest contains:

- `source`: SHA-256 digest and byte length of the original archive.
- `records`: every record in order, with `ordinal`, original integer `id`,
  hexadecimal `name_hex`, original `packed_flag` and payload digest/size.
- `version_date_bits`: the timestamp's exact 64 bits as 16 hexadecimal
  characters, or `null` when absent.
- `trailing_hex`: bytes after the optional timestamp trailer.

Names are stored as bytes so non-UTF-8 names and duplicate IDs remain distinct.
Payload bytes are untouched. Text decoding, grammar parsing and filename
normalization do not happen at this boundary.

## Typed text parsing

`pipeline parse-text` takes an input payload path and requires `--parser`,
`--kind`, `--encoding` and `--out`. The grammar is explicit; an opaque input
filename is supported. `--kind` accepts `dat` or `raw`. Encodings use the same
labels as the other text commands.

Supported parsers are `item`, `bcard`, `card`, `skill`, `monster`, `quest`,
`quest-prize`, `quest-npc`, `map-ids`, `language` and `constants`. GTD output
retains the complete structured JSON document. Language and constant-string
output uses the existing ordered pair representation, including duplicate keys.
Diagnostics are written to standard error. The output file is replaced only
after successful parsing and writing.

Cache keys must include the selected component's schema and fingerprint, exact
payload hash, grammar, kind and encoding. Release metadata belongs outside
parsed content. When a parser change produces equal output bytes, consumers can
reuse artifacts keyed by that output's hash.
