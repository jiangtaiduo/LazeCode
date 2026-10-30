// 版本化历史存储（对齐 Zed `sqlez` 用 SQLite KVP 存历史/缓存的思想）。
//
// 此前三处历史（提交信息 / 收藏分支 / 最近项目）各自直写 localStorage：
// key 无命名空间、无版本、无迁移，坏数据靠各调用方特判。
// 这里统一：命名空间 + schema 版本 + 旧 key 一次性认领迁移 + 容量上限。
//
// 为什么不用 SQLite（tauri-plugin-sql）：三份历史加起来几十条短串，
// 上 native 插件是杀鸡用牛刀。`HistoryBackend` 是可替换的（内存 / SQLite），
// 将来数据量上来时换实现不用改调用方。local-first：数据不出本机。
//
// node 安全：无 localStorage（验证脚本 / SSR）时落到模块级内存 Map，
// 逻辑（去重/截断/迁移）完全可测。

export interface HistoryBackend {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}

const memFallback = new Map<string, string>();

/** 默认后端：localStorage，异常（隐私模式）或缺失（node）时走内存。 */
export const domBackend: HistoryBackend = {
  get(key: string): string | null {
    try {
      if (typeof localStorage === 'undefined') return memFallback.get(key) ?? null;
      return localStorage.getItem(key);
    } catch {
      return memFallback.get(key) ?? null;
    }
  },
  set(key: string, value: string): void {
    try {
      if (typeof localStorage === 'undefined') throw new Error('no-dom');
      localStorage.setItem(key, value);
    } catch {
      memFallback.set(key, value);
    }
  },
  remove(key: string): void {
    try {
      if (typeof localStorage === 'undefined') throw new Error('no-dom');
      localStorage.removeItem(key);
    } catch {
      /* 删不掉就算了 */
    }
    memFallback.delete(key);
  },
};

let backend: HistoryBackend = domBackend;

/**
 * 替换存储后端（SQLite 迁移 / 测试隔离用）。生产代码只在启动时调一次；
 * 验证脚本用它隔离各用例。
 */
export function setHistoryBackend(next: HistoryBackend): void {
  backend = next;
}

export function memoryBackend(): HistoryBackend {
  const map = new Map<string, string>();
  return {
    get: (k) => map.get(k) ?? null,
    set: (k, v) => void map.set(k, v),
    remove: (k) => void map.delete(k),
  };
}

/** 命名空间 + 版本：改版时升 v2，旧数据按 key 前缀整体可辨。 */
const NS = 'lazecode.history.v1.';

export interface HistorySpec {
  /** 命名空间内的 key（不含前缀） */
  key: string;
  /** 条目上限 */
  max: number;
  /**
   * 序列化后字节预算（JSON.stringify 长度）。条数上限防"条目多"，
   * 字节预算防"单条大"——Zed ACP transcript 的教训（#60134：7MB 会话
   * 吃掉 1.3GB 常驻）：无界增长必须同时有条数和字节两道闸。
   * **新增任何列表型本地存储，一律用 HistorySpec 配齐两项，不要手写 slice。**
   */
  maxBytes?: number;
  /** 认领迁移的旧 key（无命名空间，直读）。认领=搬家：写新位后删旧位。 */
  legacyKeys?: string[];
}

export const COMMIT_MESSAGES: HistorySpec = {
  key: 'commitMessages',
  max: 10,
  // 提交信息单条可达数 KB（多行正文），10 条按最坏算给 64KB
  maxBytes: 64 * 1024,
  legacyKeys: ['ide_recent_commit_messages'],
};
export const FAVORITE_BRANCHES: HistorySpec = {
  key: 'favoriteBranches',
  max: 50,
  // 分支名短，50 条正常几 KB；8KB 预算防畸形长名
  maxBytes: 8 * 1024,
  legacyKeys: ['ide_favorite_branches'],
};
export const RECENT_WORKSPACES: HistorySpec = {
  key: 'recentWorkspaces',
  max: 50,
  // 路径单条 <1KB，50 条给 64KB 富余（2026-10-14：10 → 50，用户要开很多项目）
  maxBytes: 64 * 1024,
  legacyKeys: ['ide_recent_workspaces'],
};
/** 上次打开的项目（单值，非数组，行为与旧 `ide_last_workspace` 一致）。 */
export const LAST_WORKSPACE: HistorySpec = {
  key: 'lastWorkspace',
  max: 1,
  // 单路径 4KB 封顶：超长路径不落盘（保留旧值），防畸形数据撑大存储
  maxBytes: 4 * 1024,
  legacyKeys: ['ide_last_workspace'],
};
/** 遥测事件队列（对象数组，见 telemetry.ts）。 */
export const TELEMETRY_EVENTS: HistorySpec = {
  key: 'telemetryEvents',
  max: 200,
  // 事件小（~200B），200 条正常 <64KB；256KB 预算防 props 误塞大串
  maxBytes: 256 * 1024,
};

