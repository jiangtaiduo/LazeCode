# 变更记录

[英文](CHANGELOG.md) | 简体中文

这里记录 LazeCode 所有值得注意的变更。格式基于
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/)，版本号遵循
[语义化版本](https://semver.org/spec/v2.0.0.html)。

## 未发布

### 新增

- 后台检查发现新版本时，顶栏搜索按钮右侧显示浅绿色 `vX.Y.Z 可更新` 徽标；
  点击打开现有更新对话框。徽标文案支持英文和简体中文。

### 变更

- 项目更名为 LazeCode。Bundle identifier 改为
  `io.github.jiangtaiduo.lazecode`，项目设置改用 `.lazecode/settings.json`，
  localStorage 键改用 `lazecode.` 前缀。旧设置不会自动迁移。
- 文档改为英文和简体中文两版；所有面向用户的文档都带有对应的语言切换链接。
- README 增加状态速览表、DMG 安装指引和故障排查；应用图标与 favicon 改用
  LazeCode 字标。
- CI 和发布工作流改用 `macos-15`；GitHub 将于 2026-11-02 退役 `macos-14`
  runner 镜像。
- 没有 Developer ID 时，本地和 CI 发布构建使用 `APPLE_SIGNING_IDENTITY=-`，
  让 macOS bundle 具有完整的 ad-hoc 签名，而不是只有 linker 临时签名。
- 自动检查更新默认开启（每天最多一次）；安装更新仍需要用户明确确认。

## [0.1.1] - 2026-10-30

### 新增

- 欢迎页在 LazeCode 字标下方显示当前运行版本，更新是否生效一眼可见，无需打开设置。

### 变更

- 发布产物改用新的更新签名密钥对；应用内更新使用本版本内置的公钥校验。

## [0.1.0] - 2026-10-29

### 新增

- 原生 Ghostty VT 终端，支持增量单元格帧、Kitty 图形、多标签/分屏、回滚缓冲区
  搜索和可访问文本镜像。
- Git 集成开发环境：DAG 提交图、分支/标签/远端/工作树/子模块/LFS、提交、
  推送/拉取/抓取、合并/变基/拣选/回滚、交互式变基、贮藏、搁置、按块暂存、
  差异查看器和批量丢弃提交。
- Monaco 编辑器，支持按文件编码、原子写入、文件监听、在文件中查找、全局搜索
  和 LSP 诊断。
- 本地优先设置、窗口状态持久化、英文/简体中文界面和本地化原生菜单。
- 跨语言线格式验证（34 项检查）、终端一致性基准、378 个 Rust 测试和 98 个
  界面测试。
