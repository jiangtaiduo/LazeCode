# Contributing to LazeCode

[English](CONTRIBUTING.md) | [Simplified Chinese](CONTRIBUTING.zh-CN.md)

Thanks for helping. LazeCode is a Tauri 2 application for macOS 27 on Apple
Silicon (React 19 + Rust + native Ghostty VT). Code comments are mostly in
Chinese; issues and pull requests in English or Simplified Chinese are welcome.

## Prerequisites

- macOS 27 on Apple Silicon
- Xcode Command Line Tools
- Node.js 22.6 or newer (`nvm use` reads `.nvmrc`)
- Rust 1.90 or newer (`rust-toolchain.toml` pins 1.90.0)
- Zig only if you rebuild `libghostty-vt.a`

## Setup

```bash
npm ci
npm run tauri dev
```

Build a local ad-hoc signed app:

```bash
APPLE_SIGNING_IDENTITY=- npm run tauri build -- --bundles app
```

## Before opening a pull request

```bash
npm run check
npm run test:ui
npm run verify:fmt
./scripts/clippy.sh
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

All five must pass. If you changed the wire format, run `npm run verify:wire`
and add or update a check in `src/ghostty-vt/verifyWire.ts`.

## What needs extra care

- **Cell frame / key encoding:** Rust and TypeScript must agree on every offset
  and bit. `verifyWire.ts` is the executable contract; a change on one side
  without the other is a bug even if both compile.
- **Terminal goldens:** update them only with
  `UPDATE_GOLDENS=1 cargo test --test terminal_conformance`, then review every
  changed golden by hand.
- **i18n:** add new keys to `src/services/i18n/types.ts`, `en.ts` and
  `zh-CN.ts`. The Rust menu has a small mirror; `verify:menu` and `verify:i18n`
  guard the two sides. See `docs/i18n.md`.
- **Tauri commands:** add the command to `src/services/tauriCommands.ts` and
  `commandCatalog.ts`; `verify-tauri-commands` and `verify-command-catalog`
  fail on drift.
- **Security:** do not add IPC that accepts an arbitrary path without routing
  it through `WorkspaceRegistry` or `PathGrantRegistry`. New subprocesses need
  a timeout and must not use a shell.
- **Secrets:** never commit signing keys, `.env` files, certificates or real
  user data. Test fixtures must be synthetic.
- **Documentation:** each user-facing document has an English and a Simplified
  Chinese version. Update both in the same pull request and keep the language
  links and internal links pointing at the matching version.

## Code style

- Rust: `cargo fmt` (the repository is formatted) and clippy with `-D warnings`.
- TypeScript/CSS: the existing style is two-space indentation and single
  quotes; keep changes focused. A formatter/linter is not enforced yet.
- Keep comments factual and close to the invariant they explain.

## Commit and PR conventions

- One logical change per pull request. Include a short motivation and the
  commands you ran.
- UI changes: include a screenshot or a short recording.
- Do not include build output (`dist/`, `src-tauri/target/`) or local
  screenshots.

## Release process

Maintainers: see `docs/architecture.md` for the build and release overview.
Release tags are `vX.Y.Z`; CI builds on `macos-15` (arm64). The
`TAURI_SIGNING_PRIVATE_KEY` value is stored only in repository secrets and is
used only for updater artifacts. Without a Developer ID certificate CI signs the
app ad-hoc with `APPLE_SIGNING_IDENTITY=-`; it cannot be notarized. Do not
describe a build as notarized until it is.
