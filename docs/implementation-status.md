# LazeCode native rewrite — implementation status

> Branch: `feat-ui-redesign`. This document is the honest state of the pure-Rust
> rewrite relative to `docs/design/architecture.md`. It is updated with the
> code; if a capability is not listed as implemented, assume it is not.

## 1. What is implemented

### Workspace shell

- `winit` + `egui-winit` + `egui-wgpu`/`wgpu` (Metal), no WebView, no eframe.
- Custom macOS window: transparent titlebar, full-size content view, native
  traffic lights, dark/light palettes derived from the UI design tokens,
  system SF Pro / PingFang SC / SF Mono / Menlo stack. Settings → Appearance,
  Editor and Terminal choose installed families through `fontdb` (including
  macOS `AssetsV2` fonts); Chinese UI puts PingFang SC first for uniform
  mixed-script metrics, English keeps SF Pro first, and SF Pro uses the
  17pt Text optical size instead of the 28pt Display default. Editor and
  terminal families are independent (terminal "Match editor" by default).
- Project rail (one tile per open project, console-activity rings, fixed
  open-project action), titlebar tool group and active-project switcher,
  resizable explorer/source-control/consoles sidebar, editor stage,
  resizable bottom panel, status bar, toasts.
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
- Image tabs: GIF animates through the egui loader (cache flushed when the
  GIF tab set changes); other formats decode on a worker thread into a
  bounded cache (`crates/lazecode-ui/src/image_cache.rs`) — pixels are
  downscaled to the viewport, retained bytes capped at 64 MiB, failed
  decodes negatively cached for 2 s, entries dropped with their tab.

### Git (`gix` + porcelain)

Read path:

- `discover`, worktree resolution, HEAD/branch/detached/unborn state.
- Status computed from HEAD tree vs index vs worktree, including staged and
  unstaged states, untracked files, per-file added/deleted line counts and
  binary detection. Ignored files/directories come from a native `gix-dir`
  walk (no `git status` subprocess), cached per worktree and invalidated by
  watcher events (events under default-ignored directories invalidate the
  cache without triggering a full refresh).
- Refresh architecture: watcher batches refresh the snapshot (`.git`
  metadata events are scoped to the top-level repository); watcher tree
  reloads run as background jobs carrying the project's custom ignore
  patterns and coalesce bursts. The active checkout additionally polls every
  two seconds, cheaply — it fingerprints `.git` metadata (index, HEAD, refs,
  sequencer files) and only runs the full snapshot when that fingerprint
  moved; a 30-second unconditional backstop covers what the fingerprint
  cannot see (linked-worktree gitdirs, a dead watcher). Fetch carries a
  ten-minute watchdog; checkout/reset/merge index rollbacks go through
  `index.lock`.
- Branch list (local/remote), tags, ahead/behind against the configured upstream (falling back to
  `origin/<branch>`) via merge-base.
- Log with parent lists, refs, lane graph layout (lanes/edges computed from
  real DAG topology), commit details and changed-file stats; filters for
  author, date (`--since`), path (lockstep HEAD/worktree diff tree walk),
  topo/date/first-parent/no-merges, all-refs (heads+remotes, stash excluded)
  and branch dotted ranges (`left..right` mapped to a hidden-ancestor walk).
- File diff, commit diff stats, blame (porcelain parser), per-file history,
  submodule status. LFS status/track and stash list/files are native
  (`.gitattributes` read/write + pointer detection); remote LFS pull is
  explicitly unimplemented (the network transport is a separate milestone).
  The `git` binary remains only for blame, network sync and history
  rewrite (except native reset); read-only status/log/diff and the write
  paths above are `gix`/pure Rust.
- Compare branches as a persistent central editor tab (`TabKind::Compare`,
  stable CompareBranchesView page design): upper/lower panes with a
  draggable splitter (persisted in `settings.compare_split`), per-side
  banners + Swap on the upper pane, per-side filters (search+regex+case,
  User/Date/Branch menus, Paths re-query), commit multi-select (click/Cmd/
  Shift/⌘A/Esc/arrows), double-click commit diff, Load More(+500) with
  limit+1 truncation detection, and a shared draggable details column
  (`settings.compare_details_width`, 24-62%) per pane — single-commit files,
  parent..newest range diff, or whole-side three-dot (`other...side`)
  numstat+name-status lists as a collapsible tree with stable diff-base
  rules. Single-instance tab; closing it clears the session. All
  compare/detail loads run as background jobs (loading/Retry/empty states);
  compare arbitrary commits via the range diff path (`compare_commits`).

