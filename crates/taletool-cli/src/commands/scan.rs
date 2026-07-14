//! Handler for `taletool scan`.
//!
//! Scanning walks a data directory recursively, attempts format detection for
//! supported data files, and reports a concise classification.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use taletool_ccinf::Ccinf;
use taletool_core::validate_data_dir;

use crate::archive_detect::{DetectedArchive, detect_archive_paths, has_ccinf_header};
use crate::cli::ArchiveType;

const UNSUPPORTED_ARCHIVE_TYPE: &str = "unsupported";
const MEDIA_HEADER_LEN: usize = 12;

#[derive(Clone, Copy)]
enum MediaFormat {
    Ogg,
    Mp3,
    Wav,
    Mpeg,
}

impl MediaFormat {
    fn scan_type(self) -> &'static str {
        match self {
            Self::Ogg | Self::Mp3 | Self::Wav => "audio",
            Self::Mpeg => "video",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Ogg => "ogg",
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Mpeg => "mpeg",
        }
    }
}

/// Scan a data directory and print either human-readable or JSON output.
pub(crate) fn run_scan(
    data_dir: PathBuf,
    verbose: bool,
    json_output: bool,
    show_unsupported: bool,
    recursive: bool,
) -> anyhow::Result<()> {
    let data_dir = validate_data_dir(data_dir)?;
    let files = scan_data_dir(&data_dir, verbose, show_unsupported, recursive)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&files)?);
    } else {
        println!("data_dir: {}", data_dir.display());
        println!("files: {}", files.len());
        for file in &files {
            match &file.details {
                Some(details) => println!(
                    "  {:<24} type={:<7} {}",
                    file.file,
                    file.archive_type.as_deref().unwrap_or("unknown"),
                    details
                ),
                None => println!(
                    "  {:<24} type={}",
                    file.file,
                    file.archive_type.as_deref().unwrap_or("unknown")
                ),
            }
        }
    }
    Ok(())
}

fn scan_data_dir(
    data_dir: &Path,
    verbose: bool,
    show_unsupported: bool,
    recursive: bool,
) -> anyhow::Result<Vec<ScanFile>> {
    let mut files = Vec::new();
    scan_directory(
        data_dir,
        data_dir,
        verbose,
        show_unsupported,
        recursive,
        &mut files,
    )?;
    files.sort_by(|left, right| left.file.cmp(&right.file));
    Ok(files)
}

fn scan_directory(
    data_dir: &Path,
    directory: &Path,
    verbose: bool,
    show_unsupported: bool,
    recursive: bool,
    files: &mut Vec<ScanFile>,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() && recursive {
            scan_directory(data_dir, &path, verbose, show_unsupported, recursive, files)?;
        } else if file_type.is_file() {
            let scanned = match detect_media_format(&path) {
                Ok(Some(format)) => scan_media_file(data_dir, &path, format, verbose),
                Ok(None) if is_scannable_data_file(&path) => scan_file(data_dir, &path, verbose),
                Ok(None) => unsupported_file(data_dir, &path, None),
                Err(error) => unsupported_file(data_dir, &path, Some(error.to_string())),
            };
            if show_unsupported || !scanned.is_unsupported() {
                files.push(scanned);
            }
        }
    }
    Ok(())
}

/// Serializable result for one file found during `scan`.
#[derive(Debug, Serialize)]
struct ScanFile {
    file: String,
    archive_type: Option<String>,
    details: Option<String>,
    error: Option<String>,
}

impl ScanFile {
    fn is_unsupported(&self) -> bool {
        self.archive_type.as_deref() == Some(UNSUPPORTED_ARCHIVE_TYPE)
    }
}

/// Classify a supported data file using the same format policy as commands.
fn scan_file(data_dir: &Path, path: &Path, verbose: bool) -> ScanFile {
    let file = relative_file_name(data_dir, path);
    if has_ccinf_header(path) {
        return match Ccinf::open(path) {
            Ok(ccinf) => ScanFile {
                file,
                archive_type: Some("ccinf".to_owned()),
                details: verbose.then(|| format!("entries={}", ccinf.entries().len())),
                error: None,
            },
            Err(error) => ScanFile {
                file,
                archive_type: Some("ccinf".to_owned()),
                details: None,
                error: Some(error.to_string()),
            },
        };
    }

    match detect_archive_paths(&[path.to_path_buf()], ArchiveType::Auto) {
        Ok(DetectedArchive::Binary(archives)) => {
            let entry_count: usize = archives.iter().map(|archive| archive.entries().len()).sum();
            ScanFile {
                file,
                archive_type: Some("binary".to_owned()),
                details: verbose.then(|| format!("entries={entry_count}")),
                error: None,
            }
        }
        Ok(DetectedArchive::Text(archive)) => ScanFile {
            file,
            archive_type: Some("text".to_owned()),
            details: verbose.then(|| format!("records={}", archive.records().len())),
            error: None,
        },
        Ok(DetectedArchive::Sound(archive)) => ScanFile {
            file,
            archive_type: Some("sound".to_owned()),
            details: verbose.then(|| format!("entries={}", archive.entries().len())),
            error: None,
        },
        Err(error) => unsupported_file(data_dir, path, Some(error.to_string())),
    }
}

