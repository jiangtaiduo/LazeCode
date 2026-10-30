import type { GitBranch } from '../types';

/**
 * 本地分支统一排序：当前分支永远第一 → 收藏置顶 → 名称字典序。
 * 顶栏分支下拉与 Git Log「Local」分组共用，保证两处顺序一致。
 */
export function sortLocalBranches(
  branches: GitBranch[],
  currentBranch: string,
  isFavorite: (name: string) => boolean
): GitBranch[] {
  const isCur = (b: GitBranch) => b.isCurrent || b.name === currentBranch;
  return [...branches].sort((a, b) => {
    if (isCur(a) !== isCur(b)) return isCur(a) ? -1 : 1;
    const fa = isFavorite(a.name);
    const fb = isFavorite(b.name);
    return fa === fb ? a.name.localeCompare(b.name) : fa ? -1 : 1;
  });
}