Write path:

- Stage add/modify/delete (symlinks and executable bit included), unstage to
  HEAD, commit/amend (author/committer from Git config), branch create, delete
  and switch (switch is refused unless the worktree and editor buffers are
  clean; tracked files are materialised/deleted and the index is rewritten).
- Clean/smudge filters on stage, partial commit, status and checkout:
  `.gitattributes` `text`/`eol`/`ident`/`filter`, `core.autocrlf`/`core.eol`,
  user `filter.<driver>.clean|smudge` commands (required-filter failures
  abort; optional failures degrade to raw bytes), and a native LFS path
  (`filter=lfs` pointer clean/smudge with `.git/lfs/objects` storage; the
  `git-lfs` binary is never executed). Staged/committed blobs are canonical,
  and status compares canonical worktree bytes so CRLF/LFS files do not
  appear modified merely because of filters.
- Branch checkout is transactional: every affected path is snapshotted,
  materialisation/index/HEAD updates run under that journal, and any failure
  restores the worktree and index instead of leaving a hybrid tree.
- Unified-diff apply/reverse/cached-apply is pure Rust (`crates/lazecode-git/
  src/patch.rs`): add/delete/modify, multiple files, mode changes,
  `\ No newline at end of file`, exact context matching and worktree
  rollback on write failure. Binary and rename patches are rejected
  explicitly rather than applied incorrectly. `git apply` is no longer used.
- Commit and partial-commit now run the standard hooks in git order:
  `pre-commit`, `prepare-commit-msg`, `commit-msg`, `post-commit`. A failing
  hook aborts the commit; a `commit-msg` rewrite is used as the final message.
  Branch switches run `post-checkout` (its failure is logged, not fatal, like
  git). Hooks run as user external processes (not the `git` binary).
- The index writer refuses v4, split-index, untracked-cache, FSMonitor and
  resolve-undo indexes instead of silently dropping those extensions.
- Hunk-level stage/unstage/discard from the diff tab uses the pure-Rust
  patch applier (worktree, `--cached` and reverse paths). `git init` and
  per-path/per-directory discard also use `gix`/direct index writes; only the
  still-listed porcelain read/history paths below invoke the `git` binary.
- `pending_count` is native merge-base/count over the configured upstream.
- Native remote push, fetch (all remotes, prune), clone and
  fast-forward-only pull via gix's blocking transport (reqwest + rustls).
  Push speaks receive-pack directly, builds a zlib pack, parses
  `report-status`, updates the remote-tracking ref and writes
  `branch.<name>.remote/merge`; non-fast-forward is rejected client-side and
  force still carries the last known remote oid (lease semantics).
  Fast-forward pull verifies the merge-base and uses the native hard reset.
- Remote sync porcelain (still uses the `git` binary): pull merge/rebase,
  remote list/add/remove/rename/set-url.
- Native `reset --soft/--mixed/--hard` (branch or detached HEAD, index and
  worktree transaction with rollback); drop-HEAD/squash reuse it.
- Native merge via `gix::merge_commits`: fast-forward/up-to-date detection,
  merged tree, unmerged index stages, conflict-marker worktree, `MERGE_HEAD`/
  `ORIG_HEAD`/`MERGE_MSG`, and a real merge commit for clean merges.
- Native clean rebase: replays first-parent commits with `merge_trees`,
  preserves author identity, then hard-resets the branch; conflicted rebases
  fall back to the porcelain sequencer without touching refs/worktree first.
- Native sequencer: cherry-pick/revert (clean + conflict, author preserved),
  conflicted rebase stop/continue/abort/skip with `rebase-merge/` state,
  native `conflict_list`/`resolve_take`/`merge_file_content`; merge abort/
  continue and interactive rebase still use the `git` sequencer. Drop uses
  native reset for HEAD and rebase --onto otherwise; squash keeps its
  ancestry guards. Sequencer
  state introspection (MERGE_HEAD / rebase-merge|apply / CHERRY_PICK_HEAD /
  REVERT_HEAD) drives a conflict bar with continue/abort routed per kind.
