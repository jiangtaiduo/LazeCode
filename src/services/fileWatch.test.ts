import { describe, expect, it } from 'vitest';
import { contentHash, resolveExternalChange } from './fileWatch';

const buffer = {
  content: 'local draft',
  dirty: true,
  diskHash: contentHash('disk'),
  diskLen: 4,
  diskMtimeMs: 1,
  externalChanged: false
};

describe('resolveExternalChange', () => {
  it('prompts instead of reloading when the buffer is dirty', () => {
    expect(resolveExternalChange(buffer, 'changed on disk')).toEqual({
      kind: 'prompt',
      reason: 'changed'
    });
  });

  it('reloads a clean buffer', () => {
    const clean = { ...buffer, dirty: false };
    expect(resolveExternalChange(clean, 'changed on disk')).toEqual({
      kind: 'reload',
      content: 'changed on disk'
    });
  });

  it('clears an old conflict marker when disk returns to baseline', () => {
    expect(
      resolveExternalChange({ ...buffer, externalChanged: true }, 'disk')
    ).toEqual({ kind: 'resolved' });
  });
});
