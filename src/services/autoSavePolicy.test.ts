import { describe, expect, it } from 'vitest';
import { shouldAutoSave } from './autoSavePolicy';

const base = {
  dirty: true,
  language: 'typescript',
  error: null,
  externalChanged: false
};

describe('shouldAutoSave', () => {
  it('saves a normal dirty buffer', () => {
    expect(shouldAutoSave(base)).toBe(true);
  });

  it('never overwrites an unresolved external change', () => {
    expect(shouldAutoSave({ ...base, externalChanged: true })).toBe(false);
  });

  it('skips clean, image and error buffers', () => {
    expect(shouldAutoSave({ ...base, dirty: false })).toBe(false);
    expect(shouldAutoSave({ ...base, language: 'image' })).toBe(false);
    expect(shouldAutoSave({ ...base, error: 'denied' })).toBe(false);
  });
});