- Native stash: push (message, `-u` for untracked, `--keep-index`),
  list/files/apply/pop/drop, including stash/index/untracked commits and
  the `refs/stash` reflog. Apply performs a text three-way merge with
  conflict markers and unmerged index stages; large LFS objects stream from
  the native object store.
  Stashes are readable by the `git` CLI and vice-versa. Shelves live in `.git/lazecode-shelves/*.patch` (whole-tree
  or path-scoped save — the retract step reuses the per-file discard so
  staged-new files leave both worktree and index — native patch generation,
  native full restore, native path-filtered partial restore, delete,
  per-file listing for the unshelve dialog). Generated patches are accepted
  by `git apply --check` and preserve missing final newlines; porcelain
  rename, tag create (lightweight)/delete remain.

### Changes list (source-control sidebar)

- Ported from the stable version's commit drawer, on the new sidebar layout:
  changelist groups (default group + named lists + a fixed "Unversioned
  Files" group; untracked files never join a changelist), collapsible groups
  with Expand/Collapse All, tri-state group checkboxes, and a "Merge
  Conflicts" subgroup inside the default list with a Resolve entry.
- Checkbox = include in this commit, nothing checked by default (stable
  semantics); the commit scope is exactly the checked set via
  stage-checked/unstage-excluded, amend exempt. Multi-selection of rows
  (plain/Cmd/Shift) with display-order ranges feeds the context menu.
- Context menus: Show Diff, Jump to Source, Resolve (conflicted), Commit
  File(s)... with an own message, Revert Selected Changes (confirmed;
  batch discard closes tabs of retracted new files), Copy as Patch to
  Clipboard, Move to Changelist, Shelve Changes..., Refresh VCS Changes,
  New Changelist. Changelist create/delete/assign run as background jobs;
  delete first asks Move-to-default vs Rollback.
- Untracked files get the stable untracked menu: Add to VCS (background
  git add with the stable toasts; snapshot refresh moves files out of
  Unversioned), Jump to Source (single file), and a trash-semantics
  Delete… (single file reuses the delete confirm; multi-file confirms once,
  moves everything to the system trash, closes affected editor tabs).
- Stash button opens a dialog (message + include-untracked) instead of a
  silent push; shelve defaults to the checked files (name: file name for a
  single file, default changelist name for many); the unshelve dialog
  restores a file subset into a chosen or new changelist (entry kept).
- Commit dock: message history (10 entries, settings.toml) with a
  refill/clear popover and a Commit & Push button that opens the push
  dialog after success. Non-Git projects get a not-a-repository notice with
  an Initialize Git Repository entry.
- Context menus match the stable drawer item-for-item (tracked + untracked
  branches, greyed placeholders, separators): functional Compare Before
  with Local / Create Patch from Local Changes (save dialog) / History Up
  to Here are wired; group-header right-click carries the whole group's
  file menu.
- Deferred from the stable drawer (audited 2026-11-07, next batches): the
  inline Shelf tab (rename, recently-deleted, shelf file diffs/patches,
  import, silent unshelve shortcut), Group-By-Directory tree mode + ^P +
  Ignored Files group, commit-draft persistence per repo, staged-vs-
  unstaged diff preference on double-click, and shelf rename/delete-files
  backend fns.

Not implemented (explicit):

- network credential surfaces beyond the system `credential.helper` (gix
  push/sendpack transport); signed commits/tags.
- standalone conflict resolution window (a single overlay is equivalent in
  scope); interactive rebase **is** implemented (todo executed via
  GIT_SEQUENCE_EDITOR shim, tested) — reword keeps the original message
  until the dialog gains a message editor.

### Terminal

- `portable-pty` sessions and the **`rio-vt` 0.5** grid (Rio's embeddable
  VT core, forked from Alacritty); one reader thread per pane, a wait thread
  for reaping, process-group kill on close.
- Cell snapshot decodes Rio's packed `Square` (ContentTag/StyleId/Wide +
  `Extras` zero-width clusters) and the interned style table; bg-only cells
  keep their inline backgrounds.
- Hot scrollback (default 10 000 lines; settings offer 5k/10k/20k/50k),
  resize, keyboard encoding (Ctrl/Alt including Ctrl+Alt, application
  cursor, function keys), bracketed paste with control-byte sanitisation,
  IME commit, SGR/X10 mouse press/release/drag/wheel at the pointer cell,
  selection (inclusive endpoints, grapheme clusters) and Cmd+C, OSC title,
  bell count, 256-colour palette, and fractional trackpad wheel accumulation.
