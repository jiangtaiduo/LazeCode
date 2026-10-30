import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { ConflictStandaloneApp } from './components/ConflictStandaloneApp';
import { MergeStandaloneApp } from './components/MergeStandaloneApp';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { DEFAULT_SETTINGS, loadSettings } from './services/appSettings';
import { initLanguage, t } from './services/i18n';
import { invokeCommand, isTauriEnvironment } from './services/tauriBridge';
import { loadJsonArray, saveJsonArray, TELEMETRY_EVENTS } from './services/historyStore';
import { configureTelemetryStore } from './services/telemetry';

// 遥测持久化接线（local-first：只存本机 historyStore，不上传）。
// configure 把启动早期攒的事件搬过去，不丢。失败只影响遥测，不影响启动。
configureTelemetryStore({
  load: () => loadJsonArray(TELEMETRY_EVENTS),
  save: (events) => saveJsonArray(TELEMETRY_EVENTS, events),
});

/**
 * variant 样式按需注入。
 *
 * 界面风格曾全部内联在 theme.css 里：构建期全部产出、运行时只生效一套，
 * 既浪费包体又让规则互相覆盖。现在每个 variant 独立成文件，
 * 这里只挂载当前 variant 对应的 <link>。
 *
 * import.meta.glob + query:'?url' 让 Vite 在构建期把每个 CSS 文件单独产出
 * 为带 hash 的静态资源，并把最终 URL 交给我们，因此这里不出现硬编码路径。
 * eager:false 表示不在主 chunk 里预取全部 URL，首次用到某个 variant 时才加载。
 */
const VARIANT_URLS = import.meta.glob('/src/styles/variants/variant-*.css', {
  eager: false,
  query: '?url',
  import: 'default'
}) as Record<string, () => Promise<string>>;

/** 当前挂载的 <link>；切换 variant 时先摘除再插入，避免两套主题同时生效 */
let currentLink: HTMLLinkElement | null = null;
/** 当前已生效的 variant，重复注入同一 variant 时直接复用；0 表示尚未挂载 */
let currentVariant = 0;
/** 同一 variant 的并发调用共享同一个 promise，避免重复插入 */
let pending: Promise<void> | null = null;

/** 归一化 variant 号：设置里可能缺省、可能是字符串、可能是越界值。
 *  只保留 2 套界面风格：1 = macOS Native 深色，2 = macOS Native 浅色。
 *  旧设置里可能是 3..10（已删除的旧 PyCharm/VS Code/Nord… 系列），
 *  一律收敛到 1，否则会去请求不存在的样式文件。 */
function normalizeVariant(variant: unknown): number {
  const n = typeof variant === 'number' ? variant : Number(variant);
  return n === 2 ? 2 : 1;
}

/** 挂载指定 variant 的样式表；幂等，同一 variant 重复调用不会重复插入 */
export function applyThemeVariant(variant: unknown): Promise<void> {
  const target = normalizeVariant(variant);

  if (currentVariant === target && currentLink?.isConnected) {
    return Promise.resolve();
  }
  // 并发切到同一个 variant 时复用同一次加载
  if (pending && currentVariant === target) {
    return pending;
  }

  const loader = VARIANT_URLS[`/src/styles/variants/variant-${target}.css`];
  if (!loader) {
    // 取值已被 normalizeVariant 收敛在 1..2，走到这里说明样式文件缺失
    return Promise.reject(new Error(t().errorPage.variantMissing(target)));
  }

  currentVariant = target;
  pending = loader()
    .then((href) => {
      if (currentVariant !== target) {
        // 加载期间又被切走，本次结果作废
        return;
      }
      currentLink?.remove();
      const link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = href;
      link.dataset.themeVariant = String(target);
      // 插到最前：让 variant 的变量覆盖 theme.css，而不是被它反压
      document.head.prepend(link);
      currentLink = link;
    })
    .catch((err) => {
      if (currentVariant === target) {
        // 失败后允许下次重试
        currentVariant = 0;
      }
      throw err;
    })
    .finally(() => {
      pending = null;
    });

  return pending;
}

/**
 * 空白区域屏蔽 WKWebView 原生右键菜单。
 *
 * macOS 上 wry 没有关闭默认菜单的入口：`with_default_context_menus`
 * 是 wry 的 `#[cfg(windows)]` trait 成员（wry-0.55.1/src/lib.rs:1700），
 * 只映射 WebView2；Tauri v2 配置文件也没有对应 schema key。因此
 * 纯前端收口是唯一手段。
 *
 * 白名单保留真正有用的原生菜单：
 *   - .monaco-editor        Monaco 的 Cut/Copy/Paste 菜单
 *   - .terminal-canvas-host 终端画布（Cmd+C 走 textarea 的 copy 事件）
 *   - input/textarea/select/[contenteditable] 表单控件的原生编辑菜单
 * 未命中白名单的右键一律无原生菜单；业务菜单（文件树、tab、分支、提交…）
 * 不受影响，见下方监听器的说明。
 */
