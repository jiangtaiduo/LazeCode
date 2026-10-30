// 分层设置（对齐 Zed `settings_store.rs` 的层叠思想）。
//
// Zed 是 defaults → user → project → local 四层叠加、JSON Schema 校验、
// 改动实时生效。LazeCode 此前只有单层 localStorage：换项目时"上个项目的
// 字号"跟着走，且手改的坏值只有 loadSettings 里零散的特判兜底。
//
// 本模块只做纯逻辑（无 Tauri import，浏览器预览与 node 验证脚本都能用）：
// - SCHEMA：逐 key 校验 + 非法值回落（fall through 到下一层，绝不抛）
// - resolveSettings：defaults < user < project < session 四层合并，
//   并记录每个 key 的来源层与警告（未知 key / 非法值只警告不丢弃式崩溃）
// - 项目层文件 `.lazecode/settings.json` 的解析（parseProjectSettingsText）
// - VSCode 设置导入（importFromVSCode，映射表见函数注释）
//
// 层缓存与落盘仍在 `appSettings.ts`（load/persist/subscribe API 不变）：
// 本模块不碰 localStorage，避免两处写盘打架。
//
// 可验证性约束（见 scripts/verify-*.mjs 的惯例）：本模块**零运行时 import**
//（只有 `import type`，会被擦除）。默认值与语言归一由调用方注入 ——
// node 下直接跑验证脚本时不会把 `@tauri-apps/api` 链拖进来，
// vite 下由 appSettings.ts 统一组装（见 resolveEffectiveSettings）。

import type { AppSettings } from './appSettings';

export type SettingsLayer = 'defaults' | 'user' | 'project' | 'session';

/** 调用方注入的依赖：默认值单源（appSettings.DEFAULT_SETTINGS）+ 语言归一单源（i18n） */
export interface SettingsDeps {
  defaults: AppSettings;
  normalizeLanguage: (v: unknown) => AppSettings['language'];
}

export interface ResolvedSettings {
  values: AppSettings;
  /** 每个 key 的最终来源层 */
  sources: Record<keyof AppSettings, SettingsLayer>;
  /** 非致命问题（未知 key、非法值回落），调用方展示或记日志 */
  warnings: string[];
}

/** 项目层文件名（工作区根下）。点开头：与 .vscode 同类，不污染项目树展示。 */
export const PROJECT_SETTINGS_FILE = '.lazecode/settings.json';

export function projectSettingsPath(workspaceRoot: string): string {
  const sep = workspaceRoot.includes('\\') && !workspaceRoot.includes('/') ? '\\' : '/';
  return `${workspaceRoot.replace(/[\\/]+$/, '')}${sep}${PROJECT_SETTINGS_FILE.replace(/\//g, sep)}`;
}

type Validator<K extends keyof AppSettings> = (v: unknown) => AppSettings[K] | undefined;

function isIntIn(v: unknown, lo: number, hi: number): v is number {
  return typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi;
}

function isFiniteNumber(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v);
}

/** 逐 key schema：返回 undefined = 该层未提供有效值，继续向下回落。 */
function makeSchema(
  normalizeLanguage: (v: unknown) => AppSettings['language'],
): { [K in keyof AppSettings]: Validator<K> } {
  return {
    // 0 = 跟随界面变体；1 = IDE 深色；2 = IDE 浅色（见 SettingsDialog themeOptions）
    themeId: (v) => (isIntIn(v, 0, 2) ? v : undefined),
    // 界面风格变体目前只有 1/2（见 uiVariants）；非法值回落默认，不断言
    uiVariant: (v) => (isIntIn(v, 1, 2) ? v : undefined),
    terminalFontSize: (v) => (isFiniteNumber(v) ? Math.min(48, Math.max(8, v)) : undefined),
    editorFontSize: (v) => (isFiniteNumber(v) ? Math.min(48, Math.max(8, v)) : undefined),
    // applyUiFontSize 的有效显示区间是 13..16，但 schema 只做"有限数"守门：
    // 显示层自己会 clamp（见 appSettings.applyUiFontSize），存的值域不必收窄
    uiFontSize: (v) => (isFiniteNumber(v) ? Math.min(48, Math.max(8, v)) : undefined),
    captionStyle: (v) => (v === 'macos' || v === 'windows' ? v : undefined),
    // 语言永不回落：normalizeLanguage 把一切输入归一到 en / zh-CN
    language: (v) => normalizeLanguage(v),
    ignoredPatterns: (v) =>
      Array.isArray(v) && v.every((x) => typeof x === 'string')
        ? v.map((x) => String(x).trim()).filter(Boolean)
        : undefined,
    autoCheckUpdates: (v) => (typeof v === 'boolean' ? v : undefined),
    lastUpdateCheckTime: (v) => (typeof v === 'number' && Number.isFinite(v) ? Math.max(0, v) : undefined),
    ignoredUpdateVersion: (v) => (typeof v === 'string' ? v.trim() : undefined),
  };
}

const LAYER_ORDER: SettingsLayer[] = ['defaults', 'user', 'project', 'session'];

function asRecord(v: unknown): Record<string, unknown> | null {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  return v as Record<string, unknown>;
}

export interface SettingsInput {
  user?: unknown;
  project?: unknown;
  session?: Partial<AppSettings>;
}

