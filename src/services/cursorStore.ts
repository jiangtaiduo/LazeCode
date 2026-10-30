// 光标位置的外部 store + 状态栏指示器。
//
// 为什么不用 App state：App.tsx 是约 3800 行的单壳层，cursor 每次光标
// 移动都会变；放进 App state 会让整棵 React 树重跑（分支过滤排序、
// Git 提交列表、文件树全在路径上）。光标移动是编辑器里最高频的事件，
// 这个代价值不得。
//
// 外部 store + 按快照比较跳过更新，保证只有真正移动了光标才重渲染。

import { useEffect, useState } from 'react';

export interface CursorPosition {
  line: number;
  column: number;
}

type CursorListener = (position: CursorPosition | null) => void;

let current: CursorPosition | null = null;
const listeners = new Set<CursorListener>();

/** 写入光标位置。值未变时跳过通知。 */
export function setCursorIndicator(position: CursorPosition | null): void {
  if (
    (position === null && current === null) ||
    (position !== null &&
      current !== null &&
      position.line === current.line &&
      position.column === current.column)
  ) {
    return;
  }
  current = position;
  for (const listener of listeners) listener(current);
}

/** 订阅当前光标位置。 */
export function useCursorIndicator(): CursorPosition | null {
  const [position, setPosition] = useState<CursorPosition | null>(current);
  useEffect(() => {
    listeners.add(setPosition);
    // 订阅时同步一次：挂载期间光标可能已经变了
    setPosition(current);
    return () => {
      listeners.delete(setPosition);
    };
  }, []);
  return position;
}
