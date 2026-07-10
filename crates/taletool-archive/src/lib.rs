//! Archive format support for NosTale data files.
//!
//! This crate owns archive/container parsing and rewriting. It deliberately
//! does not know about structured asset formats, patch packages, patch opcodes,
//! package ordering, or partial-apply policy. Code that interprets PCHPKG
//! operations belongs in `taletool-patch` and should use the neutral
//! read/edit/write APIs exposed here.
//!
//! Several unrelated NosTale archive families use the `.NOS` extension. Public
//! type names therefore describe the format shape instead of claiming `.NOS` as
//! a single archive kind. The root module re-exports the stable API used by the
//! CLI and by `taletool-patch`; new code may also import from the format
//! modules directly:
//!
//! - [`binary`] for numeric-ID binary `.NOS` table/chunk archives.
//! - [`text`] for named-record text `.NOS` archives.
//! - [`deldx`] for DelDX pack files such as `snd.pck`.

pub mod binary;
pub mod deldx;
pub mod text;

pub use binary::{
    BinaryCompression, BinaryEntryPayload, BinaryNosArchive, BinaryNosArchiveEntry,
    BinaryNosArchiveError, BinaryNosArchiveRecord, BinaryNosArchiveResult,
    BinaryNosArchiveWriteEntry, BinaryNosArchiveWriteOptions, BinaryNosSplitArchive,
    write_binary_nos_archive_bytes,
};
pub use deldx::{
    DELDX_PACK_HEADER_LEN, DELDX_PACK_RESERVED_HEADER_LEN, DELDX_PACK_RESERVED_HEADER_OFFSET,
    DELDX_PACK_ROW_LEN, DELDX_PACK_ROW_PREFIX_LEN, DelDxPack, DelDxPackEntry, DelDxPackError,
    DelDxPackRecord, DelDxPackResult, DelDxPackWriteEntry, DelDxPackWriteOptions,
    PackedArchiveMutation, PackedMutationRecord, apply_packed_archive_mutation,
    normalize_deldx_pack_header_for_write, write_deldx_pack_bytes,
};
pub use text::{
    TextNosArchive, TextNosArchiveError, TextNosArchiveResult, TextNosArchiveTimestamp,
    TextNosRecord, TextNosRecordInput, write_text_nos_archive_bytes,
};
