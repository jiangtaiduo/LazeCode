# Changelog

[English](CHANGELOG.md) | [Simplified Chinese](CHANGELOG.zh-CN.md)

All notable changes to LazeCode are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Shell layout: open projects now live in a wider vertical project rail with
  a fixed “open project” button and per-project console activity rings; the
  Project, Commit, Compare, Git Log and Terminal actions moved to the
  titlebar. The titlebar now uses the packaged application icon, its tool
  icons are larger, and the rail is 50px wide with compact project tiles.
  The active-project dropdown is a quiet switcher, and Settings sits at the
  far right after search.
- Interface, editor and terminal font pickers in Settings, backed by the
  installed-font catalog (`fontdb`, including macOS `AssetsV2` fonts).
  Editor and terminal monospace families are independent; the terminal
  follows the editor unless overridden.
- Native history sequencer: cherry-pick/revert with clean and conflict paths,
  conflicted rebase stop/continue/abort/skip, native conflict list/content/
  resolution helpers, and author-preserving replay. Interactive rebase and
  merge abort/continue remain porcelain.
- Terminal cold/disk scrollback with bounded spool, stable row ids,
  rehydration, search and cleanup; kitty graphics transmission/placement/
  deletion with bounded image cache; kitty keyboard/modifyOtherKeys with
  release/repeat events; accesskit terminal text mirror.
- Signed updater: manifest check, semver gate, bounded download, SHA-256 +
  Ed25519 verification, atomic executable replacement with rollback, Settings
  → Updates UI and auto-check.
- File → New Window (Cmd+Shift+N) opens an independent LazeCode process.

- Native Git clean/smudge pipeline on stage, partial commit, status and
  checkout: `.gitattributes` `text`/`eol`/`ident`/`filter`, `core.autocrlf`/
  `core.eol`, user `filter.<driver>.clean|smudge` commands, and LFS pointer
  clean/smudge backed by `.git/lfs/objects` (the `git-lfs` binary is never
  executed). Required-filter failures abort; optional-filter failures fall
  back to raw bytes.
- Transactional branch checkout: affected worktree paths and the index are
  snapshotted and restored if materialisation or the HEAD update fails,
  instead of leaving a half-switched tree.
- Pure-Rust unified-diff application for hunk stage/unstage/discard and patch
  apply/reverse/cached-apply; binary and rename patches are rejected
  explicitly. `git init` and discard now use `gix`/direct index writes.
- Terminal tabs now host up to four split panes (`Split Right`/`Split Down`),
  each with an independent PTY, active-pane focus, per-pane close, and
  row/column equal-size layout. Closing the last pane closes the tab; restart
  and console listings target the exact pane.
- Native `git reset --soft/--mixed/--hard` for branch and detached HEAD,
  including index/worktree rollback on failure; drop-HEAD and squash reuse
  it. Ignored-file status and pending-commit counts are now computed by
  `gix` (no `git status --ignored` / `git rev-list`), and LFS status/track
  are native.
- Shelf full/partial restore now runs through the native patch applier
  instead of spawning `git apply`.
- Shelf/copy-as-patch diffs are generated natively from HEAD vs worktree
  (tracked, untracked, deletions, mode changes and missing final newlines);
  the output is accepted by `git apply --check`. No `git diff` process is
  spawned for shelving.
- Ahead/behind and pending-commit counts are memoized by worktree + head +
  upstream object IDs (bounded cache), avoiding repeated ancestry walks on
  unchanged refs.
- Git clean/smudge/status/stage/stash/shelf reuse the already-open repository
  and one attributes stack per operation instead of rebuilding both per file.
- Stash and shelf save use a lightweight change scan (no per-file diff
  stats, ignored-directory walk or ahead/behind traversal), cutting worktree
  I/O for these actions.
- Stash apply now performs a text three-way merge with Git-style conflict
  markers and unmerged index stages when the worktree and the stash both
  changed the same file; clean merges auto-resolve. Large LFS files stream from `.git/lfs/objects` instead
  of being buffered.
- Ignored-file status is cached per worktree and invalidated by watcher
  events (with a 2s safety TTL), avoiding a directory walk on every snapshot.
- Ahead/behind/pending counts use gix's revision walk (commit-graph aware)
  instead of a hand-written parent traversal.
- Native remote push, fetch (including prune), clone and fast-forward-only
  pull. Push speaks receive-pack directly with a self-built zlib pack,
  report-status parsing, remote-tracking/upstream updates and
  force-with-lease semantics. Merge/rebase pull remains porcelain until the
  merge sequencer lands.
