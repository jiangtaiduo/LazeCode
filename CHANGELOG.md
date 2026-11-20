# Changelog

[English](CHANGELOG.md) | [Simplified Chinese](CHANGELOG.zh-CN.md)

All notable changes to LazeCode are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- 21 colour skins (20 palettes read from the design moodboards plus the
  original Studio colours), each with a dark and a light variant. A skin
  drives the whole surface stack, the three text tiers, the editor's 12
  syntax classes, diff/selection/conflict colours and the terminal ANSI-16.
  Palettes are authored as ten anchor colours per mode and derived in OKLCH
  with contrast-fitted text and tokens, so every combination clears the
  accessibility thresholds covered by the new unit tests.

- Settings → Appearance now has a **Color skin** picker (per-mode memory) and
  a live miniature preview of the chrome, copy, accent button and highlighted
  code. Dark and light skins are stored independently in `settings.toml`
  (`dark_skin` / `light_skin`); existing files default to Studio unchanged.

- Studio's light diff rows now use light insert/delete tints, so dark editor
  text no longer sits on the old dark Monaco backgrounds.

### Fixed

- Double-clicking the titlebar could abort the app with "messaging
  isKindOfClass: to nil" when the `AppleActionOnDoubleClick` global default
  is unset: the preference lookup result was nil-messaged before the null
  check. The check now happens first, so an unset key falls back to zoom.

- Resizing the window no longer throws a scrolled-back console to the top of
  its history. The scroll position is stored as a line count above the live
  screen, but a resize rewraps the history — widening merges soft-wrapped
  lines until the stored count exceeds what is left, and the snapshot clamp
  then pins the view to the oldest line. The session now hands the offset to
  the grid for the resize (rio-vt stabilizes the viewport around it) and
  re-anchors the viewport on the exact line the user was reading by content
  signature, so the line under your eyes stays put in both directions.

- A console first shown while its full-screen program (omp) is streaming
  output no longer stays black until the program exits. A fresh pane has no
  previous frame, but the renderer still held presentation while the program
  was inside a synchronized update (DEC 2026) — and omp under load spends
  virtually all of its time there — so nothing was ever painted. The hold now
  only applies when a frame already exists; a fresh pane snapshots
  immediately (one possibly-partial frame, replaced by the complete frame as
  soon as the update closes). This is the black-console case when switching
  between projects whose consoles are all running omp.

- Scrolling over an open popup no longer scrolls the panel underneath. egui
  delivers the wheel to every scroll surface containing the pointer and
  panels draw before overlays, so the commit list under the branch filter
  menu (and every other popover) consumed the scroll events aimed at the
  menu. The app shell now holds the wheel back from all panels while an
  overlay owns the pointer — modals (settings, search, command palette,
  dialogs) anywhere on the window, popover menus inside their rect — and
  hands it to the overlay pass, so filter menus with scrollable lists (log
  branch/author filters, agent picker) scroll themselves and everything
  else ignores the wheel. All popovers are covered: the menus drawn by the
  overlay pass, the diff-view mode dropdown, the compare path-filter form,
  and the editor completion list and symbol picker.

- A focused console or editor keeps the keyboard to itself. egui's Tab focus
  navigation used to park focus on a clickable widget, and egui treats
  Enter/Space on a focused widget as a click — so Tab-completing a shell
  command (or indenting code) and pressing Enter also activated the focused
  project-rail tile and switched projects mid-command. The text surface now
  clears and suppresses egui focus while it owns input; Tab and Enter reach
  the shell/editor untouched.

- Consoles no longer flash a half-composited frame when a full-screen TUI
  (omp) repaints, such as after switching back to its project. While the
  program is inside a synchronized update (DEC 2026), the console keeps
  presenting the last complete frame; a short settle window does the same
  while a TUI starts its resize repaint. The last complete snapshot is kept
  across project switches so the hold has a coherent frame to show.

- Removing a worktree releases all per-pane console state (including kitty
  image textures), and panes the core closes behind the UI's back — an
  emptied Single files project — no longer leave stale snapshots or a focus
  id pointing at a dead pane behind. The shell now reconciles the per-pane
  caches against the model once per frame instead of relying on every close
  path to clean up by hand.

