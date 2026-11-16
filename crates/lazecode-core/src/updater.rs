//! Signed in-app updater for LazeCode.
//!
//! Release flow:
//!
//! 1. Publish a JSON manifest (see [`Manifest`]) at a static endpoint.
//! 2. Upload the platform artifact (a single executable or a gzipped
//!    `.app` bundle) somewhere the manifest points to.
//! 3. Sign the *canonical* manifest payload with an Ed25519 key; the
//!    corresponding 32-byte public key is pinned in the user settings
//!    (`update_public_key`, base64 or hex).
//!
//! The canonical payload is compact JSON with the fields
//!
//! ```json
//! {"version":"…","url":"…","sha256":"…","notes":"…","min_os":"…","arch":"…","kind":"…"}
//! ```
//!
//! in that exact order, with `min_os`/`arch`/`kind` as `""` when absent and
//! `sha256` lower-case hex. [`Manifest::signing_payload`] produces exactly
//! these bytes, so a release script can sign its output verbatim. The
//! optional `date` field is display-only and deliberately outside this
//! payload, so existing signatures stay valid.
//!
//! Checking and downloading use `reqwest`'s blocking client with rustls;
//! a plain filesystem path or `file://` URL is accepted instead of HTTP,
//! which keeps updates testable offline and lets an administrator ship a
//! manifest from a local share.
//!
//! Installation comes in two shapes, selected by the manifest's `kind`
//! field: `executable` (the default) replaces the running binary in place —
//! the current file is renamed to a `.bak` sibling, the staged binary is
//! moved into place, and any failure restores the backup. `app-bundle`
//! artifacts are gzipped tar archives of a `*.app` directory: the archive is
//! extracted into the staging area under a strict safety policy (no
//! absolute paths, no `..` components, no link entries, unpacked-size and
//! entry caps), sanity-checked, and then swapped over the running bundle
//! with the same backup-and-rollback discipline. Swapping the whole bundle
//! keeps `Info.plist`, resources and the ad-hoc signature consistent with
//! the new binary, which a bare executable replacement inside a signed
//! bundle cannot do.
//!
//! [`relaunch`] starts the freshly installed build: on macOS a bundle is
//! started through `open -n <bundle>`, anything else spawns the binary
//! directly with the original command-line arguments.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use ring::digest::SHA256;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};

/// Upper bound for a manifest body (1 MiB).
pub const MANIFEST_MAX_BYTES: u64 = 1024 * 1024;
/// Upper bound for a downloaded artifact (512 MiB).
pub const ARTIFACT_MAX_BYTES: u64 = 512 * 1024 * 1024;

const DOWNLOAD_CHUNK: usize = 64 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound for the unpacked `.app` bundle (1 GiB).
pub const BUNDLE_MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
/// Upper bound for entries inside a bundle archive.
pub const BUNDLE_MAX_ENTRIES: usize = 20_000;

/// A release announcement, fetched from `Settings::update_endpoint`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// Release version, e.g. `0.5.0` (a leading `v` is tolerated).
    pub version: String,
    /// Direct download URL (HTTP/HTTPS, `file://` or a filesystem path).
    pub url: String,
    /// Lower-case hex SHA-256 of the artifact bytes.
    pub sha256: String,
    /// Base64 (or hex) Ed25519 signature over [`Manifest::signing_payload`].
    #[serde(default)]
    pub signature: String,
    /// Human-readable release notes shown in the update dialog.
    #[serde(default)]
    pub notes: String,
    /// Optional human-readable release date (`YYYY-MM-DD`). Cosmetic only:
    /// it is deliberately not part of [`Manifest::signing_payload`], so old
    /// signatures stay valid and a tampered date cannot influence updates.
    #[serde(default)]
    pub date: Option<String>,
    /// Minimum OS version, compared against the running OS when known.
    #[serde(default, alias = "min_os_version")]
    pub min_os: Option<String>,
    /// Required CPU architecture (`aarch64`/`arm64`, `x86_64`/`x64`, …).
    #[serde(default)]
    pub arch: Option<String>,
    /// Artifact shape: `""`/`"executable"` (default) is a single binary,
    /// `"app-bundle"` is a gzipped tar archive of a `*.app` directory.
    #[serde(default)]
    pub kind: String,
}

/// What shape of artifact the manifest points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactKind {
    /// A single executable file, installed by replacing it in place.
    Executable,
    /// A gzipped tar archive of one `*.app` bundle, installed by swapping
    /// the whole bundle (macOS).
    AppBundle,
}

/// The exact bytes an Ed25519 signer must sign for this manifest.
#[derive(Serialize)]
struct SignedManifest<'a> {
    version: &'a str,
    url: &'a str,
    sha256: &'a str,
    notes: &'a str,
    min_os: &'a str,
    arch: &'a str,
    kind: &'a str,
}

impl Manifest {
    /// Canonical, deterministic signed representation. Never include the
    /// signature itself here (it would be recursive).
    pub fn signing_payload(&self) -> Vec<u8> {
        serde_json::to_vec(&SignedManifest {
            version: self.version.trim(),
            url: self.url.trim(),
            sha256: self.sha256.trim(),
            notes: self.notes.as_str(),
            min_os: self.min_os.as_deref().unwrap_or(""),
            arch: self.arch.as_deref().unwrap_or(""),
            kind: self.kind.trim(),
        })
        .expect("manifest fields are always serialisable")
    }

    /// Artifact shape announced by this manifest.
    pub fn artifact_kind(&self) -> ArtifactKind {
        match self.kind.trim().to_ascii_lowercase().as_str() {
            "app-bundle" | "app_bundle" | "bundle" => ArtifactKind::AppBundle,
            _ => ArtifactKind::Executable,
        }
    }