- Scrollback search runs rio-vt's `RegexSearch` DFAs over the whole grid
  (character classes work; smart-case follows the Alacritty/less
  convention: lowercase queries fold, uppercase queries are
  case-sensitive). Invalid regexes fall back to a literal scan. Cmd+F
  focuses the find field and suppresses PTY key forwarding while open.
- Freeze fix for high-throughput output: PTY-wake notifies are coalesced to
  at most one per 16 ms (frame budget) with a trailing timer so the final
  frame is never lost; explicit state changes stay immediate, and the
  focused pane caps repaints at 60 fps only while output is recent.
- Multiple console tabs per project (capped at 10 panes per worktree), with
  up to 4 split panes per tab (`Split Right`/`Split Down`, row/column
  equal-size layout, active-pane ring, hover close per pane, pane close
  removes the tab only when the last pane closes). Tab strip stays one entry
  per tab; restart/stop targets one pane, and per-pane title/elapsed time are
  preserved. Sessions are bound to their project. The shell is restricted to
  a login-shell allow-list and common credential environment variables are
  stripped before spawn.
- Bottom-dock height and right-dock width/placement persist across launches.
- Cold/disk scrollback (`lazecode-terminal/src/cold.rs`): evicted rows are
  spooled to a bounded append-only file with stable absolute row ids,
  rehydrated for scroll/search, compacted when dead bytes dominate (the new
  layout commits only after the rewrite succeeds), and deleted on session
  close; spool files of dead processes are swept at startup. Row ids are
  ordered but not contiguous — id gaps from resize/reflow self-eviction and
  zero-length placeholders for oversized rows keep `get_by_id` exact via a
  binary search. Hot memory plateaus at the configured history.
- Kitty graphics: rio-vt parses APC transmit/place/delete with chunking,
  base64 payloads, ids, z/columns/rows/source rects; decoded RGBA images are
  rendered as egui textures above the cell grid. `rio-vt/graphics` is enabled
  for PNG/JPEG payload decoding and Unicode virtual placeholder runs are
  composited above the grid. Each console has a configurable soft cache target
  (Settings → Terminal → Image cache, default 64 MiB); visible placements and
  images transmitted in the last two seconds are protected above it, while a
  4× soft-target hard cap still bounds memory. Strip/frame animations remain
  out of scope.
- Kitty keyboard and modifyOtherKeys: protocol flag stack, key codes, event
  types (release/repeat), associated text, and xterm `CSI 27;…~` are encoded
  in `input.rs`; legacy bytes remain when disabled.
- Accessibility mirror: `accesskit::Role::Terminal` node with
  viewport-bounded text, geometry/cursor/scroll/focus label and selected
  text description.
- New Window (File menu / Cmd+Shift+N) spawns an independent LazeCode
  process. Windows do not share in-memory state; the shared workspace file
  is last-writer-wins.

Not implemented (explicit):

- kitty strip/frame animations;
- OSC 133 prompt marks, OSC 7/9;9 cwd tracking and OSC 52;
- rio-vt's hyperlink/vi-mode surfaces;
- same-process multi-window model sharing (New Window is process-isolated);
- custom GPU offscreen terminal renderer with an integer-pixel grid (the
  current renderer paints cell runs through the egui painter and caches the
  cell geometry per visible row).

### LSP

The optional 11-language language-server client now lives in
`lazecode-core::lsp` and is wired into the editor. It is **default-off**
(Settings → Editor → Language servers (experimental)); the product default is
syntax highlighting only.

- one server per `(workspace root, language)`; candidates match the stable
  table (`pyright`/`basedpyright`/`pylsp`, `gopls`, `rust-analyzer`,
  `clangd`, `jdtls`, `solargraph`, `intelephense`,
  `kotlin-language-server`, `sourcekit-lsp`, `lua-language-server`);
- JSON-RPC over stdio with Content-Length framing, bounded reader/writer
  queues, per-request deadlines, server-request replies
  (`workspace/configuration`, `applyEdit`, progress/registration), process
  -group termination and crash visibility;
- full-text `didOpen`/`didChange` (200 ms debounce)/`didClose` sync with
  versions; UTF-16 ↔ Unicode-scalar position conversion is tested against
  surrogate pairs;