- Text three-way stash apply with conflict markers; large LFS objects stream
  from the native object store; ignored status is cached per worktree and
  invalidated by watcher events; ahead/behind and pending counts use the
  commit-graph-aware gix revision walk.
- Native merge (`gix::merge_commits`) with conflict stages/markers and
  merge commit creation; native clean rebase preserving author identity.
  Conflicted rebase, cherry-pick and revert still use the sequencer
  porcelain.
- Native Git stash: push (message, `-u`, `--keep-index`), list, files,
  apply, pop and drop. The implementation writes Git-compatible
  stash/index/untracked commits and `refs/stash` reflog entries, so stashes
  interoperate with the `git` CLI in both directions.
- Optional language-server integration for the stable 11-language set
  (`python/go/rust/c/cpp/java/ruby/php/kotlin/swift/lua`) with process
  lifecycle, JSON-RPC framing, document sync, diagnostics, hover,
  completion, definition, formatting, rename and document symbols. Missing
  servers or crashes degrade to syntax highlighting; no Node runtime is
  started. It is default-off (experimental) per product decision; the
  status bar shows per-file error/warning counts when enabled.
- An ignored end-to-end test against real `rust-analyzer`
  (`cargo test -p lazecode-core --test lsp_e2e -- --ignored`) plus a
  mock-server lifecycle test.

### Fixed

- Committing no longer fails when `.git/index` carries Git's optional
  resolve-undo (`REUC`) extension. `gix` cannot round-trip that extension, so
  it is dropped explicitly before writing; split-index, untracked-cache and
  FSMonitor extensions remain refused because dropping them changes index
  semantics.

- `rio-vt`'s `graphics` feature is now enabled, so Kitty PNG/JPEG payloads
  are decoded instead of rejected. Together with the explicit omp image
  environment, attached image chips can receive their texture. Unicode
  virtual placeholder runs are composited from rio-vt geometry, so omp's
  placeholder-based chips draw the actual thumbnail instead of an `img`
  text box.
- Terminal children now receive omp's explicit image-capability env
  (`PI_FORCE_IMAGE_PROTOCOL=kitty`, `PI_KITTY_PLACEHOLDERS=1`). Without it
  omp classifies LazeCode as a generic truecolor terminal and renders `img #N`
  as its green fallback grid instead of the actual thumbnail.
- Terminal parity: holding Backspace/arrows now repeats in legacy terminals
  (repeat was encoded as `KeyEvent::Repeat` and dropped; only releases must be
  dropped). Cmd+V in a focused console reads native NSPasteboard file/image
  flavors, materializes screenshots as temporary PNG/TIFF files, quotes paths
  with spaces/metacharacters, and writes the absolute path(s) through the
  current bracketed-paste mode so omp can render its own image chips.
- Console process state now comes from the same `ps -t <tty>` probe as the
  stable app, polled every 2 s in background batches. The project capsule and
  project avatars show a green busy ring or red outputting marquee, and the
  left strip, sidebar Consoles and status bar no longer count a shell that is
  merely sitting at its prompt as "running".
- Closing a pane/tab, Close Others/All, closing a project, or quitting now
  probes for live console commands and asks for confirmation before killing
  them; the modal text wraps long command lists.
- Terminal drag-selection now anchors on primary-button press instead of
  `Response::clicked` (release), which previously required a prior click
  before a drag could extend anything. `Cmd+C` is handled from egui-winit's
  `Event::Copy` (it never emits `Key::C` for the shortcut), and the macOS
  predefined Copy/Paste menu items were removed because their AppKit key
  equivalents swallowed `Cmd+C`/`Cmd+V` before egui saw them. Dragging the
  prompt now highlights it and `pbpaste` returns the selected text; pasting
  `echo PASTE_OK_123` into the shell executes normally.
- Image tabs now use a bounded, per-tab decoded-image cache instead of the
  unbounded `egui_extras` loader cache. Textures are downscaled to the visible
  viewport, retained pixels are capped at 64 MiB, and entries are dropped as
  soon as their tab closes. Closing a project also flushes Markdown image
  loaders and `CommonMarkCache`; animated GIFs keep using the frame loader and
  its cache is flushed when GIF tabs change. This removes the ~100 MB that
  previously survived after closing projects whose images had been viewed.
- Fixed UI fonts are memory-mapped instead of copied whole into the heap.
  The eight fallback files (Apple Color Emoji, Songti, Heiti, Hiragino,
  SF Pro, Menlo, Monaco, SF Mono) previously added ~335 MiB of dirty
  `Malloc Large`; they now stay file-backed. Startup footprint dropped from
  ~517 MiB to ~80 MiB in the measured build, with the same glyph coverage.
