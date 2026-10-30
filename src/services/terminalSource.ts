// 终端数据面：前端与原生 libghostty-vt 之间的唯一通道
//
// VT 解析、键盘编码、选区、搜索全部在 Rust 侧完成。前端只负责：
//   - 拿一帧紧凑的单元格缓冲用于绘制
//   - 把原始按键事件交给原生编码器
//   - 把用户选择/搜索意图翻译成原生调用
//
// 为什么这些能力不放前端：
//   - 解析：原生 SIMD，实测比 wasm 快 19~31 倍
//   - 键盘编码：kitty 协议标志、光标键应用模式等状态只有终端自己知道，
//     前端复刻这套逻辑必然与终端失步
//   - 选区/搜索：直接在滚动历史上操作，无需把历史搬进 JS

import { invoke } from '@tauri-apps/api/core';
import { decodeCellFrame, decodeCellDiff, type CellFrame, type CellDiff } from '../ghostty-vt/cellFrame';
import type { KeyInput } from '../ghostty-vt/domKeys';
import { t } from './i18n';

/** 原生引擎身份，用于确认后端类型与版本 */
export interface EngineInfo {
  libraryVersion: string;
  commit: string;
  /** 编译目标三元组 */
  target: string;
  /** 8 = 原生 64 位；4 说明仍在 wasm 后端 */
  pointerSize: number;
  backend: string;
}

/** 搜索方向 */
export type SearchSelect = 'next' | 'prev';

/**
 * 单次 `vt_snapshot` 的超时上限。
 *
 * 快照是纯内存读（4800 格 ≈ 0.2 ms），正常远低于此值；
 * 1 s 上限只用于把「永久假死」降级成「一次失败 + 下一帧重试」。
 */
const SNAPSHOT_TIMEOUT_MS = 1000;

/**
 * 带超时的 `invoke`。
 *
 * 超时只放弃等待，不取消已下发的 IPC：目标命令若仍在执行，
 * 其结果会在稍后被投递但无人接收（`vt_snapshot` 无副作用，可安全丢弃）。
 */
function invokeWithTimeout<T>(
  cmd: string,
  args: Record<string, unknown>,
  timeoutMs: number
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      reject(new Error(t().terminal.timeout(cmd, timeoutMs)));
    }, timeoutMs);

    invoke<T>(cmd, args).then(
      value => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(value);
      },
      err => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        reject(err);
      }
    );
  });
}

export interface SearchOutcome {
  total: number;
  /** 当前选中的匹配序号，无匹配时为 null */
  selected: number | null;
  /** 当前匹配的视口坐标（闭区间），用于前端画选区 */
  selection?: { startX: number; startY: number; endX: number; endY: number } | null;
}

/**
 * 一个 PTY 会话的终端数据面。
 *
 * 所有方法都是一次 IPC；帧是单缓冲二进制，其余是 JSON 小对象。
 */
export class TerminalSource {
  private readonly sessionId: string;
  private last: CellFrame | null = null;

  constructor(sessionId: string) {
    this.sessionId = sessionId;
  }

  /** 最近一帧的尺寸（用于布局前预判，避免首帧前的空窗） */
  get cols(): number {
    return this.last?.cols ?? 0;
  }

  get rows(): number {
    return this.last?.rows ?? 0;
  }

  /**
   * 取一帧当前网格状态。
   *
   * 应在 rAF 中调用并由脏信号合并：PTY 每秒可能产生上百次输出，
   * 但屏幕只需按帧率刷新。
   *
   * 超时竞速：`vt_snapshot` 是同步命令，跑在 wry 的 ipc:// custom-protocol
   * worker 上（`tauri-runtime-wry` 的 `with_asynchronous_custom_protocol`）。
   * 若它在某次调用里卡住（例如与读线程争 `terminal` 锁），后续所有
   * `vt_snapshot` 都会排在同一个 worker 线程上等它 —— 终端就此假死且
   * 无法恢复。这里给单次调用设上限，超时后 reject 让渲染循环能继续推进
   * 而不是永久停摆。
   */
  async snapshot(): Promise<CellFrame> {
    const buffer = await invokeWithTimeout<ArrayBuffer>(
      'vt_snapshot',
      { sessionId: this.sessionId },
      SNAPSHOT_TIMEOUT_MS
    );
    // 解码复用同一份 ArrayBuffer：Rust 侧每次返回新缓冲（IPC 需要独占所有权），
    // 但旧帧一旦被替换就再无引用，可以让它被 GC 而不是两帧驻留。
    const frame = decodeCellFrame(buffer);
    this.last = frame;
    return frame;
  }

