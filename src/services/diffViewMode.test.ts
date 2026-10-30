import { beforeEach, describe, expect, it } from 'vitest';
import {
  DIFF_VIEW_MODE_KEY,
  readDiffViewMode,
  rememberDiffViewMode
} from './diffViewMode';

describe('diffViewMode', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('defaults to unified and round-trips side-by-side', () => {
    expect(readDiffViewMode()).toBe('unified');
    rememberDiffViewMode('side-by-side');
    expect(readDiffViewMode()).toBe('side-by-side');
    expect(window.localStorage.getItem(DIFF_VIEW_MODE_KEY)).toBe('side-by-side');
  });

  it('ignores unknown stored values', () => {
    window.localStorage.setItem(DIFF_VIEW_MODE_KEY, 'folded');
    expect(readDiffViewMode()).toBe('unified');
  });
});
