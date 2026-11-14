#!/usr/bin/env bash
# Build, sign and stage a LazeCode release for the in-app updater.
#
# The updater (crates/lazecode-core/src/updater.rs) fetches `latest.json`
# from the repository's default endpoint, verifies its Ed25519 signature
# against the pinned public key, then downloads the artifact and verifies
# its SHA-256 before installing. This script produces both files:
#
#   releases/LazeCode-<version>-macos-<arch>.app.tar.gz   (kind: app-bundle)
#   latest.json                                            (signed manifest)
#
# Both are written into the repository so `git add` + push publishes them;
# the raw GitHub URLs are what the app fetches.
#
# Signing key (Ed25519 PKCS#8, openssl-compatible): the private key stays
# OUT of the repository, by default
#   ~/.config/lazecode-release/update-signing.key
# override with UPDATE_SIGNING_KEY=/path/to/key. Generate a fresh pair:
#   openssl genpkey -algorithm ed25519 -out "$KEY"
#   openssl pkey -in "$KEY" -pubout -outform DER | tail -c 32 | base64
#
# Usage: scripts/release-update.sh [version]
#   The version defaults to the workspace version in Cargo.toml.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  VERSION="$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
fi
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version '$VERSION' is not x.y.z" >&2
  exit 1
fi

KEY="${UPDATE_SIGNING_KEY:-$HOME/.config/lazecode-release/update-signing.key}"
if [[ ! -f "$KEY" ]]; then
  echo "error: signing key not found at $KEY" >&2
  echo "       generate one with: openssl genpkey -algorithm ed25519 -out '$KEY'" >&2
  exit 1
fi

ARCH="$(uname -m)"           # arm64 / x86_64
case "$ARCH" in
  arm64) UPDATER_ARCH="aarch64" ;;
  x86_64) UPDATER_ARCH="x86_64" ;;
  *) echo "error: unsupported arch $ARCH" >&2; exit 1 ;;
esac
OS="$(uname -s)"
if [[ "$OS" != "Darwin" ]]; then
  echo "error: this script currently packages the macOS bundle only" >&2
  exit 1
fi

# 1. Build and bundle.
./scripts/build-app.sh

# 2. Pack the bundle.
RELEASE_DIR="$ROOT/releases"
mkdir -p "$RELEASE_DIR"
ARTIFACT="$RELEASE_DIR/LazeCode-$VERSION-macos-$UPDATER_ARCH.app.tar.gz"
rm -f "$ARTIFACT"
tar -czf "$ARTIFACT" -C dist LazeCode.app

# 3. Hash and sign the canonical manifest payload. The updater's
#    Manifest::signing_payload() is compact JSON in exactly this field
#    order; sign those bytes verbatim.
SHA256="$(shasum -a 256 "$ARTIFACT" | awk '{print $1}')"
NOTES="LazeCode $VERSION"
URL="https://raw.githubusercontent.com/jiangtaiduo/LazeCode/main/releases/$(basename "$ARTIFACT")"
PAYLOAD="$(printf '{"version":"%s","url":"%s","sha256":"%s","notes":"%s","min_os":"","arch":"%s","kind":"app-bundle"}' \
  "$VERSION" "$URL" "$SHA256" "$NOTES" "$UPDATER_ARCH")"
PAYLOAD_FILE="$(mktemp)"
printf '%s' "$PAYLOAD" > "$PAYLOAD_FILE"
SIGNATURE="$(openssl pkeyutl -sign -inkey "$KEY" -rawin -in "$PAYLOAD_FILE" | base64)"
rm -f "$PAYLOAD_FILE"

# 4. Emit the manifest.
cat > "$ROOT/latest.json" <<EOF
{
  "version": "$VERSION",
  "url": "$URL",
  "sha256": "$SHA256",
  "signature": "$SIGNATURE",
  "notes": "$NOTES",
  "arch": "$UPDATER_ARCH",
  "kind": "app-bundle"
}
EOF

echo
echo "Staged release $VERSION:"
echo "  artifact : releases/$(basename "$ARTIFACT")"
echo "  manifest : latest.json"
echo
echo "Publish: git add releases/$(basename "$ARTIFACT") latest.json && git commit && git push"
echo "Verify locally before publishing:"
echo "  shasum -a 256 '$ARTIFACT'   # must print $SHA256"
