//! Verify a signed update manifest against the public key pinned in the app.
//!
//! `cargo run -p lazecode-core --example verify_update_manifest -- latest.json`
//!
//! Used by the release workflow: the macOS runner's default `openssl`
//! (1.1.1w) cannot sign or verify Ed25519 through `pkeyutl`, while the app's
//! own verifier uses ring and works everywhere.

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "latest.json".to_string());
    let bytes = std::fs::read(&path)?;
    let manifest = lazecode_core::updater::parse_manifest(&bytes)?;
    let key = lazecode_core::Settings::default().update_public_key;
    lazecode_core::updater::verify_manifest_signature(&manifest, &key)?;
    println!("verified {} ({})", manifest.version, manifest.sha256);
    Ok(())
}
