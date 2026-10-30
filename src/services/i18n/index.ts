// 多语言运行时。
//
// 与设置层的关系：语言持久化在 AppSettings.language（localStorage），这里只负责
// 「当前用哪本字典 + 查表 + 兜底」。不自己做持久化，避免出现两份真相。
//
// 为什么不引入 i18n 库：本项目无复数/性别/选择/日期格式化需求，字典是编译期
// 常量（随 bundle 打包，无加载闪烁），引入库只会增加依赖与间接层。
//
// 原生菜单的联动：Rust 侧菜单在 setup() 时用启动语言构造一次；切换语言时
// 前端调 `set_app_menu_language` 让 Rust 用同一份字典重建菜单。字典本身
// 只有一份（这里是真相），Rust 只按 label 提取 `menu` 子树。
import { invokeCommand, isTauriEnvironment } from '../tauriBridge';
import { DEFAULT_LANGUAGE, normalizeLanguage } from '../languageCore';
import { en } from './en';
import { zhCN } from './zh-CN';
import type { Language, LocaleDict } from './types';

export type { Language, LocaleDict } from './types';
export { en, zhCN };
export { DEFAULT_LANGUAGE, normalizeLanguage } from '../languageCore';

/** 语言 -> 字典。新增语言只需在此登记 + 写入 zh-CN 同级的文件。 */
const LOCALES: Record<Language, LocaleDict> = { en, 'zh-CN': zhCN };

/** 语言在设置面板里的展示名（用各自语言自称，不做翻译）。 */
export const LANGUAGE_LABELS: Record<Language, string> = {
  en: en.settings.english,
  'zh-CN': zhCN.settings.chineseSimplified,
};

const SETTINGS_KEY = 'lazecode.settings';

/** 浅合并两本字典：缺的 section/key 从 base 补上。 */
function mergeDict(base: LocaleDict, patch: Partial<LocaleDict>): LocaleDict {
  const out = { ...base } as LocaleDict;
  for (const key of Object.keys(base) as Array<keyof LocaleDict>) {
    const p = patch[key];
    if (p) (out[key] as unknown) = { ...(base[key] as object), ...(p as object) };
  }
  return out;
}

// ---- 当前语言（模块级，非 React state）----
//
// 之所以不放 context/hook：大量非组件代码（StatusBar 的定时器、对话框工具、
// 文件拖放的 catch 分支）也要取文案，走 React 上下文会把它们绑到组件树上。
// 这里用「订阅器 + 模块级变量」，组件侧用 useSyncExternalStore 订阅即可。

let currentLanguage: Language = DEFAULT_LANGUAGE;
const languageSubscribers = new Set<(lang: Language) => void>();

/** 当前语言。 */
export function getLanguage(): Language {
  return currentLanguage;
}

/** 当前字典。缺项已在 mergeDict 阶段用 en 兜底。 */
export function getDict(): LocaleDict {
  return LOCALES[currentLanguage];
}

/** 订阅语言变更；返回退订。 */
export function subscribeLanguage(listener: (lang: Language) => void): () => void {
  languageSubscribers.add(listener);
  return () => {
    languageSubscribers.delete(listener);
  };
}

/**
 * 切换语言。
 *
 * 同时做三件事，缺一不可：
 * 1. 更新模块级语言 + 通知订阅者（前端界面）
 * 2. 把语言写回设置（localStorage），下次启动沿用
 * 3. 通知 Rust 重建原生菜单
 *
 * 失败策略：3 失败只 warn（菜单保持旧语言，界面已切换 —— 不一致但不致命），
 * 2 失败意味着下次启动回退默认语言。两者都不阻断本次切换。
 */
export function setLanguage(lang: Language, persist = true): void {
  const next = normalizeLanguage(lang);
  if (next === currentLanguage && languageSubscribers.size === 0) return;
  currentLanguage = next;
  try {
    document.documentElement.lang = next;
  } catch {
    /* 非浏览器环境（验证脚本）没有 document */
  }
  for (const listener of [...languageSubscribers]) {
    try {
      listener(next);
    } catch {
      // 单个订阅者抛错不影响其他订阅者
    }
  }
  if (persist) persistLanguageToSettings(next);
  void rebuildNativeMenu(next);
}

/** 把语言写进 AppSettings（与 appSettings.ts 的存储键一致）。 */
function persistLanguageToSettings(lang: Language): void {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    const parsed = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
    parsed.language = lang;
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(parsed));
  } catch {
    // 隐私模式 / 配额耗尽：本次切换仍然生效（内存态），只是下次启动回到默认语言
  }
}

/**
 * 让 Rust 用 `lang` 重建原生菜单。
 *
 * 非 Tauri 环境（浏览器开发 / 单测）直接跳过。失败只 log：菜单语言落后于界面
 * 属于可见但不破坏性的不一致。
 *
 * `void` 掉 promise：调用方（React 事件处理器）不应等待 IPC 往返，
 * 而失败路径已在此处收敛。
 */
function rebuildNativeMenu(lang: Language): void {
  if (!isTauriEnvironment()) return;
  void invokeCommand('set_app_menu_language', { language: lang }).catch((err: unknown) => {
    console.warn('[i18n] 重建原生菜单失败，菜单语言未更新：', err);
  });
}

/** 从 AppSettings 初始化语言。App 挂载时调用一次。 */
export function initLanguage(raw: unknown): void {
  currentLanguage = normalizeLanguage(raw);
  try {
    document.documentElement.lang = currentLanguage;
  } catch {
    /* 非浏览器环境（验证脚本）没有 document */
  }
}

/** 查表取值。key 是点分路径，缺 key 时回落到 en 后仍缺则抛错（拼错 key 立刻暴露）。 */
export function t<K extends keyof LocaleDict>(section: K): LocaleDict[K];
export function t(): LocaleDict;
export function t<K extends keyof LocaleDict>(section?: K) {
  const dict = getDict();
  if (section === undefined) return dict;
  const value = dict[section];
  if (value === undefined) {
    // 走到这里说明 en 都缺 —— 是新增 key 时只改了 en 之外的字典的编程错误。
    throw new Error(`[i18n] language "${currentLanguage}" missing section "${String(section)}"`);
  }
  return value;
}

/** 语言名（用于「已切换为 X」这类通知）。 */
export function languageDisplayName(lang: Language = currentLanguage): string {
  return LANGUAGE_LABELS[lang];
}

// 后端错误码 -> 当前语言文案。单独文件：它是跨端协议的消费方，
// 与字典本身（types/en/zh-CN）分开，改动频率不同。
export { describeBackendError, ERROR_PREFIX } from './errors';

/** 测试/调试用：直接读某语言的字典（不走当前语言）。 */
export function dictOf(lang: Language): LocaleDict {
  const base = LOCALES[lang];
  // zh-CN 等非基准语言与 en 浅合并，缺项回落英文而不是 undefined
  return lang === 'en' ? base : mergeDict(en, base);
}
