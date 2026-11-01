import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { DiffViewer } from './DiffViewer';
import type { FileDiff } from './DiffViewer';

const mocks = vi.hoisted(() => ({
  invokeCommand: vi.fn()
}));

/** Monaco 就绪门：默认永不 resolve（旧用例停在轻量占位）；Monaco 路径的
 *  用例先 open() 放行。 */
const monacoGate = vi.hoisted(() => {
  let resolve!: () => void;
  const promise = new Promise<void>(r => {
    resolve = r;
  });
  return { open: () => resolve(), promise };
});

/** 测试驱动假 DiffEditor 的出口：apply = 模拟库侧把文本异步写入 model 并
 *  算完 diff；fire = 只发事件不写内容（模拟滞后到达的旧计算结果）。 */
const monacoTesting = vi.hoisted(() => ({
  latest: null as null | {
    apply: (original: string, modified: string) => void;
    fire: () => void;
  }
}));

vi.mock('../services/tauriBridge', () => ({
  invokeCommand: mocks.invokeCommand
}));

// Monaco 在 jsdom 里不可用：默认让就绪 Promise 永远不 resolve，组件停在轻量占位，
// 但工具栏、hunk 下标和文件导航仍走真实逻辑。
vi.mock('../services/monacoSetup', () => ({
  ensureMonacoReady: () => monacoGate.promise,
  MONACO_THEME_BY_APP_THEME: {},
  UNICODE_HIGHLIGHT_OPTIONS: {}
}));

