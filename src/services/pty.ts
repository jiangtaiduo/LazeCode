// PTY 桥接层
//
// 真实 shell 由 Rust 侧的 `portable-pty` 启动（macOS/Linux 用系统 shell，
// Windows 走 ConPTY）。VT 解析、键盘编码、选区、搜索都在 Rust 侧完成，
// 这一层只负责搬运字节与事件。

import { listen } from '@tauri-apps/api/event';
import { invokeCommand } from './tauriBridge';

export interface PtySpawnResult {
  /** 与 Rust `PtySpawnResult` 的 camelCase 序列化保持一致 */
  sessionId: string;
  shell: string;
  cwd: string;
}

/**
 * 单次 `pty_write` 的最大字节数。
 *
 * 64KB 是「JSON 数字数组编解码开销」与「IPC 往返次数」的折中：再大则
 * 单次序列化的瞬时内存按倍数涨，再小则大粘贴变成几百次 IPC。
 */
const PTY_WRITE_CHUNK = 64 * 1024;

export interface PtyHandle {
  sessionId: string;
  shell: string;
  cwd: string;
  /** 写入输入。字符串按 UTF-8 编码；引擎产出的控制字节直接传 Uint8Array。 */
  write(data: string | Uint8Array): Promise<void>;
  /** 通知内核尺寸变化（SIGWINCH 语义）；格子像素尺寸供终端算内联图片渲染尺寸 */
  resize(cols: number, rows: number, cellWidthPx?: number, cellHeightPx?: number): Promise<void>;
  close(): Promise<void>;
}

/**
 * 订阅「终端状态已变化」信号。
 *
 * PTY 每秒可能产生上百次输出，但屏幕只需按帧率刷新。
 * 渲染循环据此把多次输出合并成一次快照拉取。
 */
export async function subscribeDirty(
  sessionId: string,
  onDirty: () => void
): Promise<() => void> {
  return listen(`pty://dirty/${sessionId}`, () => onDirty());
}

/**
 * 订阅「会话已退出」信号。
 *
 * 读线程在 EOF / 进程结束时发出（`pty.rs`）。没有这个订阅时，
 * shell 退出后 `vt_snapshot` 只会报「会话不存在」并被渲染循环静默吞掉，
 * 终端永远停在最后一帧、rAF 空转 —— 用户看到的就是「终端假死」。
 */
export async function subscribeExit(
  sessionId: string,
  onExit: (code: number) => void
): Promise<() => void> {
  return listen<number>(`pty://exit/${sessionId}`, event => onExit(event.payload));
}

/**
 * 订阅 shell 通过 OSC 7 / OSC 9 / OSC 1337 报告的当前工作目录。
 *
 * 引擎在解析 VT 流时回调，Rust 读线程把变化转成事件；没有 shell
 * integration 时不会触发，UI 继续显示 spawn 时的 cwd。
 */
export async function subscribePwd(
  sessionId: string,
  onPwd: (pwd: string) => void
): Promise<() => void> {
  return listen<string>(`pty://pwd/${sessionId}`, event => onPwd(event.payload));
}

/**
 * 启动一个 PTY 会话。
 *
 * 终端解析在 Rust 侧同步进行，前端拿到的已是格子缓冲，
 * 因此这里不需要把字节喂给任何 JS 侧引擎。
 */
export async function spawnPty(options: {
  sessionId: string;
  cols: number;
  rows: number;
  cwd?: string;
  shell?: string;
}): Promise<PtyHandle> {
  const { sessionId, cols, rows, cwd, shell } = options;

  const result = await invokeCommand<PtySpawnResult>('pty_spawn', {
    sessionId,
    cols,
    rows,
    cwd: cwd ?? null,
    shell: shell ?? null
  });

  return {
    sessionId: result.sessionId,
    shell: result.shell,
    cwd: result.cwd,
    async write(data: string | Uint8Array) {
      const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
      if (bytes.length === 0) return;
      // 分片下发：一次粘贴几 MB 文本时，`Array.from` 会先造出等量的 JS
      // 数字数组（1MB → ~8MB）再序列化成 JSON（~3-4MB），Rust 侧还要逐字节
      // 解析。分片把峰值内存钉在单片上，字节顺序由顺序 await 保证。
      for (let off = 0; off < bytes.length; off += PTY_WRITE_CHUNK) {
        const chunk = bytes.subarray(off, Math.min(off + PTY_WRITE_CHUNK, bytes.length));
        await invokeCommand<void>('pty_write', { sessionId, data: Array.from(chunk) });
      }
    },
    async resize(nextCols: number, nextRows: number, cellWidthPx?: number, cellHeightPx?: number) {
      await invokeCommand<void>('pty_resize', {
        sessionId,
        cols: nextCols,
        rows: nextRows,
        cellWidthPx,
        cellHeightPx
      });
    },
    async close() {
      await invokeCommand<void>('pty_close', { sessionId });
    }
  };
}

/** 终止属于某个工作区的全部终端会话（关闭项目即终止） */
export async function closePtyByCwd(cwd: string): Promise<number> {
  return invokeCommand<number>('pty_close_by_cwd', { cwd });
}
