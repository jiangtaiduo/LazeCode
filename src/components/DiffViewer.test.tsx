import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { DiffViewer } from './DiffViewer';
import type { FileDiff } from './DiffViewer';

const mocks = vi.hoisted(() => ({
  invokeCommand: vi.fn()
}));

vi.mock('../services/tauriBridge', () => ({
  invokeCommand: mocks.invokeCommand
}));

// Monaco 在 jsdom 里不可用：让就绪 Promise 永远不 resolve，组件停在轻量占位，
// 但工具栏、hunk 下标和文件导航仍走真实逻辑。
vi.mock('../services/monacoSetup', () => ({
  ensureMonacoReady: () => new Promise(() => {}),
  MONACO_THEME_BY_APP_THEME: {},
  UNICODE_HIGHLIGHT_OPTIONS: {}
}));

const diff: FileDiff = {
  path: 'src/a.ts',
  oldPath: 'src/a.ts',
  isNew: false,
  isDeleted: false,
  isBinary: false,
  additions: 2,
  deletions: 2,
  lines: [
    { kind: 'hunk', oldLine: 1, newLine: 1, text: '@@ -1,1 +1,1 @@' },
    { kind: 'del', oldLine: 1, newLine: null, text: 'old-a' },
    { kind: 'add', oldLine: null, newLine: 1, text: 'new-a' },
    { kind: 'hunk', oldLine: 10, newLine: 10, text: '@@ -10,1 +10,1 @@' },
    { kind: 'del', oldLine: 10, newLine: null, text: 'old-b' },
    { kind: 'add', oldLine: null, newLine: 10, text: 'new-b' }
  ]
};

describe('DiffViewer toolbar', () => {
  beforeEach(() => {
    mocks.invokeCommand.mockReset();
    mocks.invokeCommand.mockImplementation(async (command: string) => {
      if (command === 'git_diff') return diff;
      if (command === 'git_status') {
        return [
          { path: 'src/a.ts', status: 'M', isStaged: false, isUnstaged: true, additions: 2, deletions: 2 },
          { path: 'src/b.ts', status: 'M', isStaged: false, isUnstaged: true, additions: 1, deletions: 1 }
        ];
      }
      if (command === 'read_file_content' || command === 'git_file_at_rev') return '';
      return null;
    });
  });

  it('shows hunk progress read-only, syncs next, and navigates files', async () => {
    const onNavigateFile = vi.fn();
    render(
      <DiffViewer
        filePath="/repo/src/a.ts"
        repoPath="/repo"
        onNavigateFile={onNavigateFile}
      />
    );

    const counter = await screen.findByTestId('diff-hunk-counter');
    expect(counter.tagName).toBe('SPAN');
    expect(counter).toHaveTextContent('1 / 2');
    expect(counter.querySelector('select')).toBeNull();

    const nextHunk = screen.getByTestId('diff-next-hunk');
    expect(nextHunk).toBeEnabled();
    fireEvent.click(nextHunk);
    await waitFor(() => expect(counter).toHaveTextContent('2 / 2'));

    await waitFor(() => expect(screen.getByTestId('diff-next-file')).toBeEnabled());
    fireEvent.click(screen.getByTestId('diff-next-file'));
    expect(onNavigateFile).toHaveBeenCalledWith('src/b.ts');
  });

  it('requests only the selected commits patch', async () => {
    mocks.invokeCommand.mockImplementation(async (command: string) => {
      if (command === 'git_commits_diff') return diff;
      if (command === 'git_commits_files') {
        return [{ path: 'src/a.ts', status: 'M', additions: 2, deletions: 2 }];
      }
      if (command === 'git_status') return [];
      if (command === 'read_file_content' || command === 'git_file_at_rev') return '';
      return null;
    });

    render(
      <DiffViewer
        filePath="src/a.ts"
        repoPath="/repo"
        selectedCommits={['c3', 'c1']}
      />
    );

    expect((await screen.findAllByText('2 selected commits')).length).toBeGreaterThan(0);
    expect(mocks.invokeCommand).toHaveBeenCalledWith('git_commits_diff', {
      repoPath: '/repo',
      commits: ['c3', 'c1'],
      path: 'src/a.ts'
    });
    expect(mocks.invokeCommand).toHaveBeenCalledWith('git_commits_files', {
      repoPath: '/repo',
      commits: ['c3', 'c1']
    });
  });
});
