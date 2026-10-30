# Changelog

[English](CHANGELOG.md) | [Simplified Chinese](CHANGELOG.zh-CN.md)

All notable changes to LazeCode are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
