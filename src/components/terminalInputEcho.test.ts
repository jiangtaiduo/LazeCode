import { describe, expect, it } from 'vitest';
import {
  INPUT_ECHO_GRACE_MS,
  OUTPUT_ACTIVE_MS,
  UI_REDRAW_GRACE_MS,
  isConsoleOutputting,
  isInputEcho,
  isUiRedraw,
  pruneInputSessions,
  recordUiEvent,
  recordUserInput,
  type InputTimes,
  type UiTimes
} from './terminalInputEcho';

describe('terminal input echo window', () => {
  it('treats output immediately after a keystroke as echo', () => {
    const times: InputTimes = new Map();
    recordUserInput(times, 's1', 10_000);

    expect(isInputEcho(times, 's1', 10_005)).toBe(true);
    expect(isInputEcho(times, 's1', 10_000 + INPUT_ECHO_GRACE_MS)).toBe(true);
  });

  it('keeps suppressing during continuous typing even when the latest key is newer than the echo', () => {
    const times: InputTimes = new Map();
    recordUserInput(times, 's1', 10_000);
    recordUserInput(times, 's1', 10_100);
    recordUserInput(times, 's1', 10_200);

    // 回显对应 10_000 那次按键；最新按键在 10_200，不能被误判成真实输出
    expect(isInputEcho(times, 's1', 10_005)).toBe(true);
    // 回显对应 10_100 那次按键
    expect(isInputEcho(times, 's1', 10_110)).toBe(true);
  });

  it('does not suppress real output when the UI event was recorded while outputting', () => {
    // 回归：切换到正在持续输出的控制台会记一次 UI 事件（面板显隐重同步），
    // 该事件不得把切换后 500ms 内的真实输出压成「交互重绘」——否则跑马灯
    // 短暂变绿一轮轮询后又变回红。
    const uiTimes: UiTimes = new Map();
    recordUiEvent(uiTimes, 's1', 10_000, true);

    expect(isUiRedraw(uiTimes, 's1', 10_000 + UI_REDRAW_GRACE_MS)).toBe(false);
    const noInput: InputTimes = new Map();
    expect(isConsoleOutputting(noInput, uiTimes, 's1', 100, 10_200)).toBe(true);
  });

  it('a hot event voids an older cold suppression window', () => {
    const uiTimes: UiTimes = new Map();
    recordUiEvent(uiTimes, 's1', 10_000, false); // 安静时 resize：重绘应被抑制
    recordUiEvent(uiTimes, 's1', 10_100, true); // 随后持续输出中滚轮：不再抑制

    expect(isUiRedraw(uiTimes, 's1', 10_150)).toBe(false);
  });

  it('still suppresses the one-shot repaint when the UI event was recorded while idle', () => {
    const uiTimes: UiTimes = new Map();
    recordUiEvent(uiTimes, 's1', 10_000, false);

    expect(isUiRedraw(uiTimes, 's1', 10_000 + UI_REDRAW_GRACE_MS)).toBe(true);
    const noInput: InputTimes = new Map();
    expect(isConsoleOutputting(noInput, uiTimes, 's1', 100, 10_200)).toBe(false);
  });

  it('allows output that arrives outside the echo window', () => {
    const times: InputTimes = new Map();
    recordUserInput(times, 's1', 10_000);

    expect(isInputEcho(times, 's1', 10_000 + INPUT_ECHO_GRACE_MS + 1)).toBe(false);
  });

  it('returns false without input or for an unknown session', () => {
    const times: InputTimes = new Map();
    expect(isInputEcho(times, 's1', 10_000)).toBe(false);

    recordUserInput(times, 's2', 10_000);
    expect(isInputEcho(times, 's1', 10_005)).toBe(false);
  });

  it('prunes input samples older than the output freshness window', () => {
    const times: InputTimes = new Map();
    recordUserInput(times, 's1', 10_000);
    recordUserInput(times, 's1', 10_000 + OUTPUT_ACTIVE_MS + INPUT_ECHO_GRACE_MS + 1);

    expect(times.get('s1')).toHaveLength(1);
    expect(isInputEcho(times, 's1', 10_005)).toBe(false);
  });

  it('drops records for closed sessions', () => {
    const times: InputTimes = new Map();
    recordUserInput(times, 's1', 10_000);
    recordUserInput(times, 's2', 10_000);

    pruneInputSessions(times, new Set(['s1']));
    expect(times.has('s1')).toBe(true);
    expect(times.has('s2')).toBe(false);
  });

  it('decides poll outputting from freshness and echo together', () => {
    const now = 10_000;
    const noUi: UiTimes = new Map();
    const echoTimes: InputTimes = new Map();
    recordUserInput(echoTimes, 's1', now - 150);

    // 输出发生在按键后 50ms → 回显，不点亮
    expect(isConsoleOutputting(echoTimes, noUi, 's1', 100, now)).toBe(false);

    // 按键已经过去 1s，100ms 前的输出属于真实输出
    const realTimes: InputTimes = new Map();
    recordUserInput(realTimes, 's1', now - 1000);
    expect(isConsoleOutputting(realTimes, noUi, 's1', 100, now)).toBe(true);

    // 超出新鲜度窗口
    expect(isConsoleOutputting(realTimes, noUi, 's1', OUTPUT_ACTIVE_MS, now)).toBe(false);
    expect(isConsoleOutputting(realTimes, noUi, 's1', null, now)).toBe(false);
  });

  it('suppresses TUI repaint after a resize/mouse UI event for 500ms', () => {
    const uiTimes: UiTimes = new Map();
    recordUiEvent(uiTimes, 's1', 10_000);

    expect(isUiRedraw(uiTimes, 's1', 10_000 + UI_REDRAW_GRACE_MS)).toBe(true);
    expect(isUiRedraw(uiTimes, 's1', 10_000 + UI_REDRAW_GRACE_MS + 1)).toBe(false);

    const noInput: InputTimes = new Map();
    // UI 事件后 300ms 的输出 → 交互重绘，不点亮
    expect(isConsoleOutputting(noInput, uiTimes, 's1', 100, 10_400)).toBe(false);
    // UI 事件后 501ms 的输出 → 真实输出，点亮
    expect(isConsoleOutputting(noInput, uiTimes, 's1', 100, 10_601)).toBe(true);
    // UI 事件之前的输出不受影响（只留 5ms 采样抖动）
    expect(isConsoleOutputting(noInput, uiTimes, 's1', 1000, 10_000)).toBe(true);
  });

  it('prunes UI event samples with the wider keep window', () => {
    const uiTimes: UiTimes = new Map();
    recordUiEvent(uiTimes, 's1', 10_000);
    recordUiEvent(uiTimes, 's1', 10_000 + OUTPUT_ACTIVE_MS + UI_REDRAW_GRACE_MS + 1);

    expect(uiTimes.get('s1')).toHaveLength(1);
    expect(isUiRedraw(uiTimes, 's1', 10_000 + UI_REDRAW_GRACE_MS)).toBe(false);
  });
});
