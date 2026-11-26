//! Install phase: staging an artifact, replacing the app bundle, backups
//! and relaunch/rollback.

use super::fetch::io_error;

use super::*;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

#[derive(Clone, Debug)]
pub struct InstallReport {
    pub target: PathBuf,
    /// The previous executable, retained for manual rollback.
    pub backup: Option<PathBuf>,
}

/// Replace `target` with the verified `staged` executable, restoring the
/// `.bak` sibling on failure. `target` is a parameter so tests and custom
/// install locations work.
///
/// Replaces the executable only; `Info.plist`, resources and the code
/// signature are untouched (use [`install_app_bundle`] for that).
pub fn install(staged: &Path, target: &Path) -> Result<InstallReport, UpdateError> {
    install_with(staged, target, replace_file)
}

pub(super) fn install_with<F>(
    staged: &Path,
    target: &Path,
    commit: F,
) -> Result<InstallReport, UpdateError>
where
    F: FnOnce(&Path, &Path) -> std::io::Result<()>,
{
    let metadata = std::fs::metadata(staged).map_err(io_error)?;
    if !metadata.is_file() {
        return Err(UpdateError::Install(format!(
            "staged artifact {} is not a regular file",
            staged.display()
        )));
    }
    if metadata.len() == 0 {
        return Err(UpdateError::Install("staged artifact is empty".into()));
    }
    if let Some(parent) = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }

    let backup = if target.exists() {
        let backup = unique_backup_path(target);
        std::fs::rename(target, &backup).map_err(|err| {
            UpdateError::Install(format!(
                "cannot move {} aside before installing: {err}",
                target.display()
            ))
        })?;
        Some(backup)
    } else {
        None
    };

    let mut failure = None;
    if let Err(err) = commit(staged, target) {
        failure = Some(format!("cannot replace {}: {err}", target.display()));
    }
    if failure.is_none() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(err) =
                std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
            {
                failure = Some(format!(
                    "cannot mark {} executable: {err}",
                    target.display()
                ));
            }
        }
    }
    if failure.is_none() {
        match std::fs::metadata(target) {
            Ok(meta) if meta.len() > 0 => {}
            Ok(_) => failure = Some(format!("{} is empty after install", target.display())),
            Err(err) => {
                failure = Some(format!(
                    "{} is missing after install: {err}",
                    target.display()
                ))
            }
        }
    }
    if let Some(reason) = failure {
        let restore = rollback(target, backup.as_deref());
        return Err(match restore {
            Ok(()) if backup.is_some() => {
                UpdateError::Install(format!("{reason}; the previous executable was restored"))
            }
            Ok(()) => UpdateError::Install(reason),
            Err(err) => UpdateError::Install(format!("{reason}; {err}")),
        });
    }
    Ok(InstallReport {
        target: target.to_path_buf(),
        backup,
    })
}

/// Install a verified artifact according to the manifest's announced kind.
pub fn install_artifact(
    staged: &Path,
    target: &Path,
    kind: ArtifactKind,
) -> Result<InstallReport, UpdateError> {
    match kind {
        ArtifactKind::Executable => install(staged, target),
        ArtifactKind::AppBundle => install_app_bundle(staged, target),
    }
}

/// Verify the staged bytes and install exactly those bytes, without a
/// verify/install gap. The staged path is opened once with `O_NOFOLLOW`; the
/// hash and the installation both consume that file descriptor. A replacement
/// of the path after verification cannot change what gets installed.
pub fn verify_and_install_artifact(
    manifest: &Manifest,
    public_key: &str,
    staged: &Path,
    target: &Path,
) -> Result<InstallReport, UpdateError> {
    verify_and_install_with(manifest, public_key, staged, target, || {})
}

