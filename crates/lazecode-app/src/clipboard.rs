//! Native clipboard file/image discovery for terminal paste.
//!
//! Finder puts only the POSIX path in the pasteboard's file-URL/NSFilenames
//! flavors; a browser's `File` object never exposes it. Screenshots and
//! "copy image" put PNG/TIFF data with no file flavor at all. Both are
//! materialized as absolute paths and written to the PTY, matching the stable
//! app and letting `omp` render its own image chips.
//!
//! Only macOS implements this; other platforms let the normal text path run.

use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use objc2::ClassType;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG, NSPasteboardTypeTIFF,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSString, NSURL};

/// Finder's legacy filename-array flavor.
#[cfg(target_os = "macos")]
const NSFILENAMES_PBOARD_TYPE: &str = "NSFilenamesPboardType";

#[cfg(target_os = "macos")]
fn normalize_path_string(raw: &str) -> Option<PathBuf> {
    let s = raw.trim();
    let unquoted = if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].trim().to_string()
    } else {
        s.to_string()
    };
    if unquoted.is_empty() || !unquoted.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(unquoted))
}

#[cfg(target_os = "macos")]
fn path_from_nsurl(url: &NSURL) -> Option<PathBuf> {
    if !url.isFileURL() {
        return None;
    }
    let s = url.path()?;
    normalize_path_string(&s.to_string())
}

#[cfg(target_os = "macos")]
fn image_temp_path(ext: &str) -> Option<PathBuf> {
    cleanup_stale_paste_files();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("lazecode-paste-{nanos}.{ext}"));
    Some(path)
}

/// Best-effort cleanup of paste artifacts older than an hour. A pasted
/// screenshot is consumed by the receiving program right away, so anything
/// still lying around after an hour is debris from a closed session.
#[cfg(target_os = "macos")]
fn cleanup_stale_paste_files() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("lazecode-paste-") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.modified().ok().is_some_and(|modified| {
            now.duration_since(modified)
                .map(|age| age > std::time::Duration::from_secs(3600))
                .unwrap_or(false)
        }) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Absolute paths represented by the current system clipboard.
///
/// Finder files are returned as-is. A screenshot / copied image is written
/// to `$TMPDIR/lazecode-paste-<nanos>.png|tiff` and that path is returned.
/// Empty means "no file or image flavor" (normal text paste).
pub fn file_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut out: Vec<PathBuf> = Vec::new();
        unsafe {
            let pb = NSPasteboard::generalPasteboard();

            // Finder single/multi selection.
            let classes = NSArray::from_slice(&[NSURL::class()]);
            if let Some(objs) = pb.readObjectsForClasses_options(&classes, None) {
                for obj in objs.iter() {
                    if let Some(url) = obj.downcast_ref::<NSURL>() {
                        if let Some(path) = path_from_nsurl(url) {
                            if !out.contains(&path) {
                                out.push(path);
                            }
                        }
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }

            // Legacy NSFilenamesPboardType.
            let key = NSString::from_str(NSFILENAMES_PBOARD_TYPE);
            if let Some(plist) = pb.propertyListForType(&key) {
                if let Ok(arr) = plist.downcast::<NSArray>() {
                    for item in arr.iter() {
                        if let Some(s) = item.downcast_ref::<NSString>() {
                            if let Some(path) = normalize_path_string(&s.to_string()) {
                                if !out.contains(&path) {
                                    out.push(path);
                                }
                            }
                        }
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }

            // One file URL per pasteboard item.
            if let Some(items) = pb.pasteboardItems() {
                for item in items.iter() {
                    if let Some(s) = item.stringForType(NSPasteboardTypeFileURL) {
                        if let Some(path) = normalize_path_string(&s.to_string()) {
                            if !out.contains(&path) {
                                out.push(path);
                            }
                        }
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }

            // Image-only clipboard: materialize the bytes so omp can read the
            // path. PNG first (browser copy), TIFF fallback (screenshot).
            for (ty, ext) in [(NSPasteboardTypePNG, "png"), (NSPasteboardTypeTIFF, "tiff")] {
                let Some(data) = pb.dataForType(ty) else {
                    continue;
                };
                let Some(path) = image_temp_path(ext) else {
                    continue;
                };
                let ns_path = NSString::from_str(&path.to_string_lossy());
                if data.writeToFile_atomically(&ns_path, true) {
                    out.push(path);
                    break;
                }
            }
        }
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Quote one absolute path for a shell/omp paste.
fn shell_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    const UNSAFE: &str = " \t\r\n'\"$`\\|&;<>(){}*?[]!#~";
    if text.chars().any(|c| UNSAFE.contains(c)) {
        format!("'{}'", text.replace('\'', "'\\''"))
    } else {
        text.into_owned()
    }
}

/// Join paths for bracketed paste; multiple attachments stay on one line so
/// the shell sees one command and omp can tokenize quoted paths.
pub fn attachment_text(paths: &[PathBuf]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    Some(
        paths
            .iter()
            .map(|path| shell_quote(path))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_paths_with_shell_metacharacters() {
        assert_eq!(shell_quote(Path::new("/tmp/a.png")), "/tmp/a.png");
        assert_eq!(
            shell_quote(Path::new("/tmp/my file.png")),
            "'/tmp/my file.png'"
        );
        assert_eq!(
            shell_quote(Path::new("/tmp/it's.png")),
            "'/tmp/it'\\''s.png'"
        );
    }

    #[test]
    fn attachment_text_joins_paths_on_one_line() {
        let text = attachment_text(&[
            PathBuf::from("/tmp/a.png"),
            PathBuf::from("/tmp/my file.png"),
        ])
        .unwrap();
        assert_eq!(text, "/tmp/a.png '/tmp/my file.png'");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn normalizes_absolute_paths_only() {
        assert_eq!(
            normalize_path_string("/tmp/a.md"),
            Some(PathBuf::from("/tmp/a.md"))
        );
        assert_eq!(normalize_path_string("relative/a.md"), None);
        assert_eq!(normalize_path_string(""), None);
        assert_eq!(
            normalize_path_string("\"/tmp/quoted path.md\""),
            Some(PathBuf::from("/tmp/quoted path.md"))
        );
    }
}
