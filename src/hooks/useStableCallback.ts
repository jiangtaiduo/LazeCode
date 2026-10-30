// 把「每次渲染都新建的内联回调」变成稳定引用，同时始终调用最新实现（从 App.tsx 迁出）。
//
// 稳定性是 React.memo 生效的前提：内联箭头每次渲染都是新函数，浅比较必然不等，
// 子树的 memo 就白加了。用 ref 镜像最新实现即可两者兼得 —— 组件拿到同一个
// 函数引用，事件触发时执行的仍是当前渲染的闭包（不会读到过期 state）。

import { useCallback, useRef } from 'react';

export function useStableCallback<A extends unknown[], R>(fn: (...args: A) => R): (...args: A) => R {
  const ref = useRef(fn);
  ref.current = fn;
  return useCallback((...args: A) => ref.current(...args), []);
}