- Modal surfaces now install a full-screen hit-test blocker: clicks no
  longer reach the editor, tabs, tree, splitters or titlebar behind Settings,
  dialogs, menus, search, the command palette or the update window. Native
  menu accelerators are guarded too, and `⌘W` closes the modal instead of the
  hidden tab underneath.
- Pane borders are tiered: real window splits keep a clear `border`, while
  pane headers, toolbars, internal column splits and the activity rail use
  `border_soft`/`hairline`.
- Git log branch rows only select on click (double-click still checks out);
  they no longer set the log branch filter. Tags are refreshed with the
  branch job and appear in both the branch popup and Git log. Empty Tags
  sections are hidden, the branch popup drops “Checkout Tag or Revision”,
  cascading submenus open on click only, and long submenu labels are clipped
  inside the menu.
- Ignored files appear only in the Ignored Files group and the check mark is
  right-aligned; the commit view-options checks no longer overlap labels.
  Settings update notes wrap inside the panel.
- File-tree auto-locate scrolls virtualized rows by offset before rendering,
  so files outside the viewport are revealed. The Blame button is wide
  enough for its label and reports empty/failed blame instead of doing
  nothing.
- Shortcuts follow the macOS PyCharm keymap: `⌘⇧A` find action, `⌘1`
  Project, `⌘4` bottom panel, `⌘9` Git, `⌘K` commit, `⌘⇧K` push, `⌘T`
  update/pull, `⌘⇧T` new console, `⌘B` declaration, `⌥⌘O` symbols.
- Mixed CJK/Latin UI typography: Chinese mode now leads with PingFang SC so
  punctuation and mixed runs share one font's metrics, English falls back
  from SF Pro to PingFang SC instead of Hiragino, and SF Pro uses the 17pt
  Text optical size rather than the 28pt Display default.
- Commit-details “Expand All” no longer collapses every directory: the tree
  state is now a collapsed set (`empty = all expanded`), and “Collapse All”
  fills it. Both buttons operate on every nesting level.
- Worktree status and commit staging now compare canonical (filtered) bytes,
  so CRLF files, `$Id$` files and LFS pointers are not reported as modified
  just because the worktree representation differs from the blob.
- Partial commits no longer un-stage files outside the checked set; the
  commit tree is built from HEAD + checked paths and only those index
  entries are rewritten.
- Commit and partial-commit now run `pre-commit`, `prepare-commit-msg`,
  `commit-msg` and `post-commit`; hook failures abort and message rewrites
  are honoured. Branch switches run `post-checkout` with the old/new HEAD
  and flag `1` (its failure is logged only).
- History rewrite safety: `drop` refuses merge commits, commits outside
  HEAD's ancestry and commits already contained in a remote branch;
  `squash` only accepts the newest contiguous commit suffix; merge/rebase/
  reset validate option-like ref names; `merge --continue` and
  `cherry-pick --continue` keep git's own semantics instead of silently
  skipping.
- Conflict handling: unmerged index entries are reported as `C` rows and
  land in the Merge Conflicts subgroup (no longer untracked); taking the
  deleted side of a delete/modify conflict uses `git rm`; the merge editor
  refuses to mark a file resolved while conflict markers remain and
  surfaces backend errors.
- Git path/shelf containment: discard and conflict writes validate
  repository-relative paths and symlink ancestors; shelf names reject
  `..`, separators and control characters on delete/rename/import/read.
- Git reads: blame reuses per-commit author/time metadata, file history
  follows renames, submodule paths parse correctly, ahead/behind and
  pending counts use the configured upstream before falling back to
  `origin`, and changelist writes surface persistence errors and back up
  corrupt JSON.
- Terminal: no permanent 60 fps repaint after output; 256-colour indices
  render the xterm palette; Cmd+F focuses the find field and swallows
  shell input while open; selection highlights/copies its final cell,
  grapheme clusters and skips NUL sentinel cells; bracketed paste
  neutralises control bytes and cannot be terminated early; mouse press/
  release/drag is emitted and wheel coordinates follow the pointer;
  scrollbar direction and fractional trackpad deltas are fixed; output
  wake coalescing keeps the trailing frame; parser panics close the session
  instead of freezing it; shells are allow-listed and credential env vars
  are stripped; consoles are capped at 10 per worktree and the bottom dock
  height persists.
- Status bar shows the real encoding (UTF-16/Latin-1/…) instead of always
  “Latin-1”; the terminal search highlight clears when the bar closes.

### Changed

