import { describe, expect, it } from 'vitest';
import {
  INPUT_ECHO_GRACE_MS,
  OUTPUT_ACTIVE_MS,
  isConsoleOutputting,
  isInputEcho,
  pruneInputSessions,
  recordUserInput,
  type InputTimes
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
    const echoTimes: InputTimes = new Map();
    recordUserInput(echoTimes, 's1', now - 150);

    // 输出发生在按键后 50ms → 回显，不点亮
    expect(isConsoleOutputting(echoTimes, 's1', 100, now)).toBe(false);

    // 按键已经过去 1s，100ms 前的输出属于真实输出
    const realTimes: InputTimes = new Map();
    recordUserInput(realTimes, 's1', now - 1000);
    expect(isConsoleOutputting(realTimes, 's1', 100, now)).toBe(true);

    // 超出新鲜度窗口
    expect(isConsoleOutputting(realTimes, 's1', OUTPUT_ACTIVE_MS, now)).toBe(false);
    expect(isConsoleOutputting(realTimes, 's1', null, now)).toBe(false);
  });
});
