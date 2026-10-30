// 只给「屏上可见」的终端会话订阅 PTY 脏信号。
//
// 为什么需要这条闸门：Tauri 的 `emit` 在 Rust 侧只做监听表查找，但一旦该
// webview 注册过这个事件名，投递路径就是 `Webview::emit_js` → `eval`，即
// 每条事件一次 WebView 主线程的 JS 求值（tauri 2.12 listener.rs:323 实测）。
// 终端脏信号是按帧节流的 16ms/会话，100 个常驻控制台都订阅时，全部输出
// 时的上限是 6000 次 eval/s —— 主线程会被事件派发占满，切换项目/输入都会卡。
//
// 后台会话不需要事件：VT 状态在原生引擎里持续累积，显示出来只是「拉一帧」。
// 因此隐藏期间取消订阅，切回屏上时主动标脏补一帧；引擎的脏行状态不会丢，
// 第一次 `vt_snapshot_diff` 会把隐藏期间所有变化一起带回来。

import { useEffect } from 'react';
import { subscribeDirty } from '../services/pty';

export function useDirtySignal(
  sessionId: string,
  onScreen: boolean,
  markDirty: () => void
): void {
  useEffect(() => {
    if (!onScreen) return;
    // 隐藏期间没有订阅，切回时必须补一帧（即使此后没有任何输出）。
    markDirty();
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void subscribeDirty(sessionId, markDirty).then(
      unlisten => {
        if (disposed) unlisten();
        else unsubscribe = unlisten;
      },
      // listen 失败（窗口正在关闭 / 运行时不支持）：终端本身还能用，
      // 只是失去按帧刷新；下一次激活会重新尝试。
      () => {}
    );
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [sessionId, onScreen, markDirty]);
}
