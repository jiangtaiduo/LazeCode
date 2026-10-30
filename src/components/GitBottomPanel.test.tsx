import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { GitBottomPanel } from './GitBottomPanel';
import { invokeCommand } from '../services/tauriBridge';
import type { GitBranch, GitCommit, GitTag } from '../types';

vi.mock('../services/tauriBridge', () => ({
  invokeCommand: vi.fn(async (cmd: string) => {
    if (cmd === 'git_commits_files') {
      return [
        { path: 'src/a.ts', status: 'M', additions: 2, deletions: 1 },
        { path: 'src/b.ts', status: 'A', additions: 8, deletions: 0 }
      ];
    }
    return [{ path: 'src/single.ts', status: 'M', additions: 1, deletions: 0 }];
  })
}));

beforeEach(() => {
  window.localStorage.clear();
  vi.mocked(invokeCommand).mockClear();
});

const commit: GitCommit = {
  hash: 'abc123',
  shortHash: 'abc123',
  message: 'initial',
  author: 'dev',
  authorEmail: 'dev@example.com',
  date: '1700000000',
  graphTrack: 0
};

const branch = (name: string, extra: Partial<GitBranch> = {}): GitBranch => ({
  name,
  isCurrent: false,
  isRemote: false,
  ...extra
});

const renderPanel = (branches: GitBranch[], tags: GitTag[] = [], context = vi.fn()) =>
  render(
    <GitBottomPanel
      repoPath="/repo"
      commits={[commit]}
      branches={branches}
      tags={tags}
      currentBranch="main"
      onSelectCommit={() => {}}
      onOpenCommitDiff={() => {}}
      onOpenFileDiff={() => {}}
      onOpenCommitsDiff={() => {}}
      onClose={() => {}}
      onBranchContext={context}
      isRepo
    />
  );

const clickRef = (label: string, metaKey = false) => {
  const el = screen.getByText(label);
  fireEvent.pointerDown(el);
  fireEvent.click(el, { metaKey });
};

