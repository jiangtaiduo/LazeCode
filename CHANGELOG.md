# Changelog

[English](CHANGELOG.md) | [Simplified Chinese](CHANGELOG.zh-CN.md)

All notable changes to LazeCode are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
