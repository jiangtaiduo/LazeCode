# UI 复刻差异基线（feat-ui-redesign vs 稳定版 main）

2026-11-08 双端实机截图对比（`/tmp/lazecode-ui-redesign/{new,stable}.png`，
macOS Native 深色，两窗口 1470×923 同位堆叠）。

阶段 0（commit 3cd41b6）之后已一致：深色整体色板、系统蓝 accent、Git 语义色、
玻璃面板透明度、底层渐变、SVG 图标几何、JetBrains 语法色。

## 差异清单（按区域 → 责任批次）

1. 左侧竖条：稳定版单条 40px strip（上组 Project/Commit/Compare，下组 Git
   Log/Terminal/Settings，active 态 3px accent 左条 + `rgba(255,255,255,0.1)`
   底）；新版是 58px 头像 rail + 44px activity 双条 → **阶段 1**。
2. 标题栏：稳定版 = 红绿灯 → LC logo 按钮（项目菜单）→ 分支下拉 → 项目
   标签 pill（图标+标题+×）→ 右端搜索图标；新版 = 项目名+路径块、双分支
   下拉、内联搜索框、running 指示、布局开关 → **阶段 2**。
3. 左抽屉：稳定版三互斥抽屉（Project 270 / Commit 330 / Compare），Commit
   抽屉带 Commit/Shelf 页签、图标工具行、Changes 组头、底部 Commit
   Message + Commit / Commit and Push…；新版 Explorer/源控/控制台视图切换
   → **阶段 1（结构）+ 阶段 2（Commit 抽屉）**。
4. 欢迎页：稳定版大灰字 LazeCode + 4 行快捷键（kbd 徽章），中央画布透出
   body 渐变；新版自有品牌块+卡片 → **阶段 2**。
5. Git 面板：稳定版头部 Git/Log/Console 页签 + 右侧最大化/关闭；分支列
   左侧有迷你图标栏（«/+/↓/🗑/⇄/🔍/↕/⇅/★），默认无常驻搜索框；新版多一个
   worktree 页签、无迷你栏、常驻 Filter branches 搜索框 → **阶段 2**
   （worktree 页签保留但样式对齐 subtab）。
6. 日志行：稳定版日期短格式（Yesterday 23:00 / 2026/11/6），ref 徽章与
   日期列不重叠；新版全量日期且徽章长时压日期 → **阶段 2**。
7. 状态栏：稳定版左=项目名，右=Ready/UTF-8/缩进/语言；新版左=项目+分支+
   running，右=changed/UTF-8/LF → **阶段 2**（分支信息移标题栏，对齐稳定版）。
8. 编辑器标签栏：稳定版 30px、文件图标+标题+×/脏点；新版 35px、类型徽章
   → **阶段 2**。
9. Diff 工具栏：稳定版文件名+Modified 左置，右侧 Commit hash · +1 −1、
   hunk 导航、unified/split 切换为 28px 工具栏 + 28px 等宽 pane 标题栏；
   新版部件齐但排布不同 → **阶段 2**。

## 结构决策

- 头像 rail（多项目）→ 复刻为稳定版标题栏项目标签 pill + logo 菜单；多项目
  能力保留，外观一比一。
- 新版特有且无稳定版对应物的功能（worktree 页签、命令面板等）保留，但视觉
  材质统一到稳定版组件样式。
