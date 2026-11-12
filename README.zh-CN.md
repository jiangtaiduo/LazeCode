# LazeCode

一个快速、原生的 macOS 编辑器、Git 客户端和终端 —— 完全使用 Rust 编写。

LazeCode 面向长时间在终端里工作的开发者：一个窗口管理多个项目，内置真正的
编辑器、源码状态与提交历史，以及绑定到项目的控制台会话。没有 WebView，没有
内嵌 JavaScript，也没有随包分发的运行时。

> 旧的 Tauri/React/Monaco 实现已在原生版本成为唯一构建目标后从工作树移除，
> 仍可从 Git 历史中找回。精确的能力矩阵与已知差距见
> [`docs/implementation-status.md`](docs/implementation-status.md)。

## 主要特性

- **原生外壳** —— `winit` + `egui-wgpu`/`wgpu`（Metal），自定义标题栏、原生
  红绿灯按钮、原生菜单栏与快捷键、由设计令牌派生的明暗主题、中英文界面。
- **工作区** —— 带控制台运行状态的项目栏、遵循 `.gitignore` 的资源管理器、
  文件监听、源码管理侧栏、命令面板（`⌘K`）、全局搜索、工作区/会话持久化。
- **编辑器** —— rope 缓冲区、增量语法高亮、标签页、撤销/重做、多行缩进、
  查找替换、Diff 视图、输入法、原子保存、编码与换行符保留。
- **Git** —— 状态（暂存/未暂存/未跟踪、行数统计、ahead/behind）、分支/标签、
  带 lane 图的提交历史、提交详情、暂存/取消暂存、提交/修补、分支创建/删除/
  切换，以及只读 Diff 视图。
- **终端** —— 真实 PTY 会话、默认 1 万行回滚、每项目多控制台、回滚搜索、
  选择复制、括号粘贴、SGR 鼠标点击与滚轮、输入法。
- **双语界面** —— 主要界面支持英文与简体中文。

## 构建与运行

要求：macOS 13+、Rust stable（1.98+，仓库内 `rust-toolchain.toml` 已固定版本）、
Xcode Command Line Tools。

```sh
cargo run -p lazecode-app
```

测试与静态检查：

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
```

构建可双击启动的 App：

```sh
./scripts/build-app.sh          # dist/LazeCode.app
./scripts/build-app.sh --debug  # 更快，未优化
```

当前为 ad-hoc 签名；正式分发仍需 Developer ID 签名与公证。

## 快捷键

| 快捷键 | 功能 |
|---|---|
| `⌘K` | 命令面板 |
| `⌘P` | 快速打开 |
| `⇧⌘F` | 全局搜索 |
| `⌘B` | 显示/隐藏侧栏 |
| `⌘J` | 显示/隐藏底部面板 |
| `⌘S` | 保存 |
| `⌘Z` / `⇧⌘Z` | 撤销 / 重做 |
| `⌘F` | 编辑器内查找 |
| `⌘W` | 关闭标签 |
| `⌘,` | 设置 |
| `⌘+` / `⌘-` / `⌘0` | 放大 / 缩小 / 重置 |
| `⌘Q` | 退出（有未保存修改时拦截） |

## 架构

| crate | 职责 |
|---|---|
| `lazecode-core` | 项目、标签、设置、布局、命令、持久化 |
| `lazecode-fs` | 文件树、编码感知 IO、搜索、监听 |
| `lazecode-text` | rope 文档、撤销、语法缓存、Diff 模型 |
| `lazecode-git` | `gix` 读写 porcelain 子集 |
| `lazecode-terminal` | PTY 生命周期、VT 网格、快照、输入/搜索 |
| `lazecode-ui` | 设计令牌、控件、视图、浮层 |
| `lazecode-app` | winit/egui-wgpu 外壳、事件路由、后台任务 |

目标架构：[`docs/design/architecture.md`](docs/design/architecture.md)。
本轮实现范围与明确的未实现项：
[`docs/implementation-status.md`](docs/implementation-status.md)。

## 配置

- 设置：`~/Library/Application Support/LazeCode/settings.toml`
- 工作区/会话：`~/Library/Application Support/LazeCode/workspace.toml`

两者都是纯 TOML，可在应用退出后编辑；应用以原子方式写回。

## 当前状态

这是可工作的原生版本，但还不是功能完整的替代品。本轮明确
未实现：LSP、push/pull/fetch、merge/rebase/stash、worktree/submodule/LFS、
终端冷历史与 kitty 图形协议。依赖 Git 或终端工作流前请先阅读状态文档。

## 许可证

MIT，见 [`LICENSE`](LICENSE)。
