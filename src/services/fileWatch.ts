// 外部修改检测的前端侧。
//
// Rust 的 fs_watch_files 监听打开中的文件，命中事件经 fs://external-change
// 推回；本模块提供两件事：
//   1. contentHash / resolveExternalChange —— 纯函数决策（可脱离 React 测试）；
//   2. onExternalChange —— 事件订阅入口（浏览器预览环境为空操作）。
//
// 决策靠 diskHash 基线（磁盘内容在上次读取/写入时的哈希），而不是只比内容：
// 脏缓冲的 content 天然 ≠ 磁盘，只比内容会把「自己的未保存编辑」在每次重开
// 标签时都误判成外部修改。基线未推进的典型场景是「保存后继续输入」——
// 磁盘已是刚保存的内容，缓冲是更新的草稿，两者不同但磁盘没被外人动过。

import { listen } from '@tauri-apps/api/event';
import { isTauriEnvironment } from './tauriBridge';
import type { WatchedBufferView } from './fileProbe';

/** 探针类型与快路径判定是零依赖叶节点（见 fileProbe.ts），这里重导出，
 *  调用方 import 位置不变，node 验证脚本直引叶模块。 */
export type { FileProbe, WatchedBufferView } from './fileProbe';
export { probeFastPath } from './fileProbe';

/** 外部修改事件通道名，与 Rust file_watch.rs 的 EXTERNAL_CHANGE_EVENT 一致 */
export const EXTERNAL_CHANGE_EVENT = 'fs://external-change';

export type ExternalChangeAction =
  /** 磁盘没变，或读到的内容与缓冲一致 */
  | { kind: 'noop' }
  /** 真·外部修改且缓冲有未保存编辑；或读盘失败（reason 区分文案） */
  | { kind: 'prompt'; reason: 'changed' | 'unreadable' }
  /** 缓冲干净（或 forceReload）：直接采用磁盘内容 */
  | { kind: 'reload'; content: string }
  /** 冲突已消解（磁盘回到基线/与缓冲一致）：清除既有标记 */
  | { kind: 'resolved' };

/**
 * FNV-1a 32bit + 36 进制长度。
 *
 * 只用于变更检测，不做完整性校验：32bit 碰撞靠长度位兜底
 * （同长 + 同哈希才误判，概率可忽略）。全等场景仍比较完整内容。
 * 空串保留给「从未读过盘」的哨兵，所以这里永远返回非空。
 */
export function contentHash(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return `${(h >>> 0).toString(36)}-${s.length.toString(36)}`;
}

/**
 * 判定一次外部变更检测的结果。
 *
 * @param disk 读到的磁盘内容；null = 读盘失败（被删除/无权限）
 * @param forceReload 横幅「重载」按钮：忽略 dirty 强制采用磁盘内容
 */
export function resolveExternalChange(
  buffer: WatchedBufferView,
  disk: string | null,
  forceReload = false
): ExternalChangeAction {
  if (disk === null) {
    // 读盘失败与内容变更走同一出口：都要求用户表态，reason 只影响横幅文案
    return { kind: 'prompt', reason: 'unreadable' };
  }
  const baselineKnown = buffer.diskHash !== '';
  const unchanged =
    disk === buffer.content ||
    (baselineKnown && contentHash(disk) === buffer.diskHash);
  if (unchanged) {
    return buffer.externalChanged ? { kind: 'resolved' } : { kind: 'noop' };
  }
  if (forceReload || !buffer.dirty) {
    return { kind: 'reload', content: disk };
  }
  return { kind: 'prompt', reason: 'changed' };
}

/** 与 Rust ExternalChangePayload 对应（serde camelCase）：负载是对象而非裸字符串 */
interface ExternalChangePayload {
  path: string;
}

/** 目录变更事件通道名，与 Rust file_watch.rs 的 DIR_CHANGE_EVENT 一致 */
export const DIR_CHANGE_EVENT = 'fs://dir-change';

/**
 * 订阅外部修改事件，返回退订函数。
 *
 * 浏览器预览（npm run dev）没有 Tauri 运行时，直接返回空操作，
 * 否则 @tauri-apps/api 的 listen 会抛错。
 */
export async function onExternalChange(
  handler: (path: string) => void
): Promise<() => void> {
  if (!isTauriEnvironment()) return () => {};
  const unlisten = await listen<ExternalChangePayload>(
    EXTERNAL_CHANGE_EVENT,
    ev => {
      if (ev.payload?.path) handler(ev.payload.path);
    }
  );
  return unlisten;
}

/**
 * 订阅**目录**变更（Finder 增删/改名工作区文件，或别处动过项目）。
 *
 * 负载是目录路径原文（与 workspacePath / externalRoots 直接可比的形态），
 * 不携带是哪个文件变了 —— 前端只需知道「这个目录的内容集合变了」，据此
 * 重新读目录刷新树。
 *
 * 浏览器预览环境下为空操作。
 */
export async function onDirectoryChange(
  handler: (dirPath: string) => void
): Promise<() => void> {
  if (!isTauriEnvironment()) return () => {};
  const unlisten = await listen<ExternalChangePayload>(
    DIR_CHANGE_EVENT,
    ev => {
      if (ev.payload?.path) handler(ev.payload.path);
    }
  );
  return unlisten;
}

/**
 * 目录变更的防抖合并。
 *
 * 一次用户动作会引发一连串事件：Finder 删除单个文件在 FSEvents 上就是
 * `delete` + 可能的 `rename`，删目录则是「目录 + 目录内每个文件」各一条。
 * 逐个刷新会让树反复重建（每次都 read_directory），所以按目录路径合并：
 * 同一目录在窗口期内只刷最后一次。
 *
 * @returns `{ emit, cancel }`。`cancel` **必须**在订阅方卸载时调用：
 *   只退订事件而不清定时器，会留下一个 pending 的 setTimeout，其回调闭包
 *   持有调用方的 ref 与当时的 watchedDirRoots —— 项目切换后晚 250ms 触发，
 *   拿旧根集合去刷新项目的树。
 */
export function debounceDirectoryChanges(
  handler: (dirPath: string) => void,
  delayMs = 250
): { emit: (dirPath: string) => void; cancel: () => void } {
  const timers = new Map<string, number>();
  return {
    emit: (dirPath: string) => {
      const prev = timers.get(dirPath);
      if (prev !== undefined) window.clearTimeout(prev);
      timers.set(
        dirPath,
        window.setTimeout(() => {
          timers.delete(dirPath);
          handler(dirPath);
        }, delayMs)
      );
    },
    cancel: () => {
      for (const timer of timers.values()) window.clearTimeout(timer);
      timers.clear();
    },
  };
}
