# Security policy

[English](SECURITY.md) | [Simplified Chinese](SECURITY.zh-CN.md)

## Supported versions

LazeCode is pre-1.0. Security fixes land on the `main` branch and in the latest
0.1.x release. Older builds are not supported.

## Reporting a vulnerability

Please use GitHub's **private vulnerability reporting** on this repository:
<https://github.com/jiangtaiduo/LazeCode/security/advisories/new>. Do not open a
public issue for a suspected vulnerability.

Include the affected version, macOS version and chip, a minimal reproduction,
and the impact you believe is possible. We aim to acknowledge reports within
7 days and to publish a fix or mitigation as soon as practical.

## Threat model

- **Untrusted:** repository contents (files, Markdown, Git metadata, branch
  names, commit messages, escape sequences emitted by programs you run).
- **Trusted:** the webview/renderer. The terminal and user-configured language
  servers can execute arbitrary programs by design, so a fully compromised
  renderer is outside the isolation boundary. The IPC checks below are
  defense-in-depth against malicious repository content, not a sandbox for
  arbitrary JavaScript.
- **Out of scope:** vulnerabilities that require the user to run an untrusted
  binary, grant Full Disk Access, or disable macOS protections.

## Hardening in place

- File reads, writes, deletes and pastes are restricted to registered workspace
  roots (`WorkspaceRegistry`); out-of-workspace native-picker choices use a
  session-scoped `PathGrantRegistry`.
- Git subprocess arguments reject option-like values, use `--end-of-options`
  where applicable, validate destructive paths and rebase todo lines, and run
  under a timeout.
- Git `-C <repo>` targets are checked against registered roots. After startup
  an empty registry is fail-closed.
- Clone destinations, worktree paths and shelf-patch imports require a
  workspace or a native-picker grant.
- Terminal OSC 52 clipboard writes are disabled. OSC 8 links go through an
  `http`/`https` allowlist. Kitty image storage is capped at 64 MB.
- Top-level navigation uses exact scheme/host matching; remote pages are opened
  in the system browser, never loaded in the app webview.
- The updater verifies a minisign/Ed25519 signature before installing anything.
  It checks at most once a day and is enabled by default; disable it in
  `Settings → Updates`.

## Known limitations

- **No sandbox:** LazeCode is a directly distributed, non-sandboxed macOS app.
  It can access any file you can; the workspace allow-list narrows the IPC
  surface, not the process.
- **Ad-hoc signed builds:** there is no Developer ID certificate yet. Release
  apps are ad-hoc signed but not notarized. Gatekeeper may block downloads; build
  from source if you need a trusted local binary.
- **Language servers:** the command is trusted user configuration. Only point
  LazeCode at binaries you trust.
- **Logs:** release builds write `panic.log` under
  `~/Library/Logs/io.github.jiangtaiduo.lazecode` on a crash; debug builds also
  write desktop logs. They may contain file paths and Git error output. Review
  them before sharing.
- **Remote Markdown images:** they are click-to-load, but once loaded the
  remote host sees your IP address.

## Disclosure

We follow coordinated disclosure. We will credit reporters unless they ask to
remain anonymous, and we will publish an advisory once a fixed release is
available.
