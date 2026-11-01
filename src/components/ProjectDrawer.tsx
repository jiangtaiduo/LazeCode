import React, { useState, useMemo, useCallback, useRef, useEffect, useImperativeHandle } from 'react';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';
import type { FileNode, GitChangedFile } from '../types';
import { renderFileIcon, FolderIcon, ProjectStripeIcon, IconCollapseAll, IconChevronDown, IconChevronRight, IconLocate, IconAutoLocate } from './FileIcons';
import { invokeCommand } from '../services/tauriBridge';
import { buildTreeOffsets, computeTreeWindow } from './treeWindow';

/** 外部目录节点不给 git 状态：共享的空值常量，避免每次渲染新建空对象/空 Set 击穿 TreeNodeItem 的 memo */
const EMPTY_STATUS_BY_PATH: Record<string, string> = {};
const EMPTY_PATH_SET: Set<string> = new Set();
const EMPTY_ROOTS: FileNode[] = [];
const EMPTY_CHANGED_FILES: GitChangedFile[] = [];
const CHEVRON_DOWN_11 = Object.freeze(<IconChevronDown size={11} color="var(--text-muted)" />);
const CHEVRON_RIGHT_11 = Object.freeze(<IconChevronRight size={11} color="var(--text-muted)" />);

const statusClassOf = (status?: string): string => {
  if (!status) return '';
  if (status === 'A') return 'status-added';
  if (status === 'D') return 'status-deleted';
  if (status === 'C') return 'status-conflict';
  if (status === 'U') return 'status-unversioned';
  if (status === 'I') return 'status-ignored';
  return 'status-modified';
};

interface ProjectDrawerProps {
  rootName: string;
  rootPath: string;
  /** Git 仓库根；项目为仓库子目录时与 rootPath 不同（status 路径以仓库根为基准） */
  repoRoot?: string;
  files: FileNode[];
  /**
   * 工作区根是否为单个文件。
   *
   * 不能靠 `files[0].path === rootPath` 推断：macOS 返回的路径可能一份是
   * NFC、一份是 NFD（中文/重音目录名实测会触发），字符串不等时单文件会
   * 被误画成可折叠的项目文件夹。由 App 在 read_directory 结果处显式传入。
   */
  singleFile: boolean;
  /** 追加查看的外部目录根（不属于当前项目，只读浏览，可关闭） */
  externalRoots?: FileNode[];
  activeFile: string;
  /** 抽屉宽度（可拖拽调节，由 App 持久化） */
  width?: number;
  /** 工作区 Git 变更清单：用于文件树状态着色（PyCharm 风格） */
  changedFiles?: GitChangedFile[];
  /** 展开的目录集合（提升到 App：定位 / 展开全部 / 折叠全部都需要操作它） */
  expandedPaths: Set<string>;
  onSetExpandedPaths: React.Dispatch<React.SetStateAction<Set<string>>>;
  onSelectFile: (path: string) => void;
  /** 打开新项目目录（切换/载入工作区） */
  onOpenDirectory: () => void;
  /** 追加打开其它目录到文件树（仅查看，不切换项目） */
  onOpenExternalDirectory?: () => void;
  /** 关闭某个追加的外部目录根 */
  onCloseExternalDirectory?: (path: string) => void;
  onLocateActive?: () => void;
  /** 自动定位开关状态（对齐 PyCharm Always Select Opened File，由 App 持久化） */
  autoLocate?: boolean;
  onToggleAutoLocate?: () => void;
  ignoredPatterns?: string[];
  onUpdateFileNode?: (path: string, children: FileNode[]) => void;
  /**
   * 节点右键菜单（仅项目自身节点；外部追加目录为只读查看，不提供右键菜单）
   *
   * 第四参 selected：被点行已在一个多选集内时传整个集合（长度 > 1），
   * 供菜单切到批量形态。不传 = 单文件菜单。
   */
  onNodeContext?: (node: FileNode, x: number, y: number, selected?: string[]) => void;
  /**
   * 多选集合变化上报（Cmd/Ctrl 切中、Shift 范围选择）。
   *
   * App 侧拿它驱动右键菜单的批量项文案与删除入参；不上报时 ProjectDrawer
   * 仍是完全的内部状态，单文件行为不变。
   */
  onSelectionChange?: (paths: string[]) => void;
}

