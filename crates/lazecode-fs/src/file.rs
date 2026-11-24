use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{FsError, Result};

/// Default limit for an editable whole-file read (matches the previous product).
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Hard limit even with an explicit override.
pub const MAX_FORCE_FILE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Eol {
    #[default]
    Lf,
    CrLf,
}

impl Eol {
    pub fn detect(bytes: &[u8]) -> Self {
        let mut lf = 0usize;
        let mut crlf = 0usize;
        let mut idx = 0usize;
        while idx < bytes.len() {
            if bytes[idx] == b'\n' {
                if idx > 0 && bytes[idx - 1] == b'\r' {
                    crlf += 1;
                } else {
                    lf += 1;
                }
            }
            idx += 1;
        }
        if crlf > lf {
            Eol::CrLf
        } else {
            Eol::Lf
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
        }
    }
}

/// A text file decoded into memory, ready for the rope-backed editor.
#[derive(Clone, Debug)]
pub struct TextFile {
    pub text: String,
    /// Encoding used on disk. `None` means UTF-8 without BOM.
    pub encoding: Option<&'static encoding_rs::Encoding>,
    pub eol: Eol,
    pub had_bom: bool,
    pub size: u64,
    /// True when some byte sequences could not be decoded and were replaced.
    pub lossy: bool,
}

impl TextFile {
    pub fn encoding_label(&self) -> String {
        let name = self
            .encoding
            .map(|e| e.name().to_string())
            .unwrap_or_else(|| "UTF-8".to_string());
        if self.had_bom {
            format!("{name} BOM")
        } else {
            name
        }
    }
}

pub fn read_text_file(path: &Path) -> Result<TextFile> {
    read_text_file_with_limit(path, MAX_FILE_BYTES)
}

pub fn read_text_file_with_limit(path: &Path, limit: u64) -> Result<TextFile> {
    let metadata = std::fs::metadata(path).map_err(|source| FsError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(FsError::NotAFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > limit {
        return Err(FsError::TooLarge {
            path: path.to_path_buf(),
            size: metadata.len(),
            limit,
        });
    }
    let bytes = std::fs::read(path).map_err(|source| FsError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let has_utf8_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let has_utf16_bom = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    if !has_utf8_bom && !has_utf16_bom {
        if let Some(index) = bytes.iter().position(|b| *b == 0) {
            // A NUL in the first 8 KiB is a solid binary signal; a stray NUL deep
            // in a huge file is still suspicious, so keep the check simple.
            if index < 8192 {
                return Err(FsError::Binary {
                    path: path.to_path_buf(),
                });
            }
        }
    }

    let size = bytes.len() as u64;

    // BOM handling first: encoding_rs wants the BOM left in place.
    let (had_bom, encoding, text, lossy) = if has_utf8_bom {
        let (cow, _, had_errors) = encoding_rs::UTF_8.decode(&bytes[3..]);
        (true, None, cow.into_owned(), had_errors)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        let (cow, _, had_errors) = encoding_rs::UTF_16LE.decode(&bytes);
        (
            true,
            Some(encoding_rs::UTF_16LE),
            cow.into_owned(),
            had_errors,
        )
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        let (cow, _, had_errors) = encoding_rs::UTF_16BE.decode(&bytes);
        (
            true,
            Some(encoding_rs::UTF_16BE),
            cow.into_owned(),
            had_errors,
        )
    } else if let Ok(text) = std::str::from_utf8(&bytes) {
        (false, None, text.to_string(), false)
    } else {
        let mut detector = chardetng::EncodingDetector::new();
        detector.feed(&bytes, true);
        let encoding = detector.guess(None, true);
        let (cow, _, had_errors) = encoding.decode(&bytes);
        (false, Some(encoding), cow.into_owned(), had_errors)
    };

    // Detect EOL after decoding: raw UTF-16 CRLF is `0D 00 0A 00`, where the
    // byte-level scan cannot see a `0D 0A` pair.
    let eol = Eol::detect(text.as_bytes());

    // Normalise CRLF to LF inside the buffer; the original style is remembered
    // and restored on save.
    let text = if eol == Eol::CrLf {
        text.replace("\r\n", "\n")
    } else {
        text
    };

    Ok(TextFile {
        text,
        encoding,
        eol,
        had_bom,
        size,
        lossy,
    })
}

/// Read only a byte range, for large-file paged viewing.
pub fn read_range(path: &Path, offset: u64, length: u64) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|source| FsError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|source| FsError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    let mut buffer = vec![0u8; length as usize];
    let mut read = 0usize;
    while read < buffer.len() {
        let n = file
            .read(&mut buffer[read..])
            .map_err(|source| FsError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if n == 0 {
            break;
        }
        read += n;
    }
    buffer.truncate(read);
    Ok(buffer)
}

/// Save text atomically, restoring the original encoding, BOM and line endings.
pub fn write_text_file(
    path: &Path,
    text: &str,
    encoding: Option<&'static encoding_rs::Encoding>,
    eol: Eol,
    had_bom: bool,
) -> Result<()> {
    let mut text = text.to_string();
    if eol == Eol::CrLf {
        text = text.replace('\n', "\r\n");
    }
    let encoding_name = encoding.map(|enc| enc.name());
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len() + 3);
    if had_bom {
        match encoding_name {
            Some("UTF-16LE") => bytes.extend_from_slice(&[0xFF, 0xFE]),
            Some("UTF-16BE") => bytes.extend_from_slice(&[0xFE, 0xFF]),
            None => bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]),
            Some(_) => {}
        }
    }
    match encoding_name {
        // encoding_rs's encoder output encoding for UTF-16 is UTF-8. Encode
        // UTF-16 explicitly so a UTF-16 BOM is never followed by UTF-8 bytes.
        Some("UTF-16LE") => {
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
        }
        Some("UTF-16BE") => {
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
        }
        Some(name) => {
            let (encoded, _, had_errors) = encoding
                .expect("encoding name came from the encoding")
                .encode(&text);
            if had_errors {
                return Err(FsError::Unmappable {
                    path: path.to_path_buf(),
                    encoding: name,
                });
            }
            bytes.extend_from_slice(&encoded);
        }
        None => bytes.extend_from_slice(text.as_bytes()),
    }
    write_atomic(path, &bytes)
}

