// 应用设置的存储层
//
// 与 SettingsDialog.tsx 分离：App 需要在挂载时同步读取设置来定主题、
// 交通灯样式与终端字号，不能等弹窗组件加载完。拆开后存储层小且无
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { isTauriEnvironment } from './tauriBridge';
// 语言归一化只有一份实现（i18n/index.ts）。这里 import 而非复制：
// 两处规则漂移会让设置面板显示的语言与实际查表用的语言不一致。
import { normalizeLanguage } from './i18n';
// 分层合并是纯逻辑（settingsLayers 零运行时 import，无循环依赖）：
// 本文件提供单源默认值 + 语言归一，组装成 deps 喂进去。
import { resolveSettings, parseProjectSettingsText, type SettingsDeps } from './settingsLayers';

import { DEFAULT_SETTINGS, type AppSettings } from './settingsDefaults';
export { DEFAULT_SETTINGS };
export type { AppSettings };

const SETTINGS_KEY = 'lazecode.settings';

const DEPS: SettingsDeps = { defaults: DEFAULT_SETTINGS, normalizeLanguage };

/**
 * 项目层缓存（`.lazecode/settings.json` 的有效项，见 settingsLayers）。
 *
 * 存在的理由：`loadSettings()` 是同步的（App 挂载时就要主题，不能等文件 IO），
 * 而项目文件是异步读的。App 在打开项目后调 `applyProjectSettingsText` 填缓存，
 * 之后每次 `loadSettings()` 自动叠加项目层；关闭项目时 `clearProjectLayer()`。
 */
let projectLayer: Partial<AppSettings> = {};

export function getProjectLayer(): Partial<AppSettings> {
  return { ...projectLayer };
}

export function clearProjectLayer(): void {
  projectLayer = {};
}

/**
 * 填入项目层（已解析的有效项）。返回合并后的完整设置，调用方直接 setSettings。
 * 解析抛错时缓存不动（损坏的文件不能把上一份有效层顶掉），由调用方提示用户。
 */
export function applyProjectLayer(partial: Partial<AppSettings>): AppSettings {
  projectLayer = { ...partial };
  return loadSettings();
}

/**
 * 解析项目文件文本并填缓存（App 打开项目时调）：缺文件传 null（静默无层），
 * 损坏抛错（缓存保持上一份有效层，调用方提示用户修文件）。
 * 返回合并后的设置 + 非致命警告（未知 key / 非法值，调用方展示）。
 */
export function applyProjectSettingsText(text: string | null): { settings: AppSettings; warnings: string[] } {
  const { partial, warnings } = parseProjectSettingsText(text, DEPS);
  return { settings: applyProjectLayer(partial), warnings };
}

/**
 * 设置变更订阅者。
 *
 * 存在的理由：设置是 localStorage 里的全局状态，读它的组件（DiffViewer 等）
 * 每次渲染都 loadSettings() 会做一次读取 + JSON.parse；缓存快照就必须能在
 * 用户改设置后失效。这里只在"写入成功"这一个真实变更点上通知。
 */
const settingsSubscribers = new Set<(settings: AppSettings) => void>();

/** 订阅设置变更；返回退订函数。订阅者抛错不影响其它订阅者，也不影响写盘。 */
export function subscribeSettings(listener: (settings: AppSettings) => void): () => void {
  settingsSubscribers.add(listener);
  return () => {
    settingsSubscribers.delete(listener);
  };
}

/** 从 localStorage 读取设置并合并默认值（缺字段向前兼容） */
export function loadSettings(): AppSettings {
  let user: unknown;
  try {
    const raw = window.localStorage.getItem(SETTINGS_KEY);
    user = raw ? JSON.parse(raw) : undefined;
  } catch {
    user = undefined;
  }
  // user 层非法值静默回落（历史行为：损坏的设置文件不能把应用带崩）；
  // 项目层警告由读取文件处（App 打开项目）展示，这里只合并。
  // 旧版本的编辑器配色 id（1..11）与垃圾 language 值都由 schema 接住：
  // themeId>2 非法→默认深色，language 经 normalizeLanguage 归一。
  return resolveSettings({ user, project: projectLayer }, DEPS).values;
}

export function persistSettings(
  settings: AppSettings,
  changedKeys?: Array<keyof AppSettings>
): void {
  // 只写「本次真正改动」的 key：项目层覆盖 editorFontSize=14 时，用户层
  // 仍可能保存着 18；改一个无关选项不能把 18 一起删掉（旧实现按合并值
  // 与项目层比较，切回无项目层就回落默认值）。
  let existing: Record<string, unknown> = {};
  try {
    const raw = window.localStorage.getItem(SETTINGS_KEY);
    const parsed = raw ? JSON.parse(raw) : null;
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      existing = { ...(parsed as Record<string, unknown>) };
    }
  } catch {
    // 损坏的用户层按空对象重写，与 loadSettings 的静默回落一致
  }
  const toWrite: Record<string, unknown> = { ...existing };
  const keys = changedKeys ?? (Object.keys(settings) as Array<keyof AppSettings>);
  for (const key of keys) {
    if (settings[key] === projectLayer[key]) delete toWrite[key];
    else toWrite[key] = settings[key];
  }
  try {
    window.localStorage.setItem(SETTINGS_KEY, JSON.stringify(toWrite));
  } catch {
    /* 隐私模式下写入失败可忽略 */
    return;
  }
  // 写入失败（隐私模式）不通知：缓存快照必须与实际落盘值一致
  for (const listener of settingsSubscribers) {
    try {
      listener(settings);
    } catch {
      /* 单个订阅者出错不能阻断其它订阅者 */
    }
  }
}

/**
 * 应用界面字号：Tauri 下用原生 webview 缩放（坐标/命中测试由系统处理，
 * 不会破坏 fixed 定位的右键菜单），浏览器预览降级为 CSS zoom（仅预览用）。
 *
 * 上下限对齐 macOS 的 UI 字号范围：13（小档）～16（最大档），
 * 超过 16 会让面板内的固定高度控件（28px 的 tb-btn / 22px 的面板标题栏）
 * 溢出容器。
 */
export function applyUiFontSize(px: number): void {
  const scale = Math.max(1, Math.min(1.23, px / 13));
  if (isTauriEnvironment()) {
    document.documentElement.style.zoom = '';
    void getCurrentWebview()
      .setZoom(scale)
      .catch(() => {
        /* 老版本 WebView 不支持时静默忽略 */
      });
  } else {
    document.documentElement.style.zoom = String(scale);
  }
}
