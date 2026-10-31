# LazeCode

[英文](README.md) | 简体中文

<p align="center"><img src="src-tauri/icons/icon.png" width="96" height="96" alt="LazeCode 图标"></p>

[![CI](https://github.com/jiangtaiduo/LazeCode/actions/workflows/ci.yml/badge.svg)](https://github.com/jiangtaiduo/LazeCode/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

LazeCode 是一个只保留编辑器、Git 和控制台等核心功能的 macOS 工具。
控制台可以由你自行启动任意编程代理工具；保留编辑器和 Git，主要是为了在个别场景下
手动修改和检查代码。基于 Tauri 2、Monaco 和原生 Ghostty VT 引擎开发。

无账号系统，无崩溃上报。唯一的自动后台请求是版本检查；其他可能联网的操作见
[隐私与网络](#隐私与网络)。

> **状态：基于 macOS 27 开发和测试。声明的最低版本为 macOS 13
> （Apple Silicon），但 macOS 13–26 尚未测试。仍处于 alpha 阶段。**
> 发布构建为 **ad-hoc 签名但未公证**
> （未配置 Developer ID 证书），本地使用请从源码构建；版本检查每天最多一次，
> 安装前需要用户确认（`设置 → 更新`）。

| | |
| :--- | :--- |
| 状态 | `0.1.3`，alpha |
| 平台 | macOS 13+，Apple Silicon（arm64）；已在 macOS 27 测试 |
| 技术栈 | Tauri 2、Rust 1.90+、React 19、Monaco、Ghostty VT |
| 许可证 | MIT |
| 账号 / 遥测 | 无账号；本地诊断默认关闭 |
| 自动网络请求 | 每日版本检查（默认开启，可关闭） |

---

## 定位

LazeCode 只保留三个核心界面，并刻意到此为止：

- **编辑器**——Monaco、项目树、文件监听、在文件中查找和 LSP 诊断。
- **Git**——DAG 提交图、分支与标签、远端、克隆、工作树、子模块、LFS、
  提交/推送/拉取/抓取、合并/变基/拣选/回滚、贮藏/搁置、按块暂存，以及差异与
  历史改写。
- **终端**——原生 Ghostty VT 引擎，与 PTY 在同一进程内运行；可在此启动任意编程代理工具。

Git 是一等界面，不是简单的状态查看器：日志、暂存、差异、分支/标签操作、历史改写
和远端同步都在应用内完成。

它不试图成为平台：没有账号、不内置人工智能服务商、不上传遥测，也没有扩展市场。
任意编程代理工具都可以直接在终端里运行，LazeCode 不会把它包进专有聊天面板。这样
应用可以保持小巧，编辑器和 Git 工作流也更可预期。

界面布局沿用熟悉的集成开发环境习惯：左侧项目树，底部 Git 和终端面板，提交抽屉，以及
全局搜索。**PyCharm 是主要设计参考。** 目标是让 PyCharm 用户不用重新学习一套
心智模型，就能找到项目树、Git 日志、提交抽屉、终端和搜索。

LazeCode 与 JetBrains 没有隶属关系。PyCharm 是 JetBrains s.r.o. 的商标；本项目
没有使用 JetBrains 的代码、图标或其他资源。

**搜索关键词：** Git 客户端、Git 图形界面、Git 集成开发环境、代码编辑器、终端、
macOS、Apple Silicon、Tauri、Monaco、Ghostty、Rust、本地优先、轻量、PyCharm 风格、
人工智能编程、开发者工具。

## 快速开始

LazeCode 需要 Apple Silicon 上的 macOS 13 或更高版本、Xcode Command Line Tools、
Node.js 22.6 或更高版本，以及 Rust 1.90 或更高版本。

```bash
npm ci
npm run tauri dev
```

构建本地 ad-hoc 签名应用见[从源码构建](#从源码构建)。

## 目录

- [定位](#定位)
- [快速开始](#快速开始)
- [为什么做 LazeCode](#为什么做-lazecode)
- [实测性能](#实测性能)
- [功能](#功能)
- [隐私与网络](#隐私与网络)
- [安全边界](#安全边界)
- [验证](#验证)
- [从源码构建](#从源码构建)
- [故障排查](#故障排查)
- [仓库结构](#仓库结构)
- [文档](#文档)
- [参与贡献](#参与贡献)
- [许可证](#许可证)

## 为什么做 LazeCode

大多数浏览器形态的集成开发环境通过 `xterm.js` 加 `node-pty` 嵌入终端，或者把 VT
解析器放进 WebAssembly。两种方式都会多一层进程边界，并且每帧都要序列化。

LazeCode 把 Ghostty VT 引擎作为**原生静态库**链接进 Rust 进程，只向前端传递紧凑的
单元格帧。这里没有第二个终端进程，没有 WebAssembly 沙箱，也没有跨 WebAssembly
边界的帧序列化。按键通过一次 Tauri IPC 调用进入已经持有 PTY 的进程；VT 解析器
和 PTY 始终不离开该进程。

## 实测性能

以下数据在不同主机上测得。不要跨章节比较绝对数值；更有意义的是比值。

### 1. 同一个解析器：WebAssembly 与原生

同一份负载（`src-tauri/src/vt.rs` 中的 `engine_bench_stream`，12 万行，
14.05 MB）：

| 指标 | wasm32 | 原生 arm64 |
| :--- | ---: | ---: |
| 解析吞吐 | 9.8 MB/s | **151.5 MB/s**（15.5 倍） |
| 全量快照（120×40） | 2.752 ms | **0.098 ms**（28 倍） |
| 单格读取 | 0.573 µs | **0.02 µs** |

原生 ABI 不是 WebAssembly ABI：指针是 8 字节，因此 `GhosttyGridRef` 的大小为
24，字段偏移是 `node@8`/`x@16`，而不是大小 12、`node@4`/`x@8`。把
WebAssembly 时期的偏移硬编码进来，会静默读到错误地址。参见随仓库附带的
[Ghostty 集成说明](src-tauri/vendor/ghostty-vt/README.md)。

### 2. 原生与 xterm.js：仅比较解析器

两边解析同一个 14,737,596 字节的流（`fnv1a32=8dd3affd`），120×40，取三次运行的
中位数。两列都不包含渲染器；原生列也不包含 IPC 和画布绘制。

| 负载 | libghostty-vt | xterm.js 无头模式 | 比值 |
| :--- | ---: | ---: | ---: |
| 整块写入，无分块开销 | **262.0 MB/s** | 111.4 MB/s | 2.35 倍 |
| 8 KB 分块，批量队列 | **278.9 MB/s** | 103.8 MB/s | 2.69 倍 |

数据记录于 Apple M5、xterm.js 6.0.0。之后在同一台主机上的复测得到
263.9/113.3 MB/s（2.33 倍）和 270.1/95.5 MB/s（2.83 倍）。不同运行之间大约有
±10% 的波动；比值比绝对值更稳定。

可用 `npm run bench:terminal` 复现。脚本会构建 Rust 示例、把流写入磁盘、校验
两边哈希一致，并取中位数。它还会记录一个故意不公平的模式：对每次 8 KB 的
`xterm.write` 都 `await`。这部分是调度开销，不是解析开销，因此没有放进表中。

在真实的原生管线中（解析，加上每 8 KB 读取生成一次增量差分帧，频率远高于
60 fps），引擎持续吞吐为 51.1 MB/s，整条流共产生 66.77 MB 差分。一次完整的
120×40 快照耗时 0.060 ms。

### 3. 无头管线：PTY → 引擎 → 线格式 → TypeScript 缓存

`npm run bench:pipeline` 把一个真实 PTY（`seq 1 1500000`，13.67 MB，
1,750 个 8 KB 分块）接入原生引擎，写出每一帧差分，再用 TypeScript 解码器和
`CellDiffCache` 回放：

| 阶段 | p50 | p95 | 说明 |
| :--- | ---: | ---: | :--- |
| 原生解析 + 差分 | 0.274 ms | 0.336 ms | 27.8 MB/s，1,750 帧 |
| TypeScript `decodeCellDiff` | 0.001 ms | 0.003 ms | 不复制单元格 |
| `CellDiffCache.applyDiff` | 0.010 ms | 0.014 ms | 合计 840 万个单元格 |

该次运行的进程峰值 RSS：**原生 44.4 MB，xterm.js 无头模式 169.2 MB**。两个数字
都包含各自的语言运行时，因此只能作为参考，不能当作纯解析器内存。这里不包含
画布绘制、Tauri IPC、React 调度和界面启动开销；比较的是引擎和线格式路径。

滚动感知差分是**刻意不做**的。`libghostty-vt` 的渲染状态只暴露脏行，不提供滚动
事件；因此视口滚动会按设计触发整行帧
（`scroll_viewport_marks_rows_dirty` 固定了这一行为）。解码并应用一个完整的
120×40 帧，p95 已经是 0.003/0.014 ms，滚动剩余的成本在画布绘制，而不是帧协议。

## 功能

### Git

- 带分支/用户/日期/路径过滤器的 DAG 提交图；泳道布局由真实的父提交列表计算
  （`assign_graph_lanes`），不依赖装饰顺序。
- 分支树：本地和远程检出、新建、重命名、删除；标签支持新建、列出和删除。
- 远端、克隆、子模块（添加/更新/同步）和工作树。
- 提交、推送、拉取、抓取；合并、变基、拣选和回滚，支持继续/中止；交互式变基。
- 贮藏、搁置/变更列表、按块暂存和修改提交。
- 批量文件操作和批量丢弃提交。批量丢弃只生成一次变基任务清单，拒绝脏工作区，并拒绝
  合并提交以及已经包含在远程分支中的提交。
- 基于 Monaco `DiffEditor` 的差异查看器：文件与提交比较、生成补丁、目录树，以及
  按块暂存/取消暂存/回滚。
- 提交抽屉，支持已暂存/未暂存变更列表。
- Git LFS 状态、拉取和模式跟踪。
- 凭据助手状态与配置。
- 子模块、工作树、LFS 和凭据的 Git 工具窗口。

### 终端

- 原生 Ghostty VT 引擎，支持 Kitty 图形协议（内联图片）。
- 增量单元格帧（`CellDiffCache`），不做整屏重绘。
- 多标签和分屏，每个标签最多四个 PTY；支持回滚缓冲区搜索。
- 单元格线格式包含完整的字素簇和 OSC 133 提示行标记。
- 面向屏幕阅读器的可访问文本镜像（`role="log"`）。
- 通过 OSC 7 / OSC 9 / OSC 1337 被动跟踪登录 shell 当前目录，并显示在状态栏。LazeCode
  不会改写你的 shell 配置文件。OSC 133 提示行会显示一条浅色提示带；OSC 52
  剪贴板写入保持禁用。
- 键表从随仓库附带的 `event.h` 生成（`npm run gen:key-table`）。
- 转发 SGR 鼠标事件；Cmd+点击打开 URL。
- 有界的单会话和全局回滚缓冲区，并显式回收子进程。

### 编辑器

- Monaco 按需加载工作线程；按文件处理文本编码并原子写入。
- 项目树和带防抖的文件监听，支持外部修改回放。
- 在文件中查找（内容搜索和替换），以及带可执行操作页的全局搜索。
- LSP 诊断和编辑器边栏标记。
- GitHub 风格的 Markdown 预览；远程图片只在点击后加载。
- 窗口位置、大小和最大化状态跨启动保留（`window_state.rs`，包含多显示器边界
  限制和损坏文件恢复）。

## 隐私与网络

- **没有账号，不上传遥测，也没有崩溃上报服务。**
- 本地诊断（命令名、耗时、成功/失败）**默认关闭**。可在 `设置 → 关于` 中开启；
  缓冲区只保存在本地存储，不会发送到任何地方。
- 更新检查默认开启，每天最多检查一次。它会请求
  `https://github.com/jiangtaiduo/LazeCode/releases/latest/download/latest.json`，
  并在安装前校验 minisign/Ed25519 签名。不会未经确认自动安装；可在
  `设置 → 更新` 关闭。
- 渲染 Markdown 时不会请求远程图片。LazeCode 会显示“加载远程图片”按钮，只有点击后
  才请求图片。
- 你配置的 Git 远端和语言服务器可能会自行发起网络请求。
- 本地数据位于
  `~/Library/Application Support/io.github.jiangtaiduo.lazecode`（窗口状态、语言）、
  `~/Library/Logs/io.github.jiangtaiduo.lazecode`（`panic.log`；调试构建还会写入
  桌面日志）以及 webview 的本地存储（设置、最近项目、诊断）。
- Git Console 仅在当前会话的内存中保留最近 200 条命令摘要，其中可能包含绝对路径、
  分支名和搜索词；它不会写入磁盘，但截图可能泄露这些路径。
- 最近的提交信息与收藏分支保存在 webview 本地存储中；可在提交抽屉中清除提交信息历史。

## 安全边界

如果你准备打开不受信任的仓库，下面这些边界值得了解：

- `read_file_content`、`write_file_content`、`fs_delete` 和 `fs_paste` 只能访问
  已授权的工作区根目录（`WorkspaceRegistry`）。根目录之外的请求会被拒绝。工作区
  之外的差异目标，以及原生选择器选中的路径，需要会话级的 `PathGrantRegistry`
  授权；项目关闭时授权会被清除。
- Git 子进程参数会经过 `reject_option_like`，适用时使用 `--end-of-options`；
  破坏性路径参数如果包含特殊的 pathspec 语法会被拒绝。变基 todo 行会按白名单校验。
- 每个 `git -C <repo>` 调用都会检查已登记的工作区根目录。应用启动后，空注册表是
  **失败关闭**的；只有测试夹具允许空注册表。
- `git clone`、添加工作树和导入搁置补丁的目标路径，会按工作区或原生选择器授权
  检查。
- 终端转义序列不能静默触达系统：OSC 52 剪贴板写入没有接到任何回调；OSC 8 链接
  在打开前会经过 `http`/`https` 白名单；Kitty 图片存储上限为 64 MB。
- 顶层导航使用精确的协议/主机策略：本地来源（`tauri://localhost`、
  `tauri.localhost`、`ipc.localhost`、开发用本地地址）在窗口内加载；外部
  `http(s)` 交给系统浏览器；`data:`、`blob:`、`file:` 和自定义协议会被拦截。
- panic 钩子会把崩溃位置写入 `panic.log`。发布配置使用 `panic = "abort"`，
  如果没有这个钩子，崩溃不会留下任何痕迹。

**威胁模型：** 仓库内容是不可信的；渲染器是可信的。终端和用户配置的语言
服务器按设计可以执行任意程序，因此完全被攻陷的渲染器不在隔离边界内。上面的 IPC
检查用于降低恶意仓库内容带来的意外暴露，而不是任意 JavaScript 代码的沙箱。

## 验证

Rust 侧输出二进制单元格帧，TypeScript 侧负责解码。两边的类型系统都无法证明它们
一致：字段偏移写错时，两边都能编译，运行时却会读出垃圾数据。

### 跨语言线格式契约

契约由可执行规范 [`src/ghostty-vt/verifyWire.ts`](src/ghostty-vt/verifyWire.ts)
检查，共 **34 项**，测试夹具直接从 Rust 源码解析：

```bash
npm run verify:wire
```

每项检查输出一行；第一处不一致就会以非零状态退出。检查覆盖缓冲区长度、逐格码点
与样式、宽字符占位格、差分行、字素簇、行标记、键盘键值、修饰键位域，以及宿主
内容盒计算。其中多项检查直接解析 `ghostty.h`/`event.h` 获取权威位值，而不是
相信注释。

键表还有一条下限断言：键表可以增长，但缩小意味着整组按键被删除。

### 终端行为基准

`src-tauri/tests/terminal_conformance.rs` 把确定性的字节流送进原生引擎，再把
规范化后的屏幕快照（文本、光标、非默认样式、回滚缓冲区和滚动条）与签入的基准
比较。覆盖范围包括 SGR/真彩色、光标移动与擦除、宽字符中日韩文本、emoji、备用屏、
滚动区域、DEC 模式、OSC 52/OSC 8、回滚缓冲区、调整大小、真实 PTY 往返，以及
带种子的转义序列模糊测试。

```bash
cd src-tauri && cargo test --test terminal_conformance
```

只有在
`UPDATE_GOLDENS=1 cargo test --test terminal_conformance`
时才会重新生成基准；生成后必须逐个人工检查改动。

### Rust 与界面测试

在参考环境（Apple M5 / macOS 27）上：**Rust 378 个测试通过，7 个忽略**；
**界面 104 个测试通过**。Rust 集成测试会创建真实的临时仓库并运行真实 Git：

```bash
cd src-tauri && cargo test --locked
cd .. && npm run test:ui
```

| 模块 | 锁定的内容 |
| :--- | :--- |
| `commands` | Git 参数校验、路径授权、工作区竞态、差异流 |
| `git_menu_test` | 真实仓库中的标签/丢弃/变基/合并/拣选/应用工作流 |
| `vt` | 单元格帧布局、差分合并、字素簇、滚动、搜索、鼠标、Kitty 图形 |
| `pty` | shell 启动、子进程回收、cwd 规范化、锁顺序 |
| `window_state` | 几何持久化、多显示器边界限制、损坏文件恢复 |
| `lsp` / `file_encoding` / `file_watch` | LSP 生命周期、非 UTF-8 恢复、外部修改回放 |
| `tests/` | 跨模块回归（UTF-8、路径逃逸、剪贴板）、按键/鼠标字节、终端基准 |

`--all` → `--branches --remotes` 的改动由
`git_log_all_refs_excludes_stash_commits` 固定：现代 Git 中裸 `--all` 会把
`refs/stash` 拉进来，导致两个双父提交进入日志，并在 DAG 中画出虚假的三方分叉。

### 静态检查与格式门禁

```bash
npm run check          # 线格式/菜单/目录/i18n/命令目录/tsc
npm run test:ui        # Vitest + Testing Library（jsdom）
npm run verify:fmt     # cargo fmt --check
./scripts/clippy.sh    # cargo clippy --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

## 从源码构建

### 前置条件

| 要求 | 说明 |
| :--- | :--- |
| macOS | 27，Apple Silicon（arm64）。随仓库附带的 `libghostty-vt.a` 是 arm64。 |
| Node.js | 22.6 或更高（验证脚本使用 `--experimental-strip-types`）。 |
| Rust | 1.90 或更高；`rust-toolchain.toml` 固定 1.90.0。 |
| Xcode | Command Line Tools（链接器、libc++、macOS SDK）。 |
| Zig | 仅在重新构建 VT 库时需要。 |

### VT 库

`libghostty-vt.a` 与架构相关。仓库中附带的是 macOS arm64 版本。按优先级有三种
方式满足构建：

1. **指向已有构建**（保持仓库干净，也可跨架构）：

   ```bash
   GHOSTTY_VT_LIB_DIR=/path/to/ghostty/zig-out/lib npm run tauri build
   ```

2. **从源码构建**（需要 Zig，并显式指定 Ghostty 版本）：

   ```bash
   GHOSTTY_REF=<ghostty-commit-or-tag> npm run build:vt-lib
   ```

   脚本会使用 `mktemp`、检出指定版本、以 `-Doptimize=ReleaseFast` 构建、把库和
   头文件复制到 `src-tauri/vendor/ghostty-vt/`，并写入 `build-info.txt` 和
   `SHA256SUMS`。

3. **使用仓库附带的 arm64 库**——已经在仓库中，无需操作。

如果方式 (1) 和 (3) 都无法解析，构建脚本会带着准确命令退出，而不是让链接器报
`unable to find library -lghostty-vt`。

### 运行

```bash
npm ci
npm run tauri dev
```

### 构建本地应用（ad-hoc 签名）

Tauri 会先执行 `npm run build`（`beforeBuildCommand`），产物位于
`src-tauri/target/release/bundle/`。

```bash
APPLE_SIGNING_IDENTITY=- npm run tauri build -- --bundles app
open src-tauri/target/release/bundle/macos/LazeCode.app
```

生成 DMG 并安装：

```bash
APPLE_SIGNING_IDENTITY=- npm run tauri build
open src-tauri/target/release/bundle/dmg/LazeCode_0.1.0_aarch64.dmg
```

打开 DMG，把 `LazeCode.app` 拖进 `/Applications`。

`APPLE_SIGNING_IDENTITY=-` 会生成完整的 ad-hoc bundle 签名；`--no-sign` 只留下
linker 的临时签名，会让 `codesign --verify` 失败。应用仍然没有 Developer ID、
没有公证，但可以在本地运行，不会带上 Gatekeeper 隔离属性。

### 发布构建

`src-tauri/tauri.release.conf.json` 会开启更新包产物。在设置
`TAURI_SIGNING_PRIVATE_KEY` 后（只在 CI 中配置，绝不提交）：

```bash
APPLE_SIGNING_IDENTITY=- npm run tauri build -- --config src-tauri/tauri.release.conf.json
```

没有 Developer ID 证书时保留 `APPLE_SIGNING_IDENTITY=-`，让 bundle 仍具有完整的
ad-hoc 签名。`.app` 仍然没有公证；当前状态见
[安全策略](SECURITY.zh-CN.md) 和
[`.github/workflows/release.yml`](.github/workflows/release.yml)。

## 故障排查

### macOS 拒绝打开应用

发布构建使用 ad-hoc 签名，没有公证。对于自己构建、或确认来源和校验和的构建，
可以右键点击应用并选择“打开”。如果仍被拦截，到“系统设置 → 隐私与安全性”中允许。
从网络下载的副本还可能带有隔离属性：

```bash
xattr -dr com.apple.quarantine /Applications/LazeCode.app
```

只对信任的构建清除隔离属性。

### `unable to find library -lghostty-vt`

仓库附带的 `libghostty-vt.a` 仅适用于 macOS arm64。可用下面的命令确认目标架构：

```bash
file src-tauri/vendor/ghostty-vt/lib/libghostty-vt.a
```

其他架构请把 `GHOSTTY_VT_LIB_DIR` 指向对应构建，或使用
`GHOSTTY_REF=<commit-or-tag> npm run build:vt-lib` 重新构建。参见
[VT 库](#vt-库)。

### 更新检查失败

`设置 → 更新` 会请求
`https://github.com/jiangtaiduo/LazeCode/releases/latest/download/latest.json`。
在发布带有 `latest.json` 和有效签名的 Release 之前，检查会失败；本地构建可以
直接关闭自动检查。

### 日志在哪里？

发布构建崩溃后会把 `panic.log` 写到
`~/Library/Logs/io.github.jiangtaiduo.lazecode/`；调试构建还会在那里写入桌面
日志。窗口状态和原生菜单语言位于
`~/Library/Application Support/io.github.jiangtaiduo.lazecode/`。

## 仓库结构

```text
src/
  App.tsx                 工作区编排、Git 操作、工作区切换
  components/             标题栏、Git 面板、终端和对话框
  services/               IPC 桥、设置分层、更新器、i18n、终端接入
  ghostty-vt/             单元格帧解码器、线格式验证和生成的键表
src-tauri/
  src/commands.rs         Git 命令、路径授权、DAG 布局
  src/vt.rs               原生 VT 绑定、单元格帧、差分合并
  src/pty.rs              PTY 会话、子进程回收、响应转发
  src/lsp.rs              语言服务器进程注册表
  src/file_watch.rs       防抖工作区文件监听
  src/menu.rs             原生本地化菜单
  src/window_state.rs     窗口几何持久化
  build.rs                解析 VT 静态库
  vendor/ghostty-vt/      头文件、许可证和 arm64 静态库
docs/                     架构与 i18n 说明
scripts/                  验证、基准测试和 VT 重建脚本
```

数据流和安全边界概览见 [`docs/architecture.zh-CN.md`](docs/architecture.zh-CN.md)。

## 文档

| 文档 | 英文 | 简体中文 |
| :--- | :--- | :--- |
| 项目说明 | [`README.md`](README.md) | [`README.zh-CN.md`](README.zh-CN.md) |
| 架构 | [`docs/architecture.md`](docs/architecture.md) | [`docs/architecture.zh-CN.md`](docs/architecture.zh-CN.md) |
| 界面多语言 | [`docs/i18n.md`](docs/i18n.md) | [`docs/i18n.zh-CN.md`](docs/i18n.zh-CN.md) |
| 参与贡献 | [`CONTRIBUTING.md`](CONTRIBUTING.md) | [`CONTRIBUTING.zh-CN.md`](CONTRIBUTING.zh-CN.md) |
| 安全 | [`SECURITY.md`](SECURITY.md) | [`SECURITY.zh-CN.md`](SECURITY.zh-CN.md) |
| 行为准则 | [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) | [`CODE_OF_CONDUCT.zh-CN.md`](CODE_OF_CONDUCT.zh-CN.md) |
| 变更记录 | [`CHANGELOG.md`](CHANGELOG.md) | [`CHANGELOG.zh-CN.md`](CHANGELOG.zh-CN.md) |
| 第三方声明 | [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) | [`THIRD_PARTY_NOTICES.zh-CN.md`](THIRD_PARTY_NOTICES.zh-CN.md) |
| 法律声明 | [`NOTICE`](NOTICE) | [`NOTICE.zh-CN.md`](NOTICE.zh-CN.md) |

`LICENSE`、`NOTICE` 和上游 Ghostty 集成说明属于法律或第三方文本。英文原文具有
权威性；中文翻译仅为方便阅读。

## 参与贡献

见 [`CONTRIBUTING.zh-CN.md`](CONTRIBUTING.zh-CN.md)。简要来说：

- `npm run check`、`npm run test:ui`、`npm run verify:fmt`、
  `./scripts/clippy.sh` 和
  `cargo test --manifest-path src-tauri/Cargo.toml --locked` 必须全部通过。
- 修改单元格帧或按键编码时，必须在 `verifyWire.ts` 中补充对应检查。
- 保留键表下限断言；删除一组按键必须让检查失败。
- 键值从随仓库附带的 `event.h` 生成：
  `npm run gen:key-table`；`verify:key-table`（属于 `npm run check`）会在漂移时
  失败。

## 许可证

[MIT](LICENSE)。第三方声明见 [`NOTICE.zh-CN.md`](NOTICE.zh-CN.md) 和
[`THIRD_PARTY_NOTICES.zh-CN.md`](THIRD_PARTY_NOTICES.zh-CN.md)。

Ghostty 使用 MIT 许可证（© 2024 Mitchell Hashimoto 及贡献者）。Monaco 使用
MIT。React 使用 MIT。Tauri 使用 Apache-2.0 或 MIT。
