//! Original NosTale package parsing and patch application.
//!
//! The patch engine is storage-neutral: callers provide source bytes through a
//! loader and receive a complete in-memory change set. Filesystem commits are a
//! CLI concern, and Mirrortale can commit the same change set into blob storage.

pub mod apply;
mod binary_delta;
mod binary_nos_update;
mod checksum;
mod extract_ui_eff;
pub mod package;
mod paths;

pub use apply::{
    PatchApplyResult, PatchChangeSet, PatchFile, PatchSourceFile, PatchSourceLoader,
    apply_patch_operation, apply_patch_package, apply_patch_packages,
};
pub use binary_delta::apply_binary_delta;
pub use checksum::sha1_hex;
pub use extract_ui_eff::{
    EXTRACT_UI_EFF_SHA1, EXTRACT_UI_EFF_TARGET_PATH, NSTG_DATA_PATH, NSTP_DATA_PATH,
};
pub use package::{
    ParsedPchPkg, PchOperation, PchOperationKind, PchPackageDateTimeCode, PchPkgHeader,
    PchSegmentHeader, parse_pch_pkg,
};
pub use paths::normalize_client_path;
