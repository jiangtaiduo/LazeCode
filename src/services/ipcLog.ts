// 原生 IPC 调用日志：Git 工具窗口的 Console 页签展示真实调用记录，
// 而不是预先写死的假输出。

export type IpcLogLevel = 'info' | 'error';

export interface IpcLogEntry {
  id: number;
  time: string;
  text: string;
  level: IpcLogLevel;
}

const MAX_ENTRIES = 200;

let entries: IpcLogEntry[] = [];
let nextId = 1;
const listeners = new Set<(entries: IpcLogEntry[]) => void>();

function timestamp(): string {
  const now = new Date();
  const pad = (value: number, width = 2) => String(value).padStart(width, '0');
  return `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}.${pad(now.getMilliseconds(), 3)}`;
}

let notifyScheduled = false;

/** 单条日志上限：Console 页签 `.console-line` 是 pre-wrap + break-all，MB 级单行直接冻结 WebKit 布局 */
const MAX_TEXT_LEN = 4000;

export function logIpc(text: string, level: IpcLogLevel = 'info'): void {
  // 单条截断：err.message 可能携带 MB 级后端输出，整条渲染进
  // `.console-line`（pre-wrap + break-all）足以让 WebKit 布局冻结。
  const line = text.length > MAX_TEXT_LEN ? `${text.slice(0, MAX_TEXT_LEN)}…(已截断)` : text;
  entries = [...entries, { id: nextId++, time: timestamp(), text: line, level }].slice(-MAX_ENTRIES);
  // 同一宏任务里的多条 IPC（刷新 git 三件套、一次输入的 pty_write+日志）合并为
  // 一次通知：每次 IPC 都全量推送会让订阅面板高频重渲染
  if (notifyScheduled) return;
  notifyScheduled = true;
  queueMicrotask(() => {
    notifyScheduled = false;
    listeners.forEach(listener => listener(entries));
  });
}

export function getIpcLog(): IpcLogEntry[] {
  return entries;
}

export function subscribeIpcLog(listener: (entries: IpcLogEntry[]) => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** 命令名 + 关键参数，日志里能看出到底执行了什么 */
export function formatCommand(cmd: string, args?: Record<string, unknown>): string {
  if (!args) return cmd;
  const parts: string[] = [];
  if (typeof args.path === 'string') parts.push(String(args.path));
  if (typeof args.commit === 'string') parts.push(String(args.commit).slice(0, 8));
  if (typeof args.maxCount === 'number') parts.push(`-n ${args.maxCount}`);
  if (typeof args.query === 'string') parts.push(`"${args.query}"`);
  if (typeof args.branch === 'string') parts.push(String(args.branch));
  if (typeof args.maxDepth === 'number') parts.push(`depth=${args.maxDepth}`);
  if (typeof args.staged === 'boolean' && args.staged) parts.push('--cached');
  return parts.length > 0 ? `${cmd} ${parts.join(' ')}` : cmd;
}

/** 把返回值压缩成一行可读摘要 */
export function describeResult(value: unknown): string {
  if (Array.isArray(value)) return `${value.length} 项`;
  if (value === null || value === undefined) return 'void';
  if (typeof value === 'string') {
    const flat = value.replace(/\s+/g, ' ').trim();
    return flat.length > 60 ? `${flat.slice(0, 57)}...` : flat || '(空)';
  }
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    if (Array.isArray(record.lines)) return `${record.lines.length} 行差异`;
    if (typeof record.name === 'string' && Array.isArray(record.children)) {
      return `目录 ${record.name}（${record.children.length} 项）`;
    }
    if (typeof record.name === 'string') return `文件 ${record.name}`;
    return 'ok';
  }
  return String(value);
}
