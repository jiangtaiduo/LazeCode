import { afterEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_SETTINGS,
  applyProjectLayer,
  clearProjectLayer,
  loadSettings,
  persistSettings
} from './appSettings';

afterEach(() => {
  clearProjectLayer();
  window.localStorage.clear();
});

describe('settings layering persistence', () => {
  it('keeps a user preference hidden by a project override when another setting changes', () => {
    window.localStorage.setItem(
      'lazecode.settings',
      JSON.stringify({ editorFontSize: 18, uiFontSize: 14 })
    );
    applyProjectLayer({ editorFontSize: 14 });
    const previous = loadSettings();
    expect(previous.editorFontSize).toBe(14);

    // 用户只改了 uiFontSize，editorFontSize 仍是项目覆盖值
    const next = { ...previous, uiFontSize: 15 };
    persistSettings(next, ['uiFontSize']);

    const raw = JSON.parse(window.localStorage.getItem('lazecode.settings') ?? '{}');
    expect(raw.uiFontSize).toBe(15);
    expect(raw.editorFontSize).toBe(18);
  });

  it('deletes a user override when it is explicitly set back to the project value', () => {
    window.localStorage.setItem('lazecode.settings', JSON.stringify({ editorFontSize: 18 }));
    applyProjectLayer({ editorFontSize: 14 });
    const previous = loadSettings();
    const next = { ...previous, editorFontSize: 14 };
    persistSettings(next, ['editorFontSize']);
    const raw = JSON.parse(window.localStorage.getItem('lazecode.settings') ?? '{}');
    expect(raw.editorFontSize).toBeUndefined();
    expect(loadSettings().editorFontSize).toBe(14);
  });

  it('falls back to defaults without a project layer', () => {
    expect(loadSettings().editorFontSize).toBe(DEFAULT_SETTINGS.editorFontSize);
  });
});
