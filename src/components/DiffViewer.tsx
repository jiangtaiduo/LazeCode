// 真实 Diff 查看器
//
// 数据来自 Rust 侧 `git_diff` / `git_show` / `fs_diff_files`：
// FileDiff（patch）负责统计、空态/二进制判定与多文件识别；
// 单文件差异再拉两侧完整原文（git_file_at_rev / read_file_content），
// 交给 Monaco DiffEditor —— 语法高亮、主题、并排/统一视图与编辑器共用一套能力。
// 整个提交的多文件 diff 没有单一语言，退回按行渲染 patch（保留真实行号）。

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import type { MonacoDiffEditor } from '@monaco-editor/react';
import { invokeCommand } from '../services/tauriBridge';
import { ensureMonacoReady, MONACO_THEME_BY_APP_THEME, UNICODE_HIGHLIGHT_OPTIONS } from '../services/monacoSetup';
import { languageOf } from '../services/fileTypes';
import { loadSettings, subscribeSettings, type AppSettings } from '../services/appSettings';
import { readDiffViewMode, rememberDiffViewMode, type DiffViewMode } from '../services/diffViewMode';
import { buildCompareFileTree, flattenCompareFileTree } from '../services/compareBranches';
import { t } from '../services/i18n';
import type { GitChangedFile, GitCommitFile } from '../types';
import { IconChevronUp, IconChevronDown, IconArrowLeft, IconArrowRight } from './FileIcons';

/**
 * 设置快照的单份缓存。
 *
 * themeId / fontSize 只是这两个 prop 缺省时的兜底值，但设置存在 localStorage，
 * 每次渲染都 loadSettings() 就是一次读取 + JSON.parse。设置只在 persistSettings
 * 这一个点变化，于是缓存快照 + 订阅那个点即可：缓存不能过期，也不必每次渲染重读。
 */
let settingsSnapshot: AppSettings | null = null;
function settingsSnapshotNow(): AppSettings {
  if (!settingsSnapshot) settingsSnapshot = loadSettings();
  return settingsSnapshot;
}
subscribeSettings(next => {
  settingsSnapshot = next;
});


/** DiffEditor 单独成 chunk；挂载前必须已过 ensureMonacoReady 门控，
 *  否则 loader 会回退到 jsdelivr CDN 拉 monaco。 */
const DiffEditorPane = React.lazy(() =>
  import('@monaco-editor/react').then(m => ({ default: m.DiffEditor }))
);

export interface DiffLine {
  kind: 'context' | 'add' | 'del' | 'hunk' | 'meta';
  oldLine: number | null;
  newLine: number | null;
  text: string;
  /** 行末没有换行符（EOF 标记，由后端解析 \ No newline 得出） */
  noNewline?: boolean;
  /** 该行是新文件末行且末行无换行：旧文件尾部可能还有被折叠的尾行 */
  eofNoNewline?: boolean;
}

export interface FileDiff {
  path: string;
  oldPath: string | null;
  isNew: boolean;
  isDeleted: boolean;
  isBinary: boolean;
  additions: number;
  deletions: number;
  lines: DiffLine[];
  /** 后端行数封顶标记（见 commands.rs MAX_DIFF_LINES）：增删计数仍是全量的，
   *  只有展示行被截断。调用方应提示用户而非静默。 */
  truncated?: boolean;
}

interface DiffViewerProps {
  /** 工作区中要比较的文件路径 */
  filePath: string;
  /** 仓库根目录；缺失时退化为进程 cwd（几乎总是错误场景） */
  repoPath?: string;
  /** 已暂存则比较索引 vs HEAD，否则工作区 vs 索引 */
  staged?: boolean;
  /** 非空时改为显示某个提交的 diff，忽略 filePath/staged */
  showCommit?: string | null;
  /** 非空时显示该提交版本与工作区的差异（文件历史「与工作区比较」），优先于 showCommit */
  againstRef?: string | null;
  /** 非空时显示任意两个磁盘文件的对比（项目树 Compare With…），优先于一切 git 模式 */
  fsFiles?: { oldPath: string; newPath: string } | null;
  /** 非空时显示两个提交之间的差异（Compare Versions） */
  compareCommits?: { from: string; to: string } | null;
  /** 非空时显示显式选中的多个提交的合并 patch（不做首尾范围推断） */
  selectedCommits?: string[] | null;
  /** 变化时强制重新拉取 diff（提交/回滚等改写工作区的操作后，已打开的 diff 需刷新） */
  refreshKey?: number;
  /** 主题 id（0 = 跟随 uiVariant，由调用方解好）；缺省时自己读一次设置 */
  themeId?: number;
  /** Monaco 字体大小；缺省读设置 editorFontSize */
  fontSize?: number;
  /** 到达当前文件首/末处差异后再点一次时，切换到列表中的另一个文件 */
  onNavigateFile?: (path: string) => void;
  /** hunk 级 Stage/Unstage/Rollback 成功后通知 App 丢弃缓冲/刷新 Git */
  onWorkingTreeChanged?: (absPath: string) => void;
  /** 本地改动视图切换「索引 vs HEAD / 工作区 vs 索引」 */
  onStagedChange?: (staged: boolean) => void;
}

/** 从完整 patch 行里截出单个 hunk 的补丁（含文件头，git apply 需要）。 */
function buildHunkPatch(lines: DiffLine[], hunkIndex: number): string | null {
  if (hunkIndex < 0 || lines[hunkIndex]?.kind !== 'hunk') return null;
  let fileStart = 0;
  for (let i = hunkIndex; i >= 0; i--) {
    if (lines[i].kind === 'meta' && lines[i].text.startsWith('diff --git ')) {
      fileStart = i;
      break;
    }
  }
  let end = hunkIndex + 1;
  while (end < lines.length) {
    if (lines[end].kind === 'hunk') break;
    if (lines[end].kind === 'meta' && lines[end].text.startsWith('diff --git ')) break;
    end++;
  }
  // DiffLine.text 不含 +/-/space 前缀（渲染时补）；构造给 git apply 的
  // 补丁必须还原前缀，否则上下文行会被当成新增行。
  const body: string[] = [];
  for (const l of lines.slice(fileStart, end)) {
    body.push(
      l.kind === 'add' ? `+${l.text}` :
        l.kind === 'del' ? `-${l.text}` :
          l.kind === 'context' ? ` ${l.text}` :
            l.text
    );
    // Rust parser 把 "\ No newline at end of file" 拆成标志位；重建补丁时
    // 必须把标记行补回去，否则 git apply --check 会认为末行换行语义不同。
    if (l.noNewline || l.eofNoNewline) {
      body.push('\\ No newline at end of file');
    }
  }
  if (body.length === 0) return null;
  return `${body.join('\n')}\n`;
}

/** `diff --git` meta 行里的文件路径列表（b 侧优先，含多文件 whole-commit） */
function metaSectionPaths(lines: DiffLine[]): string[] {
  const out: string[] = [];
  for (const l of lines) {
    if (l.kind === 'meta' && l.text.startsWith('diff --git ')) {
      const m = /^diff --git "?a\/(.*?)"? "?b\/(.*?)"?$/.exec(l.text);
      const p = (m && (m[2] || m[1])) || '';
      if (p && !out.includes(p)) out.push(p);
    }
  }
  return out;
}

const hasTextContent = (lines: DiffLine[]) =>
  lines.some(l => l.kind === 'context' || l.kind === 'add' || l.kind === 'del');

const isAbsPath = (p: string) => /^([A-Za-z]:[\\/]|\/)/.test(p);

/** `git show rev:path` 只认仓库相对路径 → 绝对路径剥掉仓库前缀 */
function toGitRel(p: string, repo?: string): string {
  if (repo && isAbsPath(p)) {
    const base = repo.replace(/[\\/]+$/, '');
    if (p.startsWith(base + '/')) return p.slice(base.length + 1);
    if (p.startsWith(base + '\\')) return p.slice(base.length + 1).replace(/\\/g, '/');
  }
  return p;
}

/** read_file_content 只认绝对路径 → 相对路径补仓库前缀 */
function toAbsPath(p: string, repo?: string): string {
  if (!p || isAbsPath(p) || !repo) return p;
  return `${repo.replace(/[\\/]+$/, '')}/${p}`;
}