## [0.4.11] - 2026-11-18

### Added

- In the Git log branch tree, the copy shortcut (Cmd+C / Ctrl+C) copies the
  selected branch names in full — `feature/a/b` and `origin/feature/a/b` keep
  every path segment — joining multi-selection line by line. A focused editor
  or console keeps its own copy behavior; clicking a branch row hands the
  keyboard to the tree.

- The Git drawer gains a Stash tab, shown only while the checkout has stash
  entries (PyCharm-style). It lists stashes with the branch each was created
  on, shows the selected stash's changes as a directory tree with per-folder
  file counts, and keeps `--include-untracked` snapshots under a separate
  "Untracked files on <branch>" root. Apply/Pop buttons sit in the footer.
  Every stash entry has its own disclosure chevron, so several stashes can be
  expanded side by side (per-stash details are cached; tree collapse keys are
  namespaced per stash). The drawer ⋮ menu is now the same menu as on the
  Commit and Shelf tabs. Right-clicking a stash entry offers Pop, Apply,
  Unstash…, Drop, Clear and Show Diff in the current or a new tab;
  right-clicking a file *or folder* offers Show Diff, Show Diff in a New Tab,
  Compare Before with Local, Edit Source, Open Repository Version, and
  Cherry-Pick Selected Changes (a native, path-filtered three-way apply of
  that file or folder subtree). The Unstash… dialog targets the current
  branch or a branch created on the spot, optionally pops the stash and
  reinstates the index recorded by the stash. Double-clicking a file still
  opens the stash diff, which now navigates across every file of the stash
  with F7/←/→. Repository-version tabs are read-only snapshots, so the
  checkout is never written back to.

### Changed

- Git log commit rows now show references the way PyCharm's compact view
  does: one label for the first reference group (the checked-out branch, or
  `remote & branch` when that branch tracks a remote at the commit) with a
  fanned stack of colored label icons — yellow HEAD, green local branches,
  violet remotes, gray tags. The label is capped at a third of the row, so
  commits pointing at many branches and tags no longer grow horizontally;
  hovering lists every reference on its own line, with full names, and
  tag-only commits keep their names in that tooltip. The checked-out branch
  is also ordered first among a commit's references.

### Fixed

- Console and editor keys are no longer muted after pressing Shift+Tab. The
  input gate used `egui_wants_keyboard_input()` ("any widget has focus"), so
  egui's Tab focus navigation handing focus to some button silently dropped
  every subsequent key (Ctrl+C, Shift+Tab, Option+Left, …). Only a real text
  editor (`text_edit_focused()`) takes keys away now, and Ctrl+P is left to
  focused consoles so TUI model cycling works.
- Modified arrows/Home/End/F1–F4 under the kitty keyboard protocol use the
  spec's `CSI 1;modifiers X` form instead of Private-Use `CSI u` codepoints,
  which kitty-aware TUIs (omp) ignore — Option+Left/Right word movement works
  again.
- Push and fetch now use the native `git` transport. The hand-written push
  built one pack from every object reachable from the tip, compressed each
  object on its own (no deltas) and sent it without negotiating what the
  remote already had — every push re-uploaded the whole history. Fetch (and
  clone) used gix's single-threaded pack indexing, much slower than git on
  large repositories. Clone, fetch-all and branch push all go through `git`
  again, keeping the local fast-forward guard and `--force-with-lease`.
- Stash file trees indent child folders one level under their root.

- Closing the last editor tab of a Single files workspace no longer crashes
  the app. The tab-close click happens mid-frame while an emptied ad-hoc
  checkout is dropped or another project takes over; the editor body now
  re-resolves the active project instead of using the stale index, falling
  back to another open project or the home screen.
- One physical Ctrl+C in a console no longer quits kitty-protocol programs
  (omp agents, modern TUIs). Under the kitty keyboard protocol's disambiguate
  flag the terminal must not report key releases; encoding them without an
  event-type marker made a release byte-identical to a press, so programs
  saw two Ctrl+C presses per keystroke and treated the second as quit.
  Releases are now only reported when the program explicitly requested event
  types, and ctrl/alt combos only switch to `CSI u` under disambiguate.

