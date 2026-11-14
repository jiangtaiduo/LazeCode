# Contributing

Thanks for helping. LazeCode is a native macOS application written in Rust:
`winit` + `egui-wgpu` for the shell and a small workspace under `crates/`.
There is no WebView, JavaScript or Node runtime.

## Build

Requirements: macOS 13+, Xcode command-line tools, and the Rust toolchain
pinned by `rust-toolchain.toml`.

```sh
cargo run -p lazecode-app
./scripts/build-app.sh          # dist/LazeCode.app
./scripts/build-app.sh --debug  # faster debug bundle
```

## Checks

All four must pass before a pull request:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p lazecode-app
```

UI changes should include a fresh `snapshot` image or a recording:

```sh
cargo run -p lazecode-app --bin snapshot -- /tmp/laze.png "$PWD" editor zh
```

## Layout

| crate | responsibility |
|---|---|
| `lazecode-core` | projects, tabs, settings, commands, persistence |
| `lazecode-fs` | tree scanning, encoding-aware IO, search, watchers |
| `lazecode-text` | rope documents, undo, syntax cache, diff |
| `lazecode-git` | `gix` read/write porcelain subset |
| `lazecode-terminal` | PTY lifecycle, VT grid, snapshots, input/search |
| `lazecode-ui` | theme, widgets, views, overlays |
| `lazecode-app` | winit/egui-wgpu shell, event routing, jobs |

## Guidelines

- Keep UI work off the winit thread when it can block or take unbounded time.
- Every background queue must be bounded; every cache needs an eviction rule.
- Never silently weaken a safety check (paths, Git index, file encoding).
- Add a regression test for every bug fix.
- Keep English and Simplified Chinese user-facing docs in sync.
- Do not include build output (`.app`, `target/`), signing keys or real user data.

## Releases and updates

The in-app updater fetches a signed `latest.json` from the repository's
default branch and installs the announced artifact after verifying its
Ed25519 signature and SHA-256. To publish a release:

1. Bump the workspace version in `Cargo.toml`.
2. Run `scripts/release-update.sh` (builds, packs the `.app` bundle,
   hashes and signs the manifest). The signing key lives outside the
   repository — see the script header for the expected path and format.
3. `git add releases/ latest.json`, commit and push. No GitHub Release API
   access is needed; the raw URLs are the update channel.

The tag-triggered `release.yml` workflow additionally builds a CI artifact
for the GitHub Releases page; its bytes differ from the locally signed
artifact, so the updater channel remains the signed `releases/` artifact.
