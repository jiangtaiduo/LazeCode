# Architecture

[English](architecture.md) | [Simplified Chinese](architecture.zh-CN.md)

LazeCode is a single native process (Rust/Tauri) with one or more WebView
windows. The frontend is React 19 + Monaco; the backend owns Git subprocesses,
PTYs, the VT engine, file I/O and window state.

```
┌──────────────────────────── macOS app ────────────────────────────┐
│  WebView (React)                    Rust (Tauri)                  │
│  ───────────────                    ────────────                  │
│  App.tsx  ── invoke ──────────────▶ commands.rs ── git ──▶ git     │
│  components/  ◀── events ────────── pty.rs ── PTY ──▶ shell        │
│  ghostty-vt/  ◀── cell frames ───── vt.rs ── libghostty-vt.a       │
│  services/                          window_state.rs               │
└───────────────────────────────────────────────────────────────────┘
```

## Terminal path

1. `pty.rs` opens a PTY with `portable-pty`, spawns the user's login shell from
   an allow-list, strips credential environment variables and starts a reader
   thread.
2. The reader writes PTY bytes into `vt.rs` (`NativeTerminal`), which calls the
   native `libghostty-vt` parser in the same process.
3. `vt.rs` encodes either a full snapshot or an incremental diff frame as a
   compact binary buffer (`src/ghostty-vt/cellFrame.ts` decodes it).
4. `CellDiffCache` applies the frame; `GhosttyTerminal.tsx` paints changed rows
   to a canvas and mirrors text into an accessible `role="log"` element.
5. Keystrokes are encoded in the frontend (`domKeys.ts`) and sent through one
   `pty_write` IPC call. Mouse events use the engine's mouse encoder.

The wire format is a cross-language contract. `src/ghostty-vt/verifyWire.ts`
parses the Rust-side declarations/fixtures and checks 34 invariants; keep it in
sync with any layout change.

## Git path

`commands.rs` runs `git` as a subprocess. Every call:

- passes through `reject_option_like` / `--end-of-options` where a value could
  be mistaken for an option;
- validates destructive repository-relative paths;
- runs under `run_with_timeout` (no pipe-buffer deadlock);
- is checked by `authorize_git_repo` when it uses `git -C <dir>`. After app
  startup an empty workspace registry is fail-closed.

The frontend never parses Git's human output where a stable format exists; the
DAG layout, file status and diff streams are computed in Rust and sent as JSON.

## File I/O path

`WorkspaceRegistry` holds the canonical roots the user opened.
`authorize_workspace_path` canonicalizes the target (resolving symlinks) and
requires it to be inside a root; `need_write` also rejects read-only roots.
`PathGrantRegistry` holds session-scoped grants minted by the native
NSOpenPanel picker, used when the user intentionally targets a file outside the
workspace (diff targets, clone/worktree destinations, shelf patches). Grants
are cleared when the project closes.

## Window and app lifecycle

`window_state.rs` persists geometry per window under the app config directory.
The bundle identifier is `io.github.jiangtaiduo.lazecode`, so window state and
the native menu language live under
`~/Library/Application Support/io.github.jiangtaiduo.lazecode/`.
`lib.rs` restores the main window before it is shown, handles `RunEvent::Reopen`
and the pre-exit flush, and installs a panic hook. A navigation-guard plugin
checks every top-level navigation with exact scheme/host matching.

## Frontend structure

- `App.tsx` owns workspace/project switching and Git orchestration.
- `components/` is presentational + local interaction; dialogs are lazy-loaded.
- `services/` contains the IPC bridge, settings layers, i18n, terminal source,
  file watch, Monaco setup, updater and diagnostics.
- `ghostty-vt/` is the binary decoder, generated key table and wire verifier.

## Build and release

`build.rs` resolves `libghostty-vt.a` from `GHOSTTY_VT_LIB_DIR` or the vendored
macOS arm64 copy. The vendored library is rebuilt only by
`scripts/build-vt-lib.sh` with an explicit `GHOSTTY_REF`; it records the commit,
Zig version and SHA-256 in `src-tauri/vendor/ghostty-vt/`.

`src-tauri/tauri.conf.json` is the contributor build (no updater artifacts).
`src-tauri/tauri.release.conf.json` enables updater artifacts; CI supplies
`TAURI_SIGNING_PRIVATE_KEY` from secrets. CI runs on `macos-15` (arm64).
Without a Developer ID certificate CI ad-hoc signs the bundle with
`APPLE_SIGNING_IDENTITY=-`; it is not notarized. See `SECURITY.md`.
