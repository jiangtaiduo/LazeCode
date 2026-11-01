import { describe, expect, it } from 'vitest';
import { shouldReleaseCanvas } from './terminalCanvasRelease';

describe('hidden terminal canvas release policy', () => {
  it('never releases the on-screen pane, even while the page is hidden', () => {
    expect(shouldReleaseCanvas(true, true)).toBe(false);
    expect(shouldReleaseCanvas(true, false)).toBe(false);
  });

  it('keeps hidden panes allocated while the page is visible (foreground tab switch stays instant)', () => {
    expect(shouldReleaseCanvas(false, false)).toBe(false);
  });

  it('releases hidden panes once the page itself is hidden', () => {
    expect(shouldReleaseCanvas(false, true)).toBe(true);
  });
});