/// Same as [`verify_and_install_artifact`] with a hook called right after the
/// hash check. Used by tests to replace the staged path in the verify/install
/// window; production callers pass a no-op.
pub(super) fn verify_and_install_with<F: FnOnce()>(
    manifest: &Manifest,
    public_key: &str,
    staged: &Path,
    target: &Path,
    after_verify: F,
) -> Result<InstallReport, UpdateError> {
    verify_manifest_signature(manifest, public_key)?;
    let mut file = open_no_follow(staged).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.file_type().is_file() {
        return Err(UpdateError::Install(
            "staged artifact is not a regular file".into(),
        ));
    }
    if metadata.len() == 0 {
        return Err(UpdateError::Install("staged artifact is empty".into()));
    }
    let digest = hash_reader(&mut file)?;
    ensure_digest(manifest, &digest)?;
    file.seek(SeekFrom::Start(0)).map_err(io_error)?;
    after_verify();

    match manifest.artifact_kind() {
        ArtifactKind::Executable => {
            // Copy the verified descriptor into a private, unpredictable
            // directory next to the target before the atomic swap. Installing
            // the staged path directly would reopen it and reintroduce the
            // replacement window the digest check just closed.
            let parent = target
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let work = private_dir(parent, ".LazeCode-install-").map_err(io_error)?;
            let _cleanup = TempTree(work.clone());
            let copy = temp_path(&work.join("artifact")).map_err(io_error)?;
            copy_verified(&mut file, &copy, &digest)?;
            install_with(&copy, target, replace_file)
        }
        ArtifactKind::AppBundle => {
            // The archive is decompressed from the same verified descriptor.
            let extracted = extract_app_bundle_from(&mut file)?;
            let _cleanup = TempTree(extracted.root.clone());
            install_extracted_bundle(extracted, target)
        }
    }
}

