import { beforeEach, describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { LeftStrip } from './LeftStrip';
import { setLanguage } from '../services/i18n';

describe('LeftStrip terminal running badge', () => {
  beforeEach(() => setLanguage('en', false));

  it('shows the running dot and tooltip while commands are active', () => {
    const { container } = render(
      <LeftStrip
        activeDrawer={null}
        activeBottomTool={null}
        onToggleDrawer={() => {}}
        onToggleBottomTool={() => {}}
        onOpenSettings={() => {}}
        settingsOpen={false}
        terminalRunning={['vite', 'tsc']}
      />
    );
    expect(container.querySelector('.strip-dot-running')).not.toBeNull();
    const running = screen
      .getAllByRole('button')
      .find(b => b.getAttribute('title')?.includes('running'));
    expect(running).toBeDefined();
    expect(running!.getAttribute('title')).toContain('vite、tsc');
  });

  it('hides the dot and keeps the plain tooltip when idle', () => {
    const { container } = render(
      <LeftStrip
        activeDrawer={null}
        activeBottomTool={null}
        onToggleDrawer={() => {}}
        onToggleBottomTool={() => {}}
        onOpenSettings={() => {}}
        settingsOpen={false}
        terminalRunning={[]}
      />
    );
    expect(container.querySelector('.strip-dot-running')).toBeNull();
    expect(
      screen
        .getAllByRole('button')
        .some(b => b.getAttribute('title') === 'Terminal console (Alt+F12 / ⌥F12 / ⌘`)')
    ).toBe(true);
  });
});