/// Rename a file or directory without replacing an existing entry. The
/// platform rename(2) silently replaces files on Unix; this guard makes a
/// collision an explicit error. A case-only rename (same inode) is allowed.
pub fn rename_no_overwrite(from: &Path, to: &Path) -> std::result::Result<(), RenameError> {
    if from == to {
        return Ok(());
    }
    match std::fs::symlink_metadata(to) {
        Ok(metadata) => {
            if !same_entry(from, to) {
                return Err(RenameError::TargetExists {
                    is_dir: metadata.is_dir(),
                });
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(RenameError::Io(err)),
    }
    std::fs::rename(from, to).map_err(RenameError::Io)
}

#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    #[error("target already exists")]
    TargetExists { is_dir: bool },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn same_entry(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::symlink_metadata(a), std::fs::symlink_metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

/// Write bytes through a sibling temporary file + fsync + rename so a crash
/// never leaves a partially written file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    // `create_new` makes the temp file unpredictable and refuses to follow a
    // pre-created symlink. A crashed process can leave a temp behind, so retry
    // with a different suffix instead of truncating it.
    let (tmp, mut file) = loop {
        let suffix = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos())
            .unwrap_or(0);
        let candidate = dir.join(format!(
            ".{file_name}.laze-{}-{suffix}-{nanos}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(FsError::Write {
                    path: candidate,
                    source: err,
                });
            }
        }
    };
    let result = (|| {
        file.write_all(bytes).map_err(|source| FsError::Write {
            path: tmp.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| FsError::Write {
            path: tmp.clone(),
            source,
        })?;
        drop(file);
        // Preserve the original mode when overwriting.
        if let Ok(metadata) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&tmp, metadata.permissions());
        }
        std::fs::rename(&tmp, path).map_err(|source| FsError::Write {
            path: path.to_path_buf(),
            source,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Atomic write with git's lock-file protocol: create `<path>.lock`
/// exclusively, write, rename. Used where a concurrent writer must be refused
/// rather than silently overwritten.
pub fn write_atomic_locked(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let lock_path = dir.join(format!("{file_name}.lock"));
    let mut created = false;
    let lock_result = (|| -> Result<()> {
        let mut lock = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => {
                created = true;
                file
            }
            Err(err) => {
                return Err(FsError::Write {
                    path: lock_path.clone(),
                    source: err,
                });
            }
        };
        lock.write_all(bytes).map_err(|source| FsError::Write {
            path: lock_path.clone(),
            source,
        })?;
        lock.sync_all().map_err(|source| FsError::Write {
            path: lock_path.clone(),
            source,
        })?;
        drop(lock);
        // Preserve the original mode when overwriting.
        if let Ok(metadata) = std::fs::metadata(path) {
            let _ = std::fs::set_permissions(&lock_path, metadata.permissions());
        }
        std::fs::rename(&lock_path, path).map_err(|source| FsError::Write {
            path: path.to_path_buf(),
            source,
        })
    })();
    // Only clean up when this call created the lock; removing a foreign
    // writer's lock would break git's exclusion protocol.
    if lock_result.is_err() && created {
        let _ = std::fs::remove_file(&lock_path);
    }
    lock_result
}

/// Move a file or directory to the system trash; failures surface verbatim.
pub fn move_to_trash(path: &Path) -> Result<()> {
    trash::delete(path).map_err(|err| FsError::Write {
        path: path.to_path_buf(),
        source: std::io::Error::other(err.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_write_honors_an_existing_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index");
        std::fs::write(&path, b"old").unwrap();
        write_atomic_locked(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert!(!dir.path().join("index.lock").exists());

        // A lock held by a concurrent writer must be refused, not overwritten.
        let lock = dir.path().join("index.lock");
        std::fs::write(&lock, b"held by another writer").unwrap();
        assert!(write_atomic_locked(&path, b"clobber").is_err());
        assert_eq!(std::fs::read(&lock).unwrap(), b"held by another writer");
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn round_trips_utf8_lf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hello\nworld\n").unwrap();
        let file = read_text_file(&path).unwrap();
        assert_eq!(file.eol, Eol::Lf);
        assert_eq!(file.encoding, None);
        write_text_file(&path, &file.text, file.encoding, file.eol, file.had_bom).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\nworld\n");
    }

    #[test]
    fn preserves_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let file = read_text_file(&path).unwrap();
        assert_eq!(file.eol, Eol::CrLf);
        assert_eq!(file.text, "a\nb\n");
        write_text_file(&path, &file.text, file.encoding, file.eol, file.had_bom).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"a\r\nb\r\n");
    }

    #[test]
    fn rejects_binary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"abc\0def").unwrap();
        assert!(matches!(read_text_file(&path), Err(FsError::Binary { .. })));
    }

    #[test]
    fn rename_never_overwrites_an_existing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.txt");
        let existing = dir.path().join("b.txt");
        std::fs::write(&source, b"a").unwrap();
        std::fs::write(&existing, b"b").unwrap();

        let error = rename_no_overwrite(&source, &existing).unwrap_err();
        assert!(matches!(error, RenameError::TargetExists { is_dir: false }));
        assert_eq!(std::fs::read(&source).unwrap(), b"a");
        assert_eq!(std::fs::read(&existing).unwrap(), b"b");

        let target_dir = dir.path().join("target-dir");
        std::fs::create_dir(&target_dir).unwrap();
        let error = rename_no_overwrite(&source, &target_dir).unwrap_err();
        assert!(matches!(error, RenameError::TargetExists { is_dir: true }));
        assert_eq!(std::fs::read(&source).unwrap(), b"a");

        let renamed = dir.path().join("renamed.txt");
        rename_no_overwrite(&source, &renamed).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&renamed).unwrap(), b"a");
    }
}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    fn utf16le_with_bom(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn reads_and_preserves_utf16le_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf16.txt");
        std::fs::write(&path, utf16le_with_bom("hello\r\nworld\r\n")).unwrap();
        let file = read_text_file(&path).unwrap();
        assert_eq!(file.text, "hello\nworld\n");
        assert_eq!(file.encoding.map(|enc| enc.name()), Some("UTF-16LE"));
        assert!(file.had_bom);
        assert!(!file.lossy);

        write_text_file(&path, &file.text, file.encoding, file.eol, file.had_bom).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, utf16le_with_bom("hello\r\nworld\r\n"));
    }

    #[test]
    fn refuses_unmappable_characters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.txt");
        let error = write_text_file(
            &path,
            "ok 😀",
            Some(encoding_rs::WINDOWS_1252),
            Eol::Lf,
            false,
        )
        .unwrap_err();
        assert!(matches!(error, FsError::Unmappable { .. }), "{error}");
        assert!(!path.exists());
    }
}