/// Copy a verified reader into a freshly created (no-follow) file and hash
/// the copy, so the inode installed below is the one that was checked.
fn copy_verified<R: Read>(reader: &mut R, path: &Path, expected: &str) -> Result<(), UpdateError> {
    let mut output = create_new_no_follow(path).map_err(io_error)?;
    let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0u8; DOWNLOAD_CHUNK];
    loop {
        let read = reader.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read]).map_err(io_error)?;
    }
    output.flush().map_err(io_error)?;
    output.sync_all().map_err(io_error)?;
    let actual = to_hex(hasher.finish().as_ref());
    if actual != expected {
        return Err(UpdateError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

/// Swap the running `.app` bundle with the one in `staged` (gzipped tar, one
/// `*.app` root, strict extraction policy), restoring the backup on failure.
/// Swapping the whole bundle keeps `Info.plist`, resources and signature
/// consistent.
pub fn install_app_bundle(staged: &Path, target: &Path) -> Result<InstallReport, UpdateError> {
    let file = open_no_follow(staged).map_err(io_error)?;
    let extracted = extract_app_bundle_from(file)?;
    install_extracted_bundle(extracted, target)
}

/// Finish a bundle swap from an already-extracted archive.
fn install_extracted_bundle(
    extracted: ExtractedBundle,
    target: &Path,
) -> Result<InstallReport, UpdateError> {
    let target_bundle = bundle_root(target).ok_or_else(|| {
        UpdateError::Install(format!(
            "{} is not inside a `.app` bundle",
            target.display()
        ))
    })?;
    let _extract_cleanup = TempTree(extracted.root.clone());
    let staged_bundle = extracted.bundle_path;
    let staged_binary = staged_bundle.join("Contents/MacOS");
    if !staged_binary.is_dir() {
        return Err(UpdateError::Install(
            "staged bundle has no Contents/MacOS directory".into(),
        ));
    }

    // The final rename must happen on one volume. Stage a copy in a private
    // directory next to the target's staging area so both renames below are
    // atomic and no other process can plant a path component.
    let local = private_dir(&staging_dir(), "bundle-apply-").map_err(io_error)?;
    let _local_cleanup = TempTree(local.clone());
    let local_bundle = local.join(
        target_bundle
            .file_name()
            .ok_or_else(|| UpdateError::Install("target bundle has no file name".into()))?,
    );
    if let Err(_err) = std::fs::rename(&staged_bundle, &local_bundle) {
        // Different volumes: fall back to a recursive copy next to the
        // target (the running app keeps its inode, so copying it is safe).
        copy_dir_recursive(&staged_bundle, &local_bundle).map_err(|err| {
            UpdateError::Install(format!("cannot stage bundle on target volume: {err}"))
        })?;
    }

    let backup = unique_bundle_backup_path(&target_bundle);
    std::fs::rename(&target_bundle, &backup).map_err(|err| {
        UpdateError::Install(format!(
            "cannot back up the current bundle {}: {err}",
            target_bundle.display()
        ))
    })?;
    let swap = std::fs::rename(&local_bundle, &target_bundle);
    if let Err(err) = swap {
        if let Err(restore_error) = std::fs::rename(&backup, &target_bundle) {
            log::error!(
                "bundle rollback also failed: {restore_error}; the previous bundle remains at {}",
                backup.display()
            );
        }
        return Err(UpdateError::Install(format!(
            "cannot move the new bundle into place: {err}"
        )));
    }
    // Every update leaves a `.LazeCode.app.bak-*` sibling; without pruning
    // they accumulate one full bundle per release in /Applications.
    prune_bundle_backups(&target_bundle);
    Ok(InstallReport {
        target: target_bundle,
        backup: Some(backup),
    })
}

/// Keep the newest two `.NAME.bak-*` siblings of `bundle`.
pub(super) fn prune_bundle_backups(bundle: &Path) {
    const KEEP: usize = 2;
    let Some(parent) = bundle.parent() else {
        return;
    };
    let Some(name) = bundle.file_name() else {
        return;
    };
    let prefix = format!(".{}.bak-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let mut backups: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if !file_name.starts_with(&prefix) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    if backups.len() <= KEEP {
        return;
    }
    backups.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in backups.into_iter().skip(KEEP) {
        if let Err(err) = std::fs::remove_dir_all(&path) {
            log::warn!("cannot prune old bundle backup {}: {err}", path.display());
        }
    }
}

struct ExtractedBundle {
    root: PathBuf,
    bundle_path: PathBuf,
}

/// Extract a gzipped tar archive containing exactly one `*.app` directory
/// into a fresh staging directory, rejecting anything that could escape it.
fn extract_app_bundle_from<R: Read>(source: R) -> Result<ExtractedBundle, UpdateError> {
    use tar::EntryType;
    let decoder = flate2::read::GzDecoder::new(source);
    let mut archive = tar::Archive::new(decoder);
    archive.set_preserve_permissions(true);
    let root = private_dir(&staging_dir(), "bundle-extract-").map_err(io_error)?;

    let result = (|| -> Result<PathBuf, UpdateError> {
        let mut bundle_root: Option<PathBuf> = None;
        let mut entries = 0usize;
        let mut unpacked = 0u64;
        for entry in archive.entries().map_err(io_error)? {
            let mut entry = entry.map_err(io_error)?;
            entries += 1;
            if entries > BUNDLE_MAX_ENTRIES {
                return Err(UpdateError::Install(
                    "bundle archive exceeds the entry limit".into(),
                ));
            }
            let path = entry.path().map_err(io_error)?.to_path_buf();
            if path.is_absolute()
                || path
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
            {
                return Err(UpdateError::Install(format!(
                    "bundle archive contains an unsafe path {}",
                    path.display()
                )));
            }
            // macOS tar/zip writers add AppleDouble (`._*`) and `.DS_Store`
            // sidecars; they are not extra `.app` roots.
            let metadata = path.components().any(|component| {
                let name = component.as_os_str().to_string_lossy();
                name == ".DS_Store" || name.starts_with("._")
            });
            if metadata {
                continue;
            }
            match entry.header().entry_type() {
                EntryType::Regular | EntryType::Directory => {}
                // Links inside an application bundle are never needed by this
                // updater and are the classic escape hatch, so refuse them.
                other => {
                    return Err(UpdateError::Install(format!(
                        "bundle archive contains a non-regular entry ({other:?}): {}",
                        path.display()
                    )))
                }
            }
            let mut components = path.components();
            let top = components
                .next()
                .ok_or_else(|| UpdateError::Install("bundle archive has an empty path".into()))?;
            let top_name = top.as_os_str().to_string_lossy().into_owned();
            if !top_name.ends_with(".app") {
                return Err(UpdateError::Install(format!(
                    "bundle archive entry {} is outside the `.app` root",
                    path.display()
                )));
            }
            let root_name = match &bundle_root {
                Some(existing) => existing
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                None => {
                    bundle_root = Some(root.join(&top_name));
                    top_name.clone()
                }
            };
            if top_name != root_name {
                return Err(UpdateError::Install(
                    "bundle archive contains more than one `.app` root".into(),
                ));
            }
            unpacked += entry.size();
            if unpacked > BUNDLE_MAX_UNPACKED_BYTES {
                return Err(UpdateError::Install(
                    "bundle archive exceeds the unpacked size limit".into(),
                ));
            }
            let dest = root.join(&path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(io_error)?;
            }
            entry.unpack(&dest).map_err(io_error)?;
        }
        bundle_root.ok_or_else(|| UpdateError::Install("bundle archive is empty".into()))
    })();

    match result {
        Ok(bundle_path) => Ok(ExtractedBundle { root, bundle_path }),
        Err(err) => {
            let _ = std::fs::remove_dir_all(&root);
            Err(err)
        }
    }
}

/// Resolve the enclosing `*.app` bundle: accepts either the bundle path
/// itself or any path inside it (such as `Contents/MacOS/<binary>`).
fn bundle_root(exe: &Path) -> Option<PathBuf> {
    let mut current = Some(exe);
    while let Some(path) = current {
        if path.extension().is_some_and(|ext| ext == "app") {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

fn unique_bundle_backup_path(bundle: &Path) -> PathBuf {
    let parent = bundle.parent().unwrap_or_else(|| Path::new("."));
    let name = bundle
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "LazeCode.app".to_string());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    // Dot-prefixed and without an `.app` suffix so Finder/Launchpad do not
    // show the backup as an installed application.
    parent.join(format!(".{name}.bak-{nanos}"))
}

/// Recursively copy a directory tree, preserving file permissions.
fn copy_dir_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// Start the freshly installed build: `open -n <bundle>` on macOS, otherwise
/// spawn the binary with the original arguments. The caller exits afterwards.
pub fn relaunch(target: &Path) -> Result<(), UpdateError> {
    #[cfg(target_os = "macos")]
    {
        if let Some(bundle) = bundle_root(target) {
            std::process::Command::new("/usr/bin/open")
                .arg("-n")
                .arg(&bundle)
                .spawn()
                .map_err(|err| UpdateError::Install(format!("cannot relaunch: {err}")))?;
            return Ok(());
        }
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::Command::new(target)
        .args(&args)
        .spawn()
        .map_err(|err| UpdateError::Install(format!("cannot relaunch: {err}")))?;
    Ok(())
}

fn rollback(target: &Path, backup: Option<&Path>) -> Result<(), UpdateError> {
    match backup {
        Some(backup) => {
            let _ = std::fs::remove_file(target);
            std::fs::rename(backup, target).map_err(|err| {
                UpdateError::Install(format!(
                    "rollback failed: {err} (backup kept at {})",
                    backup.display()
                ))
            })
        }
        None => {
            let _ = std::fs::remove_file(target);
            Ok(())
        }
    }
}

fn unique_backup_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "LazeCode".into());
    let directory = target.parent().unwrap_or_else(|| Path::new("."));
    let candidate = directory.join(format!("{name}.bak"));
    if !candidate.exists() {
        return candidate;
    }
    for index in 1..1000 {
        let candidate = directory.join(format!("{name}.{index}.bak"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{name}.bak"))
}

fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(rename_error) => match std::fs::copy(from, to) {
            Ok(_) => {
                std::fs::remove_file(from)?;
                Ok(())
            }
            Err(_) => Err(rename_error),
        },
    }
}

/// Cryptographically random token for staging names. Never fall back to a
/// predictable name: a guessable `.part` path is a symlink attack target.
fn random_token() -> io::Result<String> {
    use ring::rand::SecureRandom;

    let mut bytes = [0u8; 12];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| io::Error::other("system randomness unavailable for staging names"))?;
    Ok(to_hex(&bytes))
}

/// Owns a temporary directory tree and removes it on every exit path.
pub(super) struct TempTree(pub(super) PathBuf);

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn create_private(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// Create an unpredictable, owner-only directory under `parent`.
pub(super) fn private_dir(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(parent)?;
    for _ in 0..16 {
        let candidate = parent.join(format!("{prefix}{}", random_token()?));
        match create_private(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::other(
        "cannot create a private staging directory",
    ))
}

fn no_follow_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options
}

/// Open an existing staged artifact without ever following a symlink.
pub(super) fn open_no_follow(path: &Path) -> io::Result<File> {
    let file = no_follow_options().read(true).open(path)?;
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "staged artifact is a symlink",
        ));
    }
    if !file.metadata()?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "staged artifact is not a regular file",
        ));
    }
    Ok(file)
}

/// Create a fresh temp file, refusing to reuse or follow an existing path.
pub(super) fn create_new_no_follow(path: &Path) -> io::Result<File> {
    let mut options = no_follow_options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)
}

/// Unpredictable partial path for a staged download.
pub(super) fn temp_path(dest: &Path) -> io::Result<PathBuf> {
    let name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "artifact".into());
    Ok(dest.with_file_name(format!(".{name}.{}.part", random_token()?)))
}
