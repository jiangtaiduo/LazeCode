import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { appConfirm } from './services/confirmDialog';
import { ConfirmDialog } from './components/ConfirmDialog';
import { TitleBar } from './components/TitleBar';
import { LeftStrip, type ActiveDrawer, type BottomTool } from './components/LeftStrip';
import { GhosttyTerminal, type GhosttyTerminalHandle } from './components/GhosttyTerminal';
import { CommitDrawer } from './components/CommitDrawer';
import { ProjectDrawer } from './components/ProjectDrawer';
import { CompareDrawer } from './components/CompareDrawer';
import { CompareBranchesView } from './components/CompareBranchesView';
import { EditorTabBar } from './components/EditorTabBar';
import { WelcomeView } from './components/WelcomeView';
import { DiffViewer } from './components/DiffViewer';
import { monacoLspBridge } from './services/monacoLspClient';
import { GitBottomPanel } from './components/GitBottomPanel';
import { SearchEverywhereModal } from './components/SearchEverywhereModal';
import { FindInFilesModal } from './components/FindInFilesModal';
import { CloneDialog } from './components/CloneDialog';
import { GitToolsDialog } from './components/GitToolsDialog';
import { BranchContextMenu } from './components/BranchContextMenu';
import { SquashDialog } from './components/SquashDialog';
import { CommitContextMenu } from './components/CommitContextMenu';
import { ProjectTreeContextMenu } from './components/ProjectTreeContextMenu';
import type { BranchMenuState } from './components/BranchContextMenu';
import type { CommitMenuState } from './components/CommitContextMenu';
import type { TreeMenuState } from './components/ProjectTreeContextMenu';
import type { ResetMode } from './components/ResetDialog';
import type { NameDialogState } from './components/NamePromptDialog';
import { TextPromptDialog } from './components/TextPromptDialog';
import { useMarkdownViewState } from './hooks/useMarkdownViewState';
import { useTextPrompt } from './hooks/useTextPrompt';
import type { AppSettings } from './services/appSettings';
import type { MarkdownViewMode } from './components/MarkdownView';
import { DEFAULT_SETTINGS, loadSettings, persistSettings, applyUiFontSize, applyProjectSettingsText, clearProjectLayer, getProjectLayer } from './services/appSettings';
import { projectSettingsPath } from './services/settingsLayers';
import { getLanguage, initLanguage, setLanguage, t } from './services/i18n';
import { errText, errCode } from './services/errorText';
import { parentOfPath, baseNameOf, toWorkspaceAbsPath, lspRootFor } from './services/pathUtils';
import { readPanelSize, writeStorage } from './services/storage';
import {
  GIT_PANEL_HEIGHT_KEY,
  TERMINAL_PANEL_HEIGHT_KEY,
  TERMINAL_MAXIMIZED_KEY,
  appViewportHeight,
  defaultGitPanelHeight,
  readPanelLayout
} from './services/panelLayout';
import {
  RECENT_WORKSPACES,
  clearLastWorkspace,
  loadHistory,
  loadLastWorkspace,
  pushHistory,
  removeFromHistory,
  saveLastWorkspace,
} from './services/historyStore';
import { trackEvent } from './services/telemetry';
import { useStableCallback } from './hooks/useStableCallback';
import { useFavoriteBranches } from './hooks/useFavoriteBranches';
import { useCommitMessageHistory, loadCommitMessages } from './hooks/useCommitMessageHistory';
import { invokeCommand, isTauriEnvironment } from './services/tauriBridge';
import { useAppMenu, MENU } from './services/appMenu';
import { setCursorIndicator as setCursorRef, useCursorIndicator } from './services/cursorStore';
import { checkForAppUpdate, type UpdateInfo } from './services/updaterService';
import type { UpdateDialogState } from './components/UpdateDialog';
import { MONACO_THEME_BY_APP_THEME, disposeModelForPath, ensureMonacoReady } from './services/monacoSetup';
import { detectGhostty } from './services/ghostty';
import { ensureLspForLanguage, resetLsp } from './services/lsp';
import { shouldAutoSave } from './services/autoSavePolicy';
import { planProjectPrune } from './services/projectPrune';
import { planExitFlush } from './services/exitFlush';
import { isImageFile, isMarkdownFile, languageOf } from './services/fileTypes';
import { startPanelDrag, startPointerDrag } from './services/panelDrag';
import {
  contentHash,
  resolveExternalChange,
  probeFastPath,
  onExternalChange,
  onDirectoryChange,
  debounceDirectoryChanges
} from './services/fileWatch';
import type { FileProbe } from './services/fileWatch';
import { internFileTree } from './services/stringIntern';
import { PagedFileViewer } from './components/PagedFileViewer';
import type { FileRangeDto } from './components/PagedFileViewer';
import { logIpc } from './services/ipcLog';
import type { GitCommit, GitBranch, GitChangedFile, GitRepoInfo, GitTag, FileNode, Changelists, SequencerState, ShelfPreviewDto } from './types';
import './styles/theme.css';

/**
 * EditorView 懒加载。
 *
 * 它（经 monacoSetup）拖着整个 monaco 内核：约 4.5MB 未压缩 / 1.1MB gzip。
 * 静态 import 会让 monaco 进入入口图，Vite 随即在 index.html 注入
 * `<link rel="modulepreload">` + `<link rel="stylesheet">`，首屏无条件
 * 多下载 1.14MB —— 而欢迎页、文件树、Git 面板、终端都不需要它。
 *
 * 改成 React.lazy 后 monaco 落到动态 chunk，preload 标签消失，
 * 首次打开文件时才加载。配合 Suspense 显示加载态。
 */
const EditorView = React.lazy(() =>
  import('./components/EditorView').then(m => ({ default: m.EditorView }))
);

const MarkdownView = React.lazy(() =>
  import('./components/MarkdownView').then(m => ({ default: m.MarkdownView }))
);


/** localStorage 安全读写（readPanelSize/writeStorage/readStringArray）已抽到
 *  services/storage —— App 只保留编排，行为不变。 */

/**
 * ConflictDialog 同样拖着 monaco（三个冲突面板各一个 Editor），
 * 与 EditorView 同理必须懒加载，否则 monaco 又回到入口图。
 */
const ConflictDialog = React.lazy(() =>
  import('./components/ConflictDialog').then(m => ({ default: m.ConflictDialog }))
);

/**
 * 设置面板同理拆出。它的存储层已在 services/appSettings.ts（App 要
 * 同步读取），这里只有 UI 本体：两张表 + 四个分区，约 19KB 源码。
 * 打开面板时才下载，常驻在入口图里对首屏是纯负担。
 */
const SettingsDialog = React.lazy(() =>
  import('./components/SettingsDialog').then(m => ({ default: m.SettingsDialog }))
);
const UpdateDialog = React.lazy(() =>
  import('./components/UpdateDialog').then(m => ({ default: m.UpdateDialog }))
);
/**
 * Git 操作弹窗全部懒加载。
 *
 * 这些面板合计约 40KB 源码，且都只在用户显式点击按钮 / 右键菜单后
 * 才出现 —— 常驻入口图对首屏是纯负担。打开时多等一次 chunk 请求
 * （本地磁盘，通常几毫秒）换取首屏少解析这么多代码。
 *
 * SearchEverywhereModal（Ctrl+P）与三个右键菜单**不**拆：前者是
 * 键盘唤起的快速打开，拆了会把加载延迟暴露在按键到弹窗之间；后三
 * 个跟随右键弹出，同样对延迟敏感，且体积小、拆分的净收益有限。
 */
const PushDialog = React.lazy(() =>
  import('./components/PushDialog').then(m => ({ default: m.PushDialog }))
);
const PullDialog = React.lazy(() =>
  import('./components/PullDialog').then(m => ({ default: m.PullDialog }))
);
const CompareDialog = React.lazy(() =>
  import('./components/CompareDialog').then(m => ({ default: m.CompareDialog }))
);
const ResetDialog = React.lazy(() =>
  import('./components/ResetDialog').then(m => ({ default: m.ResetDialog }))
);
const FileHistoryDialog = React.lazy(() =>
  import('./components/FileHistoryDialog').then(m => ({ default: m.FileHistoryDialog }))
);
const NamePromptDialog = React.lazy(() =>
  import('./components/NamePromptDialog').then(m => ({ default: m.NamePromptDialog }))
);
const RemotesDialog = React.lazy(() =>
  import('./components/RemotesDialog').then(m => ({ default: m.RemotesDialog }))
);
const StashDialog = React.lazy(() =>
  import('./components/StashDialog').then(m => ({ default: m.StashDialog }))
);
const UnstashDialog = React.lazy(() =>
  import('./components/UnstashDialog').then(m => ({ default: m.UnstashDialog }))
);
const InteractiveRebaseDialog = React.lazy(() =>
  import('./components/InteractiveRebaseDialog').then(m => ({ default: m.InteractiveRebaseDialog }))
);
const ShelveDialog = React.lazy(() =>
  import('./components/ShelveDialog').then(m => ({ default: m.ShelveDialog }))
);

/** 持久化 key 统一收口（services/historyStore，命名空间 + 版本 + 旧 key 迁移）：
 * 提交信息 / 收藏分支 / 最近项目 / 上次项目 / 面板尺寸都不再散落裸 key。
 * 注意 ErrorBoundary.reset 按 `ide_` / `lazecode.` 前缀清键（见 main.tsx），
 * 新 key 全部落在 `lazecode.history.v1.*`，仍在清理范围内。 */
/** git log 每页加载条数（滚动到底自动翻页） */
const LOG_PAGE_SIZE = 50;

function logSinceOf(filter: 'all' | 'today' | 'week' | 'month'): string | null {
  if (filter === 'all') return null;
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() - (filter === 'today' ? 0 : filter === 'week' ? 7 : 30));
  return d.toISOString();
}
/** 收藏分支 key 已随 hook 迁入 historyStore，此处不再重复定义。 */
/**
 * 脏编辑缓冲上限：关闭标签时刻意保留未保存内容，但无限期保留会让常开窗口的内存只涨不跌，
 * 因此只保留最近 8 个文件的脏内容；超出部分必须先写盘再回收，写盘失败的保留不动。
 */
const MAX_DIRTY_BUFFERS = 8;
/**
 * 单窗口内项目会话快照上限：每份快照含整棵文件树与全部编辑缓冲，切换过的项目越多占用越高，
 * 只保留最近 20 个；更早的快照在确认其中未保存修改已写盘后才释放。
 *
 * 取值理由（2026-10-14 讨论）：快照里都是有界小东西——树是 depth-2 懒加载且路径已驻留、
 * 脏缓冲另有 MAX_DIRTY_BUFFERS=8 + 1.2s 自动保存双重封顶，单份快照通常几百 KB。
 * 真正的内存大户（终端 scrollback 单会话可达 512MB、LSP server 进程）根本不在快照里、
 * 不受此上限约束——所以把上限卡在 3 挡不住大头，只会逼用户反复重建会话。
 * 20 是预算不是目标：任何缓存都必须有上限，无界即泄漏（见内存优化审计 07）。
 */
const MAX_PROJECT_SESSIONS = 20;

/**
 * monaco 懒加载的就绪 promise。
 *
 * 存在模块级而非 React state：promise 不是可渲染数据，放进 state 只会诱发多余渲染，
 * 且闭包捕获的旧 promise 会在主题切换后失效。模块级变量让 openFile / effect /
 * 对话框回调共享同一次加载，重复调用由 ensureMonacoReady 内部幂等收敛。
 */
let monacoReadyPromise: Promise<void> | null = null;

/**
 * 触发 monaco 加载（若尚未开始）并返回就绪 promise。
 *
 * themeId 每次都转发给 ensureMonacoReady：它内部幂等 —— 未加载时执行初始化，
 * 已加载但主题不同时返回「在原初始化之上串联主题注册」的新 promise。
 * 因此这里直接采用最新返回值：调用方 await 到的是
 * 「内核就绪 + 目标主题已注册并生效」的最终状态，而非更早的中间态。
 */
function loadMonaco(themeId?: number): Promise<void> {
  monacoReadyPromise = ensureMonacoReady(themeId);
  return monacoReadyPromise;
}

/** 每个打开文件一份编辑缓冲：切标签、保存都基于它 */
interface FileBuffer {
  content: string;
  language: string;
  dirty: boolean;
  /**
   * 磁盘内容在上次读取/写入时的哈希（fileWatch.contentHash）：
   * 区分「自己的未保存编辑」与「外部修改」的基线。空串 = 从未成功读过盘
   * （图片、读取失败的占位缓冲），检测时按未知基线保守处理。
   */
  diskHash: string;
  /**
   * 磁盘基线探针（与 diskHash 同一次读盘/写盘时记录的 `(len, mtime)`，见
   * fileWatch.probeFastPath）：外部变更检测先比探针，一致则零读盘直接判定。
   * undefined = 基线未知（旧会话恢复/图片占位），退化为全量读。
   */
  diskLen?: number | null;
  diskMtimeMs?: number | null;
  /** 已检测到外部修改且本地有未保存编辑，等待用户在横幅上表态 */
  externalChanged: boolean;
  /** 读取失败原因（权限、非 UTF-8 等） */
  error: string | null;
  /** 打开/保存时使用的文本编码；缺省 UTF-8（旧快照/占位缓冲）。 */
  encoding?: string;
  /** 读取失败的后端协议码（`ERR:<code>|...` 的 code），无错误或非协议错误时为 null。
   *  存在理由：大文件拒绝（file_too_large）要在错误面板上给出「在文件管理器中
   *  显示 / 仍要打开」两个动作，靠解析 message 文案判断既脆弱又依赖语言。 */
  errorCode?: string | null;
  /**
   * 分页只读模式（P2 大文件模式，见 PagedFileViewer）：1MB 以上文件默认不进
   * Monaco，只持有一页文本。dirty 恒 false（只读无保存），diskHash 恒 ''——
   * 外部变更检测走探针-only 分支，绝不为刷新全量读盘。
   */
  paged?: {
    totalLines: number;
    page: number;
    pageSize: number;
    fileLen: number;
  } | null;
}

/**
 * 分页只读模式的体积阈值：超过即默认分页打开（不进 Monaco）。
 * 取值理由：1MB 文本 Monaco 尚可但 tokenization 已明显升温；
 * 8MB 是 read_file_content 软上限（超了直接拒绝），分页段落在两者之间。
 * 用户随时可"完整打开"逃生（自担风险）。
 */
const PAGED_VIEW_THRESHOLD: number = 1024 * 1024;
/** 分页每页行数：与 read_file_range 默认 200 对齐 */
const PAGED_PAGE_LINES = 200;

/** 取父目录路径（根路径返回自身）等纯函数与错误文本化已抽出：
 *  services/pathUtils（parentOfPath/baseNameOf/toWorkspaceAbsPath）、
 *  services/errorText（rawErrText/errText/errCode）、
 *  hooks/useStableCallback —— App 只保留编排，行为不变。 */

/** 窗口内多项目切换时的项目会话快照：切走保存、切回恢复（终端会话由 GhosttyTerminal 按项目保活） */
interface ProjectSessionSnapshot {
  openFilePaths: string[];
  activeFilePath: string;
  buffers: Record<string, FileBuffer>;
  fileTree: FileNode[];
  /** 工作区根是否为单个文件（单文件工作区不显示可折叠的项目根节点） */
  workspaceIsFile: boolean;
  treeExpanded: Set<string>;
  activeTab: 'editor' | 'diff' | 'compare-branches';
  /** Diff 标签是否存在（与「是否为当前标签」分离：切去看别的文件不该让 Diff 消失） */
  diffOpen: boolean;
  markdownViewModes: Record<string, MarkdownViewMode>;
  markdownScrollRatios: Record<string, number>;
  diffCommit: string | null;
  diffAgainstRef: string | null;
  diffCompareCommits: { from: string; to: string } | null;
  diffSelectedCommits: string[] | null;
  diffStaged: boolean;
  diffFilePath: string;
  fsCompareFiles: { oldPath: string; newPath: string } | null;
}

/** toWorkspaceAbsPath 已抽到 services/pathUtils —— App 只保留编排，行为不变。 */

/** 在已加载的树里按路径找节点；用于判断链上目录是否已经有 children，避免重复读盘。 */
function findTreeNode(nodes: FileNode[], path: string): FileNode | null {
  for (const node of nodes) {
    if (node.path === path) return node;
    if (node.isDirectory && Array.isArray(node.children)) {
      const hit = findTreeNode(node.children, path);
      if (hit) return hit;
    }
  }
  return null;
}

