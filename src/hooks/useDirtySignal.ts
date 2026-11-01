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
//
// 页面隐藏（切到别的应用、窗口被完全遮挡、最小化）时同理且更严重：rAF 停了，
// 事件消费不掉，但订阅还在 —— 16ms 一条的脏信号会整个积压在 WKWebView 的
// 求值队列里，切回来的一瞬间全部重放，主线程被占满数秒，整个窗口（包括
// 终端之外的部分）黑屏。任务输出越多、离开越久越严重。所以页面隐藏时同样
// 取消订阅，恢复可见时重新订阅 + 补一帧。

import { useEffect } from 'react';
import { subscribeDirty } from '../services/pty';

export function useDirtySignal(
  sessionId: string,
  onScreen: boolean,
  markDirty: () => void
): void {
  useEffect(() => {
    if (!onScreen) return;
    let disposed = false;
    let active = !document.hidden;
    let unsubscribe: (() => void) | null = null;
    const subscribe = () => {
      void subscribeDirty(sessionId, markDirty).then(
        unlisten => {
          // 订阅建立前状态已翻转（隐藏/卸载）：立即退订，绝不让泄漏的
          // 监听把隐藏期间的事件重新灌进来。
          if (!active || disposed) unlisten();
          else unsubscribe = unlisten;
        },
        // listen 失败（窗口正在关闭 / 运行时不支持）：终端本身还能用，
        // 只是失去按帧刷新；下一次激活会重新尝试。
        () => {}
      );
    };
    if (active) {
      // 隐藏期间没有订阅，切回时必须补一帧（即使此后没有任何输出）。
      markDirty();
      subscribe();
    }
    const onVisibility = () => {
      if (document.hidden) {
        if (!active) return;
        active = false;
        unsubscribe?.();
        unsubscribe = null;
      } else {
        if (active) return;
        active = true;
        markDirty();
        subscribe();
      }
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      disposed = true;
      active = false;
      unsubscribe?.();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [sessionId, onScreen, markDirty]);
}
