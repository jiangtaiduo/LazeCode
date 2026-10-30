import React, { useState, useRef, useEffect } from 'react';
import { IconWindowClose, IconDiff, IconCompareVersions, IconChevronDown } from './FileIcons';
import { renderFileIcon } from './FileIcons';
import { CtxMenuShell } from './CtxMenuShell';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';

interface EditorTabBarProps {
  openFiles: string[];
  activeFile: string;
  activeTab: 'editor' | 'diff' | 'compare-branches';
  /** Diff 标签是否存在（打开过且未关闭）；与 activeTab 分离，切去看别的文件不该让标签消失 */
  diffOpen?: boolean;
  diffCommit?: string | null;
  compareCommits?: { from: string; to: string } | null;
  selectedCommits?: string[] | null;
  /** Compare Branches 标签（引用名）；null = 未打开 */
  compareBranches?: { left: string; right: string } | null;
  /** 有未保存改动的文件路径 */
  dirtyFiles?: string[];
  onSelectTab: (path: string) => void;
  onSelectDiffTab: () => void;
  onSelectCompareBranches: () => void;
  onCloseCompareBranches: () => void;
  onCloseTab: (path: string, e?: React.MouseEvent) => void;
  onCloseOtherTabs: (path: string) => void;
  onCloseLeftTabs: (path: string) => void;
  onCloseRightTabs: (path: string) => void;
  onCloseAllTabs: () => void;
  onCloseDiffTab: () => void;
}

/**
 * memo：标签栏只依赖打开列表/激活文件/脏集合，与 App 里每次击键变化的缓冲内容无关。
 * 生效前提：App 侧回调与 openFiles/dirtyFiles 数组保持稳定引用。
 */
