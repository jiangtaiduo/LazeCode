import '@testing-library/jest-dom/vitest';

// jsdom 没有 ResizeObserver；CtxMenuShell 用它做视口钳制。
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;


import { afterEach } from 'vitest';
import { cleanup } from '@testing-library/react';

// vitest.config.ts 没有开 globals，RTL 不会自动注册 cleanup；不显式清，
// 同一文件里的第二个测试会看到上一个测试残留的 DOM 与监听器。
afterEach(cleanup);

// Node 26 exposes a stub `localStorage` that is undefined unless
// --localstorage-file is passed; jsdom's window.localStorage is the real one.
// App modules use the bare global, so point it at jsdom's implementation.
if (typeof window !== 'undefined' && window.localStorage) {
  Object.defineProperty(globalThis, 'localStorage', {
    value: window.localStorage,
    configurable: true
  });
}

// Node 26 can shadow jsdom's Storage with an unavailable stub; replace with a
// minimal in-memory implementation for tests that exercise settings persistence.
if (typeof window !== 'undefined' && !window.localStorage) {
  const map = new Map<string, string>();
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: {
      getItem: (k: string) => map.get(k) ?? null,
      setItem: (k: string, v: string) => void map.set(k, String(v)),
      removeItem: (k: string) => void map.delete(k),
      clear: () => map.clear(),
      key: (i: number) => [...map.keys()][i] ?? null,
      get length() { return map.size; }
    }
  });
}
