# 第三方声明

[英文](NOTICE) | 简体中文

LazeCode 使用 MIT 许可证。它是原生 Rust 应用，不捆绑 JavaScript 运行时、
WebView 或 Node 包。主要运行时依赖包括：

| 项目 | 许可证 |
| :--- | :--- |
| [egui](https://github.com/emilk/egui)、[wgpu](https://github.com/gfx-rs/wgpu)、[winit](https://github.com/rust-windowing/winit) | MIT OR Apache-2.0（winit 为 Apache-2.0） |
| [gix](https://github.com/GitoxideLabs/gitoxide) | MIT OR Apache-2.0 |
| [alacritty_terminal](https://github.com/alacritty/alacritty) | Apache-2.0 |
| [portable-pty](https://github.com/wez/wezterm) | MIT |
| [syntect](https://github.com/trishume/syntect)、[ropey](https://github.com/cessen/ropey) | MIT |
| [notify](https://github.com/notify-rs/notify) | CC0-1.0 |
| [encoding_rs](https://github.com/hsivonen/encoding_rs) | (Apache-2.0 OR MIT) AND BSD-3-Clause |

完整直接依赖表与许可证说明见
[`THIRD_PARTY_NOTICES.zh-CN.md`](THIRD_PARTY_NOTICES.zh-CN.md)。每个依赖都会
附带自己的许可证元数据；发布前请使用 `cargo about` 生成完整的传递依赖清单。

## 法律说明

- LazeCode 的界面参考 PyCharm，代码为本项目自有实现；本项目**不包含** JetBrains
  的源代码、图标或其他二进制资源。
- PyCharm 和 JetBrains 是 JetBrains s.r.o. 的商标。本项目仅在描述设计参考时
  提及这些名称，不表示与 JetBrains 存在隶属或背书关系。
- LazeCode 不上传遥测；本地设置与工作区文件保存在
  `~/Library/Application Support/LazeCode/`。
