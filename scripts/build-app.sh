#!/usr/bin/env bash
# Build a self-contained LazeCode.app (ad-hoc signed) into dist/.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PROFILE="${PROFILE:-release}"
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE="debug"
fi
if [[ "$PROFILE" == "release" ]]; then
  cargo build --release -p lazecode-app --bin lazecode
  BIN="$ROOT/target/release/lazecode"
else
  cargo build -p lazecode-app --bin lazecode
  BIN="$ROOT/target/debug/lazecode"
fi
APP="$ROOT/dist/LazeCode.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/LazeCode"
cp "$ROOT/packaging/macos/Info.plist" "$APP/Contents/Info.plist"
# Stamp the workspace version so Finder/About and the app's own
# CFBundleShortVersionString never drift from the binary.
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
plutil -replace CFBundleShortVersionString -string "$VERSION" "$APP/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$VERSION" "$APP/Contents/Info.plist"
cp "$ROOT/packaging/macos/LazeCode.icns" "$APP/Contents/Resources/LazeCode.icns"
printf 'APPL????' > "$APP/Contents/PkgInfo"
# Ad-hoc signature: enough for local use; CI release builds must notarize.
IDENTITY="${CODESIGN_IDENTITY:--}"
codesign --force --deep --sign "$IDENTITY" "$APP" >/dev/null 2>&1 || true
echo "Built $APP"