describe('GitBottomPanel branch tree', () => {
  it('renders folders, tags and the current branch under Local', () => {
    renderPanel(
      [branch('main', { isCurrent: true }), branch('feature/login'), branch('feature/pay')],
      [{ name: 'v1.0', shortHash: 'abc123', date: '', message: '' }]
    );
    expect(screen.getByText('HEAD (Current Branch)')).toBeInTheDocument();
    expect(screen.getByText('Tags')).toBeInTheDocument();
    expect(screen.getByText('v1.0')).toBeInTheDocument();
    expect(screen.getByText('feature')).toBeInTheDocument();
    expect(screen.getAllByText('main')).toHaveLength(1);
  });

  it('hides the branch list pane while keeping the action strip', () => {
    renderPanel([branch('main', { isCurrent: true })]);
    fireEvent.click(screen.getByTitle('Hide Branches Pane'));
    expect(screen.queryByText('HEAD (Current Branch)')).toBeNull();
    expect(screen.getByTitle('Show Branches Pane')).toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Show Branches Pane'));
    expect(screen.getByText('HEAD (Current Branch)')).toBeInTheDocument();
  });

  it('runs fetch from the toolbar', () => {
    const fetch = vi.fn();
    render(
      <GitBottomPanel
        repoPath="/repo"
        commits={[commit]}
        branches={[branch('main', { isCurrent: true })]}
        tags={[]}
        currentBranch="main"
        onSelectCommit={() => {}}
        onOpenCommitDiff={() => {}}
        onOpenFileDiff={() => {}}
        onOpenCommitsDiff={() => {}}
        onClose={() => {}}
        onFetch={fetch}
        isRepo
      />
    );
    fireEvent.click(screen.getByTitle('Fetch All Remotes'));
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it('retains commit selection when clicking outside the panel (e.g. in editor or diff view)', () => {
    const { container } = renderPanel([branch('main', { isCurrent: true }), branch('feat')]);
    const commitRow = container.querySelector('[data-commit-row="abc123"]') as HTMLElement;
    expect(commitRow).toHaveClass('active');

    // 点击外部区域（如 diff 视图或编辑器）绝不能取消 git log 选中的提交，保证可无缝连续查看下一个差异
    fireEvent.pointerDown(document.body);
    expect(commitRow).toHaveClass('active');
  });

  it('passes the whole multi-selection to the context menu', () => {
    const context = vi.fn();
    renderPanel([branch('main', { isCurrent: true }), branch('feat')], [], context);
    clickRef('main', true);
    clickRef('feat', true);
    fireEvent.contextMenu(screen.getByText('feat'));
    expect(context).toHaveBeenCalledTimes(1);
    const selected = context.mock.calls[0][3] as GitBranch[];
    expect(selected.map(b => b.name).sort()).toEqual(['feat', 'main']);
  });

  it('aggregates files and opens diff for exactly the selected commits', async () => {
    const commits: GitCommit[] = [
      { ...commit, hash: 'c1', shortHash: 'c1', message: 'newest', parents: ['c2'] },
      { ...commit, hash: 'c2', shortHash: 'c2', message: 'middle', parents: ['c3'] },
      { ...commit, hash: 'c3', shortHash: 'c3', message: 'oldest', parents: ['base'] }
    ];
    const onOpenCommitsDiff = vi.fn();
    const onOpenFileDiff = vi.fn();
    const invoke = vi.mocked(invokeCommand);
    invoke.mockClear();

    const { container } = render(
      <GitBottomPanel
        repoPath="/repo"
        commits={commits}
        branches={[branch('main', { isCurrent: true })]}
        tags={[]}
        currentBranch="main"
        onSelectCommit={() => {}}
        onOpenCommitDiff={() => {}}
        onOpenFileDiff={onOpenFileDiff}
        onOpenCommitsDiff={onOpenCommitsDiff}
        onClose={() => {}}
        isRepo
      />
    );

    // 初始选中最新的 c1；再依次多选最老的 c3、中间的 c2。
    fireEvent.click(container.querySelector('[data-commit-row="c3"]')!, { metaKey: true });
    fireEvent.click(container.querySelector('[data-commit-row="c2"]')!, { metaKey: true });

    expect(await screen.findByText('a.ts')).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith('git_commits_files', {
      repoPath: '/repo',
      commits: ['c1', 'c2', 'c3']
    });
    expect(screen.getByText('src')).toBeInTheDocument();
    expect(screen.getAllByText('2 files').length).toBeGreaterThan(0);

    // 眼睛菜单：Group By + Layout；Show Details 控制提交详情面板（PyCharm 语义）。
    fireEvent.click(screen.getByTitle('View Options'));
    expect(screen.getByText('Show Details')).toBeInTheDocument();
    expect(screen.getByText('^P')).toBeInTheDocument();
    expect(screen.getByText('+2')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Show Details'));
    expect(container.querySelector('.details-bottom-meta')).toBeNull();
    expect(screen.getByText('+2')).toBeInTheDocument();

    // PyCharm 语义：单击只选中，双击才打开 diff。
    fireEvent.click(screen.getByText('a.ts'));
    expect(onOpenCommitsDiff).not.toHaveBeenCalled();
    expect(screen.getByText('a.ts').closest('.details-file-item')).toHaveClass('active');
    fireEvent.doubleClick(screen.getByText('a.ts'));
    expect(onOpenCommitsDiff).toHaveBeenCalledWith(['c1', 'c2', 'c3'], 'src/a.ts');
    expect(onOpenFileDiff).not.toHaveBeenCalled();
  });

  it('toggles Group By Directory with ^P when the log panel has focus', async () => {
    const { container } = renderPanel([branch('main', { isCurrent: true })]);
    expect(await screen.findByText('src')).toBeInTheDocument();

    const panel = container.querySelector('#pycharmBottomGit') as HTMLElement;
    fireEvent.pointerDown(panel);
    expect(document.activeElement).toBe(panel);

    fireEvent.keyDown(panel, { key: 'p', ctrlKey: true });
    expect(screen.queryByText('src')).toBeNull();
    expect(screen.getByText('single.ts')).toBeInTheDocument();

    // ⌘P 与 ^P 等价（跨平台键位）
    fireEvent.keyDown(panel, { key: 'p', metaKey: true });
    expect(await screen.findByText('src')).toBeInTheDocument();
  });
});
