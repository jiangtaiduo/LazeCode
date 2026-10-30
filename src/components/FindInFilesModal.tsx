// Find in Files / Replace in Files · PyCharm 的 Ctrl+Shift+F 工作流。
//
// 搜索与替换都走后端同一套 regex 规则（literal/regex/whole-word/case），
// 结果返回 UTF-16 偏移供前端高亮；替换保留原文件编码并原子写。

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invokeCommand } from '../services/tauriBridge';
import { appConfirm } from '../services/confirmDialog';
import { t } from '../services/i18n';
import { IconSearch, IconWindowClose, renderFileIcon } from './FileIcons';

interface FindHit {
  filePath: string;
  lineNumber: number;
  lineContent: string;
  matchStart: number;
  matchEnd: number;
}

interface ReplaceResult {
  files: number;
  occurrences: number;
  errors: string[];
  changedPaths: string[];
}

interface FindInFilesModalProps {
  open: boolean;
  workspacePath: string;
  /** 从项目树右键进入时的目录范围；空 = 整个项目 */
  initialScope?: string;
  onClose: () => void;
  onOpenFile: (path: string, line: number) => void;
  /** 替换前先保存脏缓冲，避免“磁盘已替换、编辑器又用旧草稿覆盖回去”。 */
  onBeforeReplace?: () => Promise<void>;
  /** 替换成功后通知 App 丢弃/重载受影响缓冲 */
  onReplaced: (changedPaths: string[]) => void;
}

const MAX_HITS = 1000;

