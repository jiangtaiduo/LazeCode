# 变更记录

[英文](CHANGELOG.md) | 简体中文

这里记录 LazeCode 所有值得注意的变更。格式基于
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/)，版本号遵循
[语义化版本](https://semver.org/spec/v2.0.0.html)。

## [Unreleased]

### 新增

- 设置中新增界面字体、编辑器字体与终端字体选择，基于系统已安装字体目录
  （`fontdb`，包含 macOS `AssetsV2` 字体）。编辑器与终端使用独立的等宽
  字体族；终端默认跟随编辑器。
- 原生历史 sequencer：cherry-pick/revert 的干净/冲突路径、冲突 rebase 的
  停止/继续/中止/跳过、原生冲突列表/内容/解决接口，并保留作者身份。
  交互式 rebase 与 merge abort/continue 仍走 porcelain。
- 终端冷/磁盘回滚：有界 spool、稳定行 id、按需回填、搜索与关闭清理；
  kitty graphics 传输/放置/删除与有界图片缓存；kitty keyboard/
  modifyOtherKeys 的释放/重复事件；accesskit 终端文本镜像。
- 签名更新器：manifest 检查、semver 门禁、有界下载、SHA-256 + Ed25519
  校验、可回滚的原子可执行文件替换、设置 → 更新界面与自动检查。
- 文件 → 新建窗口（Cmd+Shift+N）启动独立 LazeCode 进程。

- 原生 Git clean/smudge 管线接入暂存、部分提交、状态与 checkout：
  `.gitattributes` 的 `text`/`eol`/`ident`/`filter`、`core.autocrlf`/
  `core.eol`、用户 `filter.<driver>.clean|smudge` 命令，以及由
  `.git/lfs/objects` 支撑的 LFS pointer clean/smudge（不执行 `git-lfs`
  二进制）。required filter 失败会中止；optional filter 失败回退原始字节。
- 事务化分支切换：checkout 前快照受影响的 worktree 路径与索引，物化或
  HEAD 更新失败时回滚，不再留下半切换的工作树。
- 纯 Rust 统一 diff 应用：hunk 暂存/取消暂存/丢弃与 patch 的
  apply/reverse/cached-apply；二进制与 rename patch 显式拒绝。`git init`
  与 discard 改用 `gix`/直接索引写入。
- 终端标签支持最多 4 个分屏（`向右分屏`/`向下分屏`），每个分屏独立 PTY，
  具有活动分屏焦点、单分屏关闭按钮与 row/column 等分布局。关闭最后一个
  分屏才会关闭标签；重启与控制台列表精确到分屏。
- 原生 `git reset --soft/--mixed/--hard`（支持分支与 detached HEAD，失败时
  回滚索引/工作树）；drop-HEAD 与 squash 复用该实现。忽略文件状态与待推送
  提交数改由 `gix` 计算（不再调用 `git status --ignored` / `git rev-list`），
  LFS status/track 也已原生实现。
- Shelf 全量/按路径恢复改走原生 patch 应用，不再启动 `git apply`。
- Shelf/复制为 Patch 的差异改由原生 HEAD→工作树生成（覆盖已跟踪、
  未跟踪、删除、mode 变化与缺失末尾换行）；输出可被 `git apply --check`
  接受，架藏不再启动 `git diff`。
- ahead/behind 与待推送提交数按 worktree + head + upstream 对象 ID 缓存
  （有界），ref 未变化时不再重复遍历祖先。
- Git clean/smudge/status/stage/stash/shelf 复用已打开的仓库，并且每次
  操作只构建一次 attributes 栈，不再逐文件重建。
- Stash 与架藏保存改用轻量变更扫描（不再计算逐文件增删统计、忽略目录
  遍历与 ahead/behind），减少工作树 I/O。
- stash apply 在两侧修改同一文件时执行文本三方合并，写入 Git 风格
  冲突标记及未合并索引 stage；无冲突时自动合并。大 LFS 文件从 `.git/lfs/objects` 流式写出，
  不再整块读入内存。
- 忽略文件状态按 worktree 缓存，并由 watcher 事件失效（2 秒兜底 TTL），
  避免每次 snapshot 都遍历目录。
- ahead/behind/待推送计数改用 gix revision walk（支持 commit-graph），
  不再手写父提交遍历。
- 原生远程 push、fetch（含 prune）、clone 与仅快进 pull。push 直接实现
  receive-pack：自建 zlib pack、解析 report-status、更新 remote-tracking/
  upstream，并具备 force-with-lease 语义。merge/rebase pull 仍走
  porcelain，等待 merge sequencer。
- stash apply 文本三方合并并写入冲突标记；大 LFS 对象流式写出；忽略文件
  状态按 worktree 缓存并由 watcher 失效；ahead/behind 与待推送计数改用
  支持 commit-graph 的 gix revision walk。
- 原生 merge（`gix::merge_commits`）：冲突 stage/标记与合并提交；原生
  干净 rebase 并保留作者身份。冲突 rebase、cherry-pick、revert 仍走
  sequencer porcelain。
- 原生 Git stash：push（支持 message、`-u`、`--keep-index`）、list、files、
  apply、pop、drop。实现会写入与 Git 兼容的 stash/index/untracked 提交及
  `refs/stash` reflog，因此与 `git` CLI 双向互通。
- 可选的语言服务器集成，覆盖稳定版的 11 种语言
  （`python/go/rust/c/cpp/java/ruby/php/kotlin/swift/lua`）：进程生命周期、
  JSON-RPC 分帧、文档同步、诊断、悬停、补全、跳转定义、格式化、重命名与
  文档符号。缺少 server 或崩溃时静默降级为语法高亮，不启动 Node。按产品
  决策默认关闭（实验性）；开启后状态栏显示当前文件的错误/警告数。
- 真实 `rust-analyzer` 的忽略式端到端测试
  （`cargo test -p lazecode-core --test lsp_e2e -- --ignored`）以及 mock
  server 生命周期测试。

### 修复

- 图片标签改用有上限的独立解码缓存，不再使用只增不减的 `egui_extras`
  loader 缓存。纹理按当前视口缩小，常驻像素上限 64 MiB，标签关闭后立即
  释放；关闭项目时同时清理 Markdown 图片 loader 与 `CommonMarkCache`。
  动画 GIF 仍使用逐帧 loader，并在 GIF 标签集合变化时清理缓存。此前浏览
  图片后关闭项目仍残留的约 100 MB 内存已消除。
- 固定 UI 字体改为 mmap，不再整份复制进堆。此前 Apple Color Emoji、宋体、
  黑体、冬青黑体、SF Pro、Menlo、Monaco、SF Mono 八个 fallback 文件会占用
  约 335 MiB 的 `Malloc Large`；现在保持文件映射。实测启动 footprint 从
  约 517 MiB 降到约 80 MiB，字形覆盖不变。
- 模态层新增全屏命中拦截：设置、对话框、菜单、搜索、命令面板或更新窗口
  打开时，点击不再穿透到编辑器、标签页、文件树、分隔条或标题栏；原生菜单
  快捷键同样受保护，`⌘W` 关闭当前模态层，而不是关闭下面隐藏的标签页。
- 分割线分层：真正的窗口分界保留清晰的 `border`，面板头部、工具栏、内部
  列分隔与活动栏改用 `border_soft`/`hairline`。
- Git log 左侧分支单击只选中（双击仍 checkout），不再设置日志分支过滤。
  Tags 随分支任务一起刷新，并同时出现在分支下拉与 Git log；空 Tags 分组
  隐藏；分支下拉移除“检出标签或修订版本”；级联子菜单只在点击后打开，过长
  的子菜单文字在菜单内裁切。
- 忽略文件只出现在 Ignored Files 组，勾选标记右对齐；提交视图选项的勾选
  不再压住文字。设置页更新说明自动换行。
- 文件树自动定位改为先按行偏移滚动再虚拟化渲染，视口外的文件也能定位。
  Blame 按钮宽度足够显示完整标签，并在无数据/失败时给出提示。
- 快捷键对齐 macOS PyCharm：`⌘⇧A` 查找操作、`⌘1` 项目、`⌘4` 底部面板、
  `⌘9` Git、`⌘K` 提交、`⌘⇧K` 推送、`⌘T` 更新/拉取、`⌘⇧T` 新建控制台、
  `⌘B` 转到声明、`⌥⌘O` 文档符号。
- 中英文混合排版：中文模式优先使用 PingFang SC，使标点与中英混排共用同一
  套字体度量；英文模式由 SF Pro 回退到 PingFang SC（不再回退到
  Hiragino），并让 SF Pro 使用 17pt Text optical size，而不是 28pt
  Display 默认值。
- 提交详情“全部展开”不再把所有目录折叠：树状态改为“折叠集合”
  （空集合 = 全部展开），“全部折叠”负责填充。两个按钮现在作用于所有
  层级。
- 工作树状态与提交暂存改为比较规范化（filter 后）字节，CRLF、`$Id$`
  与 LFS pointer 文件不会仅因工作树表示不同而被误报为已修改。
- 部分提交不再把未勾选文件取消暂存：提交树由 HEAD + 勾选路径构建，仅回写
  这些路径的索引项。
- 提交与部分提交现在按 git 顺序执行 `pre-commit`、`prepare-commit-msg`、
  `commit-msg`、`post-commit`；hook 失败会中止，`commit-msg` 的改写会作为
  最终提交信息。切换分支会执行 `post-checkout`（失败仅记录，不阻断）。
- 历史改写安全：`drop` 拒绝 merge 提交、非 HEAD 祖先以及已存在于远程分支的
  提交；`squash` 只接受最新的一段连续提交；merge/rebase/reset 校验以 `-`
  开头的 ref；`merge --continue` 与 `cherry-pick --continue` 保持 git 自身
  语义，不再静默 skip。
- 冲突处理：未合并索引项显示为 `C` 并进入 Merge Conflicts 组（不再落到
  Unversioned）；delete/modify 冲突选择删除侧时执行 `git rm`；合并编辑器在
  仍有冲突标记时禁止标记解决，并显示后端错误。
- Git 路径/架藏边界：discard 与冲突写回校验仓库相对路径及符号链接祖先；
  架藏删除/重命名/导入/读取拒绝 `..`、路径分隔符与控制字符。
- Git 读取：blame 复用每个提交的作者/时间；文件历史跟随重命名；子模块路径
  解析修正；ahead/behind 与待推送计数优先使用已配置 upstream，再回退
  `origin`；变更列表写入失败会回显，损坏 JSON 会备份。
- 终端：输出结束后不再永久 60fps 重绘；256 色使用 xterm 调色板；Cmd+F
  聚焦查找框并在打开时拦截 shell 输入；选区高亮/复制包含最后一个单元格、
  按字素簇复制并跳过 NUL 哨兵；括号粘贴中和控制字节且无法被内容提前结束；
  鼠标按下/释放/拖拽与滚轮坐标按指针发送；滚动条方向与触控板小数滚动修复；
  输出唤醒合并保留最后一帧；解析线程 panic 会关闭会话而不是永久卡死；
  shell 走白名单且剥离凭据环境变量；每个工作树最多 10 个控制台，底部
  dock 高度持久化。
- 状态栏显示真实编码（UTF-16/Latin-1 等），不再固定为 “Latin-1”；终端
  查找栏关闭时清除搜索高亮。

### 变更

- 页面重制地基（`feat-ui-redesign` 分支）：设计令牌层整体逐字移植稳定版
  样式表——两套同构的 macOS Native 主题（深色 variant-1 / 浅色 variant-2）
  取代自研配色；11/12/13/14px 字号阶梯与稳定版布局节奏（日志行 26px、
  树行 24px、变更行 22px、标签栏 30px、状态栏 26px、按钮 28px、右键菜单
  260px）成为共享常量；窗口底层绘制稳定版背景渐变，半透明面板在其上
  呈现「液态玻璃」质感。语法配色对齐稳定版 Monaco 主题
  （jetbrains-dark / ide-light）。
- 图标改由稳定版前端自有 SVG 源（`FileIcons.tsx`，原样提取）经纯 Rust
  `resvg` 栅格化为缓存纹理、绘制时按主题色 tint——图标几何与 Web 版
  完全一致；无稳定版对应的图标保留手绘矢量兜底。

### 新增

- 已跟踪文件右键菜单补齐稳定版抽屉的全部条目并按稳定版排序：新增功能项
  「与本地的前一版本比较」（HEAD↔工作区）、「Create Patch from Local
  Changes...」（系统保存对话框，补丁由后台任务写出）、「到此为止的历史」
  （文件历史对话框）；稳定版的灰色占位项（新标签页显示差异、与本地比较、
  打开仓库版本、拣选/提取/丢弃选中更改、从版本获取、显示与父提交的差异）
  按禁用态渲染；分隔线位置对齐；「移至 <列表>」在所选文件已全部属于该
  列表时改为禁用（多选同样生效，此前是直接隐藏）。
- 变更列表组头右键现在提供整组文件操作（提交/还原/移动/架藏；Unversioned
  组则是加入版本控制/删除），对齐稳定版组头菜单——不再只有新建/删除列表。
- 编辑器标签右键菜单新增「关闭左侧标签」及稳定版的禁用规则（首/末标签）
  与分隔线布局，保留新版补充的复制路径等条目。
- Esc 现在可以关闭对话框（稳定版行为）；冲突解决编辑器除外——防止误按
  丢掉手工合并的内容。
- Unshelve… 默认改为用架藏名新建变更列表（稳定版默认值）；删除架藏前
  先弹确认。
- 变更列表为空时不再抢先渲染「工作区干净」卡片（首份快照回来前显示
  「正在读取更改…」）；打开工作区 diff 的同时会把该文件设为变更列表
  当前选中（稳定版 selectedFile 同步）。
- Diff 窗口对齐稳定版单窗口模型：所有 diff 入口（分支比较文件、git 日志、
  提交抽屉、侧栏 Show Diff）复用同一个 diff 标签，只换内容，不再每个
  文件开一个标签。标签标题改用稳定版方案——`Diff (hash7)`、
  `Diff (from7 ↔ to7)`、`Working Tree Diff`——文件名在 diff 视图内部的
  标题栏展示。重复打开同一来源只激活标签不重新加载（避免与在途任务的
  去重竞争导致永远转圈）。
- 右键菜单不再渲染标题行（稳定版菜单从未有标题）：编辑器标签、终端
  标签、项目树、分支、提交、变更列表头、变更文件菜单直接以菜单项开始。
- 未跟踪文件现在可以通过源控右键菜单加入版本控制（「Add to VCS」/
  「Add N Files to VCS」，词条与稳定版逐字一致）：git add 走后台任务并
  按稳定版文案提示成功/失败，刷新后文件移出 Unversioned 组。未跟踪
  右键菜单整体对齐稳定版——Add to VCS、Jump to Source（单文件）、分隔
  线、废纸篓语义的 Delete…——不再显示只对已跟踪文件有意义的动作
  （提交/放弃/架藏/移动列表）。
- 未跟踪文件批量删除（「Delete N Files…」）一次确认后全部移入系统废纸
  篓，失败名单合并为一条提示。
- 分支比较以编辑器区常驻标签页复刻稳定版 CompareBranchesView 的页面设计
  （视觉用新版设计令牌）：上下两个 pane（每侧一条独立提交日志），中间
  8px 拖拽分隔条（占比持久化到 settings.toml）；每侧 banner（「存在于 X
  但不存在于 Y 的提交」，上侧带 ⇄交换）；pane 内右侧详情列（两侧共用、
  可拖 24-62%）：单提交文件 / 多选父提交..最新范围 diff / ⌘A 整侧
  merge-base 清单，以可折叠目录树呈现（numstat + 状态着色）。提交行支持
  单击/Cmd/Shift 多选、⌘A/Esc/↑↓、双击开整提交 diff、Load More(+500) 与
  截断提示；双击文件按稳定版规则选 diff 基线。标签页单实例，关闭即结束
  会话。
- 每侧独立过滤：搜索框（正则/大小写开关）、作者菜单、日期菜单
  （全部/今天/近 7 天/近 30 天）、分支引用菜单，以及触发重查的路径过滤。
- 比较与详情文件加载全部走后台任务，具备加载/重试/空态（旧实现同步阻塞
  UI 线程、失败后每帧重试并反复弹 toast）。
- 分支树工具栏：原「与工作区比较」死按钮改为「比较选中的分支」（需恰好
  选中两个分支），对齐稳定版 GitBottomPanel 工具栏。

### 修复

- 交互式变基此前是**静默空转**：对话框点「开始变基」只弹一条提示，什么
  都没执行。现在通过序列编辑器 shim 把 UI 编排的 todo（pick/reword/
  edit/squash/fixup/drop）真正交给 `git rebase -i` 执行，冲突由面板的
  继续/中止横幅接管。已补测试覆盖。
- 项目树删除文件此前是永久 unlink；现在与稳定版 `fs_delete` 一致移入系统
  废纸篓，同时关闭被删路径的编辑器标签并刷新 VCS 快照。
- 对话框懒加载在空列表时会**每帧重复执行**：Stash/Shelf 管理器、文件历史
  （每帧跑一次 `git log`）、Git 工具（无子模块的机器上每帧 spawn git）。
  现在各对话框每次打开只加载一次。
- 分支比较审计修复：分支多选改为**点击顺序**（原 `HashSet` 随机序导致比较
  左右分支随机互换）；比较会话携带项目 root，所有后台回填按 root 匹配
  （两个项目的同名分支不再互相污染）；⌘A 只全选当前侧（原来两侧全选）；
  方向键移动提交时清掉上一提交的陈旧文件清单；换向/路径重查立即作废旧
  选择；加载中切换提交不再永远卡在「加载中…」。
- 分支比较的详情缓存此前为会话级共享：一侧选中提交后，另一侧（未选中）
  会渲染出它的文件清单。详情缓存改为每侧独立，未选择的一侧固定显示
  「未选择提交」且文件区为空。
- 提交右键「与本地比较」此前把 `{hash}::{file}` 字面量当工作区路径打开；
  现在正确打开提交↔工作区差异。
- 移除从未被构造的 `CompareCommits` 死对话框状态。

### 变更

- `compare_branches` 对齐稳定版数据契约：三点（`other...side`）文件清单、
  `--numstat` 与 `--name-status` 合并（路径/状态字母/增删行数）、limit+1
  截断探测、按侧路径过滤；无共同祖先时 `merge-base` 为 `None`，文件清单
  退化为空（原实现误用 tip 对空树）。

### 新增

- 变更清单对齐稳定版提交抽屉：变更列表分组可折叠（组头 + 头部
  Expand/Collapse All），组头带三态勾选框批量勾选，冲突文件在默认列表内
  以「合并冲突」子组呈现并提供解决入口。
- 文件行多选（单击选中、Cmd/Ctrl 增减、Shift 按显示顺序范围选）与悬停
  回滚按钮；回滚前弹确认，支持批量，回滚新增文件时自动关闭其编辑器标签。
- 移植稳定版右键菜单：查看差异、跳转到源文件、解决冲突、
  提交文件…（独立提交信息）、还原选中更改、以补丁复制到剪贴板、
  移至变更列表、架藏更改…、刷新 VCS 更改与新建变更列表。
- Stash 按钮改为弹窗（说明输入 + 「包含未跟踪文件」`git stash -u`），
  不再静默 push；贮藏管理器加载失败会显示错误而非空列表。
- 架藏只架藏勾选文件（默认名：单文件取文件名、多文件取默认列表名），
  新增「Unshelve…」对话框支持按文件部分恢复并放入指定（或新建）变更列表。
- 提交信息历史（最近 10 条、去重、持久化到 settings.toml），dock 内
  弹出回填/清除；新增「提交并推送」按钮，提交成功后打开推送对话框。
- 非 Git 项目显示「当前目录不是 Git 仓库」提示与初始化入口（原 dock 内
  初始化按钮实际不可达）。

### 变更

- 变更清单勾选框语义对齐稳定版：「勾选 = 包含进本次提交」，默认全部
  不勾，提交范围即勾选集合（提交时暂存勾选项、取消其余暂存）；Amend
  不受勾选限制。
- 变更列表的新建/删除/移动改为后台任务执行（删除前先选「移入默认列表」
  还是「回滚丢弃」），存储改经真实 git-dir 解析，关联 worktree 的变更
  列表可以正确持久化。
- 架藏回滚改走逐文件放弃路径：暂存过的新文件会同时退出工作区与索引
  （此前只回滚工作区，暂存区残留变更）。
- 工具按钮行改为流式换行，窄侧栏/中文标签下不再被裁剪。

## [0.4.0] - 2026-11-04
