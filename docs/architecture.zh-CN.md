# 架构

[英文](architecture.md) | 简体中文

LazeCode 是一个单原生进程（Rust/Tauri），可以有一个或多个 WebView 窗口。前端使用
React 19 和 Monaco；后端负责 Git 子进程、PTY、VT 引擎、文件 I/O 和窗口状态。

```text
┌──────────────────────────── macOS 应用 ────────────────────────────┐
│  WebView（React）                 Rust（Tauri）                    │
│  ───────────────                 ────────────                      │
│  App.tsx  ── 调用 ───────────────▶ commands.rs ── git ──▶ git       │
│  components/  ◀── 事件 ─────────── pty.rs ── PTY ──▶ shell          │
│  ghostty-vt/  ◀── 单元格帧 ─────── vt.rs ── libghostty-vt.a         │
│  services/                        window_state.rs                   │
└─────────────────────────────────────────────────────────────────────┘
```

## 终端路径

1. `pty.rs` 使用 `portable-pty` 打开 PTY，从允许列表中选择并启动用户的登录 shell，
   清除凭据类环境变量，然后启动读取线程。
2. 读取线程把 PTY 字节写入 `vt.rs` 中的 `NativeTerminal`，后者在同一进程内调用原生
   `libghostty-vt` 解析器。
3. `vt.rs` 把全量快照或增量差分帧编码成紧凑的二进制缓冲区
   （由 `src/ghostty-vt/cellFrame.ts` 解码）。
4. `CellDiffCache` 应用帧；`GhosttyTerminal.tsx` 把有变化的行绘制到画布，并把
   文本镜像到带有 `role="log"` 的可访问元素。
5. 按键在前端编码（`domKeys.ts`），通过一次 `pty_write` IPC 调用发送。鼠标事件
   使用引擎的鼠标编码器。

线格式是跨语言契约。`src/ghostty-vt/verifyWire.ts` 解析 Rust 侧的声明和测试夹具，
检查 34 项不变量；修改布局时必须同步更新它。

## Git 路径

`commands.rs` 以子进程方式运行 `git`。每次调用都会：

- 在值可能被误认为选项时经过 `reject_option_like` / `--end-of-options`；
- 校验破坏性的仓库相对路径；
- 在 `run_with_timeout` 下运行，避免管道缓冲区死锁；
- 使用 `git -C <dir>` 时经过 `authorize_git_repo` 检查。应用启动后，空工作区
  注册表是失败关闭的。

前端不会解析 Git 的易变人类可读输出；只要 Git 提供稳定格式，DAG 布局、文件状态和
差异流都在 Rust 侧计算，再以 JSON 发送。

## 文件 I/O 路径

`WorkspaceRegistry` 保存用户打开的工作区规范根路径。`authorize_workspace_path`
会规范化目标路径（解析符号链接），并要求它位于某个根路径内；`need_write` 还会
拒绝只读根路径。

`PathGrantRegistry` 保存由原生 NSOpenPanel 选择器签发的会话级授权，用于用户明确
指定工作区之外的文件（差异目标、克隆/工作树目标、搁置补丁）。项目关闭时授权会被
清除。

## 窗口与应用生命周期

`window_state.rs` 按窗口把几何状态持久化到应用配置目录。Bundle identifier 是
`io.github.jiangtaiduo.lazecode`，因此窗口状态和原生菜单语言位于
`~/Library/Application Support/io.github.jiangtaiduo.lazecode/`。`lib.rs` 在显示
主窗口前恢复状态，处理 `RunEvent::Reopen` 和退出前的刷盘，并安装 panic 钩子。
导航守卫插件使用精确的协议/主机匹配检查每一次顶层导航。

## 前端结构

- `App.tsx` 负责工作区/项目切换和 Git 编排。
- `components/` 负责展示和局部交互；对话框按需加载。
- `services/` 包含 IPC 桥、设置层、i18n、终端数据源、文件监听、Monaco 配置、
  更新器和诊断。
- `ghostty-vt/` 是二进制解码器、生成的键表和线格式验证器。

## 构建与发布

`build.rs` 从 `GHOSTTY_VT_LIB_DIR` 或仓库附带的 macOS arm64 副本解析
`libghostty-vt.a`。只有 `scripts/build-vt-lib.sh` 会重新构建附带库，并且必须显式
提供 `GHOSTTY_REF`；它会把 commit、Zig 版本和 SHA-256 记录到
`src-tauri/vendor/ghostty-vt/`。

`src-tauri/tauri.conf.json` 是贡献者构建配置（不生成更新包）。
`src-tauri/tauri.release.conf.json` 开启更新包产物；CI 从仓库密钥提供
`TAURI_SIGNING_PRIVATE_KEY`。CI 使用 `macos-15`（arm64）。没有 Developer ID
证书时，CI 使用 `APPLE_SIGNING_IDENTITY=-` 做 ad-hoc 签名，但不公证；见
`SECURITY.zh-CN.md`。
