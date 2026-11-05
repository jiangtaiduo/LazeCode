# 架构

[英文](architecture.md) | 简体中文

LazeCode 是单原生 macOS 进程。`winit` 负责事件循环，`egui` 绘制外壳，
`wgpu` 通过 Metal 渲染，所有应用服务均为 Rust。没有 WebView、JavaScript
运行时或 Node。

## Crate 划分

| crate | 职责 |
|---|---|
| `lazecode-core` | 项目、标签、终端、设置、命令、持久化 |
| `lazecode-fs` | 项目树、编码感知文件 IO、搜索、文件监听 |
| `lazecode-text` | rope 文档、撤销、增量语法高亮、Diff |
| `lazecode-git` | `gix` 状态/日志/Diff 与有限的写操作 porcelain |
| `lazecode-terminal` | PTY 生命周期、`alacritty_terminal` 网格、快照、输入/搜索 |
| `lazecode-ui` | 设计令牌、控件、视图、浮层、i18n |
| `lazecode-app` | winit/egui-wgpu 外壳、事件路由、后台任务 |

依赖方向是单向的：

```text
app ──▶ ui ──▶ core ──▶ fs
 │       │       ├──▶ git ──▶ text ──▶ fs
 │       │       └──▶ terminal
 │       └──▶ text
 └──▶ terminal
```

## 事件与数据流

- winit 线程拥有 `AppModel`、`UiRuntime` 和 `JobQueue`，不阻塞在 Git、项目
  扫描或文件搜索上。
- UI 操作把 `Command` 放入 `AppModel::commands`；应用外壳在渲染后执行，并为
  I/O 启动带 key 去重的工作线程。
- 工作线程通过 channel 回传 `JobCompletion { key, result }`。完成时总是释放
  对应的 in-flight key，再在 UI 线程更新模型。
- `egui` 通过 `repaint_delay` 决定何时重绘；空闲时事件循环不会持续渲染。
- 终端会话有 reader 线程（VT 解析与快照）、writer 线程（有界输入队列）和
  reaper 线程；UI 只读取最新快照。
- 文件监听以较低频率轮询；变更批次会刷新项目树、重载无修改的编辑器缓冲，
  并调度 Git 状态刷新。

## Git

`lazecode-git` 直接使用 `gix`。读路径覆盖状态、日志、分支、标签、Diff 和提交
详情；写路径覆盖暂存/取消暂存、提交/修补、分支创建/删除/切换。对于无法安全
表达的仓库状态，porcelain 会明确拒绝：子模块、未解决冲突、merge/rebase/
cherry-pick 状态、不支持的 index 扩展，以及工作区之外的路径。

## 持久化

- 设置：`~/Library/Application Support/LazeCode/settings.toml`
- 工作区：`~/Library/Application Support/LazeCode/workspace.toml`

两者都采用原子写，并在加载时校验。

## 构建与发布

`cargo build -p lazecode-app` 构建二进制。
`./scripts/build-app.sh` 使用 `packaging/macos/Info.plist` 与应用图标生成
`dist/LazeCode.app`。Release CI 在 `macos-15` 上构建并签名；公证与自动更新
尚未实现。

长期设计基线见 [`design/architecture.md`](design/architecture.md)；已实现能力
矩阵与明确的未实现项见 [`implementation-status.md`](implementation-status.md)。
