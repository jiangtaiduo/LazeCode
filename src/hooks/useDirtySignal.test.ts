import { describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useDirtySignal } from './useDirtySignal';

const { subscribeDirty } = vi.hoisted(() => ({ subscribeDirty: vi.fn() }));

vi.mock('../services/pty', () => ({ subscribeDirty }));

/** 等 useEffect 里的 promise then 链跑完；条件成立即返回，不猜时长 */
const flush = () => vi.waitFor(() => undefined);

describe('useDirtySignal', () => {
  it('屏上会话订阅脏信号，并在激活时主动补一帧', async () => {
    const unlisten = vi.fn();
    subscribeDirty.mockReset();
    subscribeDirty.mockResolvedValue(unlisten);
    const markDirty = vi.fn();

    renderHook(() => useDirtySignal('s1', true, markDirty));

    // 激活即标脏：隐藏期间可能错过事件，切回必须补帧
    expect(markDirty).toHaveBeenCalledTimes(1);
    await flush();
    expect(subscribeDirty).toHaveBeenCalledWith('s1', markDirty);

    // 后端事件到达 → 标脏
    const onDirty = subscribeDirty.mock.calls[0][1] as () => void;
    onDirty();
    expect(markDirty).toHaveBeenCalledTimes(2);
  });

  it('后台会话不注册监听，切回屏上时重新订阅', async () => {
    const unlisten = vi.fn();
    subscribeDirty.mockReset();
    subscribeDirty.mockResolvedValue(unlisten);
    const markDirty = vi.fn();

    const { rerender } = renderHook(
      ({ onScreen }: { onScreen: boolean }) => useDirtySignal('s1', onScreen, markDirty),
      { initialProps: { onScreen: false } }
    );

    expect(subscribeDirty).not.toHaveBeenCalled();
    expect(markDirty).not.toHaveBeenCalled();

    rerender({ onScreen: true });
    await flush();
    expect(subscribeDirty).toHaveBeenCalledTimes(1);
    expect(markDirty).toHaveBeenCalled();

    // 切走：取消订阅，事件不再进入 JS
    rerender({ onScreen: false });
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);

    // 切回：重新订阅且立刻补帧
    rerender({ onScreen: true });
    await flush();
    expect(subscribeDirty).toHaveBeenCalledTimes(2);
    expect(markDirty.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it('卸载时取消订阅（异步订阅完成后才卸载也不泄漏）', async () => {
    const unlisten = vi.fn();
    subscribeDirty.mockReset();
    // 已 resolve 的 promise 其 .then 仍在微任务里执行：unmount 先发生，
    // 覆盖「卸载后订阅才完成」的分支（disposed=true 时必须补一次 unlisten）。
    subscribeDirty.mockReturnValue(Promise.resolve(unlisten));

    const { unmount } = renderHook(() => useDirtySignal('s1', true, () => {}));
    unmount();
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('页面隐藏时退订，恢复可见时重订阅并补一帧', async () => {
    const unlisten = vi.fn();
    subscribeDirty.mockReset();
    subscribeDirty.mockResolvedValue(unlisten);
    const markDirty = vi.fn();

    const setHidden = (hidden: boolean) => {
      Object.defineProperty(document, 'hidden', { value: hidden, configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));
    };

    renderHook(() => useDirtySignal('s1', true, markDirty));
    await flush();
    expect(subscribeDirty).toHaveBeenCalledTimes(1);

    // 切到别的应用：页面隐藏 → 立即退订。遮挡期间 16ms 一条的脏信号不再
    // eval 进 WKWebView，切回时不会再重放整个缺席期间的积压（黑屏根因）。
    setHidden(true);
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);

    // 隐藏期间触发的 visibilitychange（仍是隐藏）不能重复退订
    setHidden(true);
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);

    // 切回来：重新订阅 + 立刻补一帧
    setHidden(false);
    await flush();
    expect(subscribeDirty).toHaveBeenCalledTimes(2);
    expect(markDirty.mock.calls.length).toBeGreaterThanOrEqual(2);

    // 恢复可见后再次隐藏：新订阅也要被退订
    setHidden(true);
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(2);
  });

  it('隐藏后订阅才建立时立即退订，不把事件带回隐藏页', async () => {
    const unlisten = vi.fn();
    subscribeDirty.mockReset();
    subscribeDirty.mockResolvedValue(unlisten);
    const markDirty = vi.fn();

    // 上一个用例把 document.hidden 留在 true；本用例从可见状态起步
    Object.defineProperty(document, 'hidden', { value: false, configurable: true });
    const { unmount } = renderHook(() => useDirtySignal('s1', true, markDirty));
    await flush();
    expect(subscribeDirty).toHaveBeenCalledTimes(1);

    Object.defineProperty(document, 'hidden', { value: true, configurable: true });
    document.dispatchEvent(new Event('visibilitychange'));
    // 退订发生在订阅 promise resolve 之前：resolve 出来的监听必须被丢弃
    await flush();
    expect(unlisten).toHaveBeenCalledTimes(1);
    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
