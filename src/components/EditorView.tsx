import React, { useState, useEffect, useRef, useMemo } from 'react';
import Editor from '@monaco-editor/react';
import type { Monaco } from '@monaco-editor/react';
import { isTauriEnvironment, invokeCommand } from '../services/tauriBridge';
import { isImageFile } from '../services/fileTypes';
import { t } from '../services/i18n';
import { MONACO_THEME_BY_APP_THEME, UNICODE_HIGHLIGHT_OPTIONS } from '../services/monacoSetup';
import { IconHistory, IconAnnotate } from './FileIcons';
import { EditorContextMenu, type EditorMenuState } from './EditorContextMenu';
import type { BlameLine, FileChangeHunk } from '../types';
// 仅用于类型标注：monaco 运行时由 monacoSetup 的 ensureMonacoReady() 懒加载，
// 这里静态 import 运行时会把它拖回主 chunk，破坏懒加载拆分
import type * as monacoNs from 'monaco-editor';
import type { editor as monacoEditorNs, IDisposable } from 'monaco-editor';
import { monacoLspBridge } from '../services/monacoLspClient';

interface EditorViewProps {
  filePath: string;
  code: string;
  language: string;
  fontSize?: number;
  /** 打开后跳转到的行号（1 起） */
  gotoLine?: number | null;
  /** 应用配色主题 ID：编辑器主题跟随设置 */
  themeId?: number;
  /** 读取失败原因：有值时展示错误面板而不是空编辑器 */
  loadError?: string | null;
  /** 读取失败的后端协议码；`file_too_large` 时面板给出「显示/强开」动作 */
  loadErrorCode?: string | null;
  /** 大文件错误面板：在文件管理器中显示（走已授权的 fs_reveal） */
  onRevealFile?: (path: string) => void;
  /** 大文件错误面板：用户显式确认后带 force 重读 */
  onForceOpen?: (path: string) => void;
  /** 仓库根目录；提供后编辑器工具栏可用 git blame / 文件历史 */
  repoPath?: string;
  /** 是否开启行级 blame 注释（Annotate with Git Blame） */
  blameOn?: boolean;
  onToggleBlame?: () => void;
  onShowHistory?: () => void;
  /** blame 行点击：打开对应提交的 diff */
  onOpenCommitDiff?: (hash: string) => void;
  onChange?: (value: string | undefined) => void;
  onCursorChange?: (position: { line: number; column: number }) => void;
  onTabSizeChange?: (tabSize: number) => void;
  /** Markdown 模式切换时恢复的滚动比例（0~1） */
  scrollRatio?: number;
  /** 模式切换标识：比例不变时也能触发一次恢复 */
  scrollRestoreKey?: string | number;
  /**
   * pane 是否可见。Markdown 预览模式下源码 pane 只是被盖住，Monaco 仍有真实
   * 几何尺寸并会随 setModel 触发滚动事件；不拦的话它残留的 scrollTop 会覆盖
   * 文件自己的比例（跨文件串位）。默认 true：普通编辑器只有这一个 pane。
   */
  active?: boolean;
  onScrollChange?: (ratio: number) => void;
  /**
   * ⌘S 保存当前文件。全局快捷键在编辑器聚焦时会跳过（Monaco 的输入面命中
   * textarea 豁免分支），菜单项又依赖原生菜单转发，所以编辑器内这一路必须自带。
   */
  onSave?: () => void;
}

/** 与 Monaco options.lineHeight 保持一致，blame 列逐行对齐 */
const EDITOR_LINE_HEIGHT = 20;
/** blame 虚拟化：可视区间向外多渲染的缓冲行，避免快速滚动时露出空白 */
const BLAME_OVERSCAN = 8;
/**
 * 编辑器静态 options 提为模块级常量：
 * @monaco-editor/react 用 useEffect 比对 options 对象标识再 updateOptions，
 * 内联字面量每次 render 都是新对象 → 每次都触发全量 options 重算。
 * 只有 fontSize / folding 随 prop 变化，在组件里 useMemo 派生。
 */