- UI redesign groundwork (branch `feat-ui-redesign`): the design-token layer
  now ports the stable app's stylesheet verbatim — the two isomorphic
  macOS Native themes (dark `variant-1`, light `variant-2`) replace the
  bespoke palette, the 11/12/13/14px type scale and the stable layout
  rhythm (26px log rows, 24px tree rows, 22px change rows, 30px tab bar,
  26px status bar, 28px buttons, 260px context menus) become shared
  constants, and the window body paints the stable background gradient so
  translucent panels read as Liquid Glass. Syntax colors follow the
  stable Monaco themes (jetbrains-dark / ide-light).
- Icons are now rasterized from the stable frontend's own SVG sources
  (`FileIcons.tsx`, extracted verbatim) via pure-Rust `resvg` into cached
  alpha textures tinted at draw time — icon geometry is identical to the
  web app. Icons without a stable counterpart keep hand-drawn vectors.

### Added

- The tracked-file context menu now carries the stable drawer's full item
  set, in the stable order: functional "Compare Before with Local"
  (HEAD ↔ worktree), "Create Patch from Local Changes..." (system save
  dialog, patch written by a background job), and "History Up to Here"
  (file-history dialog) join the menu; stable's greyed placeholders (Show
  Diff in a New Tab, Compare with Local, Open Repository Version,
  Cherry-Pick/Extract/Drop Selected Changes, Get from Revision, Show
  Changes to Parents) render disabled; separators match the stable layout;
  and "Move to <list>" is now disabled (not hidden) when every selected
  file is already in that list, for multi-selections too.
- Right-clicking a changelist group header now offers the whole group's
  file operations (commit / revert / move / shelve / add-to-VCS for
  Unversioned groups), matching the stable group menu — not just
  New/Delete Changelist.
- The editor tab context menu gains "Close to the left" plus stable's
  disabled-state rules (first/last tab) and separator layout, keeping the
  new path-copy entries.
- Escape now closes dialogs (stable behaviour). The conflict editor is
  exempt so hand-merged content can't be lost by a stray press.
- Unshelve… now defaults to creating a changelist named after the shelf
  (stable default), and deleting a shelf asks for confirmation first.
- The clean worktree card is no longer drawn before the first snapshot
  arrives ("Reading changes…" instead), and opening a worktree diff now
  also selects the file in the changes list (stable selectedFile sync).
- Diff windows now follow the stable single-window model: every diff entry
  point (compare-branches files, git log, commit drawer, sidebar Show Diff)
  reuses one diff tab and only swaps its content, instead of opening a tab
  per file. Tab titles use the stable scheme — `Diff (hash7)`,
  `Diff (from7 ↔ to7)`, `Working Tree Diff` — with the file name shown in
  the diff view's own header bar. Opening the exact same source again just
  re-activates the tab without reloading (no loading-spinner race with
  in-flight jobs).
- Right-click context menus no longer render a title line (the stable
  menus never had one): editor tabs, terminal tabs, project tree, branches,
  commits, changelist headers, and change files now start directly with
  their items.
- Untracked files can now be added to version control from the source-control
  context menu ("Add to VCS" / "Add N Files to VCS", matching the stable
  wording): the git add runs as a background job with the stable success /
  failure toast, and the snapshot refresh moves the files out of the
  Unversioned group. The untracked context menu now mirrors the stable one —
  Add to VCS, Jump to Source (single file), a separator, and a trash-semantics
  Delete… — instead of showing the tracked-file actions (Commit/Discard/
  Shelve/Move to) that do not apply.
- Untracked multi-file delete ("Delete N Files…") confirms once and moves all
  files to the system trash; failed names are reported in one toast.
