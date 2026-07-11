//! Handlers for `taletool text` payload commands.
//!
//! These commands operate on individual payload files, not full text archives.
//! Archive-level packing remains in the `archive` command module.

use std::fs;

use crate::cli::TextCommand;
use crate::text_payload::{
    decode_text_payload, encode_text_payload, payload_kind_label, preview_text,
    resolve_text_payload_kind, text_output_path,
};

/// Dispatch a `text` subcommand.
pub(crate) fn run_text(command: TextCommand) -> anyhow::Result<()> {
    match command {
        TextCommand::Inspect { payload, kind } => {
            let data = fs::read(&payload)?;
            let kind = resolve_text_payload_kind(&payload, kind);
            println!("kind: {}", payload_kind_label(kind));
            println!("encoded_size: {}", data.len());
            match decode_text_payload(kind, &data) {
                Ok(decoded) => {
                    println!("decoded_size: {}", decoded.len());
                    println!("preview: {}", preview_text(&decoded));
                }
                Err(error) => println!("decode_error: {error}"),
            }
            Ok(())
        }
        TextCommand::Unpack { payload, out, kind } => {
            let data = fs::read(&payload)?;
            let kind = resolve_text_payload_kind(&payload, kind);
            let decoded = decode_text_payload(kind, &data)?;
            let path = text_output_path(&payload, &out);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, decoded)?;
            println!("unpacked {}", path.display());
            Ok(())
        }
        TextCommand::Pack { input, out, kind } => {
            let decoded = fs::read(&input)?;
            let kind = resolve_text_payload_kind(&out, kind);
            let encoded = encode_text_payload(kind, &decoded)?;
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out, encoded)?;
            println!("packed {}", out.display());
            Ok(())
        }
    }
}
