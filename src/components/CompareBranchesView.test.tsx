import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { CompareBranchesView } from './CompareBranchesView';
import type { GitCommit, GitCommitFile, GitCompareBranchesResult } from '../types';

vi.mock('../services/tauriBridge', () => ({
  invokeCommand: vi.fn()
}));

import { invokeCommand } from '../services/tauriBridge';

const invoke = vi.mocked(invokeCommand);

const commit = (
  hash: string,
  message: string,
  author: string,
  date: string,
  parents: string[] = []
): GitCommit => ({
  hash,
  shortHash: hash.slice(0, 7),
  message,
  author,
  authorEmail: `${author.toLowerCase()}@example.com`,
  date,
  graphTrack: 0,
  parents,
  branchTags: [{ name: 'main', type: 'main' }]
});

const file = (path: string, status = 'M'): GitCommitFile => ({
  path,
  status,
  additions: 2,
  deletions: 1
});

const data: GitCompareBranchesResult = {
  leftCommits: [
    commit(
      '1111111111111111111111111111111111111111',
      'left newest',
      'Alice',
      '1780000000',
      ['2222222222222222222222222222222222222222']
    ),
    commit(
      '2222222222222222222222222222222222222222',
      'left older',
      'Bob',
      '1770000000',
      ['base']
    )
  ],
  rightCommits: [
    commit('3333333333333333333333333333333333333333', 'right newest', 'Carol', '1781000000')
  ],
  leftFiles: [file('src/left.ts')],
  rightFiles: [file('src/right.ts', 'A')],
  mergeBase: 'base',
  leftTruncated: false,
  rightTruncated: false
};

const commitFiles: Record<string, GitCommitFile[]> = {
  '1111111111111111111111111111111111111111': [file('src/left.ts')],
  '2222222222222222222222222222222222222222': [file('src/older.ts')],
  '3333333333333333333333333333333333333333': [file('src/right.ts', 'A')]
};

beforeEach(() => {
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    if (command === 'git_compare_branches') return data as never;
    if (command === 'git_commit_files') {
      const hash = String(args?.commit ?? '');
      return (commitFiles[hash] ?? []) as never;
    }
    if (command === 'git_compare_commits') {
      return [file('src/left.ts'), file('src/older.ts')] as never;
    }
    return [] as never;
  });
});

