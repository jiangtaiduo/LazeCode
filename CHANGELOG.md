# Changelog

[English](CHANGELOG.md) | [Simplified Chinese](CHANGELOG.zh-CN.md)

All notable changes to LazeCode are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-11-02

### Added

- Console-task confirmation before quitting the app: if any project's console
  still has a task running (green ring or red outputting marquee on the
  project tab), both the window close button and Cmd+Q first show a confirm
  dialog listing those projects and their running commands; quitting (and
  terminating those consoles) proceeds only after "Force Quit", and cancelling
  keeps the app open. Before the dialog opens, the Rust-side "force-exit after
  10 s without a reply" flush failsafe is disarmed so the dialog may stay open
  indefinitely; concurrent close-button and Cmd+Q triggers share one dialog.

- Browser-style project tabs in the title bar: the leftmost LC logo button now
  hosts the project menu (formerly on the project-name pill), the branch
  selector follows, and every project opened in the window gets a tab for
  one-click switching. Tabs support drag-to-reorder (hover shows a close
  button, middle-click closes), and overflow collapses into a "≫" menu. The
  active project's tab is highlighted with a deep accent background plus a
  2px accent underline along its bottom edge, so the selection reads from the
  fill and the bottom line while the console state reads from the ring around
  the tab. Each
  tab's border mirrors that project's console state: gray when no process is
  running, a softened orange while a process runs (same hue as the running
  dot, dimmed so it cannot outweigh the selection), and a softened animated
  marquee while that process is actively
  producing output (within 2 s of the last output). Background projects are
  polled through a new batched `pty_sessions_busy` command that also reports
  per-session output freshness, so state for non-active projects needs no
  extra IPC round trips.
- Double-clicking a file in the changelist (diff) or in the project tree (open
  file) now auto-hides the console panel so the diff/file content takes over
  the main area. The hide is one-way; reopen the console from the tool strip.

### Fixed

- Switching to a project whose console was actively outputting could stop the
  marquee for one poll cycle (red → green → red). Activating a pane records a
  UI-event timestamp, and the 500 ms redraw grace classified the real output
  that kept arriving right after the switch as interactive repaint, so the
  next poll saw "busy but quiet". UI events now carry whether the session was
  outputting at the moment of recording (per-session result of the latest
  poll): a hot event no longer suppresses output within its window, while
  quiet consoles keep the one-shot repaint suppression. Keyboard echo stays
  unconditionally suppressed at 200 ms.
- Short white lines could appear after typing Chinese: the IME preedit was the
  only text drawn outside the per-row clip, so descenders (q/g/p/y/j) leaked
  1–2 px past the row bottom into the next row; during composition that row is
  never repainted (row-level diff says "unchanged"), and the instant the
  preedit is committed or deleted, the cursor row is erased and repainted while
  the leaked white pixels stay until that row's content changes. Preedit now
  shares the same row clip as regular glyphs. Also fixes a stale closure in the
  render loop: the rAF loop is created once when ready, so its tick kept
  calling the first-frame refresh even after a font-size change produced a new
  paint/refresh (cell size came from refs, font metrics from the stale
  closure); the tick now goes through a render-time mirror ref.
- Pasting multi-line text into the console triggered Enter per line: paste
  never checked the bracketed-paste (DECSET 2004) mode and normalized all
  newlines to `\r` before writing to the PTY, so TUIs that only recognize
  `ESC[200~…201~` paste markers (like omp) treated each `\r` as Enter. Paste
  (including Finder file-path attachments) now goes through a new
  `vt_encode_paste` command that lets the engine encode by the foreground
  program's 2004 mode: enabled — wrap in paste markers and keep newlines
  verbatim (omp inserts the whole text into its input box); disabled — legacy
  `\n`→`\r` behavior. The always-wrap attachment path is gone, so literal
  escape markers are no longer injected into programs without 2004.

- Switching between project consoles could flash the "outputting" marquee
  even though every console was idle. The echo filter only knew about
  keyboard/paste/IME writes; a PTY resize (SIGWINCH) or a forwarded mouse
  event makes the TUI repaint without recording an input time, so the repaint
  was classified as command output. Resize, mouse protocol events, key-release
  writes and pane activation now record a separate UI-event timestamp with a
  500 ms grace (keyboard echo stays at 200 ms), so those interactive repaints
  no longer light the marquee.
- The console canvas could reach the right window edge because the grid was
  derived from `clientWidth/clientHeight` including the host padding; the
  content-box fix restores the side inset, and the canvas host now uses a
  14 px horizontal inset for a clearer, symmetric right gap.
