import ReactMarkdown, { type Components } from 'react-markdown';
import React, { useEffect, useRef } from 'react';
import rehypeHighlight from 'rehype-highlight';
import remarkGfm from 'remark-gfm';
import { invokeCommand, isTauriEnvironment } from '../services/tauriBridge';
import { t } from '../services/i18n';
import { useLanguageRefresh } from '../hooks/useLanguageRefresh';
import { resolveMarkdownPath } from '../services/markdownLinks';

interface MarkdownPreviewProps {
  filePath: string;
  content: string;
  onOpenFile?: (path: string) => void;
  active?: boolean;
  scrollRatio?: number;
  onScrollChange?: (ratio: number) => void;
  /** 工作区根：链接与图片只允许落在其内，防住 `..` 逃逸到项目外读任意文件 */
  workspaceRoot?: string;
}

function resolveLocalImagePath(filePath: string, src: string | undefined, workspaceRoot?: string): string | null {
  if (!src || src.startsWith('data:') || src.startsWith('blob:')) return null;
  const localPath = resolveMarkdownPath(filePath, src, workspaceRoot);
  if (!localPath || !isTauriEnvironment()) return null;
  return localPath;
}

/** 远程图床/协议相对地址：命中即走点击加载，不在渲染时自动请求。 */
const REMOTE_SRC = /^(?:https?:)?\/\//i;

/**
 * 工作区本地图片：经 `read_image_bytes` 读 base64 走 data URL（P2-asset，
 * 与 EditorView 同因：不再经过 asset:// 协议）。
 * data:/blob: 与浏览器预览环境直解原文；加载失败显示 alt，不抛错打断整篇。
 */
