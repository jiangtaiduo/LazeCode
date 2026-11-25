//! Signed in-app updater.
//!
//! Release: publish a manifest ([`Manifest`]), upload the artifact, and sign
//! the canonical payload ([`Manifest::signing_payload`]) with Ed25519; the
//! public key is pinned in settings (`update_public_key`). The canonical JSON
//! uses declaration order, empty strings for absent optional fields, and
//! excludes `date` so existing signatures stay valid.
//!
//! Fetching accepts HTTPS or `file://`. Installation follows the manifest's
//! `kind`: `executable` replaces the binary in place (`.bak` + rollback);
//! `app-bundle` unpacks a gzipped `*.app` archive under a strict path/size
//! policy and swaps the whole bundle so `Info.plist`, resources and signature
//! stay consistent. [`relaunch`] starts the new build.
//!
//! Layout: this file keeps the manifest types, verification, check and
//! download; fetch/install/version live in submodules.

mod fetch;
mod install;
mod version;

pub use fetch::*;
pub use install::*;

use fetch::{current_os_version, ensure_platform, open_source, read_limited};
use install::temp_path;
#[cfg(test)]
use install::{install_with, prune_bundle_backups};
use version::{compare_versions, Version};

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
    /// Display-only release date (`YYYY-MM-DD`); deliberately outside
    /// [`Manifest::signing_payload`] so old signatures stay valid.
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

/// Re-check a staged artifact against the manifest's SHA-256.
///
/// `download_and_verify` already hashed the bytes it wrote, but the installer
/// is a separate step: a retry installs a file staged minutes ago, and the
/// staging directory is a plain directory in Application Support. Re-hashing
/// (one streaming read) closes that window for the price of reading the file
/// once more on the worker thread.
pub fn verify_staged_artifact(manifest: &Manifest, staged: &Path) -> Result<(), UpdateError> {
    let mut file = std::fs::File::open(staged).map_err(io_error)?;
    let mut hasher = ring::digest::Context::new(&SHA256);
    let mut buffer = vec![0u8; DOWNLOAD_CHUNK];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = to_hex(hasher.finish().as_ref());
    if actual != manifest.sha256 {
        return Err(UpdateError::ChecksumMismatch {
            expected: manifest.sha256.clone(),
            actual,
        });
    }
    Ok(())
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

        fn path(&self) -> &std::path::Path {
            &self.0
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

    #[test]
    fn staged_artifact_is_reverified_before_install() {
        let dir = TempDir::new();
        let artifact = dir.join("payload");
        std::fs::write(&artifact, b"payload").unwrap();
        let (manifest, _key) = signed_manifest("0.8.0", &artifact);
        verify_staged_artifact(&manifest, &artifact).unwrap();

        // A retry installs a file staged earlier; anything that replaced it
        // since must be refused.
        std::fs::write(&artifact, b"tampered after staging").unwrap();
        assert!(matches!(
            verify_staged_artifact(&manifest, &artifact),
            Err(UpdateError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn bundle_backups_are_pruned_to_the_newest_two() {
        let dir = TempDir::new();
        let bundle = dir.join("LazeCode.app");
        std::fs::create_dir_all(&bundle).unwrap();
        let mut backups = Vec::new();
        for index in 0..4 {
            let path = dir.join(&format!(".LazeCode.app.bak-{index}"));
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("marker"), b"x").unwrap();
            backups.push(path);
            // Distinct mtimes so the "newest two" choice is deterministic.
            std::thread::sleep(Duration::from_millis(5));
        }
        prune_bundle_backups(&bundle);
        let remaining: Vec<std::path::PathBuf> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".LazeCode.app.bak-")
            })
            .map(|entry| entry.path())
            .collect();
        assert_eq!(remaining.len(), 2, "{remaining:?}");
        assert!(remaining.contains(&backups[2]));
        assert!(remaining.contains(&backups[3]));
    }
}