const BINARY_EXTENSIONS = new Set([
  'bin', 'exe', 'dll', 'so', 'dylib', 'a', 'o', 'obj', 'lib',
  'png', 'jpg', 'jpeg', 'gif', 'bmp', 'ico', 'icns', 'webp', 'tiff', 'psd',
  'zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar', 'iso', 'dmg', 'pkg',
  'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx',
  'mp3', 'wav', 'ogg', 'flac', 'aac', 'm4a',
  'mp4', 'mov', 'avi', 'mkv', 'webm',
  'woff', 'woff2', 'ttf', 'eot', 'otf',
  'pyc', 'pyo', 'pyd', 'class', 'jar', 'war'
]);

function isBinaryPath(path?: string): boolean {
  if (!path) return false;
  const ext = path.split('.').pop()?.toLowerCase();
  return ext ? BINARY_EXTENSIONS.has(ext) : false;
}

const MonacoBusy: React.FC<{ label: string }> = ({ label }) => (
  <div className="editor-lazy-placeholder">
    <div className="editor-lazy-spinner" />
    <div className="editor-lazy-label">{label}</div>
  </div>
);

const ADD_STYLE: React.CSSProperties = {
  background: 'var(--git-add-bg)',
  borderLeft: '3px solid #3fb950'
};
const DEL_STYLE: React.CSSProperties = {
  background: 'var(--git-del-bg)',
  borderLeft: '3px solid #f85149'
};
const HUNK_STYLE: React.CSSProperties = {
  background: 'rgba(56,139,253,0.10)',
  color: 'var(--text-muted)',
  borderLeft: '3px solid transparent'
};
const META_STYLE: React.CSSProperties = {
  color: 'var(--text-muted)',
  borderLeft: '3px solid transparent'
};
const CONTEXT_STYLE: React.CSSProperties = { borderLeft: '3px solid transparent' };

const GUTTER: React.CSSProperties = {
  width: '46px',
  textAlign: 'right',
  paddingRight: '8px',
  color: 'var(--text-muted)',
  userSelect: 'none',
};

/** unified 展示行：mergedWith 非空 = 与相邻异型行合并（内容相同的 -/+ 对） */
interface UnifiedRow {
  l: DiffLine;
  i: number;
  mergedWith?: DiffLine;
}

/** 行高：与容器 lineHeight 19px 严格一致 */
const DIFF_ROW_H = 19;
/** 视口外上下各多渲染的行数 */
const DIFF_OVERSCAN = 40;

