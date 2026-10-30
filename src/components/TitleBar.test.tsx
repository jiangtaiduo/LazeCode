import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { TitleBar } from './TitleBar';
import { setLanguage } from '../services/i18n';

const baseProps = {
  projectName: 'lazecode',
  workspacePath: '/tmp/lazecode',
  recentWorkspaces: [],
  onOpenProject: vi.fn(),
  onSwitchWorkspace: vi.fn(),
  currentBranch: 'main',
  branches: [],
  onSelectBranch: vi.fn(),
  platform: 'mac' as const,
  onSearchClick: vi.fn(),
  isRepo: true,
};

describe('TitleBar update badge', () => {
  beforeEach(() => setLanguage('en', false));
  afterEach(() => setLanguage('en', false));

  it('shows the available version in English and opens the update dialog', () => {
    const onAvailableUpdateClick = vi.fn();
    render(
      <TitleBar
        {...baseProps}
        availableUpdateVersion="0.2.0"
        onAvailableUpdateClick={onAvailableUpdateClick}
      />
    );

    const badge = screen.getByRole('button', {
      name: 'LazeCode 0.2.0 is available. Click to view the update.',
    });
    expect(badge).toHaveTextContent('v0.2.0 available');

    fireEvent.click(badge);
    expect(onAvailableUpdateClick).toHaveBeenCalledOnce();
  });

  it('uses the Chinese badge text when the UI language is zh-CN', () => {
    setLanguage('zh-CN', false);
    render(<TitleBar {...baseProps} availableUpdateVersion="0.2.0" />);

    const badge = screen.getByRole('button', {
      name: 'LazeCode 0.2.0 可用，点击查看更新。',
    });
    expect(badge).toHaveTextContent('v0.2.0 可更新');
    expect(badge).not.toHaveTextContent('available');
  });
});