let lastObservedVariant = 0;
new MutationObserver(() => {
  const variant = Number(document.body.dataset.variant);
  if (variant === lastObservedVariant) return;
  lastObservedVariant = variant;
  void applyThemeVariant(variant).catch((err) => {
    console.error(t().errorPage.applyVariantFailed, err);
    // 回滚观察值，下次属性变化可重试
    lastObservedVariant = 0;
  });
}).observe(document.body, { attributes: true, attributeFilter: ['data-variant'] });

// 首屏兜底：脚本早于 App 首帧执行、body 上还没有 data-variant 时，
// 先挂载默认 variant（DEFAULT_SETTINGS.uiVariant），observer 会在 App 写入真实值时纠正。
if (!document.body.dataset.variant) {
  lastObservedVariant = DEFAULT_SETTINGS.uiVariant;
  void applyThemeVariant(DEFAULT_SETTINGS.uiVariant);
}

/**
 * 白名单保留真正有用的原生菜单：
 *   - .terminal-canvas-host 终端画布（Cmd+C 走 textarea 的 copy 事件）
 *   - input/textarea/select/[contenteditable] 表单控件的原生编辑菜单
 *
 * `.monaco-editor` **不在**白名单里：Monaco 的默认右键是英文 Web 风格菜单，
 * 与本项目的中文 PyCharm 风格割裂，且不含任何 Git 语义。改由 EditorView 挂
 * `CtxMenuShell` 提供（Go to Definition / Rename / Cut / Copy / Paste /
 * Show History / Annotate / Compare with…）。
 *
 * `[contenteditable="true"]` 之外不再补 `[contenteditable=""]` 与
 * `plaintext-only`：本项目没有这两种 contenteditable 节点，写了也是死规则。
 */
const NATIVE_CONTEXT_MENU_ALLOW =
  '.terminal-canvas-host, input, textarea, select, [contenteditable="true"]';

/**
 * capture 阶段判定：命中白名单直接放行，否则 preventDefault 让
 * WKWebView 无从弹原生菜单。
 *
 * 只调 preventDefault，绝不能 stopPropagation：业务菜单（文件树 / tab /
 * 分支 / 提交…）的 onContextMenu 由 React 委托在 root 容器上冒泡派发。
 * capture 阶段 stopPropagation 会让事件到不了 root，React 合成事件无从
 * 触发，业务菜单全部失效 —— 这是实测踩过的坑。
 *
 * 事件继续冒泡，业务侧自己的 onContextMenu 与 state 更新完全不受影响
 * （它们自带的 preventDefault 在这里变成冗余但无害的第二次）。
 */
window.addEventListener(
  'contextmenu',
  (event) => {
    const target = event.target;
    if (target instanceof Element && target.closest(NATIVE_CONTEXT_MENU_ALLOW)) return;
    event.preventDefault();
  },
  true
);

/**
 * 外部链接点击拦截（P0 第一道，与 Rust `on_navigation` 互为纵深）。
 *
 * 背景：Markdown 预览里的外链若走默认行为，webview 会整页导航到远程站点，
 * 远程 JS 将继承全部 IPC 命令（含 pty_write = 以用户身份执行 shell）。
 * 这里在 capture 阶段把带 scheme 的链接（http/https/mailto/…）与协议相对
 * 链接（//host）全部拦下，送 `open_external_url` 由系统浏览器打开 ——
 * scheme 白名单的最终判定在 Rust 侧（validate_external_url）。
 *
 * 只调 preventDefault、不 stopPropagation（与上面 contextmenu 同一条铁律）：
 * 本地文件链接（无 scheme）不受影响，照常冒泡给 React onClick 处理。
 * 浏览器预览环境（npm run dev）保持默认行为，方便联调。
 */
const EXTERNAL_HREF = /^(?:[a-z][a-z\d+.-]*:|\/\/)/i;