// 假 DiffEditor：复刻 @monaco-editor/react 的关键时序 —— 初次挂载以 props
// 建 model，diff 计算完成后才发 onDidUpdateDiff；props 变化后在 effect 里
// 异步写 model。正射真实闪现窗口：数据到齐 ≠ 屏上是新文件。
vi.mock('@monaco-editor/react', async () => {
  const React = await import('react');
  class FakePane {
    value = '';
    getValue() {
      return this.value;
    }
    setScrollTop() {}
    deltaDecorations() {
      return [] as string[];
    }
    onDidChangeCursorPosition() {
      return { dispose() {} };
    }
    onDidLayoutChange() {
      return { dispose() {} };
    }
    onDidScrollChange() {
      return { dispose() {} };
    }
  }
  class FakeDiffEditor {
    original = new FakePane();
    modified = new FakePane();
    changes: unknown[] = [];
    listeners: Array<() => void> = [];
    onDidUpdateDiff(fn: () => void) {
      this.listeners.push(fn);
      return { dispose() {} };
    }
    getLineChanges() {
      return this.changes;
    }
    getOriginalEditor() {
      return this.original;
    }
    getModifiedEditor() {
      return this.modified;
    }
    revealFirstDiff() {}
    fire() {
      for (const fn of [...this.listeners]) fn();
    }
    apply(original: string, modified: string) {
      this.original.value = original;
      this.modified.value = modified;
      this.fire();
    }
  }
  let instance: FakeDiffEditor | null = null;
  return {
    DiffEditor: function DiffEditor(props: {
      original: string;
      modified: string;
      onMount: (ed: FakeDiffEditor) => void;
    }) {
      const edRef = React.useRef<FakeDiffEditor | null>(null);
      React.useEffect(() => {
        const ed = new FakeDiffEditor();
        ed.original.value = props.original;
        ed.modified.value = props.modified;
        edRef.current = ed;
        instance = ed;
        props.onMount?.(ed);
        const t = window.setTimeout(() => ed.fire(), 0);
        return () => {
          window.clearTimeout(t);
          if (instance === ed) instance = null;
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps
      }, []);
      // props 变化后不自动写值：写入与事件都由测试显式驱动（apply），避免
      // act 异步冲刷里的 timer 竞态；组件契约只关心「事件确认内容一致才掀」。
      monacoTesting.latest = {
        apply: (o, m) => instance?.apply(o, m),
        fire: () => instance?.fire()
      };
      return <div data-testid="fake-diff-editor" />;
    }
  };
});

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

// Monaco 路径的遮罩时序：数据到齐后 @monaco-editor/react 还要异步写 model、
// 等 diff 算完，期间屏上仍是上一个文件 —— 旧实现（数据一到就掀遮罩）在这
// 里闪现旧页面。新实现等 onDidUpdateDiff 确认内容真正生效才掀。
describe('DiffViewer switch flash (monaco path)', () => {
  const diffBFull = diffB;
  let pendingB: ReturnType<typeof deferredDiff>;

  beforeEach(() => {
    monacoGate.open();
    pendingB = deferredDiff();
    mocks.invokeCommand.mockReset();
    mocks.invokeCommand.mockImplementation(async (command: string, args?: { path?: string; rev?: string }) => {
      if (command === 'git_diff') {
        return args?.path === '/repo/src/a.ts' ? diff : pendingB.promise;
      }
      if (command === 'git_status') {
        return [
          { path: 'src/a.ts', status: 'M', isStaged: false, isUnstaged: true },
          { path: 'src/b.ts', status: 'M', isStaged: false, isUnstaged: true }
        ];
      }
      if (command === 'read_file_content') {
        return args?.path === '/repo/src/a.ts' ? 'A-work' : 'B-work';
      }
      if (command === 'git_file_at_rev') {
        return args?.rev === '' ? (args?.path === 'src/a.ts' ? 'A-index' : 'B-index') : 'A-head';
      }
      return null;
    });
  });

  afterEach(() => {
    monacoTesting.latest = null;
  });

  async function renderSettledA() {
    const utils = render(<DiffViewer filePath="/repo/src/a.ts" repoPath="/repo" />);
    await screen.findByTestId('fake-diff-editor');
    // 挂载后的首个 onDidUpdateDiff（fake 用 timer 模拟「算完」）确认 A 生效
    await waitFor(() => expect(document.querySelector('.diff-loading-veil')).toBeNull());
    return utils;
  }

  it('covers from the frame the target changes and stays covered until Monaco applies the new file', async () => {
    const { rerender } = await renderSettledA();

    // 头窗：目标一变，遮罩同帧盖上（数据还没回来）
    rerender(<DiffViewer filePath="/repo/src/b.ts" repoPath="/repo" />);
    expect(document.querySelector('.diff-loading-veil')).not.toBeNull();

    // 尾窗：patch 与两侧原文都到了，但 Monaco 还没把 B 写进 model ——
    // 旧实现在这里就掀开遮罩，屏上是 A 的内容（用户看到的闪现）。
    await act(async () => pendingB.resolve(diffBFull));
    expect(document.querySelector('.diff-loading-veil')).not.toBeNull();

    // 库侧写入 B 的文本并算完 diff → 遮罩才掀开
    await act(async () => {
      monacoTesting.latest?.apply('B-index', 'B-work');
    });
    await waitFor(() => expect(document.querySelector('.diff-loading-veil')).toBeNull());
  });

  it('ignores a stale diff-updated event that arrives before the new text is applied', async () => {
    const { rerender } = await renderSettledA();
    rerender(<DiffViewer filePath="/repo/src/b.ts" repoPath="/repo" />);
    await act(async () => pendingB.resolve(diffBFull));
    expect(document.querySelector('.diff-loading-veil')).not.toBeNull();

    // 上一个文件的 diff 计算滞后完成：事件到了，但编辑器里还是 A 的文本
    await act(async () => {
      monacoTesting.latest?.fire();
    });
    expect(document.querySelector('.diff-loading-veil')).not.toBeNull();

    await act(async () => {
      monacoTesting.latest?.apply('B-index', 'B-work');
    });
    await waitFor(() => expect(document.querySelector('.diff-loading-veil')).toBeNull());
  });

  it('lifts the veil without any event when a reload brings back identical contents', async () => {
    const { rerender } = await renderSettledA();

    // 同文件 refreshKey 重拉、两侧原文一字不差：库侧不会动 model，也就不会
    // 有 onDidUpdateDiff —— 内容指纹豁免，遮罩必须照常掀开而不是永远转圈。
    rerender(<DiffViewer filePath="/repo/src/a.ts" repoPath="/repo" refreshKey={1} />);
    await waitFor(() => expect(document.querySelector('.diff-loading-veil')).toBeNull());
  });
});
