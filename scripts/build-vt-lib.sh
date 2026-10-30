#!/usr/bin/env bash
# Rebuild src-tauri/vendor/ghostty-vt from a pinned Ghostty revision.
#
# GHOSTTY_REF is REQUIRED (commit or tag). The vendored binary is a build
# artifact; never rebuild it from an unpinned branch and commit the result.
#
#   GHOSTTY_REF=v1.2.0 npm run build:vt-lib
#
# Requires Zig (the version Ghostty declares in build.zig.zon) and a network
# connection for the clone. The build intentionally uses ReleaseFast; an
# unoptimized build parses ~680x slower.
set -euo pipefail

: "${GHOSTTY_REF:?Set GHOSTTY_REF to a Ghostty commit or tag (e.g. GHOSTTY_REF=v1.2.0 npm run build:vt-lib)}"

if ! command -v zig >/dev/null 2>&1; then
  echo "error: zig is not installed (Ghostty requires the version in build.zig.zon)" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
vendor="$repo_root/src-tauri/vendor/ghostty-vt"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/lazecode-ghostty.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

git clone --filter=blob:none https://github.com/ghostty-org/ghostty.git "$tmp/ghostty"
git -C "$tmp/ghostty" checkout --detach "$GHOSTTY_REF"

(
  cd "$tmp/ghostty"
  zig build -Demit-lib-vt=true -Doptimize=ReleaseFast
)

mkdir -p "$vendor/lib"
cp "$tmp/ghostty/zig-out/lib/libghostty-vt.a" "$vendor/lib/libghostty-vt.a"

rm -rf "$vendor/include"
cp -R "$tmp/ghostty/include" "$vendor/include"

commit="$(git -C "$tmp/ghostty" rev-parse HEAD)"
zig_version="$(zig version)"
sha="$(shasum -a 256 "$vendor/lib/libghostty-vt.a" | awk '{print $1}')"

cat > "$vendor/build-info.txt" <<EOF
ghostty_ref=$GHOSTTY_REF
ghostty_commit=$commit
zig_version=$zig_version
built_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
libghostty_vt_sha256=$sha
EOF

cat > "$vendor/SHA256SUMS" <<EOF
$sha  lib/libghostty-vt.a
EOF

echo "wrote $vendor/lib/libghostty-vt.a"
echo "wrote $vendor/build-info.txt"
echo "wrote $vendor/SHA256SUMS"