- Branch comparison ported as a persistent central editor tab (stable
  CompareBranchesView page design, new-version visual tokens): upper/lower
  panes — one independent commit log per side — with a draggable 8px
  splitter (split persisted in settings.toml), a per-side banner ("Commits
  that exist in X but don't exist in Y") with the Swap button on the upper
  pane, and inside each pane a details column (shared draggable width,
  24-62%) showing single-commit files, parent..newest range diff for
  multi-select, or the whole-side merge-base list as a collapsible
  directory tree with numstat and status colors. Commit rows support
  click/Cmd/Shift selection, ⌘A/Esc/↑↓, double-click opens the whole-commit
  diff, and Load More (+500) honours truncation. Double-clicking a file
  opens the diff with the stable base rules (merge-base..side /
  parent..newest / the commit itself). The tab is single-instance; closing
  it ends the session.
- Per-side filters (independent for both sides): search with regex/case
  toggles, User menu (first-seen authors), Date menu (All/Today/7d/30d),
  Branch(ref) menu, and a Paths filter that re-queries with path limits.
- Comparison and detail file loading run as background jobs with proper
  loading / Retry / empty states (the old dialog ran `git compare` on the UI
  thread, retried every frame on failure and spammed toasts).
- Branch-tree toolbar: the dead "Compare with Working Tree" button is now
  "Compare Selected Branches" (needs exactly two selected branches), matching
  the stable GitBottomPanel toolbar.

### Fixed

- Interactive Rebase was a silent stub: the dialog's "Start Rebase" only
  showed a toast and changed nothing. It now executes a real
  `git rebase -i` with the UI-arranged todo (pick/reword/edit/squash/
  fixup/drop) via a sequence-editor shim, with conflicts surfacing in the
  panel's Continue/Abort banner. Covered by tests.
- Deleting a file from the project tree was a permanent unlink; it now moves
  the file (or directory) to the system trash like the stable `fs_delete`,
  closes editor tabs of deleted paths, and refreshes the VCS snapshot.
- Dialog lazy-loads refetched every frame while open with an empty list
  (Stash/Shelf managers, file history, Git tools — the latter spawning a git
  subprocess per frame on machines without submodules). Each dialog now
  loads once per open.
- Compare-branches audit fixes: branch multi-selection (`ref_selection`) is
  now click-ordered (was a `HashSet`, so the two sides of a comparison were
  assigned randomly); the compare session carries its project root and every
  background result is matched against it (same-named branches in two
  projects no longer cross-contaminate); ⌘A selects only the focused side
  (was both); arrow-key navigation clears the previous commit's stale file
  list; swap/path-filter reloads invalidate selections up front; and
  switching commits while a file-list load is in flight no longer sticks in
  "Loading…" forever.
- Compare-branches details were session-shared: selecting a commit in one
  pane leaked its file list into the other pane (visible as the empty side
  rendering the selected side's tree). The detail cache is now per side, and
  an unselected side always renders "No commit selected" with no files.
- Commit context menu "Compare with local" opened a worktree diff of the
  literal string `{hash}::{file}`; it now opens the commit↔worktree diff.
- Removed the never-constructed `CompareCommits` dialog state.

### Changed

- `compare_branches` now matches the stable data contract: three-dot
  (`other...side`) file lists merged from `--numstat` + `--name-status`
  (path, status letter, additions/deletions), limit+1 truncation detection,
  per-side path filters, and `merge-base` reported as `None` for unrelated
  histories (file lists degrade to empty instead of tip-vs-empty-tree).

### Added

- Source control changes list aligned with the stable version's commit drawer:
  changelist groups collapse (per group + Expand/Collapse All in the header),
  group headers carry a tri-state checkbox for bulk inclusion, and conflicted
  files surface in a "Merge Conflicts" subgroup inside the default changelist
  with a Resolve entry.
- File-row multi-selection (plain click selects, Cmd/Ctrl toggles, Shift picks
  a range in display order) plus a hover rollback button that asks for
  confirmation before discarding (batch discard supported; tabs of retracted
  new files close automatically).
- Context menus ported from the stable drawer: Show Diff, Jump to Source,
  Resolve (conflicts), Commit File(s)... with their own message, Revert
  Selected Changes, Copy as Patch to Clipboard, Move to Changelist, Shelve
  Changes..., Refresh VCS Changes and New Changelist.
- Stash button opens a dialog (message + "include untracked files"
  `git stash -u`) instead of pushing silently; the stash manager surfaces
  load errors instead of showing an empty list.
- Shelve scopes to the checked files (default name: the file name for a
  single file, the default changelist name for many) and an "Unshelve…"
  dialog restores a selected file subset into a chosen (or new) changelist.
- Commit message history (last 10, deduped, persisted in settings.toml) with
  a dock popover to refill or clear it, and a "Commit & Push" button that
  opens the push dialog after a successful commit.
- Non-Git projects show a "not a Git repository" notice with an Initialize
  Git Repository entry (the old dock button was unreachable).

### Changed

- The changes-list checkbox now means "include in this commit" with the
  stable version's default of nothing checked; the commit scope is exactly
  the checked set (staged on commit, everything else unstaged), and amend
  stays exempt from the check requirement.
- Changelist create/delete/move operations moved off the UI thread into the
  background job queue (delete asks Move-to-default vs Rollback first), and
  the changelist store resolves the real git-dir so linked worktrees persist
  their lists.
- Shelve retracts shelved paths via the per-file discard path, so staged-new
  files disappear from both worktree and index (previously a staged delta
  survived shelving).
- Tool buttons flow onto a second row instead of clipping on narrow
  sidebars / Chinese labels.

## [0.4.0] - 2026-11-04
