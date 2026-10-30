import React, { useState, useEffect, useMemo, useCallback, useRef } from 'react';
import { invokeCommand } from '../services/tauriBridge';
import type { GitCommit, GitCommitFile } from '../types';
import {
  IconCompareVersions,
  IconViewOptions,
  IconExpandAll,
  IconCollapseAll,
  IconChevronDown,
  IconChevronRight,
  IconMoreVert,
  IconWindowMinimize,
  IconWindowClose,
  FolderIcon,
  TextFileIcon
} from './FileIcons';

interface CompareDirNode {
  name: string;
  path: string;
  dirs: CompareDirNode[];
  files: GitCommitFile[];
}

/** 把仓库相对路径摊成目录树；Group By Directory 的真实实现。 */
function buildCompareTree(files: GitCommitFile[]): CompareDirNode {
  const root: CompareDirNode = { name: '', path: '', dirs: [], files: [] };
  for (const file of files) {
    const parts = file.path.replace(/\\/g, '/').split('/').filter(Boolean);
    let cur = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const name = parts[i];
      const path = parts.slice(0, i + 1).join('/');
      let next = cur.dirs.find(d => d.name === name);
      if (!next) {
        next = { name, path, dirs: [], files: [] };
        cur.dirs.push(next);
      }
      cur = next;
    }
    cur.files.push(file);
  }
  const sortNode = (n: CompareDirNode) => {
    n.dirs.sort((a, b) => a.name.localeCompare(b.name));
    n.files.sort((a, b) => a.path.localeCompare(b.path));
    n.dirs.forEach(sortNode);
  };
  sortNode(root);
  return root;
}

interface CompareDrawerProps {
  repoPath: string;
  fromCommit: GitCommit;
  toCommit: GitCommit;
  onClose: () => void;
  onMinimize: () => void;
  onSwapCommits: () => void;
  onSelectFileDiff: (path: string) => void;
  activeFilePath?: string | null;
  width?: number;
}

