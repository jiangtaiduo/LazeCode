import { useEffect, useReducer } from 'react';
import { subscribeLanguage } from '../services/i18n';

/**
 * 让 React.memo 组件在语言切换时重新渲染。
 *
 * 字典是模块级变量，不是 React state；只有 SettingsDialog 订阅时，TitleBar /
 * GitBottomPanel / CommitDrawer / ProjectDrawer 这些 memo 组件会因 props 未变
 * 而继续显示旧语言的 `t()` 结果。这里订阅语言变更并强制一次渲染。
 */
export function useLanguageRefresh(): void {
  const [, force] = useReducer((n: number) => n + 1, 0);
  useEffect(() => subscribeLanguage(() => force()), []);
}