  /**
   * 取一帧**增量**快照：只带引擎标记为脏的行。
   *
   * 与 `snapshot` 同一条 IPC 超时保护（理由见其注释），但帧小一个量级：
   * 稳态下从整屏 KB 级降到百字节级，前端也省掉整屏逐格比对。
   *
   * `full` 为 true 时整屏当脏行送（首帧 / resize / 帧缓存失效）。
   * 返回的是 `CellDiff` 索引视图，cells 仍在 `buffer` 里，由
   * `CellDiffCache` 合并进屏幕 —— 因此调用方必须持有返回的
   * `diff.buffer` 直到合并完成。
   */
  async snapshotDiff(full: boolean): Promise<CellDiff> {
    const buffer = await invokeWithTimeout<ArrayBuffer>(
      'vt_snapshot_diff',
      { sessionId: this.sessionId, full },
      SNAPSHOT_TIMEOUT_MS
    );
    return decodeCellDiff(buffer);
  }

  /**
   * 滚轮：由 Rust 侧决定去路 —— 程序开了鼠标跟踪就编码成鼠标序列转发给 PTY
   * （omp/opencode/htop 这类 TUI 自己滚），备用屏 + DEC 1007 转方向键，
   * 其余情况回滚本地视口。返回 true 表示已转发。
   */
  async wheel(delta: number, x: number, y: number, mods: number): Promise<boolean> {
    return invoke<boolean>('vt_wheel', { sessionId: this.sessionId, delta, x, y, mods });
  }

  /** 滚动到底部（回到活动区） */
  async scrollToBottom(): Promise<void> {
    await invoke('vt_scroll', { sessionId: this.sessionId, delta: null });
  }

  /**
   * 把一次按键编码为写入 PTY 的字节。
   *
   * 返回空数组表示该按键不产生输入（例如单独按下修饰键）。
   */
  async encodeKey(input: KeyInput): Promise<Uint8Array> {
    const bytes = await invoke<number[]>('vt_encode_key', {
      sessionId: this.sessionId,
      input
    });
    return Uint8Array.from(bytes);
  }

  /** 设置选区。端点为视口坐标，闭区间。 */
  async setSelection(
    startX: number,
    startY: number,
    endX: number,
    endY: number,
    rectangle = false
  ): Promise<void> {
    await invoke('vt_set_selection', {
      sessionId: this.sessionId,
      startX,
      startY,
      endX,
      endY,
      rectangle
    });
  }

  /** 双击选词：引擎按词边界算区间并装入选区，返回端点（视口坐标，闭区间）。无可选内容返回 null。 */
  async selectWord(x: number, y: number): Promise<[number, number, number, number] | null> {
    return invoke<[number, number, number, number] | null>('vt_select_word', {
      sessionId: this.sessionId,
      x,
      y
    });
  }

  /** 三击选整行：同上，按行内容算区间。 */
  async selectLine(y: number): Promise<[number, number, number, number] | null> {
    return invoke<[number, number, number, number] | null>('vt_select_line', {
      sessionId: this.sessionId,
      y
    });
  }

  async clearSelection(): Promise<void> {
    await invoke('vt_clear_selection', { sessionId: this.sessionId });
  }

  /** 取选区纯文本（供复制）。无选区时为空串。 */
  async selectedText(): Promise<string> {
    return invoke<string>('vt_selected_text', { sessionId: this.sessionId });
  }

  /** 引擎身份（版本、目标平台、后端类型） */
  async engineInfo(): Promise<EngineInfo> {
    return invoke<EngineInfo>('vt_engine_info');
  }

  /** 设置终端默认前景/背景/光标色，与 IDE 主题对齐。 */
  async setColors(fg: number, bg: number, cursor: number): Promise<void> {
    await invoke('vt_set_colors', { sessionId: this.sessionId, fg, bg, cursor });
  }

  /** 终端是否请求了鼠标跟踪；true 时点击应转发给 TUI 而不是本地选区。 */
  async mouseTracking(): Promise<boolean> {
    return invoke<boolean>('vt_mouse_tracking', { sessionId: this.sessionId });
  }

  /** 鼠标事件：action 0=press 1=release 2=motion；button 1=left 2=right 3=middle。 */
  async mouse(
    action: number,
    button: number,
    x: number,
    y: number,
    mods = 0
  ): Promise<boolean> {
    return invoke<boolean>('vt_mouse', {
      sessionId: this.sessionId,
      action,
      button,
      x,
      y,
      mods
    });
  }

  /** 在滚动历史中搜索。不传 `select` 则只统计匹配数。 */
  async search(needle: string, select?: SearchSelect): Promise<SearchOutcome> {
    return invoke<SearchOutcome>('vt_search', {
      sessionId: this.sessionId,
      needle,
      select: select ?? null
    });
  }
}