export const DiffViewer: React.FC<DiffViewerProps> = ({
  filePath,
  repoPath,
  staged = false,
  showCommit = null,
  againstRef = null,
  compareCommits = null,
  selectedCommits = null,
  fsFiles = null,
  refreshKey = 0,
  themeId,
  fontSize,
  onNavigateFile,
  onWorkingTreeChanged,
  onStagedChange
}) => {
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<DiffViewMode>(readDiffViewMode);
  const [currentHunk, setCurrentHunk] = useState(0);
  const currentHunkRef = useRef(0);
  currentHunkRef.current = currentHunk;
  /** 新文件首次显示时定位首处差异；后续 onDidUpdateDiff 不得重置当前进度 */
  const revealedRef = useRef(false);
  const highlightHunkRef = useRef<(idx: number, scrollIntoView?: boolean) => void>(() => {});
  const [hunkBusy, setHunkBusy] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportH, setViewportH] = useState(600);
  /** 单文件差异的两侧完整原文（patch 只有 hunk+上下文，重建会丢真） */
  const [contents, setContents] = useState<{ original: string; modified: string } | null>(null);
  const [monacoReady, setMonacoReady] = useState(false);
  const diffEditorRef = useRef<MonacoDiffEditor | null>(null);
  const [monacoHunkCount, setMonacoHunkCount] = useState(0);
  const modDecorationsRef = useRef<string[]>([]);
  const origDecorationsRef = useRef<string[]>([]);
  const modBoxRef = useRef<HTMLDivElement | null>(null);
  const origBoxRef = useRef<HTMLDivElement | null>(null);
  /** 当前 diff 会话的全部文件（绝对路径），用于首/末差异后再点一次跨文件跳转 */
  const [fileList, setFileList] = useState<string[]>([]);
  /** 边界提示：null=无；{dir,pending} 表示已提示、再点同方向则切文件 */
  const [edgeHint, setEdgeHint] = useState<string | null>(null);
  const edgePendingRef = useRef<1 | -1 | 0>(0);
  const edgeHintTimerRef = useRef<number | null>(null);

  const settings = useSyncExternalStore(subscribeSettings, settingsSnapshotNow);
  const resolvedThemeId = themeId ?? (settings.themeId === 0 ? settings.uiVariant : settings.themeId);
  const resolvedFontSize = fontSize ?? settings.editorFontSize ?? 13;

  const disposedRef = useRef(false);
  /** 内容/模式变化后恢复该 diff 的记忆位置；没有记忆才回顶部。
   *  用 layout effect 在绘制前认领新 key：DOM 内容替换本身也会触发 scroll 事件，
   *  晚了会把新内容的顶部位写进旧文件。旧位置已由 onScroll 持续记入 Map。 */
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = 0;
    setScrollTop(0);
    if (el) setViewportH(el.clientHeight || 600);
    edgePendingRef.current = 0;
    setEdgeHint(null);
    if (edgeHintTimerRef.current != null) {
      window.clearTimeout(edgeHintTimerRef.current);
      edgeHintTimerRef.current = null;
    }
  }, [diff, mode]);

  useEffect(() => {
    let cancelled = false;

    const safeRead = async (p: string) => {
      if (!p) return '';
      try {
        return await invokeCommand<string>('read_file_content', { path: p });
      } catch {
        return '';
      }
    };
    const safeShow = async (rev: string, relPath: string) => {
      if (!relPath) return '';
      try {
        return await invokeCommand<string>('git_file_at_rev', {
          repoPath: repoPath || null,
          rev,
          path: relPath
        });
      } catch {
        return '';
      }
    };
    /** 单文件两侧原文，优先级与 patch 来源一致（fs > againstRef > commit > staged > 工作区） */
    const loadBothSides = async (
      target: string,
      isNewFile: boolean
    ): Promise<{ original: string; modified: string }> => {
      if (fsFiles) {
        const [original, modified] = await Promise.all([
          safeRead(fsFiles.oldPath),
          safeRead(fsFiles.newPath)
        ]);
        return { original, modified };
      }
      const rel = toGitRel(target, repoPath);
      const abs = toAbsPath(target, repoPath);
      if (compareCommits) {
        const [original, modified] = await Promise.all([
          safeShow(compareCommits.from, rel),
          safeShow(compareCommits.to, rel)
        ]);
        return { original, modified };
      }
      if (againstRef) {
        const [original, modified] = await Promise.all([
          safeShow(againstRef, rel),
          safeRead(abs)
        ]);
        return { original, modified };
      }
      if (showCommit) {
        const [original, modified] = await Promise.all([
          safeShow(`${showCommit}^`, rel),
          safeShow(showCommit, rel)
        ]);
        return { original, modified };
      }
      if (staged) {
        const [original, modified] = await Promise.all([safeShow('HEAD', rel), safeShow('', rel)]);
        return { original, modified };
      }
      // 工作区比较：正常取「索引 vs 磁盘」。新增文件（未跟踪 / 已暂存但 HEAD 里没有）
      // 的索引侧与磁盘内容相同，直接对比会得出「无差异」——与 patch 的全新增语义
      // 矛盾。这类文件的旧侧只能是空文件，改取 HEAD 版本（必然缺失 → 空）。
      const [original, modified] = await Promise.all([
        safeShow(isNewFile ? 'HEAD' : '', rel),
        safeRead(abs)
      ]);
      return { original, modified };
    };

    (async () => {
      setLoading(true);
      setError(null);
      // 刻意不清空 contents：切文件时保留内容区与已挂载的 DiffEditor，新内容到达后
      // 由 @monaco-editor/react 就地写进同一对 model（它按 original/modified prop
      // 变化 setValue，不会重建编辑器）。卸载再挂载会销毁/重建两个 model 与 diff
      // 组件，大文件下切换白闪明显。加载反馈仍由工具栏的 loading 标志（"读取中…"）
      // 承担：等待期间内容区短暂显示上一个文件的内容。
      try {
        const result = fsFiles
          ? await invokeCommand<FileDiff>('fs_diff_files', {
              oldPath: fsFiles.oldPath,
              newPath: fsFiles.newPath
            })
          : selectedCommits && selectedCommits.length > 0
          ? await invokeCommand<FileDiff>('git_commits_diff', {
              repoPath: repoPath || null,
              commits: selectedCommits,
              path: filePath || null
            })
          : compareCommits
          ? await invokeCommand<FileDiff>('git_diff_commits', {
              repoPath: repoPath || null,
              fromCommit: compareCommits.from,
              toCommit: compareCommits.to,
              path: filePath || null
            })
          : againstRef
          ? await invokeCommand<FileDiff>('git_diff_ref_worktree', {
              refName: againstRef,
              path: filePath
            })
          : showCommit
            ? await invokeCommand<FileDiff>('git_show', {
                repoPath: repoPath || null,
                commit: showCommit,
                path: filePath || null
              })
            : await invokeCommand<FileDiff>('git_diff', {
                repoPath: repoPath || null,
                path: filePath,
                staged
              });
        if (cancelled) return;
        setDiff(result);
        // 单文件且有文本差异 → 再取两侧完整原文喂 Monaco
        const sectionPaths = metaSectionPaths(result.lines);
        const target = fsFiles?.newPath || filePath || sectionPaths[0] || '';
        if (
          !selectedCommits &&
          !result.isBinary &&
          hasTextContent(result.lines) &&
          sectionPaths.length <= 1 &&
          target
        ) {
          const pair = await loadBothSides(target, !!result.isNew);
          if (!cancelled) setContents(pair);
        } else {
          // 二进制或无文本差异：清空旧文件原文，避免残存文本引发 Monaco 错位
          if (!cancelled) setContents(null);
        }
      } catch (err) {
        if (!cancelled) {
          setDiff(null);
          setError(err instanceof Error ? err.message : String(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [filePath, repoPath, staged, showCommit, againstRef, compareCommits?.from, compareCommits?.to, selectedCommits, fsFiles, refreshKey]);

  /** 展示行：隐藏 diff --git / index / --- / +++ 等文件头噪音（标题栏已展示文件名），保留原索引用于跳转定位 */
  const visibleLines = useMemo(
    () =>
      (diff?.lines ?? [])
        .map((l, i) => ({ l, i }))
        .filter(x => x.l.kind !== 'meta'),
    [diff]
  );
  /** 差异块首行的索引，用于 F7/Shift+F7 跳转 */
  const hunkOffsets = useMemo(
    () =>
      (diff?.lines ?? [])
        .map((l, i) => (l.kind === 'hunk' ? i : -1))
        .filter(i => i >= 0),
    [diff]
  );

  const sectionPaths = useMemo(() => metaSectionPaths(diff?.lines ?? []), [diff]);

  // 跨文件跳转清单：提交文件 / 两个提交的范围 / ref↔工作区 / 工作区(暂存态)。
  // 统一存仓库相对路径：与右侧文件树的路径、git pathspec 以及 onNavigateFile
  // 回写的 diffFilePath 保持同一形态。Compare Branches 的范围 diff 还必须按
  // 右侧文件树的实际渲染顺序展开，绝不能用 git_status（工作区）或 git diff
  // 的路径顺序猜下一个文件。
  useEffect(() => {
    let cancelled = false;
    setFileList([]);
    (async () => {
      if (fsFiles) return;
      try {
        if (selectedCommits && selectedCommits.length > 0) {
          const files = await invokeCommand<GitCommitFile[]>('git_commits_files', {
            repoPath: repoPath || null,
            commits: selectedCommits
          });
          if (!cancelled) {
            setFileList(flattenCompareFileTree(buildCompareFileTree(files)));
          }
        } else if (compareCommits) {
          const files = await invokeCommand<GitCommitFile[]>('git_compare_commits', {
            repoPath: repoPath || null,
            fromCommit: compareCommits.from,
            toCommit: compareCommits.to
          });
          if (!cancelled) {
            setFileList(flattenCompareFileTree(buildCompareFileTree(files)));
          }
        } else if (showCommit) {
          const files = await invokeCommand<GitCommitFile[]>('git_commit_files', {
            repoPath: repoPath || null,
            commit: showCommit
          });
          if (!cancelled) {
            setFileList(flattenCompareFileTree(buildCompareFileTree(files)));
          }
        } else if (againstRef) {
          const r = await invokeCommand<{ files: { path: string }[] }>('git_compare_refs', {
            repoPath: repoPath || null,
            refName: againstRef,
            againstWorktree: true
          });
          if (!cancelled) setFileList(r.files.map(f => f.path));
        } else if (repoPath) {
          const st = await invokeCommand<GitChangedFile[]>('git_status', {
            repoPath
          });
          if (!cancelled) {
            // 同一文件可以既有 staged 又有 unstaged 改动：按对应侧分别进入列表，
            // 不能只用一个 isStaged 布尔过滤（会把 unstaged 部分整个藏掉）。
            setFileList(
              st.filter(f => (staged ? f.isStaged : f.isUnstaged)).map(f => f.path)
            );
          }
        }
      } catch {
        if (!cancelled) setFileList([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [fsFiles, showCommit, againstRef, compareCommits?.from, compareCommits?.to, selectedCommits, repoPath, staged, refreshKey]);
  /** 语言/原文取此路径：fsFiles 新侧 → 单文件 filePath → patch 唯一 section */
  const targetPath = fsFiles?.newPath || filePath || sectionPaths[0] || '';
  /** 整个提交的多文件 patch 混多语言，无法定语言 → 回退按行渲染 */
  const isMultiFile = sectionPaths.length > 1;
  const isBinaryDiff =
    !!diff?.isBinary ||
    isBinaryPath(targetPath) ||
    isBinaryPath(filePath) ||
    (diff?.lines ?? []).some(l => l.kind === 'meta' && l.text.startsWith('Binary files'));
  const readyForMonaco =
    !selectedCommits &&
    !isBinaryDiff &&
    !!diff &&
    hasTextContent(diff.lines) &&
    !isMultiFile &&
    !!targetPath &&
    (!contents || contents.original.length > 0 || contents.modified.length > 0);
  const useMonaco = readyForMonaco && monacoReady && !!contents;
  const canHunkAction =
    !isBinaryDiff &&
    !hunkBusy &&
    !!repoPath &&
    !!diff &&
    !showCommit &&
    !againstRef &&
    !compareCommits &&
    !selectedCommits &&
    !fsFiles &&
    !isMultiFile;

  const applyHunk = useCallback(
    async (hunkIndex: number, action: 'stage' | 'unstage' | 'rollback') => {
      if (!repoPath || !diff) return;
      const patch = buildHunkPatch(diff.lines, hunkIndex);
      if (!patch) return;
      setHunkBusy(true);
      setError(null);
      try {
        await invokeCommand('git_apply_patch', {
          repoPath,
          patch,
          cached: action !== 'rollback',
          reverse: action !== 'stage'
        });
        const abs = toAbsPath(targetPath, repoPath);
        onWorkingTreeChanged?.(abs);
      } catch (err) {
        setError(t().diff.hunkActionFailed(err instanceof Error ? err.message : String(err)));
      } finally {
        setHunkBusy(false);
      }
    },
    [repoPath, diff, targetPath, onWorkingTreeChanged]
  );
  /** 当前是否真的挂着 DiffEditor：卸载后 diffEditorRef 会指向已销毁实例，不能对它调 API */
  const diffPaneLiveRef = useRef(false);
  diffPaneLiveRef.current = useMonaco;

  /** 当 Monaco 就绪且 diff 产生变化时，精确定位并居中到首个差异行 */
  const revealFirstDiffNow = useCallback((ed: MonacoDiffEditor) => {
    if (disposedRef.current || !diffPaneLiveRef.current) return;
    const changes = ed.getLineChanges();
    if (!changes || changes.length === 0) return;
    const change = changes[0];
    const modEd = ed.getModifiedEditor();
    const origEd = ed.getOriginalEditor();
    if (mode === 'unified') {
      const targetLine = change.modifiedStartLineNumber > 1 ? change.modifiedStartLineNumber - 1 : change.modifiedStartLineNumber;
      modEd.revealLineInCenter(targetLine);
      modEd.setPosition({ lineNumber: targetLine, column: 1 });
    } else {
      if (change.modifiedEndLineNumber > 0) {
        modEd.revealLineInCenter(change.modifiedStartLineNumber);
        modEd.setPosition({ lineNumber: change.modifiedStartLineNumber, column: 1 });
      } else {
        origEd.revealLineInCenter(change.originalStartLineNumber);
        origEd.setPosition({ lineNumber: change.originalStartLineNumber, column: 1 });
      }
    }
    ed.revealFirstDiff();
  }, [mode]);

  useLayoutEffect(() => {
    if (!useMonaco || !contents) {
      diffEditorRef.current = null;
    }
  }, [contents, useMonaco]);

  useEffect(() => () => {
    disposedRef.current = true;
    diffEditorRef.current = null;
  }, []);

  // 仅在会用到 DiffEditor 时预热 monaco（未 ready 就挂载会回退 CDN）
  useEffect(() => {
    if (!readyForMonaco) return;
    let cancelled = false;
    ensureMonacoReady(resolvedThemeId).then(
      () => {
        if (!cancelled) setMonacoReady(true);
      },
      () => {}
    );
    return () => {
      cancelled = true;
    };
  }, [readyForMonaco, resolvedThemeId]);

  const diffOptions = useMemo(
    () => ({
      readOnly: true,
      renderSideBySide: mode === 'side-by-side',
      diffAlgorithm: 'advanced' as const,
      ignoreTrimWhitespace: false,
      renderOverviewRuler: true,
      minimap: { enabled: false },
      folding: false,
      scrollBeyondLastLine: false,
      automaticLayout: true,
      wordWrap: 'off' as const,
      renderLineHighlight: 'none' as const,
      renderIndicators: false,
      fontFamily: 'JetBrains Mono, Menlo, Monaco, "Courier New", monospace',
      fontSize: resolvedFontSize,
      lineHeight: Math.round(resolvedFontSize * 1.5),
      fontLigatures: true,
      unicodeHighlight: UNICODE_HIGHLIGHT_OPTIONS,
      padding: { top: 4, bottom: 4 },
      scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10 },
      maxComputationTime: 10000
    }),
    [mode, resolvedFontSize]
  );

  /** 按原行号把虚拟化窗口滚到某行（hunk 行）居中 */
  const scrollToLineIndex = (lineIdx: number) => {
    const container = scrollRef.current;
    if (!container) return;
    const rows = mode === 'unified' ? unifiedRows : visibleLines;
    const pos = rows.findIndex(r => r.i === lineIdx);
    if (pos < 0) {
      const el = container.querySelector<HTMLElement>(`[data-line="${lineIdx}"]`);
      el?.scrollIntoView({ block: 'center' });
      return;
    }
    const top = Math.max(0, Math.round(pos * DIFF_ROW_H - container.clientHeight / 2));
    container.scrollTop = top;
    setScrollTop(top);
  };

  /** 高亮 Monaco DiffEditor 中指定序号的差异块，并使视野对齐 */
  const ensureBoxElement = (editor: any, ref: React.MutableRefObject<HTMLDivElement | null>): HTMLDivElement | null => {
    if (!editor) return null;
    const domNode = editor.getDomNode?.();
    if (!domNode) return null;
    const linesContent = domNode.querySelector('.lines-content');
    if (!linesContent) return null;

    if (!ref.current || !linesContent.contains(ref.current)) {
      const box = document.createElement('div');
      box.className = 'diff-hunk-active-box';
      linesContent.appendChild(box);
      ref.current = box;
    }
    return ref.current;
  };

  const updateHunkBox = useCallback((changeIndex: number) => {
    const ed = diffEditorRef.current;
    if (!ed) return;
    const changes = ed.getLineChanges();
    if (!changes || changes.length === 0 || changeIndex < 0 || changeIndex >= changes.length) {
      if (modBoxRef.current) modBoxRef.current.style.display = 'none';
      if (origBoxRef.current) origBoxRef.current.style.display = 'none';
      return;
    }
    const change = changes[changeIndex];
    const isUnified = mode === 'unified';

    const modEd = ed.getModifiedEditor();
    const origEd = ed.getOriginalEditor();

    if (isUnified) {
      // 统一视图（Unified）：删除的行以 view-zone 形式置于 modified 对应行上方
      // 完整的变更由删除部分（上方）+ 新增部分（下方）合为一个连续矩形框
      const box = ensureBoxElement(modEd, modBoxRef);
      if (origBoxRef.current) origBoxRef.current.style.display = 'none';

      if (box) {
        let top = 0;
        let bottom = 0;

        const hasDeletions = change.originalEndLineNumber > 0;
        const hasAdditions = change.modifiedEndLineNumber > 0;

        if (hasDeletions && !hasAdditions) {
          // 纯删除：Monaco 的 modifiedEndLineNumber 恒为 0，
          // modifiedStartLineNumber 恰为删除区域正上方的行号。
          // 红色删除 ViewZone 恰位于该行与下一行之间。
          top = change.modifiedStartLineNumber > 0
            ? modEd.getBottomForLineNumber(change.modifiedStartLineNumber)
            : 0;
          const lineCount = modEd.getModel()?.getLineCount() ?? 1;
          if (change.modifiedStartLineNumber < lineCount) {
            bottom = modEd.getTopForLineNumber(change.modifiedStartLineNumber + 1);
          } else {
            const lineHeight = typeof modEd.getOption === 'function' ? modEd.getOption(75) : 19;
            const delLineCount = Math.max(1, change.originalEndLineNumber - change.originalStartLineNumber + 1);
            bottom = top + delLineCount * lineHeight;
          }
        } else if (hasDeletions && hasAdditions) {
          // 删改混合：上方删除 ViewZone + 下方新增行
          // 顶缘为删除 ViewZone 顶缘（即 modifiedStartLineNumber - 1 的底缘）
          // 底缘为新增最后一行的底缘
          top = change.modifiedStartLineNumber > 1
            ? modEd.getBottomForLineNumber(change.modifiedStartLineNumber - 1)
            : 0;
          bottom = modEd.getBottomForLineNumber(change.modifiedEndLineNumber);
        } else {
          // 纯新增：从新增起始行顶缘到结束行底缘
          top = modEd.getTopForLineNumber(change.modifiedStartLineNumber);
          bottom = modEd.getBottomForLineNumber(change.modifiedEndLineNumber);
        }

        const height = Math.max(18, bottom - top);
        const layout = modEd.getLayoutInfo();
        const contentWidth = layout ? layout.contentWidth : 600;
        const scrollLeft = typeof modEd.getScrollLeft === 'function' ? modEd.getScrollLeft() : 0;

        box.style.display = 'block';
        box.style.top = `${top}px`;
        box.style.height = `${height}px`;
        box.style.left = `${scrollLeft}px`;
        box.style.width = `${contentWidth}px`;

        box.classList.remove('diff-hunk-active-box-pulse');
        void box.offsetWidth;
        box.classList.add('diff-hunk-active-box-pulse');
      }
    } else {
      // 并排对比（Side-by-side）：左侧框删除区，右侧框新增区
      const modBox = ensureBoxElement(modEd, modBoxRef);
      const origBox = ensureBoxElement(origEd, origBoxRef);

      if (modBox) {
        if (change.modifiedEndLineNumber > 0) {
          const top = modEd.getTopForLineNumber(change.modifiedStartLineNumber);
          const bottom = modEd.getBottomForLineNumber(change.modifiedEndLineNumber);
          const layout = modEd.getLayoutInfo();
          const contentWidth = layout ? layout.contentWidth : 400;
          const scrollLeft = typeof modEd.getScrollLeft === 'function' ? modEd.getScrollLeft() : 0;

          modBox.style.display = 'block';
          modBox.style.top = `${top}px`;
          modBox.style.height = `${Math.max(18, bottom - top)}px`;
          modBox.style.left = `${scrollLeft}px`;
          modBox.style.width = `${contentWidth}px`;
          modBox.classList.remove('diff-hunk-active-box-pulse');
          void modBox.offsetWidth;
          modBox.classList.add('diff-hunk-active-box-pulse');
        } else {
          modBox.style.display = 'none';
        }
      }

      if (origBox) {
        if (change.originalEndLineNumber > 0) {
          const top = origEd.getTopForLineNumber(change.originalStartLineNumber);
          const bottom = origEd.getBottomForLineNumber(change.originalEndLineNumber);
          const layout = origEd.getLayoutInfo();
          const contentWidth = layout ? layout.contentWidth : 400;
          const scrollLeft = typeof origEd.getScrollLeft === 'function' ? origEd.getScrollLeft() : 0;

          origBox.style.display = 'block';
          origBox.style.top = `${top}px`;
          origBox.style.height = `${Math.max(18, bottom - top)}px`;
          origBox.style.left = `${scrollLeft}px`;
          origBox.style.width = `${contentWidth}px`;
          origBox.classList.remove('diff-hunk-active-box-pulse');
          void origBox.offsetWidth;
          origBox.classList.add('diff-hunk-active-box-pulse');
        } else {
          origBox.style.display = 'none';
        }
      }
    }
  }, [mode]);

  useEffect(() => {
    return () => {
      modBoxRef.current?.remove();
      origBoxRef.current?.remove();
      modBoxRef.current = null;
      origBoxRef.current = null;
    };
  }, []);

  /** 高亮 Monaco DiffEditor 中指定序号的差异块，并使视野对齐 */
  const highlightMonacoHunkByIndex = useCallback((idx: number, scrollIntoView = true) => {
    const ed = diffEditorRef.current;
    if (!ed) return;
    const changes = ed.getLineChanges();
    if (!changes || changes.length === 0) return;
    const clamped = Math.max(0, Math.min(idx, changes.length - 1));
    const change = changes[clamped];
    if (!change) return;

    const origEd = ed.getOriginalEditor();
    const modEd = ed.getModifiedEditor();

    // 仅在右侧标尺（Overview Ruler）保留位置指示，不在代码行内绘制分割网格
    const modDecorations: any[] = [{
      range: {
        startLineNumber: change.modifiedStartLineNumber || 1,
        startColumn: 1,
        endLineNumber: change.modifiedEndLineNumber || change.modifiedStartLineNumber || 1,
        endColumn: 1
      },
      options: {
        overviewRuler: {
          color: '#3574f0',
          position: 7
        }
      }
    }];

    const origDecorations: any[] = [{
      range: {
        startLineNumber: change.originalStartLineNumber || 1,
        startColumn: 1,
        endLineNumber: change.originalEndLineNumber || change.originalStartLineNumber || 1,
        endColumn: 1
      },
      options: {
        overviewRuler: {
          color: '#3574f0',
          position: 7
        }
      }
    }];

    try {
      modDecorationsRef.current = modEd.deltaDecorations(modDecorationsRef.current, modDecorations);
      origDecorationsRef.current = origEd.deltaDecorations(origDecorationsRef.current, origDecorations);
    } catch {}

    currentHunkRef.current = clamped;
    setCurrentHunk(clamped);
    updateHunkBox(clamped);

    if (scrollIntoView) {
      if (mode === 'unified') {
        const targetLine = change.modifiedStartLineNumber > 1 ? change.modifiedStartLineNumber - 1 : change.modifiedStartLineNumber;
        modEd.revealLineInCenter(targetLine);
      } else {
        if (change.modifiedEndLineNumber > 0) {
          modEd.revealLineInCenter(change.modifiedStartLineNumber);
        } else {
          origEd.revealLineInCenter(change.originalStartLineNumber);
        }
      }
    }
  }, [mode, updateHunkBox]);
  highlightHunkRef.current = highlightMonacoHunkByIndex;

  // diff 或路径变更时清空 monaco 高亮装饰与外框
  useEffect(() => {
    const ed = diffEditorRef.current;
    if (ed) {
      try {
        modDecorationsRef.current = ed.getModifiedEditor().deltaDecorations(modDecorationsRef.current, []);
        origDecorationsRef.current = ed.getOriginalEditor().deltaDecorations(origDecorationsRef.current, []);
      } catch {}
    }
    if (modBoxRef.current) modBoxRef.current.style.display = 'none';
    if (origBoxRef.current) origBoxRef.current.style.display = 'none';
  }, [diff, filePath]);

  // 布局变化与水平滚动时重新测量当前差异外框尺寸与位置（保持右侧完全封闭可见）
  useEffect(() => {
    const ed = diffEditorRef.current;
    if (!ed || !useMonaco) return;
    const mod = ed.getModifiedEditor();
    const orig = ed.getOriginalEditor();
    const subLayout = mod.onDidLayoutChange(() => {
      updateHunkBox(currentHunk);
    });
    const subModScroll = mod.onDidScrollChange(e => {
      if (modBoxRef.current && modBoxRef.current.style.display !== 'none') {
        modBoxRef.current.style.left = `${e.scrollLeft}px`;
      }
    });
    const subOrigScroll = orig.onDidScrollChange(e => {
      if (origBoxRef.current && origBoxRef.current.style.display !== 'none') {
        origBoxRef.current.style.left = `${e.scrollLeft}px`;
      }
    });
    return () => {
      subLayout.dispose();
      subModScroll.dispose();
      subOrigScroll.dispose();
    };
  }, [useMonaco, currentHunk, updateHunkBox]);

  /** Monaco：按当前 hunk 下标直接跳到下/上一处差异，返回是否仍停在本文件内。
   *  不再依赖 goToDiff 后的光标位置反推下标：纯删除 hunk、光标在边界等情况
   *  会让反推失配，出现「编辑器跳了但 2/8 数字没变」。 */
  const monacoJump = (dir: 1 | -1): boolean => {
    const ed = diffEditorRef.current;
    if (!ed) return true;
    const changes = ed.getLineChanges() || [];
    if (changes.length === 0) return false;
    const current = Math.max(0, Math.min(currentHunkRef.current, changes.length - 1));
    const target = current + dir;
    if (target < 0 || target >= changes.length) return false;
    highlightMonacoHunkByIndex(target, true);
    return true;
  };

  /** 按行渲染：向 dir 移一格且不回绕；false = 已在首/末处差异 */
  const legacyJump = (dir: 1 | -1): boolean => {
    if (hunkOffsets.length === 0) return false;
    const next = currentHunk + dir;
    if (next < 0 || next >= hunkOffsets.length) return false;
    currentHunkRef.current = next;
    setCurrentHunk(next);
    // 用户已经用工具栏移动过：Monaco 稍后就绪时沿用这个下标，不要重置成首块。
    revealedRef.current = true;
    scrollToLineIndex(hunkOffsets[next]);
    return true;
  };

  /** 当前文件在跨文件清单中的下标；清单/当前路径可能是相对或绝对，统一按绝对路径比较。 */
  const currentFileIndex = (): number => {
    const current = toAbsPath(filePath || '', repoPath);
    if (!current) return -1;
    return fileList.findIndex(p => toAbsPath(p, repoPath) === current);
  };

  /** 切换到 fileList 中当前文件的上/下一个；返回是否成功 */
  const navigateFile = (dir: 1 | -1): boolean => {
    if (!onNavigateFile || fileList.length === 0) return false;
    const idx = currentFileIndex();
    const nextIdx = dir === 1 ? (idx < 0 ? 0 : idx + 1) : idx < 0 ? fileList.length - 1 : idx - 1;
    if (nextIdx < 0 || nextIdx >= fileList.length) return false;
    // 手动跨文件跳转时，上一条「再点一次切文件」的边界状态必须失效，
    // 否则新文件的第一次 F7 会直接再跳一个文件。
    edgePendingRef.current = 0;
    setEdgeHint(null);
    if (edgeHintTimerRef.current != null) {
      window.clearTimeout(edgeHintTimerRef.current);
      edgeHintTimerRef.current = null;
    }
    onNavigateFile(fileList[nextIdx]);
    return true;
  };

  const clearEdgeHint = () => {
    edgePendingRef.current = 0;
    setEdgeHint(null);
    if (edgeHintTimerRef.current != null) {
      window.clearTimeout(edgeHintTimerRef.current);
      edgeHintTimerRef.current = null;
    }
  };

  const jumpHunk = (dir: 1 | -1) => {
    // 已在边界提示过 → 再点同方向 = 切文件（必须在 clearEdgeHint 之前判断）
    if (edgePendingRef.current === dir) {
      clearEdgeHint();
      if (navigateFile(dir)) return;
      setEdgeHint(dir === 1 ? t().diff.edgeLastFile : t().diff.edgeFirstFile);
      edgeHintTimerRef.current = window.setTimeout(() => setEdgeHint(null), 3000);
      return;
    }

    // 方向变了或正常跳转：清掉旧提示
    clearEdgeHint();
    const moved = useMonaco ? monacoJump(dir) : legacyJump(dir);
    if (moved) return;

    // 首/末处差异：先提示，再点一次切文件
    const idx = currentFileIndex();
    // idx < 0 = 整个提交/未知文件：向后可进第一文件，向前可进最后一文件
    const hasNextFile =
      fileList.length > 0 && (idx < 0 || (dir === 1 ? idx < fileList.length - 1 : idx > 0));
    if (onNavigateFile && hasNextFile) {
      edgePendingRef.current = dir;
      setEdgeHint(
        dir === 1
          ? t().diff.edgeNextFile
          : t().diff.edgePrevFile
      );
      edgeHintTimerRef.current = window.setTimeout(clearEdgeHint, 4000);
    } else {
      setEdgeHint(
        dir === 1
          ? fileList.length > 1
            ? t().diff.edgeLastFileLast
            : t().diff.edgeLast
          : fileList.length > 1
            ? t().diff.edgeFirstFileFirst
            : t().diff.edgeFirst
      );
      edgeHintTimerRef.current = window.setTimeout(clearEdgeHint, 3000);
    }
  };
  // 全局 F7 / Shift+F7 差异跳转监听；⌘⇧←/→ 跨文件跳转（对齐 PyCharm Diff）。
  const jumpHunkRef = useRef(jumpHunk);
  jumpHunkRef.current = jumpHunk;
  const navigateFileRef = useRef(navigateFile);
  navigateFileRef.current = navigateFile;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'F7') {
        e.preventDefault();
        jumpHunkRef.current(e.shiftKey ? -1 : 1);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
        e.preventDefault();
        navigateFileRef.current(e.key === 'ArrowRight' ? 1 : -1);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // 监听 Monaco 光标位置，点击差异行时自动高亮对应差异块
  useEffect(() => {
    const ed = diffEditorRef.current;
    if (!ed || !useMonaco) return;
    const mod = ed.getModifiedEditor();
    const sub = mod.onDidChangeCursorPosition(e => {
      const line = e.position.lineNumber;
      const changes = ed.getLineChanges() || [];
      for (let i = 0; i < changes.length; i++) {
        const c = changes[i];
        if (c.modifiedEndLineNumber > 0 && line >= c.modifiedStartLineNumber && line <= c.modifiedEndLineNumber) {
          highlightMonacoHunkByIndex(i, false);
          break;
        }
      }
    });
    return () => sub.dispose();
  }, [useMonaco, highlightMonacoHunkByIndex]);


  // 打开文件 / 切换视图后自动定位到第一处差异（Monaco 与按行渲染两条路径）
  // 打开文件或切换模式后，等待 Monaco 算完 diff 自动定位到首处差异
  useEffect(() => {
    if (!readyForMonaco || !contents) return;
    const ed = diffEditorRef.current;
    if (!ed) return;
    const tryReveal = () => {
      if (disposedRef.current || !diffPaneLiveRef.current) return;
      const changes = ed.getLineChanges();
      if (changes && changes.length > 0) {
        setMonacoHunkCount(changes.length);
        const idx = Math.min(currentHunkRef.current, changes.length - 1);
        highlightHunkRef.current(idx, true);
        if (idx === 0) revealFirstDiffNow(ed);
        revealedRef.current = true;
      }
    };
    const t1 = window.setTimeout(tryReveal, 50);
    const t2 = window.setTimeout(tryReveal, 150);
    const t3 = window.setTimeout(tryReveal, 350);
    return () => {
      window.clearTimeout(t1);
      window.clearTimeout(t2);
      window.clearTimeout(t3);
    };
  }, [readyForMonaco, contents, mode, refreshKey, highlightMonacoHunkByIndex, revealFirstDiffNow]);

  // 新的 diff 到来时 hunk 下标必须归零：Monaco 路径同样使用这个下标做
  // 工具栏的逐块 Stage/Unstage，越界会让操作落到不存在的块上。
  // 只在这里归零；onDidUpdateDiff 不能把用户刚点到的 2/8 又重置回 1/8。
  // layout effect：换文件后的第一帧之前清掉旧文件的 hunk 数，避免闪一下旧值。
  useLayoutEffect(() => {
    currentHunkRef.current = 0;
    revealedRef.current = false;
    setCurrentHunk(0);
    setMonacoHunkCount(0);
  }, [filePath, staged, refreshKey, diff]);

  useEffect(() => {
    if (readyForMonaco || !diff || hunkOffsets.length === 0) return;
    const t = window.setTimeout(() => scrollToLineIndex(hunkOffsets[0]), 0);
    return () => window.clearTimeout(t);
  }, [readyForMonaco, diff, hunkOffsets, mode, refreshKey]);
  /** unified 展示模型：兜底合并内容完全相同的相邻 -/+ 对。
      正常路径下 Rust 已把 EOF 换行差异归整为「content + 空白尾行增删」，
      这里只防御异常 patch（例如外部工具产出的 diff）。 */
  const unifiedRows = useMemo<UnifiedRow[]>(() => {
    const rows: UnifiedRow[] = [];
    for (let k = 0; k < visibleLines.length; k++) {
      const cur = visibleLines[k];
      const next = visibleLines[k + 1];
      // 合并条件：相邻 del/add 内容相同，且 add 不带 EOF 折叠语义
      // （eofNoNewline 说明旧文件尾部还有结构，按 PyCharm 形态逐行展示更直观）
      if (
        cur.l.kind === 'del' &&
        next?.l.kind === 'add' &&
        next.i === cur.i + 1 &&
        cur.l.text === next.l.text &&
        !next.l.eofNoNewline
      ) {
        rows.push({
          l: { ...cur.l, newLine: next.l.newLine, noNewline: cur.l.noNewline || next.l.noNewline },
          i: cur.i,
          mergedWith: next.l
        });
        k++;
        continue;
      }
      rows.push(cur);
    }
    return rows;
  }, [visibleLines]);

  const renderMergedRow = (row: UnifiedRow) => {
    const noNl = !!row.l.noNewline;
    const hunkStart = hunkOffsets[currentHunk] ?? -1;
    const nextHunkStart = hunkOffsets[currentHunk + 1] ?? Infinity;
    const inCurrentHunk = hunkStart >= 0 && row.i >= hunkStart && row.i < nextHunkStart;
    const isFirst = row.i === hunkStart;
    const isLast = row.i === nextHunkStart - 1 || row.i === visibleLines.length - 1;

    return (
      <div
        key={`m-${row.i}`}
        data-line={row.i}
        title={noNl ? (row.l.eofNoNewline ? t().diff.mergedEofTitle : t().diff.mergedNewlineTitle) : undefined}
        style={{
          display: 'flex',
          height: '19px',
          lineHeight: '19px',
          background: 'rgba(56,139,253,0.10)',
          borderLeft: inCurrentHunk ? '1px solid var(--accent)' : '3px solid #388bfd',
          borderRight: inCurrentHunk ? '1px solid var(--accent)' : undefined,
          borderTop: inCurrentHunk && isFirst ? '1px solid var(--accent)' : undefined,
          borderBottom: inCurrentHunk && isLast ? '1px solid var(--accent)' : undefined,
          borderRadius: 0
        }}
      >
        <span style={GUTTER}>{row.l.oldLine ?? ''}</span>
        <span style={GUTTER}>{row.l.newLine ?? ''}</span>
        <span style={{ flex: 1, whiteSpace: 'pre', paddingLeft: '6px' }}>
          ~{row.l.text}
          {noNl && (
            <span style={{ color: 'var(--text-muted)', fontStyle: 'italic', marginLeft: '8px' }}>
              {row.l.eofNoNewline ? t().diff.eofTailNote : t().diff.newlineOnlyNote}
            </span>
          )}
        </span>
      </div>
    );
  };

  const renderRow = (line: DiffLine, index: number, keyPrefix: string) => {
    const hunkStart = hunkOffsets[currentHunk] ?? -1;
    const nextHunkStart = hunkOffsets[currentHunk + 1] ?? Infinity;
    const inCurrentHunk = hunkStart >= 0 && index >= hunkStart && index < nextHunkStart;
    const isFirst = index === hunkStart;
    const isLast = index === nextHunkStart - 1 || index === visibleLines.length - 1;

    const baseStyle =
      line.kind === 'add'
        ? ADD_STYLE
        : line.kind === 'del'
          ? DEL_STYLE
          : line.kind === 'hunk'
            ? HUNK_STYLE
            : line.kind === 'meta'
              ? META_STYLE
              : CONTEXT_STYLE;

    const currentHunkStyle: React.CSSProperties = inCurrentHunk
      ? {
          borderLeft: '1px solid var(--accent)',
          borderRight: '1px solid var(--accent)',
          borderTop: isFirst ? '1px solid var(--accent)' : undefined,
          borderBottom: isLast ? '1px solid var(--accent)' : undefined,
          borderRadius: 0,
          fontWeight: line.kind === 'hunk' ? 600 : undefined
        }
      : {};

    return (
      <div
        key={`${keyPrefix}-${index}`}
        data-line={index}
        style={{
          display: 'flex',
          height: '19px',
          lineHeight: '19px',
          ...baseStyle,
          ...currentHunkStyle
        }}
      >
        <span style={GUTTER}>{line.oldLine ?? ''}</span>
        <span style={GUTTER}>{line.newLine ?? ''}</span>
        <span style={{ flex: 1, whiteSpace: 'pre', paddingLeft: '6px' }}>
          {line.kind === 'add' ? '+' : line.kind === 'del' ? '-' : ' '}
          {line.text}
          {(line.noNewline || line.eofNoNewline) && (
            <span
              style={{ color: 'var(--text-muted)', marginLeft: '6px' }}
              title={
                line.eofNoNewline
                  ? line.kind === 'del'
                    ? t().diff.delEofTitle
                    : t().diff.newEofTitle
                  : t().diff.lineNoNewlineTitle
              }
            >
              ⤶
            </span>
          )}
          {line.kind === 'hunk' && canHunkAction && (
            <span style={{ display: 'inline-flex', gap: 4, marginLeft: 10, verticalAlign: 'middle' }}>
              {staged ? (
                <button
                  className="diff-hunk-btn"
                  disabled={hunkBusy}
                  onClick={e => { e.stopPropagation(); void applyHunk(index, 'unstage'); }}
                >
                  {t().diff.unstageHunk}
                </button>
              ) : (
                <>
                  <button
                    className="diff-hunk-btn"
                    disabled={hunkBusy}
                    onClick={e => { e.stopPropagation(); void applyHunk(index, 'stage'); }}
                  >
                    {t().diff.stageHunk}
                  </button>
                  <button
                    className="diff-hunk-btn"
                    disabled={hunkBusy}
                    onClick={e => { e.stopPropagation(); void applyHunk(index, 'rollback'); }}
                  >
                    {t().diff.rollbackHunk}
                  </button>
                </>
              )}
            </span>
          )}
        </span>
      </div>
    );
  };

  const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;
  // 单文件 Monaco 视图在 diff 算完前没有可靠的 hunk 数：先不要用 git patch 的
  // hunk 数占位，否则会先闪一个 33，再跳到 Monaco 的真实值 52。
  // 多文件/二进制/无 Monaco 的降级视图才直接使用 patch hunk 数。
  const monacoHunksReady = useMonaco && monacoHunkCount > 0;
  const totalHunks = isBinaryDiff
    ? 0
    : readyForMonaco
      ? (monacoHunksReady ? monacoHunkCount : 0)
      : hunkOffsets.length;
  const displayHunk = totalHunks > 0 ? Math.min(Math.max(currentHunk, 0), totalHunks - 1) + 1 : 0;
  const fileIndex = currentFileIndex();
  const hasFileNavigation = !!onNavigateFile && fileList.length > 0;
  const canPrevFile = hasFileNavigation && (fileIndex < 0 || fileIndex > 0);
  const canNextFile = hasFileNavigation && (fileIndex < 0 || fileIndex < fileList.length - 1);
  const fileLabel = fsFiles
    ? `${baseName(fsFiles.oldPath)} ↔ ${baseName(fsFiles.newPath)}`
    : targetPath || showCommit || t().diff.workingTree;

  return (
    <div className="diff-view-pane" style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden', position: 'relative' }}>
      {/* 工具栏 */}
      <div className="pycharm-diff-toolbar">
        <div className="diff-tb-left">
          <button
            className="diff-icon-btn"
            data-testid="diff-prev-file"
            disabled={!canPrevFile}
            onClick={() => navigateFile(-1)}
            title={t().diff.previousFile}
          >
            <IconArrowLeft size={13} />
          </button>
          <button
            className="diff-icon-btn"
            data-testid="diff-next-file"
            disabled={!canNextFile}
            onClick={() => navigateFile(1)}
            title={t().diff.nextFile}
          >
            <IconArrowRight size={13} />
          </button>
          <span className="diff-toolbar-sep" />
          <button
            className="diff-icon-btn"
            data-testid="diff-prev-hunk"
            disabled={isBinaryDiff || totalHunks === 0}
            onClick={() => jumpHunk(-1)}
            title={t().diff.previousHunk}
            style={{ opacity: isBinaryDiff || totalHunks === 0 ? 0.35 : undefined, cursor: isBinaryDiff || totalHunks === 0 ? 'default' : undefined }}
          >
            <IconChevronUp size={13} />
          </button>
          <button
            className="diff-icon-btn"
            data-testid="diff-next-hunk"
            disabled={isBinaryDiff || totalHunks === 0}
            onClick={() => jumpHunk(1)}
            title={t().diff.nextHunk}
            style={{ opacity: isBinaryDiff || totalHunks === 0 ? 0.35 : undefined, cursor: isBinaryDiff || totalHunks === 0 ? 'default' : undefined }}
          >
            <IconChevronDown size={13} />
          </button>
          {!isBinaryDiff && totalHunks > 0 && (
            <span style={{ display: 'inline-flex', gap: '4px', alignItems: 'center', flexShrink: 0 }}>
              {/* 进度只读：hunk 跳转由上下箭头完成，这里不再提供下拉选择 */}
              <span
                className="diff-hunk-counter"
                data-testid="diff-hunk-counter"
                title={t().diff.stats(diff?.additions ?? 0, diff?.deletions ?? 0, totalHunks)}
              >
                {displayHunk} / {totalHunks}
              </span>
              {canHunkAction && hunkOffsets.length > 0 && (
                staged ? (
                  <button className="diff-hunk-btn" disabled={hunkBusy} onClick={() => void applyHunk(currentHunk, 'unstage')}>
                    {t().diff.unstageHunk}
                  </button>
                ) : (
                  <>
                    <button className="diff-hunk-btn" disabled={hunkBusy} onClick={() => void applyHunk(currentHunk, 'stage')}>
                      {t().diff.stageHunk}
                    </button>
                    <button className="diff-hunk-btn" disabled={hunkBusy} onClick={() => void applyHunk(currentHunk, 'rollback')}>
                      {t().diff.rollbackHunk}
                    </button>
                  </>
                )
              )}
            </span>
          )}
          {!isBinaryDiff && (
            <div className="diff-select-wrap" style={{ flexShrink: 0 }}>
              <select
                className="diff-select"
                value={mode}
                onChange={e => {
                  const next = e.target.value as DiffViewMode;
                  setMode(next);
                  rememberDiffViewMode(next);
                }}
              >
                <option value="unified">{t().diff.unifiedView}</option>
                <option value="side-by-side">{t().diff.sideBySideView}</option>
              </select>
            </div>
          )}
          {!isBinaryDiff && (
            <span className="diff-stat-text" style={{ marginLeft: '8px' }}>
              {loading
                ? t().common.loading
                : diff
                  ? t().diff.stats(diff.additions, diff.deletions, hunkOffsets.length)
                  : ''}
            </span>
          )}
        </div>
        <div className="diff-tb-right">
          {!showCommit && !againstRef && !compareCommits && !selectedCommits && !fsFiles && onStagedChange && (
            <button
              className="diff-hunk-btn"
              style={{ marginRight: '8px' }}
              onClick={() => onStagedChange(!staged)}
              title={staged ? t().diff.viewUnstaged : t().diff.viewStaged}
            >
              {staged ? t().diff.viewUnstaged : t().diff.viewStaged}
            </button>
          )}
          <span className="diff-stat-text">
            {selectedCommits
              ? t().diff.cmpCommits(selectedCommits.length)
              : againstRef
              ? t().diff.cmpWorktree(againstRef.slice(0, 8))
              : compareCommits
                ? t().diff.cmpCommitRange(compareCommits.from.slice(0, 8), compareCommits.to.slice(0, 8))
                : showCommit
                  ? t().diff.cmpCommit(showCommit.slice(0, 8))
                  : staged
                    ? t().diff.cmpStagedHead
                    : t().diff.cmpWorktreeIndex}
          </span>
        </div>
      </div>
      {/* 边界提示浮动气泡（PyCharm Balloon 规范：悬浮在 ▲/▼ 按钮下方，0 侵入工具栏） */}
      {edgeHint && (
        <div className="diff-edge-balloon" role="status">
          <span className="diff-edge-balloon-text">{edgeHint}</span>
        </div>
      )}

      {/* 文件名 */}
      <div className="diff-pane-titlebar" style={{ fontFamily: 'var(--font-code)' }}>
        <span className="diff-commit-hash">
          {selectedCommits ? t().diff.cmpCommits(selectedCommits.length) : showCommit ? showCommit.slice(0, 8) : 'diff'}
        </span>
        <span className="diff-file-target">{fileLabel}</span>
        {diff?.isNew && <span className="git-tag git-staged">{t().diff.addedTag}</span>}
        {diff?.isDeleted && <span className="git-tag git-del">{t().diff.deletedTag}</span>}
        {diff?.truncated && (
          <span className="git-tag" style={{ background: 'rgba(187,128,9,0.15)', color: 'var(--text-muted)' }} title={t().diff.truncatedHint}>
            {t().diff.truncatedTag}
          </span>
        )}
      </div>

      {/* 内容 */}
      {isBinaryDiff ? (
        <div className="diff-unknown-file-type">
          {t().diff.binaryNoText}
        </div>
      ) : readyForMonaco ? (
        <div
          style={{
            flex: 1,
            minHeight: 0,
            display: 'flex',
            flexDirection: 'column',
            overflow: 'hidden',
            background: 'var(--bg-app)'
          }}
        >
          {useMonaco && contents ? (
            <React.Suspense fallback={<MonacoBusy label={t().diff.loadingEditor} />}>
              <DiffEditorPane
                original={contents.original}
                modified={contents.modified}
                originalLanguage={languageOf(fsFiles?.oldPath || targetPath)}
                modifiedLanguage={languageOf(targetPath)}
                theme={MONACO_THEME_BY_APP_THEME[resolvedThemeId] ?? 'ide-jetbrains-dark'}
                loading={<MonacoBusy label={t().diff.loadingDiff} />}
                options={diffOptions}
                onMount={ed => {
                  diffEditorRef.current = ed;
                  const updateHunks = () => {
                    if (disposedRef.current || !diffPaneLiveRef.current) return;
                    const changes = ed.getLineChanges();
                    if (changes && changes.length > 0) {
                      setMonacoHunkCount(changes.length);
                      const idx = Math.min(currentHunkRef.current, changes.length - 1);
                      if (!revealedRef.current) {
                        currentHunkRef.current = 0;
                        setCurrentHunk(0);
                        highlightHunkRef.current(0, true);
                        revealFirstDiffNow(ed);
                        revealedRef.current = true;
                      } else {
                        // 已定位过：保留用户当前进度，只重画当前块的外框。
                        currentHunkRef.current = idx;
                        setCurrentHunk(idx);
                        highlightHunkRef.current(idx, false);
                      }
                    } else {
                      setMonacoHunkCount(0);
                      if (modBoxRef.current) modBoxRef.current.style.display = 'none';
                      if (origBoxRef.current) origBoxRef.current.style.display = 'none';
                    }
                  };
                  ed.onDidUpdateDiff(updateHunks);
                  updateHunks();
                  window.setTimeout(updateHunks, 80);
                  window.setTimeout(updateHunks, 250);
                }}
              />
            </React.Suspense>
          ) : (
            <MonacoBusy label={monacoReady ? t().diff.loadingDiff : t().diff.loadingEditor} />
          )}
        </div>
      ) : (
      <div
        ref={scrollRef}
        onScroll={e => {
          const el = e.currentTarget;
          setScrollTop(el.scrollTop);
          setViewportH(el.clientHeight || 600);
        }}
        style={{
          flex: 1,
          minHeight: 0,
          overflow: 'auto',
      fontFamily: 'JetBrains Mono, Menlo, Monaco, "Courier New", monospace',
          fontSize: '12px',
          lineHeight: '19px',
          background: 'var(--bg-app)'
        }}
      >
        {error && (
          <div style={{ padding: '14px', color: 'var(--git-del)', fontSize: '12px', whiteSpace: 'pre-wrap' }}>
            {t().diff.readFailed(error)}
          </div>
        )}

        {!error && !loading && diff && visibleLines.length === 0 && (
          <div style={{ padding: '14px', color: 'var(--text-muted)', fontSize: '12px' }}>
            {(showCommit || compareCommits || selectedCommits)
              ? t().diff.noTextDiffInCommit
              : diff.isBinary
                ? t().diff.binaryNoText
                : staged
                  ? t().diff.noDiffStagedHead
                  : t().diff.noUncommittedChanges}
          </div>
        )}

        {!error &&
          diff &&
          !diff.isBinary &&
          visibleLines.length > 0 &&
          (mode === 'unified' ? (
            <DiffRows rows={unifiedRows} scrollTop={scrollTop} viewportH={viewportH} renderRow={renderRow} renderMerged={renderMergedRow} />
          ) : (
            // 并排：左列只显示 context/del，右列只显示 context/add，按行号对齐
            <div style={{ display: 'flex' }}>
              <div style={{ flex: 1, borderRight: '1px solid var(--border-color)', minWidth: 0 }}>
                <DiffRows rows={visibleLines} scrollTop={scrollTop} viewportH={viewportH} renderRow={renderRow} renderMerged={renderMergedRow} side="l" />
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <DiffRows rows={visibleLines} scrollTop={scrollTop} viewportH={viewportH} renderRow={renderRow} renderMerged={renderMergedRow} side="r" />
              </div>
            </div>
          ))}
      </div>
      )}
    </div>
  );
};

interface DiffRowsProps {
  rows: UnifiedRow[];
  scrollTop: number;
  viewportH: number;
  renderRow: (line: DiffLine, index: number, keyPrefix: string) => React.ReactNode;
  renderMerged: (row: UnifiedRow) => React.ReactNode;
  /** 并排模式某一列；不传 = unified */
  side?: 'l' | 'r';
}

/** 虚拟化行窗口：大 diff（数千行）只挂载视口附近的行，其余用等高占位撑开滚动条 */
function DiffRows({ rows, scrollTop, viewportH, renderRow, renderMerged, side }: DiffRowsProps) {
  const total = rows.length;
  const start = Math.max(0, Math.floor(scrollTop / DIFF_ROW_H) - DIFF_OVERSCAN);
  const end = Math.min(total, Math.ceil((scrollTop + (viewportH || 600)) / DIFF_ROW_H) + DIFF_OVERSCAN);
  const slice = rows.slice(start, end);
  const rendered =
    side === undefined
      ? slice.map(r => (r.mergedWith ? renderMerged(r) : renderRow(r.l, r.i, 'u')))
      : slice.map(r =>
          r.l.kind === (side === 'l' ? 'add' : 'del') ? (
            <div key={`${side}-${r.i}`} style={{ height: `${DIFF_ROW_H}px`, background: 'var(--overlay-faint)' }} />
          ) : (
            renderRow(r.l, r.i, side)
          )
        );
  return (
    <>
      {start > 0 && <div style={{ height: `${start * DIFF_ROW_H}px` }} />}
      {rendered}
      {end < total && <div style={{ height: `${(total - end) * DIFF_ROW_H}px` }} />}
    </>
  );
}
