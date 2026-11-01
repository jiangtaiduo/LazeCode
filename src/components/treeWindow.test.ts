import { describe, expect, it } from 'vitest';
import { buildTreeOffsets, computeTreeWindow } from './treeWindow';

describe('tree window math', () => {
  it('builds prefix offsets including the total height', () => {
    expect(buildTreeOffsets([24, 22, 22])).toEqual([0, 24, 46, 68]);
    expect(buildTreeOffsets([])).toEqual([0]);
  });

  it('renders the row containing the scroll top and the row containing the viewport bottom', () => {
    // 行：24 / 22 / 22，总高 68
    const offsets = buildTreeOffsets([24, 22, 22]);
    expect(computeTreeWindow(offsets, 68, 0, 22, 0)).toEqual({
      start: 0,
      end: 1,
      topPad: 0,
      bottomPad: 44
    });
    expect(computeTreeWindow(offsets, 68, 30, 30, 0)).toEqual({
      start: 1,
      end: 3,
      topPad: 24,
      bottomPad: 0
    });
  });

  it('clamps overscan at both ends and never drops the last row', () => {
    const offsets = buildTreeOffsets([22, 22, 22, 22]);
    const atTop = computeTreeWindow(offsets, 88, 0, 22, 8);
    expect(atTop.start).toBe(0);
    expect(atTop.topPad).toBe(0);

    const atBottom = computeTreeWindow(offsets, 88, 66, 22, 8);
    expect(atBottom.end).toBe(4);
    expect(atBottom.bottomPad).toBe(0);
  });

  it('returns a valid window when scrollTop is past the content', () => {
    const offsets = buildTreeOffsets([22, 22]);
    expect(computeTreeWindow(offsets, 44, 999, 100, 0)).toEqual({
      start: 1,
      end: 2,
      topPad: 22,
      bottomPad: 0
    });
  });

  it('returns an empty window for an empty tree', () => {
    expect(computeTreeWindow([0], 0, 0, 400, 8)).toEqual({
      start: 0,
      end: 0,
      topPad: 0,
      bottomPad: 0
    });
  });
});
