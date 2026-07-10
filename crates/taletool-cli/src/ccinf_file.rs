//! JSON and file helpers for structured CCINF `.NOS` assets.

use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use taletool_archive::{CCINF_NOS_HEADER, CcinfNosArchive, CcinfNosArchiveEntry};

const CCINF_DOCUMENT_FORMAT: &str = "ccinf";
const CCINF_DOCUMENT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CcinfDocument {
    format: String,
    version: u32,
    entries: Vec<CcinfNosArchiveEntry>,
}

/// Return whether a file begins with the canonical CCINF signature.
///
/// This intentionally checks only the signature. Dedicated CCINF commands run
/// the complete parser and report structural errors from the rest of the file.
pub(crate) fn has_ccinf_header(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut header = [0; CCINF_NOS_HEADER.len()];
    file.read_exact(&mut header).is_ok() && header == CCINF_NOS_HEADER
}

/// Decode one CCINF file into a strict, versioned JSON document.
pub(crate) fn unpack_ccinf_file(archive: &CcinfNosArchive, out: &Path) -> anyhow::Result<usize> {
    let document = CcinfDocument {
        format: CCINF_DOCUMENT_FORMAT.to_owned(),
        version: CCINF_DOCUMENT_VERSION,
        entries: archive.entries().to_vec(),
    };
    create_parent_dir(out)?;
    fs::write(out, serde_json::to_vec_pretty(&document)?)?;
    Ok(document.entries.len())
}

/// Encode a strict JSON document into a canonical raw CCINF file.
pub(crate) fn pack_ccinf_file(input: &Path, out: &Path) -> anyhow::Result<CcinfNosArchive> {
    let document_bytes = fs::read(input).with_context(|| format!("reading {}", input.display()))?;
    let mut document: CcinfDocument = serde_json::from_slice(&document_bytes)
        .with_context(|| format!("parsing {}", input.display()))?;
    validate_document_header(&document)?;

    for entry in &mut document.entries {
        for cells in &mut entry.cell_lists {
            cells.sort_by_key(|cell| cell.selector);
        }
    }
    document.entries.sort_by_key(|entry| entry.entry_id as u32);

    let archive = CcinfNosArchive::from_entries(out.to_path_buf(), document.entries)?;
    create_parent_dir(out)?;
    archive.write_to(out)?;
    Ok(archive)
}