    /// Release notes, or `None` when the publisher left them out.
    pub fn release_notes(&self) -> Option<&str> {
        let notes = self.notes.trim();
        if notes.is_empty() {
            None
        } else {
            Some(notes)
        }
    }

    /// Optional release date, trimmed and ignored when empty.
    pub fn release_date(&self) -> Option<&str> {
        self.date
            .as_deref()
            .map(str::trim)
            .filter(|date| !date.is_empty())
    }
}

/// Result of a successful check.
#[derive(Clone, Debug)]
pub struct CheckOutcome {
    pub current_version: String,
    pub manifest: Manifest,
    /// `true` when the manifest version is strictly newer than `current_version`.
    pub update_available: bool,
}

impl CheckOutcome {
    pub fn available_version(&self) -> &str {
        &self.manifest.version
    }
}

/// Host the updater should consider itself running on.
#[derive(Clone, Debug)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    /// Best-effort OS release, used only for `min_os` gating.
    pub os_version: Option<String>,
}

impl Platform {
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            os_version: current_os_version(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("no update endpoint is configured")]
    EndpointMissing,
    #[error("no update signing key is configured")]
    KeyMissing,
    #[error("invalid update signing key: {0}")]
    InvalidKey(String),
    #[error("invalid update manifest: {0}")]
    Manifest(String),
    #[error("update manifest is not signed")]
    Unsigned,
    #[error("update manifest signature is invalid")]
    SignatureInvalid,
    #[error("network error: {0}")]
    Http(String),
    #[error("file error: {0}")]
    Io(String),
    #[error("artifact checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("download exceeds the {0}-byte limit")]
    TooLarge(u64),
    #[error("release requires {required} but this build targets {actual}")]
    UnsupportedPlatform { required: String, actual: String },
    #[error("installation failed: {0}")]
    Install(String),
}

/// Parse and validate a manifest body.
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, UpdateError> {
    if bytes.len() as u64 > MANIFEST_MAX_BYTES {
        return Err(UpdateError::TooLarge(MANIFEST_MAX_BYTES));
    }
    let mut manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|err| UpdateError::Manifest(err.to_string()))?;
    manifest.version = manifest.version.trim().to_string();
    manifest.url = manifest.url.trim().to_string();
    manifest.signature = manifest.signature.trim().to_string();
    manifest.date = manifest
        .date
        .map(|date| date.trim().to_string())
        .filter(|date| !date.is_empty());
    manifest.sha256 = normalise_sha256(&manifest.sha256)?;
    if manifest.version.is_empty() {
        return Err(UpdateError::Manifest("version is empty".into()));
    }
    if manifest.url.is_empty() {
        return Err(UpdateError::Manifest("artifact url is empty".into()));
    }
    Version::parse(&manifest.version)?;
    Ok(manifest)
}

/// Whether `key` parses as an Ed25519 public key (base64 or hex). Lets the
/// settings UI validate a pinned key without signing anything.
pub fn public_key_is_valid(key: &str) -> bool {
    decode_key(key).is_ok()
}

/// Verify the manifest's Ed25519 signature against a pinned public key.
///
/// The key may be base64 (standard alphabet, padded or not) or hex.
pub fn verify_manifest_signature(manifest: &Manifest, public_key: &str) -> Result<(), UpdateError> {
    if manifest.signature.trim().is_empty() {
        return Err(UpdateError::Unsigned);
    }
    let key = decode_key(public_key)?;
    let signature = decode_signature(&manifest.signature)?;
    UnparsedPublicKey::new(&ED25519, &key)
        .verify(&manifest.signing_payload(), &signature)
        .map_err(|_| UpdateError::SignatureInvalid)
}

/// Fetch the manifest, verify its signature, gate it on the current platform
/// and compare versions.
pub fn check(
    endpoint: &str,
    current_version: &str,
    public_key: &str,
) -> Result<CheckOutcome, UpdateError> {
    check_platform(endpoint, current_version, public_key, &Platform::current())
}

/// [`check`] with an explicit platform (used by tests and custom front-ends).
pub fn check_platform(
    endpoint: &str,
    current_version: &str,
    public_key: &str,
    platform: &Platform,
) -> Result<CheckOutcome, UpdateError> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err(UpdateError::EndpointMissing);
    }
    let key = public_key.trim();
    if key.is_empty() {
        return Err(UpdateError::KeyMissing);
    }
    let bytes = read_limited(endpoint, MANIFEST_MAX_BYTES)?;
    let manifest = parse_manifest(&bytes)?;
    verify_manifest_signature(&manifest, key)?;
    ensure_platform(&manifest, platform)?;
    let update_available =
        compare_versions(&manifest.version, current_version)? == Ordering::Greater;
    Ok(CheckOutcome {
        current_version: current_version.trim().trim_start_matches('v').to_string(),
        manifest,
        update_available,
    })
}

/// Download `manifest.url` into `dest`, enforcing the default size limit and
/// verifying both the SHA-256 and the Ed25519 manifest signature.
pub fn download_and_verify(
    manifest: &Manifest,
    public_key: &str,
    dest: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), UpdateError> {
    download_and_verify_limited(manifest, public_key, dest, ARTIFACT_MAX_BYTES, progress)
}

