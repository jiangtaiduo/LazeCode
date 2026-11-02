/**
 * 用户输入与 PTY 输出的时间关联。
 *
 * 键盘、粘贴、IME 上屏都会写 PTY；shell 回显和 TUI 的按键重绘随后很快
 * 到达。这些字节在 PTY 层面是“输出”，但语义上是用户输入的反馈，不应
 * 点亮顶栏项目 tab 的“正在输出”跑马灯。前端在每次写 PTY 前记一笔时间，
 * 轮询拿到 `lastOutputMs` 后反推输出时刻；落在输入后
 * `INPUT_ECHO_GRACE_MS` 内的输出视为回显/TUI 重绘。
 *
 * 只做时间窗而不是字节比对：TUI 重绘（zsh 高亮、agent 输入框）并不是
 * 对输入字节的原样回显，字节比对会漏掉它们；时间窗与用户感知一致。
 */

/** 输入后这段时间内的输出视为回显/TUI 重绘。 */
export const INPUT_ECHO_GRACE_MS = 200;

/** 输出新鲜度窗口：超过这个时间的输出本来就不算“正在输出”。 */
export const OUTPUT_ACTIVE_MS = 2000;

/** 允许的时钟/调度抖动：输出采样可能比记录输入早几毫秒。 */
const CLOCK_JITTER_MS = 5;

/** 每个会话保留的输入时间：略长于输出新鲜度，避免轮询边界丢样本。 */
const KEEP_MS = OUTPUT_ACTIVE_MS + INPUT_ECHO_GRACE_MS;

export type InputTimes = Map<string, number[]>;

/** 记录一次用户输入；顺手裁掉已经不可能命中的旧时间。 */
export function recordUserInput(inputTimes: InputTimes, sessionId: string, now: number): void {
  const cutoff = now - KEEP_MS;
  const previous = inputTimes.get(sessionId);
  const times = previous ? previous.filter(t => t >= cutoff) : [];
  times.push(now);
  inputTimes.set(sessionId, times);
}

/**
 * `outputAt` 是否落在某次用户输入后的回显窗口内。
 *
 * times 升序：从最新往前找。输出晚于某条输入的窗口时，更早的输入只会
 * 更早，直接判定不匹配；输出早于某条输入时，继续看更早的那条。
 */
export function isInputEcho(inputTimes: InputTimes, sessionId: string, outputAt: number): boolean {
  const times = inputTimes.get(sessionId);
  if (!times?.length) return false;
  for (let i = times.length - 1; i >= 0; i--) {
    const inputAt = times[i];
    if (outputAt > inputAt + INPUT_ECHO_GRACE_MS) return false;
    if (outputAt >= inputAt - CLOCK_JITTER_MS) return true;
  }
  return false;
}

/** 清理已关闭会话的输入记录，避免 Map 随开关项目无界增长。 */
export function pruneInputSessions(inputTimes: InputTimes, liveSessionIds: Set<string>): void {
  for (const sessionId of inputTimes.keys()) {
    if (!liveSessionIds.has(sessionId)) inputTimes.delete(sessionId);
  }
}

/**
 * 轮询结果是否应点亮「正在输出」。
 *
 * `lastOutputMs` 是后端快照时刻的输出年龄；`receivedAt` 是前端接收时刻。
 * 超过新鲜度窗口的输出本来就不算；落在用户输入回显窗口内的也不算。
 */
export function isConsoleOutputting(
  inputTimes: InputTimes,
  sessionId: string,
  lastOutputMs: number | null | undefined,
  receivedAt: number
): boolean {
  if (lastOutputMs == null || lastOutputMs >= OUTPUT_ACTIVE_MS) return false;
  return !isInputEcho(inputTimes, sessionId, receivedAt - lastOutputMs);
}