/** 行高与 CSS 保持同步：.tree-node-row 22 / .tree-root-item 24 / .tree-external-root 24+margin 4 */
const TREE_NODE_H = 22;
const TREE_ROOT_H = 24;
const TREE_EXTERNAL_H = 28;
/** .project-tree-container 的 padding-top：scrollTop 坐标系从这里开始 */
const TREE_PADDING_TOP = 4;
/** 两屏余量，避免快速滚动时合成线程先看到空白 */
const TREE_OVERSCAN = 8;

export interface ProjectDrawerHandle {
  /**
   * 滚动到指定路径。返回 false 表示该路径当前不在可见树里（未展开/不是文件）。
   *
   * 只负责滚动：目标行会在随后的 React 提交里挂载；调用方要闪烁高亮时
   * 在下一帧再查 `[data-tree-path]`（App 的定位逻辑保持原样）。
   */
  revealPath(path: string, align?: 'nearest' | 'center'): boolean;
}

type VisibleRow =
  | { kind: 'project-root'; key: string; height: number }
  | { kind: 'external-root'; key: string; ext: FileNode; height: number }
  | {
      kind: 'node';
      key: string;
      node: FileNode;
      depth: number;
      /** 外部追加目录只读：不接右键、不显示 git 状态 */
      allowContext: boolean;
      height: number;
    };

/**
 * memo：整个已展开的树会在 App 每次击键/终端输出时 reconcile，隔离掉与树无关的重渲染。
 * 生效前提：App 侧回调（onSelectFile 等）与 expandedPaths/changedFiles 保持稳定引用。
 */
