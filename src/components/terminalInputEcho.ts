/**
 * 用户交互与 PTY 输出的时间关联。
 *
 * 键盘、粘贴、IME 上屏都会写 PTY；shell 回显和 TUI 的按键重绘随后很快
 * 到达。这些字节在 PTY 层面是“输出”，但语义上是用户交互的反馈，不应
 * 点亮顶栏项目 tab 的“正在输出”跑马灯。前端在每次写 PTY 前记一笔时间，
 * 轮询拿到 `lastOutputMs` 后反推输出时刻；落在交互后
 * `INPUT_ECHO_GRACE_MS` 内的输出视为回显/TUI 重绘。
 *
 * 只做时间窗而不是字节比对：TUI 重绘（zsh 高亮、agent 输入框）并不是
 * 对输入字节的原样回显，字节比对会漏掉它们；时间窗与用户感知一致。
 *
 * 切换项目/tab、面板显隐会重新同步 PTY 网格，内核给 TUI 发 SIGWINCH；
 * 鼠标协议事件（点击/拖动/滚轮）也会让 TUI 整屏重绘。这些重绘同样是
 * 交互反馈，但对应的事件不写键盘输入，因此单独记一份 UI 事件时间，
 * 使用更宽的 `UI_REDRAW_GRACE_MS`（允许 TUI 重绘跨若干帧）。
 *
 * 宽限窗只该把「灭的保持灭」，不该把「亮的压灭」：记录 UI 事件时若该
 * 会话正在输出（最近一次轮询判定 outputting），窗口内的输出是持续输出
 * 的延续而非重绘，不得抑制——否则切到正在输出的项目，跑马灯会绿闪一轮
 * 轮询再变回红。事件因此在记录时带上 hot 标志。
 */

/** 输入后这段时间内的输出视为回显/TUI 重绘。 */
export const INPUT_ECHO_GRACE_MS = 200;

/**
 * UI 事件（PTY resize / 鼠标协议事件 / keyup release）后的重绘宽限。
 *
 * TUI 对 SIGWINCH、鼠标点击的整屏重绘可能跨多帧，比按键回显慢；
 * 500ms 覆盖这类重绘，同时不会长时间吞掉真实命令输出。
 */
export const UI_REDRAW_GRACE_MS = 500;

/** 输出新鲜度窗口：超过这个时间的输出本来就不算“正在输出”。 */
export const OUTPUT_ACTIVE_MS = 2000;

/** 允许的时钟/调度抖动：输出采样可能比记录输入早几毫秒。 */
const CLOCK_JITTER_MS = 5;

/** 每个会话保留的输入时间：略长于输出新鲜度，避免轮询边界丢样本。 */
const INPUT_KEEP_MS = OUTPUT_ACTIVE_MS + INPUT_ECHO_GRACE_MS;
/** UI 事件用的保留时长：必须覆盖 UI_REDRAW_GRACE_MS 的完整窗口。 */
const UI_KEEP_MS = OUTPUT_ACTIVE_MS + UI_REDRAW_GRACE_MS;

export type EchoEvent = {
  /** 事件发生的 Date.now() 时刻 */
  at: number;
  /**
   * 记录该事件时，此会话的输出是否仍在新鲜度窗口内（= 控制台正在输出）。
   * 正在输出的控制台，宽限窗内的输出是持续输出的延续而非交互重绘，
   * 不得据此灭掉跑马灯（否则切到正在输出的项目会绿闪一轮轮询）。
   */
  hot: boolean;
};

export type InputTimes = Map<string, EchoEvent[]>;
/** UI 事件时间与输入时间同构，单独存一份以便用不同宽限判定。 */
export type UiTimes = InputTimes;

function recordEvent(
  times: InputTimes,
  sessionId: string,
  now: number,
  keepMs: number,
  hot: boolean
): void {
  const cutoff = now - keepMs;
  const previous = times.get(sessionId);
  const kept = previous ? previous.filter(e => e.at >= cutoff) : [];
  kept.push({ at: now, hot });
  times.set(sessionId, kept);
}

/** 记录一次用户输入；顺手裁掉已经不可能命中的旧时间。键盘回显一律按冷
 * 处理（200ms 窗口无条件抑制）——持续输出的控制台不缺这一拍，而打字后
 * 2s 内的真实输出若被豁免反而会点亮跑马灯。 */
export function recordUserInput(inputTimes: InputTimes, sessionId: string, now: number): void {
  recordEvent(inputTimes, sessionId, now, INPUT_KEEP_MS, false);
}

/** 记录一次 UI 事件（resize / 鼠标 / keyup），用于过滤 TUI 的交互重绘。 */
export function recordUiEvent(
  uiTimes: UiTimes,
  sessionId: string,
  now: number,
  hot = false
): void {
  recordEvent(uiTimes, sessionId, now, UI_KEEP_MS, hot);
}

/**
 * `outputAt` 是否落在某个事件后的宽限窗口内。
 *
 * times 升序：从最新往前找。输出晚于某条事件的窗口时，更早的事件只会
 * 更早，直接判定不匹配；输出早于某条事件时，继续看更早的那条。
 *
 * 命中「不早于该事件」的最新一条时由它定夺：事件记录时控制台正在输出
 * （hot）的话，窗口内的输出是持续输出的延续，不算交互重绘。
 */
function eventWithinGrace(
  times: InputTimes,
  sessionId: string,
  outputAt: number,
  graceMs: number
): boolean {
  const entries = times.get(sessionId);
  if (!entries?.length) return false;
  for (let i = entries.length - 1; i >= 0; i--) {
    const entry = entries[i];
    if (outputAt > entry.at + graceMs) return false;
    if (outputAt >= entry.at - CLOCK_JITTER_MS) return !entry.hot;
  }
  return false;
}

/** `outputAt` 是否落在某次用户输入后的回显窗口内。 */
export function isInputEcho(inputTimes: InputTimes, sessionId: string, outputAt: number): boolean {
  return eventWithinGrace(inputTimes, sessionId, outputAt, INPUT_ECHO_GRACE_MS);
}

/** `outputAt` 是否落在某次 UI 事件后的重绘窗口内。 */
export function isUiRedraw(uiTimes: UiTimes, sessionId: string, outputAt: number): boolean {
  return eventWithinGrace(uiTimes, sessionId, outputAt, UI_REDRAW_GRACE_MS);
}

/** 清理已关闭会话的记录，避免 Map 随开关项目无界增长。 */
export function pruneInputSessions<V>(times: Map<string, V>, liveSessionIds: Set<string>): void {
  for (const sessionId of times.keys()) {
    if (!liveSessionIds.has(sessionId)) times.delete(sessionId);
  }
}

/**
 * 轮询结果是否应点亮「正在输出」。
 *
 * `lastOutputMs` 是后端快照时刻的输出年龄；`receivedAt` 是前端接收时刻。
 * 超过新鲜度窗口的输出本来就不算；落在用户输入回显或 UI 重绘窗口内的
 * 也不算。
 */
export function isConsoleOutputting(
  inputTimes: InputTimes,
  uiTimes: UiTimes,
  sessionId: string,
  lastOutputMs: number | null | undefined,
  receivedAt: number
): boolean {
  if (lastOutputMs == null || lastOutputMs >= OUTPUT_ACTIVE_MS) return false;
  const outputAt = receivedAt - lastOutputMs;
  if (isInputEcho(inputTimes, sessionId, outputAt)) return false;
  if (isUiRedraw(uiTimes, sessionId, outputAt)) return false;
  return true;
}
