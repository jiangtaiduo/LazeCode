import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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

// tsconfig lib 是 ES2022，没有 Promise.withResolvers：用执行器形态的 deferred。
function deferredDiff() {
  let resolve!: (value: FileDiff) => void;
  const promise = new Promise<FileDiff>(r => {
    resolve = r;
  });
  return { promise, resolve };
}

const diffB: FileDiff = {
  path: 'src/b.ts',
  oldPath: 'src/b.ts',
  isNew: false,
  isDeleted: false,
  isBinary: false,
  additions: 1,
  deletions: 1,
  lines: [
    { kind: 'hunk', oldLine: 1, newLine: 1, text: '@@ -1,1 +1,1 @@' },
    { kind: 'del', oldLine: 1, newLine: null, text: 'old-b' },
    { kind: 'add', oldLine: null, newLine: 1, text: 'new-b' }
  ]
};

// 切文件时上一个文件的内容刻意保留在挂载着的 DiffEditor 里（避免销毁重建
// model 的白闪），代价是 loading 期间会短暂残留旧内容——必须被不透明遮罩
// 盖住，否则表现为「闪现上一个文件的上次滚动位置」。
describe('DiffViewer loading veil', () => {
  beforeEach(() => {
    mocks.invokeCommand.mockReset();
    mocks.invokeCommand.mockImplementation(async (command: string) => {
      if (command === 'git_status') {
        return [
          { path: 'src/a.ts', status: 'M', isStaged: false, isUnstaged: true },
          { path: 'src/b.ts', status: 'M', isStaged: false, isUnstaged: true }
        ];
      }
      if (command === 'read_file_content' || command === 'git_file_at_rev') return '';
      return null;
    });
  });

  it('covers stale content while the next file loads, then reveals the new file', async () => {
    const pendingA = deferredDiff();
    const pendingB = deferredDiff();
    mocks.invokeCommand.mockImplementation(async (command: string, args?: { path?: string }) => {
      if (command === 'git_diff') {
        return args?.path === '/repo/src/a.ts' ? pendingA.promise : pendingB.promise;
      }
      if (command === 'git_status') {
        return [
          { path: 'src/a.ts', status: 'M', isStaged: false, isUnstaged: true },
          { path: 'src/b.ts', status: 'M', isStaged: false, isUnstaged: true }
        ];
      }
      if (command === 'read_file_content' || command === 'git_file_at_rev') return '';
      return null;
    });

    const { rerender } = render(<DiffViewer filePath="/repo/src/a.ts" repoPath="/repo" />);

    // A 还在加载：遮罩在
    expect(document.querySelector('.diff-loading-veil')).not.toBeNull();
    await act(async () => pendingA.resolve(diff));
    await waitFor(() => expect(document.querySelector('.diff-loading-veil')).toBeNull());
    expect(screen.getByText(/old-a/)).toBeTruthy();

    // 切到 B：A 的内容仍挂着（编辑器不重建），但必须被遮罩盖住
    rerender(<DiffViewer filePath="/repo/src/b.ts" repoPath="/repo" />);
    expect(document.querySelector('.diff-loading-veil')).not.toBeNull();
    await act(async () => pendingB.resolve(diffB));
    await waitFor(() => expect(screen.getByText(/old-b/)).toBeTruthy());
    expect(document.querySelector('.diff-loading-veil')).toBeNull();
  });

  it('keeps the veil while loading the very first file', async () => {
    const pending = deferredDiff();
    mocks.invokeCommand.mockImplementation(async (command: string) => {
      if (command === 'git_diff') return pending.promise;
      if (command === 'git_status') return [];
      if (command === 'read_file_content' || command === 'git_file_at_rev') return '';
      return null;
    });
    render(<DiffViewer filePath="/repo/src/a.ts" repoPath="/repo" />);
    expect(document.querySelector('.diff-loading-veil')).not.toBeNull();
    await act(async () => pending.resolve(diff));
    await waitFor(() => expect(document.querySelector('.diff-loading-veil')).toBeNull());
  });
});
