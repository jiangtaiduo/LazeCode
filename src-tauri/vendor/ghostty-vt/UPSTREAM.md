# Vendored libghostty-vt provenance

- Vendored on 2025-09-17 for macOS arm64.
- Upstream: <https://github.com/ghostty-org/ghostty>
- Build command: `zig build -Demit-lib-vt=true -Doptimize=ReleaseFast`
- The exact upstream commit was **not recorded** when this copy was made.
- Headers under `include/` are copied from the same upstream checkout.

Until this directory is rebuilt with a pinned revision, treat the binary as an
opaque build artifact. To make it reproducible:

```bash
GHOSTTY_REF=<commit-or-tag> npm run build:vt-lib
```

That writes `build-info.txt` (commit, Zig version, timestamp) and
`SHA256SUMS` next to the library.
