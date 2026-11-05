# LazeCode native rewrite — implementation status

> Branch: `feat-rust`. This document is the honest state of the pure-Rust
> rewrite relative to `docs/design/architecture.md`. It is updated with the
> code; if a capability is not listed as implemented, assume it is not.

## 1. What is implemented

### Workspace shell

- `winit` + `egui-winit` + `egui-wgpu`/`wgpu` (Metal), no WebView, no eframe.
- Custom macOS window: transparent titlebar, full-size content view, native
  traffic lights, dark/light palettes derived from the UI design tokens,
  system SF Pro / SF Mono / Hiragino Sans GB fonts.
- Project rail, project tab strip in the titlebar, activity bar, resizable
  explorer/source-control/consoles sidebar, editor stage, resizable bottom
  panel, status bar, toasts.
- Command palette (`⌘K`), find-in-files overlay, settings overlay, project and
  branch popovers, create/delete-branch dialogs, dirty-tab and quit guards.
- English/Chinese chrome for the primary surfaces; language is persisted.
- Workspace persistence (open projects, open file tabs, active tabs, sidebar
  and panel geometry) in
  `~/Library/Application Support/LazeCode/workspace.toml`; settings in
  `settings.toml` next to it.

### Projects and files

- Multi-project workspace, canonicalised paths, recent-project list.
- `ignore`-based project tree (`.gitignore`, `.git/info/exclude`, global
  excludes), with default suppression of `target/`, `node_modules/`, `dist/`,
  `.next/`, `.venv/`, `__pycache__/` and `.git/`.
- Debounced `notify` watcher; clean open buffers reload automatically, dirty
  buffers raise a warning instead of being overwritten.
- Encoding detection (`encoding_rs` + `chardetng`), BOM preservation, EOL
  detection/normalisation, atomic saves, 8 MiB editable-file limit.
- Find in files with case/whole-word/regex options.

### Editor

- `ropey` buffer with cursor/selection, undo/redo grouping, auto-indent,
  indent/outdent, comment toggle, word/line/document motions, page motion.
- Incremental `syntect` highlighting with per-line parse-state cache
  (`default-fancy`, no onig); language detection by path/extension.
- Tabs, dirty dots, breadcrumbs, line numbers, current-line highlight,
  minimap, horizontal/vertical scrolling, selection and clipboard.
- Find/replace within the document, regex/whole-word/case options.
- IME preedit/commit overlay; Cmd+Z/Shift+Z, Cmd+A, Cmd+C/X, Cmd+V, Cmd+S,
  Cmd+F, page/word/line movement.
- Git diff tab (`HEAD → worktree`) with hunk/line rendering and binary guard.

### Git (`gix` + porcelain)

Read path:

- `discover`, worktree resolution, HEAD/branch/detached/unborn state.
- Status computed from HEAD tree vs index vs worktree, including staged and
  unstaged states, untracked files, per-file added/deleted line counts and
  binary detection.
- Branch list (local/remote), tags, ahead/behind against `origin/<branch>`
  via merge-base.
- Log with parent lists, refs, lane graph layout (lanes/edges computed from
  real DAG topology), commit details and changed-file stats; filters for
  author, date (`--since`), path (lockstep HEAD/worktree diff tree walk),
  topo/date/first-parent/no-merges, all-refs (heads+remotes, stash excluded)
  and branch dotted ranges (`left..right` mapped to a hidden-ancestor walk).
- File diff, commit diff stats, blame (porcelain parser), per-file history,
  submodule status, LFS status/track, stash list/show-files.
- Compare branches (merge-base anchored, both directions: commits with
  ahead/behind counts plus `diff --name-status` file lists) and compare
  arbitrary commits.

Write path:

- Stage add/modify/delete (symlinks and executable bit included), unstage to
  HEAD, commit/amend (author/committer from Git config), branch create, delete
  and switch (switch is refused unless the worktree and editor buffers are
  clean; tracked files are materialised/deleted and the index is rewritten).
- The index writer refuses v4, split-index, untracked-cache, FSMonitor and
  resolve-undo indexes instead of silently dropping those extensions.
- Hunk-level stage/unstage/discard from the diff tab through
  `git apply --whitespace=nowarn --check` (worktree, `--cached` and reverse
  paths; the patch is piped over stdin — a temp-file variant on macOS
  flaked at high parallel-test load with `corrupt patch at :N`, stdin is
  observably stable over a 12-run soak).
- Remote sync porcelain: push (`--force-with-lease`, `--set-upstream`),
  pull (merge/rebase/ff-only), fetch (`--all --prune`), clone (non-empty
  target refused); remote list/add/remove/rename/set-url.
- History rewrite porcelain: merge, rebase (+continue/abort/skip),
  cherry-pick (+continue/abort, editor neutralised, empty picks skipped),
  revert, drop (HEAD via reset, others via rebase --onto with a hash
  whitelist), squash (merge-base --is-ancestor ordering; root gotcha
  special-cased), reset (soft/mixed/hard) with a clean-tree gate; sequencer
  state introspection (MERGE_HEAD / rebase-merge|apply / CHERRY_PICK_HEAD /
  REVERT_HEAD) drives a conflict bar with continue/abort routed per kind.
- Stash push/list/show/files/apply/drop; shelves live in
  `.git/lazecode-shelves/*.patch` (save retracts new files, restore, partial
  apply via `--include` gated on a `--check` pre-pass, delete). Porcelain
  rename, tag create (lightweight)/delete.

Not implemented (explicit):

- network credential surfaces beyond the system `credential.helper` (gix
  push/sendpack transport); signed commits/tags.