function LocalImage({ path, alt }: { path: string; alt?: string }) {
  const [dataUrl, setDataUrl] = React.useState<string | null>(null);
  const [failed, setFailed] = React.useState(false);
  React.useEffect(() => {
    let cancelled = false;
    setDataUrl(null);
    setFailed(false);
    void invokeCommand<{ mime: string; base64: string; size: number }>('read_image_bytes', { path })
      .then(dto => {
        if (!cancelled) setDataUrl(`data:${dto.mime};base64,${dto.base64}`);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => { cancelled = true; };
  }, [path]);
  if (failed) return <span title={path}>{alt ?? ''}</span>;
  if (!dataUrl) return <span className="markdown-remote-image">{alt ?? ''}</span>;
  return <img src={dataUrl} alt={alt ?? ''} />;
}

/**
 * 外部图片点击加载（P0 附带：隐私）。
 *
 * 渲染时自动请求远程图片等于向图床泄露 IP + 阅读行为，与 local-first 承诺
 * 冲突（不可信仓库的 README 里埋 1px 追踪像素是已知手法）。默认渲染占位
 * 按钮，用户点一次才真正请求；本地图片不受影响（仍走 convertFileSrc）。
 */
function RemoteImage({ src, alt }: { src: string; alt?: string }) {
  useLanguageRefresh();
  const [allowed, setAllowed] = React.useState(false);
  if (!allowed) {
    let host = src;
    try {
      host = new URL(src.startsWith('//') ? `https:${src}` : src).host || src;
    } catch {
      // 畸形 src：host 回落原文，用户点了之后由 <img> 按原逻辑失败
    }
    return (
      <button
        type="button"
        className="markdown-remote-image"
        title={t().markdownView.remoteImageHint(host)}
        onClick={() => setAllowed(true)}
      >
        {t().markdownView.loadRemoteImage}
      </button>
    );
  }
  return <img src={src} alt={alt ?? ''} />;
}

/**
 * 整篇 Markdown 由 react-markdown 同步解析 + rehype-highlight 高亮，代价随文档长度线性上升。
 * App 是无 memo 的单壳层：每次击键 setBuffers、每个滚动比例 setState 都会重渲染这棵树，
 * 不隔离的话 README/CHANGELOG 这类长文档会在无关状态变化时整篇重解析。
 * 生效前提是 App 侧传下来的回调（onOpenFile / onScrollChange）保持稳定引用，
 * 否则浅比较每帧都不等 —— 该稳定化在 App 侧。
 */
export const MarkdownPreview = React.memo(function MarkdownPreview({
  filePath,
  content,
  onOpenFile,
  active = true,
  scrollRatio = 0,
  onScrollChange,
  workspaceRoot
}: MarkdownPreviewProps) {
  useLanguageRefresh();
  const previewRef = useRef<HTMLDivElement>(null);
  /** 滚动容器（外层 pane）可用时缓存在这，供效果与原生监听共用 */
  const paneRef = useRef<HTMLElement | null>(null);
  const scrollEl = (): HTMLElement | null =>
    paneRef.current ?? previewRef.current?.closest<HTMLElement>('.markdown-preview-pane') ?? previewRef.current;

  // 用户正在滚动的标志：用户滚动期间（包含惯性滚动）绝不反向程序化写入 scrollTop，彻底根治滚太快时的撕裂/闪烁
  const isUserScrollingRef = useRef(false);
  const userScrollTimeoutRef = useRef<number | null>(null);

  /**
   * 内容层自身滚动的兜底（无外层 pane 时）。有外层 pane 时滚动发生在 pane 上，
   * 内容层不会收到 scroll 事件，走这条也不会有副作用。
   */
  const handleFallbackScroll = () => {
    const element = previewRef.current;
    if (!element || element.classList.contains('markdown-preview') === false) return;
    if (element.closest('.markdown-preview-pane')) return; // 交给 pane 上的原生监听
    if (!active) return;
    isUserScrollingRef.current = true;
    if (userScrollTimeoutRef.current !== null) {
      window.clearTimeout(userScrollTimeoutRef.current);
    }
    userScrollTimeoutRef.current = window.setTimeout(() => {
      isUserScrollingRef.current = false;
      userScrollTimeoutRef.current = null;
    }, 200);

    const maxScroll = Math.max(0, element.scrollHeight - element.clientHeight);
    onScrollChange?.(maxScroll > 0 ? element.scrollTop / maxScroll : 0);
  };

  useEffect(() => {
    if (!active || isUserScrollingRef.current) return;
    const element = scrollEl();
    if (!element) return;
    const maxScroll = Math.max(0, element.scrollHeight - element.clientHeight);
    if (maxScroll <= 0) return;
    const targetScroll = maxScroll * Math.max(0, Math.min(1, scrollRatio));
    // 仅在当前滚动位置与目标相差较大（如切换文件、切换分栏）时才程序化赋值，消除微小浮点误差引起的抖动
    if (Math.abs(element.scrollTop - targetScroll) > 8) {
      element.scrollTop = targetScroll;
    }
  }, [active, scrollRatio, filePath]);

  // scroll 事件不冒泡：绑在内容层的 React onScroll/onScrollCapture 都收不到外层
  // pane 的滚动。有外层 pane 就用原生 addEventListener 挂在 pane 上；
  // 组件被单独使用（无外层 pane、内容层自己 overflow）时保留 React onScroll。
  useEffect(() => {
    const pane = previewRef.current?.closest<HTMLElement>('.markdown-preview-pane');
    if (!pane) return;
    paneRef.current = pane;
    const onScroll = () => {
      if (!active) return;
      isUserScrollingRef.current = true;
      if (userScrollTimeoutRef.current !== null) {
        window.clearTimeout(userScrollTimeoutRef.current);
      }
      userScrollTimeoutRef.current = window.setTimeout(() => {
        isUserScrollingRef.current = false;
        userScrollTimeoutRef.current = null;
      }, 200);

      const maxScroll = Math.max(0, pane.scrollHeight - pane.clientHeight);
      onScrollChange?.(maxScroll > 0 ? pane.scrollTop / maxScroll : 0);
    };
    pane.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      pane.removeEventListener('scroll', onScroll);
      if (paneRef.current === pane) paneRef.current = null;
      if (userScrollTimeoutRef.current !== null) {
        window.clearTimeout(userScrollTimeoutRef.current);
        userScrollTimeoutRef.current = null;
      }
    };
  }, [active, filePath, onScrollChange]);

  const components: Components = React.useMemo(() => ({
    a({ node, href, children, ...props }) {
      void node;
      return (
        <a
          {...props}
          href={href}
          rel="noreferrer"
          onClick={event => {
            const localPath = href ? resolveMarkdownPath(filePath, href, workspaceRoot) : null;
            if (localPath && onOpenFile) {
              event.preventDefault();
              onOpenFile(localPath);
            }
          }}
        >
          {children}
        </a>
      );
    },
    img({ node, src, alt, ...props }) {
      void node;
      // 远程地址先占位、用户点击后才加载（见 RemoteImage）。
      if (src && REMOTE_SRC.test(src)) return <RemoteImage src={src} alt={alt} />;
      // data:/blob: 原样直解；本地路径经 read_image_bytes 异步加载。
      if (!src || src.startsWith('data:') || src.startsWith('blob:')) {
        return <img {...props} src={src} alt={alt ?? ''} />;
      }
      const localPath = resolveLocalImagePath(filePath, src, workspaceRoot);
      if (!localPath) return <img {...props} src={src} alt={alt ?? ''} />;
      return <LocalImage path={localPath} alt={alt} />;
    }
  }), [filePath, workspaceRoot, onOpenFile]);

  const renderedMarkdown = React.useMemo(() => (
    <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]} components={components}>
      {content}
    </ReactMarkdown>
  ), [content, components]);

  return (
    <div
      ref={previewRef}
      className="markdown-preview"
      data-tauri-drag-region="false"
      onScroll={handleFallbackScroll}
    >
      {renderedMarkdown}
    </div>
  );
});
