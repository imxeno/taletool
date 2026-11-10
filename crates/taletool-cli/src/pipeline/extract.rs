use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};
use taletool_archive::TextNosArchive;

#[derive(Serialize)]
struct Record {
    ordinal: usize,
    id: i32,
    name_hex: String,
    packed_flag: i32,
    payload: Object,
}
#[derive(Serialize)]
struct Object {
    digest: String,
    size: usize,
}
#[derive(Serialize)]
struct Manifest {
    version: u32,
    source: Object,
    version_date_bits: Option<String>,
    trailing_hex: String,
    records: Vec<Record>,
}

pub(super) fn run(input: &Path, out: &Path) -> anyhow::Result<()> {
    let source = fs::read(input)?;
    let archive = TextNosArchive::from_bytes(input.to_owned(), source.clone())?;
    crate::output::write_output_transactionally(out, |staging| {
        fs::create_dir_all(staging.join("payloads"))?;
        let mut written = BTreeSet::new();
        let mut records = Vec::new();
        for (ordinal, record) in archive.records().iter().enumerate() {
            let digest = format!("{:x}", Sha256::digest(&record.payload));
            if written.insert(digest.clone()) {
                fs::write(staging.join("payloads").join(&digest), &record.payload)?;
            }
            records.push(Record {
                ordinal,
                id: record.id,
                name_hex: hex::encode(&record.name_bytes),
                packed_flag: record.packed_flag,
                payload: Object {
                    digest,
                    size: record.payload.len(),
                },
            });
        }
        let manifest = Manifest {
            version: 1,
            source: Object {
                digest: format!("{:x}", Sha256::digest(&source)),
                size: source.len(),
            },
            version_date_bits: archive
                .timestamp()
                .map(|t| format!("{:016x}", t.variant.to_bits())),
            trailing_hex: hex::encode(archive.trailing_data()),
            records,
        };
        fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec(&manifest)?,
        )?;
        Ok(())
    })
}
