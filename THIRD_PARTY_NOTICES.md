# Third-party notices

[English](THIRD_PARTY_NOTICES.md) | [Simplified Chinese](THIRD_PARTY_NOTICES.zh-CN.md)

LazeCode itself is MIT-licensed (see `LICENSE`). This file summarizes the
direct third-party crates used by the native application. It is not a complete
transitive inventory; every dependency ships its own license metadata.

## Runtime — Rust

| Crate | License |
| :--- | :--- |
| `egui`, `egui-winit`, `egui-wgpu`, `wgpu`, `winit` | MIT OR Apache-2.0 (winit: Apache-2.0) |
| `gix`, `anyhow`, `thiserror`, `log`, `env_logger`, `serde`, `serde_json`, `toml`, `dirs`, `regex`, `libc` | MIT OR Apache-2.0 |
| `crossbeam-channel`, `parking_lot`, `unicode-width`, `png` | MIT OR Apache-2.0 |
| `bytemuck` | Zlib OR Apache-2.0 OR MIT |
| `pollster` | Apache-2.0/MIT |
| `rfd`, `ropey`, `syntect`, `portable-pty` | MIT |
| `alacritty_terminal`, `similar` | Apache-2.0 |
| `vte` | Apache-2.0 OR MIT |
| `muda` | Apache-2.0 OR MIT |
| `encoding_rs` | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| `chardetng` | Apache-2.0 OR MIT |
| `globset`, `ignore` | Unlicense OR MIT |
| `notify` | CC0-1.0 |

## Brand assets

File-type icon glyphs (including the Python, Docker, JavaScript, TypeScript,
HTML, CSS, Vue, Rust and Markdown marks) are used only to identify file types.
Those marks are trademarks of their respective owners; LazeCode is not
affiliated with or endorsed by them.

## Regenerating this file

This summary is maintained by hand for direct dependencies. Before a release,
regenerate a complete transitive inventory with a license tool
(for example `cargo about`) and diff the result against this file.