- interactive rebase todo editor, standalone conflict resolution window (a
  single overlay is equivalent in scope).

### Terminal

- `portable-pty` sessions and the **`rio-vt` 0.5** grid (Rio's embeddable
  VT core, forked from Alacritty); one reader thread per pane, a wait thread
  for reaping, process-group kill on close.
- Cell snapshot decodes Rio's packed `Square` (ContentTag/StyleId/Wide +
  `Extras` zero-width clusters) and the interned style table; bg-only cells
  keep their inline backgrounds.
- Hot scrollback (default 10 000 lines; settings offer 5k/10k/20k/50k),
  resize, keyboard encoding (Ctrl/Alt, application cursor, function keys),
  bracketed paste, IME commit, SGR mouse clicks/wheel, selection and Cmd+C,
  OSC title, bell count.
- Scrollback search runs rio-vt's `RegexSearch` DFAs over the whole grid
  (character classes work; smart-case follows the Alacritty/less
  convention: lowercase queries fold, uppercase queries are
  case-sensitive). Invalid regexes fall back to a literal scan.
- Freeze fix for high-throughput output: PTY-wake notifies are coalesced to
  at most one per 16 ms (frame budget) while explicit state changes stay
  immediate, and the focused pane caps repaints at 60 fps. Verified with a
  500k-line flood: the viewport converges, input stays responsive and a
  12-run soak shows no stall.
- Multiple console tabs per project, restart/stop, per-session title and
  elapsed time; sessions are bound to their project.

Not implemented (explicit):

- cold/disk history and the 10M-logical-line / 10GB soak target;
- kitty graphics, OSC 133 prompt marks, OSC 7/9;9 cwd tracking (rio exposes
  the OSC 7 cwd on `Crosswords.current_directory`; not surfaced yet),
  OSC 52;
- split panes, mouse drag/motion reporting beyond basic clicks/wheel;
- rio-vt's hyperlink/vi-mode surfaces;
- custom GPU offscreen terminal renderer with an integer-pixel grid (the
  current renderer paints cell runs through the egui painter and caches the
  cell geometry per visible row).

### LSP

Not implemented in this pass. The editor opens, edits, saves, highlights and
searches without a language server. The architecture doc's optional
11-language LSP client remains future work; no Node-based languages are
started.

### Packaging

- `scripts/build-app.sh` builds `dist/LazeCode.app` (Info.plist, ad-hoc
  signature). No notarization, auto-update or crash reporting yet.

## 2. Crate map

| crate | responsibility |
|---|---|
| `lazecode-core` | project/workspace model, tabs, settings, layout, commands, toasts, workspace persistence |
| `lazecode-fs` | tree scan, encoding-aware file IO, search, watchers |
| `lazecode-text` | rope documents, undo, incremental syntax cache, diff model |
| `lazecode-git` | gix read/write porcelain limited to the list above |
| `lazecode-terminal` | PTY lifecycle, VT grid, snapshot, key/mouse encoding, search |
| `lazecode-ui` | theme tokens, icons, widgets, all views and overlays |
| `lazecode-app` | winit/egui-wgpu shell, event routing, background Git jobs, app bundle |
| `lazecode-app --bin snapshot` | headless offscreen renderer used for visual verification |

The previous Tauri/React/Monaco application has been removed from the working
tree. It remains available in Git history, but it is no longer tracked, built,
or included in language statistics. This repository now builds only the Rust
workspace under `crates/`.

## 3. Verification performed

```sh
cargo test --workspace          # 52 tests, all passing
cargo clippy --workspace --all-targets   # clean
cargo build --release -p lazecode-app    # native binary
./scripts/build-app.sh --debug           # dist/LazeCode.app launches
cargo run -p lazecode-app --bin snapshot -- /tmp/laze.png "$PWD" editor zh
```

The snapshot binary renders the real UI offscreen (same draw path as the app)
and is the visual regression harness while the development Mac is locked.
It has been used to verify: editor + CJK, terminal, Git log/source control,
command palette, settings, and the no-project welcome screen.

## 4. Deliberate decisions

1. **Stock `alacritty_terminal`, no fork in this pass.** Eviction capture,
   kitty graphics and the cold-history pipeline need a VT fork; the current
   app uses the stock crate with a bounded hot grid. This is the largest
   deviation from the architecture document and is tracked as the next
   terminal milestone.
2. **Editor inside `lazecode-ui` + `lazecode-text`.** The architecture doc put
   the editor in the UI crate; the rope/syntax/diff engine is split into
   `lazecode-text` so it can be tested without egui.
3. **One native window.** The design's conflict/merge standalone windows are
   not implemented because merge is not implemented.
4. **No VM/PTY for the offscreen harness.** The snapshot binary reuses the
   real `lazecode_ui::draw` path and egui-wgpu; it does not mock widgets.
5. **Unsaved-change guards cover the window close button, `⌘W`, the app-menu
   Quit item and `⌘Q`; Dock Quit / Force Quit / OS logout do not pass through
   the window-close path and are not intercepted.** A future
   `NSApplicationDelegate` hook is required for that last case.
6. **Build directories are suppressed even when Git does not ignore them.**
   `target/` is added to this repository's `.gitignore`; the walkers also
   suppress common build/dependency folders so a first-run status view does
   not enumerate tens of thousands of generated files.
7. **Language support covers the primary chrome.** Dynamic Git/terminal
   system messages and a few secondary labels remain English; they are not
   silently presented as translated.
8. **Legacy tree removed.** The previous Tauri/React/Monaco tree is reachable
   from Git history but is no longer part of the working tree or Cargo
   workspace.