/**
 * 四层合并。warning 格式稳定（`层:key:原因`），调用方展示或记日志。
 * 未知 key 只警告不生效 —— 手滑写错的 key 能被发现，而不是静默无事发生。
 */
export function resolveSettings(input: SettingsInput, deps: SettingsDeps): ResolvedSettings {
  const schema = makeSchema(deps.normalizeLanguage);
  const layerValues: Record<SettingsLayer, Record<string, unknown> | null> = {
    defaults: { ...deps.defaults },
    user: asRecord(input.user),
    project: asRecord(input.project),
    session: asRecord(input.session),
  };
  const values = { ...deps.defaults };
  const sources = Object.fromEntries(
    (Object.keys(deps.defaults) as Array<keyof AppSettings>).map((k) => [k, 'defaults' as SettingsLayer]),
  ) as Record<keyof AppSettings, SettingsLayer>;
  const warnings: string[] = [];

  for (const layer of LAYER_ORDER) {
    const record = layerValues[layer];
    if (!record) continue;
    for (const [rawKey, rawValue] of Object.entries(record)) {
      if (!Object.hasOwn(deps.defaults, rawKey)) {
        if (layer !== 'defaults') warnings.push(`${layer}:${rawKey}:unknown-key`);
        continue;
      }
      const key = rawKey as keyof AppSettings;
      const validate = schema[key] as Validator<typeof key>;
      const valid = validate(rawValue);
      if (valid === undefined) {
        if (layer !== 'defaults') warnings.push(`${layer}:${rawKey}:invalid-value`);
        continue;
      }
      (values as Record<string, unknown>)[key] = valid;
      sources[key] = layer;
    }
  }
  return { values, sources, warnings };
}

/**
 * 解析项目层文件文本。返回 null = 文件不存在/不可读（调用方静默跳过，
 * 不是错误——绝大多数项目没有这个文件）；抛错 = JSON 损坏，调用方提示用户。
 */
export function parseProjectSettingsText(
  text: string | null,
  deps: SettingsDeps,
): { partial: Partial<AppSettings>; warnings: string[] } {
  if (text === null) return { partial: {}, warnings: [] };
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    throw new Error(`项目设置解析失败: ${e instanceof Error ? e.message : String(e)}`);
  }
  const record = asRecord(parsed);
  if (!record) return { partial: {}, warnings: ['project:<root>:not-an-object'] };
  const resolved = resolveSettings({ project: record }, deps);
  const partial: Partial<AppSettings> = {};
  for (const [k, source] of Object.entries(resolved.sources)) {
    if (source === 'project') {
      (partial as Record<string, unknown>)[k] = asRecord(resolved.values)?.[k];
    }
  }
  return { partial, warnings: resolved.warnings };
}

/** VSCode → LazeCode 的映射表。不在表里的 key 进 unmapped（展示给用户，不静默吞）。 */
export interface VSCodeImportResult {
  partial: Partial<AppSettings>;
  /** 成功映射的 key 数（展示用） */
  mapped: string[];
  /** 文件里有但本应用没有对应项的 key */
  unmapped: string[];
  warnings: string[];
}

export function importFromVSCode(vscode: unknown, normalizeLanguage: (v: unknown) => AppSettings['language']): VSCodeImportResult {
  const record = asRecord(vscode);
  const out: VSCodeImportResult = { partial: {}, mapped: [], unmapped: [], warnings: [] };
  if (!record) {
    out.warnings.push('not-an-object');
    return out;
  }
  const take = <K extends keyof AppSettings>(vsKey: string, toKey: K, convert: (v: unknown) => AppSettings[K] | undefined) => {
    if (!(vsKey in record)) return;
    const converted = convert(record[vsKey]);
    if (converted === undefined) {
      out.warnings.push(`${vsKey}:invalid-value`);
      return;
    }
    out.partial[toKey] = converted;
    out.mapped.push(vsKey);
  };
  // 编辑器 / 终端字号：VSCode 与本应用同为 px，直接取
  take('editor.fontSize', 'editorFontSize', (v) => (isFiniteNumber(v) ? Math.min(48, Math.max(8, v)) : undefined));
  take('terminal.integrated.fontSize', 'terminalFontSize', (v) => (isFiniteNumber(v) ? Math.min(48, Math.max(8, v)) : undefined));
  // 主题：只认明暗，不断言具体主题名（One Dark vs Dark+ 的区分本应用没有）
  take('workbench.colorTheme', 'themeId', (v) => {
    if (typeof v !== 'string') return undefined;
    if (/light/i.test(v)) return 2;
    if (/dark/i.test(v)) return 1;
    return undefined;
  });
  // 语言：zh 系进中文，其余保持调用方原值（undefined = 不覆盖）
  if ('locale' in record) {
    const v = record['locale'];
    if (typeof v === 'string') {
      out.partial.language = normalizeLanguage(v);
      out.mapped.push('locale');
    } else {
      out.warnings.push('locale:invalid-value');
    }
  }
  const KNOWN_VS_KEYS = new Set(['editor.fontSize', 'terminal.integrated.fontSize', 'workbench.colorTheme', 'locale']);
  for (const k of Object.keys(record)) {
    if (!KNOWN_VS_KEYS.has(k)) out.unmapped.push(k);
  }
  return out;
}