- The title bar could only be dragged from its outer padding/edges. The
  project tab strip's background was marked non-draggable, and the native
  `-webkit-app-region` drag competed with Tauri's drag-region handler. The
  strip background is now a deep drag region (tabs, close buttons and the
  overflow menu stay non-draggable) and dragging is handled solely by
  `data-tauri-drag-region`, so the whole title-bar background can move the
  window; double-click still maximizes.
- The console's last row (input line and cursor) was clipped against the status
  bar when the panel was maximized. The terminal grid was derived from
  `clientWidth/clientHeight`, which include the canvas host's padding, so
  the PTY received one more row and two more columns than the visible content
  box; the extra row was painted into the bottom padding and cut off. Grid
  sizing now subtracts the computed padding. Row painting is also clipped to
  its own cell row, so glyph ink that overhangs the row by a pixel can no
  longer linger on an unchanged neighbour as short white lines under text
  (which previously disappeared only after scrolling forced a repaint).
- A maximized console kept the height computed from `window.innerHeight` at
  the moment React last rendered. Maximizing or half-screening the app window
  does not trigger a re-render, so the open console stayed at its old size.
  The dock height is now a CSS `calc(100vh - titlebar - statusbar)` expression
  that the layout engine re-evaluates on every window resize.
- Bottom-panel visibility (console / changes / closed) was shared by every
  project open in a window: opening the console in project B and switching
  back to project A — which had it closed — popped the console open there too.
  Terminal sessions, per-project active terminal tabs, running-command probes
  and PTY budgets were already isolated per project; the panel toggle was the
  one piece of state left outside the project session snapshot. It now
  travels with the snapshot on both the project-switch path and the
  close-project-return-to-previous path (which also failed to restore the
  previous project's compare-versions / multi-commit diff sources, leaking
  the closed project's comparison targets into it). A project opened for the
  first time inherits whatever the panel currently shows until it has been
  switched away from once and holds its own memory.
- Switching between files in the diff viewer briefly flashed the previous
  file's page before the new diff appeared. The 0.1.3 loading veil missed both
  ends of the switch: it was raised one frame late (the loading flag is only
  set inside the data effect, so the first paint went out uncovered), and —
  the main offender — it lifted as soon as the patch data arrived, while the
  mounted Monaco diff editor still had the previous file's text in its models
  (@monaco-editor/react writes the new values asynchronously and the diff
  re-computation only finishes after that; this window is exactly the flash).
  The veil is now derived at render time from which request the on-screen data
  belongs to: it covers from the same frame the target changes and lifts only
  once the editor has confirmed the new text is in and its diff is computed
  (`onDidUpdateDiff`). Identical-content reloads are exempted via a content
  fingerprint (no model change, no event), and a failsafe timer keeps a lost
  event from pinning the veil shut.
- Project-tab close buttons are now always visible: they used to appear only
  on hover, so every tab changed width as the pointer moved across the bar.
  They sit dimmed by default and light up on hover instead.
- Switching projects no longer shoves the tab strip around. The branch pill
  was unmounted while branch info reloaded (the branch name is cleared on
  every switch and refetched), and its natural width varied with the branch
  name — the tabs to its right jumped left and back. The pill now has a
  fixed width with an ellipsized name and stays mounted while loading as a
  non-interactive skeleton, so the tab strip's start position is stable.
- The language-server chain was effectively dead after the first file in a
  workspace: provider registration and the initial `didOpen` lived in the
  editor's one-shot mount hook, while the editor only swaps models on file
  switch. From the second file on, hover / completion / definition / formatting
  silently did nothing (and if the first opened file had no server, the whole
  session never got one). Registration now follows file switches, and every
  opened file sends `didOpen` immediately so diagnostics arrive without
  requiring an edit first.
- Server-to-client LSP requests (`workspace/configuration`,
  `window/workDoneProgress/create`, sent by pylsp / rust-analyzer / tsserver)
  were consumed as if they were responses: a server request whose id collided
  with an in-flight client request resolved that request with `undefined`, and
  the server request itself was never answered (some servers stall). Dispatch
  now checks the method first and answers known server requests.
- Monaco models for paths containing `#`, `?` or `%` could never be disposed
  (the disposer looked the model up under a different URI form than the
  creator used), leaking the document and its undo stack; LSP URIs for such
  paths were also inconsistent between `didOpen` and provider calls.
- While a git refresh was in flight, a new request upgraded the follow-up run
  to the full six-command sweep; under directory-event bursts this re-ran
  log/branches/tags repeatedly. The follow-up now keeps the light shape unless
  a full refresh was actually requested.
- Project-tab console indicators treated keystroke echo and TUI redraws as
  console output, so typing into the terminal started the activity runner. A
  short per-session echo window now filters PTY output that follows user input;
  running-but-idle tabs use a macOS traffic-light green ring, active output
  uses a red runner that travels the border at a uniform pace, and the active
  tab is an accent-filled raised chip instead of an accent underline.

- Files created externally (build tools, agent CLIs, Finder) inside a
  subdirectory of an open project no longer require a manual reload to appear
  in the file tree. Two causes: the directory watcher was non-recursive, and
  notify's FSEvents backend drops events deeper than a direct child of the
  watched root, so `project/src/new.ts` never produced an event at all; and
  tree refreshes replaced expanded deeper subtrees with a depth-2 re-read.
  Directory watching is now recursive with per-directory coalescing (250 ms,
  one event per changed directory, root fallback under load), events report
  the changed directory so only that subtree is re-read and merged, expanded
  deeper subtrees survive refreshes, and expanding a folder always re-reads it.
  Events under `.git` are filtered out: the app's own git refresh writes
  `.git/index` (opportunistic update) on every run, so without the filter the
  recursive watcher fed itself — refresh, write, event, refresh — showing as
  the changelist spinner spinning forever after opening a project. The old
  non-recursive watcher happened to drop all `.git` events and masked this.

### Performance

- LSP document changes are debounced per file (200 ms): full-text sync turned
  every keystroke into a whole-document stringify + IPC + server-side reparse;
  closing a file flushes the pending change first so formatting never sees a
  stale buffer.
- The status-bar cursor indicator now subscribes in its own leaf component:
  the subscription previously sat at the top of the App component, so every
  arrow key re-ran the entire 6,500-line App body — the exact cost the
  cursor store's design comment promised to avoid.
- CommitDrawer/MergeToolDialog per-keystroke re-renders and the
  per-pointermove modal dragging hook are known remaining items, deferred
  (they need prop-stability audits / a drag-skeleton refactor; interactions
  are user-paced and not on the keystroke path).

- `git log` no longer re-runs `git rev-list HEAD` (full-history output, tens of
  thousands of lines on monorepos) on every refresh and every log pagination:
  the ancestor set is cached by repository + HEAD sha and recomputed only when
  HEAD moves.
- Editing no longer triggers the full six-command git refresh 2.5 s after each
  typing pause. Editing can only change the working-tree status, so the
  debounced refresh now runs `git status` alone (without
  `--ignored=matching`, the most expensive walk shape); log, branches and tags
  refresh on focus, after git operations, and on explicit refresh. The Ignored
  group carries over between light refreshes.
- Terminal event channels (`pty://dirty|pwd|exit`, file-watch, menu events) are
  now delivered with `emit_to` to the owning window instead of being broadcast
  to every window; under heavy terminal output with multiple windows this
  stops flooding unrelated WebViews.
- Large terminal pastes now travel as a raw byte body (`application/
  octet-stream`) with the session id in a header, instead of a JSON number
  array (~4x size inflation and per-element parsing on the Rust side),
  mirroring the snapshot path.
- Window geometry persistence no longer writes a file on every Moved/Resized
  event (~60 Hz during a drag, on the main thread): updates are coalesced in
  memory and flushed 300 ms after the last event; window destruction still
  saves immediately.
- Acquiring the user's real PATH (login-shell probe, 200-800 ms with nvm/rbenv)
  no longer blocks the first frame: it runs on a background thread at startup,
  and the first git / PTY / LSP spawn waits for it to finish.
- Inline arrow props defeating `React.memo` on TitleBar, GitBottomPanel and
  CommitDrawer are now stable callbacks, and the left tool strip is memoized —
  every keystroke previously re-rendered all of them on top of the App body.
- The renderer loop reads layout once per frame instead of twice; the six
  terminal commands that still held the global session-table lock while
  working on a terminal (`vt_encode_key`, selection family, `vt_selected_text`)
  now follow the lookup-then-release convention; `TERM_PROGRAM_VERSION` reports
  the real version instead of a hardcoded 0.1.0.

- Pasting an image from the clipboard (screenshot / browser "Copy Image") into
  the console previously did nothing: such clipboards carry image data but no
  file-path flavor. The Rust side now writes the image data to a temporary file
  and returns its path; the path goes to the terminal as a bracketed paste and
  omp renders its own thumbnail.
- With a task producing output in the console, switching to another app for a
  while and coming back left the whole window black for seconds: while occluded,
  terminal dirty-signal events piled up in the WebView evaluation queue and were
  replayed all at once, saturating the main thread. Dirty signals are now
  unsubscribed while the page is hidden and resubscribed with a catch-up frame
  on return.
- The project dropdown grew with the open-projects section and only scrolled
  internally after crossing the window's bottom edge. The popup height is now
  clamped to the viewport, and open + recent projects scroll together in a
  single region while the search box and action buttons stay pinned (matching
  the branch dropdown).

- The project file tree is now windowed: only the rows around the viewport are
  mounted, so expanding a directory with tens of thousands of entries no longer
  creates a DOM row (and an inline SVG icon) per file. Expand/collapse,
  multi-select, context menus and locate-in-tree keep their existing behavior;
  locate now scrolls by flattened index instead of `querySelector`.
- Hidden terminal canvases are released after the page has been hidden for five
  minutes. Active panes and foreground tab switches are never touched — the
  first frame after switching back from another app is still the last painted
  screen — while background tabs stop holding their Retina backing stores. A
  released pane takes one full snapshot + repaint when it is next opened.

## [0.1.3] - 2026-10-31

### Added

- While commands are still running in the console, the Terminal icon in the left
  tool strip now shows an orange dot in its top-right corner; hovering lists the
  running command names. Detection keeps running while the panel is collapsed
  (probed every 2 seconds).

### Fixed

- When the console is docked at the bottom, the terminal's own status bar and
  the global status bar stacked into two rows and squeezed the terminal view.
  The dedicated status bar is removed; the active session's cwd now merges into
  the left side of the global status bar (shown only while the console is
  open), returning the reclaimed height to the terminal.
- Double-clicking a file in the changes list to switch diffs briefly flashed
  the previous file at its last scroll position. A loading veil now covers the
  content area during loads (the diff editor stays mounted, no rebuild), and
  the new file lands directly on its first difference block.

### Changed

- When a background update check finds a new release, the title bar now shows a
  green `vX.Y.Z available` badge next to Search; clicking it opens the existing
  update dialog. The label is localized in English and Simplified Chinese.

### Changed

- Renamed the project to LazeCode. The bundle identifier is now
  `io.github.jiangtaiduo.lazecode`, project settings use
  `.lazecode/settings.json`, and local storage keys use the `lazecode.` prefix.
  Existing settings are not migrated.
- Documentation is now maintained in English and Simplified Chinese, with
  matching language links in every user-facing document.
- The README now includes an at-a-glance status table, DMG installation
  guidance, and a troubleshooting section. The app icon and favicons use the
  LazeCode wordmark.
- CI and release workflows now use `macos-15`; GitHub is retiring the `macos-14`
  runner image on 2026-11-02.
- Local and CI release builds use `APPLE_SIGNING_IDENTITY=-` when no Developer ID
  is configured, so the macOS bundle has a complete ad-hoc signature instead of
  only a linker signature.
- Automatic update checks are enabled by default (at most once a day). Installing
  an update still requires explicit user confirmation.

## [0.1.2] - 2026-10-30

### Changed

- The declared minimum macOS version is now 13.0 instead of 27.0. Tauri writes
  this value into both `LSMinimumSystemVersion` and the Mach-O deployment
  target, so macOS 13 through 26 can launch the app; only macOS 27 has been
  tested so far.

## [0.1.1] - 2026-10-30

### Added

- The welcome screen now shows the running version under the LazeCode wordmark,
  so an applied update is visible without opening Settings.

### Changed

- Release artifacts are signed with a new updater key pair; in-app updates are
  verified against the public key shipped in this build.

## [0.1.0] - 2026-10-29

### Added

- Native Ghostty VT terminal with incremental cell-frame diffs, Kitty graphics,
  multi-tab/split panes, scrollback search and an accessible text mirror.
- Git IDE: DAG log, branches/tags/remotes/worktrees/submodules/LFS, commit,
  push/pull/fetch, merge/rebase/cherry-pick/revert, interactive rebase, stash,
  shelves, hunk staging, diff viewer and batch drop-commit.
- Monaco editor with per-file encoding, atomic writes, file watching, Find in
  Files, Search Everywhere and LSP diagnostics.
- Local-first settings, window-state persistence, English/Simplified Chinese
  UI and native localized menus.
- Cross-language wire verification (34 checks), terminal conformance goldens,
  378 Rust tests and 98 UI tests.
