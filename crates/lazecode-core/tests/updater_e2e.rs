//! Updater end-to-end over real HTTP (reqwest + rustls): manifest fetch,
//! signature verify, SHA-256 download, bundle install. The live-endpoint case
//! is `#[ignore]`d.

use ring::signature::KeyPair;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

use base64::Engine as _;
use lazecode_core::updater::{self, ArtifactKind, Manifest, UpdateError, ARTIFACT_MAX_BYTES};

/// One-shot local HTTP server: answers every request from `files` (URL path
/// → bytes) and records the paths it was asked for.
struct LocalServer {
    url: String,
    shutdown: std::sync::mpsc::Sender<()>,
    log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl LocalServer {
    fn start_on(listener: TcpListener, files: Vec<(String, Vec<u8>)>) -> Self {
        let port = listener.local_addr().unwrap().port();
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log_handle = log.clone();
        std::thread::spawn(move || {
            listener.set_nonblocking(true).expect("set_nonblocking");
            loop {
                if shutdown_rx.try_recv().is_ok() {
                    return;
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Accepted sockets inherit the listener's non-blocking
                        // flag on macOS; read headers with a bounded loop
                        // instead of read_to_string (the client keeps the
                        // write side open for keep-alive).
                        let _ = stream.set_nonblocking(false);
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 1024];
                        loop {
                            match stream.read(&mut chunk) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    buf.extend_from_slice(&chunk[..n]);
                                    if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                                        break;
                                    }
                                }
                            }
                        }
                        let request = String::from_utf8_lossy(&buf).into_owned();
                        let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                        log_handle.lock().unwrap().push(path.clone());
                        let body = files
                            .iter()
                            .find(|(served, _)| *served == path)
                            .map(|(_, bytes)| bytes.clone())
                            .unwrap_or_else(|| b"not found".to_vec());
                        let status = if files.iter().any(|(served, _)| *served == path) {
                            "200 OK"
                        } else {
                            "404 Not Found"
                        };
                        let head = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(&body);
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
                }
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            shutdown: shutdown_tx,
            log,
        }
    }

    fn requested(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
    }
}

fn sign(manifest: &mut Manifest, dir: &Path) -> String {
    // Prefer the system openssl so the test also covers the release-script
    // path, but fall back to ring where `pkeyutl` cannot do Ed25519
    // (OpenSSL 1.1.1, still the default on macOS CI runners).
    if let Some(public) = try_openssl_sign(manifest, dir) {
        return public;
    }
    ring_sign(manifest)
}

fn try_openssl_sign(manifest: &mut Manifest, dir: &Path) -> Option<String> {
    let key = dir.join("signing.key");
    let generated = Command::new("openssl")
        .args(["genpkey", "-algorithm", "ed25519", "-out"])
        .arg(&key)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !generated {
        eprintln!("openssl cannot generate Ed25519 keys; signing with ring");
        return None;
    }
    let payload = dir.join("payload.bin");
    std::fs::write(&payload, manifest.signing_payload()).unwrap();
    let signature = dir.join("payload.sig");
    let signed = Command::new("openssl")
        .args(["pkeyutl", "-sign", "-inkey"])
        .arg(&key)
        .arg("-in")
        .arg(&payload)
        .arg("-out")
        .arg(&signature)
        .output()
        .unwrap();
    if !signed.status.success() {
        eprintln!(
            "openssl cannot sign Ed25519 ({}); signing with ring",
            String::from_utf8_lossy(&signed.stderr).trim()
        );
        return None;
    }
    let der = Command::new("openssl")
        .args(["pkey", "-in"])
        .arg(&key)
        .args(["-pubout", "-outform", "DER"])
        .output()
        .unwrap();
    if !der.status.success() || der.stdout.len() < 32 {
        eprintln!("openssl cannot export the Ed25519 key; signing with ring");
        return None;
    }
    manifest.signature =
        base64::engine::general_purpose::STANDARD.encode(std::fs::read(&signature).unwrap());
    Some(base64::engine::general_purpose::STANDARD.encode(&der.stdout[der.stdout.len() - 32..]))
}

fn ring_sign(manifest: &mut Manifest) -> String {
    let pkcs8 =
        ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let public = base64::engine::general_purpose::STANDARD.encode(pair.public_key().as_ref());
    let signature = pair.sign(&manifest.signing_payload());
    manifest.signature = base64::engine::general_purpose::STANDARD.encode(signature.as_ref());
    public
}

