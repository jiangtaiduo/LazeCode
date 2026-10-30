// 三路合并工具（Merge Revisions）· 对齐 PyCharm 逐块合并
//
// PyCharm 的语义模型（本机截图逐像素实测）：
//   - 一边删、一边改（DU/UD）= **不是冲突**，是若干"更改"但**默认待应用**：
//     Result 初始 = base（实测只有 base 的 4 行，theirs 新增 2 行留在右栏
//     绿色 + ≪ ✕ 标记），状态行 "N changes. No conflicts."，
//     "Apply non-conflicting changes" 按钮 / Apply 前补吃入。
//   - 两边都改同一块 = 冲突（红块），必须逐块拍板。
//   - 中间格没有 `<<<<<<<` 行：待应用态 = 删除侧淡紫斜切带 #343c46（盖到
//     最后一根非空行，指向空侧）+ 块尾 2px 绿插入线；已应用态 = context 淡紫
//     带 + 首条非空 delta 行 2px 绿顶线（#2c4134）。侧栏：该侧 vs base 的
//     add/mod 行染色（changed 空行也染），gutter 自绘镜像（左：✕ ≫ 行号；
//     右：行号 ≪ ✕）。
//
// 本对话框结构：
//   - buildMergeView 初建无标记视图（冲突块初始 ours，空则 theirs；非冲突块初始 base）。
//   - blocks[] 维护视图坐标 + choice；用户敲字走 diffRegion 前后缀对位
//     （只调坐标不动 choice），程序 splice（≫/≪）同步调坐标，互不穿越。
//   - blocksRef 同步镜：gutter 点击与 mousedown 不经过 render 闭包，
//     连点同一块时第二下看到已解决，防 double splice。
//   - 手改不算解决：Apply 只卡未决 *冲突*；块内改完点 ✕ = 以当前文本解决；
//     把块删空 = 显式删（删改冲突全取空边 → Apply 走 git rm，不留空文件）。
//   - 空白忽略（wsMode）进 alignAdded/diffRegion/sideChanges；高亮模式
//     （hlMode）只实现 Do not highlight 关染色，其余档位暂等同行级（audit 注明）。

import React, { useState, useEffect, useMemo, useRef } from 'react';
import { IconWindowClose } from './FileIcons';
import Editor, { type OnMount } from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import { invokeCommand } from '../services/tauriBridge';
import { toWorkspaceAbsPath } from '../services/pathUtils';
import { t } from '../services/i18n';
import { UNICODE_HIGHLIGHT_OPTIONS, ensureMonacoReady } from '../services/monacoSetup';
import { languageOf } from '../services/fileTypes';
import type { MergeFileContent, MergeMarked, ConflictKind } from '../types';
import {
  buildMergeView,
  diffRegion,
  alignAdded,
  locateBlocks,
  sideChanges,
  type WsMode
} from '../services/mergeHunks';

type BlockChoice = 'none' | 'ours' | 'theirs' | 'keep';
type HlMode = 'words' | 'lines' | 'split' | 'chars' | 'none';

interface Block {
  /** 视图坐标（0 起，end 不含），随敲字/splice 实时维护 */
  start: number;
  end: number;
  ours: string[];
  theirs: string[];
  base: string[];
  choice: BlockChoice;
}

const sameArr = (a: string[], b: string[]) => a.length === b.length && a.every((l, i) => l === b[i]);

/**
 * 冲突判定（对齐 PyCharm 实测与 Git 三路合并规范）：
 * 1. 双方内容不同且都存在；
 * 2. 或者一方删除（空）而另一方有变更（删改冲突）；
 * 3. 只有当某一方与 base 完全相同（即未变动，纯单侧改动）时，才属于可自动合入的非冲突更改。
 */
const isConflict = (b: Block) => {
  const oursDiff = !sameArr(b.ours, b.base);
  const theirsDiff = !sameArr(b.theirs, b.base);
  return (oursDiff && theirsDiff) || (b.ours.length > 0 && b.theirs.length > 0 && !sameArr(b.ours, b.theirs));
};
const visible = (b: Block) => b.start < b.end;

/** 词级/字符级差异范围提取（1 起列号，供 Monaco 词级方块高亮） */
function findLineDiffRange(baseLine: string | undefined, changedLine: string | undefined): { startCol: number; endCol: number } | null {
  if (!baseLine || !changedLine || baseLine === changedLine) return null;
  let start = 0;
  while (start < baseLine.length && start < changedLine.length && baseLine[start] === changedLine[start]) {
    start++;
  }
  let endBase = baseLine.length - 1;
  let endChanged = changedLine.length - 1;
  while (endBase >= start && endChanged >= start && baseLine[endBase] === changedLine[endChanged]) {
    endBase--;
    endChanged--;
  }
  if (endChanged >= start) {
    return { startCol: start + 1, endCol: endChanged + 2 };
  }
  return null;
}
/** 块内容在侧栏文件里的位置（高亮用；找不到就不染，宁缺毋滥）。 */
type BlockPos = { ours: [number, number] | null; theirs: [number, number] | null };

/** 斜切方向 = 指向空侧（实测：ours 空 → 左缘斜切，theirs 空 → 右缘镜像）。 */
type WedgeDir = 'left' | 'right';
const wedgeDirOf = (b: Block): WedgeDir => (b.ours.length === 0 ? 'left' : 'right');

const WS_OPTIONS: Array<{ id: WsMode; pick: () => string }> = [
  { id: 'none', pick: () => t().conflict.mergeTool.wsNone },
  { id: 'trim', pick: () => t().conflict.mergeTool.wsTrim },
  { id: 'space', pick: () => t().conflict.mergeTool.wsSpace },
  { id: 'space-empty', pick: () => t().conflict.mergeTool.wsSpaceEmpty },
  { id: 'imports', pick: () => t().conflict.mergeTool.wsImports }
];
const HL_OPTIONS: Array<{ id: HlMode; pick: () => string }> = [
  { id: 'words', pick: () => t().conflict.mergeTool.hlWords },
  { id: 'lines', pick: () => t().conflict.mergeTool.hlLines },
  { id: 'split', pick: () => t().conflict.mergeTool.hlSplit },
  { id: 'chars', pick: () => t().conflict.mergeTool.hlChars },
  { id: 'none', pick: () => t().conflict.mergeTool.hlNone }
];

/** ↰▾：应用所有不冲突更改（PyCharm 工具栏的主按钮） */
const IconApplyArrow: React.FC = () => (
  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
    <path
      d="M1 6h6M5.2 3.2 8 6 5.2 8.8"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
    <path d="M10.2 2.2v7.6" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
  </svg>
);