function namespaced(spec: HistorySpec): string {
  return NS + spec.key;
}

/** 读原始串：新位没有则认领旧位（搬家），都没有则 null。 */
function readRaw(spec: HistorySpec): string | null {
  const fresh = backend.get(namespaced(spec));
  if (fresh !== null) {
    // 新位已存在：顺手清掉残留旧位（降级后回升的场景），迁移只认新位
    for (const legacy of spec.legacyKeys ?? []) backend.remove(legacy);
    return fresh;
  }
  for (const legacy of spec.legacyKeys ?? []) {
    const v = backend.get(legacy);
    if (v !== null) {
      backend.set(namespaced(spec), v);
      backend.remove(legacy);
      return v;
    }
  }
  return null;
}

function parseStringArray(raw: string | null, max: number): string[] {
  if (raw === null) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    // 坏元素过滤（旧 readStringArray 同语义）：下游 `.filter`/展开不能抛
    return parsed.filter((x): x is string => typeof x === 'string').slice(0, max);
  } catch {
    return [];
  }
}

/** 读字符串历史（自动认领旧 key，坏数据给空——与旧 readStringArray 同语义）。 */
export function loadHistory(spec: HistorySpec): string[] {
  return parseStringArray(readRaw(spec), spec.max);
}

/**
 * 双上限收敛：先截条数，再按序列化体积从最旧（尾部）开始淘汰，直到装进
 * maxBytes。列表是"新在头"，丢尾=丢最旧，语义与条数截断一致。
 * 无 maxBytes 的 spec 只截条数（行为与旧实现完全一致）。
 */
export function fitBudget<T>(spec: HistorySpec, items: T[]): T[] {
  let next = items.slice(0, spec.max);
  const budget = spec.maxBytes;
  if (budget === undefined || JSON.stringify(next).length <= budget) return next;
  // 单条就超预算时至少保留 1 条（调用方看到最新数据，而不是空），
  // 其余场景从尾部淘汰。循环必终止（每次至少丢一条）。
  while (next.length > 1 && JSON.stringify(next).length > budget) next.pop();
  return next;
}

/** 头插 + 去重 + 双上限 + 落盘，返回新列表（调用方直接 setState）。 */
export function pushHistory(spec: HistorySpec, value: string): string[] {
  const next = fitBudget(spec, [value, ...loadHistory(spec).filter((v) => v !== value)]);
  backend.set(namespaced(spec), JSON.stringify(next));
  return next;
}

/** 删除单条（最近项目移除 / 取消收藏用），返回新列表。 */
export function removeFromHistory(spec: HistorySpec, value: string): string[] {
  const next = loadHistory(spec).filter((v) => v !== value);
  backend.set(namespaced(spec), JSON.stringify(next));
  return next;
}

/** 收藏切换：有则删、无则加，返回新列表 + 是否新增。 */
export function toggleHistory(spec: HistorySpec, value: string): { list: string[]; added: boolean } {
  const current = loadHistory(spec);
  if (current.includes(value)) {
    const list = current.filter((v) => v !== value);
    backend.set(namespaced(spec), JSON.stringify(list));
    return { list, added: false };
  }
  const list = fitBudget(spec, [value, ...current]);
  backend.set(namespaced(spec), JSON.stringify(list));
  return { list, added: true };
}

/** 清空（提交历史"清除历史"按钮用）。 */
export function clearHistory(spec: HistorySpec): void {
  backend.set(namespaced(spec), JSON.stringify([]));
}

export function loadLastWorkspace(): string | null {
  return readRaw(LAST_WORKSPACE);
}

export function saveLastWorkspace(path: string): void {
  // 单值也受字节预算：超长路径不落盘（保留旧值），防畸形数据撑大存储
  const budget = LAST_WORKSPACE.maxBytes;
  if (budget !== undefined && path.length > budget) return;
  backend.set(namespaced(LAST_WORKSPACE), path);
}

export function clearLastWorkspace(): void {
  backend.remove(namespaced(LAST_WORKSPACE));
}

/** 对象数组存取（遥测队列用）。坏数据给空，不抛。 */
export function loadJsonArray<T>(spec: HistorySpec): T[] {
  const raw = readRaw(spec);
  if (raw === null) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as T[]).slice(0, spec.max) : [];
  } catch {
    return [];
  }
}

export function saveJsonArray(spec: HistorySpec, items: unknown[]): void {
  backend.set(namespaced(spec), JSON.stringify(fitBudget(spec, items)));
}