- editor surfaces: inline diagnostics with status-bar counts and hover,
  async hover tooltips, completion popup (trigger characters or
  Ctrl+Space), Cmd+Click/F12 definition, Option+Cmd+L formatting,
  F2/workspace rename (open buffers and closed files), Cmd+Shift+O document
  symbols, plus palette entries;
- missing servers or crashes degrade silently to syntax highlighting; no
  Node runtime is started. TS/JS/JSON/CSS/HTML remain highlight-only.

The lifecycle is covered by a mock-server test and an ignored end-to-end
test against real `rust-analyzer`
(`cargo test -p lazecode-core --test lsp_e2e -- --ignored`).

### Updater

`lazecode-core::updater` implements a signed update flow:

- manifest JSON: version, artifact URL, SHA-256, Ed25519 signature, notes,
  min OS/arch; signed canonical payload excludes the signature;
- HTTP/file/local-path check with semver comparison;
- bounded streaming download (512 MiB), SHA-256 and Ed25519 verification
  against the pinned `update_public_key`;
- two install shapes announced by `kind`: `executable` replaces a single
  binary in place; `app-bundle` extracts a gzipped tar of one `*.app` under a
  strict extraction policy, then swaps the whole bundle (Info.plist,
  resources and the ad-hoc signature stay consistent), keeping the previous
  bundle as a `.bak-<nanos>` sibling and rolling back on failure;
- `relaunch` starts the installed bundle with `open -n` so the update takes
  effect immediately after "Restart now";
- settings: `update_endpoint`, `update_public_key`, `auto_check_updates`
  default to the repository's signed channel (raw `main/latest.json` plus the
  pinned release public key) with automatic checking on; pre-updater settings
  with empty strings are migrated to those defaults.
- automatic checks run five seconds after each launch (`last_update_check_time`); they only
  fetch the manifest and never download or install. `ignored_update_version` suppresses the badge/toast
  for one version while leaving manual checks available. Settings → Updates
  shows the last check, the ignored version, the endpoint/key and a manual
  check entry.
- the update page is a state machine (`checking`, `up-to-date`, `available`,
  `downloading`, `installing`, `ready`, `error`) with Markdown release notes
  (scrollable), release date, an ignore-this-version choice, one
  "Update and restart" action, restart-later/restart-now, and a dirty-buffer
  confirmation before relaunch. The title bar shows a green
  `vX available` pill (also progress/restart states) that opens the same
  page. Background checks never install anything.
- `scripts/release-update.sh` builds, packs, hashes and signs a release
  (`releases/*.app.tar.gz` + root `latest.json`); release notes are extracted
  from both changelogs and `date` is added as a display-only manifest field.
  The signing key stays outside the repository. Tag releases mirror the
  committed artifact and a `latest-native.json` copy on the release page,
  while the legacy Tauri `latest.json` asset on older releases is left
  untouched.

### Packaging

- `scripts/build-app.sh` builds `dist/LazeCode.app` (Info.plist stamped with
  the workspace version, ad-hoc signature). No notarization or crash
  reporting yet, and only `aarch64` artifacts are published.

## 2. Crate map

| crate | responsibility |
|---|---|
| `lazecode-core` | project/workspace model, tabs, settings, layout, commands, toasts, workspace persistence, LSP client/diagnostics, updater |
| `lazecode-fs` | tree scan, encoding-aware file IO, search, watchers |
| `lazecode-text` | rope documents, undo, incremental syntax cache, diff model |
| `lazecode-git` | gix read/write porcelain limited to the list above |
| `lazecode-terminal` | PTY lifecycle, VT grid, cold history, snapshot, key/mouse encoding, kitty graphics/keyboard, search |
| `lazecode-ui` | theme tokens, icons, widgets, all views and overlays |
| `lazecode-app` | winit/egui-wgpu shell, event routing, background Git jobs, app bundle |
| `lazecode-app --bin snapshot` | headless offscreen renderer used for visual verification |

The previous Tauri/React/Monaco application has been removed from the working
tree. It remains available in Git history, but it is no longer tracked, built,
or included in language statistics. This repository now builds only the Rust
workspace under `crates/`.

## 3. Verification performed

```sh
cargo test --workspace          # 296 tests, all passing
cargo clippy --workspace --all-targets   # clean
cargo test -p lazecode-core --test lsp_e2e -- --ignored  # real rust-analyzer
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