describe('CompareBranchesView', () => {
  it('renders two independent commit logs and loads details only after explicit selection', async () => {
    render(
      <CompareBranchesView
        repoPath="/repo"
        leftRef="feature"
        rightRef="main"
        onSwap={vi.fn()}
        onOpenCommitDiff={vi.fn()}
        onOpenCompareDiff={vi.fn()}
      />
    );

    expect(
      await screen.findByText("Commits that exist in feature but don't exist in main")
    ).toBeInTheDocument();
    expect(
      screen.getByText("Commits that exist in main but don't exist in feature")
    ).toBeInTheDocument();
    expect(screen.getAllByText('left newest').length).toBeGreaterThan(0);
    expect(screen.getAllByText('right newest').length).toBeGreaterThan(0);

    const leftPane = screen.getByRole('region', {
      name: "Commits that exist in feature but don't exist in main"
    });
    const rightPane = screen.getByRole('region', {
      name: "Commits that exist in main but don't exist in feature"
    });

    // PyCharm 打开比较页时日志与文件树都是空选，不自动选第一条。
    expect(leftPane.querySelectorAll('.cb-log-row.active')).toHaveLength(0);
    expect(rightPane.querySelectorAll('.cb-log-row.active')).toHaveLength(0);
    expect(within(leftPane).queryByText('left.ts')).not.toBeInTheDocument();
    expect(within(rightPane).queryByText('right.ts')).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith('git_commit_files', expect.anything());

    fireEvent.click(within(leftPane).getByText('left newest', { selector: '.log-subject-text' }));
    expect(await within(leftPane).findByText('left.ts')).toBeInTheDocument();
    expect(leftPane.querySelector('.cb-file-row.selected')).toBeNull();

    fireEvent.click(within(rightPane).getByText('right newest', { selector: '.log-subject-text' }));
    expect(await within(rightPane).findByText('right.ts')).toBeInTheDocument();
    expect(rightPane.querySelector('.cb-file-row.selected')).toBeNull();

    expect(invoke).toHaveBeenCalledWith('git_commit_files', {
      repoPath: '/repo',
      commit: data.leftCommits[0].hash
    });
    expect(invoke).toHaveBeenCalledWith('git_commit_files', {
      repoPath: '/repo',
      commit: data.rightCommits[0].hash
    });
  });

  it('filters one side without touching the other and opens diffs', async () => {
    const onOpenCommitDiff = vi.fn();
    const onOpenCompareDiff = vi.fn();
    render(
      <CompareBranchesView
        repoPath="/repo"
        leftRef="feature"
        rightRef="main"
        onSwap={vi.fn()}
        onOpenCommitDiff={onOpenCommitDiff}
        onOpenCompareDiff={onOpenCompareDiff}
      />
    );

    await screen.findAllByText('left newest');
    const leftPane = screen.getByRole('region', {
      name: "Commits that exist in feature but don't exist in main"
    });
    const rightPane = screen.getByRole('region', {
      name: "Commits that exist in main but don't exist in feature"
    });
    const leftSearch = within(leftPane).getByPlaceholderText('Text or hash');
    fireEvent.change(leftSearch, { target: { value: 'older' } });

    expect(within(leftPane).queryByText('left newest')).not.toBeInTheDocument();
    expect(
      within(leftPane).getByText('left older', { selector: '.log-subject-text' })
    ).toBeInTheDocument();
    expect(
      within(rightPane).getByText('right newest', { selector: '.log-subject-text' })
    ).toBeInTheDocument();

    // 选中 older 后，双击文件打开该提交的单文件 diff。
    const olderRow = within(leftPane).getByText('left older', { selector: '.log-subject-text' });
    fireEvent.click(olderRow);
    const olderFile = await within(leftPane).findByText('older.ts');
    fireEvent.doubleClick(olderFile);
    expect(onOpenCommitDiff).toHaveBeenCalledWith(data.leftCommits[1].hash, 'src/older.ts');

    // Cmd+A 全选整侧 → 聚合 diff 走 merge-base..side。
    fireEvent.keyDown(leftPane, { key: 'a', metaKey: true });
    expect(within(leftPane).getByText('2 commits selected')).toBeInTheDocument();
    const diffButtons = within(leftPane).getAllByTitle('Show Diff');
    fireEvent.click(diffButtons[diffButtons.length - 1]);
    expect(onOpenCompareDiff).toHaveBeenCalledWith('base', 'feature', undefined);
  });

  it('keeps the explicit file selection across parent re-renders', async () => {
    const view = () => (
      <CompareBranchesView
        repoPath="/repo"
        leftRef="feature"
        rightRef="main"
        onSwap={vi.fn()}
        onOpenCommitDiff={vi.fn()}
        onOpenCompareDiff={vi.fn()}
      />
    );
    const { rerender } = render(view());
    await screen.findAllByText('left newest');
    const leftPane = screen.getByRole('region', {
      name: "Commits that exist in feature but don't exist in main"
    });
    fireEvent.click(within(leftPane).getByText('left newest', { selector: '.log-subject-text' }));
    const fileRow = (await within(leftPane).findByText('left.ts')).closest('.cb-file-row');
    expect(fileRow).not.toBeNull();
    fireEvent.click(fileRow!);
    expect(leftPane.querySelector('.cb-file-row.selected')).not.toBeNull();

    // App 把 Compare 视图常驻隐藏再显示，父级重渲染不能把文件选择重置回空。
    rerender(view());
    expect(leftPane.querySelector('.cb-file-row.selected')).not.toBeNull();
    expect(fileRow).toHaveClass('cb-file-row', 'selected');
  });

  it('aggregates files for multiple selected commits and opens range diff', async () => {
    const onOpenCompareDiff = vi.fn();
    render(
      <CompareBranchesView
        repoPath="/repo"
        leftRef="feature"
        rightRef="main"
        onSwap={vi.fn()}
        onOpenCommitDiff={vi.fn()}
        onOpenCompareDiff={onOpenCompareDiff}
      />
    );

    await screen.findAllByText('left newest');
    const leftPane = screen.getByRole('region', {
      name: "Commits that exist in feature but don't exist in main"
    });
    fireEvent.click(within(leftPane).getByText('left newest', { selector: '.log-subject-text' }));
    fireEvent.click(within(leftPane).getByText('left older', { selector: '.log-subject-text' }), {
      metaKey: true
    });

    // 多选后文件树来自 “older 的父提交 → newest” 的聚合 diff。
    expect(await within(leftPane).findByText('older.ts')).toBeInTheDocument();
    expect(within(leftPane).getByText(/2 commits selected/)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith('git_compare_commits', {
      repoPath: '/repo',
      fromCommit: 'base',
      toCommit: data.leftCommits[0].hash
    });

    // 多选下双击文件必须打开范围 diff，而不是第一个提交的单提交 diff。
    fireEvent.doubleClick(within(leftPane).getByText('left.ts'));
    expect(onOpenCompareDiff).toHaveBeenCalledWith(
      'base',
      data.leftCommits[0].hash,
      'src/left.ts'
    );
  });

  it('swaps without changing the two log semantics', async () => {
    const onSwap = vi.fn();
    const { rerender } = render(
      <CompareBranchesView
        repoPath="/repo"
        leftRef="feature"
        rightRef="main"
        onSwap={onSwap}
        onOpenCommitDiff={vi.fn()}
        onOpenCompareDiff={vi.fn()}
      />
    );
    await screen.findAllByText('left newest');
    fireEvent.click(screen.getByTitle('Swap Branches'));
    expect(onSwap).toHaveBeenCalledTimes(1);

    rerender(
      <CompareBranchesView
        repoPath="/repo"
        leftRef="main"
        rightRef="feature"
        onSwap={onSwap}
        onOpenCommitDiff={vi.fn()}
        onOpenCompareDiff={vi.fn()}
      />
    );
    expect(
      await screen.findByText("Commits that exist in main but don't exist in feature")
    ).toBeInTheDocument();
  });
});