window.addEventListener(
  'click',
  (event) => {
    if (!isTauriEnvironment()) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const anchor = target.closest('a[href]');
    if (!anchor) return;
    const raw = anchor.getAttribute('href') ?? '';
    if (!EXTERNAL_HREF.test(raw)) return;
    // 协议相对（//host/path）按 https 补全再下发；其余原样下发，
    // 非白名单 scheme 由 Rust 侧拒绝（静默丢弃，不回显畸形 URL）。
    const url = raw.startsWith('//') ? `https:${raw}` : raw;
    event.preventDefault();
    void invokeCommand('open_external_url', { url }).catch(() => {
      // invokeCommand 已记 IPC 日志；界面上不弹 toast —— 用户点的是
      // 不可信内容里的链接，静默丢弃比把畸形 URL 回显出来更安全。
    });
  },
  true
);

/** 「恢复」按钮的最大重试次数：超过后改为整页 reload（见 ErrorBoundary.reset） */
const MAX_RESET_ATTEMPTS = 2;

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean; error: Error | null }
> {
  /** 已尝试过的恢复次数（组件实例级，不持久化：刷新即重新计数） */
  private resetAttempts = 0;
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('Unhandled React Error:', error, errorInfo);
  }

  /** 恢复渲染前清掉可能致病的持久化状态：错误若来自损坏的 localStorage
   *  （如 recentWorkspaces 存成对象导致 filter 抛 TypeError），只重置
   *  hasError 会让用户反复撞同一堵墙。
   *
   *  但有重试上限：错误若来自组件树内的代码路径（与持久化无关），
   *  setState(hasError:false) 后会再次抛错 → 又回到错误页。无上限时
   *  用户点多少次都回到同一页，唯一的出路是重启 app。超过上限改为
   *   reload()：让整个前端（含模块状态）重新初始化，相当于不丢磁盘数据
   *  的「软重启」。 */
  private reset = () => {
    if (this.resetAttempts >= MAX_RESET_ATTEMPTS) {
      window.location.reload();
      return;
    }
    this.resetAttempts += 1;
    try {
      // 只清历史/缓存命名空间（最可能因损坏 JSON 导致渲染异常），
      // 保留 lazecode.settings、项目记录与面板尺寸，避免一次渲染错误把用户
      // 的偏好/最近项目全抹掉。
      const keys = Object.keys(localStorage).filter(k => k.startsWith('lazecode.history.v1.'));
      keys.forEach(k => localStorage.removeItem(k));
    } catch {
      /* 隐私模式下 localStorage 抛异常：忽略，仍尝试重新渲染 */
    }
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      return (
        <div
          style={{
            height: '100vh',
            width: '100vw',
            backgroundColor: '#1e1f22',
            color: '#dfe1e5',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            padding: '32px',
            fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif'
          }}
        >
          <div style={{ fontSize: '32px', marginBottom: '16px' }}>⚠️</div>
          <h2 style={{ fontSize: '16px', fontWeight: 600, color: '#f85149', marginBottom: '8px' }}>
            {t().errorPage.title}
          </h2>
          <div
            style={{
              maxWidth: '600px',
              backgroundColor: '#2b2d30',
              padding: '12px 16px',
              borderRadius: '6px',
              fontSize: '12px',
              color: '#bcbec4',
              fontFamily: 'monospace',
              marginBottom: '16px',
              wordBreak: 'break-all'
            }}
          >
            {this.state.error?.message || t().errorPage.unknownError}
          </div>
          <button
            onClick={this.reset}
            style={{
              backgroundColor: '#3574f0',
              color: '#fff',
              border: 'none',
              padding: '6px 16px',
              borderRadius: '4px',
              fontSize: '13px',
              cursor: 'pointer'
            }}
          >
            {this.resetAttempts >= MAX_RESET_ATTEMPTS
              ? t().errorPage.reload
              : t().errorPage.recover}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

// 不使用 StrictMode：
// 本应用的核心组件在 effect 中获取「非幂等的原生资源」
// （libghostty-vt 终端句柄 + PTY 会话）。StrictMode 的 effect 双调用
// 会让两份实例争用同一块 wasm 线性内存，引发难以定位的静默数据错乱。
// 该保护在开发期提供的是「重复渲染安全」检查，此处收益为负。
const windowLabel = isTauriEnvironment() ? getCurrentWindow().label : 'main';
const isConflictWindow = windowLabel === 'win_conflicts';
const isMergeWindow = windowLabel === 'win_merge_tool';

// 首帧之前同步词典，避免中文用户启动时先闪一帧英文。
initLanguage(loadSettings().language);

ReactDOM.createRoot(document.getElementById('root')!).render(
  <ErrorBoundary>
    {isConflictWindow ? (
      <ConflictStandaloneApp />
    ) : isMergeWindow ? (
      <MergeStandaloneApp />
    ) : (
      <App />
    )}
  </ErrorBoundary>
);