export const EditorTabBar = React.memo(function EditorTabBar({
  openFiles,
  activeFile,
  activeTab,
  diffOpen = false,
  diffCommit,
  compareCommits,
  selectedCommits,
  compareBranches,
  dirtyFiles = [],
  onSelectTab,
  onSelectDiffTab,
  onSelectCompareBranches,
  onCloseCompareBranches,
  onCloseTab,
  onCloseOtherTabs,
  onCloseLeftTabs,
  onCloseRightTabs,
  onCloseAllTabs,
  onCloseDiffTab
}: EditorTabBarProps) {
  useLanguageRefresh();
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    path: string;
  } | null>(null);
  const [moreMenuOpen, setMoreMenuOpen] = useState<{ x: number; y: number } | null>(null);
  const tabListRef = useRef<HTMLDivElement>(null);

  // 激活标签（新打开或点击切换）发生变化时，自动平滑横向滚动至视口内（对齐 PyCharm）
  useEffect(() => {
    if (!tabListRef.current) return;
    const activeEl = tabListRef.current.querySelector<HTMLElement>('.ide-tab.active');
    if (activeEl) {
      activeEl.scrollIntoView({ inline: 'nearest', behavior: 'smooth', block: 'nearest' });
    }
  }, [activeFile, activeTab, openFiles]);
  // 定位交给 CtxMenuShell：它在挂载后测量真实尺寸并钳制进视口，
  // 这里只提供鼠标原点作为锚点。不要再做一套 clamp —— 两层钳制叠加
  // 会让「预钳制后的坐标」被外壳二次钳制，得到偏离鼠标的位置。

  if (openFiles.length === 0 && !diffOpen && !compareBranches) {
    return null;
  }

  const handleContextMenu = (e: React.MouseEvent, path: string) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      path
    });
  };

  const isFirstTab = (path: string) => openFiles.indexOf(path) === 0;
  const isLastTab = (path: string) => openFiles.indexOf(path) === openFiles.length - 1;

  return (
    <div className="ide-tab-bar" data-tauri-drag-region="false">
      <div className="ide-tab-list" ref={tabListRef}>
        {openFiles.map(filePath => {
          const fileName = filePath.split(/[\\/]/).pop() ?? filePath;
          const isActive = activeTab === 'editor' && activeFile === filePath;

          return (
            <div
              key={filePath}
              className={`ide-tab ${isActive ? 'active' : ''}`}
              onClick={() => onSelectTab(filePath)}
              onContextMenu={e => handleContextMenu(e, filePath)}
              title={filePath}
            >
              <span className="tab-icon">
                {renderFileIcon(fileName, false, false)}
              </span>
              <span className="tab-title">{fileName}</span>
              {dirtyFiles.includes(filePath) && (
                <span className="tab-dirty-dot" title={t().editorTab.dirtyTitle}>●</span>
              )}
              <button
                className="tab-close-btn"
                onClick={e => onCloseTab(filePath, e)}
                title={t().editorTab.closeTab}
              >
                <IconWindowClose size={12} />
              </button>
            </div>
          );
        })}

        {compareBranches && (
          <div
            className={`ide-tab is-diff-tab ${activeTab === 'compare-branches' ? 'active' : ''}`}
            onClick={onSelectCompareBranches}
            title={t().compareBranches.tabTitle(compareBranches.left, compareBranches.right)}
          >
            <span className="tab-icon" style={{ color: 'var(--accent)', display: 'inline-flex' }}>
              <IconCompareVersions size={12} />
            </span>
            <span className="tab-title">
              {t().compareBranches.tabTitle(compareBranches.left, compareBranches.right)}
            </span>
            <button
              className="tab-close-btn"
              onClick={e => {
                e.stopPropagation();
                onCloseCompareBranches();
              }}
              title={t().common.close}
            >
              <IconWindowClose size={12} />
            </button>
          </div>
        )}
        {/* Diff Tab：只要还开着就留在标签栏里（切去别的文件不消失），点回即可继续看 */}
        {diffOpen && (
          <div
            className={`ide-tab is-diff-tab ${activeTab === 'diff' ? 'active' : ''}`}
            onClick={onSelectDiffTab}
            title={t().editorTab.diffTabTitle}
          >
            <span className="tab-icon" style={{ color: 'var(--accent)', display: 'inline-flex' }}><IconDiff size={12} /></span>
            <span className="tab-title">
              {selectedCommits && selectedCommits.length > 0
                ? t().editorTab.diffCommitsTabName(selectedCommits.length)
                : compareCommits
                ? `Diff (${compareCommits.from.slice(0, 7)} ↔ ${compareCommits.to.slice(0, 7)})`
                : diffCommit
                ? t().editorTab.diffTabName(diffCommit.slice(0, 7))
                : t().editorTab.workingTreeDiff}
            </span>
            <button
              className="tab-close-btn"
              onClick={e => {
                e.stopPropagation();
                onCloseDiffTab();
              }}
              title={t().editorTab.closeDiffTab}
            >
              <IconWindowClose size={12} />
            </button>
          </div>
        )}
      </div>
      <div className="tab-bar-actions">
        <button
          className={`tab-bar-action-btn ${moreMenuOpen ? 'active' : ''}`}
          onClick={e => {
            e.stopPropagation();
            const rect = e.currentTarget.getBoundingClientRect();
            setMoreMenuOpen(prev => (prev ? null : { x: rect.right, y: rect.bottom + 2 }));
          }}
          title={t().editorTab.moreTabs}
        >
          <IconChevronDown size={13} />
        </button>
      </div>
      {/* Context Menu on Tab */}
      {contextMenu && (
        <CtxMenuShell
          className="editor-tab-ctx-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClose={() => setContextMenu(null)}
        >
          <button
            className="ctx-item ctx-row"
            onClick={() => {
              onCloseTab(contextMenu.path);
              setContextMenu(null);
            }}
          >
            <span className="ctx-label">{t().menu.closeTab}</span>
          </button>
          <button
            className="ctx-item ctx-row"
            onClick={() => {
              onCloseOtherTabs(contextMenu.path);
              setContextMenu(null);
            }}
          >
            <span className="ctx-label">{t().editorTab.closeOthers}</span>
          </button>
          <button
            className="ctx-item ctx-row"
            disabled={isFirstTab(contextMenu.path)}
            onClick={() => {
              if (!isFirstTab(contextMenu.path)) {
                onCloseLeftTabs(contextMenu.path);
                setContextMenu(null);
              }
            }}
          >
            <span className="ctx-label">{t().editorTab.closeLeft}</span>
          </button>
          <button
            className="ctx-item ctx-row"
            disabled={isLastTab(contextMenu.path)}
            onClick={() => {
              if (!isLastTab(contextMenu.path)) {
                onCloseRightTabs(contextMenu.path);
                setContextMenu(null);
              }
            }}
          >
            <span className="ctx-label">{t().editorTab.closeRight}</span>
          </button>
          <div className="ctx-sep" />
          <button
            className="ctx-item ctx-row"
            onClick={() => {
              onCloseAllTabs();
              setContextMenu(null);
            }}
          >
            <span className="ctx-label">{t().menu.closeAllTabs}</span>
          </button>
        </CtxMenuShell>
      )}

      {/* More Tabs Dropdown */}
      {moreMenuOpen && (
        <CtxMenuShell
          className="editor-tab-dropdown-menu"
          style={{ top: moreMenuOpen.y, left: Math.max(10, moreMenuOpen.x - 260) }}
          onClose={() => setMoreMenuOpen(null)}
        >
          {compareBranches && (
            <div
              className={`tab-dropdown-item ${activeTab === 'compare-branches' ? 'active' : ''}`}
              onClick={() => {
                setMoreMenuOpen(null);
                onSelectCompareBranches();
              }}
              title={t().compareBranches.tabTitle(compareBranches.left, compareBranches.right)}
            >
              <span className="tab-dropdown-icon" style={{ color: 'var(--accent)', display: 'inline-flex' }}>
                <IconCompareVersions size={12} />
              </span>
              <span className="tab-dropdown-title">
                {t().compareBranches.tabTitle(compareBranches.left, compareBranches.right)}
              </span>
              <button
                className="tab-dropdown-close"
                onClick={e => {
                  e.stopPropagation();
                  onCloseCompareBranches();
                }}
                title={t().common.close}
              >
                <IconWindowClose size={12} />
              </button>
            </div>
          )}
          {diffOpen && (
            <div
              className={`tab-dropdown-item ${activeTab === 'diff' ? 'active' : ''}`}
              onClick={() => {
                setMoreMenuOpen(null);
                onSelectDiffTab();
              }}
            >
              <span className="tab-dropdown-icon" style={{ color: 'var(--accent)', display: 'inline-flex' }}>
                <IconDiff size={12} />
              </span>
              <span className="tab-dropdown-title">
                {selectedCommits && selectedCommits.length > 0
                  ? t().editorTab.diffCommitsTabName(selectedCommits.length)
                  : compareCommits
                  ? `Diff (${compareCommits.from.slice(0, 7)} ↔ ${compareCommits.to.slice(0, 7)})`
                  : diffCommit
                  ? t().editorTab.diffTabName(diffCommit.slice(0, 7))
                  : t().editorTab.workingTreeDiff}
              </span>
              <button
                className="tab-dropdown-close"
                onClick={e => {
                  e.stopPropagation();
                  onCloseDiffTab();
                }}
                title={t().editorTab.closeDiffTab}
              >
                <IconWindowClose size={12} />
              </button>
            </div>
          )}
          {(diffOpen || compareBranches) && openFiles.length > 0 && <div className="ctx-sep" />}
          {openFiles.map(filePath => {
            const fileName = filePath.split(/[\\/]/).pop() ?? filePath;
            const isActive = activeTab === 'editor' && activeFile === filePath;

            return (
              <div
                key={filePath}
                className={`tab-dropdown-item ${isActive ? 'active' : ''}`}
                onClick={() => {
                  setMoreMenuOpen(null);
                  onSelectTab(filePath);
                }}
                title={filePath}
              >
                <span className="tab-dropdown-icon">
                  {renderFileIcon(fileName, false, false)}
                </span>
                <span className="tab-dropdown-title">{fileName}</span>
                {dirtyFiles.includes(filePath) && (
                  <span className="tab-dirty-dot" title={t().editorTab.dirtyTitle}>●</span>
                )}
                <button
                  className="tab-dropdown-close"
                  onClick={e => {
                    e.stopPropagation();
                    onCloseTab(filePath, e);
                  }}
                  title={t().editorTab.closeTab}
                >
                  <IconWindowClose size={12} />
                </button>
              </div>
            );
          })}
        </CtxMenuShell>
      )}
    </div>
  );
});