/** 只读窗格的锁图标（本地/远端不可编辑；结果格不加） */
const LockGlyph: React.FC = () => (
  <svg className="pane-lock" width="9" height="10" viewBox="0 0 9 10" aria-hidden="true">
    <path d="M2 4.2V3a2.5 2.5 0 0 1 5 0v1.2" fill="none" stroke="currentColor" strokeWidth="1.2" />
    <rect x="1" y="4.2" width="7" height="5.2" rx="1" fill="currentColor" />
  </svg>
);

/** ⚙：文本字形在系统字体里回退成实心圆点（用户截图实证），改用内联齿轮 */
const IconGear: React.FC = () => (
  <svg width="13" height="13" viewBox="0 0 16 16" aria-hidden="true">
    <circle cx="8" cy="8" r="2.4" fill="none" stroke="currentColor" strokeWidth="1.3" />
    <path
      d="M8 1.8v1.7M8 12.5v1.7M14.2 8h-1.7M3.5 8H1.8M12.4 3.6l-1.2 1.2M4.8 11.2l-1.2 1.2M12.4 12.4l-1.2-1.2M4.8 4.8 3.6 3.6"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
    />
  </svg>
);

interface SideBand {
  /** 0 起，end 不含 */
  start: number;
  end: number;
  add: boolean;
  conflict?: boolean;
  isDeleted?: boolean;
  dir: WedgeDir;
}

interface SideEditorProps {
  side: 'ours' | 'theirs';
  value: string;
  theme: string;
  language: string;
  /** 有 gutter 标记的行（1 起） */
  marks: number[];
  /** 处于冲突块中的行号（1 起，用于标红行号与行槽） */
  conflictRows?: Set<number>;
  /** 着色带（0 起连续段）：斜切 + 底色走外层 overlay，盖住 gutter+编辑器 */
  bands: SideBand[];
  /** 词级高亮差异范围（1 起） */
  wordDiffs?: Array<{ line: number; startCol: number; endCol: number }>;
  onMark: (line1: number, action: 'apply' | 'ignore') => void;
  onReady?: (ed: Monaco.editor.IStandaloneCodeEditor) => void;
}
/**
 * 侧栏（本地/远端）：镜像 gutter 自绘 —— 左窗格 [✕][≫][行号]，右窗格
 * [行号][≪][✕]（箭头永远贴行号侧、✕ 在外侧，对齐 PyCharm 实测）。
 * 行号/标记的纵向位置用 getTopForLineNumber 实测，滚动经 transform 同步；
 * 着色带的斜切 overlay 挂在外层行容器（z-index:-1），把 gutter 一并盖住。
 */
