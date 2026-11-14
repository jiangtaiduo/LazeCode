# LazeCode

A fast, native macOS editor, Git client and terminal — written in Rust.

LazeCode is a focused coding workspace for people who live in a terminal:
multiple projects in one window, a real editor, source-control status and
history, and project-bound console sessions. There is no WebView, no embedded
JavaScript, and no bundled runtime.

> The previous Tauri/React/Monaco implementation was removed from the working
> tree after the native port became the only build target. It remains available
> in Git history. The exact capability matrix and known gaps are in
> [`docs/implementation-status.md`](docs/implementation-status.md).

## Highlights

- **Native shell** — `winit` + `egui-wgpu`/`wgpu` on Metal, custom titlebar,
  native traffic lights, native menu bar and accelerators, light/dark palettes
  derived from the LazeCode design tokens, English/Chinese chrome.
- **Workspace** — project rail with console-activity indicators, explorer
  with `.gitignore` awareness, file watcher, source-control sidebar, command
  palette (`⌘K`), find in files, workspace/session persistence.
- **Editor** — rope buffer, incremental syntax highlighting, tabs, undo/redo,
  multi-line indent, find/replace, diff view, IME, atomic saves, encoding and
  EOL preservation.
- **Git** — status (staged/unstaged/untracked, line counts, ahead/behind),
  branch/tag/worktree lists, commit log with lane graph, commit details,
  stage/unstage, commit/amend, merge, rebase, cherry-pick, stash, push/pull
  (native transport), reset, hunk-level staging and discard.
- **Signed auto-update** — the app checks a signed release manifest at
  startup and verifies the artifact's SHA-256 before installing; the whole
  `.app` bundle is swapped atomically with a rollback backup.
- **Terminal** — real PTY sessions, 10k-line scrollback by default, multiple
  consoles per project, scrollback search, selection/copy, bracketed paste,
  SGR mouse clicks and wheel, IME.
- **Bilingual UI** — English and 简体中文 for the primary surfaces.

## Build and run

Requirements: macOS 13+, Rust stable (1.98+; the current toolchain is pinned by
`rust-toolchain.toml`), Xcode command-line tools.

```sh
cargo run -p lazecode-app
```

Tests and lint:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
```

Build a launchable app bundle:

```sh
./scripts/build-app.sh          # dist/LazeCode.app
./scripts/build-app.sh --debug  # faster, unoptimised
```

The bundle is ad-hoc signed. For distribution it still needs a Developer ID
signature and notarisation.

## Updates

The app polls a signed release manifest (`latest.json` at the repository
root) once at startup and can then download, verify (Ed25519 signature +
SHA-256) and install the update by swapping the whole `.app` bundle; the
previous build is kept as a backup. Publish a release like this:

```sh
scripts/release-update.sh          # build, pack, hash, sign → releases/ + latest.json
git add releases/ latest.json && git commit && git push
```

The signing key lives outside the repository (default
`~/.config/lazecode-release/update-signing.key`); only its public half is
pinned in the app. See `scripts/release-update.sh` for the exact steps.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `⌘K` | Command palette |
| `⌘P` | Quick open |
| `⇧⌘F` | Find in files |
| `⌘B` | Toggle sidebar |
| `⌘J` | Toggle bottom panel |
| `⌘S` | Save |
| `⌘Z` / `⇧⌘Z` | Undo / redo |
| `⌘F` | Find in editor |
| `⌘W` | Close tab |
| `⌘,` | Settings |
| `⌘+` / `⌘-` / `⌘0` | Zoom in / out / reset |
| `⌘Q` | Quit (guards unsaved buffers) |

## Architecture

| crate | responsibility |
|---|---|
| `lazecode-core` | projects, tabs, settings, layout, commands, persistence |
| `lazecode-fs` | tree scanning, encoding-aware IO, search, watchers |
| `lazecode-text` | rope documents, undo, syntax cache, diff model |
| `lazecode-git` | `gix` read/write porcelain subset |
| `lazecode-terminal` | PTY lifecycle, VT grid, snapshots, input/search |
| `lazecode-ui` | design tokens, widgets, views, overlays |
| `lazecode-app` | winit/egui-wgpu shell, event routing, background jobs |

Design and long-term architecture: [`docs/design/architecture.md`](docs/design/architecture.md).
Implementation status and explicit non-goals for this pass:
[`docs/implementation-status.md`](docs/implementation-status.md).

## Configuration

- Settings: `~/Library/Application Support/LazeCode/settings.toml`
- Workspace/session: `~/Library/Application Support/LazeCode/workspace.toml`

Both are plain TOML and may be edited while the app is stopped. The app writes
them atomically.

## Status

This is a working native release. The editor, terminal, Git read/write
surface (including branch compare, worktrees, stash and native push/pull),
LSP client and signed updater are implemented as described above. Known gaps
are tracked in [`docs/implementation-status.md`](docs/implementation-status.md):
the app is ad-hoc signed (distribution still needs Developer ID + notarisation),
only an Apple-silicon build is published, TS/JS/JSON/CSS/HTML stay
highlight-only (no language servers), remote LFS and interactive rebase remain
porcelain, and the terminal still paints cell runs through egui instead of a
custom GPU grid.

## License

MIT. See [`LICENSE`](LICENSE).