## [0.4.10] - 2026-11-17

### Added

- Native drag-and-drop accepts one or more files (opened together in the
  single-file workspace) or one or more folders (opened as projects, or
  focused when already open). Dropping files and folders together is rejected
  without opening anything.
- Syntax highlighting now uses the bat syntax collection (via `two-face`), so
  modern TypeScript/JSX, Vue, Svelte, TOML, YAML, SQL, Dockerfiles and other
  common formats are highlighted without any language server.
- JSON files can be reformatted with the built-in formatter or minified to one
  line. Key order is preserved, indentation follows the editor settings, and
  invalid JSON is rejected without changing the buffer.
- Markdown preview uses a larger reading scale, centered responsive width,
  constrained images, heading navigation, and a per-tab scroll position. Its
  scrollbar now uses the same geometry and palette as the source editor,
  including split view.
- Each project/worktree remembers its own editor/console/diff surface, drawer
  selection, and bottom-panel layout. Switching projects or worktrees restores
  that checkout's previous view, and the state is saved in `workspace.toml`;
  new checkouts start from the default layout.
- Project-rail initials follow word and camel-case boundaries: `LazeCode` →
  `LC`, `my_project` → `MP`, `lazecode`/`LAZECODE` → `LA`.

### Fixed

- Cmd/Ctrl-clicking an already selected Git Log commit now removes only that
  row and moves the active details to the nearest remaining selection. The
  deselected commit no longer stays highlighted or keeps the details panel.
- The commit panel now lists the net HEAD ↔ worktree difference (JetBrains
  semantics) instead of raw `git status`: a path whose staged and unstaged
  changes cancel out — index diverged from HEAD while the disk file matches
  HEAD — no longer appears in the list with an empty diff.
- The main checkout no longer registers a second recursive watcher, tree scan
  and Git snapshot that were never read, and the file tree now applies the
  configured ignore list immediately on open.
- Filesystem events under configured ignores, `target/` and `dist/` no longer
  trigger tree or Git refreshes. Content-only saves no longer rescan the whole
  tree, collapsed folders stay loaded, and manual tree refresh runs off the UI
  thread.
- Linked worktrees now have a cheap `.git` fingerprint that follows the
  `gitdir:` pointer and shared refs instead of falling back to the 30-second
  full Git refresh.
- A non-Git folder no longer shows a duplicate red "not a Git repository"
  refresh error above the initialize prompt, and long empty-state/error text is
  wrapped or clipped inside the commit drawer.
- Idle CPU no longer spikes above the stable build: the event loop clamps its
  wait deadline after all poll deadlines, advances the VCS poll schedule even
  without a project, ignores the self-referential RedrawRequested repaint, and
  only scrolls the active tab when it is clipped. This removes the 50k/s wait
  loop and the 60 fps tab-strip repaint loop.
- In the single-file workspace the Project, Commit and Git Log titlebar buttons
  stay visible but are disabled instead of collapsing the toolbar, and the
  unused Compare slot no longer leaves a gap before the terminal button.

## [0.4.9] - 2026-11-16

### Fixed

- Kitty image-cache trimming now removes exactly the excess unused bytes
  instead of reserving an extra image-sized slot, so a burst above the soft
  target cannot evict a visible placement through an eviction overshoot.

## [0.4.8] - 2026-11-16

### Fixed

- Kitty images in a console no longer blank the oldest visible pasted
  screenshots once several large images are on screen. Each console now has a
  configurable soft image-cache target (default 64 MiB); visible placements
  and images transmitted in the last two seconds are protected, and total
  image memory is still hard-capped at four times the soft target. Settings →
  Terminal → Image cache exposes 32/64/128/256 MiB presets.

## [0.4.7] - 2026-11-16

### Changed

- The updater now has a full update page (checking, up to date, available,
  downloading, installing, ready and error states), Markdown release notes,
  an ignore-this-version choice and a restart confirmation when editors have
  unsaved changes. The title bar shows an update pill for available,
  downloading and restart-ready builds. Automatic checks run five seconds
  after each launch and never install without an explicit click.