const SideEditor: React.FC<SideEditorProps> = ({ side, value, theme, language, marks, conflictRows, bands, wordDiffs, onMark, onReady }) => {
  const edRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const gutRef = useRef<HTMLDivElement | null>(null);
  const ovRef = useRef<HTMLDivElement | null>(null);
  const bandsRef = useRef<SideBand[]>(bands);
  bandsRef.current = bands;
  const decoIdsRef = useRef<string[]>([]);
  const [tops, setTops] = useState<number[]>([]);

  useEffect(() => {
    const ed = edRef.current;
    if (!ed) return;
    const decos: Monaco.editor.IModelDeltaDecoration[] = (wordDiffs ?? []).map(wd => ({
      range: {
        startLineNumber: wd.line,
        startColumn: wd.startCol,
        endLineNumber: wd.line,
        endColumn: wd.endCol
      },
      options: {
        className: 'conflict-word-hl',
        inlineClassName: 'conflict-word-hl'
      }
    }));
    decoIdsRef.current = ed.deltaDecorations(decoIdsRef.current, decos);
  }, [wordDiffs]);

  const reposition = () => {
    const ed = edRef.current;
    if (!ed) return;
    const st = ed.getScrollTop();
    if (gutRef.current) gutRef.current.style.transform = `translateY(${-st}px)`;
    const cont = ovRef.current;
    if (!cont) return;
    const n = ed.getModel()?.getLineCount() ?? 0;
    const lh = tops.length > 1 ? tops[1] - tops[0] : 18;
    const kids = cont.children;
    bandsRef.current.forEach((b, i) => {
      const el = kids[i] as HTMLElement | undefined;
      if (!el) return;
      const t0 = ed.getTopForLineNumber(b.start + 1) - st;
      const bottom =
        b.end < n ? ed.getTopForLineNumber(b.end + 1) : ed.getTopForLineNumber(n) + lh;
      const h = b.isDeleted ? 2 : bottom - ed.getTopForLineNumber(b.start + 1);
      el.style.top = `${t0}px`;
      el.style.height = `${h}px`;
      if (b.isDeleted) {
        el.style.clipPath = 'none';
      } else {
        const R = Math.max(8, Math.round(h / 3));
        el.style.clipPath =
          b.dir === 'right'
            ? `polygon(0 0, calc(100% - ${R}px) 0, 100% 100%, 100% 100%)`
            : `polygon(0 0, 100% 0, 100% 100%, ${R}px 100%)`;
      }
    });
  };

  const measure = () => {
    const ed = edRef.current;
    if (!ed) return;
    const nn = ed.getModel()?.getLineCount() ?? 0;
    const t: number[] = [];
    for (let i = 1; i <= nn; i++) t.push(ed.getTopForLineNumber(i));
    setTops(t);
  };

  const handleMount: OnMount = ed => {
    edRef.current = ed;
    onReady?.(ed);
    ed.onDidChangeModelContent(() => measure());
    // 布局完成 / 字体就绪都要重量一次：首帧前 getTopForLineNumber 可能是 0，
    // 只靠 rAF+120ms 兜不住慢布局（行号会全错位）
    ed.onDidLayoutChange(() => {
      measure();
      reposition();
    });
    ed.onDidScrollChange(() => reposition());
    requestAnimationFrame(() => measure());
    setTimeout(() => measure(), 120);
    if (typeof document !== 'undefined' && document.fonts) {
      document.fonts.ready.then(() => measure()).catch(() => undefined);
    }
  };

  useEffect(() => {
    reposition();
    const id = setTimeout(() => reposition(), 60);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, bands, marks]);

  const markSet = useMemo(() => new Set(marks), [marks]);
  const lh = tops.length > 1 ? tops[1] - tops[0] : 18;
  // 行数由 value 推导、不依赖 tops 状态是否完整；tops 缺项按等行高回退 ——
  // 保证每一行都渲染出行号（布局前测量为 0/缺项时行号会整片消失，结构性兜底）
  const rowCount = Math.max(1, value.split('\n').length);
  const rowTops = Array.from({ length: rowCount }, (_, i) => tops[i] ?? i * lh);
  const markGlyph = side === 'ours' ? '≫' : '≪';

  return (
    <div className="side-editor-row">
      {side === 'theirs' && (
        <div className="side-gutter right" ref={gutRef}>
          {rowTops.map((top, i) => {
            const isConf = conflictRows?.has(i + 1);
            return (
              <div
                key={i}
                className={`side-gutter-row ${isConf ? 'conflict' : ''}`}
                style={{ top, height: i + 1 < rowTops.length ? rowTops[i + 1] - top : lh }}
              >
                <span className={`side-num ${isConf ? 'conflict' : ''}`}>{i + 1}</span>
                {markSet.has(i + 1) && (
                  <>
                    <button
                      type="button"
                      className="side-mk apply"
                      title={t().conflict.mergeTool.sideApplyTitle}
                      onClick={() => onMark(i + 1, 'apply')}
                    >
                      {markGlyph}
                    </button>
                    <button
                      type="button"
                      className="side-mk ignore"
                      title={t().conflict.mergeTool.sideIgnoreTitle}
                      onClick={() => onMark(i + 1, 'ignore')}
                    >
                      ✕
                    </button>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}
      <div className="side-editor-wrap">
        <Editor
          height="100%"
          value={value}
          theme={theme}
          language={language}
          onMount={handleMount}
          options={{
            readOnly: true,
            minimap: { enabled: false },
            fontSize: 12,
            lineNumbers: 'off',
            glyphMargin: false,
            folding: false,
            renderLineHighlight: 'none',
            scrollBeyondLastLine: false,
            automaticLayout: true,
            unicodeHighlight: UNICODE_HIGHLIGHT_OPTIONS
          }}
        />
      </div>
      {side === 'ours' && (
        <div className="side-gutter left" ref={gutRef}>
          {rowTops.map((top, i) => {
            const isConf = conflictRows?.has(i + 1);
            return (
              <div
                key={i}
                className={`side-gutter-row ${isConf ? 'conflict' : ''}`}
                style={{ top, height: i + 1 < rowTops.length ? rowTops[i + 1] - top : lh }}
              >
                {markSet.has(i + 1) && (
                  <>
                    <button
                      type="button"
                      className="side-mk ignore"
                      title={t().conflict.mergeTool.sideIgnoreTitle}
                      onClick={() => onMark(i + 1, 'ignore')}
                    >
                      ✕
                    </button>
                    <button
                      type="button"
                      className="side-mk apply"
                      title={t().conflict.mergeTool.sideApplyTitle}
                      onClick={() => onMark(i + 1, 'apply')}
                    >
                      {markGlyph}
                    </button>
                  </>
                )}
                <span className={`side-num ${isConf ? 'conflict' : ''}`}>{i + 1}</span>
              </div>
            );
          })}
        </div>
      )}
      <div className="side-wedge" ref={ovRef}>
        {bands.map((b, i) => (
          <div
            key={i}
            className={`side-wedge-band ${b.conflict ? 'conflict' : b.add ? 'add' : 'mod'} ${b.isDeleted ? 'deleted-rule' : ''}`}
          />
        ))}
      </div>
    </div>
  );
};

interface MergeToolDialogProps {
  open: boolean;
  repoPath: string;
  path: string;
  /**
   * 冲突种类：删改冲突缺的一边按空合（左/右窗格一边是空的），Apply 时结果
   * 为空表示"全取了删除边"，走 git_use_merge_side 确认删除而不是暂存空文件。
   */
  kind: ConflictKind;
  /** 右窗格标题（已带操作词：Changes from cherry-pick xxx / Changes from xxx） */
  theirsTitle: string;
  /** Show Details 内容：本地分支 / 对方作者·主题 */
  localDetails?: string;
  theirsDetails?: string;
  monacoTheme?: string;
  busy?: boolean;
  onClose: () => void;
  /** Apply 成功后：写盘 + 暂存已做，父组件走正常解决流（刷新/关框） */
  onApplied: (path: string, message: string) => void;
  isNativeWindow?: boolean;
}

export const MergeToolDialog: React.FC<MergeToolDialogProps> = ({
  open,
  repoPath,
  path,
  kind,
  theirsTitle,
  localDetails,
  theirsDetails,
  monacoTheme = 'ide-jetbrains-dark',
  busy = false,
  onClose,
  onApplied,
  isNativeWindow = false
}) => {
  const [marked, setMarked] = useState('');
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [ours, setOurs] = useState('');
  const [theirs, setTheirs] = useState('');
  const [loading, setLoading] = useState(false);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hunkIdx, setHunkIdx] = useState(0);
  const [wsMode, setWsMode] = useState<WsMode>('none');
  const [hlMode, setHlMode] = useState<HlMode>('words');
  // 自动应用默认关：初始 Result = base，非冲突更改待应用（对齐 PyCharm 实测 ——
  // Result 只有 base 行，theirs 新增留在右栏绿色 + ≪ ✕ 标记）
  const [autoApply, setAutoApply] = useState(false);
  const [localDetailsOpen, setLocalDetailsOpen] = useState(false);
  const [remoteDetailsOpen, setRemoteDetailsOpen] = useState(false);
  const [gearOpen, setGearOpen] = useState(false);
  const [sideMarks, setSideMarks] = useState<{ ours: number[]; theirs: number[] }>({ ours: [], theirs: [] });
  const [sideBands, setSideBands] = useState<{ ours: SideBand[]; theirs: SideBand[] }>({ ours: [], theirs: [] });
  const [sideConflictRows, setSideConflictRows] = useState<{ ours: Set<number>; theirs: Set<number> }>({
    ours: new Set(),
    theirs: new Set()
  });
  const [sideWordDiffs, setSideWordDiffs] = useState<{
    ours: Array<{ line: number; startCol: number; endCol: number }>;
    theirs: Array<{ line: number; startCol: number; endCol: number }>;
  }>({
    ours: [],
    theirs: []
  });
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const oursRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const theirsRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const decoIdsRef = useRef<string[]>([]);
  const blockPosRef = useRef<BlockPos[]>([]);
  const blocksRef = useRef<Block[]>([]);
  const gearRef = useRef<HTMLDivElement | null>(null);
  const resultWedgeRef = useRef<HTMLDivElement | null>(null);
  const resultBandsRef = useRef<Array<{ top: number; height: number; dir: WedgeDir; conflict?: boolean }>>([]);
  const resultConflictLinesRef = useRef<Set<number>>(new Set());

  const updateResultLineNumbers = (ed: Monaco.editor.IStandaloneCodeEditor, conflictLines: Set<number>) => {
    const dom = ed.getDomNode();
    if (!dom) return;
    const els = dom.querySelectorAll('.margin-view-overlays .line-numbers');
    els.forEach(el => {
      const num = parseInt(el.textContent || '', 10);
      if (!isNaN(num) && conflictLines.has(num)) {
        (el as HTMLElement).style.color = '#e55353';
        (el as HTMLElement).style.fontWeight = '600';
      } else if (!isNaN(num)) {
        (el as HTMLElement).style.color = '';
        (el as HTMLElement).style.fontWeight = '';
      }
    });
  };
  const syncBlocks = (b: Block[]) => {
    blocksRef.current = b;
    setBlocks(b);
  };

  useEffect(() => {
    ensureMonacoReady().catch(() => {});
  }, []);

  const fileName = path.split(/[\\/]/).pop() ?? path;
  const fileLanguage = languageOf(path);

  useEffect(() => {
    if (!open || !repoPath || !path) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setHunkIdx(0);
    Promise.all([
      invokeCommand<MergeMarked>('git_merge_file_marked', { repoPath, path }),
      invokeCommand<MergeFileContent>('git_merge_file_content', { repoPath, path })
    ])
      .then(([m, c]) => {
        if (cancelled) return;
        if (c.kind === 'resolved') {
          // 竞态：工具打开后文件已在别处解决，无块可合，直接报错让用户关框
          setError(t().conflict.alreadyResolved(path));
          return;
        }
        const view = buildMergeView(m.merged);
        // 侧栏高亮位置：块内容在各自整份文件里是连续片段，按块序移动指针定位
        const oPos = locateBlocks(c.ours.split('\n'), view.hunks.map(h => h.ours));
        const tPos = locateBlocks(c.theirs.split('\n'), view.hunks.map(h => h.theirs));
        blockPosRef.current = view.hunks.map((_, i) => ({ ours: oPos[i], theirs: tPos[i] }));
        setMarked(view.text);
        syncBlocks(view.hunks.map(h => ({ ...h, choice: 'none' as BlockChoice })));
        setOurs(c.ours);
        setTheirs(c.theirs);
      })
      .catch(err => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, repoPath, path]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !acting && !busy) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, acting, busy, onClose]);

  useEffect(() => {
    if (!open || !gearOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!gearRef.current?.contains(e.target as Node)) setGearOpen(false);
    };
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  }, [open, gearOpen]);

  /** 导航对象 = 所有可见块（更改 + 未决冲突都算，对齐 PyCharm 的 ↑↓）。 */
  const navList = useMemo(() => blocks.filter(visible), [blocks]);
  const cur = navList.length === 0 ? 0 : Math.min(hunkIdx, navList.length - 1);

  // —— 状态行（PyCharm: "2 changes. No conflicts."）——
  // 计的是"侧栏 vs base"的待决更改数（与结果格当前是否已吃入无关）：
  // 已拍板（choice≠none）的块不再计。
  const changesCount = useMemo(
    () =>
      blocks.reduce((n, b) => {
        if (isConflict(b) || !visible(b) || b.choice !== 'none') return n;
        const side = b.ours.length > 0 ? b.ours : b.theirs;
        if (side.length === 0) return n;
        return n + alignAdded(b.base, side, wsMode).filter(Boolean).length;
      }, 0),
    [blocks, wsMode]
  );
  const conflictsLeft = blocks.filter(b => isConflict(b) && b.choice === 'none' && visible(b)).length;

  const revealLine = (line1: number) => {
    editorRef.current?.revealLineInCenter(line1);
  };

  const gotoBlock = (idx: number) => {
    if (navList.length === 0) return;
    const next = (idx + navList.length) % navList.length;
    setHunkIdx(next);
    revealLine(navList[next].start + 1);
  };

  /**
   * 用户敲字：前后缀定变更区，只调坐标不动 choice —— 手改不算解决。
   * 唯一例外：把未决块删空 = 显式删除手势，记 keep（删改冲突即确认删除）。
   */
  const handleChange = (v: string | undefined) => {
    const next = v ?? '';
    const reg = diffRegion(marked.split('\n'), next.split('\n'), wsMode);
    const delta = reg.newEnd - reg.oldEnd;
    syncBlocks(blocksRef.current.map(b => {
      let start = b.start;
      let end = b.end;
      if (end <= reg.start) return b;
      if (start >= reg.oldEnd) {
        start += delta;
        end += delta;
      } else {
        if (start >= reg.start) start = reg.start;
        end = end >= reg.oldEnd ? end + delta : reg.newEnd;
        if (end < start) end = start;
      }
      if (b.choice === 'none' && start === end) {
        return { ...b, start, end, choice: 'keep' as BlockChoice };
      }
      if (start === b.start && end === b.end) return b;
      return { ...b, start, end };
    }));
    setMarked(next);
    setError(null);
  };

  /** 吃掉一块（blocks 坐标）：整段换成一方行，记 choice，调后续坐标。 */
  const takeSide = (b: Block, side: 'ours' | 'theirs') => {
    const list = blocksRef.current;
    const idx = list.indexOf(b);
    if (idx < 0 || b.choice !== 'none') return;
    const rep = side === 'ours' ? b.ours : b.theirs;
    const lines = marked.split('\n');
    setMarked([...lines.slice(0, b.start), ...rep, ...lines.slice(b.end)].join('\n'));
    const delta = rep.length - (b.end - b.start);
    syncBlocks(list.map((x, i) => {
      if (i < idx) return x;
      if (i === idx) return { ...x, end: x.start + rep.length, choice: side };
      return { ...x, start: x.start + delta, end: x.end + delta };
    }));
    setError(null);
    setHunkIdx(Math.min(idx, Math.max(0, navList.length - 2)));
    setTimeout(() => revealLine(b.start + 1), 0);
  };

  /**
   * ✕ 忽略某侧修改（对齐 PyCharm 实测）：
   * 1. 若另一侧有不同于 base 的变更，忽略此侧等价于采纳另一侧；
   * 2. 若另一侧未变（或为空），忽略当前侧等价于放弃此侧变更、保持 base，并将该块标记为已解决（keep）。
   */
  const ignoreSide = (b: Block, side: 'ours' | 'theirs') => {
    const list = blocksRef.current;
    const idx = list.indexOf(b);
    if (idx < 0 || b.choice !== 'none') return;
    const other = side === 'ours' ? 'theirs' : 'ours';
    const otherLines = other === 'ours' ? b.ours : b.theirs;

    if (otherLines.length > 0 && !sameArr(otherLines, b.base)) {
      takeSide(b, other);
      return;
    }

    const lines = marked.split('\n');
    const rep = b.base;
    setMarked([...lines.slice(0, b.start), ...rep, ...lines.slice(b.end)].join('\n'));
    const delta = rep.length - (b.end - b.start);
    syncBlocks(list.map((x, i) => {
      if (i < idx) return x;
      if (i === idx) return { ...x, end: x.start + rep.length, choice: 'keep' as BlockChoice };
      return { ...x, start: x.start + delta, end: x.end + delta };
    }));
    setError(null);
    setHunkIdx(Math.min(idx, Math.max(0, navList.length - 2)));
    setTimeout(() => revealLine(b.start + 1), 0);
  };

  /** ✕：以块当前文本解决（块内手改完的标准收尾）。 */
  const keepBlock = (b: Block) => {
    const list = blocksRef.current;
    if (list.indexOf(b) < 0 || b.choice !== 'none') return;
    syncBlocks(list.map(x => (x === b ? { ...x, choice: 'keep' as BlockChoice } : x)));
    setError(null);
    setHunkIdx(Math.min(list.indexOf(b), Math.max(0, navList.length - 2)));
  };

  /**
   * 重铺整份结果文本：fn 返回非 null = 该块换成给定行（坐标重算，choice 不动）。
   * 应用按钮 / 自动应用开关 / 侧栏忽略手势 / Apply 前补应用共用。
   * 返回新全文（无实际改动返回 null）。
   */
  const rebuildWith = (fn: (b: Block, idx: number, body: string[]) => string[] | null): string | null => {
    const list = blocksRef.current;
    const lines = marked.split('\n');
    const out: string[] = [];
    const next: Block[] = [];
    let cur = 0;
    let changed = false;
    for (let i = 0; i < list.length; i++) {
      const b = list[i];
      out.push(...lines.slice(cur, b.start));
      const body = lines.slice(b.start, b.end);
      const rep = fn(b, i, body);
      if (rep && !sameArr(rep, body)) changed = true;
      const start = out.length;
      out.push(...(rep ?? body));
      const end = out.length;
      next.push({ ...b, start, end });
      cur = b.end;
    }
    out.push(...lines.slice(cur));
    const text = out.join('\n');
    if (!changed) return null;
    syncBlocks(next);
    setMarked(text);
    return text;
  };

  /** 应用计划：该方向上仍可应用（内容 ≠ 目标且不是用户手改文本）的块。 */
  const applyPlan = (dir: 'ours' | 'theirs' | 'both'): Set<number> => {
    const plan = new Set<number>();
    const list = blocksRef.current;
    const lines = marked.split('\n');
    list.forEach((b, i) => {
      if (!visible(b) || isConflict(b) || b.choice !== 'none') return;
      const side = dir === 'ours' ? b.ours : dir === 'theirs' ? b.theirs : b.ours.length > 0 ? b.ours : b.theirs;
      if (side.length === 0) return;
      const body = lines.slice(b.start, b.end);
      if (sameArr(body, side)) return;
      // 用户手改过的块（≠ base 且 ≠ 任一方）不覆盖
      if (!sameArr(body, b.base) && !sameArr(body, b.ours) && !sameArr(body, b.theirs)) return;
      plan.add(i);
    });
    return plan;
  };

  const applyAllNonConflict = (dir: 'ours' | 'theirs' | 'both') => {
    if (acting || busy) return;
    const plan = applyPlan(dir);
    if (plan.size === 0) return;
    rebuildWith((b, i) => {
      if (!plan.has(i)) return null;
      return dir === 'ours' ? b.ours : dir === 'theirs' ? b.theirs : b.ours.length > 0 ? b.ours : b.theirs;
    });
  };

  const toggleAutoApply = (on: boolean) => {
    if (acting || busy) return;
    setAutoApply(on);
    rebuildWith((b, _i, body) => {
      if (!visible(b) || isConflict(b) || b.choice !== 'none') return null;
      const side = b.ours.length > 0 ? b.ours : b.theirs;
      const target = on ? side : b.base;
      if (!sameArr(body, b.base) && !sameArr(body, b.ours) && !sameArr(body, b.theirs)) return null;
      if (sameArr(body, target)) return null;
      return target;
    });
  };

  /**
   * 侧栏 gutter 点击：≫/≪ = 把此侧内容吃进结果（冲突即拍板）；
   * ✕ = 忽略此侧更改 —— 他侧非空取他侧（= 确认保留他侧），否则回 base。
   */
  const onSideMark = (side: 'ours' | 'theirs', line1: number, action: 'apply' | 'ignore') => {
    if (acting || busy) return;
    const list = blocksRef.current;
    const pos = blockPosRef.current;
    let bi = -1;
    list.forEach((b, i) => {
      if (bi >= 0 || !visible(b) || b.choice !== 'none') return;
      const r = pos[i]?.[side];
      if (r && line1 - 1 >= r[0] && line1 - 1 < r[1]) bi = i;
    });
    if (bi < 0) {
      // 空侧锚定行：该侧内容为空，动态匹配对应锚定行等于 line1 的可见未决块（通用任意位置删除与冲突）
      list.forEach((b, i) => {
        if (bi >= 0 || !visible(b) || b.choice !== 'none') return;
        const sl = side === 'ours' ? b.ours : b.theirs;
        if (sl.length === 0) {
          const otherR = pos[i]?.[side === 'ours' ? 'theirs' : 'ours'];
          const anchor = otherR ? otherR[0] + 1 : Math.max(1, b.start + 1);
          if (anchor === line1) bi = i;
        }
      });
    }
    if (bi < 0) return;
    const b = list[bi];
    if (b.choice !== 'none') return;
    if (action === 'apply') {
      takeSide(b, side);
      return;
    }
    ignoreSide(b, side);
  };

  // gutter 点击走 ref（监听注册一次）：连点同一块时第二下看到已解决，不 double 计
  const takeRef = useRef((_line: number, _action: 'ours' | 'theirs') => {});
  useEffect(() => {
    takeRef.current = (line, action) => {
      const b = blocksRef.current.find(
        x => x.choice === 'none' && visible(x) && (action === 'theirs' ? x.end === line : x.start + 1 === line)
      );
      if (!b || acting || busy) return;
      takeSide(b, action);
    };
  });

  /** 结果格斜切 overlay（盖住行号 + 正文，z-index:-1 压在文本下、面板底上）。 */
  const updateResultWedge = (ed: Monaco.editor.IStandaloneCodeEditor) => {
    const dom = ed.getDomNode();
    if (!dom) return;
    let w = resultWedgeRef.current;
    if (!w || !dom.contains(w)) {
      w = dom.querySelector<HTMLDivElement>('.merge-wedge-overlay');
      if (!w) {
        w = document.createElement('div');
        w.className = 'merge-wedge-overlay';
        dom.appendChild(w);
      }
      resultWedgeRef.current = w;
    }
    const st = ed.getScrollTop();
    const bands = resultBandsRef.current;
    const kids = w.children;
    while (kids.length > bands.length) w.removeChild(kids[kids.length - 1]);
    while (kids.length < bands.length) {
      const d = document.createElement('div');
      d.className = 'merge-wedge band';
      w.appendChild(d);
    }
    bands.forEach((b, i) => {
      const el = kids[i] as HTMLElement;
      el.className = `merge-wedge band ${b.conflict ? 'conflict' : ''}`;
      el.style.top = `${b.top - st}px`;
      el.style.height = `${b.height}px`;
      const R = Math.max(8, Math.round(b.height / 3));
      el.style.clipPath =
        b.dir === 'right'
          ? `polygon(0 0, calc(100% - ${R}px) 0, 100% 100%, 100% 100%)`
          : `polygon(0 0, 100% 0, 100% 100%, ${R}px 100%)`;
    });
  };

  /** 结果格着色：冲突红底（整块覆盖，对齐 PyCharm 实测）+ ≫/≪ 标记；非冲突块分两态 ——
   *  待应用（内容==base）：删除侧 → 淡紫斜切带（盖到最后一根非空行）+
   *  块尾 2px 绿插入线；已应用/手改：context 淡紫带 + 首条非空 delta 绿顶线。 */
  const applyDecos = (ed: Monaco.editor.IStandaloneCodeEditor, list: Block[]) => {
    const decos: Monaco.editor.IModelDeltaDecoration[] = [];
    const whole = (ln: number): Monaco.IRange => ({ startLineNumber: ln, startColumn: 1, endLineNumber: ln, endColumn: 1 });
    const lines = ed.getModel()?.getLinesContent() ?? [];
    const bands: Array<{ top: number; height: number; dir: WedgeDir; conflict?: boolean }> = [];
    const lh = lines.length > 1 ? ed.getTopForLineNumber(2) - ed.getTopForLineNumber(1) : 18;
    const conflictLines = new Set<number>();
    for (const b of list) {
      if (!visible(b) || b.choice !== 'none') continue;
      if (isConflict(b)) {
        for (let i = b.start; i < b.end && i < lines.length; i++) {
          conflictLines.add(i + 1);
          decos.push({
            range: whole(i + 1),
            options: {
              className: 'merge-hunk-bg conflict',
              isWholeLine: true,
              marginClassName: 'merge-hunk-margin conflict',
              overviewRuler: {
                color: '#e55353',
                position: 7
              }
            }
          });
        }
        bands.push({
          top: ed.getTopForLineNumber(b.start + 1),
          height: Math.max(lh, (b.end - b.start) * lh),
          dir: wedgeDirOf(b),
          conflict: true
        });
        decos.push({
          range: whole(b.start + 1),
          options: {
            glyphMarginClassName: 'merge-glyph-take-left conflict',
            glyphMarginHoverMessage: { value: t().conflict.mergeTool.takeLeftTitle }
          }
        });
        decos.push({
          range: whole(b.end),
          options: {
            glyphMarginClassName: 'merge-glyph-take-right conflict',
            glyphMarginHoverMessage: { value: t().conflict.mergeTool.takeRightTitle }
          }
        });
      } else if (hlMode !== 'none') {
        const body = lines.slice(b.start, b.end);
        const side = b.ours.length > 0 ? b.ours : b.theirs;
        if (sameArr(body, b.base)) {
          // 待应用态（Result = base，对齐 PyCharm 实测）：淡紫斜切带盖到
          // 最后一根非空行（尾部空行不染）；存留侧有内容 → 块尾绿插入线
          let lastNb = -1;
          for (let i = b.end - 1; i >= b.start; i--) {
            if (lines[i] !== undefined && lines[i] !== '') {
              lastNb = i;
              break;
            }
          }
          if (lastNb >= b.start) {
            bands.push({
              top: ed.getTopForLineNumber(b.start + 1),
              height: (lastNb - b.start + 1) * lh,
              dir: wedgeDirOf(b)
            });
          }
          if (side.length > 0) {
            decos.push({
              range: whole(b.end),
              options: { className: 'merge-pending-rule', isWholeLine: true }
            });
          }
          continue;
        }
        const added = alignAdded(b.base, body, wsMode);
        // 连续 context 段（含空行）→ 淡紫斜切带（overlay 盖行号+正文）
        let k = 0;
        while (k < added.length) {
          if (added[k]) {
            k++;
            continue;
          }
          let j = k;
          while (j < added.length && !added[j]) j++;
          bands.push({
            top: ed.getTopForLineNumber(b.start + k + 1),
            height: (j - k) * lh,
            dir: wedgeDirOf(b)
          });
          k = j;
        }
        // 首条非空 delta 行 = 2px 绿顶线（delta 空行不染，对齐实测）
        const ruleIdx = added.findIndex(
          (isAdd, idx) => isAdd && lines[b.start + idx] !== undefined && lines[b.start + idx] !== ''
        );
        if (ruleIdx >= 0) {
          decos.push({
            range: whole(b.start + ruleIdx + 1),
            options: { className: 'merge-delta-rule', isWholeLine: true }
          });
        }
      }
    }
    decoIdsRef.current = ed.deltaDecorations(decoIdsRef.current, decos);
    resultBandsRef.current = bands;
    resultConflictLinesRef.current = conflictLines;
    updateResultWedge(ed);
    updateResultLineNumbers(ed, conflictLines);
  };

  useEffect(() => {
    const ed = editorRef.current;
    if (ed) applyDecos(ed, blocks);
    // blocks 是唯一真相；t() 语言切换这种极小概率不管
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [blocks, wsMode, hlMode]);

  /** 侧栏着色 + gutter 标记：冲突块整段红底 + 词级高亮；非冲突块保留 add 绿 / mod 蓝 */
  useEffect(() => {
    const paint = (lines: string[], which: 'ours' | 'theirs') => {
      const marks: number[] = [];
      const bands: SideBand[] = [];
      const conflictRows = new Set<number>();
      const wordDiffs: Array<{ line: number; startCol: number; endCol: number }> = [];

      blocksRef.current.forEach((b, i) => {
        if (!visible(b) || b.choice !== 'none') return;
        const r = blockPosRef.current[i]?.[which];
        const sideArr = which === 'ours' ? b.ours : b.theirs;
        const dir = wedgeDirOf(b);
        const isConf = isConflict(b) && b.choice === 'none';

        if (!r) {
          // 该侧块内容为空（删除侧）：锚定该块在基准/他侧对应的起始行号，通用支持任意行位置的删除块
          if (sideArr.length === 0 && b.choice === 'none') {
            const otherR = blockPosRef.current[i]?.[which === 'ours' ? 'theirs' : 'ours'];
            const anchor = otherR ? otherR[0] + 1 : Math.max(1, b.start + 1);
            marks.push(anchor);
            if (isConf) {
              conflictRows.add(anchor);
              bands.push({
                start: anchor - 1,
                end: anchor,
                add: false,
                conflict: true,
                isDeleted: true,
                dir
              });
            }
          }
          return;
        }

        if (isConf) {
          // 冲突块在有内容的侧栏：全部染冲突红底，行号变红
          for (let ln = r[0]; ln < r[1]; ln++) {
            conflictRows.add(ln + 1);
          }
          bands.push({
            start: r[0],
            end: r[1],
            add: false,
            conflict: true,
            isDeleted: false,
            dir
          });
          marks.push(r[0] + 1);

          // 词级高亮（对齐 PyCharm 词级方块高亮模式）
          if (hlMode !== 'none') {
            const minLen = Math.min(b.base.length, sideArr.length);
            for (let j = 0; j < minLen; j++) {
              const diff = findLineDiffRange(b.base[j], sideArr[j]);
              if (diff) {
                wordDiffs.push({
                  line: r[0] + j + 1,
                  startCol: diff.startCol,
                  endCol: diff.endCol
                });
              }
            }
          }
          return;
        }

        // 非冲突更改块（常规单侧改动，保持原有 add 绿 / mod 蓝底色规则）
        const slice = lines.slice(r[0], r[1]);
        const kinds = sideChanges(b.base, slice, wsMode);
        let run: { start: number; end: number; add: boolean } | null = null;
        const flush = () => {
          if (run) {
            bands.push({ ...run, conflict: false, isDeleted: false, dir });
            run = null;
          }
        };
        kinds.forEach((k, idx) => {
          const ln = r[0] + idx;
          // none 行断带；changed 的空行照样染
          if (lines[ln] === undefined || k === 'none') {
            flush();
            return;
          }
          if (hlMode === 'none') return;
          if (run && run.add !== (k === 'add')) flush();
          if (run) run.end = ln + 1;
          else run = { start: ln, end: ln + 1, add: k === 'add' };
        });
        flush();
        if (b.choice === 'none') {
          const first = kinds.findIndex(
            (k, idx) => k !== 'none' && lines[r[0] + idx] !== undefined && lines[r[0] + idx] !== ''
          );
          if (first >= 0) marks.push(r[0] + first + 1);
        }
      });
      return { marks, bands, conflictRows, wordDiffs };
    };
    const oRes = paint(ours.split('\n'), 'ours');
    const tRes = paint(theirs.split('\n'), 'theirs');
    setSideMarks({ ours: oRes.marks, theirs: tRes.marks });
    setSideBands({ ours: oRes.bands, theirs: tRes.bands });
    setSideConflictRows({ ours: oRes.conflictRows, theirs: tRes.conflictRows });
    setSideWordDiffs({ ours: oRes.wordDiffs, theirs: tRes.wordDiffs });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ours, theirs, blocks, wsMode, hlMode]);

  const onEditorMount: OnMount = ed => {
    editorRef.current = ed;
    applyDecos(ed, blocksRef.current);
    ed.onDidScrollChange(() => {
      updateResultWedge(ed);
      updateResultLineNumbers(ed, resultConflictLinesRef.current);
    });
    ed.onDidLayoutChange(() => applyDecos(ed, blocksRef.current));
    // 按元素类名认 gutter 点击，不碰 MouseTargetType 枚举
    // （monaco-editor 只做类型导入，运行时枚举拿不到）。
    ed.onMouseDown(e => {
      const el = e.target.element as HTMLElement | null;
      const hit = el?.closest?.('.merge-glyph-take-left, .merge-glyph-take-right');
      if (!hit || !e.target.position) return;
      const action = (hit as HTMLElement).classList.contains('merge-glyph-take-left') ? 'ours' as const : 'theirs' as const;
      takeRef.current(e.target.position.lineNumber, action);
    });
  };

  const apply = async () => {
    const rest = blocksRef.current.filter(b => b.choice === 'none' && isConflict(b) && visible(b));
    if (rest.length > 0) {
      setError(t().conflict.mergeTool.stillMarked(rest.length));
      setHunkIdx(0);
      revealLine(rest[0].start + 1);
      return;
    }
    setActing(true);
    setError(null);
    try {
      // 待应用的非冲突更改（Result 还停在 base）在 Apply 时吃入 —— Apply = 采纳
      // 合并结果；显式忽略过的块（choice='keep'）与手改块（内容≠base）不碰。
      const final =
        rebuildWith((b, _i, body) => {
          if (!visible(b) || isConflict(b) || b.choice !== 'none') return null;
          if (!sameArr(body, b.base)) return null;
          const side = b.ours.length > 0 ? b.ours : b.theirs;
          return side.length > 0 ? side : null;
        }) ?? marked;
      if (final.trim() === '' && kind !== 'content') {
        // 删改冲突且结果为空 = 逐块全取了"删除"边：语义是确认删除，
        // 走 git_use_merge_side（git rm），不能写空文件再 add（那会留下空文件）。
        await invokeCommand('git_use_merge_side', {
          repoPath,
          path,
          side: kind === 'deleted_by_us' ? 'ours' : 'theirs'
        });
      } else {
        await invokeCommand('write_file_content', { path: toWorkspaceAbsPath(repoPath, path), content: final });
        await invokeCommand('git_stage_file', { repoPath, path });
      }
      onApplied(path, t().conflict.mergeTool.applied(path));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActing(false);
    }
  };

  if (!open) return null;

  const paneOptions = {
    readOnly: true,
    minimap: { enabled: false },
    fontSize: 12,
    lineNumbers: 'on' as const,
    scrollBeyondLastLine: false,
    automaticLayout: true,
    unicodeHighlight: UNICODE_HIGHLIGHT_OPTIONS
  };

  const statusText = loading
    ? t().common.loading
    : `${changesCount > 0 ? t().conflict.mergeTool.statusChanges(changesCount) : t().conflict.mergeTool.statusNoChanges}. ${
        conflictsLeft > 0 ? t().conflict.mergeTool.statusConflicts(conflictsLeft) : t().conflict.mergeTool.statusNoConflicts
      }.`;

  const canLeft = applyPlan('ours').size > 0;
  const canRight = applyPlan('theirs').size > 0;
  const canBoth = applyPlan('both').size > 0;

  return (
    <div className={`py-dialog-backdrop merge-tool-backdrop ${isNativeWindow ? 'is-native-window' : ''}`}>
      <div
        className="py-merge-tool-dialog"
        style={
          isNativeWindow
            ? { position: 'relative', width: '100vw', height: '100vh', border: 'none', borderRadius: 0, boxShadow: 'none' }
            : undefined
        }
      >
        <div className="py-dialog-header" data-tauri-drag-region={isNativeWindow ? 'true' : undefined}>
          <span className="py-dialog-title" style={isNativeWindow ? { paddingLeft: 70 } : undefined}>
            {t().conflict.mergeTool.title(toWorkspaceAbsPath(repoPath, path))}
          </span>
          {!isNativeWindow && (
            <button className="py-dialog-close" onClick={onClose} title={t().common.close}>
              <IconWindowClose size={13} />
            </button>
          )}
        </div>

        <div className="merge-tool-toolbar">
          <button className="py-btn sm" disabled={navList.length === 0} onClick={() => gotoBlock(cur - 1)} title={t().conflict.mergeTool.prevTitle}>↑</button>
          <button className="py-btn sm" disabled={navList.length === 0} onClick={() => gotoBlock(cur + 1)} title={t().conflict.mergeTool.nextTitle}>↓</button>
          <span className="tb-sep" />
          <button
            className="py-btn sm tb-main-apply"
            disabled={!canBoth || acting || busy}
            onClick={() => applyAllNonConflict('both')}
            title={t().conflict.mergeTool.applyAllTitle}
          >
            <IconApplyArrow />
            <span className="tb-caret">▾</span>
          </button>
          <span className="tb-label">{t().conflict.mergeTool.applyLabel}</span>
          {/* 箭头指向来源侧：» = 右侧（theirs），« = 左侧（ours），»« = 双侧 */}
          <button
            className="py-btn sm"
            disabled={!canRight || acting || busy}
            onClick={() => applyAllNonConflict('theirs')}
            title={t().conflict.mergeTool.applyFromRightTitle}
          >»</button>
          <button
            className="py-btn sm"
            disabled={!canBoth || acting || busy}
            onClick={() => applyAllNonConflict('both')}
            title={t().conflict.mergeTool.applyAllTitle}
          >»«</button>
          <button
            className="py-btn sm"
            disabled={!canLeft || acting || busy}
            onClick={() => applyAllNonConflict('ours')}
            title={t().conflict.mergeTool.applyFromLeftTitle}
          >«</button>
          <button
            className="py-btn sm"
            disabled={!canBoth || acting || busy}
            onClick={() => applyAllNonConflict('both')}
            title={t().conflict.mergeTool.wandTitle}
          >✎</button>
          <span className="tb-sep" />
          <select
            className="tb-select"
            value={wsMode}
            disabled={acting || busy}
            onChange={e => setWsMode(e.target.value as WsMode)}
          >
            {WS_OPTIONS.map(o => (
              <option key={o.id} value={o.id}>{o.pick()}</option>
            ))}
          </select>
          <select
            className="tb-select"
            value={hlMode}
            disabled={acting || busy}
            onChange={e => setHlMode(e.target.value as HlMode)}
          >
            {HL_OPTIONS.map(o => (
              <option key={o.id} value={o.id}>{o.pick()}</option>
            ))}
          </select>
          <span className="tb-sep" />
          <button
            className="py-btn sm"
            disabled={navList.length === 0 || acting || busy}
            onClick={() => keepBlock(navList[cur])}
            title={t().conflict.mergeTool.takeKeepTitle}
          >✕</button>
          <button
            className={`py-btn sm tb-toggle${autoApply ? ' active' : ''}`}
            disabled={acting || busy}
            onClick={() => toggleAutoApply(!autoApply)}
            title={t().conflict.mergeTool.autoApplyTitle}
          >⇅</button>
          <div className="tb-gear-wrap" ref={gearRef}>
            <button className="py-btn sm" onClick={() => setGearOpen(v => !v)} title={t().conflict.mergeTool.gearTitle}><IconGear /></button>
            {gearOpen && (
              <div className="tb-menu">
                <label className="tb-menu-item">
                  <input
                    type="checkbox"
                    checked={autoApply}
                    onChange={e => {
                      toggleAutoApply(e.target.checked);
                      setGearOpen(false);
                    }}
                  />
                  {t().conflict.mergeTool.gearAutoApply}
                </label>
              </div>
            )}
          </div>
          <span className="merge-tool-count">{statusText}</span>
        </div>

        {error && <div className="push-error" style={{ margin: '0 14px 8px' }}>{error}</div>}

        <div className="conflict-panes merge-tool-panes">
          <div className="conflict-pane">
            <div className="pane-head">
              <span className="conflict-pane-title">
                <LockGlyph />
                {t().conflict.mergeTool.localPane}
              </span>
              {localDetails && (
                <button className="show-details" onClick={() => setLocalDetailsOpen(v => !v)}>
                  {t().conflict.mergeTool.showDetails}
                </button>
              )}
            </div>
            {localDetailsOpen && localDetails && <div className="pane-details">{localDetails}</div>}
            <div className="side-holder">
              <SideEditor
                side="ours"
                value={ours}
                theme={monacoTheme}
                language={fileLanguage}
                marks={sideMarks.ours}
                conflictRows={sideConflictRows.ours}
                bands={sideBands.ours}
                wordDiffs={sideWordDiffs.ours}
                onMark={(line1, action) => onSideMark('ours', line1, action)}
                onReady={ed => { oursRef.current = ed; }}
              />
            </div>
          </div>
          <div className="conflict-pane">
            <div className="pane-head">
              <span className="conflict-pane-title result-title">
                {t().conflict.mergeTool.resultPane}
                <span className="pane-filename">{fileName}</span>
              </span>
            </div>
            <Editor
              height="100%"
              value={marked}
              onChange={v => handleChange(v)}
              onMount={onEditorMount}
              theme={monacoTheme}
              language={fileLanguage}
              options={{ ...paneOptions, readOnly: false, glyphMargin: true }}
            />
          </div>
          <div className="conflict-pane">
            <div className="pane-head">
              <span className="conflict-pane-title">
                <LockGlyph />
                {theirsTitle}
              </span>
              {theirsDetails && (
                <button className="show-details" onClick={() => setRemoteDetailsOpen(v => !v)}>
                  {t().conflict.mergeTool.showDetails}
                </button>
              )}
            </div>
            {remoteDetailsOpen && theirsDetails && <div className="pane-details">{theirsDetails}</div>}
            <div className="side-holder">
              <SideEditor
                side="theirs"
                value={theirs}
                theme={monacoTheme}
                language={fileLanguage}
                marks={sideMarks.theirs}
                conflictRows={sideConflictRows.theirs}
                bands={sideBands.theirs}
                wordDiffs={sideWordDiffs.theirs}
                onMark={(line1, action) => onSideMark('theirs', line1, action)}
                onReady={ed => { theirsRef.current = ed; }}
              />
            </div>
          </div>
        </div>

        <div className="merge-tool-footer">
          <span>
            <button className="py-btn" disabled={navList.length === 0 || acting || busy} onClick={() => takeSide(navList[cur], 'ours')}>
              {t().conflict.mergeTool.acceptLeft}
            </button>
            <button className="py-btn" disabled={navList.length === 0 || acting || busy} onClick={() => takeSide(navList[cur], 'theirs')}>
              {t().conflict.mergeTool.acceptRight}
            </button>
          </span>
          <span>
            <button className="py-btn" disabled={acting || busy} onClick={onClose}>
              {t().conflict.mergeTool.cancel}
            </button>
            <button className="py-btn primary" disabled={loading || acting || busy} onClick={() => void apply()}>
              {t().conflict.mergeTool.apply}
            </button>
          </span>
        </div>
      </div>
    </div>
  );
};