export const CompareDrawer: React.FC<CompareDrawerProps> = ({
  repoPath,
  fromCommit,
  toCommit,
  onClose,
  onMinimize,
  onSwapCommits,
  onSelectFileDiff,
  activeFilePath,
  width
}) => {
  const [files, setFiles] = useState<GitCommitFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rootExpanded, setRootExpanded] = useState(true);
  const [collapsedDirs, setCollapsedDirs] = useState<Set<string>>(new Set());
  const [groupByDirectory, setGroupByDirectory] = useState(true);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const tree = useMemo(() => buildCompareTree(files), [files]);
  const [viewMenuOpen, setViewMenuOpen] = useState(false);
  const viewMenuRef = useRef<HTMLDivElement>(null);

  const rootName = useMemo(() => {
    if (!repoPath) return 'Repository';
    const parts = repoPath.replace(/\\/g, '/').split('/').filter(Boolean);
    return parts[parts.length - 1] || 'Repository';
  }, [repoPath]);

  const fromShort = useMemo(() => {
    return fromCommit.shortHash || fromCommit.hash.slice(0, 8);
  }, [fromCommit]);

  const toShort = useMemo(() => {
    return toCommit.shortHash || toCommit.hash.slice(0, 8);
  }, [toCommit]);

  const loadFiles = useCallback(async () => {
    if (!repoPath || !fromCommit?.hash || !toCommit?.hash) return;
    setLoading(true);
    setError(null);
    try {
      const result = await invokeCommand<GitCommitFile[]>('git_compare_commits', {
        repoPath,
        fromCommit: fromCommit.hash,
        toCommit: toCommit.hash
      });
      setFiles(result || []);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setFiles([]);
    } finally {
      setLoading(false);
    }
  }, [repoPath, fromCommit.hash, toCommit.hash]);

  useEffect(() => {
    void loadFiles();
  }, [loadFiles]);

  // 编辑器侧打开 diff 后同步选中行；单击只选中，双击才打开（PyCharm 语义）。
  useEffect(() => {
    if (activeFilePath) setSelectedFile(activeFilePath);
  }, [activeFilePath]);

  // 点击外部自动关闭 View Options 菜单
  useEffect(() => {
    if (!viewMenuOpen) return;
    const handlePointerDown = (e: PointerEvent) => {
      if (viewMenuRef.current && !viewMenuRef.current.contains(e.target as Node)) {
        setViewMenuOpen(false);
      }
    };
    document.addEventListener('pointerdown', handlePointerDown);
    return () => document.removeEventListener('pointerdown', handlePointerDown);
  }, [viewMenuOpen]);

  // ^P 快捷键切换 Group By Directory。
  // Git Log 面板内的文件清单也使用 ^P：焦点在 Log 面板时让给那边。
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === 'p' || e.key === 'P')) {
        const target = e.target as HTMLElement | null;
        if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) return;
        if ((document.activeElement as HTMLElement | null)?.closest('#pycharmBottomGit')) return;
        e.preventDefault();
        setGroupByDirectory(v => !v);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  const toggleExpand = useCallback(() => {
    setRootExpanded(prev => !prev);
  }, []);

  const getStatusColor = (status: string) => {
    const s = status.toUpperCase();
    if (s.startsWith('A') || s.startsWith('?')) return 'var(--git-green, #3fb950)';
    if (s.startsWith('D')) return 'var(--git-del, #f85149)';
    return 'var(--git-blue, #58a6ff)';
  };

  const toggleDir = (path: string) => {
    setCollapsedDirs(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const renderFileRow = (file: GitCommitFile, depth: number) => {
    const isSelected = (selectedFile ?? activeFilePath) === file.path;
    return (
      <div
        key={file.path}
        className={`compare-file-row ${isSelected ? 'selected' : ''}`}
        style={{ paddingLeft: 10 + depth * 14 }}
        onClick={() => setSelectedFile(file.path)}
        onDoubleClick={() => onSelectFileDiff(file.path)}
        title={`${file.path} (${file.status}) +${file.additions} -${file.deletions}`}
      >
        <span className="compare-file-icon">
          <TextFileIcon size={14} />
        </span>
        <span
          className="compare-file-name"
          style={{ color: isSelected ? '#ffffff' : getStatusColor(file.status) }}
        >
          {file.path.split('/').pop()}
        </span>
        <span className="compare-numstat">
          <i className="num-add">+{file.additions}</i>
          {file.deletions > 0 && <i className="num-del">-{file.deletions}</i>}
        </span>
      </div>
    );
  };

  const renderDirNodes = (node: CompareDirNode, depth: number): React.ReactNode => (
    <>
      {node.dirs.map(dir => {
        const collapsed = collapsedDirs.has(dir.path);
        return (
          <div key={dir.path}>
            <div
              className="compare-dir-row"
              style={{ paddingLeft: 10 + depth * 14 }}
              onClick={() => toggleDir(dir.path)}
              title={dir.path}
            >
              <span className={`tree-chevron ${collapsed ? 'collapsed' : 'expanded'}`}>
                {collapsed ? (
                  <IconChevronRight size={11} color="var(--text-muted)" />
                ) : (
                  <IconChevronDown size={11} color="var(--text-muted)" />
                )}
              </span>
              <FolderIcon size={14} isOpen={!collapsed} />
              <span className="compare-dir-name">{dir.name}</span>
            </div>
            {!collapsed && (
              <div>
                {renderDirNodes(dir, depth + 1)}
              </div>
            )}
          </div>
        );
      })}
      {node.files.map(f => renderFileRow(f, depth))}
    </>
  );

  return (
    <aside
      className="compare-drawer"
      id="compareDrawer"
      style={width ? { width: `${width}px`, flexShrink: 0 } : undefined}
    >
      {/* 1. Header (对齐 PyCharm 截图：Changes Between {hash1} and {hash2} ×  ⋮  —) */}
      <div className="compare-drawer-header">
        <div className="compare-tab-pill">
          <span className="compare-tab-title" title={`Changes Between ${fromShort} and ${toShort}`}>
            Changes Between {fromShort} and {toShort}
          </span>
          <button
            type="button"
            className="compare-tab-close"
            onClick={onClose}
            title="Close Comparison"
          >
            <IconWindowClose size={11} />
          </button>
        </div>
        <div className="compare-header-actions">
          <button type="button" className="tb-icon-btn" title="More Options">
            <IconMoreVert size={14} />
          </button>
          <button type="button" className="tb-icon-btn" onClick={onMinimize} title="Minimize">
            <IconWindowMinimize size={13} />
          </button>
        </div>
      </div>

      {/* 2. Toolbar (对齐 PyCharm 截图：左侧 ⇆ 👁 ，右侧 ↕ 展开与折叠两个按钮) */}
      <div className="compare-drawer-toolbar">
        <div className="compare-tb-left">
          <button
            type="button"
            className="tb-icon-btn"
            onClick={onSwapCommits}
            title="Swap Revisions (交换对比版本)"
          >
            <IconCompareVersions size={14} />
          </button>
          <div className="commit-view-menu-wrap" ref={viewMenuRef}>
            <button
              type="button"
              className={`tb-icon-btn ${viewMenuOpen ? 'active' : ''}`}
              onClick={() => setViewMenuOpen(v => !v)}
              title="View Options"
            >
              <IconViewOptions size={14} />
            </button>
            {viewMenuOpen && (
              <div className="commit-view-menu">
                <div className="commit-view-menu-header">Group By</div>
                <div
                  className={`commit-view-menu-item ${groupByDirectory ? 'active' : ''}`}
                  onClick={() => {
                    setGroupByDirectory(prev => !prev);
                    setViewMenuOpen(false);
                  }}
                >
                  <span className="commit-view-item-label">
                    <span className="commit-view-check">{groupByDirectory ? '✓' : ''}</span>
                    <span>Directory</span>
                  </span>
                  <span className="commit-view-shortcut">^P</span>
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="compare-tb-right">
          <button
            type="button"
            className="tb-icon-btn"
            onClick={() => {
              setCollapsedDirs(new Set());
              setRootExpanded(true);
            }}
            disabled={!groupByDirectory}
            title="Expand All"
          >
            <IconExpandAll size={14} />
          </button>
          <button
            type="button"
            className="tb-icon-btn"
            onClick={() => setRootExpanded(false)}
            disabled={!groupByDirectory}
            title="Collapse All"
          >
            <IconCollapseAll size={14} />
          </button>
        </div>
      </div>

      {/* 3. Tree Content */}
      <div className="compare-tree-container">
        {loading && (
          <div className="compare-tree-hint">Loading differences...</div>
        )}
        {error && (
          <div className="compare-tree-error">{error}</div>
        )}
        {!loading && !error && files.length === 0 && (
          <div className="compare-tree-hint">No differences between versions</div>
        )}
        {!loading && files.length > 0 && (
          <div className="compare-tree-list">
            {groupByDirectory ? (
              <>
                {/* Root Folder Row */}
                <div
                  className="compare-tree-root"
                  onClick={toggleExpand}
                  title={repoPath}
                >
                  <span className="tree-chevron">
                    {rootExpanded ? (
                      <IconChevronDown size={11} color="var(--text-muted)" />
                    ) : (
                      <IconChevronRight size={11} color="var(--text-muted)" />
                    )}
                  </span>
                  <FolderIcon size={15} isOpen={rootExpanded} />
                  <span className="compare-root-name">{rootName}</span>
                  <span className="compare-root-count">
                    {files.length} {files.length === 1 ? 'file' : 'files'}
                  </span>
                </div>

                {/* Directory tree under root */}
                {rootExpanded && (
                  <div className="compare-tree-children">
                    {renderDirNodes(tree, 0)}
                  </div>
                )}
              </>
            ) : (
              /* Flat List without directory grouping */
              <div className="compare-tree-children" style={{ paddingLeft: 0 }}>
                {files.map(file => {
                  const isSelected = (selectedFile ?? activeFilePath) === file.path;
                  const statusColor = getStatusColor(file.status);
                  return (
                    <div
                      key={file.path}
                      className={`compare-file-row flat-mode ${isSelected ? 'selected' : ''}`}
                      onClick={() => setSelectedFile(file.path)}
                      onDoubleClick={() => onSelectFileDiff(file.path)}
                      title={`${file.path} (${file.status}) +${file.additions} -${file.deletions}`}
                    >
                      <span className="compare-file-icon">
                        <TextFileIcon size={14} />
                      </span>
                      <span
                        className="compare-file-name"
                        style={{ color: isSelected ? '#ffffff' : statusColor }}
                      >
                        {file.path}
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </div>
    </aside>
  );
};
