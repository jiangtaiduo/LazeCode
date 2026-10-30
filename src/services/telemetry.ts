// 遥测（对齐 Zed `telemetry::event!("Noun Verbed")` 的命名与纪律）。
//
// local-first：事件只存本机，不上传、不出网。记什么、不记什么：
// - 记：命令名 + 分类 + 耗时 + 成败（性能回归与崩溃前兆分析用）；
// - 永不记：路径、文件内容、命令参数、token（props 类型只允许
//   string/number/boolean，且调用方不得塞路径——见各调用点）。
//
// 事件名必须 "Noun Verbed"（首字母大写、两词），非法名直接丢弃并 console.warn
//（开发期 fail-fast，用户无感知）。开关：`lazecode.telemetry.enabled`，
// 默认关（opt-in）。设置面板「关于」分区提供开关。
//
// 高频命令（vt_snapshot 系 60fps 轮询）不记：记了就是每秒 60 次落盘。
// 落盘节流：内存攒 25 条或调 flushTelemetry() 才写一次；beforeunload 兜底刷一次。
//
// 可验证性约束（见 settingsLayers）：本模块**零运行时 import**，
// 持久化由调用方注入（App 启动时接 historyStore，验证脚本接内存）。
// 因此缓冲上限是本地常量 TELEMETRY_BUFFER_MAX，必须与
// historyStore.TELEMETRY_EVENTS.max 同值（verify-telemetry.mjs 断言两者相等）。

export interface TelemetryEvent {
  /** 毫秒时间戳 */
  ts: number;
  name: string;
  props: Record<string, string | number | boolean>;
}

export interface TelemetryStore {
  load(): TelemetryEvent[];
  save(events: TelemetryEvent[]): void;
}

/** 缓冲上限：与 historyStore.TELEMETRY_EVENTS.max 同值，改一处改两处。 */
export const TELEMETRY_BUFFER_MAX = 200;

/** "Noun Verbed"：首字母大写、恰两词（Zed telemetry::event! 同约定）。 */
const NAME_RE = /^[A-Z][A-Za-z0-9]* [A-Z][A-Za-z0-9]*$/;

export function isValidEventName(name: string): boolean {
  return NAME_RE.test(name);
}

const OPT_OUT_KEY = 'lazecode.telemetry.enabled';

export function isTelemetryEnabled(): boolean {
  try {
    if (typeof localStorage === 'undefined') return false;
    return localStorage.getItem(OPT_OUT_KEY) === '1';
  } catch {
    return false;
  }
}

export function setTelemetryEnabled(on: boolean): void {
  try {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(OPT_OUT_KEY, on ? '1' : '0');
  } catch {
    /* 隐私模式：开关记不住，保持默认关（只记本地，不出网） */
  }
}

/** 高频轮询命令：记了等于每秒几十次落盘，跳过（性能数据用 ipcLog 的抽样）。 */
const HIGH_FREQ_COMMANDS: ReadonlySet<string> = new Set(['vt_snapshot', 'vt_snapshot_diff']);

function memoryStore(): TelemetryStore {
  let arr: TelemetryEvent[] = [];
  return {
    load: () => [...arr],
    save: (events) => {
      arr = events.slice(-TELEMETRY_BUFFER_MAX);
    },
  };
}

let store: TelemetryStore = memoryStore();

/**
 * 接入持久化（App 启动时调一次，接 historyStore；验证脚本接内存）。
 * 切换时把内存里攒的搬过去，不丢启动早期的事件。
 */
export function configureTelemetryStore(next: TelemetryStore): void {
  const pending = store.load();
  store = next;
  if (pending.length > 0) {
    const merged = [...next.load(), ...pending].slice(-TELEMETRY_BUFFER_MAX);
    next.save(merged);
  }
}

let buffer: TelemetryEvent[] | null = null;
let dirty = 0;
const FLUSH_EVERY = 25;
let unloadHooked = false;

function maybeHookUnload(): void {
  if (unloadHooked) return;
  unloadHooked = true;
  try {
    if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
      window.addEventListener('beforeunload', () => {
        flushTelemetry();
      });
    }
  } catch {
    /* 非浏览器环境无钩子可挂 */
  }
}

/** 刷盘（退出兜底与节流点调）。 */
export function flushTelemetry(): void {
  if (buffer === null || dirty === 0) return;
  store.save(buffer);
  dirty = 0;
}

/**
 * 记一条事件。非法名 / 已关闭时返回 null（调用方无需分支）。
 * props 禁止路径与参数原文（隐私，见文件头）。
 */
export function trackEvent(name: string, props?: Record<string, string | number | boolean>): TelemetryEvent | null {
  if (!isValidEventName(name)) {
    if (typeof console !== 'undefined') console.warn(`[telemetry] 非法事件名（应为"Noun Verbed"）: ${name}`);
    return null;
  }
  if (!isTelemetryEnabled()) return null;
  maybeHookUnload();
  if (buffer === null) buffer = store.load();
  const entry: TelemetryEvent = { ts: Date.now(), name, props: props ?? {} };
  buffer = [...buffer, entry].slice(-TELEMETRY_BUFFER_MAX);
  dirty += 1;
  if (dirty >= FLUSH_EVERY) flushTelemetry();
  return entry;
}

/**
 * IPC 命令执行埋点。分类由调用方注入（命令目录在 tauriBridge 侧），
 * 本模块不 import 目录 —— 保持叶节点，node 验证脚本可直跑。
 */
export function trackCommand(cmd: string, ms: number, ok: boolean, category = 'unknown'): void {
  if (HIGH_FREQ_COMMANDS.has(cmd)) return;
  trackEvent('Command Executed', {
    command: cmd,
    category,
    ms: Math.max(0, Math.round(ms)),
    ok,
  });
}

/** 最近 N 条（Console / 诊断面板用）。 */
export function recentEvents(n = 50): TelemetryEvent[] {
  const all = buffer ?? store.load();
  return all.slice(-Math.max(1, n));
}