/// Pack a minimal `*.app` bundle the way `scripts/release-update.sh` does.
fn pack_bundle(dir: &Path, artifact: &Path, body: &str) {
    let stage = dir.join("stage");
    std::fs::create_dir_all(stage.join("LazeCode.app/Contents/MacOS")).unwrap();
    std::fs::write(stage.join("LazeCode.app/Contents/Info.plist"), "<plist/>").unwrap();
    std::fs::write(stage.join("LazeCode.app/Contents/MacOS/LazeCode"), body).unwrap();
    let file = std::fs::File::create(artifact).unwrap();
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);
    tar.append_dir_all("LazeCode.app", stage.join("LazeCode.app"))
        .unwrap();
    tar.into_inner().unwrap().finish().unwrap();
    let _ = std::fs::remove_dir_all(&stage);
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("laze-updater-e2e-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn full_update_flow_over_real_http_installs_the_bundle() {
    let dir = scratch("flow");
    let artifact = dir.join("releases/LazeCode-9.9.9-macos-aarch64.app.tar.gz");
    std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    pack_bundle(&dir, &artifact, "updated binary bytes");
    let sha256 = {
        use ring::digest::{digest, SHA256};
        hex(digest(&SHA256, &std::fs::read(&artifact).unwrap()).as_ref())
    };

    // Bind the server socket first so the artifact URL is known before the
    // manifest is signed (the URL is part of the signed payload).
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let manifest_url = "/latest.json";
    let artifact_url = "/releases/LazeCode-9.9.9-macos-aarch64.app.tar.gz";

    let mut manifest = Manifest {
        version: "9.9.9".into(),
        url: format!("{base}{artifact_url}"),
        sha256,
        signature: String::new(),
        notes: "E2E bundle update".into(),
        date: None,
        min_os: None,
        arch: None, // no gate: the test must run on any CI arch
        kind: "app-bundle".into(),
    };
    let key_dir = dir.join("keys");
    std::fs::create_dir_all(&key_dir).unwrap();
    let public = sign(&mut manifest, &key_dir);

    let server = LocalServer::start_on(
        listener,
        vec![
            (manifest_url.into(), serde_json::to_vec(&manifest).unwrap()),
            (artifact_url.into(), std::fs::read(&artifact).unwrap()),
        ],
    );
    let server = &server;

    // 1. check(): fetch + verify signature + platform gate + version compare.
    let outcome = updater::check(&format!("{}/latest.json", server.url), "0.4.0", &public)
        .expect("check must succeed");
    assert!(outcome.update_available);
    assert_eq!(outcome.available_version(), "9.9.9");
    assert_eq!(outcome.manifest.artifact_kind(), ArtifactKind::AppBundle);

    // 2. download_and_verify(): size-capped download + SHA-256 match.
    let staged = dir.join("staged/LazeCode-9.9.9");
    updater::download_and_verify_limited(
        &outcome.manifest,
        &public,
        &staged,
        ARTIFACT_MAX_BYTES,
        &mut |_downloaded, _total| {},
    )
    .expect("download must succeed");

    // 3. install_artifact(): extract, swap a fake installation, keep backup.
    let target_bundle = dir.join("Applications/LazeCode.app");
    std::fs::create_dir_all(target_bundle.join("Contents/MacOS")).unwrap();
    std::fs::write(target_bundle.join("Contents/MacOS/LazeCode"), "old binary").unwrap();
    let report = updater::install_artifact(&staged, &target_bundle, ArtifactKind::AppBundle)
        .expect("install must succeed");
    assert_eq!(
        std::fs::read_to_string(target_bundle.join("Contents/MacOS/LazeCode")).unwrap(),
        "updated binary bytes"
    );
    assert!(report.backup.is_some());

    // The updater fetched both the manifest and the artifact over HTTP.
    let requested = server.requested();
    assert!(
        requested.iter().any(|path| path == "/latest.json"),
        "manifest must be fetched over HTTP: {requested:?}"
    );
    assert!(
        requested.iter().any(|path| path.ends_with(".tar.gz")),
        "artifact must be fetched over HTTP: {requested:?}"
    );

    // 4. A tampered manifest must never install: flipping the hash without
    //    re-signing fails the signature check before any download.
    let mut tampered = manifest.clone();
    tampered.sha256 = "00".repeat(32);
    let unsigned_err = updater::download_and_verify_limited(
        &tampered,
        &public,
        &dir.join("staged/unsigned"),
        ARTIFACT_MAX_BYTES,
        &mut |_, _| {},
    )
    .expect_err("a tampered manifest must fail");
    assert!(matches!(unsigned_err, UpdateError::SignatureInvalid));

    // 5. A compromised mirror serves different bytes; a re-signed manifest
    //    with the wrong hash must fail the checksum comparison.
    let mut mirrored = manifest.clone();
    mirrored.sha256 = "00".repeat(32);
    let public = sign(&mut mirrored, &key_dir);
    let mismatch_err = updater::download_and_verify_limited(
        &mirrored,
        &public,
        &dir.join("staged/mirrored"),
        ARTIFACT_MAX_BYTES,
        &mut |_, _| {},
    )
    .expect_err("checksum mismatch must fail");
    assert!(matches!(mismatch_err, UpdateError::ChecksumMismatch { .. }));
}

/// Talks to the live update endpoint (`Settings::update_endpoint` default).
/// Ignored by default: requires network access and a published release.
#[test]
#[ignore]
fn real_published_manifest_check() {
    let endpoint = std::env::var("LAZE_UPDATE_ENDPOINT").unwrap_or_else(|_| {
        "https://raw.githubusercontent.com/jiangtaiduo/LazeCode/main/latest.json".into()
    });
    let public =
        std::env::var("LAZE_UPDATE_PUBKEY").unwrap_or_else(|_| default_pinned_public_key());
    let outcome = updater::check(&endpoint, "0.0.1", &public)
        .expect("published manifest must fetch, verify and platform-check");
    println!(
        "published version {} (current 0.0.1, update_available={})",
        outcome.available_version(),
        outcome.update_available
    );
    let staged = scratch("real").join("artifact");
    updater::download_and_verify(
        &outcome.manifest,
        &public,
        &staged,
        &mut |downloaded, total| {
            println!("  downloaded {} of {:?}", downloaded, total);
        },
    )
    .expect("published artifact must download and verify");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn default_pinned_public_key() -> String {
    // Avoid a cyclic import of settings internals: the pinned key is also
    // asserted by the settings tests; here it is read from a fresh default.
    lazecode_core::Settings::default().update_public_key
}