export const FindInFilesModal: React.FC<FindInFilesModalProps> = ({
  open,
  workspacePath,
  initialScope = '',
  onClose,
  onOpenFile,
  onBeforeReplace,
  onReplaced
}) => {
  const [query, setQuery] = useState('');
  const [replacement, setReplacement] = useState('');
  const [showReplace, setShowReplace] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [wholeWord, setWholeWord] = useState(false);
  const [useRegex, setUseRegex] = useState(false);
  const [include, setInclude] = useState('');
  const [scope, setScope] = useState(initialScope);
  const [hits, setHits] = useState<FindHit[]>([]);
  /** 产生当前 hits 的查询签名；与实时输入不一致时禁止 Replace All */
  const [hitsKey, setHitsKey] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [replacing, setReplacing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setScope(initialScope);
    setError(null);
    setNotice(null);
    const timer = window.setTimeout(() => inputRef.current?.focus(), 20);
    return () => window.clearTimeout(timer);
  }, [open, initialScope]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !replacing) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, replacing, onClose]);

  const searchKey = useMemo(
    () =>
      JSON.stringify([
        query,
        scope,
        workspacePath,
        caseSensitive,
        useRegex,
        wholeWord,
        include.trim()
      ]),
    [query, scope, workspacePath, caseSensitive, useRegex, wholeWord, include]
  );

  useEffect(() => {
    if (!open) return;
    if (!query.trim()) {
      setHits([]);
      setHitsKey('');
      setLoading(false);
      setError(null);
      return;
    }
    let cancelled = false;
    // 查询变了就先作废旧结果：不能在旧 hits 上点 Replace All。
    setHits([]);
    setHitsKey('');
    setLoading(true);
    const timer = window.setTimeout(() => {
      invokeCommand<FindHit[]>('global_search', {
        repoPath: scope || workspacePath || null,
        query,
        caseSensitive,
        useRegex,
        wholeWord,
        include: include.trim() || null,
        maxResults: MAX_HITS
      })
        .then(list => {
          if (cancelled) return;
          setHits(list || []);
          setHitsKey(searchKey);
          setError(null);
        })
        .catch(err => {
          if (cancelled) return;
          setHits([]);
          setHitsKey('');
          setError(err instanceof Error ? err.message : String(err));
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }, 160);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [open, searchKey, query, scope, workspacePath, caseSensitive, useRegex, wholeWord, include]);

  const runReplace = useCallback(async () => {
    if (!query.trim() || replacing || loading || hitsKey !== searchKey) return;
    setReplacing(true);
    setError(null);
    setNotice(null);
    try {
      // 搜索最多返回 MAX_HITS 条；达到上限时先做一次不写盘的精确统计，
      // 否则确认框会低报替换范围（实际替换是整棵树）。
      let total = hits.length;
      let uniqueFiles = new Set(hits.map(h => h.filePath)).size;
      if (hits.length >= MAX_HITS) {
        const preview = await invokeCommand<ReplaceResult>('replace_in_files', {
          repoPath: scope || workspacePath || null,
          query,
          replacement,
          caseSensitive,
          useRegex,
          wholeWord,
          include: include.trim() || null,
          dryRun: true
        });
        total = preview.occurrences;
        uniqueFiles = preview.files;
      }
      if (!(await appConfirm(t().findInFiles.replaceConfirm(total, uniqueFiles), {
        title: t().findInFiles.title,
        kind: 'warning',
        okLabel: t().findInFiles.replaceAll
      }))) {
        setReplacing(false);
        return;
      }
      await onBeforeReplace?.();
      const result = await invokeCommand<ReplaceResult>('replace_in_files', {
        repoPath: scope || workspacePath || null,
        query,
        replacement,
        caseSensitive,
        useRegex,
        wholeWord,
        include: include.trim() || null,
        dryRun: false
      });
      setNotice(t().findInFiles.replaced(result.occurrences, result.files));
      if (result.errors.length > 0) setError(result.errors.slice(0, 5).join('\n'));
      onReplaced(result.changedPaths);
      setHits([]);
    } catch (err) {
      setError(t().findInFiles.replaceFailed(err instanceof Error ? err.message : String(err)));
    } finally {
      setReplacing(false);
    }
  }, [query, replacement, replacing, loading, hitsKey, searchKey, hits, scope, workspacePath, caseSensitive, useRegex, wholeWord, include, onBeforeReplace, onReplaced]);
  const backdropMouseDownRef = useRef(false);


  if (!open) return null;

  const scopeLabel = scope ? scope.replace(workspacePath, '').replace(/^[\\/]/, '') || '.' : t().findInFiles.allProject;

  return (
    <div
      className="search-modal-backdrop find-in-files-backdrop"
      onMouseDown={e => {
        backdropMouseDownRef.current = !replacing && e.target === e.currentTarget;
      }}
      onClick={e => {
        if (backdropMouseDownRef.current && e.target === e.currentTarget && !replacing) {
          onClose();
        }
        backdropMouseDownRef.current = false;
      }}
    >
      <div className="find-in-files-dialog" onClick={e => e.stopPropagation()}>
        <div className="find-in-files-header">
          <span className="find-in-files-title">{t().findInFiles.title}</span>
          <span className="find-in-files-scope" title={scope || workspacePath}>
            {t().findInFiles.scope}: {scopeLabel}
          </span>
          <button className="py-dialog-close" onClick={onClose} title={t().findInFiles.close}>
            <IconWindowClose size={13} />
          </button>
        </div>

        <div className="find-in-files-search-row">
          <span className="find-in-files-icon"><IconSearch size={14} color="var(--text-muted)" /></span>
          <input
            ref={inputRef}
            className="find-in-files-input"
            value={query}
            spellCheck={false}
            placeholder={t().findInFiles.placeholderSearch}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void runReplace();
            }}
          />
          <button
            type="button"
            className={`find-in-files-opt ${caseSensitive ? 'active' : ''}`}
            title={t().findInFiles.caseSensitive}
            onClick={() => setCaseSensitive(v => !v)}
          >
            Aa
          </button>
          <button
            type="button"
            className={`find-in-files-opt ${wholeWord ? 'active' : ''}`}
            title={t().findInFiles.wholeWord}
            onClick={() => setWholeWord(v => !v)}
          >
            ab
          </button>
          <button
            type="button"
            className={`find-in-files-opt ${useRegex ? 'active' : ''}`}
            title={t().findInFiles.regex}
            onClick={() => setUseRegex(v => !v)}
          >
            .*
          </button>
          <button
            type="button"
            className={`find-in-files-opt ${showReplace ? 'active' : ''}`}
            title={t().findInFiles.showReplace}
            onClick={() => setShowReplace(v => !v)}
          >
            ⇄
          </button>
        </div>

        {showReplace && (
          <div className="find-in-files-search-row find-in-files-replace-row">
            <span className="find-in-files-icon" />
            <input
              className="find-in-files-input"
              value={replacement}
              spellCheck={false}
              placeholder={t().findInFiles.placeholderReplace}
              onChange={e => setReplacement(e.target.value)}
            />
            <button
              type="button"
              className="py-btn primary sm"
              disabled={replacing || loading || hits.length === 0 || hitsKey !== searchKey}
              onClick={() => void runReplace()}
            >
              {t().findInFiles.replaceAll}
            </button>
          </div>
        )}

        <div className="find-in-files-search-row find-in-files-filter-row">
          <span className="find-in-files-icon" />
          <input
            className="find-in-files-input"
            value={include}
            spellCheck={false}
            placeholder={t().findInFiles.includePlaceholder}
            onChange={e => setInclude(e.target.value)}
          />
          <span className="find-in-files-count">
            {loading ? t().findInFiles.searching : t().findInFiles.resultsCount(hits.length)}
          </span>
        </div>

        {error && <div className="find-in-files-error">{error}</div>}
        {notice && <div className="find-in-files-notice">{notice}</div>}

        <div className="find-in-files-results">
          {!loading && !error && hits.length === 0 && query.trim() && (
            <div className="find-in-files-empty">{t().findInFiles.noResults}</div>
          )}
          {hits.map((hit, idx) => {
            const before = hit.lineContent.slice(0, hit.matchStart);
            const match = hit.lineContent.slice(hit.matchStart, hit.matchEnd);
            const after = hit.lineContent.slice(hit.matchEnd);
            const name = hit.filePath.split(/[\\/]/).pop() ?? hit.filePath;
            return (
              <div
                key={`${hit.filePath}:${hit.lineNumber}:${idx}`}
                className="find-in-files-hit"
                onClick={() => {
                  onOpenFile(hit.filePath, hit.lineNumber);
                  onClose();
                }}
                title={`${hit.filePath}:${hit.lineNumber}`}
              >
                <span className="find-in-files-file-icon">{renderFileIcon(name, false, false)}</span>
                <span className="find-in-files-path">{hit.filePath.replace(workspacePath, '').replace(/^[\\/]/, '')}</span>
                <span className="find-in-files-line-no">{hit.lineNumber}</span>
                <span className="find-in-files-line-text">
                  {before}
                  <mark>{match}</mark>
                  {after}
                </span>
              </div>
            );
          })}
        </div>

        <div className="find-in-files-footer">{t().findInFiles.openFileHint}</div>
      </div>
    </div>
  );
};
