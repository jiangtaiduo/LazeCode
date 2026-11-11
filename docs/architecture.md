# Architecture

[English](architecture.md) | [Simplified Chinese](architecture.zh-CN.md)

LazeCode is one native macOS process. `winit` owns the event loop, `egui` draws
the shell, `wgpu` renders through Metal, and every application service is Rust.
There is no WebView, JavaScript runtime or bundled Node.

## Crates

| crate | responsibility |
|---|---|
| `lazecode-core` | projects, tabs, terminals, settings, commands, persistence, LSP client and diagnostics |
| `lazecode-fs` | project tree, encoding-aware file I/O, search, watcher |
| `lazecode-text` | rope documents, undo, incremental syntax highlighting, diff |
| `lazecode-git` | `gix` status/log/diff and a limited write porcelain |
| `lazecode-terminal` | PTY lifecycle, `alacritty_terminal` grid, snapshots, input/search |
| `lazecode-ui` | design tokens, widgets, views, overlays, i18n |
| `lazecode-app` | winit/egui-wgpu shell, event routing, background jobs |

Dependency direction is one-way:

```text
app ──▶ ui ──▶ core ──▶ fs
 │       │       ├──▶ git ──▶ text ──▶ fs
 │       │       └──▶ terminal
 │       └──▶ text
 └──▶ terminal
```

## Event and data flow

- The winit thread owns `AppModel`, `UiRuntime` and `JobQueue`. It never blocks
  on Git, project scans or file search.
- UI actions push a `Command` into `AppModel::commands`. The app shell executes
  them after rendering, spawning a bounded/keyset worker for I/O.
- Workers send `JobCompletion { key, result }` back through a channel. The
  completion always releases its in-flight key, then updates the model on the
  UI thread.
- `egui` decides when a repaint is needed (`repaint_delay`); the event loop does
  not repaint continuously while idle.
- Terminal sessions have a reader thread (VT parsing and snapshots), a writer
  thread (bounded input queue) and a reaper thread. UI reads only the latest
  snapshot.
- File watchers are polled at a low frequency; change batches reload the tree,
  clean editor buffers and schedule a Git status refresh.
- Language servers are optional and scoped per `(workspace root, language)`.
  Each server has a reader thread and a bounded writer queue; the UI drains
  completed responses once per frame. Documents use full-text `didChange`
  with a 200 ms debounce. Diagnostics stay in `lazecode-core::lsp`;
  `lazecode-ui::lsp_ui` renders hover, completion, the symbol picker,
  formatting/rename edits and the status-bar counts.

## Git

`lazecode-git` uses `gix` directly. Read paths cover status, log, branches,
tags, diffs and commit details. Write paths cover stage/unstage, commit/amend,
branch create/delete and branch switch. The porcelain deliberately refuses
operations it cannot represent safely: submodules, unmerged indexes,
merge/rebase/cherry-pick state, unsupported index extensions and paths outside
the worktree.

## Persistence

- Settings: `~/Library/Application Support/LazeCode/settings.toml`
- Workspace: `~/Library/Application Support/LazeCode/workspace.toml`

Both are written atomically and validated on load.

## Build and release

`cargo build -p lazecode-app` builds the binary.
`./scripts/build-app.sh` produces `dist/LazeCode.app` from
`packaging/macos/Info.plist` and the app icon. Release CI builds and signs the
bundle on `macos-15`; notarisation and auto-update are not implemented yet.

The long-term design baseline lives in
[`design/architecture.md`](design/architecture.md). The implemented capability
matrix and explicit non-goals live in
[`implementation-status.md`](implementation-status.md).