const ProjectDrawerImpl = React.forwardRef<ProjectDrawerHandle, ProjectDrawerProps>(function ProjectDrawer({
  rootName,
  rootPath,
  repoRoot,
  files,
  singleFile,
  externalRoots = EMPTY_ROOTS,
  activeFile,
  width,
  changedFiles = EMPTY_CHANGED_FILES,
  expandedPaths,
  onSetExpandedPaths,
  onSelectFile,
  onOpenDirectory,
  onOpenExternalDirectory,
  onCloseExternalDirectory,
  onLocateActive,
  autoLocate = false,
  onToggleAutoLocate,
  ignoredPatterns,
  onUpdateFileNode,
  onNodeContext,
  onSelectionChange
}: ProjectDrawerProps, ref) {
  useLanguageRefresh();
  const [loadingPaths, setLoadingPaths] = useState<Set<string>>(new Set());
  /**
   * 多选集合（文件树选中项）。空集合 = 单次点击语义（只有 activeFile 高亮）。
   *
   * 与 activeFile 分离：后者是「当前打开/聚焦的文件」，前者是「待批量操作的目标」。
   * 只有在真做过多选手势（Cmd/Ctrl 点、Shift 范围）后才非空。
   */
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  /** Shift 范围选择的锚点：上一次单纯点击的行。只在有 selection 时参与运算 */
  const anchorPathRef = useRef<string | null>(null);

  // 折叠全部（同时清掉多选，避免「折叠后选中的行已不可见」的幽灵选中态）
  const handleCollapseAll = () => {
    onSetExpandedPaths(new Set());
    setSelectedPaths(new Set());
    anchorPathRef.current = null;
  };

  const toggleInSet = useCallback((path: string) => {
    onSetExpandedPaths(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, [onSetExpandedPaths]);

  // 展开 / 折叠文件夹。展开时**总是现读**：children 非空只说明「曾经读过」，
  // 不代表现在 —— 目录在折叠期间发生的外部变更（watcher 未覆盖的窗口、
  // 上一次读取之后的任何写入）只有现读才能看见。深度上限 2 使单次读取
  // 有界，展开点击付一次浅层 IPC 换掉一整类「展开了看到旧内容」的 staleness。
  // useCallback：TreeNodeItem 加了 memo，内联函数会让整棵树每次渲染都失效。
  const toggleFolder = useCallback(async (node: FileNode) => {
    const isExpanded = expandedPaths.has(node.path);
    toggleInSet(node.path);

    if (!isExpanded) {
      setLoadingPaths(prev => new Set(prev).add(node.path));
      try {
        const res = await invokeCommand<FileNode>('read_directory', { path: node.path, maxDepth: 2, ignored: ignoredPatterns });
        const childList = Array.isArray(res?.children) ? res.children : [];
        if (onUpdateFileNode) {
          onUpdateFileNode(node.path, childList);
        } else {
          node.children = childList;
        }
      } catch (err) {
        console.warn('Failed to load child directory:', err);
      } finally {
        setLoadingPaths(prev => {
          const next = new Set(prev);
          next.delete(node.path);
          return next;
        });
      }
    }
  }, [expandedPaths, toggleInSet, onUpdateFileNode, ignoredPatterns]);

  // Git 状态：把仓库相对路径映射成绝对路径，供树节点着色
  const gitStatusByPath = useMemo(() => {
    const map: Record<string, string> = {};
    const base = repoRoot || rootPath;
    if (!base) return map;
    const prefix = base.endsWith('/') ? base : `${base}/`;
    for (const file of changedFiles) {
      map[file.path.startsWith('/') ? file.path : `${prefix}${file.path}`] = file.status;
    }
    return map;
  }, [changedFiles, repoRoot, rootPath]);

   const directoryHasChanges = useMemo(() => {
     const prefixes = new Set<string>();
    for (const [filePath, status] of Object.entries(gitStatusByPath)) {
      // 被忽略的文件（'I'）不属于未提交变更，不给父目录冒泡变更蓝点
      if (status === 'I') continue;
       const segments = filePath.split('/');
       for (let i = 2; i < segments.length; i++) {
         prefixes.add(segments.slice(0, i).join('/'));
       }
     }
     return prefixes;
   }, [gitStatusByPath]);

  // 过滤树（filterText 状态已删除：它从未有写入入口，恒为 ''。
  // 保留 filterNode 纯函数供未来过滤入口复用，此处直接透传原列表）。
  const visibleFiles = files;

  const visibleExternalRoots = externalRoots;

  // 格式化展示路径（如果是绝对路径，将用户的 home 目录简写为 ~）
  const displayPath = useMemo(() => {
    if (!rootPath) return '';
    return rootPath.replace(/^\/Users\/[^/]+/, '~').replace(/^C:\\Users\\[^\\]+/, '~');
  }, [rootPath]);

  const abbreviatePath = (p: string) =>
    p.replace(/^\/Users\/[^/]+/, '~').replace(/^C:\\Users\\[^\\]+/, '~');

  const projectRootExpanded = !!rootPath && expandedPaths.has(rootPath);

  /**
   * 可见行扁平化：窗口化的唯一数据源。
   *
   * 与原先递归渲染的视觉顺序严格一致：项目根 → 已展开子节点 → 外部根
   * （同样只展开时收子节点）。行高来自 CSS 常量，窗口边界用前缀和二分，
   * 不再依赖每一行都挂载。
   */
  const rows = useMemo<VisibleRow[]>(() => {
    const out: VisibleRow[] = [];
    const walk = (nodes: FileNode[], depth: number, allowContext: boolean) => {
      for (const node of nodes) {
        out.push({
          kind: 'node',
          // allowContext 参与 key：同一个路径理论上不会同时出现在项目和外部根，
          // 但项目根自身也有一行（kind 不同），用前缀彻底避免 key 冲突。
          key: `${allowContext ? 'p' : 'e'}:${node.path}`,
          node,
          depth,
          allowContext,
          height: TREE_NODE_H
        });
        if (node.isDirectory && expandedPaths.has(node.path) && Array.isArray(node.children)) {
          walk(node.children, depth + 1, allowContext);
        }
      }
    };

    if (singleFile) {
      walk(visibleFiles, 0, true);
    } else if (rootPath) {
      out.push({ kind: 'project-root', key: `root:${rootPath}`, height: TREE_ROOT_H });
      if (projectRootExpanded) walk(visibleFiles, 1, true);
    }

    for (const ext of visibleExternalRoots) {
      out.push({ kind: 'external-root', key: `ext:${ext.path}`, ext, height: TREE_EXTERNAL_H });
      if (expandedPaths.has(ext.path)) walk(ext.children ?? [], 1, false);
    }
    return out;
  }, [singleFile, rootPath, projectRootExpanded, visibleFiles, visibleExternalRoots, expandedPaths]);

  const offsets = useMemo(() => buildTreeOffsets(rows.map(row => row.height)), [rows]);
  const totalHeight = offsets[rows.length] ?? 0;

  /** 路径 → 扁平行下标：定位不需要先挂载目标行，直接按前缀和滚动。 */
  const pathIndex = useMemo(() => {
    const map = new Map<string, number>();
    rows.forEach((row, index) => {
      if (row.kind === 'node') map.set(row.node.path, index);
    });
    return map;
  }, [rows]);

  /**
   * 当前可见文件的扁平有序列表（只含文件，不含目录行）。
   *
   * Shift 范围选择需要「屏幕上从上到下」的顺序，而树是递归结构。这里按
   * visibleFiles → 已展开目录的子节点 的顺序摊平，与视觉顺序一致。
   * 外部根不参与（与原先 renderRootChildren(..., false) 一致）。
   */
  const flatFileRows = useMemo(() => {
    const out: string[] = [];
    for (const row of rows) {
      if (row.kind === 'node' && row.allowContext && !row.node.isDirectory) out.push(row.node.path);
    }
    return out;
  }, [rows]);

  const treeScrollRef = useRef<HTMLDivElement>(null);
  const [treeScrollTop, setTreeScrollTop] = useState(0);
  const [treeViewportH, setTreeViewportH] = useState(0);

  useEffect(() => {
    const el = treeScrollRef.current;
    if (!el) return;
    const update = () => setTreeViewportH(el.clientHeight);
    update();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // 折叠根/批量折叠后内容变短：原生 DOM 会自动把 scrollTop 夹回合法区间，
  // 窗口化后 totalHeight 变了但 state 可能还停在旧位置 —— 自己补一次夹取，
  // 否则窗口会算出「最后一行」而屏幕看起来是空的。
  useEffect(() => {
    const el = treeScrollRef.current;
    if (!el) return;
    const max = Math.max(0, totalHeight - el.clientHeight + TREE_PADDING_TOP * 2);
    if (el.scrollTop > max) {
      el.scrollTop = max;
      setTreeScrollTop(max);
    }
  }, [totalHeight]);

  const treeWindow = useMemo(
    () =>
      computeTreeWindow(
        offsets,
        totalHeight,
        Math.max(0, treeScrollTop - TREE_PADDING_TOP),
        treeViewportH || 400,
        TREE_OVERSCAN
      ),
    [offsets, totalHeight, treeScrollTop, treeViewportH]
  );

  useImperativeHandle(
    ref,
    () => ({
      revealPath(path, align = 'nearest') {
        const index = pathIndex.get(path);
        const el = treeScrollRef.current;
        if (index === undefined || !el) return false;
        const rowTop = TREE_PADDING_TOP + offsets[index];
        const rowHeight = rows[index]?.height ?? TREE_NODE_H;
        const viewport = el.clientHeight || 400;
        const current = el.scrollTop;
        let next = current;
        if (align === 'center') {
          next = Math.max(0, rowTop - Math.max(0, (viewport - rowHeight) / 2));
        } else if (rowTop < current) {
          next = rowTop;
        } else if (rowTop + rowHeight > current + viewport) {
          next = rowTop + rowHeight - viewport;
        }
        // jsdom 的 scrollHeight 为 0；只有真实内容高于视口时才需要夹取上界。
        if (el.scrollHeight > viewport) next = Math.min(next, el.scrollHeight - viewport);
        if (Math.abs(next - current) > 0.5) {
          el.scrollTop = next;
          setTreeScrollTop(next);
        }
        return true;
      }
    }),
    [pathIndex, offsets, rows]
  );

  /**
   * 行点击的多选手势分发。
   *
   *  - 无修饰键：单选（activeFile 走原路径，清空多选），并更新 Shift 锚点
   *  - Cmd/Ctrl + 点击：切中该项，锚点不动（锚点始终是「上次单纯点击的行」）
   *  - Shift + 点击：取锚点到当前行之间的**全部可见行**（含两端）
   *
   * 目录仍然只做展开折叠：目录不参与批量操作（删除目录本就递归删除其内容，
   * 把子文件也列进多选只会重复命中同一批文件）。
   */
  const handleRowPointer = useCallback(
    (node: FileNode, e: React.MouseEvent) => {
      if (node.isDirectory) {
        toggleFolder(node);
        return;
      }
      const isToggle = e.metaKey || e.ctrlKey;
      const isRange = e.shiftKey;
      if (isToggle) {
        setSelectedPaths(prev => {
          const next = new Set(prev);
          if (next.has(node.path)) next.delete(node.path);
          else next.add(node.path);
          // 空集合时把被取消的那一项留作锚点之外的明确状态：直接返回空，
          // 下一次单纯点击会重置锚点，不会出现「范围选到上一次的残留」。
          return next;
        });
        return;
      }
      if (isRange && anchorPathRef.current) {
        const from = flatFileRows.indexOf(anchorPathRef.current);
        const to = flatFileRows.indexOf(node.path);
        if (from !== -1 && to !== -1) {
          const [lo, hi] = from <= to ? [from, to] : [to, from];
          setSelectedPaths(prev => {
            const next = new Set(prev);
            for (let i = lo; i <= hi; i++) next.add(flatFileRows[i]);
            return next;
          });
          return;
        }
      }
      // 单纯点击：单选语义。PyCharm 单击只选中，双击才打开文件；
      // 单击打开会让「先选中再操作」的路径被打断（右键/批量操作前必开文件）。
      anchorPathRef.current = node.path;
      setSelectedPaths(new Set([node.path]));
    },
    [toggleFolder, flatFileRows]
  );

  /** 空白处单击清空多选（PyCharm / VS Code 一致行为） */
  const handleTreeBackgroundClick = useCallback(() => {
    setSelectedPaths(new Set());
    anchorPathRef.current = null;
  }, []);

  // 多选集合变化上报给 App（右键菜单文案 / 批量删除入参）。
  // 用 effect 而不是在 setter 里调用：setter 里调回调违反 React 的「渲染期
  // 无副作用」，StrictMode 双调用时会重复触发。
  const selectionKey = useMemo(() => Array.from(selectedPaths).sort().join('\n'), [selectedPaths]);
  const onSelectionChangeRef = useRef(onSelectionChange);
  onSelectionChangeRef.current = onSelectionChange;
  useEffect(() => {
    const cb = onSelectionChangeRef.current;
    if (!cb) return;
    cb(selectedPaths.size > 0 ? Array.from(selectedPaths) : []);
    // selectionKey 是选定路径的有序签名，等价于 selectedPaths 的身份但不随
    // Set 的新引用变化 —— 避免每次 toggleInSet 都新建 Set 时死循环。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectionKey]);

  /** 渲染单行：项目根 / 外部根 / 普通节点。递归已由 rows 摊平，组件只画一行。 */
  const renderRow = (row: VisibleRow) => {
    if (row.kind === 'project-root') {
      return (
        <div
          className="tree-root-item"
          data-tree-path={rootPath}
          onClick={() => toggleInSet(rootPath)}
          onContextMenu={e => {
            if (!onNodeContext || !rootPath) return;
            e.preventDefault();
            onNodeContext(
              { name: rootName || 'Project', path: rootPath, isDirectory: true, children: files },
              e.clientX,
              e.clientY
            );
          }}
          title={rootPath}
        >
          <span className={`tree-chevron ${projectRootExpanded ? 'expanded' : 'collapsed'}`}>
            {projectRootExpanded ? CHEVRON_DOWN_11 : CHEVRON_RIGHT_11}
          </span>
          <FolderIcon size={15} isOpen={projectRootExpanded} />
          <span className="tree-root-name">{rootName || 'Project'}</span>
          <span className="tree-root-path">{displayPath}</span>
        </div>
      );
    }

    if (row.kind === 'external-root') {
      const ext = row.ext;
      const extExpanded = expandedPaths.has(ext.path);
      return (
        <div
          className="tree-root-item tree-external-root"
          data-tree-path={ext.path}
          onClick={() => toggleInSet(ext.path)}
          title={t().drawer.externalDirTooltip(ext.path)}
        >
          <span className={`tree-chevron ${extExpanded ? 'expanded' : 'collapsed'}`}>
            {extExpanded ? CHEVRON_DOWN_11 : CHEVRON_RIGHT_11}
          </span>
          <FolderIcon size={15} isOpen={extExpanded} />
          <span className="tree-root-name">{ext.name}</span>
          <span className="external-root-badge">{t().drawer.externalBadge}</span>
          <span className="tree-root-path">{abbreviatePath(ext.path)}</span>
          <button
            className="external-root-close"
            title={t().drawer.removeExternalDir}
            onClick={e => {
              e.stopPropagation();
              onCloseExternalDirectory?.(ext.path);
            }}
          >
            ×
          </button>
        </div>
      );
    }

    return (
      <TreeNodeItem
        node={row.node}
        depth={row.depth}
        activeFile={activeFile}
        selectedPaths={selectedPaths}
        expandedPaths={expandedPaths}
        loadingPaths={loadingPaths}
        gitStatusByPath={row.allowContext ? gitStatusByPath : EMPTY_STATUS_BY_PATH}
        directoryHasChanges={row.allowContext ? directoryHasChanges : EMPTY_PATH_SET}
        onToggleFolder={toggleFolder}
        onSelectFile={onSelectFile}
        onRowPointer={handleRowPointer}
        onNodeContext={row.allowContext ? onNodeContext : undefined}
      />
    );
  };

  return (
    <aside className="project-drawer" id="projectDrawer" style={width ? { width: `${width}px`, flexShrink: '0' } : undefined}>
      {/* 1. Tool Window Header */}
      <div className="tool-window-header">
        <div className="tool-window-title">
          <span>{t().drawer.projectTitle}</span>
        </div>
        <div className="tool-window-actions">
          <button
            className="tb-icon-btn"
            onClick={onLocateActive}
            disabled={!activeFile || !onLocateActive}
            title={t().drawer.locateInTree}
          >
            <IconLocate size={14} />
          </button>
          <button
            className={`tb-icon-btn ${autoLocate ? 'active' : ''}`}
            onClick={onToggleAutoLocate}
            disabled={!onToggleAutoLocate}
            title={t().drawer.autoLocate}
          >
            <IconAutoLocate size={14} />
          </button>
          <button
            className="tb-icon-btn"
            onClick={handleCollapseAll}
            disabled={singleFile}
            title={t().drawer.collapseAll}
          >
            <IconCollapseAll size={14} />
          </button>
          <button
            className="tb-icon-btn"
            onClick={onOpenExternalDirectory}
            disabled={!rootPath || !onOpenExternalDirectory}
            title={t().drawer.openExternalDir}
          >
            <ProjectStripeIcon size={15} color="var(--text-muted)" />
          </button>
        </div>
      </div>

      {/* 2. File Tree Content */}
      <div
        className="project-tree-container"
        ref={treeScrollRef}
        onScroll={e => setTreeScrollTop(e.currentTarget.scrollTop)}
      >
        {!rootPath ? (
          <div className="empty-project-prompt">
            <div style={{ color: 'var(--text-secondary)', marginBottom: '8px' }}>{t().drawer.noProjectOpen}</div>
            <button className="open-dir-btn" onClick={onOpenDirectory}>
              {t().drawer.openFolder}
            </button>
          </div>
        ) : (
          <div className="tree-list" onClick={handleTreeBackgroundClick}>
            {treeWindow.topPad > 0 && <div aria-hidden="true" style={{ height: treeWindow.topPad }} />}
            {rows.slice(treeWindow.start, treeWindow.end).map(row => (
              <div
                key={row.key}
                className="tree-row-slot"
                style={{ height: row.height, display: 'flow-root' }}
              >
                {renderRow(row)}
              </div>
            ))}
            {treeWindow.bottomPad > 0 && <div aria-hidden="true" style={{ height: treeWindow.bottomPad }} />}
          </div>
        )}
      </div>
    </aside>
  );
});

export const ProjectDrawer = React.memo(ProjectDrawerImpl);

interface TreeNodeItemProps {
  node: FileNode;
  depth: number;
  activeFile: string;
  /** 多选集合；空集合时只有 activeFile 高亮 */
  selectedPaths: Set<string>;
  expandedPaths: Set<string>;
  loadingPaths: Set<string>;
  gitStatusByPath: Record<string, string>;
  directoryHasChanges: Set<string>;
  onToggleFolder: (node: FileNode) => void;
  onSelectFile: (path: string) => void;
  /** 多选手势分发（Cmd 切中 / Shift 范围）/ 目录展开折叠 */
  onRowPointer?: (node: FileNode, e: React.MouseEvent) => void;
  onNodeContext?: (node: FileNode, x: number, y: number, selected?: string[]) => void;
}

// memo：窗口每次滚动都会重渲染可见行；props 都是原始值或上层稳定引用
// （Set/记录由 ProjectDrawer 的 useMemo 提供），未展开、未选中且 git 状态未变的节点可直接跳过。
// 本组件只画一行：展开后的子节点由父组件的扁平 rows 负责，不再递归挂载。
const TreeNodeItem: React.FC<TreeNodeItemProps> = React.memo(function TreeNodeItem({
  node,
  depth,
  activeFile,
  selectedPaths,
  expandedPaths,
  loadingPaths,
  gitStatusByPath,
  directoryHasChanges,
  onToggleFolder,
  onSelectFile,
  onRowPointer,
  onNodeContext
}) {
  const isExpanded = expandedPaths.has(node.path);
  const isLoading = loadingPaths.has(node.path);
  const isSelected = activeFile === node.path;
  // 多选了就该盖过「打开中」的高亮：用户当下的操作目标是被选中的集合
  const isMultiSelected = selectedPaths.has(node.path);
  const gitStatus = node.isDirectory ? undefined : gitStatusByPath[node.path];
  const showDirectoryMark = node.isDirectory && directoryHasChanges.has(node.path);

  const handleClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (onRowPointer) {
      onRowPointer(node, e);
      return;
    }
    // 无多选回调时退回旧行为（外部目录子树未接线）
    if (node.isDirectory) {
      onToggleFolder(node);
    } else {
      onSelectFile(node.path);
    }
  };

  const handleContext = (e: React.MouseEvent) => {
    if (!onNodeContext) return;
    e.preventDefault();
    e.stopPropagation();
    // 已经多选了、且点的行就在多选集里 → 把整个集合带上（批量操作）；
    // 否则右键只作用于被点的这一行（标准文件管理器语义）。
    if (selectedPaths.has(node.path) && selectedPaths.size > 1) {
      onNodeContext(node, e.clientX, e.clientY, Array.from(selectedPaths));
    } else {
      onNodeContext(node, e.clientX, e.clientY);
    }
  };

  return (
    <div
      className={`tree-node-row ${isMultiSelected ? 'selected' : ''} ${isSelected ? 'opened' : ''} ${node.isDirectory ? 'is-dir' : 'is-file'}`}
      style={{ paddingLeft: `${depth * 14 + 6}px` }}
      data-tree-path={node.path}
      onClick={handleClick}
      onDoubleClick={() => {
        if (!node.isDirectory) onSelectFile(node.path);
      }}
      onContextMenu={handleContext}
      title={node.isDirectory ? node.path : `${node.path}${onNodeContext ? `\n${t().drawer.nodeContextHint}` : ''}`}
    >
      {node.isDirectory ? (
        <span className={`tree-chevron ${isExpanded ? 'expanded' : 'collapsed'}`}>
          {isLoading ? '⋯' : isExpanded ? CHEVRON_DOWN_11 : CHEVRON_RIGHT_11}
        </span>
      ) : (
        <span className="tree-chevron-placeholder" />
      )}

      <span className="tree-icon-box">
        {renderFileIcon(node.name, node.isDirectory, isExpanded)}
      </span>

      <span className={`tree-label ${isSelected ? 'selected-label' : ''} ${statusClassOf(gitStatus)}`}>{node.name}</span>
      {gitStatus && gitStatus !== 'I' && (
        <span className={`tree-git-badge git-status-${gitStatus.toLowerCase()}`} title={`Git: ${gitStatus}`}>
          {gitStatus}
        </span>
      )}
      {showDirectoryMark && <span className="tree-git-dot" title={t().drawer.containsUncommitted} />}
    </div>
  );
});
