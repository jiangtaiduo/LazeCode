# 参与贡献

感谢参与。LazeCode 是一个用 Rust 编写的原生 macOS 应用：外壳使用
`winit` + `egui-wgpu`，代码位于 `crates/` 工作区。项目没有 WebView、
JavaScript 或 Node 运行时。

## 构建

要求：macOS 13+、Xcode Command Line Tools，以及 `rust-toolchain.toml`
固定的 Rust 工具链。

```sh
cargo run -p lazecode-app
./scripts/build-app.sh          # dist/LazeCode.app
./scripts/build-app.sh --debug  # 更快的调试包
```

## 检查

提交 PR 前以下四项必须通过：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p lazecode-app
```

界面改动请附上最新 `snapshot` 截图或录屏：

```sh
cargo run -p lazecode-app --bin snapshot -- /tmp/laze.png "$PWD" editor zh
```

## 目录

| crate | 职责 |
|---|---|
| `lazecode-core` | 项目、标签、设置、命令、持久化 |
| `lazecode-fs` | 文件树、编码感知 IO、搜索、监听 |
| `lazecode-text` | rope 文档、撤销、语法缓存、Diff |
| `lazecode-git` | `gix` 读写 porcelain 子集 |
| `lazecode-terminal` | PTY 生命周期、VT 网格、快照、输入/搜索 |
| `lazecode-ui` | 主题、控件、视图、浮层 |
| `lazecode-app` | winit/egui-wgpu 外壳、事件路由、后台任务 |

## 约定

- 可能阻塞或无界耗时的 UI 工作不要放在 winit 线程。
- 所有后台队列必须有界；所有缓存必须有淘汰策略。
- 不得静默削弱安全校验（路径、Git index、文件编码）。
- 每个 bug 修复都应增加回归测试。
- 英文与简体中文用户文档保持同步。
- 不要提交构建产物（`.app`、`target/`）、签名密钥或真实用户数据。