- Release manifests now carry changelog-derived release notes and an optional
  display date; tag releases verify the committed manifest against the
  artifact before packaging.
- Centralized frame pacing: foreground windows paint at most at 60 fps,
  background windows at 10 fps, and occluded windows stop painting entirely.
  Terminal stream output runs at 30 fps while typing keeps the 60 fps budget;
  the project activity ring and git-log paging are also capped at 30 fps.
  This removes compositor-rate full-window repaints from chatty terminals.
- Diff pages render only the visible row range and merge adjacent overview
  ruler marks. A large diff no longer paints every line or recomputes
  off-screen inline diffs on each repaint.

## [0.4.6] - 2026-11-15

### Fixed

- Terminal snapshot buffers no longer keep gigabyte capacity after an
  abnormal geometry: columns/rows are capped at 1024x512 and the cell buffer
  is rebuilt once its capacity exceeds four times the live cells. This
  removes the 1.4 GB physical footprint observed after long sessions (one
  76,158,400-cell buffer).
- The app bundle now displays and presents itself as **LazeCode**
  (`CFBundleDisplayName`) instead of "LazeCode Native".

### Added

- Tag releases ship a compressed DMG again
  (`LazeCode_<version>_aarch64.dmg`), matching the 0.2.0 releases, via the
  new `scripts/build-dmg.sh`.

## [0.4.5] - 2026-11-15

### Fixed

- Console activity recognises the idle shell by name rather than the pid
  recorded at spawn, and normalises parenthesised `ps` names such as
  `(bash)` (macOS 15), so an idle console is no longer reported busy.

## [0.4.4] - 2026-11-14

### Fixed

- Cloning and fast-forward pulling no longer require a configured git
  identity: repositories are opened with the documented
  `gitoxide.committer.*Fallback` values, so a fresh machine or CI runner
  without `user.name`/`user.email` can fetch refs like `git` does.

## [0.4.3] - 2026-11-14

### Fixed

- Release archives are created with macOS AppleDouble sidecars suppressed
  (`COPYFILE_DISABLE=1`), and bundle extraction ignores any `._*` or
  `.DS_Store` entries, so a tar built on macOS can no longer present a
  sidecar as an extra `.app` root and fail installation. The release
  workflow now rejects archives with extra roots or metadata.
- The filesystem tree regression test builds a synthetic repository instead
  of hardcoding the developer checkout, so CI passes at any clone path.

## [0.4.2] - 2026-11-14

### Added

- The updater installs real releases: manifests announce an artifact shape
  (`kind: "app-bundle"` ships a gzipped tar of the whole `.app`), and the
  installation swaps the entire bundle atomically — Info.plist, resources
  and signature stay consistent, with the previous build kept as a backup
  and restored on any failure. Extraction rejects absolute paths, `..`
  components and link entries, and enforces unpacked-size/entry caps. A
  "Restart now" button relaunches the installed build immediately. Updates
  work out of the box: the default endpoint points at the repository's
  signed `latest.json`, the release signing key's public half is pinned as
  the default, and startup checking is enabled by default. Publishing is
  scripted (`scripts/release-update.sh`: build → pack → hash → sign), and
  an integration test pins the openssl/ring Ed25519 cross-compatibility.

- The home "Open" action and titlebar "Open…" now use one native picker that
  accepts a folder or several files. Folders keep the normal project
  workspace; files open in a single virtual "Single files" checkout that has
  no explorer, Git snapshot, Git drawer, branch/worktree chips or status-bar
  branch. More
  files append to the same workspace, opening a folder switches to it, and
  the project menu keeps the Single files workspace one click away. Its
  consoles start in the user's home directory, and the workspace is restored
  across restarts. Opening a console from the empty home screen creates the
  same virtual workspace only as a console host, so HOME is never registered
  as a project and the titlebar keeps its empty "Open" state. Ad-hoc file tabs
  keep their normal file badge instead of showing the out-of-project diff
  marker, closing the last one returns to an open project (or the empty home
  when there is none), and the console toggle stays unlit until a console is
  actually opened.
