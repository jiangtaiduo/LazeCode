# 第三方声明

[英文](THIRD_PARTY_NOTICES.md) | 简体中文

LazeCode 本身使用 MIT 许可证（见 `LICENSE`）。本文件汇总编译进应用或随应用一起
分发的第三方组件。它覆盖直接依赖以及 macOS 应用包中包含的完整文本，不是完整的
传递依赖清单；每个依赖都会附带自己的许可证元数据。

## 运行时：Rust

| Rust 包 | 许可证 |
| :--- | :--- |
| `tauri`、`tauri-build`、`tauri-plugin-dialog`、`tauri-plugin-log`、`tauri-plugin-updater` | Apache-2.0 OR MIT |
| `serde`、`serde_json`、`log`、`dirs`、`png`、`regex`、`toml`、`libc` | MIT OR Apache-2.0 |
| `portable-pty`、`trash`、`objc2`、`objc2-foundation` | MIT |
| `objc2-app-kit` | Zlib OR Apache-2.0 OR MIT |
| `encoding_rs` | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| `chardetng` | Apache-2.0 OR MIT |
| `globset`、`ignore` | Unlicense OR MIT |
| `notify` | CC0-1.0 |
| `nucleo-matcher` | **MPL-2.0** |

### MPL-2.0 组件

`nucleo-matcher` 以未修改的依赖形式使用。源码可从
<https://crates.io/crates/nucleo-matcher> 和
<https://github.com/helix-editor/nucleo> 获取。MPL-2.0 许可证文本见
<https://www.mozilla.org/MPL/2.0/>。LazeCode 没有修改任何受 MPL 覆盖的文件。

## 运行时：JavaScript

| 包 | 许可证 |
| :--- | :--- |
| `react`、`react-dom`、`react-markdown`、`remark-gfm`、`rehype-highlight` | MIT |
| `monaco-editor`、`@monaco-editor/react` | MIT |
| `@tauri-apps/api`、`@tauri-apps/plugin-dialog`、`@tauri-apps/plugin-updater` | Apache-2.0 OR MIT |

`monaco-editor` 还包含其他第三方组件。Microsoft 的 `ThirdPartyNotices.txt` 以
`Monaco-ThirdPartyNotices.txt` 为名包含在应用包中，也可以在 `monaco-editor`
npm 包中找到。

## Ghostty

原生 VT 引擎构建自 [Ghostty](https://github.com/ghostty-org/ghostty)，使用 MIT
许可证，© 2024 Mitchell Hashimoto 及 Ghostty 贡献者。完整 MIT 文本位于
`src-tauri/vendor/ghostty-vt/LICENSE`，并以 `ghostty-vt-LICENSE` 为名包含在
应用包中。

## 品牌资源

文件类型图标（包括 Python、Docker、JavaScript、TypeScript、HTML、CSS、Vue、Rust
和 Markdown 标志）仅用于标识文件类型。这些标志属于各自所有者；LazeCode 与它们
没有隶属关系，也不受其背书。

## 重新生成本文件

本摘要由人工维护，覆盖直接依赖。发布前请使用许可证工具（例如 `cargo about`
和 `license-checker`）生成完整的传递依赖清单，并与本文件比对。
