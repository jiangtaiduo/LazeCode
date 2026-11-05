# 第三方声明

[英文](THIRD_PARTY_NOTICES.md) | 简体中文

LazeCode 本身使用 MIT 许可证（见 `LICENSE`）。本文件汇总原生应用直接使用的
第三方 crate，不是完整的传递依赖清单；每个依赖都会附带自己的许可证元数据。

## 运行时：Rust

| Rust 包 | 许可证 |
| :--- | :--- |
| `egui`、`egui-winit`、`egui-wgpu`、`wgpu`、`winit` | MIT OR Apache-2.0（winit 为 Apache-2.0） |
| `gix`、`anyhow`、`thiserror`、`log`、`env_logger`、`serde`、`serde_json`、`toml`、`dirs`、`regex`、`libc` | MIT OR Apache-2.0 |
| `crossbeam-channel`、`parking_lot`、`unicode-width`、`png` | MIT OR Apache-2.0 |
| `bytemuck` | Zlib OR Apache-2.0 OR MIT |
| `pollster` | Apache-2.0/MIT |
| `rfd`、`ropey`、`syntect`、`portable-pty` | MIT |
| `alacritty_terminal`、`similar` | Apache-2.0 |
| `vte` | Apache-2.0 OR MIT |
| `muda` | Apache-2.0 OR MIT |
| `encoding_rs` | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| `chardetng` | Apache-2.0 OR MIT |
| `globset`、`ignore` | Unlicense OR MIT |
| `notify` | CC0-1.0 |

## 品牌资源

文件类型图标（包括 Python、Docker、JavaScript、TypeScript、HTML、CSS、Vue、Rust
和 Markdown 标志）仅用于标识文件类型。这些标志属于各自所有者；LazeCode 与它们
没有隶属关系，也不受其背书。

## 重新生成本文件

本摘要由人工维护，覆盖直接依赖。发布前请使用许可证工具（例如 `cargo about`）
生成完整的传递依赖清单，并与本文件比对。
