//! Verify the content-addressed pipeline contract through the CLI.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
use taletool_archive::{
    TextNosArchiveWriteOptions, TextNosRecordInput, write_text_nos_archive_bytes,
};
use taletool_text::encode_dat_payload;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "taletool-pipeline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_taletool"))
            .current_dir(&self.0)
            .args(["pipeline"])
            .args(args)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn extraction_preserves_order_duplicates_name_bytes_and_exact_trailer_bits() {
    let fixture = Fixture::new();
    let data = encode_dat_payload(b"same\n").unwrap();
    let records = [
        TextNosRecordInput {
            id: 3,
            name: "first.dat".into(),
            name_bytes: b"first.dat".to_vec(),
            packed_flag: 1,
            payload: data.clone(),
        },
        TextNosRecordInput {
            id: 3,
            name: "invalid".into(),
            name_bytes: vec![0xff, 0xfe],
            packed_flag: 7,
            payload: data.clone(),
        },
    ];
    let variant = f64::from_bits(0x7ff8000000000001);
    let source = write_text_nos_archive_bytes(
        &records,
        &TextNosArchiveWriteOptions {
            version_date: Some(variant),
            trailing_data: vec![1, 2, 3],
        },
    )
    .unwrap();
    fs::write(fixture.0.join("source.NOS"), &source).unwrap();
    let result = fixture.command(&["extract-text", "source.NOS", "--out", "out"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(fixture.0.join("out/manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["version_date_bits"], "7ff8000000000001");
    assert_eq!(manifest["trailing_hex"], "010203");
    assert_eq!(
        manifest["source"]["digest"],
        format!("{:x}", Sha256::digest(&source))
    );
    assert_eq!(manifest["records"][0]["ordinal"], 0);
    assert_eq!(manifest["records"][1]["ordinal"], 1);
    assert_eq!(manifest["records"][1]["id"], 3);
    assert_eq!(manifest["records"][1]["name_hex"], "fffe");
    assert_eq!(manifest["records"][1]["packed_flag"], 7);
    assert_eq!(
        fs::read_dir(fixture.0.join("out/payloads"))
            .unwrap()
            .count(),
        1
    );
    let digest = manifest["records"][0]["payload"]["digest"]
        .as_str()
        .unwrap();
    assert_eq!(
        fs::read(fixture.0.join("out/payloads").join(digest)).unwrap(),
        data
    );
}
#[test]
fn explicit_parser_is_independent_of_the_payload_filename_and_retains_full_item_fields() {
    let fixture = Fixture::new();
    let text =
        "VNUM 7 100\nNAME zts1e\nINDEX 0 0 0 0 3 0\nTYPE 0 1\nFLAG 0\nDATA 0\nBUFF 0\nLINEDESC 0\n";
    fs::write(
        fixture.0.join("opaque-payload"),
        encode_dat_payload(text.as_bytes()).unwrap(),
    )
    .unwrap();
    let output = fixture.command(&[
        "parse-text",
        "opaque-payload",
        "--parser",
        "item",
        "--kind",
        "dat",
        "--encoding",
        "windows-1252",
        "--out",
        "item.json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&fs::read(fixture.0.join("item.json")).unwrap()).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["kind"], "item");
    assert_eq!(document["entries"][0]["vnum"], 7);
    assert_eq!(document["entries"][0]["price"], 100);
    assert_eq!(document["entries"][0]["index"][4], 3);
    assert_eq!(document["entries"][0]["name"], "zts1e");
}
#[test]
fn components_report_narrow_source_dependencies_and_stable_fingerprints() {
    let fixture = Fixture::new();
    let first = fixture.command(&["components"]);
    let second = fixture.command(&["components"]);
    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
    let report: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["version"], 1);
    let components = report["components"].as_object().unwrap();
    for component in components.values() {
        let fingerprint = component["fingerprint"].as_str().unwrap();
        assert_eq!(fingerprint.len(), 64);
        assert!(fingerprint.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(component["schema"], 1);
    }
    let extraction = serde_json::to_string(&components["extract.text"]["sources"]).unwrap();
    assert!(!extraction.contains("taletool-text"));
    assert!(!extraction.contains("pipeline/parse.rs"));
    let language = serde_json::to_string(&components["parse.language"]["sources"]).unwrap();
    assert!(!language.contains("/gtd/"));
    let item = serde_json::to_string(&components["parse.item"]["sources"]).unwrap();
    assert!(item.contains("/gtd/entity.rs"));
    assert!(!item.contains("pipeline/extract.rs"));
}
#[test]
fn corrupt_input_and_invalid_encoding_do_not_publish_partial_outputs() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("broken.NOS"), [1, 2]).unwrap();
    let output = fixture.command(&["extract-text", "broken.NOS", "--out", "out"]);
    assert!(!output.status.success());
    assert!(!fixture.0.join("out").exists());
    fs::write(fixture.0.join("payload"), b"plain").unwrap();
    fs::write(fixture.0.join("keep.json"), b"keep").unwrap();
    let output = fixture.command(&[
        "parse-text",
        "payload",
        "--parser",
        "language",
        "--kind",
        "raw",
        "--encoding",
        "invalid-encoding",
        "--out",
        "keep.json",
    ]);
    assert!(!output.status.success());
    assert_eq!(fs::read(fixture.0.join("keep.json")).unwrap(), b"keep");
}