const EDITOR_OPTIONS_BASE = {
  lineHeight: EDITOR_LINE_HEIGHT,
  fontFamily: 'JetBrains Mono, Menlo, Monaco, "Courier New", monospace',
  fontLigatures: true,
  minimap: { enabled: true, side: 'right' as const },
  scrollBeyondLastLine: false,
  automaticLayout: true,
  tabSize: 2,
  wordWrap: 'off' as const,
  lineNumbers: 'on' as const,
  renderLineHighlight: 'all' as const,
  cursorBlinking: 'smooth' as const,
  smoothScrolling: true,
  bracketPairColorization: { enabled: true },
  guides: {
    bracketPairs: true,
    indentation: true
  },
  // 中文不做 unicode 警告高亮：全角标点（：，；（））会被 monaco 的「同形字符」
  // 表判成可混淆而套上黄色边框，被误读成代码报错；不可见字符仍保持防护
  unicodeHighlight: UNICODE_HIGHLIGHT_OPTIONS
};
/** blame 虚拟化渲染窗口：[start, end] 闭区间，超出部分不生成 DOM */
interface BlameVisibleRange {
  start: number;
  end: number;
}

export const EditorView: React.FC<EditorViewProps> = ({
  filePath,
  code,
  language,
  fontSize = 13,
  gotoLine = null,
  themeId = 1,
  loadError = null,
  loadErrorCode = null,
  onRevealFile,
  onForceOpen,
  repoPath,
  blameOn = false,
  onToggleBlame,
  onShowHistory,
  onOpenCommitDiff,
  onChange,
  onCursorChange,
  onTabSizeChange,
  scrollRatio,
  scrollRestoreKey,
  active = true,
  onScrollChange,
  onSave
}) => {
  const parts = filePath.split(/[\\/]/).filter(Boolean);
  const fileName = parts[parts.length - 1] ?? filePath;
  const isImage = language === 'image' || isImageFile(filePath);

  const [imageSrc, setImageSrc] = useState('');
  const [imageError, setImageError] = useState<string | null>(null);
  const [imageSize, setImageSize] = useState<{ width: number; height: number } | null>(null);
  const [zoom, setZoom] = useState(1);
  /** 编辑器右键菜单位置：null = 关闭 */
  const [editorMenu, setEditorMenu] = useState<EditorMenuState | null>(null);
  const [blameLines, setBlameLines] = useState<BlameLine[]>([]);
  /** blame 最新数据：供滚动回调等非 render 闭包读取，避免陈旧闭包读到旧行数 */
  const blameLinesRef = useRef<BlameLine[]>([]);
  /** blame 虚拟化窗口：只渲染该区间内的行 */
  const [visibleRange, setVisibleRange] = useState<BlameVisibleRange>({ start: 1, end: 0 });
  /** gutter 变更条装饰集合与 monaco 实例引用 */
  const decoRef = useRef<monacoEditorNs.IEditorDecorationsCollection | null>(null);
  const monacoRef = useRef<typeof monacoNs | null>(null);
  const editorRef = useRef<monacoEditorNs.IStandaloneCodeEditor | null>(null);
  /** onDid* 注册的监听统一收在这里：wrapper 不调用 onMount 的返回值，只能自建卸载清理 */
  const disposablesRef = useRef<IDisposable[]>([]);
  const scrollChangeRef = useRef(onScrollChange);
  scrollChangeRef.current = onScrollChange;
  /** 保存回调同样只在这里读最新值：onMount 只跑一次，闭包会永远停在首帧 */
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;
  /**
   * 可见性给滚动事件快车道读：滚动回调里拿到的必须是当前值，
   * 不能用闭包里的旧 prop（React 18 批处理下 mode 切换会有滞后窗口）。
   */
  const editorActiveRef = useRef(active ?? true);
  editorActiveRef.current = active ?? true;
  const scrollRestoreKeyRef = useRef<string | number | undefined>(undefined);
  // gotoLine 变化时跳转：onMount 只在编辑器创建时消费一次初始值，
  // 同一文件二次跳转（Search Everywhere 再次定位已打开文件）不触发 remount，
  // 必须在这里补；编辑器尚未挂载时跳过，由 onMount 兜底
  useEffect(() => {
    if (!gotoLine || gotoLine <= 0) return;
    const editor = editorRef.current;
    if (!editor) return;
    editor.revealLineInCenter(gotoLine);
    editor.setPosition({ lineNumber: gotoLine, column: 1 });
    editor.focus();
  }, [gotoLine]);
  // 仅 Markdown 源码/预览切换用 scrollRatio 按比例复位；普通文件必须交给
  // @monaco-editor/react 的 saveViewState/restoreViewState 保存每文件滚动与光标。
  // 之前该 effect 对所有文件都执行，scrollRatio 缺省 0 会在每次切文件后把视口
  // 拉回顶部（光标仍停在原行，于是状态栏显示 341:1 但屏幕是第 1 行）。
  useEffect(() => {
    const editor = editorRef.current;
    if (!editor) return;
    if (scrollRatio === undefined) return;
    const key = `${active ? 1 : 0}|${filePath}|${scrollRestoreKey}`;
    if (key === scrollRestoreKeyRef.current) return;
    scrollRestoreKeyRef.current = key;
    if (gotoLine && gotoLine > 0) return;
    if (!active) return;
    const maxScroll = Math.max(0, editor.getContentHeight() - editor.getLayoutInfo().height);
    editor.setScrollTop(maxScroll * Math.max(0, Math.min(1, scrollRatio)));
  }, [scrollRatio, scrollRestoreKey, gotoLine, active, filePath]);

  useEffect(() => {
    if (!isImage) return;
    // 图片经 `read_image_bytes` 读 base64 走 data URL（P2-asset）：
    // 不再经过 asset:// 协议 —— 其 scope 曾放 `$HOME/**`，任何 XSS 都能读家目录。
    // 命令侧已做工作区授权 + 扩展名白名单 + 8MB 上限。
    if (!isTauriEnvironment()) {
      setImageSrc(filePath);
      setImageError(null);
      setZoom(1);
      setImageSize(null);
      return;
    }
    let cancelled = false;
    setImageSrc('');
    setImageError(null);
    void invokeCommand<{ mime: string; base64: string; size: number }>('read_image_bytes', { path: filePath })
      .then(dto => {
        if (cancelled) return;
        setImageSrc(`data:${dto.mime};base64,${dto.base64}`);
        setZoom(1);
        setImageSize(null);
      })
      .catch(err => {
        if (cancelled) return;
        setImageSrc('');
        setImageError(err instanceof Error ? err.message : String(err));
      });
    return () => { cancelled = true; };
  }, [filePath, isImage]);

  // blame 数据随开关/文件变化拉取
  useEffect(() => {
    let cancelled = false;
    if (!blameOn || !repoPath || isImage || loadError) {
      blameLinesRef.current = [];
      setBlameLines([]);
      return;
    }
    invokeCommand<BlameLine[]>('git_blame', { repoPath, path: filePath })
      .then(lines => {
        if (!cancelled) {
          // ref 与 state 同步：滚动回调等非 render 闭包需要读最新行数
          blameLinesRef.current = lines;
          setBlameLines(lines);
        }
      })
      .catch(() => {
        if (!cancelled) {
          blameLinesRef.current = [];
          setBlameLines([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [blameOn, repoPath, filePath, isImage, loadError]);

  // gutter 变更条：HEAD ↔ 工作区行级差异，编辑内容变化后防抖刷新
  useEffect(() => {
    let cancelled = false;
    if (!repoPath || isImage || loadError) {
      decoRef.current?.clear();
      return;
    }
    const timer = setTimeout(() => {
      invokeCommand<FileChangeHunk[]>('git_file_change_hunks', { repoPath, path: filePath })
        .then(hunks => {
          if (cancelled) return;
          const monaco = monacoRef.current;
          if (!monaco || !decoRef.current) return;
          const colors: Record<string, string> = { added: 'var(--git-green)', modified: 'var(--git-blue)', removed: 'var(--git-del)' };
          const decs = hunks.map(h => ({
            range: new monaco.Range(h.startLine, 1, h.endLine, 1),
            options: {
              isWholeLine: true,
              className: h.kind === 'added' ? 'py-line-added' : h.kind === 'modified' ? 'py-line-modified' : undefined,
              linesDecorationsClassName: `py-stripe-${h.kind}`,
              overviewRuler: { color: colors[h.kind] ?? '#888', position: 4 }
            }
          }));
          decoRef.current.set(decs);
        })
        .catch(() => {
          decoRef.current?.clear();
        });
    }, 500);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [repoPath, filePath, code, isImage, loadError]);

  /**
   * 用 monaco 自己的裁剪结果算 blame 渲染窗口，避免自己按 scrollTop 推算
   * 与编辑器实际渲染行错位（folding / 行高变化都会让线性换算失真）。
   */
  const syncVisibleRange = () => {
    const editor = editorRef.current;
    const total = blameLinesRef.current.length;
    if (!editor || total === 0) return;
    const ranges = editor.getVisibleRanges();
    if (ranges.length === 0) return;
    let start = ranges[0].startLineNumber;
    let end = ranges[0].endLineNumber;
    for (const r of ranges) {
      if (r.startLineNumber < start) start = r.startLineNumber;
      if (r.endLineNumber > end) end = r.endLineNumber;
    }
    const next: BlameVisibleRange = {
      start: Math.max(1, start - BLAME_OVERSCAN),
      end: Math.min(total, end + BLAME_OVERSCAN)
    };
    // 与当前窗口一致时跳过 setState，否则滚动会持续触发无意义重渲染
    setVisibleRange(prev => (prev.start === next.start && prev.end === next.end ? prev : next));
  };

  // blame 数据到位 / 开关切换时重算窗口：blame 是异步到达的，此时编辑器可能已滚动
  useEffect(() => {
    if (blameOn && blameLines.length > 0) {
      syncVisibleRange();
    }
  }, [blameLines, blameOn]);

  // 组件卸载时释放所有 onDid* 监听：wrapper 只在 ready 时调用一次 onMount 且不消费其返回值
  useEffect(
    () => () => {
      for (const d of disposablesRef.current) d.dispose();
      disposablesRef.current = [];
      editorRef.current = null;
      decoRef.current = null;
    },
    []
  );
  useEffect(() => {
    return () => {
      void monacoLspBridge.notifyDocClose(filePath);
    };
  }, [filePath]);

  // 只有 fontSize / folding 随 prop 变化；useMemo 保证对象标识稳定，
  // 否则 wrapper 的 options effect 每次 render 都触发全量 updateOptions
  const editorOptions = useMemo<monacoEditorNs.IStandaloneEditorConstructionOptions>(
    () => ({ ...EDITOR_OPTIONS_BASE, fontSize, folding: !blameOn }),
    [fontSize, blameOn]
  );

  // blame 虚拟化：只渲染可视区间 ± 缓冲行，千行文件不再一次性生成上千个 DOM 节点
  const visibleBlameLines = blameOn
    ? blameLines.filter(b => b.line >= visibleRange.start && b.line <= visibleRange.end)
    : [];

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      {/* Breadcrumbs */}
      <div
        style={{
          height: '24px',
          backgroundColor: 'var(--overlay-faint)',
          borderBottom: '1px solid var(--border-subtle)',
          display: 'flex',
          alignItems: 'center',
          padding: '0 12px',
          gap: '6px',
          fontSize: '11px',
          color: 'var(--text-muted)',
          flexShrink: 0
        }}
      >
        {parts.map((p, idx) => (
          <React.Fragment key={idx}>
            <span style={{ color: idx === parts.length - 1 ? 'var(--text-primary)' : undefined }}>{p}</span>
            {idx < parts.length - 1 && <span>›</span>}
          </React.Fragment>
        ))}
        {/* Git 注释工具栏：文件历史 + 行级 blame */}
        {!isImage && !loadError && (onShowHistory || (repoPath && onToggleBlame)) && (
          <span style={{ marginLeft: 'auto', display: 'flex', gap: '4px' }}>
            {onShowHistory && (
              <button className="editor-git-btn" onClick={onShowHistory} title={t().editor.showHistory}>
                <IconHistory size={12} />
                {t().editor.history}
              </button>
            )}
            {repoPath && onToggleBlame && (
              <button
                className={`editor-git-btn ${blameOn ? 'active' : ''}`}
                onClick={onToggleBlame}
                title={t().editor.annotateBlame}
              >
                <IconAnnotate size={12} />
                Annotate
              </button>
            )}
          </span>
        )}
        {isImage && imageSize && (
          <span style={{ marginLeft: 'auto', color: 'var(--text-muted)' }}>
            {imageSize.width} × {imageSize.height} px
          </span>
        )}
      </div>

      {/* Main Canvas: Image Preview or Monaco Editor Canvas */}
      {isImage ? (
        <div
          style={{
            flex: 1,
            minHeight: 0,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            backgroundColor: 'var(--bg-app-deep)',
            position: 'relative',
            overflow: 'auto',
            padding: '24px'
          }}
        >
          {/* Zoom Toolbar */}
          <div
            style={{
              position: 'absolute',
              top: '12px',
              right: '12px',
              display: 'flex',
              gap: '4px',
              background: 'rgba(30, 31, 34, 0.85)',
              padding: '4px 8px',
              borderRadius: '6px',
              border: '1px solid #393b40',
              zIndex: 10
            }}
          >
            <button
              className="tb-icon-btn"
              onClick={() => setZoom(z => Math.max(0.2, z - 0.2))}
              title={t().editor.zoomOut}
            >
              −
            </button>
            <span style={{ fontSize: '11px', color: 'var(--text-primary)', padding: '2px 4px', minWidth: '40px', textAlign: 'center' }}>
              {Math.round(zoom * 100)}%
            </span>
            <button
              className="tb-icon-btn"
              onClick={() => setZoom(z => Math.min(5, z + 0.2))}
              title={t().editor.zoomIn}
            >
              +
            </button>
            <button
              className="tb-icon-btn"
              onClick={() => setZoom(1)}
              title={t().editor.zoomReset}
            >
              1:1
            </button>
          </div>

          {/* Checkerboard Image Container */}
          <div
            style={{
              backgroundImage:
                'linear-gradient(45deg, var(--overlay-hover) 25%, transparent 25%), linear-gradient(-45deg, var(--overlay-hover) 25%, transparent 25%), linear-gradient(45deg, transparent 75%, var(--overlay-hover) 75%), linear-gradient(-45deg, transparent 75%, var(--overlay-hover) 75%)',
              backgroundSize: '16px 16px',
              backgroundPosition: '0 0, 0 8px, 8px -8px, -8px 0px',
              backgroundColor: 'var(--bg-app)',
              border: '1px solid var(--border-color)',
              borderRadius: 'var(--radius-btn)',
              padding: '16px',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              boxShadow: 'var(--shadow-popup)'
            }}
          >
            {imageError ? (
              <div className="editor-error-panel">
                <div className="editor-error-icon">⚠</div>
                <div className="editor-error-title">{t().editor.cannotOpenFile}</div>
                <div className="editor-error-detail">{imageError}</div>
              </div>
            ) : imageSrc ? (
            <img
              src={imageSrc}
              alt={fileName}
              style={{
                maxWidth: '100%',
                maxHeight: 'calc(100vh - 200px)',
                transform: `scale(${zoom})`,
                transformOrigin: 'center center',
                transition: 'transform 0.1s ease',
                display: 'block'
              }}
              onLoad={e => {
                const img = e.currentTarget;
                setImageSize({ width: img.naturalWidth, height: img.naturalHeight });
              }}
            />
            ) : null}
          </div>
        </div>
      ) : loadError ? (
        <div className="editor-error-panel">
          <div className="editor-error-icon">⚠</div>
          <div className="editor-error-title">{t().editor.cannotOpenFile}</div>
          <div className="editor-error-detail">{loadError}</div>
          {loadErrorCode === 'file_too_large' && (
            <div className="editor-error-actions">
              <button type="button" onClick={() => onRevealFile?.(filePath)}>
                {t().editor.revealInFinder}
              </button>
              <button type="button" onClick={() => onForceOpen?.(filePath)}>
                {t().editor.forceOpen}
              </button>
            </div>
          )}
        </div>
      ) : (
        <div style={{ flex: 1, minHeight: 0, display: 'flex', position: 'relative' }}>
          {/* Git blame 侧列：只渲染可视区间内的行，逐行与 Monaco 行号对齐 */}
          {blameOn && blameLines.length > 0 && (
            <div className="blame-gutter" style={{ position: 'relative' }}>
              {/* 占位撑出行号↔行高映射：外层 overflow:hidden 不产生滚动，仅用于定位 */}
              <div
                aria-hidden
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  right: 0,
                  height: `${blameLines.length * EDITOR_LINE_HEIGHT}px`,
                  pointerEvents: 'none'
                }}
              />
              {visibleBlameLines.map(b => (
                <div
                  key={b.line}
                  className={`blame-row ${b.author === 'Not Committed Yet' ? 'uncommitted' : ''}`}
                  style={{
                    position: 'absolute',
                    top: `${(b.line - 1) * EDITOR_LINE_HEIGHT}px`,
                    left: 0,
                    right: 0,
                    height: `${EDITOR_LINE_HEIGHT}px`,
                    lineHeight: `${EDITOR_LINE_HEIGHT}px`
                  }}
                  title={t().editor.blameTooltip(b.shortHash, b.author, b.date)}
                  onClick={() => onOpenCommitDiff?.(b.hash)}
                >
                  <span className="blame-author">{b.author}</span>
                  <span className="blame-date">{b.date}</span>
                  <span className="blame-hash">{b.shortHash}</span>
                </div>
              ))}
            </div>
          )}
          <div
            style={{ flex: 1, minWidth: 0 }}
            onContextMenu={e => {
              // Monaco 默认右键已被 main.tsx 的白名单排除（不会弹原生菜单），
              // 这里换成本项目的中文 PyCharm 风格菜单。
              e.preventDefault();
              e.stopPropagation();
              // 命中的是空白区（非标识符）时禁用重构类项
              const word = editorRef.current?.getModel()?.getWordAtPosition(
                editorRef.current.getPosition() ?? { lineNumber: 1, column: 1 }
              );
              setEditorMenu({ x: e.clientX, y: e.clientY, hasSymbol: !!word });
            }}
          >
            <Editor
              height="100%"
              path={filePath}
              language={language}
              value={code}
              theme={MONACO_THEME_BY_APP_THEME[themeId] ?? 'ide-jetbrains-dark'}
              onChange={(val) => {
                onChange?.(val);
                if (val !== undefined) void monacoLspBridge.notifyDocChange(filePath, language, val);
              }}
              onMount={(editor, monaco: Monaco) => {
                editorRef.current = editor;
                monacoRef.current = monaco;
                decoRef.current = editor.createDecorationsCollection();
                monacoLspBridge.registerLanguageProviders(monaco as any, language);
                void monacoLspBridge.notifyDocOpen(filePath, language, code);
                // 滚动 / 布局 / 换模型都要重算 blame 窗口：blame 数据异步到达，
                // 打开文件后可能先滚动再拿到 blame，不监听就停在上次的位置
                const onScroll = () => {
                  // 隐藏 pane 不上报：它的 scrollTop 是别的文件残留，不是本文件位置
                  if (!editorActiveRef.current) return;
                  syncVisibleRange();
                  const maxScroll = Math.max(0, editor.getContentHeight() - editor.getLayoutInfo().height);
                  scrollChangeRef.current?.(maxScroll > 0 ? editor.getScrollTop() / maxScroll : 0);
                };
                disposablesRef.current.push(
                  editor.onDidScrollChange(onScroll),
                  editor.onDidLayoutChange(onScroll),
                  editor.onDidChangeModel(onScroll)
                );

                // 编辑器快捷键：CmdOrCtrl+L 跳转到行、CmdOrCtrl+R 重命名符号。
                // Monaco 有内置 action（editor.action.gotoLine / editor.action.rename）
                // 但默认不绑键，这里只做绑定而不重复实现。
                // 注意 addCommand 的回调必须是空参：命令系统不传参数。
                editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyL, () => {
                  editor.getAction('editor.action.gotoLine')?.run();
                });
                editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyR, () => {
                  editor.getAction('editor.action.rename')?.run();
                });
                editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyG, () => {
                  // Cmd+G 在 macOS Safari 语义是「再次查找」：复用 nextMatch
                  editor.getAction('editor.action.nextMatchFindAction')?.run();
                });
                // Cmd+S 保存：编辑器聚焦时全局键盘分支会跳过（Monaco 的输入面是
                // textarea），必须在这里自己绑一路，否则焦点在编辑器里按 ⌘S 静默无效
                editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
                  onSaveRef.current?.();
                });
                // Option+Cmd+L (macOS) / Ctrl+Alt+L (Windows) 对齐 PyCharm 格式化当前文件
                editor.addCommand(monaco.KeyMod.Alt | monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyL, () => {
                  editor.getAction('editor.action.formatDocument')?.run();
                });

                if (gotoLine && gotoLine > 0) {
                  editor.revealLineInCenter(gotoLine);
                  editor.setPosition({ lineNumber: gotoLine, column: 1 });
                  editor.focus();
                }
                const model = editor.getModel();
                if (model) {
                  onTabSizeChange?.(model.getOptions().tabSize);
                  disposablesRef.current.push(
                    model.onDidChangeOptions(() => onTabSizeChange?.(model.getOptions().tabSize))
                  );
                }
                const position = editor.getPosition();
                if (position) {
                  onCursorChange?.({ line: position.lineNumber, column: position.column });
                }
                disposablesRef.current.push(
                  editor.onDidChangeCursorPosition(e => {
                    onCursorChange?.({ line: e.position.lineNumber, column: e.position.column });
                  })
                );
              }}
              options={editorOptions}
            />
          </div>
        </div>
      )}

      <EditorContextMenu
        menu={editorMenu}
        onClose={() => setEditorMenu(null)}
        runAction={id => void editorRef.current?.getAction(id)?.run()}
        onCopy={() => {
          // Monaco 的 copy action 写的是它的内部选区；无选区时回退到浏览器默认
          const editor = editorRef.current;
          if (editor && editor.getModel()?.getValueInRange(editor.getSelection() ?? { startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 1 })) {
            void editor.getAction('editor.action.clipboardCopyAction')?.run();
          } else {
            document.execCommand('copy');
          }
        }}
        onShowHistory={onShowHistory}
        onAnnotate={onToggleBlame}
        showGit={!!repoPath}
      />
    </div>
  );
};
