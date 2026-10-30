/**
 * Diff 查看模式记忆。
 *
 * 用户上一次选的 Unified / Side by Side 是全局偏好：每个文件、每次新开 Diff
 * 都沿用同一个模式，而不是永远回落到 unified。localStorage 只存这一个字符串，
 * 读取失败（隐私模式/损坏值）时安全回退到 unified。
 */
export type DiffViewMode = 'side-by-side' | 'unified';

export const DIFF_VIEW_MODE_KEY = 'ide_diff_view_mode';

export function readDiffViewMode(): DiffViewMode {
  try {
    return window.localStorage.getItem(DIFF_VIEW_MODE_KEY) === 'side-by-side'
      ? 'side-by-side'
      : 'unified';
  } catch {
    return 'unified';
  }
}

export function rememberDiffViewMode(mode: DiffViewMode): void {
  try {
    window.localStorage.setItem(DIFF_VIEW_MODE_KEY, mode);
  } catch {
    // 隐私模式/配额耗尽：只丢记忆，不影响本次 diff。
  }
}
