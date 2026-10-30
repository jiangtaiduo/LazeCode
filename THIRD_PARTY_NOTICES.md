# Third-party notices

[English](THIRD_PARTY_NOTICES.md) | [Simplified Chinese](THIRD_PARTY_NOTICES.zh-CN.md)

LazeCode itself is MIT-licensed (see `LICENSE`). This file summarizes the
third-party components that are compiled into or bundled with the application.
It covers direct dependencies and the full texts included in the macOS bundle;
it is not a complete transitive inventory. Each dependency ships its own
license metadata.

## Runtime — Rust

| Crate | License |
| :--- | :--- |
| `tauri`, `tauri-build`, `tauri-plugin-dialog`, `tauri-plugin-log`, `tauri-plugin-updater` | Apache-2.0 OR MIT |
| `serde`, `serde_json`, `log`, `dirs`, `png`, `regex`, `toml`, `libc` | MIT OR Apache-2.0 |
| `portable-pty`, `trash`, `objc2`, `objc2-foundation` | MIT |
| `objc2-app-kit` | Zlib OR Apache-2.0 OR MIT |
| `encoding_rs` | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| `chardetng` | Apache-2.0 OR MIT |
| `globset`, `ignore` | Unlicense OR MIT |
| `notify` | CC0-1.0 |
| `nucleo-matcher` | **MPL-2.0** |

### MPL-2.0 components

`nucleo-matcher` is used unmodified as a dependency. Its source is available
from <https://crates.io/crates/nucleo-matcher> and
<https://github.com/helix-editor/nucleo>. The MPL-2.0 license text is available
at <https://www.mozilla.org/MPL/2.0/>. No MPL-covered files are modified by
LazeCode.

## Runtime — JavaScript

| Package | License |
| :--- | :--- |
| `react`, `react-dom`, `react-markdown`, `remark-gfm`, `rehype-highlight` | MIT |
| `monaco-editor`, `@monaco-editor/react` | MIT |
| `@tauri-apps/api`, `@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-updater` | Apache-2.0 OR MIT |

`monaco-editor` bundles additional third-party components. Microsoft's
`ThirdPartyNotices.txt` is included in the app bundle as
`Monaco-ThirdPartyNotices.txt` and is also available in the `monaco-editor`
npm package.

## Ghostty

The native VT engine is built from
[Ghostty](https://github.com/ghostty-org/ghostty), MIT-licensed,
© 2024 Mitchell Hashimoto and Ghostty contributors. The full MIT text is in
`src-tauri/vendor/ghostty-vt/LICENSE` and is included in the app bundle as
`ghostty-vt-LICENSE`.

## Brand assets

File-type icon glyphs (including the Python, Docker, JavaScript, TypeScript,
HTML, CSS, Vue, Rust and Markdown marks) are used only to identify file types.
Those marks are trademarks of their respective owners; LazeCode is not
affiliated with or endorsed by them.

## Regenerating this file

This summary is maintained by hand for direct dependencies. Before a release,
regenerate a complete transitive inventory with a license tool
(for example `cargo about` and `license-checker`) and diff the result against
this file.
