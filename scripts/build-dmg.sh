#!/usr/bin/env bash
# Build a compressed DMG of dist/LazeCode.app for a GitHub release.
#
#   ./scripts/build-dmg.sh [version]
#
# Writes dist/LazeCode_<version>_<arch>.dmg (arch is aarch64 on Apple
# silicon), matching the names used by the 0.2.0 Tauri releases.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APP="$ROOT/dist/LazeCode.app"
if [[ ! -d "$APP" ]]; then
  echo "error: $APP is missing; run ./scripts/build-app.sh first" >&2
  exit 1
fi

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
fi
case "$(uname -m)" in
  arm64) ARCH=aarch64 ;;
  x86_64) ARCH=x86_64 ;;
  *) echo "error: unsupported arch $(uname -m)" >&2; exit 1 ;;
esac

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/LazeCode.app"
ln -s /Applications "$STAGE/Applications"

OUT="$ROOT/dist/LazeCode_${VERSION}_${ARCH}.dmg"
rm -f "$OUT"
hdiutil create -quiet -volname "LazeCode" -srcfolder "$STAGE" \
  -ov -format UDZO "$OUT"
echo "Built $OUT"
