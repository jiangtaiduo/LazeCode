//! Verify a signed update manifest with the pinned public key:
//! `cargo run -p lazecode-core --example verify_update_manifest -- latest.json`
//! Used by the release workflow (macOS openssl 1.1.1w cannot do Ed25519).

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