- Home now shows recent projects and recent files in separate groups; recent
  files are recorded when opened from the home list, an ad-hoc file workspace,
  the explorer, search or the command palette.
- The titlebar project menu grows up to 80% of the screen before its list
  starts scrolling, rather than always capping at a fixed height. Row spacing
  is included in the layout measurement, so a scrollbar only appears when the
  menu genuinely reaches the 80% cap.
- Coding Agent entries are now configurable in Settings → Coding Agents:
  name, description and shell script per entry, plus add/remove/restore
  defaults. The default menu keeps Claude Code, OpenCode and Shell, adds
  `omp`, and removes Codex CLI and Aider. The agent menu is available from
  both the Consoles drawer header and the terminal dock header.
- A Consoles action in the titlebar opens the previously unreachable
  console drawer; without an open project it starts a console rooted at the
  user's home directory. Project, Commit and Git Log titlebar actions are
  disabled while no project is open.
- Shell layout: open projects now live in a wider vertical project rail with
  a fixed “open project” button and per-project console activity rings; the
  Project, Commit, Compare, Git Log and Terminal actions moved to the
  titlebar. The titlebar now uses the packaged application icon, its tool
  icons are larger, and the rail is 50px wide with compact project tiles.
  Worktree and branch selectors are borderless with larger type and even
  spacing. The active-project dropdown is a quiet switcher, and Settings
  sits at the far right after search.
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

- The filesystem tree regression test no longer hardcodes a developer
  checkout path: it builds a synthetic repository in a tempdir, so CI and
  any clone location exercise the same scan path.

- External Git operations are visible without reopening the project: watcher
  events under `.git` (index, HEAD, refs and linked-worktree metadata) now
  refresh the source-control snapshot even when no working-tree file changed.
  Open text buffers also poll their disk metadata and reload external edits
  (or warn when dirty), covering watcher coalescing and atomic replacements.
  The active checkout additionally runs a two-second VCS refresh fallback;
  when HEAD or the branch moves, the commit graph, log and branch inventory
  are refreshed as well.
- Agent console admission is now global (`terminal.max_agent_consoles`,
  default 20) and the per-worktree allowance is 12, so the initial login shell
  does not consume one of ten agent slots. Agent terminals use smaller
  scrollback, cold-spool and graphics-cache budgets than shells.
- Running-command detection no longer assumes the PTY child is always the
  shell. `zsh -lc omp`/`zsh -lc sleep` exec the command in place and are now
  reported busy before a console is closed. Busy polling uses one `ps`
  snapshot for all panes instead of spawning one `ps` per pane every two
  seconds.
- Switching worktrees now switches the branch chip, source-control changes,
  file tree, searches, diffs and Git commands to that checkout instead of
  reusing the main worktree's snapshot. Checked file selections are cleared
  on every switch so staging/commit cannot leak into another worktree.
  Removing a worktree no longer forces the Consoles drawer open; the current
  drawer stays on the fallback checkout.
- Titlebar double-click now uses a native event monitor that consumes the
  second click, runs the macOS double-click action (`AppleActionOnDoubleClick`),
  and never lets AppKit start a second zoom. The old path called `zoom:`
  without consuming the event, so AppKit could zoom again and produce the
  stretch-and-snap flicker. Double-clicks on titlebar buttons remain excluded.
- The empty Git Tools "Remotes" tab was removed; Remotes has its own dialog
  and the empty tab had no reachable entry.
- Git remote and branch workflows now match the stable behavior: Fetch All
  performs a native fetch and refreshes tracking refs; remote branches
  checkout as local tracking branches; remote branches delete without
  touching the local ref; unmerged local branches require an explicit force
  confirmation; tags checkout detached; and New Branch records the selected
  start point with an optional checkout. The Push dialog again exposes force
  (with lease), tag push, remote selection and pending commits; Remotes has
  edit/delete; tag rows have their own context menu; multi-select supports
  update, files diff and kind-aware deletion; and non-current branch Update
  fetches instead of pulling the current branch.