fn validate_document_header(document: &CcinfDocument) -> anyhow::Result<()> {
    if document.format != CCINF_DOCUMENT_FORMAT {
        bail!(
            "CCINF document has unsupported format {:?}; expected {:?}",
            document.format,
            CCINF_DOCUMENT_FORMAT
        );
    }
    if document.version != CCINF_DOCUMENT_VERSION {
        bail!(
            "CCINF document has unsupported version {}; expected {}",
            document.version,
            CCINF_DOCUMENT_VERSION
        );
    }
    Ok(())
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use taletool_archive::{CCINF_NOS_CELL_LIST_COUNT, CcinfNosCell};

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    fn entry(entry_id: i32, selectors: &[u16]) -> CcinfNosArchiveEntry {
        let mut cell_lists = std::array::from_fn(|_| Vec::new());
        cell_lists[0] = selectors
            .iter()
            .map(|selector| CcinfNosCell {
                selector: *selector,
                texture_resource_key: i32::from(*selector) * 10,
            })
            .collect();
        CcinfNosArchiveEntry {
            entry_id,
            base_resource_key: entry_id + 1,
            remap_table_file_id: entry_id + 2,
            animation_file_id: entry_id + 3,
            cell_lists,
        }
    }

    #[test]
    fn direct_json_unpack_and_pack_round_trip() {
        let root = temp_dir("ccinf-round-trip");
        let json_path = root.join("NSpnData.json");
        let output = root.join("NSpnData.NOS");
        fs::create_dir_all(&root).unwrap();
        let source = CcinfNosArchive::from_entries(
            root.join("source.NOS"),
            vec![entry(1, &[2, 3]), entry(7, &[4])],
        )
        .unwrap();

        assert_eq!(unpack_ccinf_file(&source, &json_path).unwrap(), 2);
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert_eq!(document["format"], CCINF_DOCUMENT_FORMAT);
        assert_eq!(document["version"], CCINF_DOCUMENT_VERSION);
        assert_eq!(
            document["entries"][0]["cell_lists"]
                .as_array()
                .unwrap()
                .len(),
            CCINF_NOS_CELL_LIST_COUNT
        );

        let rebuilt = pack_ccinf_file(&json_path, &output).unwrap();
        assert_eq!(rebuilt.entries(), source.entries());
        assert_eq!(rebuilt.as_bytes(), source.as_bytes());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pack_canonicalizes_entry_and_cell_order() {
        let root = temp_dir("ccinf-sorting");
        let json_path = root.join("input.json");
        fs::create_dir_all(&root).unwrap();
        let document = CcinfDocument {
            format: CCINF_DOCUMENT_FORMAT.to_owned(),
            version: CCINF_DOCUMENT_VERSION,
            entries: vec![entry(-1, &[3, 1, 2]), entry(7, &[5, 4])],
        };
        fs::write(&json_path, serde_json::to_vec(&document).unwrap()).unwrap();

        let archive = pack_ccinf_file(&json_path, &root.join("NSmnData.NOS")).unwrap();
        assert_eq!(archive.entries()[0].entry_id, 7);
        assert_eq!(archive.entries()[1].entry_id, -1);
        assert_eq!(
            archive.entries()[0].cell_lists[0]
                .iter()
                .map(|cell| cell.selector)
                .collect::<Vec<_>>(),
            vec![4, 5]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_or_non_strict_documents() {
        let root = temp_dir("ccinf-invalid-document");
        let json_path = root.join("input.json");
        fs::create_dir_all(&root).unwrap();

        fs::write(
            &json_path,
            json!({
                "format": CCINF_DOCUMENT_FORMAT,
                "version": 999,
                "entries": []
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_ccinf_file(&json_path, &root.join("bad.NOS")).is_err());

        fs::write(
            &json_path,
            json!({
                "format": CCINF_DOCUMENT_FORMAT,
                "version": CCINF_DOCUMENT_VERSION,
                "entries": [],
                "header_hex": "not-supported"
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_ccinf_file(&json_path, &root.join("bad.NOS")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_cell_list_shape_and_oversized_lists() {
        let root = temp_dir("ccinf-cell-lists");
        let json_path = root.join("input.json");
        fs::create_dir_all(&root).unwrap();
        let shaped_entry = entry(1, &[1]);
        let mut entry_json = serde_json::to_value(shaped_entry).unwrap();
        entry_json["cell_lists"] = json!([[]]);
        fs::write(
            &json_path,
            json!({
                "format": CCINF_DOCUMENT_FORMAT,
                "version": CCINF_DOCUMENT_VERSION,
                "entries": [entry_json]
            })
            .to_string(),
        )
        .unwrap();
        assert!(pack_ccinf_file(&json_path, &root.join("bad.NOS")).is_err());

        let mut oversized = entry(1, &[]);
        oversized.cell_lists[CCINF_NOS_CELL_LIST_COUNT - 1] = vec![
            CcinfNosCell {
                selector: 1,
                texture_resource_key: 2,
            };
            256
        ];
        fs::write(
            &json_path,
            serde_json::to_vec(&CcinfDocument {
                format: CCINF_DOCUMENT_FORMAT.to_owned(),
                version: CCINF_DOCUMENT_VERSION,
                entries: vec![oversized],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(pack_ccinf_file(&json_path, &root.join("bad.NOS")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_only_the_canonical_header() {
        let root = temp_dir("ccinf-header");
        let path = root.join("input.NOS");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, CCINF_NOS_HEADER).unwrap();
        assert!(has_ccinf_header(&path));

        fs::write(&path, b"not ccinf").unwrap();
        assert!(!has_ccinf_header(&path));
        fs::remove_dir_all(root).unwrap();
    }
}
