//! Install phase: staging an artifact, replacing the app bundle, backups
//! and relaunch/rollback.

use super::fetch::io_error;
use super::*;

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

/// Swap the running `.app` bundle with the one in `staged` (gzipped tar, one
/// `*.app` root, strict extraction policy), restoring the backup on failure.
/// Swapping the whole bundle keeps `Info.plist`, resources and signature
/// consistent.
pub fn install_app_bundle(staged: &Path, target: &Path) -> Result<InstallReport, UpdateError> {
    let target_bundle = bundle_root(target).ok_or_else(|| {
        UpdateError::Install(format!(
            "{} is not inside a `.app` bundle",
            target.display()
        ))
    })?;
    let extracted = extract_app_bundle(staged)?;
    let staged_bundle = extracted.bundle_path;
    let staged_binary = staged_bundle.join("Contents/MacOS");
    if !staged_binary.is_dir() {
        return Err(UpdateError::Install(
            "staged bundle has no Contents/MacOS directory".into(),
        ));
    }

    // The final rename must happen on one volume. Stage a copy next to the
    // target first so both renames below are same-directory and atomic.
    let local = staging_dir().join(format!(
        "bundle-apply-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&local).map_err(io_error)?;
    let local_bundle = local.join(
        target_bundle
            .file_name()
            .ok_or_else(|| UpdateError::Install("target bundle has no file name".into()))?,
    );
    if let Err(_err) = std::fs::rename(&staged_bundle, &local_bundle) {
        // Different volumes: fall back to a recursive copy next to the
        // target (the running app keeps its inode, so copying it is safe).
        copy_dir_recursive(&staged_bundle, &local_bundle).map_err(|err| {
            let _ = std::fs::remove_dir_all(&local);
            UpdateError::Install(format!("cannot stage bundle on target volume: {err}"))
        })?;
        let _ = std::fs::remove_dir_all(&extracted.root);
    } else {
        let _ = std::fs::remove_dir_all(&extracted.root);
    }

    let backup = unique_bundle_backup_path(&target_bundle);
    std::fs::rename(&target_bundle, &backup).map_err(|err| {
        let _ = std::fs::remove_dir_all(&local);
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
        let _ = std::fs::remove_dir_all(&local);
        return Err(UpdateError::Install(format!(
            "cannot move the new bundle into place: {err}"
        )));
    }
    let _ = std::fs::remove_dir_all(&local);
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
fn extract_app_bundle(archive: &Path) -> Result<ExtractedBundle, UpdateError> {
    use tar::EntryType;
    let file = std::fs::File::open(archive).map_err(io_error)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    archive.set_preserve_permissions(true);
    let root = staging_dir().join(format!(
        "bundle-extract-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&root).map_err(io_error)?;

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

pub(super) fn temp_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "artifact".into());
    dest.with_file_name(format!(".{name}.{}.part", std::process::id()))
}
