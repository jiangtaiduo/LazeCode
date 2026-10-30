// 全局搜索（Search Everywhere）· 异步极速多维检索
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { invokeCommand } from '../services/tauriBridge';
import { getLanguage, t } from '../services/i18n';
import { renderFileIcon, IconSearch } from './FileIcons';

interface ContentHit {
  filePath: string;
  lineNumber: number;
  lineContent: string;
  matchStart: number;
  matchEnd: number;
}

interface FileHit {
  name: string;
  path: string;
  isDirectory: boolean;
}

type Tab = 'files' | 'content' | 'actions';

interface ActionHit {
  id: string;
  label: string;
  hint: string;
}

interface SearchEverywhereModalProps {
  isOpen: boolean;
  workspacePath?: string;
  openFiles?: string[];
  onClose: () => void;
  onOpenFile: (path: string, line?: number) => void;
  /** 操作页执行 App 里已有的动作（不复制业务逻辑） */
  onRunAction?: (id: string) => void;
}

const TABS: Tab[] = ['files', 'content', 'actions'];

export const SearchEverywhereModal: React.FC<SearchEverywhereModalProps> = ({
  isOpen,
  workspacePath = '',
  openFiles = [],
  onClose,
  onOpenFile,
  onRunAction
}) => {
  const [tab, setTab] = useState<Tab>('files');
  const [query, setQuery] = useState('');
  const [fileHits, setFileHits] = useState<FileHit[]>([]);
  const [contentHits, setContentHits] = useState<ContentHit[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // App 订阅语言切换后重渲染本组件；把语言放进依赖，避免动作名冻结在旧语言。
  const language = getLanguage();
  const actions: ActionHit[] = React.useMemo(() => [
    { id: 'open-project', label: t().welcome.openWorkspace, hint: t().menu.openProject },
    { id: 'find-in-files', label: t().findInFiles.title, hint: '⌘⇧F' },
    { id: 'commit', label: t().menu.commitEllipsis, hint: '⌘K' },
    { id: 'push', label: t().menu.push, hint: '⇧⌘K' },
    { id: 'update', label: t().menu.updateProject, hint: '⌘T' },
    { id: 'fetch', label: t().menu.fetch, hint: '' },
    { id: 'git-log', label: t().menu.gitLog, hint: '⌥9' },
    { id: 'terminal', label: t().menu.terminal, hint: '⌥F12' },
    { id: 'settings', label: t().menu.preferences, hint: '⌘,' },
    { id: 'git-tools', label: t().gitTools.title, hint: '' },
    { id: 'refresh', label: t().menu.refresh, hint: 'F5' }
  ], [language]);
  const actionHits = React.useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? actions.filter(a => a.label.toLowerCase().includes(q) || a.id.includes(q)) : actions;
  }, [actions, query]);
  useEffect(() => {
    if (isOpen) {
      setError(null);
      // 记录定时器：弹窗在 20ms 内被关闭时清理，避免定时器把焦点从编辑器夺走
      const timer = setTimeout(() => inputRef.current?.focus(), 20);
      return () => window.clearTimeout(timer);
    }
  }, [isOpen]);

  // 文件检索：异步调用 Rust search_files，避免全量递归锁死内存
  useEffect(() => {
    if (!isOpen || tab !== 'files') return;

    if (!query.trim()) {
      // 查询为空时直接展示当前已打开的标签文件，零耗时
      const defaultList: FileHit[] = openFiles.map(p => ({
        name: p.split(/[\\/]/).pop() ?? p,
        path: p,
        isDirectory: false
      }));
      setFileHits(defaultList);
      setSearching(false);
      return;
    }

    let cancelled = false;
    setSearching(true);
    const timer = window.setTimeout(() => {
      invokeCommand<FileHit[]>('search_files', {
        repoPath: workspacePath || null,
        query: query.trim(),
        limit: 50
      })
        .then(hits => {
          if (!cancelled) {
            setFileHits(hits || []);
            setActiveIndex(0);
            setError(null);
          }
        })
        .catch(err => {
          if (!cancelled) {
            setFileHits([]);
            setError(err instanceof Error ? err.message : String(err));
          }
        })
        .finally(() => {
          if (!cancelled) setSearching(false);
        });
    }, 120);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [isOpen, tab, query, workspacePath, openFiles]);

  // 内容检索：防抖后调用异步 global_search
  useEffect(() => {
    if (!isOpen || tab !== 'content' || query.trim().length === 0) {
      setContentHits([]);
      setSearching(false);
      return;
    }
    let cancelled = false;
    setSearching(true);
    const timer = window.setTimeout(() => {
      invokeCommand<ContentHit[]>('global_search', {
        repoPath: workspacePath || null,
        query: query.trim(),
        caseSensitive: false
      })
        .then(hits => {
          if (!cancelled) {
            setContentHits(hits || []);
            setActiveIndex(0);
            setError(null);
          }
        })
        .catch(err => {
          if (!cancelled) {
            setContentHits([]);
            setError(err instanceof Error ? err.message : String(err));
          }
        })
        .finally(() => {
          if (!cancelled) setSearching(false);
        });
    }, 150);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [isOpen, tab, query, workspacePath]);

  const currentCount = tab === 'files' ? fileHits.length : tab === 'content' ? contentHits.length : actionHits.length;

  const handleSelectActive = useCallback(() => {
    if (tab === 'files') {
      const hit = fileHits[activeIndex];
      if (hit && !hit.isDirectory) {
        onOpenFile(hit.path);
        onClose();
      }
    } else if (tab === 'content') {
      const hit = contentHits[activeIndex];
      if (hit) {
        onOpenFile(hit.filePath, hit.lineNumber);
        onClose();
      }
    } else {
      const hit = actionHits[activeIndex];
      if (hit) {
        onRunAction?.(hit.id);
        onClose();
      }
    }
  }, [tab, fileHits, contentHits, actionHits, activeIndex, onOpenFile, onRunAction, onClose]);

  // 键盘导航
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (currentCount > 0) {
        setActiveIndex(idx => (idx + 1) % currentCount);
      }
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (currentCount > 0) {
        setActiveIndex(idx => (idx - 1 + currentCount) % currentCount);
      }
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      handleSelectActive();
      return;
    }
    if (e.key === 'Tab') {
      e.preventDefault();
      setTab(t => (t === 'files' ? 'content' : t === 'content' ? 'actions' : 'files'));
      setActiveIndex(0);
      return;
    }
  };
  const backdropMouseDownRef = useRef(false);


  if (!isOpen) return null;

  return (
    <div
      className="search-modal-backdrop"
      onMouseDown={e => {
        backdropMouseDownRef.current = e.target === e.currentTarget;
      }}
      onClick={e => {
        if (backdropMouseDownRef.current && e.target === e.currentTarget) {
          onClose();
        }
        backdropMouseDownRef.current = false;
      }}
    >
      <div className="search-modal-dialog" onClick={e => e.stopPropagation()} onKeyDown={handleKeyDown}>
        {/* Header Tabs */}
        <div className="search-modal-tabs">
          {TABS.map(spec => (
            <button
              key={spec}
              className={`search-tab-btn ${tab === spec ? 'active' : ''}`}
              onClick={() => {
                setTab(spec);
                setActiveIndex(0);
                inputRef.current?.focus();
              }}
            >
              {spec === 'files'
                ? t().search.tabFiles
                : spec === 'content'
                  ? t().search.tabContent
                  : t().search.tabActions}
            </button>
          ))}
          <span className="search-modal-hint">{t().search.tabHint}</span>
        </div>

        {/* Search Input Box */}
        <div className="search-input-wrapper">
          <span className="search-input-icon"><IconSearch size={13} color="var(--text-muted)" /></span>
          <input
            ref={inputRef}
            type="text"
            className="search-main-input"
            placeholder={
              tab === 'files'
                ? t().search.placeholderFiles
                : tab === 'content'
                  ? t().search.placeholderContent
                  : t().search.tabActions
            }
            value={query}
            onChange={e => setQuery(e.target.value)}
          />
          {searching && <span className="search-spinner">⋯</span>}
        </div>

        {/* Results List */}
        <div className="search-results-list">
          {error && <div className="search-error-msg">{t().search.searchFailed(error)}</div>}

          {tab === 'files' ? (
            fileHits.length === 0 ? (
              <div className="search-empty-prompt">
                {searching ? t().search.searchingFiles : query ? t().search.noFileMatch : t().search.noOpenFiles}
              </div>
            ) : (
              fileHits.map((hit, idx) => {
                const isSelected = idx === activeIndex;
                const displayPath = workspacePath ? hit.path.replace(workspacePath, '') : hit.path;
                return (
                  <div
                    key={hit.path}
                    className={`search-result-item ${isSelected ? 'selected' : ''}`}
                    onClick={() => {
                      if (!hit.isDirectory) {
                        onOpenFile(hit.path);
                        onClose();
                      }
                    }}
                    onMouseEnter={() => setActiveIndex(idx)}
                  >
                    <span className="result-item-icon">
                      {renderFileIcon(hit.name, hit.isDirectory, false)}
                    </span>
                    <span className="result-item-name">{hit.name}</span>
                    <span className="result-item-path" title={hit.path}>
                      {displayPath}
                    </span>
                  </div>
                );
              })
            )
          ) : tab === 'content' ? (
            contentHits.length === 0 ? (
              <div className="search-empty-prompt">
                {searching ? t().search.searchingContent : query ? t().search.noContentMatch : t().search.enterKeyword}
              </div>
            ) : (
              contentHits.map((hit, idx) => {
                const isSelected = idx === activeIndex;
                const fileName = hit.filePath.split(/[\\/]/).pop() ?? hit.filePath;
                return (
                  <div
                    key={`${hit.filePath}:${hit.lineNumber}`}
                    className={`search-result-item content-hit ${isSelected ? 'selected' : ''}`}
                    onClick={() => {
                      onOpenFile(hit.filePath, hit.lineNumber);
                      onClose();
                    }}
                    onMouseEnter={() => setActiveIndex(idx)}
                  >
                    <span className="result-item-icon">
                      {renderFileIcon(fileName, false, false)}
                    </span>
                    <div className="content-hit-body">
                      <div className="content-hit-header">
                        <span className="content-hit-file">{fileName}</span>
                        <span className="content-hit-line">:{hit.lineNumber}</span>
                        <span className="content-hit-fullpath">{hit.filePath}</span>
                      </div>
                      <div className="content-hit-preview">{hit.lineContent}</div>
                    </div>
                  </div>
                );
              })
            )
          ) : actionHits.length === 0 ? (
            <div className="search-empty-prompt">{t().findInFiles.noResults}</div>
          ) : (
            actionHits.map((hit, idx) => {
              const isSelected = idx === activeIndex;
              return (
                <div
                  key={hit.id}
                  className={`search-result-item ${isSelected ? 'selected' : ''}`}
                  onClick={() => {
                    onRunAction?.(hit.id);
                    onClose();
                  }}
                  onMouseEnter={() => setActiveIndex(idx)}
                >
                  <span className="result-item-icon">
                    {renderFileIcon('Action', false, false)}
                  </span>
                  <span className="result-item-name">{hit.label}</span>
                  <span className="result-item-path">{hit.hint}</span>
                </div>
              );
            })
          )}
        </div>

        {/* Footer */}
        <div className="search-modal-footer">
          <span>{t().search.navUpDown}</span>
          <span>{t().search.openEnter}</span>
          <span>{t().search.escExit}</span>
        </div>
      </div>
    </div>
  );
};