fn unsupported_file(data_dir: &Path, path: &Path, error: Option<String>) -> ScanFile {
    ScanFile {
        file: relative_file_name(data_dir, path),
        archive_type: Some(UNSUPPORTED_ARCHIVE_TYPE.to_owned()),
        details: None,
        error,
    }
}

fn scan_media_file(data_dir: &Path, path: &Path, format: MediaFormat, verbose: bool) -> ScanFile {
    ScanFile {
        file: relative_file_name(data_dir, path),
        archive_type: Some(format.scan_type().to_owned()),
        details: verbose.then(|| format!("format={}", format.name())),
        error: None,
    }
}

fn relative_file_name(data_dir: &Path, path: &Path) -> String {
    path.strip_prefix(data_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn detect_media_format(path: &Path) -> std::io::Result<Option<MediaFormat>> {
    let mut file = fs::File::open(path)?;
    let mut header = [0; MEDIA_HEADER_LEN];
    let mut bytes_read = 0;
    while bytes_read < header.len() {
        let read = file.read(&mut header[bytes_read..])?;
        if read == 0 {
            break;
        }
        bytes_read += read;
    }
    Ok(media_format(&header[..bytes_read]))
}

fn media_format(header: &[u8]) -> Option<MediaFormat> {
    if header.len() >= 12 && header.starts_with(b"RIFF") && &header[8..12] == b"WAVE" {
        Some(MediaFormat::Wav)
    } else if header.len() >= 5 && header.starts_with(b"OggS") && header[4] == 0 {
        Some(MediaFormat::Ogg)
    } else if has_id3v2_header(header) || has_mp3_frame_header(header) {
        Some(MediaFormat::Mp3)
    } else if header.starts_with(&[0x00, 0x00, 0x01, 0xba])
        || header.starts_with(&[0x00, 0x00, 0x01, 0xb3])
    {
        Some(MediaFormat::Mpeg)
    } else {
        None
    }
}

fn has_id3v2_header(header: &[u8]) -> bool {
    header.len() >= 10
        && header.starts_with(b"ID3")
        && header[3] != 0xff
        && header[4] != 0xff
        && header[6..10].iter().all(|byte| byte & 0x80 == 0)
}

fn has_mp3_frame_header(header: &[u8]) -> bool {
    if header.len() < 4 {
        return false;
    }
    let version = (header[1] >> 3) & 0x03;
    let layer = (header[1] >> 1) & 0x03;
    let bitrate = (header[2] >> 4) & 0x0f;
    let sample_rate = (header[2] >> 2) & 0x03;
    header[0] == 0xff
        && header[1] & 0xe0 == 0xe0
        && version != 0x01
        && layer == 0x01
        && !matches!(bitrate, 0x00 | 0x0f)
        && sample_rate != 0x03
}

fn is_scannable_data_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|extension| {
            extension.eq_ignore_ascii_case("nos") || extension.eq_ignore_ascii_case("pck")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use taletool_archive::{
        BinaryCompression, BinaryNosArchiveWriteEntry, BinaryNosArchiveWriteOptions,
        write_binary_nos_archive_bytes,
    };
    use taletool_ccinf::write_ccinf_bytes;
    use taletool_zlib::ZlibProfile;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("taletool-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn scan_classifies_ccinf_as_a_supported_data_file() {
        let root = temp_dir("ccinf-scan");
        let path = root.join("NSmnData.NOS");
        fs::create_dir_all(&root).unwrap();
        fs::write(&path, write_ccinf_bytes(&[]).unwrap()).unwrap();

        let scanned = scan_file(&root, &path, true);
        assert_eq!(scanned.file, "NSmnData.NOS");
        assert_eq!(scanned.archive_type.as_deref(), Some("ccinf"));
        assert_eq!(scanned.details.as_deref(), Some("entries=0"));
        assert!(scanned.error.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn binary_scan_reports_entries_without_a_per_file_chunk_count() {
        let root = temp_dir("binary-scan-details");
        let path = root.join("NSmpData00.NOS");
        fs::create_dir_all(&root).unwrap();
        let bytes = write_binary_nos_archive_bytes(
            &[
                BinaryNosArchiveWriteEntry::new(1, b"first".to_vec()),
                BinaryNosArchiveWriteEntry::new(2, b"second".to_vec()),
            ],
            &BinaryNosArchiveWriteOptions::new(
                *b"NT Data 06\0\0\x15\x07\x04 ",
                0,
                BinaryCompression::Raw,
                ZlibProfile::default_level(9),
            ),
        )
        .unwrap();
        fs::write(&path, bytes).unwrap();

        let scanned = scan_file(&root, &path, true);
        assert_eq!(scanned.archive_type.as_deref(), Some("binary"));
        assert_eq!(scanned.details.as_deref(), Some("entries=2"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursive_scan_hides_unsupported_files_by_default() {
        let root = temp_dir("recursive-scan-default");
        let supported_dir = root.join("supported");
        let unsupported_dir = root.join("unsupported");
        fs::create_dir_all(&supported_dir).unwrap();
        fs::create_dir_all(&unsupported_dir).unwrap();
        fs::write(
            supported_dir.join("NSmnData.NOS"),
            write_ccinf_bytes(&[]).unwrap(),
        )
        .unwrap();
        fs::write(unsupported_dir.join("notes.txt"), b"notes").unwrap();
        fs::write(unsupported_dir.join("broken.NOS"), b"not an archive").unwrap();

        let scanned = scan_data_dir(&root, false, false, true).unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(
            scanned[0].file,
            Path::new("supported")
                .join("NSmnData.NOS")
                .to_string_lossy()
        );
        assert_eq!(scanned[0].archive_type.as_deref(), Some("ccinf"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursive_scan_can_show_every_unsupported_file() {
        let root = temp_dir("recursive-scan-unsupported");
        let supported_dir = root.join("supported");
        let unsupported_dir = root.join("unsupported");
        fs::create_dir_all(&supported_dir).unwrap();
        fs::create_dir_all(&unsupported_dir).unwrap();
        fs::write(
            supported_dir.join("NSmnData.NOS"),
            write_ccinf_bytes(&[]).unwrap(),
        )
        .unwrap();
        fs::write(unsupported_dir.join("notes.txt"), b"notes").unwrap();
        fs::write(unsupported_dir.join("broken.NOS"), b"not an archive").unwrap();

        let scanned = scan_data_dir(&root, false, true, true).unwrap();
        let expected_files = [
            Path::new("supported").join("NSmnData.NOS"),
            Path::new("unsupported").join("broken.NOS"),
            Path::new("unsupported").join("notes.txt"),
        ]
        .map(|path| path.to_string_lossy().into_owned());
        assert_eq!(
            scanned
                .iter()
                .map(|file| file.file.as_str())
                .collect::<Vec<_>>(),
            expected_files
        );
        assert_eq!(scanned[0].archive_type.as_deref(), Some("ccinf"));
        assert!(scanned[0].error.is_none());
        assert!(scanned[1].is_unsupported());
        assert!(scanned[1].error.is_some());
        assert!(scanned[2].is_unsupported());
        assert!(scanned[2].error.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_can_be_limited_to_the_immediate_directory() {
        let root = temp_dir("non-recursive-scan");
        let nested_dir = root.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(root.join("root.NOS"), write_ccinf_bytes(&[]).unwrap()).unwrap();
        fs::write(
            nested_dir.join("nested.NOS"),
            write_ccinf_bytes(&[]).unwrap(),
        )
        .unwrap();

        let scanned = scan_data_dir(&root, false, false, false).unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].file, "root.NOS");
        assert_eq!(scanned[0].archive_type.as_deref(), Some("ccinf"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_shows_audio_and_mpeg_video_files_by_default() {
        let root = temp_dir("media-scan");
        fs::create_dir_all(&root).unwrap();
        for (name, contents) in [
            ("effect.av", b"RIFF\x04\x00\x00\x00WAVE".as_slice()),
            ("intro.nam", b"\x00\x00\x01\xba MPEG".as_slice()),
            ("music.30000", b"OggS\x00\x02 audio".as_slice()),
            ("raw-layer3.data", b"\xff\xfb\x90\x64 audio".as_slice()),
            ("sequence.bin", b"\x00\x00\x01\xb3 video".as_slice()),
            ("song.data", b"ID3\x03\x00\x00\x00\x00\x00\x00".as_slice()),
            ("fake.ogg", b"not ogg".as_slice()),
            ("fake.mp3", b"not mp3".as_slice()),
            ("fake.wav", b"RIFF\x00\x00\x00\x00NOPE".as_slice()),
            ("fake.ntm", b"not mpeg".as_slice()),
        ] {
            fs::write(root.join(name), contents).unwrap();
        }

        let scanned = scan_data_dir(&root, true, false, true).unwrap();
        let actual = scanned
            .iter()
            .map(|file| {
                (
                    file.file.as_str(),
                    file.archive_type.as_deref().unwrap(),
                    file.details.as_deref().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                ("effect.av", "audio", "format=wav"),
                ("intro.nam", "video", "format=mpeg"),
                ("music.30000", "audio", "format=ogg"),
                ("raw-layer3.data", "audio", "format=mp3"),
                ("sequence.bin", "video", "format=mpeg"),
                ("song.data", "audio", "format=mp3"),
            ]
        );

        let expanded = scan_data_dir(&root, false, true, true).unwrap();
        let misleading_extensions = expanded
            .iter()
            .filter(|file| file.file.starts_with("fake."))
            .collect::<Vec<_>>();
        assert_eq!(misleading_extensions.len(), 4);
        assert!(
            misleading_extensions
                .iter()
                .all(|file| file.is_unsupported())
        );
        fs::remove_dir_all(root).unwrap();
    }
}