export const App: React.FC = () => {
  // State
  const [activeDrawer, setActiveDrawer] = useState<ActiveDrawer>('project');
  const [activeTab, setActiveTab] = useState<'editor' | 'diff' | 'compare-branches'>('editor');
  /**
   * Diff 页是否打开（标签栏里是否保留 Diff 标签）。
   * 与 activeTab 分离：打开别的文件只切换激活标签，Diff 标签与它的比较目标
   * 都留在原地，点回标签即可继续看（对齐 PyCharm 的 diff 标签语义）。
   */
  const [diffOpen, setDiffOpen] = useState(false);
  /** Diff 视口专属的比对文件路径（与 activeFilePath 解耦，防止文件树点击串改比对目标） */
  const [diffFilePath, setDiffFilePath] = useState('');
  /** 打开 diff 时的来源：工作区文件 vs 某个提交 */
  const [diffStaged, setDiffStaged] = useState(false);
  const [diffCommit, setDiffCommit] = useState<string | null>(null);
  const [compareSession, setCompareSession] = useState<{
    fromCommit: GitCommit;
    toCommit: GitCommit;
  } | null>(null);
  /** Compare Branches：独立于 Compare Versions 抽屉的两条日志会话 */
  const [compareBranches, setCompareBranches] = useState<{
    left: GitBranch;
    right: GitBranch;
  } | null>(null);
  const [diffCompareCommits, setDiffCompareCommits] = useState<{
    from: string;
    to: string;
  } | null>(null);
  /** 底部日志多选提交的合并 diff（只包含显式选中的提交）。 */
  const [diffSelectedCommits, setDiffSelectedCommits] = useState<string[] | null>(null);
  const [activeBottomTool, setActiveBottomTool] = useState<BottomTool>(null);
  /** 空串 = 未打开项目（顶栏显示 Welcome，分支按钮隐藏）；打开后由 refreshGit 写入 */
  const [currentBranch, setCurrentBranch] = useState('');
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [findInFilesOpen, setFindInFilesOpen] = useState(false);
  const [findInFilesScope, setFindInFilesScope] = useState('');
  const [cloneOpen, setCloneOpen] = useState(false);
  const [gitToolsOpen, setGitToolsOpen] = useState(false);
  // setter 从未被调用：平台在挂载时按 UA 确定，之后不变。保留 useState 形态
  // （首值延迟初始化只跑一次），去掉无人调用的 setter。
  const [platform] = useState<'mac' | 'win'>(() =>
    // 之前恒为 'mac' 且 setPlatform 从未调用，Windows 构建没有窗口控制按钮；按 UA 判定
    /Win/i.test(navigator.userAgent) ? 'win' : 'mac'
  );
  const [workspacePath, setWorkspacePath] = useState<string>('');
  /** 当前工作区根是单个文件（而不是目录）：文件树不渲染可折叠的项目根节点 */
  const [workspaceIsFile, setWorkspaceIsFile] = useState(false);
  useEffect(() => {
    monacoLspBridge.setWorkspaceRoot(workspacePath);
  }, [workspacePath]);
  const [recentWorkspaces, setRecentWorkspaces] = useState<string[]>([]);
  // 同步读持久化设置：词典在 main.tsx 已按同一份设置初始化，首帧
  // 不能再从 DEFAULT（英文/默认主题）起跳，否则中文用户会看到闪帧。
  const [settings, setSettings] = useState<AppSettings>(() => loadSettings());
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [engineInfo, setEngineInfo] = useState<{ engineVersion?: string; ghosttyVersion?: string; themeCount?: number }>({});
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const [updateDialogState, setUpdateDialogState] = useState<UpdateDialogState>('checking');
  const [pendingUpdateInfo, setPendingUpdateInfo] = useState<UpdateInfo | null>(null);
  /** 后台检查发现的新版本：顶栏常驻入口；底部 toast 关闭后仍保留。 */
  const [availableUpdate, setAvailableUpdate] = useState<UpdateInfo | null>(null);
  const [backgroundUpdateToast, setBackgroundUpdateToast] = useState<UpdateInfo | null>(null);
  /**
   * monaco 是否已就绪。只作为渲染门控信号：未就绪时编辑器位置渲染骨架占位，
   * 避免 <Editor> 在 loader.config 之前 mount —— 那会触发 CDN 回退、离线卡 Loading。
   */
  const [monacoReady, setMonacoReady] = useState(false);

  // Git data
  const [commits, setCommits] = useState<GitCommit[]>([]);
  const [branches, setBranches] = useState<GitBranch[]>([]);
  const [tags, setTags] = useState<GitTag[]>([]);
  /** Git Log 工具栏：排序/First Parent/No Merges（透传 git_log） */
  const [logSortMode, setLogSortMode] = useState<'date' | 'topo'>('date');
  const [logFirstParent, setLogFirstParent] = useState(false);
  const [logNoMerges, setLogNoMerges] = useState(false);
  const [changedFiles, setChangedFiles] = useState<GitChangedFile[]>([]);
  /** sequencer 状态（合并/拣选/变基在途）：随 refreshGit 更新，失败为 null */
  const [sequencer, setSequencer] = useState<SequencerState | null>(null);
  /**
   * 外部驱动的提交跳转请求（Go to Parent/Child 菜单）：面板消费后统一完成
   * 选中 + 滚动 + 闪烁（GitBottomPanel.jumpToCommit）。seq 保证同 hash 连点
   * 也能重复触发；null = 无请求。
   *
   * 教训（2026-10-14，.audit/11）：这里曾经是个"只写不读"的 ref（美其名曰
   * 省一次重渲染），而面板选中态完全自管，App 的 querySelector 又够不着
   * 虚拟化列表未渲染的行 —— 跳转只剩"可能发生的滚动"，目标在屏内时体感为零。
   */
  const [focusCommit, setFocusCommit] = useState<{ hash: string; seq: number } | null>(null);
  const focusSeqRef = useRef(0);
  /** 当前目录是否为 git 仓库；false 时面板提供 git init 入口 */
  const [isRepo, setIsRepo] = useState(true);
  /**
   * Git 仓库根。项目可以是仓库的子目录：此时 git status 的路径是仓库根
   * 相对路径，所有 Git 子进程也必须以仓库根为 -C，否则 status/diff/commit
   * 的 pathspec 会按子目录解析，出现“有变更但 diff 为空 / git add 找不到”。
   * 文件树仍以 workspacePath 为根，二者不能混用。
   */
  const [gitRepoRoot, setGitRepoRoot] = useState('');
  const gitRepoRootRef = useRef('');
  const [hasMoreCommits, setHasMoreCommits] = useState(false);
  const [logLoadingMore, setLogLoadingMore] = useState(false);
  // Push / Update Project 对话框
  const [pushDialogOpen, setPushDialogOpen] = useState(false);
  const [pullDialogOpen, setPullDialogOpen] = useState(false);
  /** 远程管理对话框（Git 菜单 Manage Remotes… / Push 对话框 Manage…） */
  const [remotesDialogOpen, setRemotesDialogOpen] = useState(false);
  /** 远程增删改后递增：Push 对话框据此重读 remote 下拉 */
  const [remotesNonce, setRemotesNonce] = useState(0);
  // 分支右键菜单与分支比较
  const [branchMenu, setBranchMenu] = useState<BranchMenuState | null>(null);
  const [compareRef, setCompareRef] = useState<string | null>(null);
  // 提交右键菜单与 Reset 对话框
  const [commitMenu, setCommitMenu] = useState<CommitMenuState | null>(null);
  const [resetTarget, setResetTarget] = useState<GitCommit | null>(null);
  const [squashCommitsTarget, setSquashCommitsTarget] = useState<GitCommit[] | null>(null);
  /** 日志分支过滤：'' = 全部分支（对齐 PyCharm 默认 All Branches），否则只看该分支 */
  const [logBranch, setLogBranch] = useState('');
  /** 日志路径过滤：只看改动过该路径的提交 */
  const [logPath, setLogPath] = useState('');
  const [logUser, setLogUser] = useState('');
  const [logDate, setLogDate] = useState<'all' | 'today' | 'week' | 'month'>('all');
  // Diff 第三种模式：某提交版本 ↔ 工作区（Compare with Local）
  const [diffAgainstRef, setDiffAgainstRef] = useState<string | null>(null);
  // 分支比较对话框的工作区模式 / Pull 对话框预选策略
  const [compareWorktree, setCompareWorktree] = useState(false);
  const [pullDialogRebase, setPullDialogRebase] = useState(false);
  // 文件历史 / 行级 blame
  const [historyFile, setHistoryFile] = useState<string | null>(null);
  const [blameOn, setBlameOn] = useState(false);
  // 项目树右键菜单 / 应用级文件剪贴板（Cut/Copy → Paste）/ 新建重命名删除对话框
  const [treeMenu, setTreeMenu] = useState<TreeMenuState | null>(null);
  /**
   * 文件树多选集合。空数组 = 单文件语义（右键菜单只作用于被点行）。
   * 来源：ProjectDrawer 的 Cmd/Ctrl 切中与 Shift 范围选择。
   *  值在 App 内从未被读取（批量操作走 ProjectDrawer 内部）：保留 setter 供子组件
   *  回调，只去掉无人读取的值位，避免每次选区变化都触发全量重渲染。
   */
  const [, setTreeSelection] = useState<string[]>([]);
  const [fsClipboard, setFsClipboard] = useState<{ path: string; cut: boolean } | null>(null);
  const [nameDialog, setNameDialog] = useState<NameDialogState | null>(null);
  /** 通用文本输入框（替代 window.prompt —— 抽离为 hooks/useTextPrompt） */
  const { textPromptReq, appPrompt, resolveTextPrompt } = useTextPrompt();
  // Compare With…：任意两个磁盘文件的内容对比
  const [fsCompareFiles, setFsCompareFiles] = useState<{ oldPath: string; newPath: string } | null>(null);
  // Stash 对话框
  const [stashDialogOpen, setStashDialogOpen] = useState(false);
  const [unstashDialogOpen, setUnstashDialogOpen] = useState(false);
  // Shelve / 交互式变基 / 冲突解决
  const [shelveDialogOpen, setShelveDialogOpen] = useState(false);
  /** 本地变更列表（changelist）：文件分组与 Shelve/Unshelve 的目标 */
  const [changelists, setChangelists] = useState<Changelists>({ lists: ['Changes'], files: {} });
  /** 递增触发已打开 diff 重新拉取（提交/回滚等改写工作区后） */
  const [diffRefreshKey, setDiffRefreshKey] = useState(0);
  const [rebaseBase, setRebaseBase] = useState<GitCommit | null>(null);
  const [conflictDialogOpen, setConflictDialogOpen] = useState(false);
  const [isConflictModalActive, setIsConflictModalActive] = useState(false);
  const conflictCount = changedFiles.filter(f => f.status === 'C').length;
  /** 收藏分支（hook 化：hooks/useFavoriteBranches，含持久化与上限） */
  const { favoriteBranches, toggleFavoriteBranch } = useFavoriteBranches();

  // 多 Tab 与工作区状态
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  /** 追加查看的外部目录根：不属于当前项目，仅提供浏览，可关闭 */
  const [externalRoots, setExternalRoots] = useState<FileNode[]>([]);
  /** 文件树展开的目录集合（提升到 App：定位 / 展开全部 / 折叠全部都要操作） */
  const [treeExpanded, setTreeExpanded] = useState<Set<string>>(new Set());
  /** 文件树/展开集合的渲染期镜像：定位时判断链上目录是否已加载，避免重复读盘与整树重建 */
  const fileTreeRef = useRef(fileTree);
  fileTreeRef.current = fileTree;
  /**
   * 最近一次「点击编辑器标签」的记录。
   *
   * 自动定位只在标签点击时触发：文件树点击本身已经选中了那一行，再定位一次
   * 只会重复展开/滚动并造成闪烁。记录路径 + 时间，避免过期的点击误触发。
   */
  const tabActivationRef = useRef<{ path: string; at: number } | null>(null);
  /** 定位函数的最新引用：selectTabStable 需要直接调用，避免 activeFilePath 不变时不触发 effect */
  const locateActiveRef = useRef<(quiet?: boolean) => void>(() => {});
  /**
   * 文件树自动定位开关（对齐 PyCharm Always Select Opened File，默认开）：
   * 开时切标签/开文件自动在树中展开并滚动到对应行。localStorage 持久化，
   * 从未设置过视为开 —— 与 PyCharm 默认行为一致。
   */
  const [autoLocateTree, setAutoLocateTree] = useState(() => {
    try {
      return localStorage.getItem('ide_tree_autolocate') !== '0';
    } catch {
      return true;
    }
  });
  const toggleAutoLocateTree = useCallback(() => {
    setAutoLocateTree(prev => {
      const next = !prev;
      try {
        localStorage.setItem('ide_tree_autolocate', next ? '1' : '0');
      } catch {
        /* 无存储环境（隐私模式等）仅本次生效 */
      }
      if (!next) {
        // 关开关：在途的异步定位就地作废 + 撤掉待执行的滚动/闪烁定时器，
        // 否则"关了但还是定位一次"。locateEpochRef/clearLocateFlashTimers 在
        // 后面声明，回调执行时（事件触发）早已初始化，无 TDZ 问题。
        locateEpochRef.current++;
        clearLocateFlashTimers();
      }
      return next;
    });
  }, []);
  const [openFilePaths, setOpenFilePaths] = useState<string[]>([]);
  const [activeFilePath, setActiveFilePath] = useState('');
  /** Markdown 文件的查看模式与滚动状态（抽离至 hooks/useMarkdownViewState） */
  const {
    markdownViewModes,
    setMarkdownViewModes,
    markdownScrollRatios,
    setMarkdownScrollRatios,
    activeMarkdownMode,
    activeMarkdownScrollRatio,
    handleMarkdownScroll,
    markdownModeChangeStable,
    markdownScrollChangeStable,
  } = useMarkdownViewState(activeFilePath);
  // ============ 窗口内多项目切换（对齐 PyCharm）：切走快照、切回恢复 ============
  const projectSessionsRef = useRef<Record<string, ProjectSessionSnapshot>>({});
  /** 终端组件句柄：关闭项目前用它确认该项目是否还有命令在跑。 */
  const terminalRef = useRef<GhosttyTerminalHandle>(null);
  /** 有常驻终端的项目：快照淘汰时 pin 住，避免打开新项目顺手 kill 运行中的任务。 */
  const liveTerminalProjectsRef = useRef<Set<string>>(new Set());
  /** 本窗口打开项目的活跃顺序（最近在前）：关闭当前项目时据此回到上一个仍打开的项目 */
  const openOrderRef = useRef<string[]>([]);
  /** 快照淘汰的显式通知源：projectSessionsRef 不进依赖比较，删了要靠它触发列表重算 */
  const [sessionPruneTick, setSessionPruneTick] = useState(0);
  /** GhosttyTerminal 报告有终端的项目；useStableCallback 避免列表抖动引发重渲染。 */
  const onLiveProjectsChangeStable = useStableCallback((projects: string[]) => {
    liveTerminalProjectsRef.current = new Set(projects);
  });
  /** 控制台进程占用变化：只存命令名列表（空数组 = 无进程在跑），驱动工具条角标 */
  const onTerminalBusyChangeStable = useStableCallback((busy: boolean, commands: string[]) => {
    setTerminalRunning(busy ? commands : []);
  });
  /** 控制台活动会话 cwd 变化：并入全局状态栏 */
  const onTerminalCwdChangeStable = useStableCallback((pwd: string | null) => {
    setTerminalCwd(pwd);
  });
  /** 本窗口打开（可切换）的项目；当前项目始终在列 */
  const [sessionProjects, setSessionProjects] = useState<{ path: string; name: string }[]>([]);
  /** 其它窗口打开的项目（点击聚焦对应窗口） */
  const [otherWindows, setOtherWindows] = useState<{ label: string; title: string; path: string }[]>([]);
  const [gotoLine, setGotoLine] = useState<number | null>(null);
  /** 工作区载入请求序号：快速切换项目时丢弃过期响应 */
  const workspaceSeqRef = useRef(0);
  /**
   * Git 刷新请求的序号与在途标记。
   *
   * 触发源很多：工作区载入、窗口聚焦、每次按键防抖、以及十几处写操作
   * 后的手动刷新。这些调用会互相重叠，同一时刻可能有多个
   * git_log/git_branches/git_status 在跑。后果有两个：
   *
   * 1. 过期响应后到，把新数据覆盖成旧数据（切分支后闪回旧分支名）；
   * 2. 多个 git 子进程同时抢同一个仓库的 index.lock。
   *
   * `seq` 记最新一次请求的序号，响应回来时只接受序号最大的那次；
   * `inFlight` 保证同一时刻只有一轮真正的读取，期间到来的请求折叠成
   * 一次「待重跑」，在当前轮结束后补上（补跑走 `refreshGitRef` 最新闭包，
   * 避免旧 workspacePath 的 refreshGit 把数据写回已切换的项目）。
   */
  const gitSeqRef = useRef(0);
  const gitInFlightRef = useRef(false);
  const gitPendingRef = useRef(false);
  /** 始终指向最近一次创建的 refreshGit：在途 finally 补跑时不能用旧闭包 */
  const refreshGitRef = useRef<(() => void) | null>(null);
  /** refreshGit 在途（读操作）：与 gitBusy（写操作）分开，避免刷新时禁用 Commit 按钮 */
  const [gitRefreshing, setGitRefreshing] = useState(false);
  /** 切换/重开项目时递增：即使 workspacePath 未变也要强制重跑 refreshGit effect */
  const [gitRefreshTick, setGitRefreshTick] = useState(0);
  /** 编辑缓冲（按绝对路径索引）：切标签不丢改动，保存写回磁盘 */
  const buffersRef = useRef<Record<string, FileBuffer>>({});
  const [buffers, setBuffers] = useState<Record<string, FileBuffer>>({});
  /**
   * 编辑计数器：每次编辑器内容变化时递增，只用于让「停顿后自动保存」的定时器续期。
   * 脏路径集合在同一文件里继续打字时不变，自动保存 effect 因此不会重跑 —— 不递增
   * 这个值，语义就退化为「首次变脏后写一次」。与 setBuffers 在同一次事件里批量更新，
   * 不额外产生渲染。
   */
  const [editTick, setEditTick] = useState(0);
  /**
   * 写盘在途的路径集合。自家 write_file_content 也会触发 watcher 事件，
   * 在基线推进前处理该事件会把保存回声误判成外部修改 —— 检测时先避开在途保存。
   */
  const savingRef = useRef<Set<string>>(new Set());
  /**
   * 打开中的标签与当前文件的 ref 镜像。
   * 脏缓冲淘汰需要判断「这个路径是不是还开着」，但不能为此给 putBuffer 增加
   * openFilePaths / activeFilePath 依赖 —— 那会让 putBuffer 及其一串下游回调的身份
   * 随每次标签切换反复失效重建。用 ref 读最新值（与 themeIdRef 同一思路）。
   */
  const openFilePathsRef = useRef<string[]>([]);
  openFilePathsRef.current = openFilePaths;
  const activeFilePathRef = useRef('');
  activeFilePathRef.current = activeFilePath;
  /**
   * 当前工作区路径的 ref 镜像（与上面两个同一思路）。loadWorkspace 要读它判断
   * 「当前项目是否已存过会话快照」，但不能把它放进 loadWorkspace 的依赖 ——
   * 挂载时的「恢复上次项目」effect 依赖 loadWorkspace，路径一变就会重跑，
   * 变成每切换一次项目都重新载入（连带清空刚恢复的标签）。
   */
  const workspacePathRef = useRef('');
  workspacePathRef.current = workspacePath;
  /**
   * 单文件工作区标记的 ref 镜像：LSP 的 root 必须在同步路径里读到最新值，
   * 而 resolveLspRoot 不能依赖 workspaceIsFile state（否则身份抖动、effect 重建）。
   */
  const workspaceIsFileRef = useRef(false);
  workspaceIsFileRef.current = workspaceIsFile;
  /**
   * 渲染期的工作区代次快照。切换项目的处理器会同步递增 `workspaceSeqRef`，
   * 而旧渲染的 effect 可能在下一帧前才执行 —— 用 ref 当前值判断会让旧项目
   * 的 LSP 重启误判为「仍是当前」。把渲染期的值固化下来，effect 里比较它。
   */
  const workspaceSeq = workspaceSeqRef.current;
  /** 当前 Git 操作根：仓库根优先，非仓库回落到工作区。 */
  const resolveGitRoot = useCallback(
    () => gitRepoRootRef.current || workspacePathRef.current,
    []
  );
  /**
   * LSP 根：Git 仓库根优先；非仓库的单文件工作区必须取父目录。
   *
   * 直接把文件路径交给 `lsp_server_start` 会让 Rust 侧 `current_dir(file)` 失败
   * （Not a directory），gopls/pyright 等全部起不来。终端 cwd 已经做过同样的
   * 父目录修正，LSP 不能漏。
   */
  const resolveLspRoot = useCallback(() => {
    return lspRootFor(workspacePathRef.current, workspaceIsFileRef.current, gitRepoRootRef.current);
  }, []);

  // 工作区切换 / 快照恢复后重启 LSP。
  //
  // resetLsp 在切换的同步路径里已递增 generation 并把 stop 排进队列；这里
  // 的 ensure 会排在 stop 之后执行。isCurrent 用渲染期代次 + 当前路径双重
  // 校验，快速 A→B→A 时旧代次的启动不会串到新项目。
  useEffect(() => {
    if (!workspacePath) return;
    const isCurrent = () =>
      workspaceSeqRef.current === workspaceSeq &&
      workspacePathRef.current === workspacePath;
    const languages = new Set(openFilePaths.map(languageOf));
    for (const language of languages) {
      void ensureLspForLanguage(language, resolveLspRoot(), isCurrent);
    }
  }, [workspacePath, workspaceSeq, openFilePaths, resolveLspRoot]);
  /** 脏缓冲淘汰的 in-flight 标记：连续输入会让 putBuffer 反复触发，避免并发淘汰互相踩踏 */
  const dirtyEvictionRunning = useRef(false);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);

  /**
   * 状态栏文字的自动清除定时器。
   *
   * 40+ 处节点原先直接 `setStatusMessage(...)` 且从不置回 null，状态栏于是永远
   * 停留在最后一条消息上：几小时后用户仍看到「已删除 a.ts」，不知道该信息是否
   * 仍然有效。改为统一的 showStatus 收口，定时过期后清空 —— 通知类消息的意义
   * 只存在于「刚发生」这个窗口内。
   *
   * 与 gitToastTimerRef 同构：单一 pending 句柄，新消息取消上一个待清除并重新
   * 计时（否则连续操作时先到的定时器会把后到的消息提前清掉）。因此引用必须稳定，
   * 否则所有依赖它的 useCallback 会随之失效——这里 deps 为 []。
   *
   * 声明紧贴 statusMessage：从 loadWorkspace（workspace 授权失败）开始的调用点
   * 都在它之前，把实现放在文件后半段会让 TypeScript 报「used before its
   * declaration」，而把它沉到更前面又会与 gitToastTimerRef 那段注释失去对照。
   */
  const statusToastTimerRef = useRef<number | null>(null);

  const showStatus = useCallback((text: string, ms = 5000) => {
    clearTimeout(statusToastTimerRef.current ?? undefined);
    setStatusMessage(text);
    statusToastTimerRef.current = window.setTimeout(() => {
      statusToastTimerRef.current = null;
      setStatusMessage(null);
    }, ms);
  }, []);

  // 卸载时清掉待执行的清除回调，防止对已卸载组件 setState
  useEffect(() => () => {
    clearTimeout(statusToastTimerRef.current ?? undefined);
  }, []);

  /**
   * 光标位置由模块级 store 持有，不进 App state。
   *
   * App.tsx 是 3800 行的单壳层，cursor 每按一次方向键就变一次；
   * 若它是 App state，整棵树（TitleBar 的分支过滤排序、GitBottomPanel 的
   * filteredCommits、CommitDrawer 的目录树）都会跟着重跑 —— 光标移动是
   * 编辑器里最高频的事件之一，这个代价不可接受。改成外部 store 后只有
   * 订阅了它的 `<CursorIndicator>` 会重渲染。
   */
  const cursor = useCursorIndicator();
  const [tabSize, setTabSize] = useState<number | null>(null);
  /** 提交信息历史（hook 化：hooks/useCommitMessageHistory，含持久化与清空入口） */
  const { commitMessages, setCommitMessages, rememberCommitMessage, clearCommitMessages } = useCommitMessageHistory();

  const dropBuffer = useCallback((path: string) => {
    const next = { ...buffersRef.current };
    delete next[path];
    buffersRef.current = next;
    setBuffers(next);
  }, []);

  /**
   * 脏缓冲 LRU 淘汰：超出 MAX_DIRTY_BUFFERS 时，把最久未使用的脏缓冲写盘后回收。
   *
   * 由 putBuffer 触发（所有缓冲写入的唯一收口），因此覆盖打开文件、编辑、保存、
   * 自动保存、重命名迁移每条路径。安全规则：必须写盘成功才 dropBuffer，
   * 写盘失败就保留该缓冲并提示用户手动保存 —— 绝不能悄悄丢改动。
   *
   * 淘汰优先级：先回收标签栏里已看不到的孤立缓冲，再动仍开着的标签。
   * 开着的标签被回收后重新点击会从磁盘读回，内容不丢，但脏标记会消失、
   * 用户会以为改动被吞了，所以尽量留到最后。
   * 当前激活文件永不回收：它的内容正由 EditorView 渲染，回收会让编辑器瞬间清空。
   */
  const evictDirtyBuffers = useCallback(async () => {
    // 连续输入会让 putBuffer 反复触发，串行执行避免两次淘汰同时写同一批文件
    if (dirtyEvictionRunning.current) return;
    dirtyEvictionRunning.current = true;
    try {
      // 对象键序即「最近使用」顺序（putBuffer 先删再插），所以每档内部从头到尾就是 LRU 队列
      const dirty = Object.keys(buffersRef.current).filter(p => buffersRef.current[p].dirty);
      const open = new Set(openFilePathsRef.current);
      const orphaned = dirty.filter(p => !open.has(p));
      const visible = dirty.filter(p => open.has(p));
      const queue = [...orphaned, ...visible];
      // 只淘汰固定数量的候选：循环里再重新收集列表可能越收越长（写盘期间
      // 用户还在输入），用本轮快照才能保证循环收敛
      for (const victim of queue.slice(0, queue.length - MAX_DIRTY_BUFFERS)) {
        if (victim === activeFilePathRef.current) continue;
        const buffer = buffersRef.current[victim];
        if (!buffer || !shouldAutoSave(buffer)) continue;
        try {
          await invokeCommand('write_file_text', { path: victim, content: buffer.content, encoding: buffer.encoding ?? 'UTF-8' });
          // 写盘期间用户可能又改了同一文件：内容变了说明磁盘上是旧版本，只能保留
          const current = buffersRef.current[victim];
          if (!current?.dirty || current.content !== buffer.content) continue;
          dropBuffer(victim);
          // 已落盘且缓冲被真正回收：孤立缓冲（标签栏里看不到）顺带释放 monaco
          // model；仍开着的标签保留 model —— 内容随时会被重新读回，不必重建。
          if (!open.has(victim)) disposeModelForPath(victim);
        } catch (err) {
          showStatus(
            t().git.autoRecoverFailed(baseNameOf(victim), errText(err))
          );
          // 更早的缓冲同理写不过去，继续尝试只会重复失败刷屏，直接停在本轮
          return;
        }
      }
    } finally {
      dirtyEvictionRunning.current = false;
    }
  }, [dropBuffer]);

  const putBuffer = useCallback((path: string, buffer: FileBuffer) => {
    // 先删再插：让对象键序表达「最近使用」顺序，脏缓冲超限时据此做 LRU 淘汰
    const next = { ...buffersRef.current };
    delete next[path];
    next[path] = buffer;
    buffersRef.current = next;
    setBuffers(next);
    void evictDirtyBuffers();
  }, [evictDirtyBuffers]);

  const activeBuffer = activeFilePath ? buffers[activeFilePath] : undefined;
  const codeContent = activeBuffer?.content ?? '';
  const editorLanguage = activeBuffer?.language ?? 'plaintext';
  /**
   * 脏文件路径数组。
   *
   * 必须 useMemo：它是 EditorTabBar（memo 组件）的 `dirtyFiles` prop。
   * 每次渲染新建数组会让浅比较必不等，memo 形同虚设 —— 击键时
   * putBuffer → setBuffers → dirtyPaths 新数组 → tab 栏连带重建。
   * 依赖用 buffers 对象本身（状态更新时才是新引用）。
   */
  const dirtyPaths = useMemo(
    () => Object.keys(buffers).filter(path => buffers[path].dirty),
    [buffers]
  );

  /**
   * 当前生效的应用主题 id（themeId 0 表示跟随界面变体）。
   * 用 ref 读取而非直接读 settings：openFile / requestMonaco 的身份因此不随
   * settings 变化而改变，避免 useCallback 依赖链反复失效。
   */
  const themeIdRef = useRef(1);
  themeIdRef.current = settings.themeId === 0 ? settings.uiVariant : settings.themeId;

  /**
   * 请求加载 monaco 并登记就绪回调。
   *
   * 预取场景（openFile、打开冲突对话框）都是 fire-and-forget，所以这里统一吞掉
   * 失败分支：加载失败时编辑器位置由 render 门控兜底（保持占位），
   * 同时不会在控制台冒出未处理的 promise rejection。
   */
  const requestMonaco = useCallback((themeId?: number) => {
    loadMonaco(themeId).then(
      () => setMonacoReady(true),
      () => setMonacoReady(false)
    );
  }, []);

  /**
   * 「外部删除 → 关闭标签页」的前向引用。
   *
   * closeFilesUnderPaths 定义在几百行之后（它依赖 releaseBuffers 与 openFile，
   * 而那两个又依赖本函数所在的 checkExternalChange 链），直接引用会 TDZ。
   * 用 ref 转发：实现仍是那一份，这里只是提前拿到可调用的壳。
   */
  const closeFilesForExternalDeleteRef = useRef<(paths: string[]) => void>(() => {});
  const closeFilesForExternalDelete = useCallback(
    (paths: string[]) => closeFilesForExternalDeleteRef.current(paths),
    []
  );

  /**
   * 目录变更刷新用的前向引用。
   *
   * refreshTreeDir 定义在很后面（它依赖 updateFileTreeNode 与 workspacePath
   * 的时序），而目录 watcher 的注册必须在 watchedDirRoots 就绪时就建好。
   * refreshGitRef 已由 refreshGit 自己声明并绑定（它的 deps 链很长，
   * 见 1792 行的赋值），这里只补 refreshTreeDir 这一半。
   */
  const refreshTreeDirRef = useRef<(dir: string) => Promise<void>>(async () => {});

  /**
   * 处理一次外部变更检测：watcher 事件、重开标签校验、横幅「重载」共用。
   *
   * 决策在 fileWatch.resolveExternalChange（纯函数，可独立测试）；这里只负责
   * 读盘与落地。快路径在前：`file_probe` 只做一次 stat，`(len, mtime)` 与
   * 基线一致就直接判定"磁盘未变"（零读盘）——事件/重开校验绝大多数时候
   * 文件根本没变，全量读盘+哈希全是浪费。探针与基线对不上才全量读，
   * 此时判定仍是哈希精确的（快路径只跳过"没变"的情况，不替"变了"下结论）。
   * 读盘失败时用探针结果区分「文件被删」与「读不出来」：
   * 前者关掉标签页，后者仍按冲突交给人表态（探针本身失败才回退 fs_path_exists）。
   */
  const checkExternalChange = useCallback(
    async (path: string, opts?: { forceReload?: boolean; retry?: number }) => {
      const buffer = buffersRef.current[path];
      if (!buffer || buffer.language === 'image') return;
      const retry = opts?.retry ?? 0;
      if (savingRef.current.has(path) && retry < 20) {
        // 自己的保存在途：事件很可能是那次写盘的回声，等写完（基线推进）再判。
        // 20 次上限防写盘卡死时无限自我重排；之后的新事件会重新触发检测。
        window.setTimeout(
          () => void checkExternalChange(path, { ...opts, retry: retry + 1 }),
          250
        );
        return;
      }
      // P2 分页只读缓冲：探针-only 刷新，永远不全量读盘。
      // 脏恒为 false（只读），无冲突语义：对上三元组即无事；对不上重拉当前页；
      // 文件消失则关标签（只读缓冲不可能是用户唯一副本，无数据丢失风险）。
      if (buffer.paged) {
        let probe: FileProbe | null = null;
        try {
          probe = await invokeCommand<FileProbe>('file_probe', { path });
        } catch {
          return;
        }
        const current = buffersRef.current[path];
        if (!current?.paged) return;
        if (
          probe &&
          probe.exists &&
          probe.mtimeMs != null &&
          current.diskMtimeMs != null &&
          probe.len === current.diskLen &&
          probe.mtimeMs === current.diskMtimeMs
        ) {
          return; // 未变
        }
        if (probe && !probe.exists) {
          closeFilesForExternalDelete([path]);
          showStatus(t().status.externalDeletedTabClosed(baseNameOf(path)));
          return;
        }
        try {
          const startLine = (current.paged.page - 1) * current.paged.pageSize + 1;
          const r = await invokeCommand<FileRangeDto>('read_file_range', { path, startLine, maxLines: current.paged.pageSize });
          const latest = buffersRef.current[path];
          if (!latest?.paged) return;
          putBuffer(path, {
            ...latest,
            content: r.lines.join('\n'),
            diskLen: probe ? probe.len : latest.diskLen,
            diskMtimeMs: probe ? probe.mtimeMs : latest.diskMtimeMs,
            paged: { ...latest.paged, totalLines: r.totalLines, fileLen: probe ? probe.len : latest.paged.fileLen }
          });
        } catch {
          // 读页失败（权限/瞬时错误）：保留旧页，下次事件再试。不惊动用户。
        }
        return;
      }
      // 探针快路径（一次 stat，微秒级）：命中则连读盘都省了。
      let probe: FileProbe | null = null;
      try {
        probe = await invokeCommand<FileProbe>('file_probe', { path });
      } catch {
        probe = null;
      }
      if (probe && probeFastPath(buffer, probe) === 'unchanged') {
        // 磁盘与基线一致：等价于 resolveExternalChange 的 unchanged 分支。
        // 之前误标的 externalChanged 在此消解（磁盘回到了基线/本就是误报），
        // 否则无事可做——注意不能"更新基线"，基线本来就是对的。
        const current = buffersRef.current[path];
        if (current?.externalChanged) {
          putBuffer(path, { ...current, externalChanged: false });
        }
        return;
      }
      let disk: string | null = null;
      let diskEncoding = 'UTF-8';
      try {
        const read = await invokeCommand<{ text: string; encoding: string }>('read_file_text', { path });
        disk = read.text;
        diskEncoding = read.encoding;
      } catch {
        disk = null;
      }
      // 读不到时先问「还在不在」。磁盘上已经没这个文件 → 关闭标签页：
      // 继续显示会让用户以为文件还在，未保存的改动若之后再存盘，会把一个
      // 已删除的文件写回来（Finder 里看着像复活）。探测本身失败（权限、
      // 磁盘错误）时不关 tab，退回 resolveExternalChange 的 prompt 分支。
      if (disk === null) {
        // 探针已给出存在性时不再问一次 fs_path_exists（同口径，省一趟 IPC）；
        // 只有探针本身失败才回退旧路。
        // 探测三态：文件不在（关 tab）/ 在（走 prompt）/ 探测本身失败（也走 prompt）。
        // 把探测失败当成「不在」会在权限/磁盘错误时关掉一个干净标签 ——
        // 用户只是重命名了父目录、或磁盘瞬时不可读，却丢了刚打开的文件。
        let exists: boolean | null = probe ? probe.exists : null;
        if (exists === null) {
          try {
            exists = await invokeCommand<boolean>('fs_path_exists', { path });
          } catch {
            exists = null;
          }
        }
        if (exists === false) {
          const buf = buffersRef.current[path];
          // 有未保存改动时不关：那可能是用户唯一的副本，静默丢掉是数据丢失。
          // 改成提示，让人自己决定（保存成新文件 / 丢弃 / 手动处理）。
          if (buf?.dirty) {
            if (!buf.externalChanged) {
              putBuffer(path, { ...buf, externalChanged: true });
              showStatus(t().status.externalDeletedKeepTab(baseNameOf(path)));
            }
            return;
          }
          closeFilesForExternalDelete([path]);
          showStatus(t().status.externalDeletedTabClosed(baseNameOf(path)));
          return;
        }
      }
      const current = buffersRef.current[path];
      if (!current) return; // 检测期间标签已关闭、缓冲已释放
      const action = resolveExternalChange(current, disk, opts?.forceReload);
      if (action.kind === 'reload') {
        putBuffer(path, {
          ...current,
          content: action.content,
          encoding: diskEncoding,
          diskHash: contentHash(action.content),
          // 基线三元组一起推进：用触发这次读取的探针值（读盘与探针之间
          // 文件若又被改，下次检测探针对不上会再读一次，自愈，不误判）。
          diskLen: probe ? probe.len : null,
          diskMtimeMs: probe ? probe.mtimeMs : null,
          dirty: false,
          error: null,
          externalChanged: false
        });
      } else if (action.kind === 'prompt') {
        if (opts?.forceReload) {
          showStatus(t().status.reloadFailedNoDisk(baseNameOf(path)));
        } else if (!current.externalChanged) {
          putBuffer(path, { ...current, externalChanged: true });
        }
      } else if (action.kind === 'resolved') {
        putBuffer(path, { ...current, externalChanged: false });
      }
    },
    [putBuffer]
  );

  /**
   * 横幅「保留我的修改」：以当前磁盘状态为新基线后清除标记 ——
   * 我已知情并选择继续编辑；只有之后**新的**外部变更才重新提示。
   *
   * 快路径在前：探针与基线一致说明磁盘根本没动（误标/已恢复），直接清标记，
   * 省一次全量读——这正是横幅最常见的 dismissal 情形。
   */
  const dismissExternalChange = useCallback(async () => {
    const path = activeFilePath;
    const current = path ? buffersRef.current[path] : undefined;
    if (!path || !current) return;
    try {
      const probe = await invokeCommand<FileProbe>('file_probe', { path });
      if (probeFastPath(current, probe) === 'unchanged') {
        const latest = buffersRef.current[path];
        if (latest) putBuffer(path, { ...latest, externalChanged: false });
        return;
      }
    } catch {
      // 探针失败不影响主流程：继续全量读退化为旧行为
    }
    let diskHash = current.diskHash;
    let diskLen = current.diskLen;
    let diskMtimeMs = current.diskMtimeMs;
    try {
      const text = await invokeCommand<string>('read_file_content', { path });
      diskHash = contentHash(text);
      // 读完立刻探针：基线三元组描述的是刚读到的内容，越新鲜越准。
      // （与 checkExternalChange 的 reload 分支同一约定）
      try {
        const probe = await invokeCommand<FileProbe>('file_probe', { path });
        diskLen = probe.len;
        diskMtimeMs = probe.mtimeMs;
      } catch {
        diskLen = null;
        diskMtimeMs = null;
      }
    } catch {
      // 读不到就维持旧基线：宁可下次变更多提示一次，也不把失败当成「已确认」
      showStatus(t().status.externalChangedKeepLocal);
    }
    // 读盘期间用户可能已经切走、甚至改了内容：**以缓冲里的最新值回写**，
    // 只用 await 拿到的 diskHash 覆盖基线字段。直接铺开 current（await 前的
    // 快照）会把用户这期间的编辑回退掉。
    const latest = buffersRef.current[path];
    if (!latest) return; // 缓冲已释放（关标签/切项目）：无处可写
    putBuffer(path, { ...latest, diskHash, diskLen, diskMtimeMs, externalChanged: false });
  }, [activeFilePath, putBuffer]);

  /** 打开文件并加入多 Tab 列表；已有缓冲（含未保存改动）直接复用 */
  const openFile = useCallback(async (path: string, line?: number, opts?: { full?: boolean }) => {
    // 预取 monaco：文件树点击、标签切换、搜索结果、工作区自动打开都经过这里，
    // 在 await 读取文件内容的同时就开始下载编辑器内核，用户感知不到加载过程。
    // 主题用 ref 取最新值，避免 openFile 身份随 settings 抖动。
    requestMonaco(themeIdRef.current);
    // 记录「本次打开归属的工作区」：await 期间用户可能切到别的项目，
    // 那时旧响应必须丢弃。少了这道闸，A 项目的文件内容会被 putBuffer 进
    // B 项目（loadWorkspace 刚把 buffersRef 清空），该缓冲随后被判定为脏，
    // autoSaveAll 会把它写进 B 项目里同名的文件 —— 跨项目数据损坏。
    const seq = workspaceSeqRef.current;
    if (!isImageFile(path)) {
      void ensureLspForLanguage(languageOf(path), resolveLspRoot(), () => workspaceSeqRef.current === seq);
    }

    setOpenFilePaths(prev => (prev.includes(path) ? prev : [...prev, path]));
    setActiveFilePath(path);
    setActiveTab('editor');
    setGotoLine(line ?? null);

    if (buffersRef.current[path]) {
      // 缓冲可能是关标签时保留的脏内容或会话快照恢复的旧数据：磁盘可能已在
      // 缓冲闲置期间被人改过（那段时间它不在监听列表里），重开时补一次校验。
      if (!isImageFile(path)) void checkExternalChange(path);
      return;
    }

    if (isImageFile(path)) {
      putBuffer(path, { content: '', language: 'image', dirty: false, diskHash: '', externalChanged: false, error: null });
      return;
    }

    // P2 大文件模式：先探针定大小，超阈值进分页只读（不全量读、不进 Monaco）。
    // 探针失败不拦路：退化为下面的全量打开（守门仍在，8MB 以上照样拒绝）。
    // full=true 是分页横幅"完整打开"逃生口：跳过分页，走正常全量路径。
    try {
      const probe = await invokeCommand<FileProbe>('file_probe', { path });
      if (seq !== workspaceSeqRef.current) return;
      if (!opts?.full && probe && probe.isFile && probe.len > PAGED_VIEW_THRESHOLD) {
        try {
          const first = await invokeCommand<FileRangeDto>('read_file_range', { path, startLine: 1, maxLines: PAGED_PAGE_LINES });
          if (seq !== workspaceSeqRef.current) return;
          putBuffer(path, {
            content: first.lines.join('\n'),
            language: languageOf(path),
            dirty: false,
            diskHash: '',
            diskLen: probe.len,
            diskMtimeMs: probe.mtimeMs,
            externalChanged: false,
            error: null,
            errorCode: null,
            paged: { totalLines: first.totalLines, page: 1, pageSize: PAGED_PAGE_LINES, fileLen: probe.len }
          });
        } catch (err) {
          if (seq !== workspaceSeqRef.current) return;
          const message = errText(err);
          putBuffer(path, { content: '', language: languageOf(path), dirty: false, diskHash: '', externalChanged: false, error: message, errorCode: errCode(err) });
          showStatus(t().status.openFileFailed(message));
        }
        return;
      }
    } catch {
      /* 探针失败：继续全量打开 */
    }

    try {
      const { text, encoding } = await invokeCommand<{ text: string; encoding: string }>('read_file_text', { path });
      if (seq !== workspaceSeqRef.current) return;
      // 读完立刻探针：这次打开的基线三元组一次配齐，之后的重开校验/
      // watcher 事件直接走快路径。探针失败不拦打开（基线未知=全量读退化）。
      let diskLen: number | null = null;
      let diskMtimeMs: number | null = null;
      try {
        const probe = await invokeCommand<FileProbe>('file_probe', { path });
        diskLen = probe.len;
        diskMtimeMs = probe.mtimeMs;
      } catch {
        /* 保持未知基线 */
      }
      putBuffer(path, { content: text, language: languageOf(path), encoding, dirty: false, diskHash: contentHash(text), diskLen, diskMtimeMs, externalChanged: false, error: null, errorCode: null });
    } catch (err) {
      if (seq !== workspaceSeqRef.current) return;
      const message = errText(err);
      putBuffer(path, { content: '', language: languageOf(path), dirty: false, diskHash: '', externalChanged: false, error: message, errorCode: errCode(err) });
      showStatus(t().status.openFileFailed(message));
    }
  }, [putBuffer, requestMonaco, checkExternalChange, showStatus, resolveLspRoot]);

  /**
   * 大文件「仍要打开」：用户在 file_too_large 错误面板上显式确认后，
   * 带 force=true 重读（绕过 8MB 软上限，256MB 硬上限仍生效）。
   * 读回后走正常 putBuffer；失败则保留原错误面板并提示。
   */
  const forceOpenFile = useCallback(async (path: string) => {
    const seq = workspaceSeqRef.current;
    try {
      const { text, encoding } = await invokeCommand<{ text: string; encoding: string }>('read_file_text', { path, force: true });
      if (seq !== workspaceSeqRef.current) return;
      let diskLen: number | null = null;
      let diskMtimeMs: number | null = null;
      try {
        const probe = await invokeCommand<FileProbe>('file_probe', { path });
        diskLen = probe.len;
        diskMtimeMs = probe.mtimeMs;
      } catch {
        /* 保持未知基线 */
      }
      putBuffer(path, { content: text, language: languageOf(path), encoding, dirty: false, diskHash: contentHash(text), diskLen, diskMtimeMs, externalChanged: false, error: null, errorCode: null });
    } catch (err) {
      if (seq !== workspaceSeqRef.current) return;
      const message = errText(err);
      putBuffer(path, { content: '', language: languageOf(path), dirty: false, diskHash: '', externalChanged: false, error: message, errorCode: errCode(err) });
      showStatus(t().status.openFileFailed(message));
    }
  }, [putBuffer, showStatus]);

  /** 分页翻页在途（按钮禁用位，防连点攒 IPC） */
  const [pagedLoading, setPagedLoading] = useState(false);

  /**
   * 分页翻到某页（1-based）：只拉一页，跨 IPC 的永远是一页文本。
   * 范围钳制在这里做（调用方 PagedFileViewer 也钳制，双保险）。
   */
  const goToPagedPage = useCallback(async (path: string, page: number) => {
    const current = buffersRef.current[path];
    if (!current?.paged) return;
    const totalPages = Math.max(1, Math.ceil(current.paged.totalLines / current.paged.pageSize));
    const target = Math.min(totalPages, Math.max(1, Math.floor(page) || 1));
    if (target === current.paged.page) return;
    setPagedLoading(true);
    try {
      const startLine = (target - 1) * current.paged.pageSize + 1;
      const r = await invokeCommand<FileRangeDto>('read_file_range', { path, startLine, maxLines: current.paged.pageSize });
      const latest = buffersRef.current[path];
      if (!latest?.paged) return;
      putBuffer(path, {
        ...latest,
        content: r.lines.join('\n'),
        paged: { ...latest.paged, page: target, totalLines: r.totalLines }
      });
    } catch (err) {
      showStatus(t().status.openFileFailed(errText(err)));
    } finally {
      setPagedLoading(false);
    }
  }, [putBuffer, showStatus]);

  /**
   * 分页横幅"完整打开"逃生口：丢掉分页缓冲，走 openFile 全量路径
   * （full=true 跳过分页分支；8MB 以上仍会被软上限拒绝，自担风险）。
   */
  const openPagedFull = useCallback((path: string) => {
    dropBuffer(path);
    void openFile(path, undefined, { full: true });
  }, [dropBuffer, openFile]);

  /** 大文件错误面板「在文件管理器中显示」：走已授权的 fs_reveal。 */
  const revealActiveFile = useCallback(async (path: string) => {
    try {
      await invokeCommand('fs_reveal', { path });
    } catch (err) {
      showStatus(errText(err));
    }
  }, [showStatus]);

  /**
   * 把当前打开的文件集合推给 Rust 侧监听（fs_watch_files 全量替换）。
   * 只推非图片：图片缓冲不参与文本比对，事件对它没有意义。
   */
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    const paths = openFilePaths.filter(p => !isImageFile(p));
    void invokeCommand('fs_watch_files', { paths }).catch(() => {
      // 监听建立失败只是失去「自动感知外部修改」这一层，invokeCommand 已记日志，
      // 不值得为此打断用户
    });
  }, [openFilePaths]);

  /**
   * 监听项目根与追加的外部根（fs_watch_dirs）。
   *
   * 解决的问题：在 Finder 里删除/新增文件后，文件树不会自己更新，必须手动
   * 「Reload from Disk」。这里让工作区目录的任何变更都触发一次重读。
   *
   * 监听的根 = workspacePath + externalRoots（不含展开中的子目录 —— 那会让
   * 监听集合随展开动作频繁重建；子目录的变更先由父目录的事件覆盖到
   * read_directory 的 maxDepth=2，深层的再等用户展开时读到）。
   */
  const watchedDirRoots = useMemo(() => {
    const roots: string[] = [];
    if (workspacePath) roots.push(workspacePath);
    for (const r of externalRoots) roots.push(r.path);
    return roots;
  }, [workspacePath, externalRoots]);

  useEffect(() => {
    if (!isTauriEnvironment()) return;
    void invokeCommand('fs_watch_dirs', { paths: watchedDirRoots }).catch(() => {
      // 目录监听失败 = 退回改动前的手动刷新，不打断用户
    });
  }, [watchedDirRoots]);

  /**
   * 目录变更 → 重读目录刷新文件树。
   *
   * 惰性去重：同一目录在 debounce 窗口内只刷一次（Finder 删一个文件会连发
   * 多条事件）；未知路径（既不是项目根也不是外部根）忽略，避免把展开子目录
   * 的事件拿来重读项目根。
   */
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    let dispose: (() => void) | null = null;
    let tornDown = false;
    // debounceDirectoryChanges 的返回值是「取消 + 清定时器」：
    // 只调 dispose（事件退订）会留下一个 250ms 的 pending 定时器，
    // 而回调闭包持有 refreshGitRef / refreshTreeDirRef 与本次的
    // watchedDirRoots —— 与下面 onExternalChange 的效果同样处理。
    const cancel = debounceDirectoryChanges(dirPath => {
      // 项目根：换掉一级子树。外部根：只刷那一条。
      if (watchedDirRoots.includes(dirPath)) {
        void refreshTreeDirRef.current(dirPath);
      }
      // 目录内容变了，Git 工作区状态大概率也变了 —— 一起刷，
      // 否则文件树的着色会停在变更前的状态。
      refreshGitRef.current?.();
    });
    void onDirectoryChange(cancel.emit).then(unlisten => {
      if (tornDown) unlisten();
      else dispose = unlisten;
    });
    return () => {
      tornDown = true;
      dispose?.();
      cancel.cancel();
    };
  }, [watchedDirRoots]);

  /**
   * 订阅外部修改事件。写盘常连发多个事件（内容改动 + rename 落位），
   * 按路径合并成一次读盘；读盘后的基线比较天然过滤重复与自家保存回声。
   */
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    const timers = new Map<string, number>();
    let dispose: (() => void) | null = null;
    let tornDown = false;
    void onExternalChange(path => {
      const prev = timers.get(path);
      if (prev !== undefined) window.clearTimeout(prev);
      timers.set(
        path,
        window.setTimeout(() => {
          timers.delete(path);
          void checkExternalChange(path);
        }, 120)
      );
    }).then(unlisten => {
      if (tornDown) unlisten();
      else dispose = unlisten;
    });
    return () => {
      tornDown = true;
      dispose?.();
      timers.forEach(t => window.clearTimeout(t));
      timers.clear();
    };
  }, [checkExternalChange]);

  /** 关闭 Tab 时清理缓冲：未保存的缓冲刻意保留，重新打开可继续编辑 */
  const releaseBuffers = useCallback((paths: string[]) => {
    paths.forEach(path => {
      if (buffersRef.current[path]?.dirty) return;
      dropBuffer(path);
      // 缓冲真正被丢弃（各调用点随后都会把该路径移出 openFilePaths）：释放对应的
      // monaco model —— @monaco-editor/react 只释放卸载时的当前 model，不释放
      // 换过 path 的旧 model，一个会话里打开过的每个文件都会常驻正文 + undo 栈。
      disposeModelForPath(path);
    });
  }, [dropBuffer]);

  /** 关闭指定 Tab（useStableCallback：EditorTabBar 是 memo 组件，回调引用
   *  每次渲染都变会让浅比较必不等；stable 后实现仍是最新闭包，读到最新 state） */
  const closeFile = useStableCallback((pathToClose: string, e?: React.MouseEvent) => {
    e?.stopPropagation();
    releaseBuffers([pathToClose]);
    const closedIndex = openFilePathsRef.current.indexOf(pathToClose);
    const remaining = openFilePathsRef.current.filter(p => p !== pathToClose);
    setOpenFilePaths(remaining);
    setMarkdownViewModes(prev => {
      if (!(pathToClose in prev)) return prev;
      const next = { ...prev };
      delete next[pathToClose];
      return next;
    });
    setMarkdownScrollRatios(prev => {
      if (!(pathToClose in prev)) return prev;
      const next = { ...prev };
      delete next[pathToClose];
      return next;
    });
    if (activeFilePathRef.current === pathToClose) {
      if (remaining.length > 0) {
        void openFile(remaining[Math.min(Math.max(closedIndex, 0), remaining.length - 1)]);
      } else {
        setActiveFilePath('');
      }
    }
  });

  /** 关闭其他 Tab */
  const closeOtherFiles = useStableCallback((keepPath: string) => {
    releaseBuffers(openFilePathsRef.current.filter(p => p !== keepPath));
    setOpenFilePaths([keepPath]);
    if (activeFilePathRef.current !== keepPath) {
      void openFile(keepPath);
    }
  });

  /** 关闭左侧全部 Tab */
  const closeLeftFiles = useStableCallback((targetPath: string) => {
    const idx = openFilePathsRef.current.indexOf(targetPath);
    if (idx <= 0) return;
    releaseBuffers(openFilePathsRef.current.slice(0, idx));
    if (openFilePathsRef.current.indexOf(activeFilePathRef.current) < idx) {
      void openFile(targetPath);
    }
    setOpenFilePaths(openFilePathsRef.current.slice(idx));
  });

  /** 关闭右侧全部 Tab */
  const closeRightFiles = useStableCallback((targetPath: string) => {
    const idx = openFilePathsRef.current.indexOf(targetPath);
    if (idx < 0 || idx >= openFilePathsRef.current.length - 1) return;
    releaseBuffers(openFilePathsRef.current.slice(idx + 1));
    if (openFilePathsRef.current.indexOf(activeFilePathRef.current) > idx) {
      void openFile(targetPath);
    }
    setOpenFilePaths(openFilePathsRef.current.slice(0, idx + 1));
  });

  /** 关闭全部 Tab */
  const closeAllFiles = useStableCallback(() => {
    releaseBuffers(openFilePathsRef.current);
    setOpenFilePaths([]);
    setMarkdownViewModes({});
    setMarkdownScrollRatios({});
    setActiveFilePath('');
    setActiveTab('editor');
    setDiffOpen(false);
    setDiffCommit(null);
    setDiffAgainstRef(null);
    setDiffCompareCommits(null);
    setDiffSelectedCommits(null);
    setFsCompareFiles(null);
    setDiffFilePath('');
    setCompareBranches(null);
  });

  /** 编辑器内容变化：写入缓冲并标记未保存 */
  const handleEditorChange = useCallback((value: string | undefined) => {
    const path = activeFilePath;
    const buffer = buffersRef.current[path];
    if (!path || !buffer) return;
    putBuffer(path, { ...buffer, content: value ?? '', dirty: true });
    setEditTick(t => t + 1);
  }, [activeFilePath, putBuffer]);

  /** 保存当前文件到磁盘 */
  const saveActiveFile = useCallback(async (): Promise<void> => {
    const path = activeFilePath;
    const buffer = buffersRef.current[path];
    if (!path || !buffer || buffer.language === 'image' || !buffer.dirty) return;
    try {
      savingRef.current.add(path);
      try {
        await invokeCommand('write_file_text', { path, content: buffer.content, encoding: buffer.encoding ?? 'UTF-8' });
      } finally {
        savingRef.current.delete(path);
      }
      // 磁盘真相 = 刚写入的 buffer.content：无论用户是否在写盘期间继续输入都
      // 推进基线并关闭冲突标记（自家写盘覆盖了磁盘），否则「保存 + 继续输入」
      // 会在下一次检测里因基线停在旧值被误判成外部修改。
      // 写盘期间用户又改了同一文件时保留 dirty，交给下一轮自动保存。
      const current = buffersRef.current[path];
      if (current) {
        putBuffer(path, {
          ...current,
          diskHash: contentHash(buffer.content),
          dirty: current.content !== buffer.content,
          externalChanged: false
        });
        // 写盘推进了磁盘 mtime：基线三元组不同步的话，下一次 watcher 事件
        // 探针对不上会白读一次全量。顺手刷新（失败不影响保存主流程）。
        try {
          const probe = await invokeCommand<FileProbe>('file_probe', { path });
          const latest = buffersRef.current[path];
          if (latest) putBuffer(path, { ...latest, diskLen: probe.len, diskMtimeMs: probe.mtimeMs });
        } catch {
          /* 探针是优化层 */
        }
      }
      showStatus(t().status.savedFile(path.split(/[\\/]/).pop() ?? path));
    } catch (err) {
      showStatus(t().status.saveFailed(errText(err)));
    }
  }, [activeFilePath, putBuffer]);

  /** 自动保存全部脏缓冲（对齐 PyCharm：编辑停顿后写盘，失焦立即写盘） */
  const autoSaveAll = useCallback(async (): Promise<void> => {
    // 序号闸：切换项目的窗口失焦回调可能与切换动作交错，此刻
    // buffersRef 里的旧项目缓冲若被写盘，路径虽仍是各自的原文件，
    // 但切项目途中「先落 A 的缓冲再切到 B」会让 A 的未保存内容
    // 落在用户已经确认丢弃的时刻之后。与 openFile/loadWorkspace 同一约定。
    const seq = workspaceSeqRef.current;
    // externalChanged：外部修改尚未由用户表态。自动保存绝不能替用户选择
    // “本地覆盖磁盘”，否则冲突横幅还没看就丢了外部内容。显式 ⌘S 仍可写盘。
    const targets = Object.entries(buffersRef.current).filter(([, b]) => shouldAutoSave(b));
    if (targets.length === 0) return;
    const saved: string[] = [];
    for (const [path, buffer] of targets) {
      // 写盘是逐文件 await 的：期间可能已切换项目。此时表里的缓冲已被
      // 换成新项目的快照，继续写就是把旧项目内容写到已失效的引用上。
      if (seq !== workspaceSeqRef.current) return;
      try {
        savingRef.current.add(path);
        try {
          await invokeCommand('write_file_text', { path, content: buffer.content, encoding: buffer.encoding ?? 'UTF-8' });
        } finally {
          savingRef.current.delete(path);
        }
        // 同 saveActiveFile：推进磁盘基线；写盘期间内容被继续编辑时保留 dirty
        const current = buffersRef.current[path];
        if (current) {
          const stillDirty = current.content !== buffer.content;
          putBuffer(path, {
            ...current,
            diskHash: contentHash(buffer.content),
            dirty: stillDirty,
            externalChanged: false
          });
          // 同 saveActiveFile：同步基线三元组，避免下次检测白读一次
          try {
            const probe = await invokeCommand<FileProbe>('file_probe', { path });
            const latest = buffersRef.current[path];
            if (latest) putBuffer(path, { ...latest, diskLen: probe.len, diskMtimeMs: probe.mtimeMs });
          } catch {
            /* 探针是优化层 */
          }
          if (!stillDirty) saved.push(path.split(/[\\/]/).pop() ?? path);
        }
      } catch {
        /* 写盘失败保持 dirty，状态栏继续提示未保存 */
      }
    }
    if (saved.length > 0) {
      showStatus(
        t().git.autoSavedFiles(String(saved.length)) + `（${saved.slice(0, 2).join(', ')}${saved.length > 2 ? '…' : ''}）`
      );
    }
  }, [putBuffer]);

  /**
   * 退出/关窗前的最后写盘：当前项目 + 所有项目会话快照内的脏缓冲。
   *
   * 后台项目的缓冲只存在 projectSessionsRef 的内存快照里；退出时如果只跑
   * autoSaveAll（仅 buffersRef.current），这些修改会随进程消失。这里逐个
   * 写盘，成功后才标记 clean；externalChanged 则拒绝退出，避免覆盖磁盘上
   * 尚未由用户表态的外部修改。
   */
  const flushAllDirtyBuffers = useCallback(async (): Promise<boolean> => {
    const groups: { buffers: Record<string, FileBuffer>; snapshot?: ProjectSessionSnapshot }[] = [
      { buffers: buffersRef.current }
    ];
    for (const snapshot of Object.values(projectSessionsRef.current)) {
      groups.push({ buffers: snapshot.buffers, snapshot });
    }
    const plan = planExitFlush(groups.map(group => group.buffers));
    if (plan.blocked > 0) {
      // 外部修改尚未由用户表态：自动写盘会覆盖磁盘内容，宁可取消退出。
      showStatus(t().status.exitSaveBlocked(plan.blocked));
      return false;
    }
    let saved = 0;
    for (const write of plan.writes) {
      try {
        await invokeCommand('write_file_text', {
          path: write.path,
          content: write.content,
          encoding: write.encoding ?? 'UTF-8'
        });
      } catch (err) {
        showStatus(t().status.saveFailed(errText(err)));
        return false;
      }
      saved += 1;
      // 计划已按「当前项目优先」去重；这里找到首个持有该路径的组标记 clean。
      const group = groups.find(item => !!item.buffers[write.path]);
      if (!group) continue;
      const buffer = group.buffers[write.path];
      if (!buffer) continue;
      const diskHash = contentHash(write.content);
      if (group.snapshot) {
        group.snapshot.buffers[write.path] = {
          ...buffer,
          diskHash,
          dirty: false,
          externalChanged: false
        };
      } else {
        const current = buffersRef.current[write.path];
        if (current) {
          putBuffer(write.path, { ...current, diskHash, dirty: false, externalChanged: false });
        }
      }
    }
    if (saved > 0) showStatus(t().git.autoSavedFiles(String(saved)));
    return true;
  }, [putBuffer, showStatus]);

  /** 关窗/退出事件可能并发触发（JS close + Rust exit）：合并成一次写盘。 */
  const flushInFlight = useRef<Promise<boolean> | null>(null);
  const flushAllOnce = useCallback((): Promise<boolean> => {
    if (flushInFlight.current) return flushInFlight.current;
    const pending = flushAllDirtyBuffers().finally(() => {
      flushInFlight.current = null;
    });
    flushInFlight.current = pending;
    return pending;
  }, [flushAllDirtyBuffers]);

  // 关窗：Tauri 的 close 是异步的，先 prevent，写盘完成后再 destroy。
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let closing = false;
    const win = getCurrentWindow();
    void win
      .onCloseRequested(async event => {
        if (closing) {
          event.preventDefault();
          return;
        }
        event.preventDefault();
        closing = true;
        // 合并/冲突子窗口存在时，Rust 的 CloseRequested 会拦截并聚焦子窗口。
        // 这里不要 destroy 主窗口，否则会把子窗口和未完成的合并流程一起带走。
        try {
          const others = await invokeCommand<{ label: string }[]>('list_other_windows');
          if (others.some(w => w.label === 'win_merge_tool' || w.label === 'win_conflicts')) {
            closing = false;
            return;
          }
        } catch {
          /* 查询失败时按无子窗口处理 */
        }
        const ok = await flushAllOnce().catch(() => false);
        if (!ok) {
          closing = false;
          return;
        }
        try {
          await win.destroy();
        } catch {
          closing = false;
        }
      })
      .then(fn => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [flushAllOnce]);

  // Cmd+Q / 原生菜单退出：Rust 先 prevent_exit 并广播此事件，前端写盘后
  // 再通知后端真正退出。写盘失败时通知 cancel，应用保持打开。
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen('app://before-exit', async () => {
      const ok = await flushAllOnce().catch(() => false);
      if (ok) {
        await invokeCommand('finish_app_exit').catch(() => {});
      } else {
        await invokeCommand('cancel_app_exit').catch(() => {});
      }
    })
      .then(fn => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [flushAllOnce]);

  // 编辑停顿 1.2s 后自动保存。editTick 让同一文件里的连续输入也重置计时器，
  // 否则 dirty 路径集合不变、effect 不重跑，定时器不会续期。
  const autoSaveTimer = useRef<number | null>(null);
  const dirtySignature = dirtyPaths.join('|');
  useEffect(() => {
    if (dirtyPaths.length === 0) return;
    if (autoSaveTimer.current !== null) window.clearTimeout(autoSaveTimer.current);
    autoSaveTimer.current = window.setTimeout(() => {
      autoSaveTimer.current = null;
      void autoSaveAll();
    }, 1200);
    return () => {
      if (autoSaveTimer.current !== null) {
        window.clearTimeout(autoSaveTimer.current);
        autoSaveTimer.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dirtySignature, editTick, autoSaveAll]);

  // 窗口失焦立即保存（PyCharm: Save files on frame deactivation）
  useEffect(() => {
    const onBlur = () => void autoSaveAll();
    window.addEventListener('blur', onBlur);
    return () => window.removeEventListener('blur', onBlur);
  }, [autoSaveAll]);

  // 文件树抽屉宽度（可拖拽调节并持久化）
  const [projectDrawerWidth, setProjectDrawerWidth] = useState<number>(() =>
    readPanelSize('ide_project_drawer_width', 270, 180, 560)
  );

  // Commit 抽屉宽度（可拖拽调节并持久化，与文件树抽屉一致）
  const [commitDrawerWidth, setCommitDrawerWidth] = useState<number>(() =>
    readPanelSize('ide_commit_drawer_width', 330, 240, 640)
  );

  const startResizeCommitDrawer = (e: React.PointerEvent) => {
    const startX = e.clientX;
    const startW = commitDrawerWidth;
    const maxW = () => Math.min(640, Math.round(window.innerWidth * 0.5));
    // Commit 抽屉在左侧、手柄在右缘：向右拖 = 加宽
    const compute = (clientX: number) => Math.max(240, Math.min(maxW(), startW + (clientX - startX)));

    startPanelDrag(
      e,
      { element: document.getElementById('pycharmCommitDrawer'), axis: 'x' },
      'col-resize',
      compute,
      finalW => {
        setCommitDrawerWidth(finalW);
        writeStorage('ide_commit_drawer_width', String(finalW));
      },
    );
  };

  const startResizeProjectDrawer = (e: React.PointerEvent) => {
    const startX = e.clientX;
    const startW = projectDrawerWidth;
    const maxW = () => Math.min(560, Math.round(window.innerWidth * 0.5));
    const compute = (clientX: number) => Math.max(180, Math.min(maxW(), startW + (clientX - startX)));

    const element = document.getElementById('projectDrawer')
      ?? document.getElementById('compareDrawer')
      ?? document.querySelector<HTMLElement>('.project-drawer, .compare-drawer');

    startPanelDrag(
      e,
      { element, axis: 'x' },
      'col-resize',
      compute,
      finalW => {
        setProjectDrawerWidth(finalW);
        writeStorage('ide_project_drawer_width', String(finalW));
      },
    );
  };
  const [initialPanelLayout] = useState(() => readPanelLayout(appViewportHeight()));
  /** Git / 控制台分别记忆非最大化高度：互不影响 */
  const [gitPanelHeight, setGitPanelHeight] = useState(initialPanelLayout.gitHeight);
  const [terminalPanelHeight, setTerminalPanelHeight] = useState(initialPanelLayout.terminalHeight);
  /** 无落盘高度时，首开 Git 要按“打开那一刻”的窗口高度重算 40%；每次启动只重算一次 */
  const gitPanelHeightInitializedRef = useRef(initialPanelLayout.gitHeightStored);
  const [gitPanelMaximized, setGitPanelMaximized] = useState(false);
  const [terminalMaximized, setTerminalMaximized] = useState(initialPanelLayout.terminalMaximized);
  const [terminalPlacement, setTerminalPlacement] = useState<'bottom' | 'right'>(() => {
    try {
      return localStorage.getItem('ide_terminal_placement') === 'right' ? 'right' : 'bottom';
    } catch {
      return 'bottom';
    }
  });
  /** 控制台贴右时的宽度（px），跨会话持久化 */
  const [terminalDockWidth, setTerminalDockWidth] = useState<number>(() =>
    readPanelSize('ide_terminal_dock_width', 460, 240, 1200)
  );
  /** 控制台拖拽吸附预览：高亮即将停靠的目标区域 */
  const [terminalSnapTarget, setTerminalSnapTarget] = useState<'right' | 'bottom' | null>(null);
  /** 拖拽开始时记录的主区几何信息，用于定位吸附预览层 */
  const terminalSnapZoneRef = useRef<{ left: number; top: number; width: number; height: number } | null>(null);
  /** 当前项目控制台正在运行的命令名（去重）：LeftStrip Terminal 图标角标 + tooltip */
  const [terminalRunning, setTerminalRunning] = useState<string[]>([]);
  /** 控制台活动会话的 shell cwd：并入全局状态栏（终端面板自带的专用状态栏已移除） */
  const [terminalCwd, setTerminalCwd] = useState<string | null>(null);

  /** 切换控制台停靠位置并持久化（下次打开 app 恢复同样的位置） */
  const applyTerminalPlacement = (placement: 'bottom' | 'right') => {
    setTerminalPlacement(placement);
    writeStorage('ide_terminal_placement', placement);
  };

  /** 贴右模式下左右拖拽调整控制台宽度（实时更新，松手落盘） */
  const startResizeTerminalDock = (e: React.PointerEvent) => {
    const startX = e.clientX;
    const startW = terminalDockWidth;
    const clampW = (w: number) => Math.max(240, Math.min(Math.max(280, window.innerWidth - 360), w));
    // 控制台把手在主区右缘：向左拖 = 加宽
    const compute = (clientX: number) => {
      const w = clampW(startW + (startX - clientX));
      // 拖动即时生效：宽度 ≥75% 主区宽时擦除主区内容（松手后 effect 按提交值再校准）
      setPanelDominant(0, w);
      return w;
    };

    startPanelDrag(
      e,
      { element: document.querySelector<HTMLElement>('.ide-terminal-dock'), axis: 'x' },
      'col-resize',
      compute,
      finalW => {
        setTerminalDockWidth(finalW);
        setPanelDominant(0, finalW);
        writeStorage('ide_terminal_dock_width', String(finalW));
      },
    );
  };

  /** 按住控制台把柄拖动：靠近主区右缘吸附为右侧停靠，靠近下缘吸附回底部，其余位置松手取消。
   *  用指针捕获原语：光标移出窗口松手也能收到 pointerup，系统手势抢占走 onCancel。 */
  const startTerminalSnapDrag = (e: React.PointerEvent) => {
    const center = document.querySelector('.ide-center-workspace');
    const rect = center
      ? center.getBoundingClientRect()
      : { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight };
    const statusH = document.querySelector('.ide-statusbar')?.getBoundingClientRect().height ?? 22;
    const dockH = Math.max(120, rect.height - statusH);
    terminalSnapZoneRef.current = { left: rect.left, top: rect.top, width: rect.width, height: dockH };

    // 右侧 28% 区域命中「贴右」，下方 28% 命中「贴底」，角落优先贴右
    const zoneOf = (x: number, y: number): 'right' | 'bottom' | null => {
      if (x >= rect.left + rect.width * 0.72) return 'right';
      if (y >= rect.top + dockH * 0.72) return 'bottom';
      return null;
    };

    startPointerDrag(e, 'grabbing', {
      onMove: (x, y) => {
        const zone = zoneOf(x, y);
        setTerminalSnapTarget(prev => (prev === zone ? prev : zone));
      },
      onEnd: (x, y) => {
        const target = zoneOf(x, y);
        if (target) applyTerminalPlacement(target);
        setTerminalSnapTarget(null);
      },
      onCancel: (x, y) => {
        // 系统抢占指针：按抢占前所在区域收尾，与正常松手一致
        const target = zoneOf(x, y);
        if (target) applyTerminalPlacement(target);
        setTerminalSnapTarget(null);
      }
    });
  };

  // 关闭底部面板时退出最大化。git 面板沿用老行为（收起即复位）；
  // 控制台按要求「最大化状态下隐藏，重新显示仍保持最大化」——故不复位 terminalMaximized，
  // 由最大化按钮/双击标题栏显式还原。
  useEffect(() => {
    if (activeBottomTool === null && gitPanelMaximized) {
      setGitPanelMaximized(false);
    }
  }, [activeBottomTool, gitPanelMaximized]);

  // 首次打开 Git Log 时按“打开那一刻”的窗口高度重算 40%：若 app 启动后用户
  // 先调整了窗口再打开，仍以当前窗口为准。拖过（已有落盘高度）则不重算。
  useLayoutEffect(() => {
    if (activeBottomTool !== 'git' || gitPanelHeightInitializedRef.current) return;
    gitPanelHeightInitializedRef.current = true;
    setGitPanelHeight(defaultGitPanelHeight(appViewportHeight()));
  }, [activeBottomTool]);

  // 控制台最大化状态跨重启保留；首次缺省为 true（见 readPanelLayout）。
  useEffect(() => {
    writeStorage(TERMINAL_MAXIMIZED_KEY, terminalMaximized ? '1' : '0');
  }, [terminalMaximized]);

  /** 面板可拖到的最大高度 = 窗口高 − 顶栏 − 状态栏：面板顶正好落在顶栏底部（最大化），
   *  状态栏保留在底部不被 grid 溢出裁剪。不再用 85% 截断。
   *  顶栏/状态栏高度取 CSS 变量（与 theme.css 的 --titlebar-h/--panel-title-h 同源），
   *  首帧样式未挂载时按当前值兜底。 */
  const workspaceMaxH = () => {
    const cs = getComputedStyle(document.documentElement);
    const px = (name: string, fallback: number) => {
      const n = Number.parseFloat(cs.getPropertyValue(name));
      return Number.isFinite(n) && n > 0 ? n : fallback;
    };
    return Math.max(200, Math.round(window.innerHeight - px('--titlebar-h', 36) - px('--panel-title-h', 28)));
  };
  /** 面板超过 75% 后擦除主区内容（编辑器只剩窄缝，露背景像渲染残留）。
   *  底部面板按高度判、贴右控制台按宽度判；贴右的基准取主区容器宽度而不是窗口宽度 ——
   *  左侧还有抽屉，用窗口宽度当基准永远够不到阈值。
   *  class 直写 DOM：拖动中每帧 setState 会重跑整棵 App 树，见 panelDrag.ts 注释。 */
  const setPanelDominant = (h: number, w = 0) => {
    const el = document.querySelector<HTMLElement>('.ide-center-workspace');
    if (!el) return;
    el.classList.toggle(
      'is-panel-dominant',
      h >= window.innerHeight * 0.75 || w >= el.clientWidth * 0.75
    );
  };

  // 最大化/恢复/关面板/改窗口尺寸/改停靠宽度都会让高度与 dominant 真值变化：从状态回算一次
  // （拖动中由 compute 直写，不经这里，避免 effect 用旧值反写 class）
  useEffect(() => {
    const apply = () => {
      const bottomVisible = activeBottomTool !== null && !(activeBottomTool === 'terminal' && terminalPlacement === 'right');
      const activeHeight = activeBottomTool === 'git' ? gitPanelHeight : terminalPanelHeight;
      const h = !bottomVisible
        ? 0
        : ((activeBottomTool === 'git' ? gitPanelMaximized : terminalMaximized)
            ? Infinity
            : Math.min(activeHeight, workspaceMaxH()));
      // 贴右控制台按宽度判 dominant；最大化 = 占满整行，直接给 Infinity
      const dockVisible = activeBottomTool === 'terminal' && terminalPlacement === 'right';
      const w = !dockVisible ? 0 : terminalMaximized ? Infinity : terminalDockWidth;
      setPanelDominant(h, w);
    };
    apply();
    window.addEventListener('resize', apply);
    return () => window.removeEventListener('resize', apply);
  }, [activeBottomTool, terminalPlacement, gitPanelMaximized, terminalMaximized, gitPanelHeight, terminalPanelHeight, terminalDockWidth]);

  const startResizeBottomPanel = (e: React.PointerEvent) => {
    // 两个把手（底部面板 / 控制台）共用本函数，尺寸宿主不同：
    // 取把手自身的最近容器，避免依赖 activeBottomTool 判断当前谁可见。
    const host = (e.currentTarget as HTMLElement | null)?.closest<HTMLElement>(
      '.ide-bottom-panel-wrapper, .ide-terminal-dock'
    );
    const isGitPanel = host?.classList.contains('ide-bottom-panel-wrapper') ?? false;
    const startY = e.clientY;
    const maxH = () => workspaceMaxH();
    const startH = Math.min(isGitPanel ? gitPanelHeight : terminalPanelHeight, maxH());
    // startPanelDrag 以 (clientX, clientY) 位置传参：垂直拖拽必须取第二个参数。
    // compute 每次指针移动都跑：顺手按当前高度切 dominant（>75% 隐藏主区背景），
    // 拖动即时生效，松手后 effect 会按提交值再校准一次。
    const compute = (_clientX: number, clientY: number) => {
      const h = Math.round(Math.max(100, Math.min(maxH(), startH + (startY - clientY))));
      setPanelDominant(h);
      return h;
    };

    startPanelDrag(
      e,
      { element: host ?? null, axis: 'y' },
      'row-resize',
      compute,
      finalH => {
        if (isGitPanel) {
          setGitPanelHeight(finalH);
          gitPanelHeightInitializedRef.current = true;
          writeStorage(GIT_PANEL_HEIGHT_KEY, String(finalH));
        } else {
          setTerminalPanelHeight(finalH);
          writeStorage(TERMINAL_PANEL_HEIGHT_KEY, String(finalH));
        }
        setPanelDominant(finalH);
      },
    );
  };

  /** 保存并更新最近工作区列表（historyStore：命名空间 + 旧 key 自动迁移） */
  const saveRecentWorkspace = (root: string) => {
    if (!root) return;
    saveLastWorkspace(root);
    setRecentWorkspaces(pushHistory(RECENT_WORKSPACES, root));
  };

  /** 移除单个最近项目 */
  const removeRecentWorkspace = (pathToRemove: string, e: React.MouseEvent) => {
    e.stopPropagation();
    setRecentWorkspaces(removeFromHistory(RECENT_WORKSPACES, pathToRemove));
    if (loadLastWorkspace() === pathToRemove) clearLastWorkspace();
  };

  /**
   * 切换/打开项目时立刻清掉上一个项目的 Git 展示数据，并作废在途 refreshGit。
   *
   * 否则 Changes 列表在新数据到达前一直显示旧项目内容；在途请求若不作废，
   * 还可能在 single-flight 补跑时把旧仓库状态写进新项目。
   */
  const resetGitUiForWorkspaceSwitch = useCallback(() => {
    // 旧 root 的 LSP 进程与 Monaco providers 不能跨项目复用。
    void resetLsp();
    gitRepoRootRef.current = '';
    setGitRepoRoot('');
    setChangedFiles([]);
    setCommits([]);
    setBranches([]);
    setTags([]);
    setChangelists({ lists: ['Changes'], files: {} });
    setFocusCommit(null);
    setSequencer(null);
    setHasMoreCommits(false);
    setCurrentBranch('');
    // 日志过滤必须跟着项目重置：logPath 是 A 项目的路径，切到 B 项目后
    // git_log 拿着它当 pathspec，B 仓库里匹配不到 → 静默返回空列表，
    // 用户看到的是「新项目没有提交」而非「过滤不匹配」。
    // PyCharm 的 Log 过滤器本就是项目会话的一部分，随切换清空。
    setLogBranch('');
    setLogPath('');
    setLogUser('');
    setLogDate('all');
    // 全部带着「某个项目的对象」的弹窗必须同时关闭。
    // 它们跨项目持有的对象属于上一个仓库：resetTarget/rebaseBase 是上一个
    // 仓库的 commit hash，historyFile/nameDialog 持有的是旧项目的路径。
    // 换项目后这些弹窗若留在屏上，确认按钮会用「新 repoPath + 旧对象」
    // 去敲 git -- reset 到旧 hash / diff 旧文件 —— 前者是明确的操作错误
    // （git 报 unknown revision），后者可能读到旧项目磁盘上仍存在的路径，
    // 把另一个仓库的内容呈现给用户。
    setResetTarget(null);
    setSquashCommitsTarget(null);
    setRebaseBase(null);
    setHistoryFile(null);
    setNameDialog(null);
    setFsCompareFiles(null);
    setConflictDialogOpen(false);
    setCompareRef(null);
    setCompareWorktree(false);
    setCompareBranches(null);
    setPushDialogOpen(false);
    setPullDialogOpen(false);
    setPullDialogRebase(false);
    setStashDialogOpen(false);
    setUnstashDialogOpen(false);
    setShelveDialogOpen(false);
    setGitToolsOpen(false);
    setFindInFilesOpen(false);
    setFindInFilesScope('');
    // 作废在途结果；若仍有请求在跑，置 pending 让 finally 用最新 refreshGit 补跑
    gitSeqRef.current += 1;
    if (gitInFlightRef.current) {
      gitPendingRef.current = true;
    }
    setGitRefreshing(true);
    setGitRefreshTick(t => t + 1);
  }, []);

  /**
   * 项目层设置刷新（Zed settings 分层）：读 `<root>/.lazecode/settings.json`，
   * 存在即叠加到 settings，缺文件静默回落 user 层，损坏提示修文件并忽略该层。
   *
   * `loadSettings()` 是同步的（挂载时就要主题），项目层只能后填缓存再
   * setSettings —— 因此本函数是唯一写项目层缓存的地方（连同下面的
   * clearProjectLayer 调用点），不要在别处直接碰缓存。
   *
   * `isCurrent` 由调用方提供过期门：打开流程用 workspace seq，
   * 切回流程用 workspacePathRef。过期项目的层不能落地。
   */
  const refreshProjectLayer = useCallback((root: string, isCurrent: () => boolean) => {
    if (!isTauriEnvironment()) {
      clearProjectLayer();
      setSettings(loadSettings());
      return;
    }
    void invokeCommand<string>('read_file_content', { path: projectSettingsPath(root) }).then(
      text => {
        if (!isCurrent()) return;
        try {
          const { settings, warnings } = applyProjectSettingsText(text);
          setSettings(settings);
          if (warnings.length > 0) showStatus(t().status.projectSettingsWarnings(warnings.join('; ')));
        } catch (err) {
          clearProjectLayer();
          setSettings(loadSettings());
          showStatus(t().status.projectSettingsBroken(errText(err)));
        }
      },
      () => {
        // 文件不存在（大多数项目）：清掉上一项目的层，回落 user 层
        if (!isCurrent()) return;
        clearProjectLayer();
        setSettings(loadSettings());
      },
    );
  }, [showStatus]);

  /** 载入工作区：极速浅层扫描 (depth=2) + 自动保存历史 */
  const loadWorkspace = useCallback(
    async (root: string) => {
      if (!root) return;
      // 载入新项目会清空全部缓冲。switchToProject 在调用本函数前已把当前会话
      // 存进快照（数据不会丢，不必打扰用户）；启动恢复时 buffersRef 为空。
      // 保留下面这个确认分支作为兜底：未来若新增直接调用 loadWorkspace 的路径，
      // 脏缓冲也不会静默丢失。
      const dirtyCount = Object.values(buffersRef.current).filter(b => b.dirty).length;
      const sessionPreserved = !!workspacePathRef.current && !!projectSessionsRef.current[workspacePathRef.current];
      if (dirtyCount > 0 && !sessionPreserved) {
        if (!(await appConfirm(t().dialog.unsavedOnOpen(dirtyCount), { title: t().dialog.unsavedOnOpenTitle, kind: 'warning', okLabel: 'Save' }))) return;
        await autoSaveAll();
        const remaining = Object.values(buffersRef.current).filter(b => b.dirty).length;
        if (remaining > 0) {
          // 写盘失败时不能继续：下面会清空缓冲，继续就是静默丢改动
          showStatus(t().status.stillDirtyCancelledCount(remaining));
          return;
        }
      }
      const seq = ++workspaceSeqRef.current;
      openOrderRef.current = [root, ...openOrderRef.current.filter(p => p !== root)];
      setExternalRoots([]);
      // 向后端登记工作区根：read/write/delete/paste 只允许落在已登记根之内。
      // 后端原本对这些命令零路径校验，等于「打开项目即授予任意文件读写」。
      //
      // **登记必须先于一切"已打开该项目的"状态**：下面 setWorkspacePath /
      // saveRecentWorkspace 一旦执行，UI 就会显示项目已打开，而所有 fs 命令
      // 此时都会因越界被拒 —— 用户看到的是「项目打开了但点啥都报错」。
      // 登记失败时整段放弃，UI 停留在上一个项目，最近项目列表也不留下
      // 打不开的条目（否则下次从该列表进入还会再失败一次）。
      try {
        await invokeCommand('register_workspace', { path: root, readOnly: false });
      } catch (err) {
        // 不能把序号倒回（`seq - 1`）：workspaceSeqRef 是单调递增的「谁最新
        // 谁有效」闸（openFile / refreshTreeDir / switchToProject 都靠它）。
        // 倒回会让一个已被 switchToProject 作废的在途请求重新通过
        // `seq !== current` 检查 —— 旧项目的树/文件内容就此落地。
        //
        // 正确做法：再向前推一格（abortedSeq 记录这个「已失败」的序号），
        // 让 seq 保持单调，同时本函数下方所有 `seq !== current` 检查自然
        // 失配而提前返回。monotonic forward 不可能复活任何旧的 in-flight。
        workspaceSeqRef.current = seq + 1;
        showStatus(t().status.openProjectFailedAuth(errText(err)));
        openOrderRef.current = openOrderRef.current.filter(p => p !== root);
        setExternalRoots([]);
        return;
      }
      setWorkspacePath(root);
      saveRecentWorkspace(root);
      // 项目层设置与树加载并行：读文件是独立 IPC，不阻塞树渲染
      refreshProjectLayer(root, () => seq === workspaceSeqRef.current);
      // 遥测（只记"打开了"，不记路径）：耗时/命令明细由 tauriBridge 统一记
      trackEvent('Project Opened');
      resetGitUiForWorkspaceSwitch();
      try {
        const tree = await invokeCommand<FileNode>('read_directory', { path: root, maxDepth: 2, ignored: settings.ignoredPatterns });
        if (seq !== workspaceSeqRef.current) return;
        // Diff 属于某个仓库：换了项目就没有可比较的对象，标签一并关掉
        setActiveTab('editor');
        setDiffOpen(false);
        setDiffCommit(null);
        setDiffAgainstRef(null);
        setDiffCompareCommits(null);
        setDiffSelectedCommits(null);
        setDiffStaged(false);
        setFsCompareFiles(null);
        setMarkdownViewModes({});
        setMarkdownScrollRatios({});
        // 标签与缓冲全部丢弃（脏缓冲在上面已写盘）：连带释放各自的 monaco model
        Object.keys(buffersRef.current).forEach(disposeModelForPath);
        buffersRef.current = {};
        setBuffers({});

        if (!tree.isDirectory) {
          // 单文件工作区：将单个文件放入树中，并立刻打开它
          setWorkspaceIsFile(true);
          setFileTree(internFileTree([tree]));
          setTreeExpanded(new Set());
          setOpenFilePaths([root]);
          setActiveFilePath(root);
          void openFile(root);
        } else {
          setWorkspaceIsFile(false);
          setFileTree(internFileTree(tree.children ?? []));
          setTreeExpanded(new Set([root]));
          setOpenFilePaths([]);
          setActiveFilePath('');
        }
      } catch (err) {
        showStatus(t().status.loadWorkspaceFailed(errText(err)));
      }
    },
    [resetGitUiForWorkspaceSwitch, showStatus, autoSaveAll, refreshProjectLayer, openFile]
  );

  /** 动态更新树节点子目录（支持懒加载） */
  const updateFileTreeNode = useCallback((parentPath: string, newChildren: FileNode[]) => {
    // 入口统一驻留：所有 read_directory 的 children 都经这里进树（懒加载/
    // 刷新/展开/定位），路径串只保留一份 canonical 拷贝（见 stringIntern）。
    const safeChildren = internFileTree(Array.isArray(newChildren) ? newChildren : []);
    setFileTree(prevTree => {
      const updateRecursive = (list: FileNode[]): FileNode[] => {
        if (!Array.isArray(list)) return [];
        return list.map(item => {
          if (item.path === parentPath) {
            return { ...item, children: safeChildren };
          }
          if (item.isDirectory && Array.isArray(item.children)) {
            return { ...item, children: updateRecursive(item.children) };
          }
          return item;
        });
      };
      return updateRecursive(prevTree);
    });
  }, []);

  /**
   * 释放超出 MAX_PROJECT_SESSIONS 的旧项目快照。
   *
   * 每份快照自带整棵文件树与全部编辑缓冲，切换过的项目越多常驻内存越高，因此只保留
   * 最近的若干个。淘汰顺序按对象键序（= 快照写入顺序 = 最久未切换）。
   *
   * 安全规则：快照里的脏缓冲必须先写盘再删除，写盘失败就整份保留 —— 用户切回该项目时
   * 靠快照恢复未保存改动，删掉就是静默丢数据。串行写盘是为了避免嵌套工作区里
   * 同一路径出现在两份快照中时互相覆盖。
   */
  const pruneProjectSessions = useCallback(async (keepPath: string) => {
    const sessions = projectSessionsRef.current;
    const plan = planProjectPrune(
      Object.keys(sessions),
      keepPath,
      liveTerminalProjectsRef.current,
      MAX_PROJECT_SESSIONS
    );
    if (plan.pinnedOverflow > 0) {
      showStatus(t().status.projectsPinnedWarning(plan.pinnedOverflow));
    }
    if (plan.prunable.length === 0) return;
    let pruned = false;
    for (const projectPath of plan.prunable) {
      const snap = sessions[projectPath];
      if (!snap) continue;
      let failed = false;
      for (const [path, buffer] of Object.entries(snap.buffers)) {
        if (!buffer.dirty || buffer.language === 'image' || buffer.error) continue;
        try {
          await invokeCommand('write_file_text', { path, content: buffer.content, encoding: buffer.encoding ?? 'UTF-8' });
        } catch (err) {
          showStatus(
            t().git.releaseSessionFailed(baseNameOf(projectPath), errText(err))
          );
          failed = true;
          break;
        }
      }
      // 有改动没落盘就整份保留，宁可暂时超出上限也不能丢数据
      if (!failed) {
        delete sessions[projectPath];
        // 该项目的缓冲就此丢弃（不属于当前项目，无编辑器持有）：连带释放 model
        Object.keys(snap.buffers).forEach(disposeModelForPath);
        pruned = true;
      }
    }
    // 快照存在 ref 里、不参与依赖比较，淘汰后显式触发列表重算
    if (pruned) setSessionPruneTick(t => t + 1);
  }, []);

  /** 窗口内切换项目：快照当前项目会话 → 恢复目标项目（有快照恢复，无快照正常载入） */
  const switchToProject = useCallback(
    (target: string) => {
      if (!target || target === workspacePath) return;
      // 作废在途的 loadWorkspace 响应：快照恢复是同步的，旧项目的异步树/文件响应不得再落地
      workspaceSeqRef.current += 1;
      setExternalRoots([]);
      // 立刻清掉旧项目的 Changes/分支/提交，并作废在途 Git 刷新
      resetGitUiForWorkspaceSwitch();
      openOrderRef.current = [
        target,
        ...(workspacePath && workspacePath !== target ? [workspacePath] : []),
        ...openOrderRef.current.filter(p => p !== target && p !== workspacePath),
      ];
      if (workspacePath) {
        projectSessionsRef.current[workspacePath] = {
          openFilePaths,
          activeFilePath,
          buffers: { ...buffersRef.current },
          fileTree,
          workspaceIsFile,
          treeExpanded: new Set(treeExpanded),
          activeTab: activeTab === 'compare-branches' ? 'editor' : activeTab,
          diffOpen,
          markdownViewModes: { ...markdownViewModes },
          markdownScrollRatios: { ...markdownScrollRatios },
          diffCommit,
          diffAgainstRef,
          diffCompareCommits,
          diffSelectedCommits,
          diffStaged,
          diffFilePath,
          fsCompareFiles,
        };
      }
      // 2) 恢复目标项目
      const snap = projectSessionsRef.current[target];
      if (snap) {
        delete projectSessionsRef.current[target];
        buffersRef.current = { ...snap.buffers };
        setBuffers(buffersRef.current);
        setFileTree(snap.fileTree);
        setWorkspaceIsFile(snap.workspaceIsFile ?? false);
        setTreeExpanded(new Set(snap.treeExpanded));
        setOpenFilePaths(snap.openFilePaths);
        setActiveFilePath(snap.activeFilePath);
        setActiveTab(snap.activeTab === 'compare-branches' ? 'editor' : snap.activeTab);
        setDiffOpen(snap.diffOpen);
        setMarkdownViewModes({ ...snap.markdownViewModes });
        setMarkdownScrollRatios({ ...snap.markdownScrollRatios });
        setDiffCommit(snap.diffCommit);
        setDiffAgainstRef(snap.diffAgainstRef);
        setDiffCompareCommits(snap.diffCompareCommits);
        setDiffSelectedCommits(snap.diffSelectedCommits ?? null);
        setDiffStaged(snap.diffStaged);
        setDiffFilePath(snap.diffFilePath || '');
        setFsCompareFiles(snap.fsCompareFiles);
        setWorkspacePath(target);
        saveRecentWorkspace(target);
        // Git 状态已清空；随 workspacePath 变化的 refreshGit effect 立即拉新项目数据
      } else {
        void loadWorkspace(target);
      }
      // 3) 快照数可能因本次新增超出上限，异步回收最旧的那些（含写盘，故不阻塞切换）
      void pruneProjectSessions(target);
    },
    [
      workspacePath,
      openFilePaths,
      activeFilePath,
      fileTree,
      workspaceIsFile,
      treeExpanded,
      activeTab,
      markdownViewModes,
      markdownScrollRatios,
      diffCommit,
      diffAgainstRef,
      diffCompareCommits,
      diffSelectedCommits,
      diffStaged,
      diffFilePath,
      fsCompareFiles,
      loadWorkspace,
      resetGitUiForWorkspaceSwitch
    ]
  );

  // Finder "Open With" / Dock recent documents -> native RunEvent::Opened ->
  // open the path as the current project (switchToProject handles dirty buffers).
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    let unlisten: (() => void) | undefined;
    void listen<string[]>('app://open-paths', event => {
      const target = event.payload.find(Boolean);
      if (target) switchToProject(target);
    }).then(fn => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [switchToProject]);

  /** 本窗口可切换项目列表随工作区变化而更新（当前项目始终在列） */
  useEffect(() => {
    const paths = Array.from(new Set([workspacePath, ...Object.keys(projectSessionsRef.current)])).filter(Boolean);
    const mapped = paths.map(p => ({ path: p, name: p.split(/[\\/]/).filter(Boolean).pop() ?? p }));
    setSessionProjects(prev =>
      prev.length === mapped.length && prev.every((x, i) => x.path === mapped[i].path) ? prev : mapped
    );
  }, [workspacePath, sessionPruneTick]);

  /** 窗口标题同步为当前项目名：供其它窗口的「打开的项目」分区展示 */
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    const name =
      workspacePath
        .split(/[\\/]/)
        .filter(p => p && p !== '.')
        .pop() || 'LazeCode';
    invokeCommand('set_window_title', { title: workspacePath ? name : 'LazeCode', path: workspacePath }).catch(() => {});
  }, [workspacePath]);

  /** 项目菜单展开时刷新其它窗口列表 */
  const refreshOtherWindows = useCallback(() => {
    if (!isTauriEnvironment()) return;
    invokeCommand<{ label: string; title: string; path: string }[]>('list_other_windows', {})
      .then(list => setOtherWindows(Array.isArray(list) ? list : []))
      .catch(() => setOtherWindows([]));
  }, []);

  /** 把其它窗口带到前台 */
  const focusWindow = useCallback((label: string) => {
    invokeCommand('focus_window', { label }).catch(() => {});
  }, []);

  /** 通过原生对话框选择工作区目录或单个文件（支持打开单文件编辑/预览） */
  const pickWorkspace = useCallback(async () => {
    if (!isTauriEnvironment()) {
      showStatus(t().status.browserPreviewNoDirPicker);
      return;
    }
    try {
      let picked: string | null = null;
      try {
        picked = await invokeCommand<string | null>('pick_path', {
          title: t().titleBar.openNewProject,
          defaultPath: workspacePath || undefined
        });
      } catch {
        const res = await openDialog({
          directory: false,
          multiple: false,
          title: t().titleBar.openNewProject
        });
        picked = typeof res === 'string' ? res : null;
      }
      if (picked) switchToProject(picked);
    } catch (err) {
      showStatus(t().status.openDirFailed(errText(err)));
    }
  }, [switchToProject, showStatus, workspacePath]);

  // 非 git 管理的目录或单文件：自动切回项目树，不保留 commit 面板
  useEffect(() => {
    if (!isRepo && activeDrawer === 'commit') {
      setActiveDrawer('project');
    }
  }, [isRepo, activeDrawer]);

  /**
   * 关闭位于给定路径（含目录前缀）下的所有标签页。
   *
   * 共享给两处语义：删除文件/目录后关掉受影响 tab（deletePathsToTrash），
   * 以及外部目录从文件树移除后关掉其下全部 tab（closeExternalRoot）。
   * 定义在 closeExternalRoot 之前：const 声明有 TDZ，反序引用会在运行时炸。
   */
  const closeFilesUnderPaths = useCallback(
    (roots: string[]) => {
      const prefixes = roots.map(r => (r.endsWith('/') ? r : `${r}/`));
      const affected = openFilePaths.filter(p => roots.includes(p) || prefixes.some(x => p.startsWith(x)));
      if (affected.length === 0) return;
      releaseBuffers(affected);
      const remaining = openFilePaths.filter(p => !affected.includes(p));
      setOpenFilePaths(remaining);
      if (affected.includes(activeFilePath)) {
        if (remaining.length > 0) void openFile(remaining[0]);
        else setActiveFilePath('');
      }
    },
    [openFilePaths, activeFilePath, releaseBuffers, openFile]
  );

  // 把实现接到前向 ref：checkExternalChange（外部删除 → 关标签）在几百行前
  // 就要用到它，见 closeFilesForExternalDeleteRef 的说明。
  closeFilesForExternalDeleteRef.current = closeFilesUnderPaths;

  /** 追加打开其它目录：只读挂到文件树末尾，不切换项目、不写入最近项目 */
  const pickExternalDirectory = useCallback(async () => {
    if (!isTauriEnvironment()) {
      showStatus(t().status.browserPreviewNoDirPicker);
      return;
    }
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: t().git.openExternalDirTitle
      });
      if (typeof picked !== 'string' || !picked) return;
      if (picked === workspacePath) {
        showStatus(t().status.dirAlreadyRoot);
        return;
      }
      const already = externalRoots.some(r => r.path === picked);
      if (already) {
        setTreeExpanded(prev => new Set(prev).add(picked));
        showStatus(t().status.dirAlreadyInTree);
        return;
      }
      const name = picked.split(/[\\/]/).filter(Boolean).pop() || picked;
      // 登记为只读根：read_file_content 等命令会用它校验「路径在工作区内」，
      // 不登记则外部目录里的文件必然打不开（只能看到目录结构）。
      // readOnly 保证写命令仍只认可写根，外部目录不会变成可写面。
      try {
        await invokeCommand('register_workspace', { path: picked, readOnly: true });
      } catch (err) {
        showStatus(t().status.dirAuthFailed(errText(err)));
        return;
      }
      setExternalRoots(prev => [...prev, { name, path: picked, isDirectory: true, children: [] }]);
      setTreeExpanded(prev => new Set(prev).add(picked));
      try {
        const node = await invokeCommand<FileNode>('read_directory', { path: picked, maxDepth: 2, ignored: settings.ignoredPatterns });
        const children = internFileTree(Array.isArray(node?.children) ? node.children : []);
        setExternalRoots(prev => prev.map(r => (r.path === picked ? { ...r, children } : r)));
      } catch (err) {
        showStatus(t().status.readDirFailed(errText(err)));
      }
    } catch (err) {
      showStatus(t().status.openDirFailed(errText(err)));
    }
  }, [workspacePath, externalRoots]);

  /**
   * 从文件树移除追加的外部目录（不删除磁盘文件）。
   *
   * 移除后文件树里已无该目录的任何节点，其下已打开的标签页必须同时关闭：
   * 留着的话点开 tab 后定位不到节点、刷新刷不到内容、Git 状态也无从关联，
   * 是一整套空转操作。语义与 deletePathsToTrash 关掉受影响 tab 一致
   * （App.tsx 的 closeFilesUnderPaths）。
   */
  const closeExternalRoot = useCallback(
    (path: string) => {
      // 与登记配对：移除后该目录不再是授权根，读命令不再放行
      if (isTauriEnvironment()) {
        void invokeCommand('unregister_workspace', { path }).catch(() => {});
      }
      setExternalRoots(prev => prev.filter(r => r.path !== path));
      setTreeExpanded(prev => {
        const next = new Set(prev);
        next.delete(path);
        for (const p of Array.from(next)) {
          if (p.startsWith(`${path}/`)) next.delete(p);
        }
        return next;
      });
      closeFilesUnderPaths([path]);
      showStatus(t().status.removedFromTree(baseNameOf(path)));
    },
    [closeFilesUnderPaths]
  );

  /** 更新外部目录子树（与项目树 updateFileTreeNode 平行） */
  const updateExternalTreeNode = useCallback((parentPath: string, children: FileNode[]) => {
    const safeChildren = Array.isArray(children) ? children : [];
    const walk = (node: FileNode): FileNode => {
      if (node.path === parentPath) return { ...node, children: safeChildren };
      if (node.isDirectory && Array.isArray(node.children)) {
        return { ...node, children: node.children.map(walk) };
      }
      return node;
    };
    setExternalRoots(prev => prev.map(walk));
  }, []);

  // 设置里的 language 是唯一入口：用户层、项目层、VSCode 导入都走这里。
  // 项目层语言只影响本次会话，不能再写回用户层（persist=false）。
  useEffect(() => {
    if (getLanguage() !== settings.language) setLanguage(settings.language, false);
  }, [settings.language]);

  // 首次挂载：读取持久化设置 + 载入历史项目或保持空状态 + 探测引擎信息
  useEffect(() => {
    const saved = loadSettings();
    setSettings(saved);
    // 字典必须在首帧渲染前就位：useState 的初始值 DEFAULT_SETTINGS.language
    // 与 loadSettings 的结果可能不同（用户上次选了中文），而大量组件在
    // 首次渲染时就会读字典。放在 setSettings 之后、其余效果之前，
    // 让本 effect 内后续的文案读取已是正确语言。
    initLanguage(saved.language);
    document.body.setAttribute('data-variant', String(saved.uiVariant ?? DEFAULT_SETTINGS.uiVariant));
    document.body.classList.toggle('platform-win', saved.captionStyle === 'windows');
    document.body.classList.toggle('platform-mac', saved.captionStyle === 'macos');

    // 读取最近项目记录（historyStore：坏数据给空，旧 key 自动认领迁移）
    setRecentWorkspaces(loadHistory(RECENT_WORKSPACES));

    // 读取本机用过的提交信息（仅记录真实提交过的内容）
    setCommitMessages(loadCommitMessages());

    // 自动打开上次打开过的项目；若从未打开过则停留在空欢迎页
    const lastWorkspace = loadLastWorkspace();
    if (lastWorkspace && lastWorkspace.trim()) {
      void loadWorkspace(lastWorkspace.trim());
    } else {
      setWorkspacePath('');
      setWorkspaceIsFile(false);
      setFileTree([]);
      setOpenFilePaths([]);
      setActiveFilePath('');
    }

    // 引擎版本自省
    void invokeCommand<{ libraryVersion: string; backend: string }>('vt_engine_info')
      .then(info => setEngineInfo(prev => ({ ...prev, engineVersion: info.libraryVersion })))
      .catch(() => undefined);
    void detectGhostty()
      .then(info =>
        setEngineInfo(prev => ({
          ...prev,
          ghosttyVersion: info.installed ? info.version.replace(/^Ghostty\s*/, '') : undefined,
          themeCount: info.theme_count || undefined
        }))
      )
      .catch(() => undefined);
  }, [loadWorkspace]);

  const lastShiftAt = useRef(0);

  const [gitBusy, setGitBusy] = useState(false);
  const [gitMessage, setGitMessage] = useState<string | null>(null);

  /**
   * Git 操作提示（toast）的自动清除定时器。
   *
   * 原有 14 处 `setTimeout(() => setGitMessage(null), N)` 各自为政，两个
   * 问题：句柄全部丢弃，同一时间可能有多个 pending 定时器；用户连续触发
   * 两个操作时，先到的定时器会把后到的提示提前清掉，消息一闪而过。
   *
   * 这里只保留一个句柄：新提示先取消上一个待清除，重新计时；
   * unmount 时清掉，避免组件卸载后回调 setState。
   */
  const gitToastTimerRef = useRef<number | null>(null);

  const showGitToast = useCallback((text: string, ms = 6000) => {
    clearTimeout(gitToastTimerRef.current ?? undefined);
    setGitMessage(text);
    gitToastTimerRef.current = window.setTimeout(() => {
      gitToastTimerRef.current = null;
      setGitMessage(null);
    }, ms);
  }, []);

  // 卸载时清掉待执行的 toast 回调，防止对已卸载组件 setState
  useEffect(() => () => {
    clearTimeout(gitToastTimerRef.current ?? undefined);
  }, []);

  const refreshGit = useCallback(async () => {
    if (!workspacePath) {
      setGitRefreshing(false);
      return;
    }
    const seq = ++gitSeqRef.current;
    if (gitInFlightRef.current) {
      gitPendingRef.current = true;
      setGitRefreshing(true);
      return;
    }
    gitInFlightRef.current = true;
    setGitRefreshing(true);
    try {
      // 先识别是否为 git 仓库：非仓库时清空数据，面板提示并提供 git init
      const info = await invokeCommand<GitRepoInfo>('git_repo_info', { repoPath: workspacePath });
      if (seq !== gitSeqRef.current) return;
      setIsRepo(info.isRepo);
      if (!info.isRepo) {
        gitRepoRootRef.current = '';
        setGitRepoRoot('');
        setCommits([]);
        setBranches([]);
        setChangedFiles([]);
        setSequencer(null);
        setCurrentBranch('');  // 非 git 仓库不显示分支名
        setHasMoreCommits(false);
        logIpc('目标目录不是 git 仓库，跳过 git 数据读取');
        return;
      }

      // 仓库根可能与工作区不同（打开的是仓库子目录）。所有 Git 子进程与
      // status/diff 的仓库相对路径都以它为准；文件树仍用 workspacePath。
      const root = info.root || workspacePath;
      gitRepoRootRef.current = root;
      setGitRepoRoot(root);
      // 仓库根可能在已授权工作区之外（子目录项目）：登记为只读根，保证
      // diff 读取仓库上层文件不会因授权失败而显示空内容。
      if (root !== workspacePath) {
        void invokeCommand('register_workspace', { path: root, readOnly: true }).catch(() => {});
      }

      const [log, brs, status, seqState, tagList] = await Promise.all([
        invokeCommand<GitCommit[]>('git_log', {
          repoPath: root,
          maxCount: LOG_PAGE_SIZE,
          allRefs: !logBranch,
          branch: logBranch || null,
          path: logPath || null,
          author: logUser || null,
          since: logSinceOf(logDate),
          topoOrder: logSortMode === 'topo',
          firstParent: logFirstParent,
          noMerges: logNoMerges
        }),
        invokeCommand<GitBranch[]>('git_branches', { repoPath: root }),
        invokeCommand<GitChangedFile[]>('git_status', { repoPath: root }),
        // sequencer 状态（MERGE_HEAD/CHERRY_PICK_HEAD）：本地 rev-parse + 三次
        // stat，约毫秒级。冲突横幅/对话框的 Abort/Continue 全靠它亮灭。
        invokeCommand<SequencerState>('git_sequencer_state', { repoPath: root }).catch(() => null),
        invokeCommand<GitTag[]>('git_list_tags', { repoPath: root }).catch(() => [])
      ]);
      // 序号已被更新：期间又有新请求发起，这批数据是旧的，丢弃
      if (seq !== gitSeqRef.current) return;
      setCommits(log);
      setTags(tagList);
      setHasMoreCommits(log.length >= LOG_PAGE_SIZE);
      setSequencer(seqState);
      if (brs.length) {
        setBranches(brs);
        const cur = brs.find(b => b.isCurrent);
        // detached HEAD 没有 current 分支：必须清掉上一个分支名，不能继续
        // 把 main 显示成当前分支（Push 会把提交推到错误分支）。
        setCurrentBranch(cur ? cur.name : '');
      } else {
        setCurrentBranch(info.branch || 'main');
      }
      setChangedFiles(status);
      const head = brs.find(b => b.isCurrent)?.name ?? info.branch;
      logIpc(
        t().git.gitDataRefreshed(`${brs.length} branches / ${log.length} commits / ${status.length} changed files / HEAD '${head}'`)
      );
    } catch (err) {
      if (seq !== gitSeqRef.current) return;
      const message = errText(err);
      setChangedFiles([]);
      setSequencer(null);
      // 失败时不能留着上一个项目的 root：后续 commit/reset 会打错仓库。
      gitRepoRootRef.current = '';
      setGitRepoRoot('');
      logIpc(`刷新 Git 数据失败：${message}`, 'error');
      showStatus(t().status.gitDataReadFailed(message));
    } finally {
      gitInFlightRef.current = false;
      // 在途期间有新请求进来（含切换项目时的作废重置）：用最新 refreshGit 补跑，
      // 不能沿用本闭包——deps 里的 workspacePath 可能已是旧项目。
      if (gitPendingRef.current) {
        gitPendingRef.current = false;
        const rerun = refreshGitRef.current;
        if (rerun) {
          rerun();
        } else {
          setGitRefreshing(false);
        }
      } else {
        setGitRefreshing(false);
      }
    }
  }, [workspacePath, logBranch, logPath, logUser, logDate, logSortMode, logFirstParent, logNoMerges]);
  refreshGitRef.current = () => void refreshGit();

  /** 递增 diff 刷新键：提交/回滚/贮藏等操作改写工作区后，已打开的 diff 需重算 */
  const bumpDiff = useCallback(() => setDiffRefreshKey(k => k + 1), []);

  /** 变更列表随工作区加载；操作命令都返回最新 DTO，这里只负责初始读取 */
  useEffect(() => {
    if (!workspacePath || !isRepo) return;
    let cancelled = false;
    invokeCommand<Changelists>('changelists_load', { repoPath: gitRepoRoot || workspacePath })
      .then(list => {
        if (!cancelled) setChangelists(list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [workspacePath, isRepo, gitRepoRoot]);

  /** 滚动到底加载更早的提交（按 skip 分页） */
  const loadMoreCommits = useCallback(async () => {
    if (!workspacePath || logLoadingMore || !isRepo) return;
    // 与 refreshGit 共用同一序号：切分支 / 切路径 / 切项目都会递增它，
    // 在途的旧页回来时序号已变，必须整页丢弃 —— 否则别的分支的提交会被
    // append 进当前列表（hash 去重挡不住错源），分页起点也随之错位。
    const seq = gitSeqRef.current;
    setLogLoadingMore(true);
    try {
      const more = await invokeCommand<GitCommit[]>('git_log', {
        repoPath: resolveGitRoot(),
        maxCount: LOG_PAGE_SIZE,
        skip: commits.length,
        allRefs: !logBranch,
        branch: logBranch || null,
        path: logPath || null,
        author: logUser || null,
        since: logSinceOf(logDate),
        topoOrder: logSortMode === 'topo',
        firstParent: logFirstParent,
        noMerges: logNoMerges
      });
      if (seq !== gitSeqRef.current) return;
      if (more.length) {
        setCommits(prev => [...prev, ...more.filter(m => !prev.some(p => p.hash === m.hash))]);
      }
      setHasMoreCommits(more.length >= LOG_PAGE_SIZE);
    } catch (err) {
      logIpc(`加载更早提交失败：${errText(err)}`, 'error');
    } finally {
      // 无论是否被丢弃都要复位：卡住 true 会让后续所有分页失效
      setLogLoadingMore(false);
    }
  }, [workspacePath, logLoadingMore, isRepo, commits.length, logBranch, logPath, logUser, logDate, logSortMode, logFirstParent, logNoMerges]);

  /** 在当前目录初始化 git 仓库 */
  const handleInitRepo = useCallback(async () => {
    if (!workspacePath) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_init', { repoPath: workspacePath });
      showStatus(t().status.gitRepoInitialized);
      await refreshGit();
    } catch (err) {
      showStatus(t().status.gitInitFailed(errText(err)));
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  // 切换分支（远程分支名如 origin/feat 由后端 DWIM 检出并建立跟踪）
  const handleSelectBranch = useCallback(async (branchName: string) => {
    if (!workspacePath) return;
    try {
      await invokeCommand('git_checkout', { repoPath: resolveGitRoot(), branch: branchName });
      await refreshGit();
    } catch (err) {
      const message = errText(err);
      showGitToast(t().git.branchSwitchFailed(message), 8000);
    }
  }, [workspacePath, refreshGit]);

  /** Checkout Tag：轻量/附注标签都按 detached HEAD 检出。 */
  const handleCheckoutTag = useCallback(async (name: string) => {
    if (!workspacePath) return;
    if (!(await appConfirm(t().dialog.checkoutTagConfirm(name), {
      title: t().dialog.checkoutTagTitle,
      kind: 'warning',
      okLabel: t().dialog.checkoutTagOk
    }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_checkout', { repoPath: resolveGitRoot(), branch: name });
      showGitToast(t().git.checkedOutTag(name), 5000);
      await refreshGit();
    } catch (err) {
      showGitToast(t().git.checkoutFailed(errText(err)), 8000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  const handleDeleteTag = useCallback(async (tag: GitTag) => {
    if (!workspacePath) return;
    if (!(await appConfirm(t().dialog.deleteTagConfirm(tag.name), {
      title: t().dialog.deleteTagTitle,
      kind: 'warning',
      okLabel: t().dialog.deleteTagOk
    }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_delete_tag', { repoPath: resolveGitRoot(), name: tag.name });
      showGitToast(t().git.tagDeleted(tag.name), 5000);
      await refreshGit();
    } catch (err) {
      showGitToast(t().git.tagDeleteFailed(errText(err)), 7000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  /** Checkout Tag or Revision...：输入任意 ref 后检出（detached HEAD 需确认）。 */
  const handleCheckoutRefPrompt = useCallback(async () => {
    if (!workspacePath) return;
    const ref = await appPrompt(t().git.checkoutRefPrompt, '');
    if (!ref || !ref.trim()) return;
    const target = ref.trim();
    if (!(await appConfirm(t().dialog.checkoutRefConfirm(target), {
      title: t().dialog.checkoutRefTitle,
      kind: 'warning',
      okLabel: t().dialog.checkoutRefOk
    }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_checkout', { repoPath: resolveGitRoot(), branch: target });
      showGitToast(t().git.checkedOutRef(target), 5000);
      await refreshGit();
    } catch (err) {
      showGitToast(t().git.checkoutFailed(errText(err)), 8000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit, appPrompt]);

  // 创建并切换新分支
  const handleCreateBranch = useCallback(async (branchName: string) => {
    if (!workspacePath) return;
    try {
      await invokeCommand('git_create_branch', { repoPath: resolveGitRoot(), branch: branchName });
      await refreshGit();
    } catch (err) {
      const message = errText(err);
      showGitToast(t().git.branchCreateFailed(message), 8000);
    }
  }, [workspacePath, refreshGit]);

  // Git 数据随工作区载入；gitRefreshTick：切换/重开时强制重跑（path 可能未变）
  useEffect(() => {
    void refreshGit();
  }, [refreshGit, gitRefreshTick]);

  // 编辑内容变化后防抖自动刷新 Git 状态（配合自动保存，对齐 PyCharm 的即时反馈）。
  // 2500ms 而非 1500ms：git_status 要 spawn 3 个 git 子进程（1 次 status +
  // 1 次合并 diff），连续输入时每个停顿都触发一轮，容易和输入节奏共振。
  // 现在有 single-flight 兜底，防抖再长一点也不会丢最终状态。
  useEffect(() => {
    if (!workspacePath || !isRepo) return;
    const timer = setTimeout(() => {
      void refreshGit();
    }, 2500);
    return () => clearTimeout(timer);
  }, [codeContent, workspacePath, isRepo, refreshGit]);

  // 窗口重新聚焦时刷新（外部 git 操作 / 分支切换后的同步）
  useEffect(() => {
    const onFocus = () => {
      if (workspacePath && isRepo) void refreshGit();
    };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [workspacePath, isRepo, refreshGit]);
  /**
   * 打开 Diff 页：标记标签存在并激活。
   * 所有进入 diff 的入口都走这里，避免漏掉 diffOpen 让标签栏丢掉 Diff 标签。
   * returnTo = 关闭 Diff 后回到哪个主标签；Compare Branches 打开文件时传
   * 'compare-branches'，其余入口默认回编辑器。
   */
  const diffReturnTabRef = useRef<'editor' | 'compare-branches'>('editor');
  const enterDiff = useCallback((returnTo: 'editor' | 'compare-branches' = 'editor') => {
    diffReturnTabRef.current = returnTo;
    setDiffOpen(true);
    setActiveTab('diff');
  }, []);

  /**
   * 所有进入 Diff 的入口先清空互斥的差异来源。
   * diffCommit / againstRef / compareCommits / fsFiles 在 DiffViewer 里按优先级
   * 只认第一个非空值：若只设置新来源而不清旧来源，旧的 Compare Branches 范围
   * 会盖住刚从工作区打开的文件，表现为「该提交没有文本差异」。
   */
  const clearDiffSource = useCallback(() => {
    setDiffCommit(null);
    setDiffAgainstRef(null);
    setDiffCompareCommits(null);
    setDiffSelectedCommits(null);
    setFsCompareFiles(null);
  }, []);

  const handleSelectFileForDiff = (path: string, staged = false) => {
    const abs = toWorkspaceAbsPath(resolveGitRoot() || workspacePath, path);
    clearDiffSource();
    setDiffFilePath(abs);
    setDiffStaged(staged);
    // 左侧 Commit 列表在 Compare Branches 页上打开 diff 时，关闭应回到比较页，
    // 而不是回到没有打开文件的空编辑器。
    enterDiff(activeTab === 'compare-branches' ? 'compare-branches' : 'editor');
  };

  useEffect(() => {
    document.body.setAttribute('data-variant', String(settings.uiVariant ?? DEFAULT_SETTINGS.uiVariant));
  }, [settings.uiVariant]);


  /** 界面字号：启动与设置变更时都走这里（含首次挂载） */
  useEffect(() => {
    applyUiFontSize(settings.uiFontSize);
  }, [settings.uiFontSize]);
  const handleSelectFile = (path: string) => {
    // 树里的文件点击：目标行已经在眼前，不需要反向定位；
    // 且无条件切回代码编辑器（对齐 PyCharm：绝不在 Diff 视口中用文件树篡改比对目标）
    tabActivationRef.current = null;
    setActiveTab('editor');
    void openFile(path);
  };

  /** 在 Diff 面板中查看某个提交（或提交中的某文件） */
  const openCommitDiff = useCallback((commit: string, path = '') => {
    clearDiffSource();
    setDiffCommit(commit);
    setDiffFilePath(path);
    setDiffStaged(false);
    enterDiff();
  }, [clearDiffSource, enterDiff]);

  /** 底部日志多选提交：只合并显式选中的提交，不包含中间未选提交。 */
  const openCommitsDiff = useCallback((commits: string[], path = '') => {
    clearDiffSource();
    setDiffSelectedCommits(commits);
    setDiffFilePath(path);
    setDiffStaged(false);
    enterDiff();
  }, [clearDiffSource, enterDiff]);

  /** 提交内文件 ↔ 工作区比较（Compare with Local） */
  const openCompareWithLocal = useCallback((commit: string, path: string) => {
    clearDiffSource();
    setDiffAgainstRef(commit);
    setDiffStaged(false);
    setDiffFilePath(path);
    enterDiff();
  }, [clearDiffSource, enterDiff]);

  // 进入 Diff 前正在编辑的文件：关闭 Diff 时恢复。
  // Diff 的主体文件（提交内文件 / 树右键 Show Diff 等）通常不在标签页里，
  // 若关闭后仍把 activeFilePath 留在它身上，编辑器会渲染成没有缓冲的空页面。
  const lastEditorPathRef = useRef('');
  useEffect(() => {
    if (activeTab === 'editor' && activeFilePath && openFilePaths.includes(activeFilePath)) {
      lastEditorPathRef.current = activeFilePath;
    }
  }, [activeTab, activeFilePath, openFilePaths]);

  // monaco 兜底预取：编辑器页可见时就应开始加载。
  // openFile 覆盖了绝大多数入口，但会话恢复 / 标签页直接选中等路径不经过它；
  // 这里按「编辑器页确实要渲染」判定，漏掉的路径也能拿到就绪状态。
  useEffect(() => {
    if (monacoReady) return;
    if (activeTab !== 'editor' || !activeFilePath) return;
    if (!openFilePaths.includes(activeFilePath)) return;
    if (isImageFile(activeFilePath)) return;
    requestMonaco(themeIdRef.current);
  }, [monacoReady, activeTab, activeFilePath, openFilePaths, requestMonaco]);

  /** 关闭 Diff 页：回到进入 Diff 之前正在编辑的文件（对齐 PyCharm）。
   *  useStableCallback：它是 EditorTabBar（memo）的 onCloseDiffTab ；
   *  依赖 openFilePaths 会让每次标签开关都重建引用，击穿 memo。 */
  const closeDiffView = useStableCallback(() => {
    const returnTo =
      diffReturnTabRef.current === 'compare-branches' && compareBranches
        ? 'compare-branches'
        : 'editor';
    setActiveTab(returnTo);
    setDiffOpen(false);
    setDiffCommit(null);
    setDiffAgainstRef(null);
    setDiffCompareCommits(null);
    setDiffSelectedCommits(null);
    setFsCompareFiles(null);
    setDiffFilePath('');
    diffReturnTabRef.current = 'editor';
    // Diff 是 Compare Branches 里双击文件打开的：关闭后回到那条比较日志，
    // 不应改写编辑器 activeFilePath（它通常不是被 diff 的提交内文件）。
    if (returnTo === 'compare-branches') return;
    const prev = lastEditorPathRef.current;
    const open = openFilePathsRef.current;
    if (prev && open.includes(prev)) {
      // 进入 Diff 时 activeFilePath 可能已被改成提交内文件/仓库相对路径
      // （Commit 抽屉的 changedFiles 是相对路径）。这里必须显式恢复，
      // 否则关闭 Diff 后编辑器拿着一个没有 buffer 的路径：语言回落
      // plaintext、内容为空，只剩 git hunk 的绿色行背景。
      setActiveFilePath(prev);
      if (!buffersRef.current[prev]) void openFile(prev);
    } else if (open.length > 0) {
      const target = open[0];
      setActiveFilePath(target);
      if (!buffersRef.current[target]) void openFile(target);
    } else if (compareBranches) {
      // 没有可恢复的编辑器文件，但 Compare Branches 还开着：不要停在空页面。
      setActiveTab('compare-branches');
      setActiveFilePath('');
    } else {
      setActiveFilePath('');
    }
  });

  // ── EditorTabBar：稳定回调（标签栏不随缓冲内容变化，回调重建即击穿 memo）──
  const selectTabStable = useStableCallback((path: string) => {
    // 只有这里的激活会触发自动定位（见 locate 的 effect）
    tabActivationRef.current = { path, at: performance.now() };
    setActiveTab('editor');
    if (path === activeFilePath) {
      // 点击已激活的标签不会改变 activeFilePath，effect 不会触发；
      // 但用户仍期望这次点击定位。直接调用最新定位实现。
      if (autoLocateTree) void locateActiveRef.current(true);
      return;
    }
    void openFile(path);
  });
  const selectDiffTabStable = useStableCallback(() => setActiveTab('diff'));
  const selectCompareBranchesStable = useStableCallback(() => setActiveTab('compare-branches'));

  /** rememberCommitMessage 已随 hook 迁出（hooks/useCommitMessageHistory）。 */

  const handleCommit = async (msg: string, isAmend: boolean, paths: string[] = []) => {
    // amend 允许空消息：后端用 --no-edit 保留原提交信息。
    if (!msg.trim() && !isAmend) {
      showGitToast(t().git.commitMessageEmpty);
      return;
    }
    setGitBusy(true);
    const committableCount = changedFiles.filter(f => f.status !== 'I').length;
    try {
      await invokeCommand<string>('git_commit', {
        repoPath: resolveGitRoot() || null,
        message: msg,
        amend: isAmend,
        // 仅提交勾选的文件；全部勾选时传 null 由后端提交全部变更
        paths: paths.length > 0 && paths.length < committableCount ? paths : null
      });
      showGitToast(t().git.committed(msg), 4000);
      rememberCommitMessage(msg);
      await refreshGit();
      bumpDiff();
      return true;
    } catch (err) {
      showGitToast(t().git.commitFailed(errText(err)), 4000);
      return false;
    } finally {
      setGitBusy(false);
    }
  };

  const handleCommitAndPush = async (msg: string, isAmend: boolean, paths: string[] = []) => {
    if (!msg.trim() && !isAmend) {
      showGitToast(t().git.commitMessageEmpty);
      return;
    }
    setGitBusy(true);
    const committableCount = changedFiles.filter(f => f.status !== 'I').length;
    try {
      await invokeCommand<string>('git_commit', {
        repoPath: resolveGitRoot() || null,
        message: msg,
        amend: isAmend,
        paths: paths.length > 0 && paths.length < committableCount ? paths : null
      });
      rememberCommitMessage(msg);
      await refreshGit();
      bumpDiff();
      // 对齐 PyCharm：Commit and Push 打开 Push 对话框确认待推提交，而不是静默推送
      setPushDialogOpen(true);
      return true;
    } catch (err) {
      showGitToast(t().git.commitFailed(errText(err)), 4000);
      return false;
    } finally {
      setGitBusy(false);
    }
  };

  /** Push 对话框推送成功：关闭对话框、刷新数据并提示 */
  const handlePushed = useCallback(
    async (output: string) => {
      setPushDialogOpen(false);
      const firstLine = output.trim().split('\n').find(l => l.trim()) ?? '';
      showGitToast(t().git.pushSucceeded(firstLine ? firstLine.trim() : ''), 4000);
      await refreshGit();
    },
    [refreshGit, showGitToast]
  );

  /** Update Project：git pull（merge 或 rebase 策略由对话框选择） */
  const handleUpdateProject = useCallback(
    async (rebase: boolean) => {
      if (!workspacePath) return;
      setGitBusy(true);
      try {
        const out = await invokeCommand<string>('git_pull', { repoPath: resolveGitRoot(), rebase });
        setPullDialogOpen(false);
        const firstLine = out.trim().split('\n').find(l => l.trim()) ?? '';
        showGitToast(
          firstLine ? t().git.updateDone(firstLine.trim()) : rebase ? t().git.updateDoneRebase : t().git.updateDoneMerge,
          8000
        );
        await refreshGit();
      } catch (err) {
        const message = errText(err);
        setPullDialogOpen(false);
        showGitToast(t().git.updateFailed(message), 8000);
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit, showGitToast]
  );

  /** Fetch：获取远程更新但不整合 */
  const handleFetch = useCallback(async () => {
    if (!workspacePath) return;
    setGitBusy(true);
    try {
      await invokeCommand<string>('git_fetch', { repoPath: resolveGitRoot() });
      showGitToast(t().git.fetchDone, 4000);
      await refreshGit();
    } catch (err) {
      showGitToast(t().git.fetchFailed(errText(err)), 4000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit, showGitToast]);

  // ---- 分支右键菜单动作（对齐 PyCharm 分支右键）----
  /** toggleFavoriteBranch 已随 hook 迁出（hooks/useFavoriteBranches）。 */

  /** 从选定 ref 创建并切换新分支（New Branch from Selected） */
  const handleCreateBranchFrom = useCallback(
    async (refName: string) => {
      const name = await appPrompt(t().git.branchCreatedPrompt(refName));
      if (!name || !name.trim()) return;
      if (!workspacePath) return;
      try {
        await invokeCommand('git_create_branch', { repoPath: resolveGitRoot(), branch: name.trim(), startPoint: refName });
        showGitToast(t().git.branchCreatedFrom(refName, name.trim()), 6000);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.branchCreateFailed(errText(err)), 6000);
      }
    },
    [workspacePath, refreshGit, showGitToast, appPrompt]
  );

  /** 重命名本地分支 */
  const handleRenameBranch = useCallback(
    async (oldName: string) => {
      const newName = await appPrompt(t().git.branchRenamePrompt(oldName), oldName);
      if (!newName || !newName.trim() || newName.trim() === oldName) return;
      if (!workspacePath) return;
      try {
        await invokeCommand('git_rename_branch', { repoPath: resolveGitRoot(), oldName, newName: newName.trim() });
        showGitToast(t().git.branchRenamed(oldName, newName.trim()), 6000);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.branchRenameFailed(errText(err)), 6000);
      }
    },
    [workspacePath, refreshGit, showGitToast, appPrompt]
  );

  /** 删除分支：本地未合并时二次确认后强删；远程分支删除走 push --delete */
  const handleDeleteBranch = useCallback(
    async (branch: GitBranch) => {
      if (!workspacePath) return;
      if (branch.isRemote) {
        if (!(await appConfirm(t().dialog.deleteRemoteBranchConfirm(branch.name), { title: t().dialog.deleteRemoteBranchTitle, kind: 'warning', okLabel: 'Delete' }))) return;
      } else if (!(await appConfirm(t().dialog.deleteBranchConfirm(branch.name), { title: t().dialog.deleteBranchTitle, kind: 'warning', okLabel: 'Delete' }))) return;

      setGitBusy(true);
      try {
        await invokeCommand('git_delete_branch', { repoPath: resolveGitRoot(), name: branch.name, isRemote: branch.isRemote, force: false });
        showGitToast(t().git.branchDeleted(branch.name), 6000);
        await refreshGit();
      } catch (err) {
        const message = errText(err);
        // -d 拒绝未合并分支：二次确认后强制删除
        if (!branch.isRemote && message.includes('not fully merged')) {
          if (await appConfirm(t().dialog.forceDeleteBranchConfirm(branch.name), { title: t().dialog.forceDeleteBranchTitle, kind: 'warning', okLabel: 'Delete' })) {
            try {
              await invokeCommand('git_delete_branch', { repoPath: resolveGitRoot(), name: branch.name, isRemote: false, force: true });
              showGitToast(t().git.branchForceDeleted(branch.name), 6000);
              await refreshGit();
            } catch (err2) {
              showGitToast(t().git.branchDeleteFailed(errText(err2)), 6000);
            }
          }
        } else {
          showGitToast(t().git.branchDeleteFailed(message), 6000);
        }
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit, showGitToast]
  );

  /** 分支树工具栏：新建分支（先输入名字，再复用 TitleBar 的创建逻辑） */
  const handleCreateBranchPrompt = useCallback(async () => {
    const name = await appPrompt(t().git.branchCreatedPrompt(currentBranch || 'HEAD'));
    if (name?.trim()) await handleCreateBranch(name.trim());
  }, [appPrompt, currentBranch, handleCreateBranch]);

  /** Show Files Diff / Compare Versions 的文件差异抽屉：把两个 ref 当作“提交”
   *  交给 CompareDrawer，底层 git diff A B 本就接受 ref 名。 */
  const handleShowFilesDiff = useCallback((a: GitBranch, b: GitBranch) => {
    const asRefCommit = (ref: GitBranch): GitCommit => ({
      hash: ref.name,
      shortHash: ref.name,
      message: '',
      author: '',
      authorEmail: '',
      date: '',
      graphTrack: 0
    });
    setCompareSession({ fromCommit: asRefCommit(b), toCommit: asRefCommit(a) });
    setActiveDrawer('compare');
  }, []);

  /** Compare Branches：打开中央比较标签页（两条独立提交日志），不是文件抽屉。 */
  const handleCompareBranches = useCallback((a: GitBranch, b: GitBranch) => {
    setCompareBranches({ left: a, right: b });
    setActiveTab('compare-branches');
    // 旧的 Compare Versions 抽屉与本次比较无关，避免左侧同时挂着一个文件清单。
    setActiveDrawer(prev => (prev === 'compare' ? 'project' : prev));
  }, []);

  const handleSwapCompareBranches = useCallback(() => {
    setCompareBranches(prev =>
      prev ? { left: prev.right, right: prev.left } : prev
    );
  }, []);

  const handleCloseCompareBranches = useCallback(() => {
    setCompareBranches(null);
    // 如果 Diff 是由这条比较打开且正停在 Diff 标签上，关闭比较后让它回编辑器。
    diffReturnTabRef.current = 'editor';
    setActiveTab(prev => (prev === 'compare-branches' ? 'editor' : prev));
  }, []);

  /** Compare Branches 文件双击：单提交走 git show，整侧走 merge-base..side。 */
  const openCompareBranchesCommitDiff = useCallback(
    (commit: string, path = '') => {
      clearDiffSource();
      setDiffCommit(commit);
      setDiffStaged(false);
      setDiffFilePath(path);
      enterDiff('compare-branches');
    },
    [clearDiffSource, enterDiff]
  );

  const openCompareBranchesRangeDiff = useCallback(
    (from: string, to: string, path = '') => {
      clearDiffSource();
      setDiffCompareCommits({ from, to });
      setDiffStaged(false);
      setDiffFilePath(path);
      enterDiff('compare-branches');
    },
    [clearDiffSource, enterDiff]
  );

  /** Update Selected：一次 fetch，刷新 ahead/behind 与远端分支。 */
  const handleUpdateSelected = useCallback(async (refs: GitBranch[]) => {
    if (!workspacePath || refs.length === 0) return;
    setGitBusy(true);
    try {
      await invokeCommand<string>('git_fetch', { repoPath: resolveGitRoot() });
      showGitToast(t().git.fetchDone, 4000);
      await refreshGit();
    } catch (err) {
      showGitToast(t().git.fetchFailed(errText(err)), 6000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit, showGitToast]);

  /** Delete Branches：一次确认后逐项删除；单项失败不阻断其余项。 */
  const handleDeleteSelected = useCallback(async (refs: GitBranch[]) => {
    if (!workspacePath || refs.length === 0) return;
    const names = refs.map(r => r.name).join('\n');
    if (!(await appConfirm(t().dialog.deleteRefsConfirm(names), {
      title: t().dialog.deleteRefsTitle,
      kind: 'warning',
      okLabel: t().branchMenu.deletePlain
    }))) return;
    setGitBusy(true);
    const failures: string[] = [];
    let deleted = 0;
    try {
      for (const ref of refs) {
        try {
          if (ref.isTag) {
            await invokeCommand('git_delete_tag', { repoPath: resolveGitRoot(), name: ref.name });
          } else {
            await invokeCommand('git_delete_branch', {
              repoPath: resolveGitRoot(),
              name: ref.name,
              isRemote: ref.isRemote,
              force: false
            });
          }
          deleted++;
        } catch (err) {
          failures.push(`${ref.name}: ${errText(err)}`);
        }
      }
      if (failures.length) showGitToast(t().dialog.refsDeleteFailed(failures.join('; ')), 9000);
      else showGitToast(t().dialog.refsDeleted(deleted), 5000);
      await refreshGit();
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit, showGitToast]);

  /** Checkout and Rebase onto：先检出 ref，再把新分支变基到原当前分支。 */
  const handleCheckoutAndRebase = useCallback(async (refName: string, current: string) => {
    if (!workspacePath) return;
    if (!(await appConfirm(t().dialog.checkoutAndRebaseConfirm(refName, current), {
      title: t().dialog.checkoutAndRebaseTitle,
      okLabel: 'Checkout and Rebase'
    }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_checkout', { repoPath: resolveGitRoot(), branch: refName });
      await invokeCommand('git_rebase', { repoPath: resolveGitRoot(), refName: current });
      showGitToast(t().git.rebased(refName, current), 6000);
      await refreshGit();
    } catch (err) {
      const message = errText(err);
      showGitToast(
        message.includes('CONFLICT') ? t().git.rebaseConflictHint(message) : t().git.rebaseFailed(message),
        10000
      );
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit, showGitToast]);

  /** 标签右键 Push to origin：直接推送该标签 ref。 */
  const handlePushTag = useCallback(async (name: string) => {
    if (!workspacePath) return;
    setGitBusy(true);
    try {
      const out = await invokeCommand<string>('git_push', {
        repoPath: resolveGitRoot(),
        branch: name
      });
      const firstLine = out.trim().split('\n').find(l => l.trim()) ?? '';
      showGitToast(t().git.pushSucceeded(firstLine), 5000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 8000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit, showGitToast]);

  // ---- 提交右键菜单动作（对齐 PyCharm Log 右键）----

  /** 复制提交哈希 */
  const handleCopyHash = useCallback((commit: GitCommit) => {
    navigator.clipboard
      ?.writeText(commit.hash)
      .then(() => showGitToast(t().git.hashCopied(commit.shortHash), 3000))
      .catch(() => showGitToast(t().git.copyFailed, 3000));
  }, [showGitToast]);

  /** 检出某个提交（detached HEAD） */
  const handleCheckoutRevision = useCallback(
    async (commit: GitCommit) => {
      if (!workspacePath) return;
      if (!(await appConfirm(t().dialog.checkoutCommitConfirm(commit.shortHash), { title: t().dialog.checkoutCommitTitle, okLabel: 'Checkout' }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_checkout', { repoPath: resolveGitRoot(), branch: commit.hash });
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.checkoutFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit]
  );

  /** Cherry-Pick 提交到当前分支 */
  const handleCherryPick = useCallback(
    async (commit: GitCommit) => {
      if (!workspacePath) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_cherry_pick', { repoPath: resolveGitRoot(), hash: commit.hash });
        showGitToast(t().git.cherryPicked(commit.shortHash));
        await refreshGit();
      } catch (err) {
        const message = errText(err);
        showGitToast(
          message.includes('CONFLICT')
            ? t().git.cherryPickConflictHint(message)
            : t().git.cherryPickFailed(message),
          10000
        );
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit]
  );

  /** 在提交上创建标签：输入名称 + 可选描述（有描述则为附注标签） */
  const handleNewTag = useCallback(
    async (commit: GitCommit) => {
      const name = await appPrompt(t().git.tagCreatePrompt(commit.shortHash));
      if (!name || !name.trim()) return;
      const message = (await appPrompt(t().git.tagMessagePrompt, undefined, { allowEmpty: true })) ?? '';
      if (!workspacePath) return;
      try {
        await invokeCommand('git_create_tag', {
          repoPath: resolveGitRoot(),
          name: name.trim(),
          commit: commit.hash,
          message: message.trim() || null
        });
        showGitToast(t().git.tagCreated(name.trim()));
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.tagCreateFailed(errText(err)));
      }
    },
    [workspacePath, refreshGit, appPrompt]
  );

  /** Undo Commit：软重置 HEAD~1，更改保留在暂存区 */
  const handleUndoCommit = useCallback(
    async (commit: GitCommit) => {
      if (!workspacePath) return;
      if (!(await appConfirm(t().dialog.undoCommitConfirm(commit.shortHash), { title: t().dialog.undoCommitTitle, okLabel: 'Undo' }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_reset', { repoPath: resolveGitRoot(), commit: 'HEAD~1', mode: 'soft' });
        showGitToast(t().git.undoCommitDone(commit.shortHash));
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.undoCommitFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit]
  );

  /** Reset Current Branch to Here：对话框选择模式后执行 */
  const handleResetConfirm = useCallback(
    async (mode: ResetMode) => {
      if (!workspacePath || !resetTarget) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_reset', { repoPath: resolveGitRoot(), commit: resetTarget.hash, mode });
        showGitToast(t().git.resetDone(currentBranch, resetTarget.shortHash, mode));
        setResetTarget(null);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.resetFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, resetTarget, currentBranch, refreshGit]
  );

  // ---- 交互式变基 / Shelve / 冲突解决 / 提交导航 ----

  /** 交互式变基完成 */
  const handleRebaseDone = useCallback(
    async (message: string) => {
      setRebaseBase(null);
      showGitToast(message);
      await refreshGit();
    },
    [refreshGit]
  );

  /** Shelve Changes...：把勾选文件（可多个，含未跟踪）存为补丁并还原工作区 */
  const handleShelve = useCallback(
    async (paths: string[]) => {
      if (!workspacePath || paths.length === 0) return;
      const defName = paths.length === 1 ? paths[0].split(/[\\/]/).pop() ?? 'shelf' : 'Changes';
      const name = await appPrompt(t().git.stashNamePrompt, defName);
      if (!name || !name.trim()) return;
      setGitBusy(true);
      try {
        const patch = await invokeCommand<string>('git_diff_patch_paths', { repoPath: resolveGitRoot(), paths });
        if (!patch.trim()) {
          showGitToast(t().git.stashNoChanges);
          return;
        }
        await invokeCommand('git_shelve_save', { repoPath: resolveGitRoot(), name: name.trim(), patch });
        for (const p of paths) {
          try {
            await invokeCommand('git_discard_file_changes', { repoPath: resolveGitRoot(), path: p });
            // 贮藏后文件回到 HEAD：新增文件关闭标签页；其余丢弃缓冲，激活中的立即重读
            // （标签/缓冲按绝对路径为键，与变更清单的相对路径换算）
            const abs = toWorkspaceAbsPath(resolveGitRoot(), p);
            const rec = changedFiles.find(f => f.path === p);
            if (rec && (rec.status === 'A' || rec.status === 'U')) {
              releaseBuffers([abs]);
              setOpenFilePaths(prev => prev.filter(x => x !== abs));
              setActiveFilePath(cur => (cur === abs ? '' : cur));
            } else {
              dropBuffer(abs);
              if (abs === activeFilePath) await openFile(abs);
            }
          } catch {
            /* 单个文件回滚失败不阻断其余文件 */
          }
        }
        showGitToast(t().git.stashDone(name.trim()));
        await refreshGit();
        bumpDiff();
      } catch (err) {
        showGitToast(t().git.stashFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, changedFiles, activeFilePath, refreshGit, releaseBuffers, dropBuffer, openFile, bumpDiff, showGitToast, appPrompt]
  );

  /** Unshelve：应用贮藏（可只勾选部分文件），并把文件指派到所选变更列表（不存在则创建） */
  const handleUnshelveToChangelist = useCallback(
    async (shelfName: string, paths: string[], target: string) => {
      if (!workspacePath || paths.length === 0) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_apply_shelf_partial', { repoPath: resolveGitRoot(), name: shelfName, paths });
        if (target !== 'Changes' && !changelists.lists.includes(target)) {
          const created = await invokeCommand<Changelists>('changelist_create', { repoPath: resolveGitRoot(), name: target });
          setChangelists(created);
        }
        const dto = await invokeCommand<Changelists>('changelist_assign', { repoPath: resolveGitRoot(), paths, list: target });
        setChangelists(dto);
        // 应用补丁后磁盘内容变化：打开中的标签页丢弃缓冲（键为绝对路径），激活中的立即重读
        for (const p of paths) {
          dropBuffer(toWorkspaceAbsPath(resolveGitRoot(), p));
        }
        if (paths.some(p => toWorkspaceAbsPath(resolveGitRoot(), p) === activeFilePath)) await openFile(activeFilePath);
        showGitToast(t().git.unstashDone(shelfName, target, paths.length));
        await refreshGit();
        bumpDiff();
      } catch (err) {
        showGitToast(t().git.unstashFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, changelists, activeFilePath, refreshGit, dropBuffer, openFile, bumpDiff, showGitToast]
  );

  /** 新建变更列表 */
  const handleCreateChangelist = useCallback(
    async (name: string) => {
      if (!workspacePath) return;
      try {
        const dto = await invokeCommand<Changelists>('changelist_create', { repoPath: resolveGitRoot(), name });
        setChangelists(dto);
        showGitToast(t().git.changelistCreated(name));
      } catch (err) {
        showGitToast(t().git.changelistCreateFailed(errText(err)));
      }
    },
    [workspacePath, showGitToast]
  );

  /** 删除变更列表：rollback=true 还原其下文件，否则移回默认列表 Changes */
  const handleDeleteChangelist = useCallback(
    async (name: string, rollback: boolean) => {
      if (!workspacePath) return;
      try {
        const affected = Object.entries(changelists.files)
          .filter(([, l]) => l === name)
          .map(([p]) => p);
        const dto = await invokeCommand<Changelists>('changelist_delete', { repoPath: resolveGitRoot(), name, rollback });
        setChangelists(dto);
        if (rollback) {
          for (const p of affected) {
            const abs = toWorkspaceAbsPath(resolveGitRoot(), p);
            dropBuffer(abs);
            if (abs === activeFilePath) await openFile(abs);
          }
          bumpDiff();
        }
        showGitToast(rollback ? t().git.changelistDeletedRollback(name) : t().git.changelistDeletedToChanges(name));
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.changelistDeleteFailed(errText(err)));
      }
    },
    [workspacePath, changelists, activeFilePath, refreshGit, dropBuffer, openFile, bumpDiff, showGitToast]
  );

  /** 移动文件到指定变更列表 */
  const handleAssignChangelist = useCallback(
    async (paths: string[], list: string) => {
      if (!workspacePath || paths.length === 0) return;
      try {
        const dto = await invokeCommand<Changelists>('changelist_assign', { repoPath: resolveGitRoot(), paths, list });
        setChangelists(dto);
        showGitToast(paths.length > 1 ? t().git.changelistMovedMany(list, paths.length) : t().git.changelistMovedOne(paths[0].split(/[\\/]/).pop() ?? paths[0], list));
      } catch (err) {
        showGitToast(t().git.changelistMoveFailed(errText(err)));
      }
    },
    [workspacePath, showGitToast]
  );

  const handleUnshelveDone = useCallback(
    async (message: string) => {
      setShelveDialogOpen(false);
      showGitToast(message);
      await refreshGit();
    },
    [refreshGit]
  );

  const handleAllConflictsResolved = useCallback(
    async (message: string) => {
      setConflictDialogOpen(false);
      showGitToast(t().git.conflictsResolved(message), 8000);
      await refreshGit();
    },
    [refreshGit]
  );

  /**
   * 取消合并 / 放弃拣选 / 继续拣选（冲突横幅与对话框的 Abort/Continue）。
   * 二次确认是必须的：abort 丢弃已解决的内容（不可逆），continue 落子新提交。
   */
  const handleAbortMerge = useCallback(async () => {
    if (!workspacePath) return;
    if (!(await appConfirm(t().gitPanel.abortMergeConfirm, { title: t().gitPanel.abortMergeTitle, kind: 'warning', okLabel: 'Abort' }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_merge_abort', { repoPath: resolveGitRoot() });
      setConflictDialogOpen(false);
      showGitToast(t().gitPanel.mergeAborted, 6000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 8000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  const handleAbortCherryPick = useCallback(async () => {
    if (!workspacePath) return;
    if (!(await appConfirm(t().gitPanel.abortCherryPickConfirm, { title: t().gitPanel.abortCherryPickTitle, kind: 'warning', okLabel: 'Abort' }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_cherry_pick_abort', { repoPath: resolveGitRoot() });
      setConflictDialogOpen(false);
      showGitToast(t().gitPanel.cherryPickAborted, 6000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 8000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  const handleContinueCherryPick = useCallback(async () => {
    if (!workspacePath) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_cherry_pick_continue', { repoPath: resolveGitRoot() });
      showGitToast(t().gitPanel.cherryPickContinued, 6000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 10000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  const handleContinueMerge = useCallback(async () => {
    if (!workspacePath) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_merge_continue', { repoPath: resolveGitRoot() });
      showGitToast(t().gitPanel.mergeContinued, 6000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 10000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  const handleContinueRebase = useCallback(async () => {
    if (!workspacePath) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_rebase_continue', { repoPath: resolveGitRoot() });
      showGitToast(t().gitPanel.rebaseContinued, 6000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 10000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  const handleSkipRebase = useCallback(async () => {
    if (!workspacePath) return;
    if (!(await appConfirm(t().gitPanel.skipRebaseConfirm, { title: t().gitPanel.skipRebaseTitle, kind: 'warning', okLabel: t().gitPanel.skipRebaseOk }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_rebase_skip', { repoPath: resolveGitRoot() });
      showGitToast(t().gitPanel.rebaseSkipped, 6000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 10000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  const handleAbortRebase = useCallback(async () => {
    if (!workspacePath) return;
    if (!(await appConfirm(t().gitPanel.abortRebaseConfirm, { title: t().gitPanel.abortRebaseTitle, kind: 'warning', okLabel: 'Abort' }))) return;
    setGitBusy(true);
    try {
      await invokeCommand('git_rebase_abort', { repoPath: resolveGitRoot() });
      setConflictDialogOpen(false);
      showGitToast(t().gitPanel.rebaseAborted, 6000);
      await refreshGit();
    } catch (err) {
      showGitToast(errText(err), 8000);
    } finally {
      setGitBusy(false);
    }
  }, [workspacePath, refreshGit]);

  /**
   * 冲突对话框内单个文件解决：toast + 刷新 git 状态，列表实时缩短、
   * 自动进下一个文件。最后一个文件对话框自己走 onAllResolved 关框，
   * 这里不再重复刷新（afterAction 里二选一）。
   */
  const handleConflictFileResolved = useCallback(
    async (_path: string, message: string) => {
      showGitToast(message, 6000);
      await refreshGit();
    },
    [refreshGit]
  );

  // ---- 提交导航（Go to Parent/Child）----

  const goToCommit = useCallback(
    (hash: string) => {
      const target = commits.find(c => c.hash === hash);
      if (!target) return;
      setFocusCommit({ hash: target.hash, seq: ++focusSeqRef.current });
    },
    [commits]
  );

  const canGoParent = (c: GitCommit | null) => !!c && (c.parents?.length ?? 0) > 0 && commits.some(x => x.hash === c.parents?.[0]);
  const canGoChild = (c: GitCommit | null) =>
    !!c && commits.some(x => (x.parents ?? []).includes(c.hash));

  /** Stash 成功：关闭对话框、提示并刷新 */
  const handleStashed = async (message: string) => {
    setStashDialogOpen(false);
    showGitToast(message);
    await refreshGit();
  };

  /** Unstash 的 Apply/Pop/Drop 成功：关闭对话框、提示并刷新 */
  const handleUnstashChanged = async (message: string) => {
    setUnstashDialogOpen(false);
    showGitToast(message);
    await refreshGit();
  };

  // ---- 分支菜单：Merge / Rebase / Pull / Update / Show Diff with Working Tree ----

  /** Merge ref 到当前分支 */
  const handleMergeIntoCurrent = useCallback(
    async (refName: string) => {
      if (!workspacePath) return;
      if (!(await appConfirm(t().dialog.mergeBranchConfirm(refName, currentBranch), { title: t().dialog.mergeBranchTitle, okLabel: 'Merge' }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_merge', { repoPath: resolveGitRoot(), refName });
        showGitToast(t().git.merged(refName, currentBranch));
        await refreshGit();
      } catch (err) {
        const message = errText(err);
        showGitToast(
          message.includes('CONFLICT')
            ? t().git.mergeConflictHint(message)
            : t().git.mergeFailed(message),
          10000
        );
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, currentBranch, refreshGit]
  );

  /** 当前分支变基到 ref 之上 */
  const handleRebaseOntoCurrent = useCallback(
    async (refName: string) => {
      if (!workspacePath) return;
      if (!(await appConfirm(t().dialog.rebaseBranchConfirm(currentBranch, refName), { title: t().dialog.rebaseBranchTitle, okLabel: 'Rebase' }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_rebase', { repoPath: resolveGitRoot(), refName });
        showGitToast(t().git.rebased(currentBranch, refName));
        await refreshGit();
      } catch (err) {
        const message = errText(err);
        showGitToast(
          message.includes('CONFLICT')
            ? t().git.rebaseConflictHint(message)
            : t().git.rebaseFailed(message),
          10000
        );
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, currentBranch, refreshGit]
  );

  /** Update：当前分支弹 Pull 对话框（merge 策略），其余分支仅 fetch */
  const handleBranchUpdate = useCallback(
    async (branchName: string) => {
      if (!workspacePath) return;
      if (branchName === currentBranch) {
        setPullDialogRebase(false);
        setPullDialogOpen(true);
        return;
      }
      setGitBusy(true);
      try {
        await invokeCommand('git_fetch', { repoPath: resolveGitRoot() });
        showGitToast(t().git.updateDone(branchName));
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.fetchFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, currentBranch, refreshGit]
  );

  // ---- 提交菜单：Edit Message / Drop / Patch ----

  /** 编辑 HEAD 提交信息（amend） */
  const handleEditCommitMessage = useCallback(
    async (commit: GitCommit) => {
      if (!workspacePath) return;
      const msg = await appPrompt(t().git.amendMessagePrompt, commit.message);
      if (!msg || !msg.trim() || msg.trim() === commit.message) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_amend_message', { repoPath: resolveGitRoot(), message: msg.trim() });
        showGitToast(t().git.amendDone(msg.trim()));
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.amendFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit, appPrompt]
  );

  /** Drop Commit：摘除一个未推送的提交 */
  const handleDropCommit = useCallback(
    async (commit: GitCommit) => {
      if (!workspacePath) return;
      if (!(await appConfirm(t().dialog.dropCommitConfirm(commit.shortHash), { title: t().dialog.dropCommitTitle, kind: 'warning', okLabel: 'Drop' }))) return;
      const run = async (force: boolean) => {
        await invokeCommand('git_drop_commit', {
          repoPath: resolveGitRoot(),
          hash: commit.hash,
          force
        });
        showGitToast(t().git.commitDropped(commit.shortHash));
        await refreshGit();
      };
      setGitBusy(true);
      try {
        await run(false);
      } catch (err) {
        if (errCode(err) === 'worktree_dirty') {
          if (await appConfirm(t().dialog.dropCommitDirtyConfirm(commit.shortHash), {
            title: t().dialog.dropCommitDirtyTitle,
            kind: 'warning',
            okLabel: t().dialog.dropCommitDirtyOk
          })) {
            try {
              await run(true);
              return;
            } catch (forceErr) {
              showGitToast(t().git.dropCommitFailed(errText(forceErr)), 10000);
              return;
            }
          }
          return;
        }
        const message = errText(err);
        showGitToast(
          message.includes('CONFLICT')
            ? t().git.rebaseAbortHint(message)
            : t().git.dropCommitFailed(message),
          10000
        );
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit]
  );

  const handleCherryPickMany = useCallback(
    async (list: GitCommit[]) => {
      if (!workspacePath || list.length === 0) return;
      setGitBusy(true);
      try {
        // 选择顺序是点击顺序，不是历史顺序：按 commits 里的位置重排，
        // 再从最旧到最新逐个应用，否则后代提交会先于前置提交落地。
        const order = new Map(commits.map((c, i) => [c.hash, i]));
        const sorted = [...list].sort(
          (a, b) => (order.get(b.hash) ?? -1) - (order.get(a.hash) ?? -1)
        );
        for (const commit of sorted) {
          await invokeCommand('git_cherry_pick', { repoPath: resolveGitRoot(), hash: commit.hash });
        }
        showGitToast(t().git.cherryPickManyDone(list.length), 6000);
        await refreshGit();
      } catch (err) {
        showGitToast(errText(err), 10000);
        await refreshGit();
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, commits, refreshGit]
  );

  const handleRevertMany = useCallback(
    async (list: GitCommit[]) => {
      if (!workspacePath || list.length === 0) return;
      setGitBusy(true);
      try {
        // revert 也按历史顺序重排（新→旧），点击顺序不可信。
        const order = new Map(commits.map((c, i) => [c.hash, i]));
        const sorted = [...list].sort(
          (a, b) => (order.get(a.hash) ?? Number.MAX_SAFE_INTEGER) - (order.get(b.hash) ?? Number.MAX_SAFE_INTEGER)
        );
        for (const commit of sorted) {
          await invokeCommand('git_revert_commit', { repoPath: resolveGitRoot(), hash: commit.hash });
        }
        showGitToast(t().git.revertManyDone(list.length), 6000);
        await refreshGit();
      } catch (err) {
        showGitToast(errText(err), 10000);
        await refreshGit();
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit]
  );

  const handleDropMany = useCallback(
    async (list: GitCommit[]) => {
      if (!workspacePath || list.length === 0) return;
      if (!(await appConfirm(t().dialog.dropCommitsConfirm(list.length), {
        title: t().dialog.dropCommitsTitle,
        kind: 'warning',
        okLabel: t().dialog.dropCommitsOk
      }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_drop_commits', {
          repoPath: resolveGitRoot(),
          hashes: list.map(c => c.hash)
        });
        showGitToast(t().git.commitsDropped(list.length), 6000);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.dropCommitFailed(errText(err)), 10000);
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit]
  );

  const handleCreatePatchMany = useCallback(
    async (list: GitCommit[]) => {
      if (!workspacePath || list.length === 0) return;
      try {
        const patches: string[] = [];
        for (const commit of list) {
          patches.push(await invokeCommand<string>('git_commit_patch', {
            repoPath: resolveGitRoot(),
            hash: commit.hash
          }));
        }
        await savePatchToFile(patches.join('\n'), `commits-${list[0].shortHash}.patch`);
      } catch (err) {
        showGitToast(t().git.patchGenerateFailed(errText(err)));
      }
    },
    [workspacePath]
  );

  const handleConfirmSquash = useCallback(
    async (message: string) => {
      if (!workspacePath || !squashCommitsTarget || squashCommitsTarget.length < 2) return;
      const target = squashCommitsTarget;
      setGitBusy(true);
      try {
        await invokeCommand('git_squash_commits', {
          repoPath: resolveGitRoot(),
          hashes: target.map(c => c.hash),
          message
        });
        showGitToast(t().git.squashDone(target.length));
        setSquashCommitsTarget(null);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.squashFailed(errText(err)), 10000);
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, squashCommitsTarget, refreshGit]
  );

  /** 补丁保存到文件（Create Patch...） */
  const savePatchToFile = async (text: string, defaultName: string) => {
    if (!isTauriEnvironment()) {
      showGitToast(t().git.browserPreviewNoSave);
      return;
    }
    const path = await saveDialog({
      defaultPath: defaultName,
      filters: [{ name: 'Patch', extensions: ['patch'] }]
    });
    if (!path) return;
    try {
      await invokeCommand('write_file_content', { path, content: text });
      showGitToast(t().git.patchSaved(path));
    } catch (err) {
      showGitToast(t().git.patchSaveFailed(errText(err)));
    }
  };

  const copyToClipboard = (text: string, label: string) => {
    navigator.clipboard
      ?.writeText(text)
      .then(() => showGitToast(t().git.copiedToClipboard(label), 3000))
      .catch(() => showGitToast(t().git.clipboardUnavailable, 4000));
  };

  const handleCopyCommitPatch = useCallback(
    async (hash: string) => {
      if (!workspacePath) return;
      try {
        const patch = await invokeCommand<string>('git_commit_patch', { repoPath: resolveGitRoot(), hash });
        copyToClipboard(patch, t().git.commitPatchLabel(hash.slice(0, 7)));
      } catch (err) {
        showGitToast(t().git.patchGenerateFailed(errText(err)));
      }
    },
    [workspacePath]
  );

  const handleCreateCommitPatch = useCallback(
    async (hash: string) => {
      if (!workspacePath) return;
      try {
        const patch = await invokeCommand<string>('git_commit_patch', { repoPath: resolveGitRoot(), hash });
        await savePatchToFile(patch, `commit-${hash.slice(0, 7)}.patch`);
      } catch (err) {
        showGitToast(t().git.patchGenerateFailed(errText(err)));
      }
    },
    [workspacePath]
  );

  const handleCopyLocalPatch = useCallback(
    async (path: string) => {
      if (!workspacePath) return;
      try {
        const patch = await invokeCommand<string>('git_diff_patch', { repoPath: resolveGitRoot(), path });
        copyToClipboard(patch, t().git.localPatchLabel(path.split(/[\\/]/).pop() ?? path));
      } catch (err) {
        showGitToast(t().git.patchGenerateFailed(errText(err)));
      }
    },
    [workspacePath]
  );

  const handleCreateLocalPatch = useCallback(
    async (path: string) => {
      if (!workspacePath) return;
      try {
        const patch = await invokeCommand<string>('git_diff_patch', { repoPath: resolveGitRoot(), path });
        const name = path.split(/[\\/]/).pop() ?? 'changes';
        await savePatchToFile(patch, `${name}.patch`);
      } catch (err) {
        showGitToast(t().git.patchGenerateFailed(errText(err)));
      }
    },
    [workspacePath]
  );

  /** Get from Revision：把提交版本的文件检出到工作区（覆盖本地该文件的更改） */
  const handleGetFromRevision = useCallback(
    async (hash: string, path: string) => {
      if (!workspacePath) return;
      const name = path.split(/[\\/]/).pop() ?? path;
      if (!(await appConfirm(t().dialog.overwriteWorktreeConfirm(name, hash.slice(0, 7)), { title: t().dialog.overwriteWorktreeTitle, kind: 'warning', okLabel: 'Overwrite' }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_checkout_file_revision', { repoPath: resolveGitRoot(), commit: hash, path });
        showGitToast(t().git.checkedOut(name));
        // 标签与缓冲以绝对路径为键，而 path 是仓库相对路径：不换算的话真正缓冲
        // 不会被丢弃（编辑器继续显示旧内容），若它还是脏的，随后的自动保存会把
        // 刚检出的提交版本覆盖回草稿，同时多出一个读不出来的伪标签。
        const abs = toWorkspaceAbsPath(resolveGitRoot(), path);
        dropBuffer(abs);
        if (openFilePaths.includes(abs)) await openFile(abs);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.checkoutFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, openFilePaths, refreshGit, dropBuffer, openFile]
  );

  /** 将未跟踪文件加入版本控制（对齐 PyCharm Add to VCS） */
  const handleAddToVcs = useCallback(
    async (paths: string[]) => {
      if (!workspacePath || paths.length === 0) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_add', { repoPath: resolveGitRoot(), paths });
        showGitToast(t().git.filesAdded(paths.length > 1 ? `${paths.length}` : paths[0].split(/[\\/]/).pop() ?? paths[0]));
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.filesAddFailed(errText(err)));
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit]
  );

  /** 手动回滚单个文件的未提交更改（未跟踪文件将被删除） */
  const handleDiscardFile = useCallback(
    async (path: string) => {
      if (!workspacePath) return;
      const fileName = path.split(/[\\/]/).pop() ?? path;
      if (!(await appConfirm(t().dialog.rollbackFileConfirm(fileName), { title: t().dialog.rollbackFileTitle, kind: 'warning', okLabel: 'Rollback' }))) return;
      setGitBusy(true);
      try {
      await invokeCommand('git_discard_file_changes', { repoPath: resolveGitRoot(), path });
      showGitToast(t().git.fileReverted(fileName));
      // 标签页与缓冲以绝对路径为键，而变更清单里是仓库相对路径：必须换算，
      // 否则 openFile 会按进程工作目录找文件，报「文件不存在」
      const abs = toWorkspaceAbsPath(resolveGitRoot(), path);
      // 新增/未跟踪文件被回滚后已不存在：关闭其标签页；其余情况重读磁盘内容
      const rec = changedFiles.find(f => f.path === path);
      if (rec && (rec.status === 'A' || rec.status === 'U')) {
        releaseBuffers([abs]);
        setOpenFilePaths(prev => prev.filter(p => p !== abs));
        setActiveFilePath(cur => (cur === abs ? '' : cur));
      } else {
        dropBuffer(abs);
        // 只有仍打开的标签才重读：无条件 openFile 会给未打开的文件凭空开一个标签
        if (abs === activeFilePath) await openFile(abs);
      }
        await refreshGit();
        bumpDiff();
      } catch (err) {
        showGitToast(t().git.fileRevertFailed(errText(err)), 4000);
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, changedFiles, refreshGit, releaseBuffers, dropBuffer, openFile, bumpDiff, showGitToast]
  );

  // ============ 项目树右键：文件系统操作（对齐 PyCharm Project 视图） ============

  /** 只重读受影响目录，保留文件树的展开状态 */
  const refreshTreeDir = useCallback(
    async (dirPath: string) => {
      if (!workspacePath || !dirPath) return;
      const seq = workspaceSeqRef.current;
      try {
        const node = await invokeCommand<FileNode>('read_directory', { path: dirPath, maxDepth: 2, ignored: settings.ignoredPatterns });
        // 读取在途时可能已切换项目：旧项目的 children 不得写进新树
        if (seq !== workspaceSeqRef.current) return;
        // 单文件工作区：read_directory 返回的就是文件本身，children 为 null。
        // 若仍按目录取 children，刷新一次树就被清空（打开单文件后 ignored
        // patterns 初始化 / 文件监听都会走到这里）。
        if (!node.isDirectory) {
          if (dirPath === workspacePath) {
            setWorkspaceIsFile(true);
            setFileTree(internFileTree([node]));
          }
          return;
        }
        if (dirPath === workspacePath) setWorkspaceIsFile(false);
        const children = internFileTree(Array.isArray(node.children) ? node.children : []);
        if (dirPath === workspacePath) {
          setFileTree(children);
        } else {
          updateFileTreeNode(dirPath, children);
        }
      } catch {
        // 目录可能已被删除/移动：忽略，Git 状态由 refreshGit 兜底
      }
    },
    [workspacePath, updateFileTreeNode]
  );

  // 接到前向 ref：目录 watcher 的 effect 在几百行前就要用它，见
  // refreshTreeDirRef 的说明。
  refreshTreeDirRef.current = refreshTreeDir;
  // 忽略规则（.idea、.git 等）变更时重新读取当前项目树
  const prevIgnoredRef = useRef(settings.ignoredPatterns);
  useEffect(() => {
    if (prevIgnoredRef.current !== settings.ignoredPatterns) {
      prevIgnoredRef.current = settings.ignoredPatterns;
      if (workspacePath) {
        void refreshTreeDir(workspacePath);
      }
    }
  }, [settings.ignoredPatterns, workspacePath, refreshTreeDir]);


  /** Diff hunk 级 Stage/Unstage/Rollback 完成：丢弃该文件缓冲并刷新 diff/Git。 */
  const handleHunkChanged = useCallback(
    (absPath: string) => {
      dropBuffer(absPath);
      bumpDiff();
      void refreshGit();
    },
    [dropBuffer, bumpDiff, refreshGit]
  );

  /** Find in Files 替换完成：丢弃/重读受影响的打开缓冲，刷新树与 Git 状态。 */
  const handleFilesReplaced = useCallback(
    async (changedPaths: string[]) => {
      if (changedPaths.length === 0) return;
      for (const path of changedPaths) {
        const buffer = buffersRef.current[path];
        if (buffer?.dirty) {
          // onBeforeReplace 已保存过；仍脏说明写盘失败，绝不能用磁盘替换结果
          // 覆盖用户缓冲，也不能让后续自动保存把替换结果冲掉。
          showStatus(t().status.replaceSkippedDirty(baseNameOf(path)));
          continue;
        }
        dropBuffer(path);
        if (openFilePathsRef.current.includes(path)) {
          await openFile(path);
        }
      }
      const dirs = [...new Set(changedPaths.map(parentOfPath))];
      await Promise.all(dirs.map(d => refreshTreeDir(d)));
      void refreshGit();
      bumpDiff();
    },
    [dropBuffer, openFile, refreshTreeDir, refreshGit, bumpDiff, showStatus]
  );

  /** 文件/目录被重命名或移动后，同步打开的标签页与编辑缓冲 */
  const relocateOpenFiles = useCallback(
    (oldPath: string, newPath: string) => {
      const relocate = (p: string): string | null => {
        if (p === oldPath) return newPath;
        if (p.startsWith(`${oldPath}/`)) return `${newPath}${p.slice(oldPath.length)}`;
        return null;
      };
      const affected = openFilePaths.some(p => relocate(p) !== null);
      if (!affected) return;
      openFilePaths.forEach(p => {
        const np = relocate(p);
        if (!np) return;
        const buf = buffersRef.current[p];
        if (buf) {
          dropBuffer(p);
          putBuffer(np, buf);
        }
      });
      setOpenFilePaths(prev => prev.map(p => relocate(p) ?? p));
      setMarkdownViewModes(prev => {
        const next = { ...prev };
        openFilePaths.forEach(p => {
          const relocated = relocate(p);
          if (!relocated || !(p in next)) return;
          next[relocated] = next[p];
          delete next[p];
        });
        return next;
      });
      setMarkdownScrollRatios(prev => {
        const next = { ...prev };
        openFilePaths.forEach(p => {
          const relocated = relocate(p);
          if (!relocated || !(p in next)) return;
          next[relocated] = next[p];
          delete next[p];
        });
        return next;
      });
      const newActive = activeFilePath ? relocate(activeFilePath) : null;
      if (newActive) setActiveFilePath(newActive);
    },
    [openFilePaths, activeFilePath, dropBuffer, putBuffer]
  );

  /** 把绝对路径移入废纸篓：关闭受影响标签页，刷新文件树与 Git 状态（项目树 / 变更清单共用） */
  const deletePathsToTrash = useCallback(
    async (absPaths: string[]) => {
      const failed: string[] = [];
      for (const p of absPaths) {
        try {
          await invokeCommand('fs_delete', { path: p });
          closeFilesUnderPaths([p]);
        } catch {
          failed.push(baseNameOf(p));
        }
      }
      for (const dir of [...new Set(absPaths.map(parentOfPath))]) {
        await refreshTreeDir(dir);
      }
      void refreshGit();
      if (failed.length > 0) {
        showStatus(t().status.deleteFailedList(failed.join(', ')));
      } else {
        showStatus(
          absPaths.length > 1
            ? t().git.deletedManyToTrash(absPaths.length)
            : t().git.deletedOneToTrash(baseNameOf(absPaths[0]))
        );
      }
    },
    [closeFilesUnderPaths, refreshTreeDir, refreshGit]
  );

  const handleTreeNewFile = useCallback((dir: string) => setNameDialog({ mode: 'new-file', targetDir: dir }), []);
  const handleTreeNewDirectory = useCallback((dir: string) => setNameDialog({ mode: 'new-dir', targetDir: dir }), []);
  const handleTreeRename = useCallback((path: string, name: string) => setNameDialog({ mode: 'rename', path, name }), []);
  const handleTreeDelete = useCallback(
    (path: string, name: string, isDir: boolean, selected?: string[]) => {
      // 多选 >= 2 项时走批量确认：paths 含 path 自身，对话框改用批量文案。
      // 单文件与旧行为完全一致。
      if (selected && selected.length > 1) {
        setNameDialog({ mode: 'delete', path, name, isDir, paths: selected });
        return;
      }
      setNameDialog({ mode: 'delete', path, name, isDir });
    },
    []
  );

  const handleTreeCut = useCallback((path: string) => {
    setFsClipboard({ path, cut: true });
    showStatus(t().status.cut(baseNameOf(path)));
  }, []);

  const handleTreeCopy = useCallback((path: string) => {
    setFsClipboard({ path, cut: false });
    showStatus(t().status.copiedOne(baseNameOf(path)));
  }, []);

  const handleTreePaste = useCallback(
    async (destDir: string) => {
      if (!fsClipboard) return;
      const { path: src, cut } = fsClipboard;
      try {
        const dest = await invokeCommand<string>('fs_paste', { src, destDir, moveIt: cut });
        await refreshTreeDir(destDir);
        if (cut) {
          const srcParent = parentOfPath(src);
          if (srcParent !== destDir) await refreshTreeDir(srcParent);
          setFsClipboard(null);
          relocateOpenFiles(src, dest);
        }
        void refreshGit();
        showStatus(t().status.pastedTo(cut ? 'cut' : 'copy', baseNameOf(destDir)));
      } catch (err) {
        showStatus(t().status.pasteFailed(errText(err)));
      }
    },
    [fsClipboard, refreshTreeDir, relocateOpenFiles, refreshGit]
  );

  const handleTreeCopyPath = useCallback(
    async (path: string, absolute: boolean) => {
      const rel = workspacePath && path.startsWith(`${workspacePath}/`) ? path.slice(workspacePath.length + 1) : path;
      const text = absolute ? path : rel;
      try {
        await navigator.clipboard.writeText(text);
        showStatus(t().status.pathCopied(text));
      } catch {
        showStatus(t().status.copyPathFailed);
      }
    },
    [workspacePath]
  );

  const handleTreeReveal = useCallback(async (path: string) => {
    try {
      await invokeCommand('fs_reveal', { path });
    } catch (err) {
      showStatus(t().status.showInFileManagerFailed(errText(err)));
    }
  }, []);

  const handleTreeOpenTerminal = useCallback(async (path: string) => {
    try {
      await invokeCommand('launch_ghostty', { cwd: path });
      showStatus(t().status.openedInExternalTerminal(baseNameOf(path)));
    } catch (err) {
      showStatus(t().status.openTerminalFailed(errText(err)));
    }
  }, []);

  /** 从磁盘重新加载：目录刷新子树，文件丢弃缓冲后重读（对齐 PyCharm Reload from Disk） */
  const handleTreeReload = useCallback(
    async (path: string, isDir: boolean) => {
      if (isDir) {
        await refreshTreeDir(path);
      } else {
        dropBuffer(path);
        await openFile(path);
        await refreshTreeDir(parentOfPath(path));
      }
      showStatus(t().status.reloadedFromDisk(baseNameOf(path)));
    },
    [refreshTreeDir, dropBuffer, openFile]
  );

  /** 目录级 Rollback：恢复已跟踪修改 + 清理未跟踪文件，同步标签页 */
  const handleTreeRollbackDir = useCallback(
    async (path: string) => {
      if (!workspacePath) return;
      // 目录级回滚无差别作用于整棵子树，比单文件更需要确认（对齐单文件回滚）
      if (!(await appConfirm(t().dialog.rollbackDirConfirm(baseNameOf(path)), { title: t().dialog.rollbackDirTitle, kind: 'warning', okLabel: 'Rollback' }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_discard_file_changes', { repoPath: resolveGitRoot(), path });
        showGitToast(t().git.dirReverted(baseNameOf(path)));
        // 目录下打开的文件：仍存在的重读磁盘，已消失的（未跟踪被清理）关闭标签
        const under = openFilePaths.filter(p => p === path || p.startsWith(`${path}/`));
        for (const p of under) {
          try {
            await invokeCommand('read_file_content', { path: p });
            dropBuffer(p);
            await openFile(p);
          } catch {
            releaseBuffers([p]);
            setOpenFilePaths(prev => prev.filter(x => x !== p));
            setActiveFilePath(cur => (cur === p ? '' : cur));
          }
        }
        await refreshTreeDir(path);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.fileRevertFailed(errText(err)), 4000);
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, openFilePaths, dropBuffer, openFile, releaseBuffers, refreshTreeDir, refreshGit, showGitToast]
  );

  const handleTreeRollback = useCallback(
    (path: string, isDir: boolean) => {
      if (isDir) void handleTreeRollbackDir(path);
      else void handleDiscardFile(path);
    },
    [handleTreeRollbackDir, handleDiscardFile]
  );

  /** Compare With…：选择另一个文件做内容对比 */
  const handleTreeCompareWith = useCallback(async (path: string) => {
    if (!isTauriEnvironment()) {
      showStatus(t().status.browserPreviewNoFilePicker);
      return;
    }
    try {
      // 走 Rust 原生 pick_path：只有它返回的路径会被登记为一次性读授权，
      // fs_diff_files 才能读取工作区外的对比文件。
      const picked = await invokeCommand<string | null>('pick_path', {
        title: t().git.pickCompareFileTitle(baseNameOf(path)),
        defaultPath: path
      });
      if (typeof picked === 'string' && picked !== path) {
        clearDiffSource();
        setFsCompareFiles({ oldPath: picked, newPath: path });
        setDiffStaged(false);
        setDiffFilePath(path);
        enterDiff();
      }
    } catch (err) {
      showStatus(t().status.pickFileFailed(errText(err)));
    }
  }, [clearDiffSource, enterDiff]);

  /** 在目录树中定位当前打开的文件：逐层加载链上目录 → 全部展开 → 滚动并闪烁高亮 */
  /** 定位闪烁定时器句柄：连续点击「定位」时先取消上一个，避免旧节点被延迟移除高亮 */
  const locateFlashTimersRef = useRef<number[]>([]);
  /**
   * 定位代次：关开关 / 切文件时 +1，在途的异步定位（读盘 + 定时器）发现代次
   * 对不上就地作废 —— 否则关了开关，已在飞的定位照样落地，看起来"关不掉"。
   */
  const locateEpochRef = useRef(0);
  const clearLocateFlashTimers = useCallback(() => {
    locateFlashTimersRef.current.forEach(t => window.clearTimeout(t));
    locateFlashTimersRef.current = [];
  }, []);
  useEffect(() => clearLocateFlashTimers, [clearLocateFlashTimers]);

  const locateActiveInTree = useCallback(async (quiet = false) => {
    if (!activeFilePath || !workspacePath || !activeFilePath.startsWith(workspacePath)) return;
    const rel = activeFilePath.slice(workspacePath.length + 1);
    const segs = rel.split('/').filter(Boolean);
    const seq = workspaceSeqRef.current;
    const epoch = locateEpochRef.current;
    try {
      // 树是懒加载的：逐层确保链上目录的 children 存在。
      // 已经加载过的目录直接跳过 —— 每次切标签都重读并替换整棵子树，
      // 会让 ProjectDrawer 整体重建，用户看到的就是文件树闪一下。
      for (let i = 0; i < segs.length - 1; i++) {
        const dir = `${workspacePath}/${segs.slice(0, i + 1).join('/')}`;
        const existing = findTreeNode(fileTreeRef.current, dir);
        if (existing && Array.isArray(existing.children)) continue;
        const node = await invokeCommand<FileNode>('read_directory', { path: dir, maxDepth: 1, ignored: settings.ignoredPatterns });
        // 逐层读取期间可能已切换项目：链上结果与下面的展开/定位一并作废
        if (seq !== workspaceSeqRef.current) return;
        // 关开关 / 切文件导致代次变化：在途定位同样作废
        if (epoch !== locateEpochRef.current) return;
        const children = Array.isArray(node.children) ? node.children : [];
        updateFileTreeNode(dir, children);
      }
    } catch {
      return;
    }
    if (seq !== workspaceSeqRef.current) return;
    if (epoch !== locateEpochRef.current) return;
    setTreeExpanded(prev => {
      let next: Set<string> | null = null;
      // 根节点也可能被用户折叠：定位根级文件时必须先展开项目根
      if (!prev.has(workspacePath)) {
        next = new Set(prev);
        next.add(workspacePath);
      }
      segs.slice(0, -1).forEach((_, i) => {
        const dir = `${workspacePath}/${segs.slice(0, i + 1).join('/')}`;
        if (!prev.has(dir)) {
          if (!next) next = new Set(prev);
          next.add(dir);
        }
      });
      // 全部已展开时返回原引用：ProjectDrawer 的 memo 不会被击穿
      return next ?? prev;
    });
    // 等渲染完成后滚动到目标行。quiet（自动定位）时不闪烁：`opened` 高亮本就常驻，
    // 闪烁动画只留给手动定位按钮，避免每次切标签都闪一下。
    clearLocateFlashTimers();
    const scrollToRow = () => {
      const el = document.querySelector(`[data-tree-path="${CSS.escape(activeFilePath)}"]`);
      if (!el) return null;
      // nearest：目标行已在视口内时完全不滚动，避免每次切标签都跳一下
      el.scrollIntoView({ block: 'nearest' });
      return el;
    };
    if (quiet) {
      const outer = window.setTimeout(() => {
        if (epoch !== locateEpochRef.current) return;
        scrollToRow();
      }, 60);
      locateFlashTimersRef.current.push(outer);
      return;
    }
    const outer = window.setTimeout(() => {
      if (epoch !== locateEpochRef.current) return;
      const el = document.querySelector(`[data-tree-path="${CSS.escape(activeFilePath)}"]`);
      if (!el) return;
      el.scrollIntoView({ block: 'center' });
      el.classList.remove('tree-locate-flash');
      void (el as HTMLElement).offsetWidth; // 强制重排以重启动画
      el.classList.add('tree-locate-flash');
      const inner = window.setTimeout(() => el.classList.remove('tree-locate-flash'), 1500);
      locateFlashTimersRef.current.push(inner);
    }, 60);
    locateFlashTimersRef.current.push(outer);
  }, [activeFilePath, workspacePath, updateFileTreeNode, clearLocateFlashTimers]);
  locateActiveRef.current = locateActiveInTree;

  // 自动定位（文件树顶部开关，对齐 PyCharm Always Select Opened File）：
  // 开时切标签/开文件自动展开链上目录并滚动到对应行。quiet 模式不闪烁。
  // 只跟编辑器标签：Diff 页的 activeFilePath 可能是提交内文件，不跟。
  useEffect(() => {
    const activation = tabActivationRef.current;
    tabActivationRef.current = null;
    if (!autoLocateTree) return;
    if (activeTab !== 'editor' || !activeFilePath) return;
    // 只有点击编辑器标签才自动定位。文件树点击、搜索打开、启动恢复等
    // 都不触发；2s 窗口防止一次无效点击在后续无关打开时“复活”。
    if (!activation || activation.path !== activeFilePath) return;
    if (performance.now() - activation.at > 2000) return;
    void locateActiveInTree(true);
  }, [autoLocateTree, activeTab, activeFilePath, locateActiveInTree]);

  // 多窗口同步：任一窗口拨开关，其它窗口实时跟随（storage 事件只发给别的
  // document，本窗口自己的 set 不会回环）。
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key !== 'ide_tree_autolocate') return;
      const next = e.newValue !== '0';
      setAutoLocateTree(prev => {
        if (prev === next) return prev;
        if (!next) {
          locateEpochRef.current++;
          clearLocateFlashTimers();
        }
        return next;
      });
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, [clearLocateFlashTimers]);

  // ── ProjectDrawer：稳定回调（整棵已展开树每次都 reconcile，回调重建即击穿 memo）──
  const openExternalDirectoryStable = useStableCallback(() => void pickExternalDirectory());
  const locateActiveStable = useStableCallback(() => void locateActiveInTree());
  const updateFileNodeStable = useStableCallback((path: string, children: FileNode[]) => {
    // ProjectDrawer 的懒加载读取在 App 之外 await：回来时用户可能已切换
    // 项目。只接受仍属于当前工作区或已登记外部根的节点，丢弃旧项目结果。
    const inExternal = externalRoots.some(r => path === r.path || path.startsWith(`${r.path}/`));
    const inWorkspace =
      !!workspacePath && (path === workspacePath || path.startsWith(`${workspacePath}/`));
    if (!inExternal && !inWorkspace) return;
    if (inExternal) updateExternalTreeNode(path, children);
    else updateFileTreeNode(path, children);
  });
  const nodeContextStable = useStableCallback(
    (node: FileNode, x: number, y: number, selected?: string[]) => {
      // selected 非空表示被点的行已在一个多选集内 → 批量菜单。
      // 用数组而非 Set：setTreeMenu 的 state 需要可比较的新引用，
      // 且长度 > 1 已足够判定批量形态。
      setTreeMenu({
        x,
        y,
        path: node.path,
        name: node.name,
        isDir: node.isDirectory,
        selectedPaths: selected && selected.length > 1 ? selected : undefined
      });
    }
  );

  /** 展开当前目录及子目录（一次性深读整棵子树，忽略规则在后端生效） */
  const handleTreeExpandAll = useCallback(
    async (dirPath: string) => {
      const seq = workspaceSeqRef.current;
      try {
        const node = await invokeCommand<FileNode>('read_directory', { path: dirPath, maxDepth: 16, ignored: settings.ignoredPatterns });
        // 深读可能持续较久：期间切换项目则整批结果作废（否则树里会混入旧项目节点）
        if (seq !== workspaceSeqRef.current) return;
        if (!node.isDirectory) {
          if (dirPath === workspacePath) {
            setWorkspaceIsFile(true);
            setFileTree(internFileTree([node]));
          }
          return;
        }
        const children = internFileTree(Array.isArray(node.children) ? node.children : []);
        if (dirPath === workspacePath) setFileTree(children);
        else updateFileTreeNode(dirPath, children);
        const dirs: string[] = [];
        const walk = (list: FileNode[]) => {
          for (const n of list) {
            if (n.isDirectory) {
              dirs.push(n.path);
              if (Array.isArray(n.children)) walk(n.children);
            }
          }
        };
        walk(children);
        setTreeExpanded(prev => {
          const next = new Set(prev);
          next.add(dirPath);
          dirs.forEach(d => next.add(d));
          return next;
        });
      } catch (err) {
        showStatus(t().status.expandDirFailed(errText(err)));
      }
    },
    [workspacePath, updateFileTreeNode]
  );

  /** 折叠当前目录及子目录（只影响以该目录为前缀的已展开路径，不动整个项目） */
  const handleTreeCollapseAll = useCallback((dirPath: string) => {
    setTreeExpanded(prev => new Set([...prev].filter(p => p !== dirPath && !p.startsWith(`${dirPath}/`))));
  }, []);

  /** 新建 / 重命名 / 删除 对话框的确认处理 */
  const handleNameDialogConfirm = useCallback(
    async (value: string) => {
      const dialog = nameDialog;
      if (!dialog) return;
      setNameDialog(null);
      try {
        if (dialog.mode === 'new-file') {
          const created = await invokeCommand<string>('fs_create_file', { dir: dialog.targetDir, name: value });
          setTreeExpanded(prev => new Set([...prev, dialog.targetDir!]));
          await refreshTreeDir(dialog.targetDir!);
          void refreshGit();
          void openFile(created);
          showStatus(t().status.createdFile(baseNameOf(created)));
        } else if (dialog.mode === 'new-dir') {
          const created = await invokeCommand<string>('fs_create_directory', { dir: dialog.targetDir, name: value });
          setTreeExpanded(prev => new Set([...prev, dialog.targetDir!]));
          await refreshTreeDir(dialog.targetDir!);
          showStatus(t().status.createdDir(baseNameOf(created)));
        } else if (dialog.mode === 'rename') {
          const newPath = await invokeCommand<string>('fs_rename', { path: dialog.path, newName: value });
          relocateOpenFiles(dialog.path!, newPath);
          await refreshTreeDir(parentOfPath(dialog.path!));
          void refreshGit();
          showStatus(t().status.renamedTo(baseNameOf(newPath)));
        } else if (dialog.mode === 'delete') {
          // paths 是批量多选的完整集合；单文件确认时它不存在，退回 path。
          await deletePathsToTrash(dialog.paths?.length ? dialog.paths : [dialog.path!]);
        }
      } catch (err) {
        showStatus(t().status.fileOpFailed(errText(err)));
      }
    },
    [
      nameDialog,
      refreshTreeDir,
      refreshGit,
      openFile,
      relocateOpenFiles,
      deletePathsToTrash
    ]
  );

  /** Commit 抽屉 Shelf 页操作后的统一回调：提示 + 刷新 Git 状态 */
  const handleShelfChanged = useCallback(
    (message: string) => {
      showGitToast(message);
      void refreshGit();
    },
    [showGitToast, refreshGit]
  );

  // ── CommitDrawer：稳定回调（memo 生效前提；实现始终取最新闭包）──
  const discardFileStable = useStableCallback((path: string) => void handleDiscardFile(path));
  const addToVcsStable = useStableCallback((paths: string[]) => void handleAddToVcs(paths));
  const shelveStable = useStableCallback((paths: string[]) => void handleShelve(paths));
  const createChangelistStable = useStableCallback(
    (name: string) => void handleCreateChangelist(name)
  );
  const deleteChangelistStable = useStableCallback((name: string, rollback: boolean) =>
    void handleDeleteChangelist(name, rollback)
  );
  const assignToChangelistStable = useStableCallback((paths: string[], list: string) =>
    void handleAssignChangelist(paths, list)
  );
  const unshelveStable = useStableCallback((shelf: string, paths: string[], target: string) =>
    void handleUnshelveToChangelist(shelf, paths, target)
  );
  const commitFileStable = useStableCallback(async (path: string) => {
    const msg = await appPrompt(t().git.onlyCommitFilePrompt(path.split(/[\\/]/).pop() ?? path));
    if (!msg || !msg.trim()) return;
    void handleCommit(msg.trim(), false, [path]);
  });
  const commitPathsStable = useStableCallback(async (paths: string[]) => {
    if (paths.length === 0) return;
    const msg = await appPrompt(t().git.onlyCommitFilesPrompt(paths.length));
    if (!msg || !msg.trim()) return;
    void handleCommit(msg.trim(), false, paths);
  });
  const jumpToSourceStable = useStableCallback(
    (path: string) => void openFile(toWorkspaceAbsPath(resolveGitRoot(), path))
  );
  const copyLocalPatchStable = useStableCallback(
    (path: string) => void handleCopyLocalPatch(path)
  );
  const createLocalPatchStable = useStableCallback(
    (path: string) => void handleCreateLocalPatch(path)
  );
  const shelfDiffStable = useStableCallback(
    async (shelfName: string, filePath: string, inNewTab = false, compareWithLocal = false) => {
      try {
        const res = await invokeCommand<ShelfPreviewDto>('git_shelf_diff_preview', {
          repoPath: resolveGitRoot() || workspacePath,
          name: shelfName,
          path: filePath,
          compareWithLocal,
        });
        clearDiffSource();
        setFsCompareFiles({ oldPath: res.oldPath, newPath: res.newPath });
        setDiffFilePath(res.newPath);
        setDiffStaged(false);
        enterDiff();
        if (inNewTab) {
          setActiveTab('diff');
        }
      } catch (err) {
        showStatus(`预览失败: ${errText(err)}`);
      }
    }
  );
  /** 冲突对话框内含 3 个 monaco Editor，先确保内核就绪再打开（Commit 抽屉与 Git 面板共用） */
  const resolveConflictsStable = useStableCallback(async () => {
    if (isTauriEnvironment() && workspacePath) {
      setIsConflictModalActive(true);
      try {
        await invokeCommand('open_conflict_window', { repoPath: resolveGitRoot() });
      } catch (err) {
        console.error('Failed to open native conflict window', err);
        requestMonaco(themeIdRef.current);
        setConflictDialogOpen(true);
      }
    } else {
      requestMonaco(themeIdRef.current);
      setConflictDialogOpen(true);
    }
  });
  const abortMergeStable = useStableCallback(() => void handleAbortMerge());
  const abortCherryPickStable = useStableCallback(() => void handleAbortCherryPick());
  const abortRebaseStable = useStableCallback(() => void handleAbortRebase());
  const continueCherryPickStable = useStableCallback(() => void handleContinueCherryPick());
  const continueMergeStable = useStableCallback(() => void handleContinueMerge());
  const continueRebaseStable = useStableCallback(() => void handleContinueRebase());
  const skipRebaseStable = useStableCallback(() => void handleSkipRebase());
  /** TitleBar 只关心"操作是否在途 + 哪一类"：切片保住 memo（sequencer 每次轮询都换引用） */
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const titleBarSequencer = useMemo(
    () =>
      sequencer && (sequencer.merging || sequencer.cherryPicking || sequencer.rebasing)
        ? {
            merging: sequencer.merging,
            cherryPicking: sequencer.cherryPicking,
            rebasing: sequencer.rebasing
          }
        : null,
    [sequencer?.merging, sequencer?.cherryPicking, sequencer?.rebasing]
  );
  const closeCommitDrawerStable = useStableCallback(() => setActiveDrawer(null));

  /** 变更清单右键：删除未跟踪文件（移入废纸篓）。单个文件复用项目树的删除确认对话框 */
  const handleDeleteUnversioned = useCallback(
    async (paths: string[]) => {
      if (!workspacePath || paths.length === 0) return;
      // 统一走 toWorkspaceAbsPath：手写拼接不识别 Windows 盘符（`C:/x`
      // 会拼成 `/C:/x`），也不去 workspace 尾斜杠（出现 `//`）。
      // 同函数其余调用点都已用它。
      const absPaths = paths.map(p => toWorkspaceAbsPath(resolveGitRoot(), p));
      if (absPaths.length === 1) {
        setNameDialog({
          mode: 'delete',
          path: absPaths[0],
          name: baseNameOf(absPaths[0]),
          isDir: false
        });
      } else if (await appConfirm(t().dialog.trashUnversionedConfirm(absPaths.length), { title: t().dialog.trashUnversionedTitle, kind: 'warning', okLabel: 'Delete' })) {
        void deletePathsToTrash(absPaths);
      }
    },
    [workspacePath, deletePathsToTrash]
  );

  /** 手动回滚某次提交：生成反向提交（git revert） */
  const handleRevertCommit = useCallback(
    async (hash: string) => {
      if (!workspacePath) return;
      const short = hash.slice(0, 7);
      if (!(await appConfirm(t().dialog.revertCommitConfirm(short), { title: t().dialog.revertCommitTitle, okLabel: 'Revert' }))) return;
      setGitBusy(true);
      try {
        await invokeCommand('git_revert_commit', { repoPath: resolveGitRoot(), hash });
        showGitToast(t().git.revertedCommit(short), 4000);
        await refreshGit();
      } catch (err) {
        showGitToast(t().git.fileRevertFailed(errText(err)), 4000);
      } finally {
        setGitBusy(false);
      }
    },
    [workspacePath, refreshGit, showGitToast]
  );

  // 监听独立冲突窗口事件（跨 WebviewWindow 广播）
  useEffect(() => {
    if (!isTauriEnvironment()) return;
    let unlisten1: (() => void) | undefined;
    let unlisten2: (() => void) | undefined;
    let unlisten3: (() => void) | undefined;

    listen<{ path: string; message: string }>('ide_conflict_resolved', e => {
      void handleConflictFileResolved(e.payload.path, e.payload.message);
    }).then(u => { unlisten1 = u; });

    listen<{ message: string }>('ide_all_conflicts_resolved', e => {
      setIsConflictModalActive(false);
      void handleAllConflictsResolved(e.payload.message);
    }).then(u => { unlisten2 = u; });

    listen('ide_conflict_closed', () => {
      setIsConflictModalActive(false);
      void refreshGit();
    }).then(u => { unlisten3 = u; });

    return () => {
      unlisten1?.();
      unlisten2?.();
      unlisten3?.();
    };
  }, [refreshGit, handleConflictFileResolved, handleAllConflictsResolved]);

  // 当模态遮罩激活时，监听主窗口聚焦与轻量轮询，如果两个冲突窗口都已关闭则自动解除阻断
  useEffect(() => {
    if (!isConflictModalActive || !isTauriEnvironment()) return;

    const checkWindowAlive = async () => {
      try {
        const isOpen = await invokeCommand<boolean>('is_conflict_window_open');
        if (!isOpen) {
          setIsConflictModalActive(false);
          void refreshGit();
        }
      } catch {
        // ignore
      }
    };

    window.addEventListener('focus', checkWindowAlive);
    const interval = setInterval(checkWindowAlive, 500);

    return () => {
      window.removeEventListener('focus', checkWindowAlive);
      clearInterval(interval);
    };
  }, [isConflictModalActive, refreshGit]);
  const initRepoStable = useStableCallback(() => void handleInitRepo());
  const pushStable = useStableCallback(() => {
    if (!currentBranch) {
      showStatus(t().git.detachedPushBlocked);
      return;
    }
    setPushDialogOpen(true);
  });
  const pullStable = useStableCallback(() => setPullDialogOpen(true));
  const fetchStable = useStableCallback(() => void handleFetch());
  const stashStable = useStableCallback(() => setStashDialogOpen(true));
  const unstashStable = useStableCallback(() => setUnstashDialogOpen(true));
  const shelfStable = useStableCallback(() => setShelveDialogOpen(true));
  const loadMoreStable = useStableCallback(() => void loadMoreCommits());
  const toggleMaximizeStable = useStableCallback(() => setGitPanelMaximized(v => !v));
  const toggleTerminalMaximizeStable = useStableCallback(() => setTerminalMaximized(v => !v));
  const refreshGitStable = useStableCallback(() => void refreshGit());
  // 面板自管选中态（GitBottomPanel.selectedCommit）：App 不再镜像，onSelectCommit
  // 只是满足面板调用约定的空透；真正的外部跳转走 focusCommit → jumpToCommit。
  const selectCommitStable = useStableCallback((_commit: GitCommit) => {});
  const openCommitDiffStable = useStableCallback((hash: string) => openCommitDiff(hash));
  const openFileDiffStable = useStableCallback((hash: string, path: string) =>
    openCommitDiff(hash, path)
  );
  const openCommitsDiffStable = useStableCallback((commits: string[], path: string = '') =>
    openCommitsDiff(commits, path)
  );
  const compareWithLocalStable = useStableCallback((hash: string, path: string) =>
    openCompareWithLocal(hash, path)
  );
  const getFromRevisionStable = useStableCallback((hash: string, path: string) =>
    void handleGetFromRevision(hash, path)
  );
  const copyCommitPatchStable = useStableCallback(
    (hash: string) => void handleCopyCommitPatch(hash)
  );
  const createCommitPatchStable = useStableCallback(
    (hash: string) => void handleCreateCommitPatch(hash)
  );
  const revertCommitStable = useStableCallback((hash: string) => void handleRevertCommit(hash));
  const bottomBranchContextStable = useStableCallback(
    (branch: GitBranch, x: number, y: number, selected?: GitBranch[]) =>
      setBranchMenu({ branch, x, y, branches: selected })
  );
  const commitContextStable = useStableCallback(
    (commit: GitCommit, x: number, y: number, selectedCommits?: GitCommit[]) =>
      setCommitMenu({ commit, x, y, selectedCommits })
  );

  const handleCompareVersions = useCallback((c1: GitCommit, c2: GitCommit) => {
    // 对齐 PyCharm Compare Versions：选中的两个提交做差异对比
    setCompareSession({ fromCommit: c2, toCommit: c1 });
    setActiveDrawer('compare');
  }, []);

  const handleSwapCompareCommits = useCallback(() => {
    setCompareSession(prev => {
      if (!prev) return null;
      const next = { fromCommit: prev.toCommit, toCommit: prev.fromCommit };
      if (diffCompareCommits) {
        setDiffCompareCommits({ from: next.fromCommit.hash, to: next.toCommit.hash });
      }
      return next;
    });
  }, [diffCompareCommits]);

  const handleCloseCompareSession = useCallback(() => {
    setCompareSession(null);
    setDiffCompareCommits(null);
    if (activeDrawer === 'compare') {
      setActiveDrawer('project');
    }
  }, [activeDrawer]);

  const handleSelectCompareFile = useCallback((path: string) => {
    if (!compareSession) return;
    clearDiffSource();
    setDiffCompareCommits({
      from: compareSession.fromCommit.hash,
      to: compareSession.toCommit.hash
    });
    setDiffStaged(false);
    setDiffFilePath(path);
    enterDiff();
  }, [clearDiffSource, compareSession, enterDiff]);
  const closeGitPanelStable = useStableCallback(() => setActiveBottomTool(null));

  /** 关闭当前项目：优先切回上一个仍打开的项目；若已无其他打开项目则回到欢迎页 */
  const closeWorkspace = useCallback(async () => {
    // 有未保存改动时先确认：closeWorkspace 直接清空缓冲，静默丢弃即数据丢失
    const dirtyCount = Object.values(buffersRef.current).filter(b => b.dirty).length;
    if (dirtyCount > 0 && !(await appConfirm(t().dialog.unsavedOnClose(dirtyCount), { title: t().dialog.unsavedOnOpenTitle, kind: 'warning', okLabel: t().dialog.discard }))) {
      return;
    }
    const closedPath = workspacePath;
    // 关闭项目会终止该项目的全部终端。和关标签/预算回收保持同一语义：
    // 有命令在跑时先确认，用户取消就整个关闭放弃（快照、PTY 都不动）。
    if (closedPath && isTauriEnvironment()) {
      const proceed = (await terminalRef.current?.confirmProjectClose(closedPath)) ?? true;
      if (!proceed) return;
    }
    // 关闭即终止：丢弃该项目快照并终止它的全部终端会话（释放进程与内存）
    if (closedPath) {
      delete projectSessionsRef.current[closedPath];
      if (isTauriEnvironment()) {
        // 注销工作区授权：关闭后该目录下的文件不再可读写
        void invokeCommand('unregister_workspace', { path: closedPath }).catch(() => {});
        // 单文件工作区的终端 cwd 是文件所在目录（见 GhosttyTerminal spawnCwd），
        // 关闭时也必须按同一形态匹配，否则 pty_close_by_cwd 会漏掉。
        const terminalCwd = workspaceIsFile ? parentOfPath(closedPath) : closedPath;
        void invokeCommand('pty_close_by_cwd', { cwd: terminalCwd }).catch(() => {});
      }
    }
    // 作废在途加载；在清除顺序前找出上一个仍打开（有会话快照）的项目
    workspaceSeqRef.current += 1;
    // 关闭/恢复同样是工作区切换：旧 root 的 LSP 进程与 providers 必须先停。
    // 少了这一步，恢复同语言项目时 ensure 会命中 clients.has 直接返回，
    // 新项目继续使用已关闭项目的 LSP root。
    void resetLsp();
    openOrderRef.current = openOrderRef.current.filter(p => p !== closedPath);
    const prevPath = openOrderRef.current.find(p => projectSessionsRef.current[p]);
    const prevSnap = prevPath ? projectSessionsRef.current[prevPath] : undefined;
    setWorkspacePath('');
    setWorkspaceIsFile(false);
    setFileTree([]);
    setExternalRoots([]);
    setTreeExpanded(new Set());
    setOpenFilePaths([]);
    setMarkdownViewModes({});
    setMarkdownScrollRatios({});
    setActiveFilePath('');
    setActiveTab('editor');
    setDiffOpen(false);
    // 缓冲全部丢弃（脏缓冲在上面已确认丢弃）：连带释放各自的 monaco model
    Object.keys(buffersRef.current).forEach(disposeModelForPath);
    buffersRef.current = {};
    setBuffers({});
    setCommits([]);
    setBranches([]);
    setTags([]);
    setChangedFiles([]);
    setChangelists({ lists: ['Changes'], files: {} });
    // 仓库根必须和列表一起清：否则切到 B 后、B 的 git_repo_info 返回前，
    // 所有 resolveGitRoot() 仍会把操作打到已关闭的 A 仓库。
    gitRepoRootRef.current = '';
    setGitRepoRoot('');
    setFindInFilesOpen(false);
    setFindInFilesScope('');
    setCurrentBranch('');  // 无项目 / 无版本管理时不显示分支按钮
    setFocusCommit(null);
    setSequencer(null);
    setIsRepo(true);
    setHasMoreCommits(false);
    setGitRefreshing(false);
    gitSeqRef.current += 1;
    gitPendingRef.current = false;
    setBranchMenu(null);
    setCompareRef(null);
    setCompareBranches(null);
    setCompareSession(null);
    setDiffCompareCommits(null);
    setDiffSelectedCommits(null);
    setCommitMenu(null);
    setResetTarget(null);
    setSquashCommitsTarget(null);
    setHistoryFile(null);
    setBlameOn(false);
    setTreeMenu(null);
    setFsClipboard(null);
    setNameDialog(null);
    setFsCompareFiles(null);
    setStashDialogOpen(false);
    setUnstashDialogOpen(false);
    setDiffAgainstRef(null);
    setCompareWorktree(false);
    setShelveDialogOpen(false);
    setRebaseBase(null);
    setConflictDialogOpen(false);
    setActiveBottomTool(null);
    setActiveDrawer('project');
    if (prevPath && prevSnap) {
      // 恢复上一个打开中的项目会话
      delete projectSessionsRef.current[prevPath];
      buffersRef.current = { ...prevSnap.buffers };
      setBuffers(buffersRef.current);
      setFileTree(prevSnap.fileTree);
      setWorkspaceIsFile(prevSnap.workspaceIsFile ?? false);
      setTreeExpanded(new Set(prevSnap.treeExpanded));
      setOpenFilePaths(prevSnap.openFilePaths);
      setActiveFilePath(prevSnap.activeFilePath);
      setMarkdownViewModes({ ...prevSnap.markdownViewModes });
      setMarkdownScrollRatios({ ...prevSnap.markdownScrollRatios });
      setActiveTab(prevSnap.activeTab === 'compare-branches' ? 'editor' : prevSnap.activeTab);
      setDiffOpen(prevSnap.diffOpen);
      setDiffCommit(prevSnap.diffCommit);
      setDiffAgainstRef(prevSnap.diffAgainstRef);
      setDiffStaged(prevSnap.diffStaged);
      setDiffFilePath(prevSnap.diffFilePath || '');
      setFsCompareFiles(prevSnap.fsCompareFiles);
      setWorkspacePath(prevPath);
      saveRecentWorkspace(prevPath);
      // 切回的项目也可能有自己的项目层：重读（门：期间又切走则丢弃）
      refreshProjectLayer(prevPath, () => workspacePathRef.current === prevPath);
      showStatus(t().status.projectClosedBackTo(baseNameOf(prevPath)));
    } else {
      clearLastWorkspace();
      // 无项目可回：清掉已关项目的层，回落 user 层
      clearProjectLayer();
      setSettings(loadSettings());
      showStatus(t().status.projectClosedNoneLeft);
    }
  }, [workspacePath, workspaceIsFile, refreshProjectLayer]);

  const handleNewWindow = useCallback(() => {
    if (isTauriEnvironment()) {
      void invokeCommand('create_new_window');
    }
  }, []);

  // ── TitleBar：稳定回调（memo 生效前提；实现始终取最新闭包）──
  const switchWorkspaceStable = useStableCallback((path: string) => switchToProject(path));
  const selectBranchStable = useStableCallback(
    (branchName: string) => void handleSelectBranch(branchName)
  );
  const titleBarBranchContextStable = useStableCallback(
    (branch: GitBranch, x: number, y: number, side?: 'right' | 'left') =>
      setBranchMenu({ branch, x, y, side })
  );
  const closeBranchContextStable = useStableCallback(() => setBranchMenu(null));
  const searchClickStable = useStableCallback(() => setIsSearchOpen(true));
  // 无工作区时 TitleBar 不显示关闭入口：条件留在 JSX，函数体与原来一致
  const closeProjectStable = useStableCallback(() => closeWorkspace());

  /**
   * 跳转到行（原生菜单 Navigate › 跳转到行…，加速键 Cmd+L）。
   *
   * 该菜单项此前没有对应 handler，点了没反应：菜单的加速键会被系统菜单先截获，
   * Monaco 内的 Cmd+L 绑定因此也不生效。这里把动作接回既有的跳转通道
   * （openFile 的 line 参数 → gotoLine → EditorView 的 revealLineInCenter）。
   * 先置 null 再写目标行：对同一行连按两次时值不变，EditorView 的依赖更新不会
   * 触发，中间隔一次渲染把同值跳转变成两次变化。
   */
  const handleGotoLine = useCallback(async () => {
    const path = activeFilePath;
    if (!path) return;
    const input = await appPrompt(t().git.goToLinePrompt);
    if (input === null) return;
    const line = Number.parseInt(input, 10);
    if (!Number.isFinite(line) || line <= 0) return;
    setGotoLine(null);
    window.setTimeout(() => void openFile(path, line), 0);
  }, [activeFilePath, openFile, appPrompt]);
  const hasDirtyBuffers = useCallback(() => {
    return Object.values(buffersRef.current).some(b => b.dirty);
  }, []);

  const confirmDiscardDirty = useCallback(async () => {
    const dirtyCount = Object.values(buffersRef.current).filter(b => b.dirty).length;
    if (dirtyCount <= 0) return true;
    return await appConfirm(t().dialog.unsavedOnClose(dirtyCount), {
      title: t().updater.saveBeforeRestartTitle,
      kind: 'warning',
      okLabel: t().updater.restartNow,
    });
  }, []);

  const handleCheckForUpdates = useCallback((silent = false) => {
    if (silent) {
      void checkForAppUpdate()
        .then(info => {
          const latest = loadSettings();
          persistSettings({ ...latest, lastUpdateCheckTime: Date.now() });
          if (!info) {
            setAvailableUpdate(null);
            setBackgroundUpdateToast(null);
            return;
          }
          if (latest.ignoredUpdateVersion && latest.ignoredUpdateVersion === info.version) {
            setAvailableUpdate(null);
            setBackgroundUpdateToast(null);
            return;
          }
          setAvailableUpdate(info);
          setBackgroundUpdateToast(info);
        })
        .catch(() => {});
      return;
    }

    const latest = loadSettings();
    persistSettings({ ...latest, lastUpdateCheckTime: Date.now() });
    setPendingUpdateInfo(null);
    setUpdateDialogState('checking');
    setUpdateDialogOpen(true);
  }, []);

  const openAvailableUpdate = useCallback(() => {
    if (!availableUpdate) return;
    setPendingUpdateInfo(availableUpdate);
    setUpdateDialogState('available');
    setUpdateDialogOpen(true);
    setBackgroundUpdateToast(null);
  }, [availableUpdate]);

  useEffect(() => {
    const current = loadSettings();
    if (!current.autoCheckUpdates) return;
    const ONE_DAY = 24 * 60 * 60 * 1000;
    if (!current.lastUpdateCheckTime || Date.now() - current.lastUpdateCheckTime > ONE_DAY) {
      const timer = window.setTimeout(() => {
        handleCheckForUpdates(true);
      }, 5000);
      return () => clearTimeout(timer);
    }
  }, [handleCheckForUpdates]);


  // 原生应用菜单（File / View / Navigate / Git / Help）→ 既有动作。
  // Rust 侧只 emit 事件，业务全在这里复用，不产生第二份实现。
  useAppMenu({
    [MENU.NEW_WINDOW]: () => handleNewWindow(),
    [MENU.OPEN_PROJECT]: () => void pickWorkspace(),
    [MENU.CLOSE_PROJECT]: () => workspacePath && closeWorkspace(),
    [MENU.SAVE]: () => void saveActiveFile(),
    [MENU.SAVE_ALL]: () => void autoSaveAll(),
    [MENU.CLOSE_TAB]: () => {
      if (activeTab === 'compare-branches') handleCloseCompareBranches();
      else if (activeTab === 'diff') closeDiffView();
      else if (activeFilePath) closeFile(activeFilePath);
    },
    [MENU.CLOSE_ALL_TABS]: () => closeAllFiles(),
    [MENU.TOGGLE_PROJECT_DRAWER]: () => setActiveDrawer(d => (d === 'project' ? null : 'project')),
    [MENU.TOGGLE_COMMIT_DRAWER]: () => setActiveDrawer(d => (d === 'commit' ? null : 'commit')),
    [MENU.TOGGLE_GIT_PANEL]: () => setActiveBottomTool(t => (t === 'git' ? null : 'git')),
    [MENU.TOGGLE_TERMINAL]: () => setActiveBottomTool(t => (t === 'terminal' ? null : 'terminal')),
    [MENU.TOGGLE_SETTINGS]: () => setSettingsOpen(o => !o),
    [MENU.SEARCH_EVERYWHERE]: () => setIsSearchOpen(true),
    [MENU.GO_TO_LINE]: () => handleGotoLine(),
    [MENU.COMMIT]: () => workspacePath && isRepo && setActiveDrawer('commit'),
    [MENU.PUSH]: () => workspacePath && isRepo && setPushDialogOpen(true),
    [MENU.MANAGE_REMOTES]: () => workspacePath && isRepo && setRemotesDialogOpen(true),
    [MENU.UPDATE_PROJECT]: () => workspacePath && isRepo && setPullDialogOpen(true),
    [MENU.FETCH]: () => workspacePath && isRepo && void handleFetch(),
    [MENU.STASH]: () => workspacePath && isRepo && setStashDialogOpen(true),
    [MENU.UNSTASH]: () => workspacePath && isRepo && setUnstashDialogOpen(true),
    [MENU.SHELVE]: () => workspacePath && isRepo && setShelveDialogOpen(true),
    [MENU.RESOLVE_CONFLICTS]: () => {
      if (conflictCount > 0) {
        // 冲突对话框内含 3 个 monaco Editor，先确保内核就绪再打开
        requestMonaco(themeIdRef.current);
        resolveConflictsStable();
      }
    },
    [MENU.SHOW_HISTORY]: () => activeFilePath && setHistoryFile(activeFilePath),
    [MENU.REFRESH]: () => void refreshGit(),
    [MENU.CHECK_FOR_UPDATES]: () => handleCheckForUpdates(false),
  });

  // 全局快捷键：Ctrl/Cmd+W 关闭当前 Tab，Ctrl/Cmd+Shift+N 新建窗口，Ctrl/Cmd+, 打开设置，Esc 关闭，Ctrl+` 切换终端
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      // 原生模态（window.prompt / confirm / alert）挂着期间，按键的目标是
      // body 而非输入框，输入域豁免失效 —— 用户在 prompt 里打 Cmd+W
      // 会把后台的 tab 关掉。window.prompt 已整体迁走（见 TextPromptDialog），
      // 该标记不再被设置；保留此行仅作无害兜底。
      if (document.body.dataset.nativeModal === '1') return;
      // Cmd/Ctrl+S 保存：Monaco 的输入面是 textarea.inputarea，必须在输入域判定前拦截；
      // 终端例外（.terminal-canvas-host 内 Cmd+S 继续下发 PTY 流控）
      if (!target?.closest('.terminal-canvas-host') && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        if (e.repeat || e.isComposing) return;
        e.preventDefault();
        void saveActiveFile();
        return;
      }

      // 长按重复与输入法组合期过滤（注意此处为 lib.dom 原生 KeyboardEvent）
      if (e.repeat || e.isComposing) return;

      const inTerminal = Boolean(target?.closest('.terminal-canvas-host'));
      const inPlainInput = Boolean(target?.closest('input, textarea:not(.inputarea), select, [contenteditable]'));

      // 1. 工具窗口切换：同时支持 PyCharm 默认 Alt+1/0/9/F12 与 macOS 原生 Cmd+1/0/9/F12
      const isAltTool = e.altKey && !e.ctrlKey && !e.metaKey;
      const isCmdTool = (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey;
      if ((isAltTool || isCmdTool) && (e.code === 'Digit1' || e.code === 'Digit0' || e.code === 'Digit9' || e.code === 'F12')) {
        e.preventDefault();
        if (e.code === 'Digit1') {
          setActiveDrawer(d => (d === 'project' ? null : 'project'));
        } else if (e.code === 'Digit0') {
          setActiveDrawer(d => (d === 'commit' ? null : 'commit'));
        } else if (e.code === 'F12') {
          setActiveBottomTool(t => (t === 'terminal' ? null : 'terminal'));
        } else {
          setActiveBottomTool(t => (t === 'git' ? null : 'git'));
        }
        return;
      }

      // 2. 双击 Shift 打开 Search Everywhere（在 Monaco 或主工作区均生效，模态框开启时跳过）
      if (e.key === 'Shift' && !e.ctrlKey && !e.metaKey && !e.altKey && !isSearchOpen && !settingsOpen) {
        const now = Date.now();
        if (now - lastShiftAt.current < 400) {
          e.preventDefault();
          setIsSearchOpen(true);
          lastShiftAt.current = 0;
        } else {
          lastShiftAt.current = now;
        }
        return;
      }

      // 3. 打开偏好设置 (Cmd/Ctrl + ,)
      if ((e.ctrlKey || e.metaKey) && e.key === ',') {
        e.preventDefault();
        setSettingsOpen(o => !o);
        return;
      }

      // 4. 切换终端控制台 (Cmd/Ctrl + `)
      if ((e.ctrlKey || e.metaKey) && e.key === '`') {
        e.preventDefault();
        setActiveBottomTool(t => (t === 'terminal' ? null : 'terminal'));
        return;
      }

      // 5. 全局查找 Find in Files (Cmd/Ctrl + Shift + F)
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        if (workspacePath) {
          setFindInFilesScope('');
          setFindInFilesOpen(true);
        }
        return;
      }

      // 6. 新建窗口 (Cmd/Ctrl + Shift + N)
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        handleNewWindow();
        return;
      }

      // 7. Git 推送 Push (Cmd/Ctrl + Shift + K) - 终端除外
      if (!inTerminal && (e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        if (workspacePath && isRepo) setPushDialogOpen(true);
        return;
      }

      // 8. Git 更新 Update Project (Cmd/Ctrl + T) - 终端与普通输入框除外
      if (!inTerminal && !inPlainInput && (e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 't') {
        e.preventDefault();
        if (workspacePath && isRepo) setPullDialogOpen(true);
        return;
      }

      // 9. 关闭当前 Tab (Cmd/Ctrl + W)
      // 终端保留自己的 shell 流控 \x17，对话框输入框内不关 Tab；Monaco 内或常规区域正常关闭
      if (!inTerminal && !inPlainInput && (e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 'w') {
        if (activeTab === 'diff') {
          e.preventDefault();
          closeDiffView();
          return;
        } else if (activeFilePath) {
          e.preventDefault();
          closeFile(activeFilePath);
          return;
        }
      }

      // 10. Escape 按层级关闭模态弹窗
      if (e.key === 'Escape') {
        if (isSearchOpen) {
          setIsSearchOpen(false);
          return;
        } else if (settingsOpen) {
          setSettingsOpen(false);
          return;
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [settingsOpen, isSearchOpen, activeFilePath, activeTab, closeFile, closeDiffView, handleNewWindow, saveActiveFile, workspacePath, isRepo]);

  /** 工作区显示名：取路径最后一段，未打开时显示项目名称 */
  const workspaceName =
    workspacePath
      .split(/[\\/]/)
      .filter(p => p && p !== '.')
      .pop() || 'LazeCode';

  /** 状态栏面包屑：相对工作区根目录（对齐 PyCharm「项目 › src › App.tsx」样式） */
  const statusCrumbs = (() => {
    const target = activeFilePath || workspacePath;
    if (!target) return ['LazeCode'];
    const root = workspacePath.replace(/[\\/]+$/, '');
    let display = target;
    if (root) {
      const normTarget = target.replace(/\\/g, '/');
      const normRoot = root.replace(/\\/g, '/');
      if (normTarget.toLowerCase() === normRoot.toLowerCase()) {
        display = '';
      } else if (normTarget.toLowerCase().startsWith(`${normRoot.toLowerCase()}/`)) {
        display = normTarget.slice(normRoot.length + 1);
      }
    }
    const parts = display.split(/[\\/]/).filter(Boolean);
    return parts.length > 0 ? parts : [workspaceName];
  })();
  /**
   * 状态栏主提示文字：Git 反馈优先于普通状态消息。
   *
   * 两者都是「刚才发生了什么」的同位通知，但 Git 消息来自提交/推送/分支删除等
   * 不可逆或难撤销的操作，用户最需要看到它的结果，所以同时存在时只显示 Git 的；
   * 普通消息（保存、复制、创建文件）即便被挤掉也可以从操作本身推断出来。
   */
  const statusbarText = gitMessage ?? statusMessage;
  const compareBranchesTab = useMemo(
    () =>
      compareBranches
        ? { left: compareBranches.left.name, right: compareBranches.right.name }
        : null,
    [compareBranches]
  );
  const markdownOpenFileStable = useStableCallback((path: string) => void openFile(path));
  const sourceEditor = activeFilePath ? (
    // P2 分页只读：不挂 Monaco，直接渲染当页 <pre> + 翻页器
    activeBuffer?.paged ? (
      <PagedFileViewer
        pageText={codeContent}
        totalLines={activeBuffer.paged.totalLines}
        page={activeBuffer.paged.page}
        pageSize={activeBuffer.paged.pageSize}
        fileLen={activeBuffer.paged.fileLen}
        loading={pagedLoading}
        onPage={page => void goToPagedPage(activeFilePath, page)}
        onOpenFull={() => openPagedFull(activeFilePath)}
      />
    ) : monacoReady || isImageFile(activeFilePath) || activeBuffer?.error ? (
      <React.Suspense
        fallback={
          <div className="editor-lazy-placeholder">
            <div className="editor-lazy-spinner" />
            <div className="editor-lazy-label">{t().git.loadingEditor}</div>
          </div>
        }
      >
        {/* 工作区切换（项目 ↔ 单文件）会清空并 dispose 全部 Monaco model；
            强制重挂编辑器，避免 @monaco-editor/react 在模型切换的瞬时窗口里
            对 null model 调 getFullModelRange（实测单文件打开时整页崩溃）。 */}
        <EditorView
          key={workspacePath}
          filePath={activeFilePath}
          code={codeContent}
          language={editorLanguage}
          fontSize={settings.editorFontSize}
          gotoLine={gotoLine}
          themeId={themeIdRef.current}
          loadError={activeBuffer?.error ?? null}
          loadErrorCode={activeBuffer?.errorCode ?? null}
          onRevealFile={revealActiveFile}
          onForceOpen={forceOpenFile}
          repoPath={gitRepoRoot || workspacePath}
          blameOn={blameOn}
          onToggleBlame={() => setBlameOn(v => !v)}
          onShowHistory={() => activeFilePath && setHistoryFile(activeFilePath)}
          onOpenCommitDiff={hash => openCommitDiff(hash)}
          onChange={handleEditorChange}
          onSave={saveActiveFile}
          onCursorChange={setCursorRef}
          onTabSizeChange={setTabSize}
          scrollRatio={isMarkdownFile(activeFilePath) ? activeMarkdownScrollRatio : undefined}
          scrollRestoreKey={isMarkdownFile(activeFilePath) ? activeMarkdownMode : undefined}
          active={!isMarkdownFile(activeFilePath) || activeMarkdownMode !== 'preview'}
          onScrollChange={isMarkdownFile(activeFilePath) ? ratio => handleMarkdownScroll(activeFilePath, ratio) : undefined}
        />
      </React.Suspense>
    ) : (
      <div
        style={{
          height: '100%',
          width: '100%',
          display: 'flex',
          flexDirection: 'column',
          gap: '10px',
          padding: '12px 16px',
          boxSizing: 'border-box',
          overflow: 'hidden'
        }}
      >
        {Array.from({ length: 14 }, (_, i) => (
          <div
            key={i}
            style={{
              height: '12px',
              width: `${35 + ((i * 37) % 60)}%`,
              borderRadius: '3px',
              backgroundColor: 'var(--bg-header, #2b2d30)',
              opacity: 0.55,
              flexShrink: 0
            }}
          />
        ))}
      </div>
    )
  ) : null;

  return (
    <div style={{ height: '100vh', width: '100vw', display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      {/* 1. Titlebar */}
      <TitleBar
        projectName={workspacePath ? workspaceName : ''}
        workspacePath={workspacePath}
        recentWorkspaces={recentWorkspaces}
        onOpenProject={pickWorkspace}
        onSwitchWorkspace={switchWorkspaceStable}
        sessionProjects={sessionProjects}
        otherWindows={otherWindows}
        onFocusWindow={focusWindow}
        onProjectMenuOpen={refreshOtherWindows}
        onCloseProject={workspacePath ? closeProjectStable : undefined}
        currentBranch={currentBranch}
        branches={branches}
        tags={tags}
        onSelectBranch={selectBranchStable}
        onCheckoutTag={name => void handleCheckoutTag(name)}
        onDeleteTag={tag => void handleDeleteTag(tag)}
        onCheckoutRef={() => void handleCheckoutRefPrompt()}
        onCreateBranch={handleCreateBranch}
        favorites={favoriteBranches}
        onBranchContext={titleBarBranchContextStable}
        onCloseBranchContext={closeBranchContextStable}
        onRemoveRecent={removeRecentWorkspace}
        platform={platform}
        onSearchClick={searchClickStable}
        availableUpdateVersion={availableUpdate?.version}
        onAvailableUpdateClick={openAvailableUpdate}
        onNewWindow={handleNewWindow}
        sequencer={titleBarSequencer}
        conflictCount={conflictCount}
        onResolveConflicts={resolveConflictsStable}
        onContinueCherryPick={continueCherryPickStable}
        onContinueMerge={continueMergeStable}
        onContinueRebase={continueRebaseStable}
        onSkipRebase={skipRebaseStable}
        onAbortMerge={abortMergeStable}
        onAbortCherryPick={abortCherryPickStable}
        onAbortRebase={abortRebaseStable}
        onUpdateProject={pullStable}
        onOpenCommit={() => setActiveDrawer(prev => prev === 'commit' ? null : 'commit')}
        onPush={pushStable}
        isRepo={isRepo}
      />
      {/* 2. Main IDE Body */}
      <div style={{ flex: 1, minHeight: 0, display: 'flex', overflow: 'hidden' }}>
        {/* 2.1. Left Tool Strip */}
        <LeftStrip
          activeDrawer={activeDrawer}
          activeBottomTool={activeBottomTool}
          onToggleDrawer={setActiveDrawer}
          onToggleBottomTool={tool =>
            setActiveBottomTool(activeBottomTool === tool ? null : tool)
          }
          onOpenSettings={() => setSettingsOpen(true)}
          settingsOpen={settingsOpen}
          changeCount={changedFiles.filter(f => f.status !== 'I').length}
          isGitRepo={isRepo}
          hasCompareSession={!!compareSession}
          compareTooltip={compareSession ? `Changes Between ${compareSession.fromCommit.shortHash || compareSession.fromCommit.hash.slice(0, 8)} and ${compareSession.toCommit.shortHash || compareSession.toCommit.hash.slice(0, 8)}` : undefined}
          terminalRunning={terminalRunning}
        />
        {/* 2.2. Left Drawer (Commit or Project)。CommitDrawer 关闭时只隐藏不卸载：
            提交信息、勾选文件是组件本地草稿，PyCharm 关闭 Commit 工具窗口后仍保留。 */}
        {workspacePath && isRepo && (
          <div style={{ display: activeDrawer === 'commit' ? 'contents' : 'none' }}>
            <CommitDrawer
              key={workspacePath}
              changedFiles={changedFiles}
              selectedFile={activeTab === 'diff' ? (diffFilePath || activeFilePath) : activeFilePath}
              onSelectFileForDiff={handleSelectFileForDiff}
              onCommit={handleCommit}
              onCommitAndPush={handleCommitAndPush}
              onDiscardFile={discardFileStable}
              onAddToVcs={addToVcsStable}
              onDeleteUntracked={handleDeleteUnversioned}
              onShelve={shelveStable}
              changelists={changelists.lists}
              fileListMap={changelists.files}
              onCreateChangelist={createChangelistStable}
              onDeleteChangelist={deleteChangelistStable}
              onAssignToChangelist={assignToChangelistStable}
              onUnshelve={unshelveStable}
              repoPath={gitRepoRoot || workspacePath}
              onShelfChanged={handleShelfChanged}
              onCommitFile={commitFileStable}
              onCommitPaths={commitPathsStable}
              onJumpToSource={jumpToSourceStable}
              onCompareBeforeLocal={path => openCompareWithLocal('HEAD', path)}
              onFileHistory={path => setHistoryFile(path)}
              onCopyLocalPatch={copyLocalPatchStable}
              onCreateLocalPatch={createLocalPatchStable}
              conflictCount={conflictCount}
              onResolveConflicts={resolveConflictsStable}
              onShelfDiff={shelfDiffStable}
              recentMessages={commitMessages}
              onClearHistory={clearCommitMessages}
              onPromptName={appPrompt}
              busy={gitBusy}
              refreshing={gitRefreshing}
              isRepo={isRepo}
              width={commitDrawerWidth}
              onRefresh={refreshGit}
              onClose={closeCommitDrawerStable}
            />
            <div
              className="drawer-resizer-v"
              onPointerDown={startResizeCommitDrawer}
              title={t().git.dragResizeCommitWidth}
            />
          </div>
        )}

        {activeDrawer === 'project' && (
          <>
            <ProjectDrawer
              rootName={workspaceName}
              rootPath={workspacePath}
              repoRoot={isRepo ? (gitRepoRoot || workspacePath) : undefined}
              files={fileTree}
              singleFile={workspaceIsFile}
              ignoredPatterns={settings.ignoredPatterns}
              externalRoots={externalRoots}
              activeFile={activeTab === 'diff' ? (diffFilePath || activeFilePath) : activeFilePath}
              width={projectDrawerWidth}
              changedFiles={changedFiles}
              expandedPaths={treeExpanded}
              onSetExpandedPaths={setTreeExpanded}
              onSelectFile={handleSelectFile}
              onOpenDirectory={pickWorkspace}
              onOpenExternalDirectory={openExternalDirectoryStable}
              onCloseExternalDirectory={closeExternalRoot}
              onLocateActive={locateActiveStable}
              autoLocate={autoLocateTree}
              onToggleAutoLocate={toggleAutoLocateTree}
              onUpdateFileNode={updateFileNodeStable}
              onNodeContext={nodeContextStable}
              onSelectionChange={setTreeSelection}
            />
            <div
              className="drawer-resizer-v"
              onPointerDown={startResizeProjectDrawer}
              title={t().git.dragResizeTreeWidth}
            />
          </>
        )}

        {activeDrawer === 'compare' && compareSession && (
          <>
            <CompareDrawer
              repoPath={gitRepoRoot || workspacePath}
              fromCommit={compareSession.fromCommit}
              toCommit={compareSession.toCommit}
              onClose={handleCloseCompareSession}
              onMinimize={() => setActiveDrawer(null)}
              onSwapCommits={handleSwapCompareCommits}
              onSelectFileDiff={handleSelectCompareFile}
              activeFilePath={activeFilePath}
              width={projectDrawerWidth}
            />
            <div
              className="drawer-resizer-v"
              onPointerDown={startResizeProjectDrawer}
              title={t().git.dragResizeCommitWidth}
            />
          </>
        )}

        {/* 2.3. Center Workspace (Editor / Diff + Bottom Panels) */}
        {/* terminal-maximized 只在控制台可见时生效：隐藏最大化面板必须把主区还回来 */}
        <div className={`ide-center-workspace${terminalPlacement === 'right' ? ' terminal-docked-right' : ''}${terminalPlacement === 'right' && terminalMaximized && activeBottomTool === 'terminal' ? ' terminal-maximized' : ''}`}>
          {/* Welcome Screen when no workspace is opened */}
          <div
            className="ide-main-region"
            style={{
              display:
                (gitPanelMaximized && activeBottomTool === 'git') ||
                (terminalMaximized && activeBottomTool === 'terminal' && terminalPlacement === 'bottom')
                  ? 'none'
                  : 'flex'
            }}
          >
          {!workspacePath ? (
            <WelcomeView
              onOpenProject={pickWorkspace}
              onCloneRepository={() => setCloneOpen(true)}
              onOpenRecent={path => void loadWorkspace(path)}
              onRemoveRecent={removeRecentWorkspace}
              recentProjects={recentWorkspaces}
              onSearchClick={() => setIsSearchOpen(true)}
              onOpenTerminal={() => setActiveBottomTool('terminal')}
              onOpenSettings={() => setSettingsOpen(true)}
              onNewWindow={handleNewWindow}
            />
          ) : (
            <>
              {/* Multi-Tab Bar with Context Menu */}
              <EditorTabBar
                openFiles={openFilePaths}
                activeFile={activeFilePath}
                activeTab={activeTab}
                diffOpen={diffOpen}
                diffCommit={diffCommit}
                compareCommits={diffCompareCommits}
                selectedCommits={diffSelectedCommits}
                compareBranches={compareBranchesTab}
                dirtyFiles={dirtyPaths}
                onSelectTab={selectTabStable}
                onSelectDiffTab={selectDiffTabStable}
                onSelectCompareBranches={selectCompareBranchesStable}
                onCloseCompareBranches={handleCloseCompareBranches}
                onCloseTab={closeFile}
                onCloseOtherTabs={closeOtherFiles}
                onCloseLeftTabs={closeLeftFiles}
                onCloseRightTabs={closeRightFiles}
                onCloseAllTabs={closeAllFiles}
                onCloseDiffTab={closeDiffView}
              />

              {/* Center Canvas: Either Code Editor or Diff Viewer or Empty State */}
              <div className="ide-center-canvas">
                {/* Compare 视图常驻：切到 Diff / 编辑器时不卸载，返回时保留
                    提交、文件选中、滚动位置与展开状态。 */}
                {compareBranches && (
                  <div
                    className={`compare-branches-canvas${activeTab === 'compare-branches' ? '' : ' is-hidden'}`}
                    aria-hidden={activeTab !== 'compare-branches'}
                  >
                    <CompareBranchesView
                      key={`${compareBranches.left.name}\u0000${compareBranches.right.name}`}
                      repoPath={gitRepoRoot || workspacePath}
                      repoName={workspaceName}
                      leftRef={compareBranches.left.name}
                      rightRef={compareBranches.right.name}
                      onSwap={handleSwapCompareBranches}
                      onOpenCommitDiff={openCompareBranchesCommitDiff}
                      onOpenCompareDiff={openCompareBranchesRangeDiff}
                    />
                  </div>
                )}
                {activeTab === 'compare-branches' ? null : activeTab === 'diff' ? (
                  <DiffViewer
                    filePath={
                      diffFilePath ||
                      (diffCommit || diffAgainstRef || diffCompareCommits || diffSelectedCommits || fsCompareFiles
                        ? ''
                        : activeFilePath)
                    }
                    repoPath={gitRepoRoot || workspacePath}
                    staged={diffStaged}
                    showCommit={diffCommit}
                    againstRef={diffAgainstRef}
                    compareCommits={diffCompareCommits}
                    selectedCommits={diffSelectedCommits}
                    fsFiles={fsCompareFiles}
                    refreshKey={diffRefreshKey}
                    themeId={themeIdRef.current}
                    fontSize={settings.editorFontSize}
                    onNavigateFile={path => {
                      setDiffFilePath(path);
                      enterDiff(diffReturnTabRef.current);
                    }}
                    onWorkingTreeChanged={handleHunkChanged}
                    onStagedChange={setDiffStaged}
                  />
                ) : activeFilePath && openFilePaths.length > 0 ? (
                  <div className="editor-pane">
                    {activeBuffer?.externalChanged && (
                      <div className="external-change-banner">
                        <span>⚠ 文件已在磁盘上被修改（或无法读取），与本地未保存的修改冲突</span>
                        <span className="external-change-actions">
                          <button className="pycharm-btn-primary" onClick={() => void dismissExternalChange()}>
                            保留我的修改
                          </button>
                          <button
                            className="pycharm-btn-secondary"
                            onClick={() => activeFilePath && void checkExternalChange(activeFilePath, { forceReload: true })}
                          >
                            重载磁盘版本（放弃本地）
                          </button>
                        </span>
                      </div>
                    )}
                    {isMarkdownFile(activeFilePath) && !activeBuffer?.error && !activeBuffer?.paged ? (
                      <React.Suspense
                        fallback={
                          <div className="editor-lazy-placeholder">
                            <div className="editor-lazy-spinner" />
                            <div className="editor-lazy-label">{t().git.loadingMarkdown}</div>
                          </div>
                        }
                      >
                        <MarkdownView
                          filePath={activeFilePath}
                          content={codeContent}
                          mode={activeMarkdownMode}
                          onModeChange={markdownModeChangeStable}
                          onOpenFile={markdownOpenFileStable}
                          source={sourceEditor}
                          scrollRatio={activeMarkdownScrollRatio}
                          onScrollChange={markdownScrollChangeStable}
                          workspaceRoot={workspacePath}
                        />
                      </React.Suspense>
                    ) : (
                      sourceEditor
                    )}
                  </div>
                ) : (
                  <div className="editor-empty-placeholder">
                    <div className="empty-app-title">LazeCode</div>
                    <div className="empty-shortcuts-list">
                      <div className="empty-shortcut-row">
                        <span>全局搜索</span>
                        <kbd>⇧ ⇧</kbd>
                      </div>
                      <div className="empty-shortcut-row">
                        <span>打开内置终端</span>
                        <kbd>{'⌥ F12 / ⌘ `'}</kbd>
                      </div>
                      <div className="empty-shortcut-row">
                        <span>偏好设置</span>
                        <kbd>⌘ ,</kbd>
                      </div>
                      <div className="empty-shortcut-row">
                        <span>从左侧文件树选择文件打开</span>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </>
          )}
          </div>

          {/* 2.4. Bottom Tool Windows：常驻挂载，仅切换可见性。
              终端 PTY 与画布因此跨「打开/隐藏」保持会话与滚动缓冲；
              只有用户手动关闭终端标签才会终止对应会话。 */}
          <div
            className={`ide-bottom-panel-wrapper ${gitPanelMaximized ? 'is-maximized' : ''}`}
            style={{
              height: gitPanelMaximized ? `${workspaceMaxH()}px` : `${Math.min(gitPanelHeight, workspaceMaxH())}px`,
              flex: gitPanelMaximized ? '1 1 auto' : undefined,
              flexShrink: 0,
              display: activeBottomTool === 'git' ? 'flex' : 'none',
              flexDirection: 'column',
              position: 'relative'
            }}
          >
            {!gitPanelMaximized && (
              <div
                className="bottom-panel-resizer"
                onPointerDown={startResizeBottomPanel}
                title={t().git.dragResizePanelHeight}
              />
            )}
            <div style={{ display: activeBottomTool === 'git' ? 'flex' : 'none', flex: 1, minHeight: 0, flexDirection: 'column' }}>
              <GitBottomPanel
                repoPath={gitRepoRoot || workspacePath}
                repoName={workspaceName}
                commits={commits}
                branches={branches}
                tags={tags}
                currentBranch={currentBranch}
                onSelectBranch={selectBranchStable}
                onCheckoutTag={name => void handleCheckoutTag(name)}
                onCompareBranches={handleCompareBranches}
                onUpdateSelected={handleUpdateSelected}
                onDeleteSelected={handleDeleteSelected}
                onDeleteTag={name => {
                  const tag = tags.find(tg => tg.name === name);
                  if (tag) void handleDeleteTag(tag);
                }}
                onCreateBranch={handleCreateBranchPrompt}
                onToggleFavorite={toggleFavoriteBranch}
                sortMode={logSortMode}
                onSortModeChange={setLogSortMode}
                firstParent={logFirstParent}
                onFirstParentChange={setLogFirstParent}
                noMerges={logNoMerges}
                onNoMergesChange={setLogNoMerges}
                busy={gitBusy}
                refreshing={gitRefreshing}
                isRepo={isRepo}
                onInitRepo={initRepoStable}
                onPush={pushStable}
                onPull={pullStable}
                onFetch={fetchStable}
                onStash={stashStable}
                onUnstash={unstashStable}
                onShelf={shelfStable}
                conflictCount={conflictCount}
                onResolveConflicts={resolveConflictsStable}
                onLoadMore={loadMoreStable}
                hasMore={hasMoreCommits}
                loadingMore={logLoadingMore}
                focusCommit={focusCommit}
                maximized={gitPanelMaximized}
                onToggleMaximize={toggleMaximizeStable}
                onRefresh={refreshGitStable}
                onSelectCommit={selectCommitStable}
                onOpenCommitDiff={openCommitDiffStable}
                onOpenFileDiff={openFileDiffStable}
                onOpenCommitsDiff={openCommitsDiffStable}
                onCompareWithLocal={compareWithLocalStable}
                onGetFromRevision={getFromRevisionStable}
                activeFile={activeTab === 'diff' ? (diffFilePath || activeFilePath) : activeFilePath}
                onCopyCommitPatch={copyCommitPatchStable}
                onCreateCommitPatch={createCommitPatchStable}
                onRevertCommit={revertCommitStable}
                favorites={favoriteBranches}
                onBranchContext={bottomBranchContextStable}
                onCommitContext={commitContextStable}
                logBranch={logBranch}
                onLogBranchChange={setLogBranch}
                logPath={logPath}
                onLogPathChange={setLogPath}
                userFilter={logUser}
                onUserFilterChange={setLogUser}
                dateFilter={logDate}
                onDateFilterChange={setLogDate}
                onClose={closeGitPanelStable}
                onOpenTools={() => setGitToolsOpen(true)}
              />
            </div>

          </div>

          {/* 控制台停靠容器：底部 ⇄ 右侧只切换 class 与内联尺寸，同一 DOM 节点不卸载，
              PTY 会话因此跨停靠位置保留；拖拽吸附与宽度调节逻辑见组件顶部 */}
          <div
            className="ide-terminal-dock"
            style={
              terminalPlacement === 'right'
                ? {
                    // 最大化：占满整行（主区被 CSS 压成 0 宽，内容同时被 dominant 擦除）
                    width: terminalMaximized
                      ? '100%'
                      : `${Math.min(terminalDockWidth, Math.max(240, window.innerWidth - 360))}px`,
                    display: activeBottomTool === 'terminal' ? 'flex' : 'none'
                  }
                : {
                    height: terminalMaximized ? `${workspaceMaxH()}px` : `${Math.min(terminalPanelHeight, workspaceMaxH())}px`,
                    flex: terminalMaximized ? '1 1 auto' : undefined,
                    display: activeBottomTool === 'terminal' ? 'flex' : 'none'
                  }
            }
          >
            {(!terminalMaximized || terminalPlacement === 'right') && (
              <div
                className="terminal-dock-resizer dock-bottom"
                onPointerDown={startResizeBottomPanel}
                title={t().git.dragResizeConsoleHeight}
              />
            )}
            <div
              className="terminal-dock-resizer dock-right"
              onPointerDown={startResizeTerminalDock}
              title={t().git.dragResizeConsoleWidth}
            />
            <GhosttyTerminal
              ref={terminalRef}
              cwd={workspacePath || '.'}
              spawnCwd={workspaceIsFile ? parentOfPath(workspacePath) : workspacePath || '.'}
              projectKey={workspacePath}
              openProjects={sessionProjects.map(p => p.path)}
              onNotice={showStatus}
              onLiveProjectsChange={onLiveProjectsChangeStable}
              onBusyChange={onTerminalBusyChangeStable}
              onActivePwdChange={onTerminalCwdChangeStable}
              fontSize={settings.terminalFontSize}
              themeKey={`${settings.uiVariant}:${settings.themeId}`}
              visible={activeBottomTool === 'terminal'}
              placement={terminalPlacement}
              snapTarget={terminalSnapTarget}
              onGripPointerDown={startTerminalSnapDrag}
              onTogglePlacement={() => applyTerminalPlacement(terminalPlacement === 'right' ? 'bottom' : 'right')}
              maximized={terminalMaximized}
              onToggleMaximize={toggleTerminalMaximizeStable}
              onClose={() => setActiveBottomTool(null)}
            />
          </div>

          {/* 控制台拖拽吸附预览：高亮即将停靠的目标区域 */}
          {terminalSnapTarget && terminalSnapZoneRef.current && (
            <div
              className={`terminal-snap-preview is-${terminalSnapTarget}`}
              style={
                terminalSnapTarget === 'right'
                  ? {
                      left: terminalSnapZoneRef.current.left + terminalSnapZoneRef.current.width / 2,
                      top: terminalSnapZoneRef.current.top,
                      width: terminalSnapZoneRef.current.width / 2,
                      height: terminalSnapZoneRef.current.height
                    }
                  : {
                      left: terminalSnapZoneRef.current.left,
                      top: terminalSnapZoneRef.current.top + terminalSnapZoneRef.current.height / 2,
                      width: terminalSnapZoneRef.current.width,
                      height: terminalSnapZoneRef.current.height / 2
                    }
              }
            />
          )}
          {/* Status Bar */}
          <footer className="ide-statusbar">
            <div className="statusbar-left">
              {activeBottomTool === 'terminal' && terminalCwd && (
                <span className="statusbar-terminal-cwd" title={terminalCwd}>
                  {terminalCwd}
                </span>
              )}
              {statusCrumbs.map((part, i, arr) => (
                <span key={i} className="statusbar-crumb-wrap">
                  <span className={`statusbar-crumb ${i === arr.length - 1 ? 'active' : ''}`}>
                    {part}
                  </span>
                  {i < arr.length - 1 && <span className="statusbar-crumb-sep">›</span>}
                </span>
              ))}
            </div>
            <div className="statusbar-right">
              {statusbarText && <span className="statusbar-item statusbar-message">{statusbarText}</span>}
              {dirtyPaths.length > 0 && (
                <span className="statusbar-item statusbar-dirty">{t().git.dirtyFilesHint(dirtyPaths.length)}</span>
              )}
              {cursor && activeTab === 'editor' && activeFilePath && (
                <span className="statusbar-item">{cursor.line}:{cursor.column}</span>
              )}
              <span className="statusbar-item">{openFilePaths.length > 0 ? t().git.tabCount(openFilePaths.length) : t().git.ready}</span>
              <span className="statusbar-item">{activeBuffer?.encoding ?? 'UTF-8'}</span>
              <span className="statusbar-item">{tabSize ? t().git.spacesCount(tabSize) : t().git.indentFollowsFile}</span>
              <span className="statusbar-item active-lang">{editorLanguage}</span>
            </div>
          </footer>
        </div>
      </div>

      {/* 3. Global Modals */}
      <SearchEverywhereModal
        isOpen={isSearchOpen}
        workspacePath={workspacePath}
        openFiles={openFilePaths}
        onClose={() => setIsSearchOpen(false)}
        onOpenFile={(path, line) => {
          void openFile(path, line);
        }}
        onRunAction={id => {
          if (id === 'open-project') void pickWorkspace();
          else if (id === 'find-in-files') {
            setFindInFilesScope('');
            setFindInFilesOpen(true);
          }
          else if (id === 'commit' && isRepo) setActiveDrawer('commit');
          else if (id === 'push' && isRepo) setPushDialogOpen(true);
          else if (id === 'update' && isRepo) setPullDialogOpen(true);
          else if (id === 'fetch' && isRepo) void handleFetch();
          else if (id === 'git-log') setActiveBottomTool('git');
          else if (id === 'terminal') setActiveBottomTool('terminal');
          else if (id === 'settings') setSettingsOpen(true);
          else if (id === 'git-tools' && isRepo) setGitToolsOpen(true);
          else if (id === 'refresh') void refreshGit();
        }}
      />

      <FindInFilesModal
        open={findInFilesOpen && !!workspacePath}
        workspacePath={workspacePath}
        initialScope={
          findInFilesScope === workspacePath ||
          (!!workspacePath && findInFilesScope.startsWith(workspacePath.replace(/[\\/]+$/, '') + '/'))
            ? findInFilesScope
            : ''
        }
        onClose={() => setFindInFilesOpen(false)}
        onBeforeReplace={autoSaveAll}
        onOpenFile={(path, line) => void openFile(path, line)}
        onReplaced={paths => void handleFilesReplaced(paths)}
      />

      <CloneDialog
        open={cloneOpen}
        onClose={() => setCloneOpen(false)}
        onCloned={path => void loadWorkspace(path)}
      />

      <GitToolsDialog
        open={gitToolsOpen && !!workspacePath}
        repoPath={gitRepoRoot || workspacePath}
        onClose={() => setGitToolsOpen(false)}
        onChanged={() => void refreshGit()}
      />

      {/* Push 对话框（对齐 PyCharm Ctrl+Shift+K） */}
      <React.Suspense fallback={null}>
        <PushDialog
          open={pushDialogOpen && !!workspacePath}
          repoPath={gitRepoRoot || workspacePath}
          currentBranch={currentBranch}
          branches={branches}
          onClose={() => setPushDialogOpen(false)}
          onPushed={output => void handlePushed(output)}
          onManageRemotes={() => setRemotesDialogOpen(true)}
          remotesNonce={remotesNonce}
        />
      </React.Suspense>

      {/* 远程管理对话框（对齐 PyCharm Git → Manage Remotes…） */}
      <React.Suspense fallback={null}>
        <RemotesDialog
          open={remotesDialogOpen && !!workspacePath}
          repoPath={gitRepoRoot || workspacePath}
          onClose={() => setRemotesDialogOpen(false)}
          onChanged={() => setRemotesNonce(n => n + 1)}
        />
      </React.Suspense>

      {/* Update Project 对话框（对齐 PyCharm Ctrl+T） */}
      <React.Suspense fallback={null}>
        <PullDialog
          open={pullDialogOpen && !!workspacePath}
          currentBranch={currentBranch}
          busy={gitBusy}
          initialRebase={pullDialogRebase}
          onClose={() => setPullDialogOpen(false)}
          onUpdate={rebase => void handleUpdateProject(rebase)}
        />
      </React.Suspense>

      {/* 分支操作菜单（TitleBar 下拉点击滑出的子菜单 与 Git 面板分支树右键 共用） */}
      <BranchContextMenu
        menu={branchMenu}
        currentBranch={currentBranch}
        onClose={() => setBranchMenu(null)}
        onCheckout={b => void handleSelectBranch(b)}
        onCreateBranchFrom={refName => void handleCreateBranchFrom(refName)}
        onRename={oldName => void handleRenameBranch(oldName)}
        onDelete={branch => void handleDeleteBranch(branch)}
        onCompare={refName => { setCompareWorktree(false); setCompareRef(refName); }}
        onShowDiffWithWorktree={refName => { setCompareWorktree(true); setCompareRef(refName); }}
        onUpdate={branch => void handleBranchUpdate(branch.name)}
        onPush={() => setPushDialogOpen(true)}
        onMergeIntoCurrent={refName => void handleMergeIntoCurrent(refName)}
        onRebaseOntoCurrent={refName => void handleRebaseOntoCurrent(refName)}
        onPullInto={rebase => { setPullDialogRebase(rebase); setPullDialogOpen(true); }}
        onCheckoutAndRebase={(refName, current) => void handleCheckoutAndRebase(refName, current)}
        onCompareBranches={handleCompareBranches}
        onShowFilesDiff={handleShowFilesDiff}
        onUpdateSelected={refs => void handleUpdateSelected(refs)}
        onDeleteSelected={refs => void handleDeleteSelected(refs)}
        onCheckoutTag={name => void handleCheckoutTag(name)}
        onPushTag={name => void handlePushTag(name)}
        onDeleteTag={name => {
          const tag = tags.find(tg => tg.name === name);
          if (tag) void handleDeleteTag(tag);
        }}
      />

      {/* 分支比较对话框（Compare with Current） */}
      <React.Suspense fallback={null}>
        <CompareDialog
          open={!!compareRef && !!workspacePath}
          repoPath={gitRepoRoot || workspacePath}
          refName={compareRef ?? ''}
          currentBranch={currentBranch}
          worktree={compareWorktree}
          onClose={() => setCompareRef(null)}
        />
      </React.Suspense>

      {/* 提交右键菜单（Log 表格右键） */}
      <CommitContextMenu
        menu={commitMenu}
        onClose={() => setCommitMenu(null)}
        onCompareVersions={handleCompareVersions}
        onCopyHash={handleCopyHash}
        onCheckoutRevision={c => void handleCheckoutRevision(c)}
        onCherryPick={c => void handleCherryPick(c)}
        onNewTag={c => void handleNewTag(c)}
        onNewBranch={c => void handleCreateBranchFrom(c.hash)}
        onReset={c => setResetTarget(c)}
        onUndo={c => void handleUndoCommit(c)}
        onRevert={c => void handleRevertCommit(c.hash)}
        onEditMessage={c => void handleEditCommitMessage(c)}
        onDrop={c => void handleDropCommit(c)}
        onCopyPatch={c => void handleCopyCommitPatch(c.hash)}
        onCreatePatch={c => void handleCreateCommitPatch(c.hash)}
        onInteractiveRebase={c => setRebaseBase(c)}
        onGoToParent={c => goToCommit(c.parents?.[0] ?? '')}
        onGoToChild={c => {
          const child = commits.find(x => (x.parents ?? []).includes(c.hash));
          if (child) goToCommit(child.hash);
        }}
        canGoParent={canGoParent(commitMenu?.commit ?? null)}
        canGoChild={canGoChild(commitMenu?.commit ?? null)}
        onCherryPickMany={list => void handleCherryPickMany(list)}
        onRevertMany={list => void handleRevertMany(list)}
        onDropMany={list => void handleDropMany(list)}
        onCreatePatchMany={list => void handleCreatePatchMany(list)}
        onSquashCommits={list => setSquashCommitsTarget(list)}
      />

      {/* 压缩提交（Squash Commits）对话框 */}
      <SquashDialog
        commits={squashCommitsTarget}
        onConfirm={msg => void handleConfirmSquash(msg)}
        onClose={() => setSquashCommitsTarget(null)}
        busy={gitBusy}
      />

      {/* Reset Current Branch to Here 对话框 */}
      <React.Suspense fallback={null}>
        <ResetDialog
          open={!!resetTarget}
          commitHash={resetTarget?.hash ?? ''}
          commitMessage={resetTarget?.message ?? ''}
          currentBranch={currentBranch}
          busy={gitBusy}
          onClose={() => setResetTarget(null)}
          onConfirm={mode => void handleResetConfirm(mode)}
        />
      </React.Suspense>

      {/* 项目树右键菜单（文件 / 文件夹，对齐 PyCharm Project 视图） */}
      <ProjectTreeContextMenu
        menu={treeMenu}
        hasClipboard={!!fsClipboard}
        onClose={() => setTreeMenu(null)}
        onNewFile={handleTreeNewFile}
        onNewDirectory={handleTreeNewDirectory}
        onCut={handleTreeCut}
        onCopy={handleTreeCopy}
        onPaste={p => void handleTreePaste(p)}
        onCopyPath={(p, absolute) => void handleTreeCopyPath(p, absolute)}
        onRename={handleTreeRename}
        onDelete={handleTreeDelete}
        onReveal={p => void handleTreeReveal(p)}
        onOpenTerminal={p => void handleTreeOpenTerminal(p)}
        onAddToVcs={p => void handleAddToVcs([p])}
        onRollback={handleTreeRollback}
        onExpandAll={p => void handleTreeExpandAll(p)}
        onCollapseAll={handleTreeCollapseAll}
        onShowDiff={handleSelectFileForDiff}
        onShowHistory={p => setHistoryFile(p)}
        onAnnotate={p => {
          void openFile(p);
          setBlameOn(true);
        }}
        onCompareWith={p => void handleTreeCompareWith(p)}
        onReload={(p, isDir) => void handleTreeReload(p, isDir)}
        onFindInPath={p => {
          setFindInFilesScope(p);
          setFindInFilesOpen(true);
        }}
        repoRoot={isRepo ? (gitRepoRoot || workspacePath) : undefined}
      />

      {/* 项目树 新建 / 重命名 / 删除 对话框 */}
      <React.Suspense fallback={null}>
        <NamePromptDialog
          state={nameDialog}
          onConfirm={v => void handleNameDialogConfirm(v)}
          onClose={() => setNameDialog(null)}
        />
      </React.Suspense>

      {/* 通用文本输入框（window.prompt 替代品：wry 未实现 JS 面板） */}
      <TextPromptDialog request={textPromptReq} onConfirm={v => resolveTextPrompt(v)} onClose={() => resolveTextPrompt(null)} />

      {/* 通用确认弹窗（替代原生 NSAlert） */}
      <ConfirmDialog />

      {/* 文件历史对话框（Show History for File） */}
      <React.Suspense fallback={null}>
        <FileHistoryDialog
          open={!!historyFile && !!workspacePath}
          repoPath={gitRepoRoot || workspacePath}
          filePath={historyFile ?? ''}
          onClose={() => setHistoryFile(null)}
        />
      </React.Suspense>

      {/* Stash Changes 对话框（贮藏更改） */}
      <React.Suspense fallback={null}>
        <StashDialog
          open={stashDialogOpen && !!workspacePath}
          repoPath={gitRepoRoot || workspacePath}
          changedFiles={changedFiles}
          busy={gitBusy}
          onClose={() => setStashDialogOpen(false)}
          onStashed={message => void handleStashed(message)}
        />
      </React.Suspense>

      {/* Unstash Changes 对话框（恢复贮藏） */}
      <React.Suspense fallback={null}>
        <UnstashDialog
          open={unstashDialogOpen && !!workspacePath}
          repoPath={gitRepoRoot || workspacePath}
          busy={gitBusy}
          onClose={() => setUnstashDialogOpen(false)}
          onChanged={message => void handleUnstashChanged(message)}
        />
      </React.Suspense>

      {/* 交互式变基对话框 */}
      {rebaseBase && (
        <React.Suspense fallback={null}>
          <InteractiveRebaseDialog
            open
            repoPath={gitRepoRoot || workspacePath}
            baseCommit={rebaseBase}
            commits={commits}
            busy={gitBusy}
            onClose={() => setRebaseBase(null)}
            onDone={message => void handleRebaseDone(message)}
          />
        </React.Suspense>
      )}

      {isConflictModalActive && (
        <div
          className="main-window-modal-overlay"
          onClick={async () => {
            try {
              await invokeCommand('focus_window', { label: 'win_merge_tool' });
            } catch {
              try {
                await invokeCommand('focus_window', { label: 'win_conflicts' });
              } catch {
                setIsConflictModalActive(false);
                void refreshGit();
              }
            }
          }}
        />
      )}

      {/* 冲突解决对话框（浏览器环境或未开启原生窗口时兜底） */}
      {!isTauriEnvironment() && (
        <React.Suspense fallback={null}>
          <ConflictDialog
            open={conflictDialogOpen && conflictCount > 0}
            repoPath={gitRepoRoot || workspacePath}
            files={changedFiles.filter(f => f.status === 'C')}
            monacoTheme={MONACO_THEME_BY_APP_THEME[themeIdRef.current] ?? 'ide-jetbrains-dark'}
            busy={gitBusy}
            onClose={() => setConflictDialogOpen(false)}
            onAllResolved={message => void handleAllConflictsResolved(message)}
            onFileResolved={(path, message) => void handleConflictFileResolved(path, message)}
            currentBranch={currentBranch}
            sequencer={sequencer}
          />
        </React.Suspense>
      )}
      {/* Shelf 贮藏架对话框 */}
      <React.Suspense fallback={null}>
        <ShelveDialog
          open={shelveDialogOpen && !!workspacePath}
          repoPath={gitRepoRoot || workspacePath}
          busy={gitBusy}
          onClose={() => setShelveDialogOpen(false)}
          onChanged={message => void handleUnshelveDone(message)}
        />
      </React.Suspense>

      {/* 设置对话框 */}
      <React.Suspense fallback={null}>
        <SettingsDialog
          open={settingsOpen}
          settings={settings}
          about={engineInfo}
          workspacePath={workspacePath}
          onCheckForUpdates={() => handleCheckForUpdates(false)}
          projectOverrides={getProjectLayer()}
          onClearProjectOverrides={() => {
            clearProjectLayer();
            setSettings(loadSettings());
          }}
          onClose={() => setSettingsOpen(false)}
          onChange={(next, changedKeys) => {
            setSettings(next);
            persistSettings(next, changedKeys);
            document.body.setAttribute('data-variant', String(next.uiVariant ?? DEFAULT_SETTINGS.uiVariant));
            document.body.classList.toggle('platform-win', next.captionStyle === 'windows');
            document.body.classList.toggle('platform-mac', next.captionStyle === 'macos');
            // 换了 Monaco 主题：内核若已加载则补注册新主题，否则本次请求的 themeId
            // 会成为初始化时的首个主题，避免后续 Editor setTheme 到未注册的主题名
            const nextThemeId = next.themeId === 0 ? next.uiVariant : next.themeId;
            if (monacoReady) requestMonaco(nextThemeId);
          }}
        />
      </React.Suspense>

      {/* 应用更新对话框 */}
      <React.Suspense fallback={null}>
        <UpdateDialog
          open={updateDialogOpen}
          initialState={updateDialogState}
          initialUpdateInfo={pendingUpdateInfo}
          onClose={() => setUpdateDialogOpen(false)}
          onIgnoreVersion={v => {
            const cur = loadSettings();
            persistSettings({ ...cur, ignoredUpdateVersion: v });
            setAvailableUpdate(null);
            setBackgroundUpdateToast(null);
          }}
          hasDirtyBuffers={hasDirtyBuffers}
          confirmDiscardDirty={confirmDiscardDirty}
        />
      </React.Suspense>

      {/* 后台检测发现新版本的气泡提示 */}
      {backgroundUpdateToast && (
        <div className="py-update-balloon">
          <div className="py-update-balloon-header">
            <span style={{ fontWeight: 600 }}>{t().updater.toastTitle}</span>
            <button
              type="button"
              className="py-dialog-close"
              onClick={() => setBackgroundUpdateToast(null)}
              title={t().common.close}
            >
              ×
            </button>
          </div>
          <div style={{ fontSize: '12px', margin: '6px 0 10px', color: 'var(--text-secondary)' }}>
            {t().updater.toastDesc(backgroundUpdateToast.version)}
          </div>
          <div style={{ display: 'flex', gap: '8px', justifyContent: 'flex-end' }}>
            <button
              type="button"
              className="py-btn"
              style={{ height: '24px', minWidth: '48px', fontSize: '11px', padding: '0 8px' }}
              onClick={() => {
                const v = backgroundUpdateToast.version;
                const cur = loadSettings();
                persistSettings({ ...cur, ignoredUpdateVersion: v });
                setAvailableUpdate(null);
                setBackgroundUpdateToast(null);
              }}
            >
              {t().updater.toastActionDismiss}
            </button>
            <button
              type="button"
              className="py-btn primary"
              style={{ height: '24px', minWidth: '48px', fontSize: '11px', padding: '0 8px' }}
              onClick={() => {
                setPendingUpdateInfo(backgroundUpdateToast);
                setUpdateDialogState('available');
                setUpdateDialogOpen(true);
                setBackgroundUpdateToast(null);
              }}
            >
              {t().updater.toastActionUpdate}
            </button>
          </div>
        </div>
      )}
    </div>
  );
};