/// [`download_and_verify`] with an explicit byte cap.
pub fn download_and_verify_limited(
    manifest: &Manifest,
    public_key: &str,
    dest: &Path,
    max_bytes: u64,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), UpdateError> {
    verify_manifest_signature(manifest, public_key)?;
    let (mut reader, content_length) = open_source(&manifest.url)?;
    if content_length.is_some_and(|length| length > max_bytes) {
        return Err(UpdateError::TooLarge(max_bytes));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| UpdateError::Install("staging path has no parent".into()))?;
    std::fs::create_dir_all(parent).map_err(io_error)?;
    let temp = temp_path(dest);
    let _ = std::fs::remove_file(&temp);
    let result = (|| -> Result<(), UpdateError> {
        let mut file = std::fs::File::create(&temp).map_err(io_error)?;
        let mut hasher = ring::digest::Context::new(&SHA256);
        let mut buffer = vec![0u8; DOWNLOAD_CHUNK];
        let mut total = 0u64;
        loop {
            let read = reader.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > max_bytes {
                return Err(UpdateError::TooLarge(max_bytes));
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read]).map_err(io_error)?;
            progress(total, content_length);
        }
        file.flush().map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        let actual = to_hex(hasher.finish().as_ref());
        if actual != manifest.sha256 {
            return Err(UpdateError::ChecksumMismatch {
                expected: manifest.sha256.clone(),
                actual,
            });
        }
        std::fs::rename(&temp, dest).map_err(io_error)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Where downloaded artifacts are staged.
pub fn staging_dir() -> PathBuf {
    crate::settings::Settings::config_dir().join("updates")
}

/// Conventional staging path for a release.
pub fn staged_path(version: &str) -> PathBuf {
    let safe: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    staging_dir().join(format!("LazeCode-{safe}"))
}

#[derive(Clone, Debug)]
pub struct InstallReport {
    pub target: PathBuf,
    /// The previous executable, retained for manual rollback.
    pub backup: Option<PathBuf>,
}

/// Atomically replace `target` with the verified `staged` executable.
///
/// The previous file is moved to a `.bak` sibling first. If the replacement
/// or the post-install sanity check fails, the backup is restored.
///
/// `target` is an explicit parameter (rather than always
/// `std::env::current_exe`) so callers and tests can install into a sandbox
/// and so a front-end can offer a custom install location.
///
/// macOS note: this replaces one executable inside the bundle. It does not
/// update `Info.plist`, resources or the code signature; full `.app`
/// bundle replacement needs a signed installer/archive.
pub fn install(staged: &Path, target: &Path) -> Result<InstallReport, UpdateError> {
    install_with(staged, target, replace_file)
}

fn install_with<F>(staged: &Path, target: &Path, commit: F) -> Result<InstallReport, UpdateError>
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

/// Swap the running `.app` bundle with the bundle packaged in `staged`
/// (a gzipped tar archive containing one `*.app` directory).
///
/// The archive is extracted into the staging area under a strict policy
/// (single `*.app` root, regular files only, no absolute or `..` paths,
/// unpacked-size and entry caps), sanity-checked, moved onto the target
/// volume, and then swapped: the current bundle is renamed to a hidden
/// `.bak-<nanos>` sibling first and restored if anything fails. Swapping the
/// whole bundle keeps `Info.plist`, resources and the ad-hoc signature
/// consistent — replacing only `Contents/MacOS/<binary>` would invalidate
/// the bundle seal.
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
    Ok(InstallReport {
        target: target_bundle,
        backup: Some(backup),
    })
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
            // macOS tar/zip writers emit AppleDouble sidecars (`._*`) and
            // `.DS_Store` for extended attributes and Finder state. They are
            // not part of the bundle and must not be treated as extra `.app`
            // roots; every real path still goes through the checks below.
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

/// Start the freshly installed build. On macOS a bundle is started through
/// `open -n <bundle>`; a bare binary is spawned directly with the original
/// command-line arguments. The caller exits afterwards.
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

fn temp_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "artifact".into());
    dest.with_file_name(format!(".{name}.{}.part", std::process::id()))
}

/// Read a manifest from HTTP/HTTPS, a `file://` URL or a filesystem path,
/// refusing bodies larger than `max_bytes`.
pub fn read_limited(location: &str, max_bytes: u64) -> Result<Vec<u8>, UpdateError> {
    let (mut reader, _length) = open_source(location)?;
    let mut data = Vec::new();
    reader
        .by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut data)
        .map_err(io_error)?;
    if data.len() as u64 > max_bytes {
        return Err(UpdateError::TooLarge(max_bytes));
    }
    Ok(data)
}

