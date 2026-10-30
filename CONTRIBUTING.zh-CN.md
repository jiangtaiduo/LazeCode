# 参与 LazeCode 开发

[英文](CONTRIBUTING.md) | 简体中文

感谢参与。LazeCode 是一个面向 Apple Silicon 上 macOS 27 的 Tauri 2 应用
（React 19 + Rust + 原生 Ghostty VT）。代码注释以中文为主；欢迎使用中文或英文
提交问题报告和拉取请求。

## 前置条件

- Apple Silicon 上的 macOS 27
- Xcode Command Line Tools
- Node.js 22.6 或更高（`nvm use` 会读取 `.nvmrc`）
- Rust 1.90 或更高（`rust-toolchain.toml` 固定 1.90.0）
- 仅当重新构建 `libghostty-vt.a` 时需要 Zig

## 环境准备

```bash
npm ci
npm run tauri dev
```

构建本地 ad-hoc 签名应用：

```bash
APPLE_SIGNING_IDENTITY=- npm run tauri build -- --bundles app
```

## 提交拉取请求之前

```bash
npm run check
npm run test:ui
npm run verify:fmt
./scripts/clippy.sh
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

以上五项必须全部通过。如果修改了线格式，请运行 `npm run verify:wire`，并在
`src/ghostty-vt/verifyWire.ts` 中补充或更新对应检查。

## 需要特别小心的地方

- **单元格帧 / 按键编码：** Rust 与 TypeScript 必须对每个偏移和位达成一致。
  `verifyWire.ts` 是可执行契约；只改一边、另一边不改，即使两边都能编译也是缺陷。
- **终端基准：** 只能用
  `UPDATE_GOLDENS=1 cargo test --test terminal_conformance` 重新生成，然后逐个
  人工检查改动过的基准文件。
- **界面多语言：** 新 key 要同时加到 `src/services/i18n/types.ts`、`en.ts` 和
  `zh-CN.ts`。Rust 菜单有一份小型镜像；`verify:menu` 和 `verify:i18n` 会检查
  两边。详见 `docs/i18n.zh-CN.md`。
- **Tauri 命令：** 新命令要同时加入 `src/services/tauriCommands.ts` 和
  `commandCatalog.ts`；`verify-tauri-commands` 和 `verify-command-catalog` 会在
  漂移时失败。
- **安全：** 不要新增接受任意路径、却不经过 `WorkspaceRegistry` 或
  `PathGrantRegistry` 的 IPC。新的子进程必须设置超时，并且不得通过 shell 执行。
- **密钥：** 绝不提交签名密钥、`.env` 文件、证书或真实用户数据。测试夹具必须
  是合成的。
- **文档：** 每个面向用户的文档都有英文和简体中文两个版本。请在同一个拉取请求中
  同时更新两版，并确保语言切换链接和内部链接指向对应语言版本。

## 代码风格

- Rust：运行 `cargo fmt`（仓库已格式化），并使用带 `-D warnings` 的 clippy。
- TypeScript/CSS：现有风格是两空格缩进和单引号；保持改动集中。目前尚未强制使用
  格式化器或静态检查器。
- 注释要陈述事实，并尽量靠近它所解释的不变量。

## 提交与拉取请求约定

- 每个拉取请求只做一件逻辑上的事。说明动机，并列出你运行过的命令。
- 界面改动：附上截图或简短录屏。
- 不要提交构建产物（`dist/`、`src-tauri/target/`）或本地截图。

## 发布流程

维护者：构建和发布概览见 `docs/architecture.zh-CN.md`。发布标签为 `vX.Y.Z`；
CI 在 `macos-15`（arm64）上构建。`TAURI_SIGNING_PRIVATE_KEY` 只存放在仓库密钥中，
只用于更新包产物。没有 Developer ID 证书时，CI 使用
`APPLE_SIGNING_IDENTITY=-` 做 ad-hoc 签名；应用无法公证。在真正完成
公证之前，不要把构建描述为已公证。
