// 收藏分支（从 App.tsx 迁出，行为不变）。
//
// 本机持久化的字符串集合：读时做数组形态校验（坏 JSON 直接给空，
// 否则下游 `new Set(favorites)` 抛 TypeError → ErrorBoundary 白屏）；
// 写时设 50 个上限，防止无界增长拖慢分支排序。

import { useCallback, useState } from 'react';
import { FAVORITE_BRANCHES, loadHistory, toggleHistory } from '../services/historyStore';

export function useFavoriteBranches(): {
  favoriteBranches: string[];
  toggleFavoriteBranch: (name: string) => void;
} {
  const [favoriteBranches, setFavoriteBranches] = useState<string[]>(() =>
    loadHistory(FAVORITE_BRANCHES)
  );

  const toggleFavoriteBranch = useCallback((name: string) => {
    // 上限与持久化由 historyStore 拥有（50 条），与提交历史同口径
    setFavoriteBranches(toggleHistory(FAVORITE_BRANCHES, name).list);
  }, []);

  return { favoriteBranches, toggleFavoriteBranch };
}