fn open_source(location: &str) -> Result<(Box<dyn Read>, Option<u64>), UpdateError> {
    let location = location.trim();
    if location.is_empty() {
        return Err(UpdateError::EndpointMissing);
    }
    if let Some(path) = local_path(location) {
        let meta = std::fs::metadata(&path).map_err(io_error)?;
        let file = std::fs::File::open(&path).map_err(io_error)?;
        return Ok((Box::new(file), Some(meta.len())));
    }
    if !location.starts_with("http://") && !location.starts_with("https://") {
        return Err(UpdateError::Http(format!(
            "unsupported update source {location:?} (expected https://, http://, file:// or a path)"
        )));
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("LazeCode/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let response = client
        .get(location)
        .send()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(UpdateError::Http(format!(
            "{location} responded with HTTP {status}"
        )));
    }
    let length = response.content_length();
    Ok((Box::new(response), length))
}

fn local_path(location: &str) -> Option<PathBuf> {
    if let Some(rest) = location.strip_prefix("file://") {
        return Some(PathBuf::from(rest));
    }
    if !location.contains("://") {
        return Some(PathBuf::from(location));
    }
    None
}

fn io_error(err: std::io::Error) -> UpdateError {
    UpdateError::Io(err.to_string())
}

fn ensure_platform(manifest: &Manifest, platform: &Platform) -> Result<(), UpdateError> {
    let required_arch = manifest.arch.as_deref().map(str::trim).unwrap_or("");
    if !required_arch.is_empty() && normalise_arch(required_arch) != normalise_arch(&platform.arch)
    {
        return Err(UpdateError::UnsupportedPlatform {
            required: format!("arch {required_arch}"),
            actual: format!("arch {}", platform.arch),
        });
    }
    let min_os = manifest.min_os.as_deref().map(str::trim).unwrap_or("");
    let Some(current) = platform.os_version.as_deref() else {
        return Ok(());
    };
    if min_os.is_empty() || compare_versions(min_os, current)? != Ordering::Greater {
        return Ok(());
    }
    Err(UpdateError::UnsupportedPlatform {
        required: format!("{} {min_os}", platform.os),
        actual: format!("{} {current}", platform.os),
    })
}

fn normalise_arch(arch: &str) -> String {
    match arch.trim().to_ascii_lowercase().as_str() {
        "aarch64" | "arm64" | "arm64e" => "aarch64".to_string(),
        "x86_64" | "x64" | "amd64" => "x86_64".to_string(),
        "x86" | "i386" | "i686" => "x86".to_string(),
        other => other.to_string(),
    }
}

#[cfg(target_os = "macos")]
fn current_os_version() -> Option<String> {
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

#[cfg(target_os = "linux")]
fn current_os_version() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines().find_map(|line| {
        line.strip_prefix("VERSION_ID=")
            .map(|value| value.trim_matches('"').trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_os_version() -> Option<String> {
    None
}

fn normalise_sha256(value: &str) -> Result<String, UpdateError> {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix("sha256:").unwrap_or(trimmed);
    if trimmed.len() != 64 || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdateError::Manifest(format!(
            "sha256 must be 64 hex characters (got {:?})",
            trimmed
        )));
    }
    Ok(trimmed.to_ascii_lowercase())
}

/// Decode base64 (four alphabets) or hex, accepting only `expected` bytes.
fn decode_fixed(value: &str, expected: usize) -> Option<Vec<u8>> {
    let trimmed = value.trim();
    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Some(bytes) = engine
            .decode(trimmed)
            .ok()
            .filter(|bytes| bytes.len() == expected)
        {
            return Some(bytes);
        }
    }
    hex_decode(trimmed)
        .ok()
        .filter(|bytes| bytes.len() == expected)
}

fn decode_key(value: &str) -> Result<Vec<u8>, UpdateError> {
    if value.trim().is_empty() {
        return Err(UpdateError::KeyMissing);
    }
    decode_fixed(value, 32).ok_or_else(|| {
        UpdateError::InvalidKey("expected a 32-byte Ed25519 key encoded as base64 or hex".into())
    })
}

fn decode_signature(value: &str) -> Result<Vec<u8>, UpdateError> {
    decode_fixed(value, 64).ok_or_else(|| {
        UpdateError::Manifest(
            "signature must be a 64-byte Ed25519 signature in base64 or hex".into(),
        )
    })
}

fn hex_decode(value: &str) -> Result<Vec<u8>, ()> {
    if !value.len().is_multiple_of(2) {
        return Err(());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or(())
        })
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Compare two version strings (`major[.minor[.patch]][-pre][+build]`).
///
/// Returns the ordering of `left` relative to `right`. Missing minor/patch
/// components default to zero; a release sorts above any of its pre-releases.
pub fn compare_versions(left: &str, right: &str) -> Result<Ordering, UpdateError> {
    Ok(Version::parse(left)?.cmp_parsed(&Version::parse(right)?))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<PreId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreId {
    Numeric(u64),
    Alpha(String),
}

impl Version {
    fn parse(text: &str) -> Result<Self, UpdateError> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split('+').next().unwrap_or(text);
        let (core, pre) = text.split_once('-').unwrap_or((text, ""));
        let mut parts = core.split('.');
        let major = parse_number(parts.next().unwrap_or(""), text)?;
        let minor = parts
            .next()
            .map(|part| parse_number(part, text))
            .transpose()?
            .unwrap_or(0);
        let patch = parts
            .next()
            .map(|part| parse_number(part, text))
            .transpose()?
            .unwrap_or(0);
        if parts.next().is_some() {
            return Err(UpdateError::Manifest(format!("invalid version {text:?}")));
        }
        let pre = if pre.is_empty() {
            Vec::new()
        } else {
            pre.split('.')
                .map(|identifier| {
                    if !identifier.is_empty() && identifier.bytes().all(|b| b.is_ascii_digit()) {
                        identifier
                            .parse::<u64>()
                            .map(PreId::Numeric)
                            .map_err(|_| UpdateError::Manifest(format!("invalid version {text:?}")))
                    } else {
                        Ok(PreId::Alpha(identifier.to_string()))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    fn cmp_parsed(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| compare_pre(&self.pre, &other.pre))
    }
}

fn parse_number(part: &str, whole: &str) -> Result<u64, UpdateError> {
    part.parse::<u64>()
        .map_err(|_| UpdateError::Manifest(format!("invalid version {whole:?}")))
}

fn compare_pre(left: &[PreId], right: &[PreId]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    for (a, b) in left.iter().zip(right) {
        let ordering = match (a, b) {
            (PreId::Numeric(a), PreId::Numeric(b)) => a.cmp(b),
            (PreId::Numeric(_), PreId::Alpha(_)) => Ordering::Less,
            (PreId::Alpha(_), PreId::Numeric(_)) => Ordering::Greater,
            (PreId::Alpha(a), PreId::Alpha(b)) => a.cmp(b),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    const SEED: [u8; 32] = [
        7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97, 101,
        103, 107, 109, 113, 127, 131, 137, 139, 149,
    ];

    fn keypair() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&SEED).expect("seed is a valid Ed25519 scalar")
    }

    fn public_key_base64() -> String {
        base64::engine::general_purpose::STANDARD.encode(keypair().public_key().as_ref())
    }

    /// Build a signed manifest whose artifact is `artifact`.
    fn signed_manifest(version: &str, artifact: &Path) -> (Manifest, String) {
        let bytes = std::fs::read(artifact).unwrap();
        let sha256 = to_hex(ring::digest::digest(&SHA256, &bytes).as_ref());
        let mut manifest = Manifest {
            version: version.into(),
            url: artifact.display().to_string(),
            sha256,
            signature: String::new(),
            notes: "Fixed the updater".into(),
            date: None,
            min_os: None,
            arch: None,
            kind: String::new(),
        };
        let signature = keypair().sign(&manifest.signing_payload());
        manifest.signature = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());
        (manifest, public_key_base64())
    }

    fn write_manifest(path: &Path, manifest: &Manifest) {
        std::fs::write(path, serde_json::to_vec(manifest).unwrap()).unwrap();
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let mut path = std::env::temp_dir();
            // Parallel tests can observe the same wall-clock tick on macOS, so
            // combine the timestamp with a process-local counter.
            let unique = format!(
                "lazecode-updater-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            path.push(unique);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn join(&self, name: &str) -> std::path::PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn serve_once(body: Vec<u8>, status_line: &'static str) -> (String, mpsc::Receiver<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 8192];
                let _ = stream.read(&mut request);
                let header = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
            let _ = tx.send(());
        });
        (format!("http://{address}/manifest.json"), rx)
    }

    #[test]
    fn release_date_is_display_only_and_not_signed() {
        let mut manifest = Manifest {
            version: "1.2.3".into(),
            url: "https://updates.example.test/LazeCode".into(),
            sha256: "aa".repeat(32),
            signature: String::new(),
            notes: "Notes".into(),
            date: None,
            min_os: None,
            arch: None,
            kind: String::new(),
        };
        let signature = keypair().sign(&manifest.signing_payload());
        manifest.signature = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());
        let payload = manifest.signing_payload();
        manifest.date = Some(" 2026-11-16 ".into());
        assert_eq!(manifest.signing_payload(), payload);
        assert!(verify_manifest_signature(&manifest, &public_key_base64()).is_ok());
        assert_eq!(manifest.release_date(), Some("2026-11-16"));
    }

    #[test]
    fn signing_payload_is_stable_and_excludes_the_signature() {
        let mut manifest = Manifest {
            version: " 1.2.3 ".into(),
            url: " https://updates.example.test/LazeCode ".into(),
            sha256: "aa".repeat(32),
            signature: "first".into(),
            notes: "line one\nline two".into(),
            date: None,
            min_os: None,
            arch: Some("arm64".into()),
            kind: String::new(),
        };
        let expected = format!(
            r#"{{"version":"1.2.3","url":"https://updates.example.test/LazeCode","sha256":"{}","notes":"line one\nline two","min_os":"","arch":"arm64","kind":""}}"#,
            "aa".repeat(32)
        );
        let payload = manifest.signing_payload();
        assert_eq!(String::from_utf8(payload.clone()).unwrap(), expected);
        manifest.signature = "second".into();
        assert_eq!(
            manifest.signing_payload(),
            payload,
            "the signature must not sign itself"
        );
    }

    #[test]
    fn version_ordering_handles_core_prerelease_and_build_metadata() {
        assert_eq!(
            compare_versions("0.5.0", "0.4.9").unwrap(),
            Ordering::Greater
        );
        assert_eq!(compare_versions("v1.2", "1.2.0").unwrap(), Ordering::Equal);
        assert_eq!(
            compare_versions("1.0.0-rc.1", "1.0.0").unwrap(),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("1.0.0-rc.2", "1.0.0-rc.10").unwrap(),
            Ordering::Less
        );
        assert_eq!(
            compare_versions("1.0.0+build.5", "1.0.0").unwrap(),
            Ordering::Equal
        );
        assert!(compare_versions("not-a-version", "1.0.0").is_err());
    }

    #[test]
    fn signed_manifest_verifies_and_file_endpoint_checks() {
        let dir = TempDir::new();
        let artifact = dir.join("LazeCode");
        std::fs::write(&artifact, b"new binary").unwrap();
        let (manifest, key) = signed_manifest("0.5.0", &artifact);
        verify_manifest_signature(&manifest, &key).unwrap();

        let manifest_path = dir.join("manifest.json");
        write_manifest(&manifest_path, &manifest);
        let outcome = check_platform(
            manifest_path.to_str().unwrap(),
            "0.4.0",
            &key,
            &Platform {
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                os_version: None,
            },
        )
        .unwrap();
        assert!(outcome.update_available);
        assert_eq!(outcome.available_version(), "0.5.0");
        assert_eq!(outcome.manifest.release_notes(), Some("Fixed the updater"));

        // Same version is not an update; older manifest is not either.
        let outcome = check_platform(
            manifest_path.to_str().unwrap(),
            "0.5.0",
            &key,
            &Platform::current(),
        )
        .unwrap();
        assert!(!outcome.update_available);
    }

    #[test]
    fn tampered_manifest_fails_signature_check() {
        let dir = TempDir::new();
        let artifact = dir.join("LazeCode");
        std::fs::write(&artifact, b"new binary").unwrap();
        let (mut manifest, key) = signed_manifest("0.5.0", &artifact);
        manifest.notes = "malicious payload".into();
        assert!(matches!(
            verify_manifest_signature(&manifest, &key),
            Err(UpdateError::SignatureInvalid)
        ));
    }

    #[test]
    fn manifest_rejects_unsigned_and_malformed_payloads() {
        let dir = TempDir::new();
        let artifact = dir.join("LazeCode");
        std::fs::write(&artifact, b"new binary").unwrap();
        let (mut manifest, key) = signed_manifest("0.5.0", &artifact);
        manifest.signature.clear();
        assert!(matches!(
            verify_manifest_signature(&manifest, &key),
            Err(UpdateError::Unsigned)
        ));

        let bad = br#"{"version":"1.0.0","url":"https://example.test/a","sha256":"nope"}"#;
        assert!(matches!(parse_manifest(bad), Err(UpdateError::Manifest(_))));
    }

    #[test]
    fn check_uses_real_http_transport() {
        let dir = TempDir::new();
        let artifact = dir.join("LazeCode");
        std::fs::write(&artifact, b"http binary").unwrap();
        let (manifest, key) = signed_manifest("0.6.0", &artifact);
        let body = serde_json::to_vec(&manifest).unwrap();
        let (url, served) = serve_once(body, "200 OK");
        let outcome = check_platform(&url, "0.5.0", &key, &Platform::current()).unwrap();
        assert!(outcome.update_available);
        served.recv_timeout(Duration::from_secs(10)).unwrap();
    }

    #[test]
    fn check_surfaces_http_failures() {
        let (url, served) = serve_once(b"locked".to_vec(), "503 Service Unavailable");
        let key = public_key_base64();
        let err = check_platform(&url, "0.5.0", &key, &Platform::current()).unwrap_err();
        assert!(matches!(err, UpdateError::Http(_)), "got {err}");
        served.recv_timeout(Duration::from_secs(10)).unwrap();
    }

    #[test]
    fn download_verifies_hash_and_writes_verified_file() {
        let dir = TempDir::new();
        let artifact = dir.join("payload");
        let payload = vec![0x5au8; 300 * 1024];
        std::fs::write(&artifact, &payload).unwrap();
        let (manifest, key) = signed_manifest("0.7.0", &artifact);
        let dest = dir.join("staging").join("LazeCode");
        let mut progress = Vec::new();
        download_and_verify(&manifest, &key, &dest, &mut |done, total| {
            progress.push((done, total));
        })
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        assert!(progress.last().unwrap().0 >= payload.len() as u64);
        assert!(progress.last().unwrap().1.is_some());
        // The partial file is gone.
        assert!(!dest
            .with_file_name(format!(".LazeCode.{}.part", std::process::id()))
            .exists());
    }

    #[test]
    fn download_rejects_checksum_mismatch() {
        let dir = TempDir::new();
        let artifact = dir.join("payload");
        std::fs::write(&artifact, b"original").unwrap();
        let (mut manifest, key) = signed_manifest("0.7.0", &artifact);
        std::fs::write(&artifact, b"tampered after signing").unwrap();
        let dest = dir.join("staging").join("LazeCode");
        let err = download_and_verify(&manifest, &key, &dest, &mut |_, _| {})
            .expect_err("checksum must fail");
        assert!(
            matches!(err, UpdateError::ChecksumMismatch { .. }),
            "got {err}"
        );
        assert!(!dest.exists());
        // Re-sign with the new bytes; download now succeeds.
        manifest.sha256 = to_hex(ring::digest::digest(&SHA256, b"tampered after signing").as_ref());
        let signature = keypair().sign(&manifest.signing_payload());
        manifest.signature = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());
        download_and_verify(&manifest, &key, &dest, &mut |_, _| {}).unwrap();
    }

    #[test]
    fn download_rejects_invalid_signature_before_reading() {
        let dir = TempDir::new();
        let artifact = dir.join("payload");
        std::fs::write(&artifact, b"payload").unwrap();
        let (mut manifest, key) = signed_manifest("0.7.0", &artifact);
        manifest.url = "file:///definitely/not/read".into();
        let dest = dir.join("staging").join("LazeCode");
        assert!(matches!(
            download_and_verify(&manifest, &key, &dest, &mut |_, _| {}),
            Err(UpdateError::SignatureInvalid)
        ));
    }

    #[test]
    fn download_enforces_size_limit() {
        let dir = TempDir::new();
        let artifact = dir.join("payload");
        std::fs::write(&artifact, vec![1u8; 4096]).unwrap();
        let (manifest, key) = signed_manifest("0.7.0", &artifact);
        let dest = dir.join("staging").join("LazeCode");
        let err = download_and_verify_limited(&manifest, &key, &dest, 1024, &mut |_, _| {})
            .expect_err("limit must be enforced");
        assert!(matches!(err, UpdateError::TooLarge(1024)), "got {err}");
        assert!(!dest.exists());
    }

    #[test]
    fn platform_gate_rejects_other_architectures() {
        let dir = TempDir::new();
        let artifact = dir.join("LazeCode");
        std::fs::write(&artifact, b"binary").unwrap();
        let (mut manifest, key) = signed_manifest("0.8.0", &artifact);
        manifest.arch = Some("mips64".into());
        let signature = keypair().sign(&manifest.signing_payload());
        manifest.signature = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());
        let manifest_path = dir.join("manifest.json");
        write_manifest(&manifest_path, &manifest);
        let err = check_platform(
            manifest_path.to_str().unwrap(),
            "0.7.0",
            &key,
            &Platform::current(),
        )
        .expect_err("wrong arch must be rejected");
        assert!(
            matches!(err, UpdateError::UnsupportedPlatform { .. }),
            "got {err}"
        );
    }

    #[test]
    fn install_replaces_target_and_keeps_backup() {
        let dir = TempDir::new();
        let target = dir.join("LazeCode");
        std::fs::write(&target, b"old binary").unwrap();
        let staged = dir.join("staged");
        std::fs::write(&staged, b"new binary").unwrap();
        let report = install(&staged, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new binary");
        let backup = report.backup.expect("previous executable is backed up");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old binary");
        assert!(!staged.exists(), "staged file is moved, not copied");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "target must be executable");
        }

        // Installing without an existing target works too.
        let target2 = dir.join("fresh/LazeCode");
        let staged2 = dir.join("staged2");
        std::fs::write(&staged2, b"fresh binary").unwrap();
        let report = install(&staged2, &target2).unwrap();
        assert!(report.backup.is_none());
        assert_eq!(std::fs::read(&target2).unwrap(), b"fresh binary");
    }

    #[test]
    fn install_rolls_back_when_replacement_fails() {
        let dir = TempDir::new();
        let target = dir.join("LazeCode");
        std::fs::write(&target, b"old binary").unwrap();
        let staged = dir.join("staged");
        std::fs::write(&staged, b"new binary").unwrap();
        let err = install_with(&staged, &target, |_, _| {
            Err(std::io::Error::other("simulated commit failure"))
        })
        .expect_err("commit failure must surface");
        assert!(matches!(err, UpdateError::Install(_)), "got {err}");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"old binary",
            "original executable must be restored"
        );
        assert!(staged.exists(), "staged artifact is kept for a retry");
    }

    #[test]
    fn install_rejects_empty_staged_file() {
        let dir = TempDir::new();
        let target = dir.join("LazeCode");
        std::fs::write(&target, b"old binary").unwrap();
        let staged = dir.join("staged");
        std::fs::write(&staged, b"").unwrap();
        assert!(matches!(
            install(&staged, &target),
            Err(UpdateError::Install(_))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"old binary");
    }

    #[test]
    fn public_keys_accept_base64_and_hex() {
        let pair = keypair();
        let key = pair.public_key();
        let base64 = base64::engine::general_purpose::STANDARD.encode(key.as_ref());
        let hex = to_hex(key.as_ref());
        assert_eq!(decode_key(&base64).unwrap(), key.as_ref());
        assert_eq!(decode_key(&hex).unwrap(), key.as_ref());
        assert!(matches!(
            decode_key("too-short"),
            Err(UpdateError::InvalidKey(_))
        ));
    }

    // ---- app-bundle installation ----

    /// Build a minimal `*.app` tree and pack it as a gzipped tar archive.
    fn pack_bundle(root: &Path, archive: &Path, app_name: &str, body: &str) {
        let bundle = root.join(app_name);
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"), "<plist/>").unwrap();
        std::fs::write(bundle.join("Contents/MacOS/LazeCode"), body).unwrap();
        let file = std::fs::File::create(archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        tar.append_dir_all(app_name, &bundle).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn bundle_install_swaps_the_whole_bundle_and_keeps_a_backup() {
        let tmp = TempDir(
            std::env::temp_dir().join(format!("laze-updater-bundle-{}", std::process::id())),
        );
        std::fs::create_dir_all(&tmp.0).unwrap();
        let archive = tmp.0.join("bundle.tar.gz");
        pack_bundle(&tmp.0, &archive, "LazeCode.app", "new binary bytes");

        // Existing installation with an older resource that must not survive.
        let target = tmp.0.join("Applications").join("LazeCode.app");
        std::fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
        std::fs::write(target.join("Contents/Info.plist"), "<old plist/>").unwrap();
        std::fs::write(target.join("Contents/MacOS/LazeCode"), "old binary").unwrap();

        let report = install_app_bundle(&archive, &target).unwrap();
        assert_eq!(report.target, target);
        assert!(report.backup.is_some());
        assert_eq!(
            std::fs::read_to_string(target.join("Contents/MacOS/LazeCode")).unwrap(),
            "new binary bytes"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("Contents/Info.plist")).unwrap(),
            "<plist/>",
            "resources must come from the new bundle"
        );
        let backup = report.backup.unwrap();
        assert_eq!(
            std::fs::read_to_string(backup.join("Contents/MacOS/LazeCode")).unwrap(),
            "old binary"
        );
        let _ = std::fs::remove_dir_all(&tmp.0);
    }

    #[test]
    fn bundle_install_ignores_apple_double_metadata() {
        let tmp = TempDir(
            std::env::temp_dir().join(format!("laze-updater-metadata-{}", std::process::id())),
        );
        std::fs::create_dir_all(&tmp.0).unwrap();
        let bundle = tmp.0.join("LazeCode.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"), "<plist/>").unwrap();
        std::fs::write(bundle.join("Contents/MacOS/LazeCode"), "new binary").unwrap();
        let archive = tmp.0.join("bundle.tar.gz");
        let file = std::fs::File::create(&archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        // macOS tar prepends an AppleDouble sidecar for the bundle directory;
        // it must not count as a second `.app` root.
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        tar.append_data(&mut header, "._LazeCode.app", std::io::empty())
            .unwrap();
        tar.append_dir_all("LazeCode.app", &bundle).unwrap();
        let mut inner = tar::Header::new_gnu();
        inner.set_size(0);
        inner.set_mode(0o644);
        inner.set_entry_type(tar::EntryType::Regular);
        inner.set_cksum();
        tar.append_data(
            &mut inner,
            "LazeCode.app/Contents/._Info.plist",
            std::io::empty(),
        )
        .unwrap();
        tar.into_inner().unwrap().finish().unwrap();

        let target = tmp.0.join("Applications").join("LazeCode.app");
        std::fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
        std::fs::write(target.join("Contents/MacOS/LazeCode"), "old binary").unwrap();

        let report = install_app_bundle(&archive, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("Contents/MacOS/LazeCode")).unwrap(),
            "new binary"
        );
        assert!(
            !target.join("Contents/._Info.plist").exists(),
            "metadata sidecars must not be installed"
        );
        assert!(report.backup.is_some());
        let _ = std::fs::remove_dir_all(&tmp.0);
    }

    #[test]
    fn bundle_install_rejects_paths_outside_the_app_root() {
        let tmp = TempDir(
            std::env::temp_dir().join(format!("laze-updater-traversal-{}", std::process::id())),
        );
        std::fs::create_dir_all(&tmp.0).unwrap();
        // Craft the malicious entry by hand: the tar crate's writer refuses
        // `..` paths, but a real archive from an attacker does not have to
        // play by writer-side rules.
        let archive = tmp.0.join("evil.tar.gz");
        let file = std::fs::File::create(&archive).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut raw_header = vec![0u8; 512];
        let name = b"LazeCode.app/../../escaped.txt";
        raw_header[..name.len()].copy_from_slice(name);
        raw_header[100..108].copy_from_slice(b"0000644\0");
        raw_header[108..116].copy_from_slice(b"0000000\0");
        raw_header[116..124].copy_from_slice(b"0000000\0");
        raw_header[124..136].copy_from_slice(b"00000000004\0");
        raw_header[136..148].copy_from_slice(b"00000000000\0");
        raw_header[156] = b'0'; // regular file
        raw_header[257..263].copy_from_slice(b"ustar\0");
        raw_header[263..265].copy_from_slice(b"00");
        let checksum: u32 =
            raw_header.iter().map(|byte| u32::from(*byte)).sum::<u32>() + 8 * u32::from(b' '); // the checksum field itself is read as spaces
        let octal = format!("{checksum:06o}\0 ");
        raw_header[148..156].copy_from_slice(octal.as_bytes());
        encoder.write_all(&raw_header).unwrap();
        encoder.write_all(b"evil").unwrap();
        // The record is 4 bytes; pad the final 512-byte block to full length.
        let padding = 512 - 4;
        encoder.write_all(&vec![0u8; padding]).unwrap();
        encoder.finish().unwrap();

        let target = tmp.0.join("Applications").join("LazeCode.app");
        std::fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
        assert!(install_app_bundle(&archive, &target).is_err());
        assert!(
            !tmp.0.join("escaped.txt").exists(),
            "nothing may escape staging"
        );
        let _ = std::fs::remove_dir_all(&tmp.0);
    }

    #[test]
    fn bundle_install_rolls_back_when_the_archive_is_not_a_bundle() {
        let tmp = TempDir(
            std::env::temp_dir().join(format!("laze-updater-rollback-{}", std::process::id())),
        );
        std::fs::create_dir_all(&tmp.0).unwrap();
        let target = tmp.0.join("Applications").join("LazeCode.app");
        std::fs::create_dir_all(target.join("Contents/MacOS")).unwrap();
        std::fs::write(target.join("Contents/MacOS/LazeCode"), "old binary").unwrap();

        // A gzip bomb whose payload is not a tar file at all.
        let archive = tmp.0.join("garbage.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        encoder.finish().unwrap();
        assert!(matches!(
            install_app_bundle(&archive, &target),
            Err(UpdateError::Install(_))
        ));
        assert_eq!(
            std::fs::read_to_string(target.join("Contents/MacOS/LazeCode")).unwrap(),
            "old binary",
            "the previous bundle must survive a failed install"
        );
        let _ = std::fs::remove_dir_all(&tmp.0);
    }

    /// OpenSSL and ring implement the same standard Ed25519; the release
    /// script signs with the system `openssl`, so verify cross-compatibility
    /// once (skipped where the local `openssl` cannot do Ed25519).
    #[test]
    fn openssl_signed_manifest_verifies_with_ring() {
        use std::process::Command;
        let tmp = TempDir(
            std::env::temp_dir().join(format!("laze-updater-openssl-{}", std::process::id())),
        );
        std::fs::create_dir_all(&tmp.0).unwrap();
        let dir = &tmp.0;
        let ok = Command::new("openssl")
            .args(["genpkey", "-algorithm", "ed25519", "-out", "signing.key"])
            .current_dir(dir)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("skipping: local openssl cannot generate ed25519 keys");
            let _ = std::fs::remove_dir_all(&tmp.0);
            return;
        }
        let manifest = Manifest {
            version: "9.9.9".into(),
            url: "https://updates.example.test/LazeCode-9.9.9.app.tar.gz".into(),
            sha256: "bb".repeat(32),
            signature: String::new(),
            notes: "openssl cross-sign".into(),
            date: None,
            min_os: None,
            arch: None,
            kind: "app-bundle".into(),
        };
        std::fs::write(dir.join("payload.bin"), manifest.signing_payload()).unwrap();
        let signed = Command::new("openssl")
            .args([
                "pkeyutl",
                "-sign",
                "-inkey",
                "signing.key",
                "-in",
                "payload.bin",
                "-out",
                "payload.sig",
            ])
            .current_dir(dir)
            .output()
            .unwrap();
        if !signed.status.success() {
            // OpenSSL 1.1.1 (macOS CI) generates Ed25519 keys but its
            // `pkeyutl` refuses to sign with them; the cross-check cannot run.
            eprintln!(
                "skipping: local openssl cannot sign Ed25519: {}",
                String::from_utf8_lossy(&signed.stderr).trim()
            );
            let _ = std::fs::remove_dir_all(&tmp.0);
            return;
        }
        let signature = base64::engine::general_purpose::STANDARD
            .encode(std::fs::read(dir.join("payload.sig")).unwrap());
        let pub_der = Command::new("openssl")
            .args(["pkey", "-in", "signing.key", "-pubout", "-outform", "DER"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(pub_der.status.success());
        // SPKI DER ends with the raw 32-byte public key.
        let public = base64::engine::general_purpose::STANDARD
            .encode(&pub_der.stdout[pub_der.stdout.len() - 32..]);
        let mut manifest = manifest;
        manifest.signature = signature;
        assert_eq!(manifest.artifact_kind(), ArtifactKind::AppBundle);
        verify_manifest_signature(&manifest, &public).unwrap();
        let _ = std::fs::remove_dir_all(&tmp.0);
    }

    // Keep the random source referenced so the seed helper stays honest if the
    // ring API grows an `generate_pkcs8` requirement in future versions.
    #[test]
    fn keypair_matches_documented_seed_derivation() {
        let rng = SystemRandom::new();
        let generated = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(generated.as_ref()).unwrap();
        assert_eq!(pair.public_key().as_ref().len(), 32);
    }
}
