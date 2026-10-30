# 第三方声明

[英文](NOTICE) | 简体中文

LazeCode 使用 MIT 许可证。它依赖以下项目。

> 本文件是英文 `NOTICE` 的方便阅读译本；如与英文原文有冲突，以英文原文为准。

## 运行时依赖（随应用分发）

| 项目 | 许可证 |
| :--- | :--- |
| [React](https://github.com/facebook/react) | MIT |
| [React DOM](https://github.com/facebook/react) | MIT |
| [Monaco Editor](https://github.com/microsoft/monaco-editor) | MIT |
| [@monaco-editor/react](https://github.com/suren-atoyan/monaco-react) | MIT |
| [Tauri API](https://github.com/tauri-apps/tauri) | Apache-2.0 OR MIT |
| [@tauri-apps/plugin-dialog](https://github.com/tauri-apps/plugins-workspace) | MIT OR Apache-2.0 |
| [@tauri-apps/plugin-updater](https://github.com/tauri-apps/plugins-workspace) | MIT OR Apache-2.0 |
| [tauri-plugin-log](https://github.com/tauri-apps/plugins-workspace) | Apache-2.0 OR MIT |
| [react-markdown](https://github.com/remarkjs/react-markdown) | MIT |
| [remark-gfm](https://github.com/remarkjs/remark-gfm) | MIT |
| [rehype-highlight](https://github.com/rehypejs/rehype-highlight) | MIT |
| [Ghostty](https://github.com/ghostty-org/ghostty) | MIT |
| [nucleo-matcher](https://github.com/helix-editor/nucleo) | MPL-2.0 |
| [notify](https://github.com/notify-rs/notify) | CC0-1.0 |

Ghostty 的版权归 © 2024 Mitchell Hashimoto 及 Ghostty 贡献者所有。
`libghostty-vt` 静态库由未经修改的 Ghostty 源码构建；
`src-tauri/vendor/ghostty-vt/include/` 下的头文件逐字复制自该仓库。完整汇总以及
Monaco/React 的署名见 `THIRD_PARTY_NOTICES.zh-CN.md`。

## 构建工具（不随应用分发）

| 项目 | 许可证 |
| :--- | :--- |
| [Vite](https://github.com/vitejs/vite) | MIT |
| [TypeScript](https://github.com/microsoft/TypeScript) | Apache-2.0 |
| [Tauri CLI](https://github.com/tauri-apps/tauri) | Apache-2.0 OR MIT |
| [@vitejs/plugin-react](https://github.com/vitejs/vite) | MIT |

## 法律说明

- LazeCode 的界面参考 PyCharm，代码为本项目自有实现；本项目**不包含** JetBrains
  的源代码、图标或其他二进制资源。
- PyCharm 和 JetBrains 是 JetBrains s.r.o. 的商标。本项目仅在描述设计参考与兼容性
  时提及这些名称，不构成商标性使用，也不表示与 JetBrains 存在隶属或背书关系。
- 不上传遥测，也没有账号系统或崩溃上报服务。可以在设置中开启一个小型本地诊断
  缓冲区；它永远不会离开本机。唯一的自动网络请求是可选的更新检查（默认开启，
  每天最多一次，请求本仓库的 GitHub 发布页；可在设置中关闭）。你配置的 Git 远端
  和语言服务器可能自行发起网络请求。
