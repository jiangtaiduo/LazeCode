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

describe('TitleBar branch pill stability', () => {
  beforeEach(() => setLanguage('en', false));
  afterEach(() => setLanguage('en', false));

  it('keeps the branch pill mounted (skeleton) while branch info reloads after a project switch', () => {
    // 切项目时 currentBranch 先被清空再重拉：pill 若整块卸载，后面的
    // 项目 tab 会被左右推来推去 —— 加载中必须是骨架占位而不是消失
    const { container } = render(<TitleBar {...baseProps} currentBranch="" />);
    expect(container.querySelector('.branch-skeleton-bar')).not.toBeNull();
    // 占位不可点：不该出现分支下拉按钮
    expect(container.querySelector('.branch-pill-loading')).not.toBeNull();
    expect(screen.queryByTitle('Click to switch or create a Git branch')).toBeNull();
  });

  it('shows the real branch name once loaded', () => {
    render(<TitleBar {...baseProps} currentBranch="main" />);
    expect(document.querySelector('.branch-skeleton-bar')).toBeNull();
    expect(screen.getByText('main')).toBeTruthy();
  });

  it('renders no branch pill on the welcome screen (no workspace)', () => {
    const { container } = render(<TitleBar {...baseProps} workspacePath="" currentBranch="" />);
    expect(container.querySelector('.branch-skeleton-bar')).toBeNull();
  });
});
