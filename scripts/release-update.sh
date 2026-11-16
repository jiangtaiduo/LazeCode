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
# COPYFILE_DISABLE=1 stops macOS tar from adding `._*` AppleDouble sidecars
# for extended attributes; they would appear as extra `.app` roots to the
# updater's extractor.
COPYFILE_DISABLE=1 tar -czf "$ARTIFACT" -C dist LazeCode.app
# Fail closed if the archive still carries metadata or another `.app` root.
python3 - "$ARTIFACT" <<'PY'
import sys, tarfile
with tarfile.open(sys.argv[1]) as archive:
    names = archive.getnames()
roots = {name.split("/", 1)[0] for name in names if name.strip()}
metadata = [name for name in names if any(part.startswith("._") or part == ".DS_Store" for part in name.split("/"))]
if roots != {"LazeCode.app"} or metadata:
    sys.exit(f"bad bundle archive: roots={sorted(roots)} metadata={metadata[:3]}")
PY

# 3. Hash the artifact and gather readable release notes from the
#    changelogs. Both the notes and the date are part of the manifest; the
#    notes are signed, the date is display-only.
SHA256="$(shasum -a 256 "$ARTIFACT" | awk '{print $1}')"
DATE="$(date -u +%Y-%m-%d)"
if [[ -n "${UPDATE_NOTES:-}" ]]; then
  NOTES="$UPDATE_NOTES"
else
  NOTES="$(python3 - "$VERSION" "$ROOT/CHANGELOG.md" "$ROOT/CHANGELOG.zh-CN.md" <<'PY'
import re
import sys
from pathlib import Path

version = sys.argv[1]
sections = []
for path in sys.argv[2:]:
    text = Path(path).read_text(encoding="utf-8")
    match = re.search(r"^## \[" + re.escape(version) + r"\][^\n]*$", text, re.MULTILINE)
    if not match:
        continue
    start = match.end()
    following = re.search(r"^## \[", text[start:], re.MULTILINE)
    body = (text[start:start + following.start()] if following else text[start:]).strip()
    if body:
        sections.append(body)
print("\n\n---\n\n".join(sections) if sections else f"LazeCode {version}")
PY
)"
fi

BASE_URL="${UPDATE_BASE_URL:-https://raw.githubusercontent.com/jiangtaiduo/LazeCode/main}"
URL="${BASE_URL%/}/releases/$(basename "$ARTIFACT")"
PAYLOAD_FILE="$(mktemp)"
python3 - "$VERSION" "$URL" "$SHA256" "$NOTES" "$UPDATER_ARCH" > "$PAYLOAD_FILE" <<'PY'
import json
import sys

version, url, sha256, notes, arch = sys.argv[1:6]
print(
    json.dumps(
        {
            "version": version,
            "url": url,
            "sha256": sha256,
            "notes": notes,
            "min_os": "",
            "arch": arch,
            "kind": "app-bundle",
        },
        separators=(",", ":"),
        ensure_ascii=False,
    ),
    end="",
)
PY
# No `-rawin`: OpenSSL 1.1.1 does not know it, while Ed25519 signs the
# input bytes as the raw message in every supported version.
SIGNATURE="$(openssl pkeyutl -sign -inkey "$KEY" -in "$PAYLOAD_FILE" | base64)"
MANIFEST_OUT_PATH="${MANIFEST_OUT:-$ROOT/latest.json}"
python3 - "$PAYLOAD_FILE" "$SIGNATURE" "$DATE" "$MANIFEST_OUT_PATH" <<'PY'
import json
import sys
from pathlib import Path

payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
payload["signature"] = sys.argv[2]
payload["date"] = sys.argv[3]
Path(sys.argv[4]).write_text(
    json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
    encoding="utf-8",
)
PY
rm -f "$PAYLOAD_FILE"

# 4. Emit the manifest (already written above) and report the publish paths.
echo
echo "Staged release $VERSION:"
echo "  artifact : releases/$(basename "$ARTIFACT")"
echo "  manifest : ${MANIFEST_OUT:-$ROOT/latest.json}"
echo
echo "Publish: git add releases/$(basename "$ARTIFACT") latest.json && git commit && git push"
echo "Verify locally before publishing:"
echo "  shasum -a 256 '$ARTIFACT'   # must print $SHA256"
