//! Exercise archive extraction through the binary, including stdout reports.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use taletool_archive::{
    BinaryCompression, BinaryNosArchive, BinaryNosArchiveWriteEntry, BinaryNosArchiveWriteOptions,
};
use taletool_zlib::ZlibProfile;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "taletool-extract-cli-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn write_archive(&self, name: &str, id: i32, data: &[u8]) {
        let path = self.0.join(name);
        BinaryNosArchive::from_entries(
            path.clone(),
            vec![BinaryNosArchiveWriteEntry::new(id, data.to_vec())],
            &BinaryNosArchiveWriteOptions::new(
                [0; 16],
                0,
                BinaryCompression::Zlib,
                ZlibProfile::default_level(9),
            ),
        )
        .unwrap()
        .write_to(path)
        .unwrap();
    }

    fn extract(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_taletool"))
            .current_dir(&self.0)
            .args(["archive", "extract"])
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

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn extract_reports_found_missing_sizes_and_sha256_through_wildcard_inputs() {
    let fixture = Fixture::new();
    fixture.write_archive("NStgData00.NOS", 8, b"abc");
    fixture.write_archive("nstgdata02.nos", 6, b"");
    let report = stdout(fixture.extract(&[
        "NStgData*.NOS",
        "--id",
        "8",
        "5",
        "--id",
        "6",
        "8",
        "--out",
        "out",
    ]));
    let lines = report.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 4, "{report}");
    assert!(lines[0].starts_with(
        "found id=8 size=3 sha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad "
    ));
    assert!(lines[0].contains("stored_id=8 stored_size="));
    assert!(lines[0].contains(&format!(
        "archive={}",
        Path::new(".").join("NStgData00.NOS").display()
    )));
    assert_eq!(lines[1], "missing id=5 size=- sha256=-");
    assert!(lines[2].starts_with(
        "found id=6 size=0 sha256=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 "
    ));
    assert_eq!(lines[3], "found=2 missing=1");
    assert_eq!(fs::read(fixture.0.join("out/8.bin")).unwrap(), b"abc");
    assert_eq!(fs::read(fixture.0.join("out/6.bin")).unwrap(), b"");
    assert!(!fixture.0.join("out/5.bin").exists());
}

#[test]
fn extract_reads_id_files_and_rejects_bad_lists_without_output() {
    let fixture = Fixture::new();
    fixture.write_archive("custom.NOS", -1, b"abc");
    fs::write(fixture.0.join("ids.txt"), "# IDs\n4294967295 -1\n42\n").unwrap();
    let report = stdout(fixture.extract(&["custom.NOS", "--ids", "ids.txt", "--out", "out"]));
    assert!(report.starts_with("found id=-1 size=3 sha256="));
    assert!(report.contains("missing id=42 size=- sha256=-\nfound=1 missing=1"));
    assert_eq!(fs::read(fixture.0.join("out/-1.bin")).unwrap(), b"abc");

    fs::write(fixture.0.join("ids.txt"), "42\ninvalid\n").unwrap();
    let output = fixture.extract(&["custom.NOS", "--ids", "ids.txt", "--out", "bad-out"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ids.txt:2"));
    assert!(output.stdout.is_empty());
    assert!(!fixture.0.join(Path::new("bad-out")).exists());
}