- Worktrees now have explicit lifecycle safety: the New worktree dialog can
  create a new branch (or pick an unused local one) because a branch can only
  be checked out in one worktree; occupied branches are disabled and rejected
  on submit; removing a worktree with staged, unstaged or untracked changes
  opens a warning with the change count and requires Force remove; a stale
  dialog re-checks the checkout and turns a dirty removal failure into the
  same confirmation instead of only a toast.
- Branch/tag data is now cached when a project opens, so the first branch
  menu includes the Tags section instead of waiting for a Git Log refresh.
- Project identity colors are assigned from a collision-free palette that
  excludes the red/green reserved for console activity rings and runners.
- The output marquee now follows the exact rounded outline as a single bright
  dash over a dim ring, with round caps and no square-corner chords.
- Find in Files now searches live with a 160 ms debounce, includes the project
  root in its result signature, and no longer renders a duplicate close hit
  target or top-aligned placeholder text.
- Editor and terminal hover now consistently request the text/I-beam cursor.
- The editor gutter has its own clip rect, restoring line numbers and Git
  blame text.
- Switching projects preserves cached console activity, and PTY resize/input
  echo no longer starts the marquee.
- The Compare Versions drawer can now be resized from its right edge and
  remembers the width. File rows reserve room for the numstat, ellipsize
  long names, show the parent path after the file name, and paint additions/
  deletions above the clipped text.
- The empty-workspace page doubles the packaged application icon (42 → 84
  px), keeps the action labels aligned, and renders shortcut keycaps at a
  readable size. The titlebar's empty “Open a project” selector is
  vertically centred.
- The project selector now opens one menu containing open projects and recent
  projects (open paths are filtered out of recents), with per-entry removal
  and a direct Open project action.
- Plain folders (no Git repository) show “no worktree” and “no branch” without
  dropdown chevrons or menus.
- Native titlebar zoom is suppressed across the custom 40px strip, so
  double-clicking any titlebar icon no longer resizes the window.
- Commit changelist rows put the parent path after the filename and clip both
  before the rollback control. The duplicate vertical “Git” caption in the
  Log toolbar is gone, and Compare Versions uses the same surface as Commit/
  Shelf.

- Project tree rows no longer inherit egui's item spacing, reclaiming the
  inter-row gap and fitting more files in the same drawer height.
- Titlebar drag/maximize is restricted to empty titlebar space; double-clicking
  a toolbar, project, worktree, branch, sequencer or search control no longer
  zooms the window.
- Project activity rings are 1px thicker inward and the outputting runner now
  repaints per frame with a fading tail instead of a fixed 16ms jump.

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
- A full audit pass over the November optimization batch fixed the issues it
  surfaced: the two-second VCS fallback now fingerprints `.git` metadata
  (index/HEAD/refs/sequencer files) and only runs a full snapshot when it
  moved, with a 30-second unconditional backstop; the status snapshot walks
  the HEAD tree once instead of twice; find-in-files revokes superseded
  scans through the cancel flag instead of piling up one uncancellable
  thread per keystroke; watcher tree reloads run as background jobs with
  the project's custom ignore patterns (and burst-coalesce), watcher
  `.git` detection is scoped to the top-level repository, events under
  default-ignored directories invalidate the ignored-file cache without
  triggering full refreshes; image tabs decode off-thread with a loading
  state, negatively cache failed decodes (no more per-frame retry) and keep
  exact byte accounting across viewport-growth reloads; terminal cold
  history resolves row ids by binary search (resize/reflow id gaps no
  longer shift rows out of place), oversized rows keep later ids aligned,
  spool compaction commits its new layout only on success, leftover spool
  files of dead processes are swept at startup; the pending kitty-image
  queue is bounded by the session graphics budget; the trailing-timer wake
  path cannot strand a final frame; busy detection treats only dash-form
  login shells (and the configured shell) as idle, so `exec fish` is
  reported busy; stash apply re-reads attributes after materialising a
  `.gitattributes` from the stash; checkout/reset/merge index rollbacks go
  through `index.lock` instead of blind overwrites; fetch carries a
  ten-minute watchdog; closing a console, closing a project and quitting
  fork their one-off busy probe off-thread instead of stalling the frame;
  and stale clipboard paste artifacts are cleaned up.

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
