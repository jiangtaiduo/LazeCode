import { describe, expect, it } from 'vitest';
import {
  allCompareDirPaths,
  branchNamesOf,
  buildCompareFileTree,
  collectCompareAuthors,
  collectCompareRefs,
  dateFilterCutoff,
  EMPTY_COMPARE_FILTER,
  filterCompareCommits,
  filterCompareFilesByPath,
  flattenCompareFileTree,
  formatCompareDetailsDate
} from './compareBranches';
import type { GitCommit, GitCommitFile } from '../types';

const commit = (overrides: Partial<GitCommit>): GitCommit => ({
  hash: 'a'.repeat(40),
  shortHash: 'aaaaaaa',
  message: 'subject',
  author: 'Alice',
  authorEmail: 'alice@example.com',
  date: String(Math.floor(Date.now() / 1000)),
  graphTrack: 0,
  ...overrides
});

const file = (path: string, status = 'M', additions = 1, deletions = 0): GitCommitFile => ({
  path,
  status,
  additions,
  deletions
});

const fixedNow = new Date(2026, 4, 20, 12, 0, 0);
const seconds = (date: Date) => String(Math.floor(date.getTime() / 1000));

describe('Compare Branches filtering', () => {
  const commits = [
    commit({
      hash: 'f'.repeat(40),
      shortHash: 'fffffff',
      message: 'Fix video parser',
      author: 'Alice',
      date: seconds(new Date(2026, 4, 20, 10, 0, 0)),
      branchTags: [{ name: 'feature/video', type: 'remote' }]
    }),
    commit({
      hash: 'e'.repeat(40),
      shortHash: 'eeeeeee',
      message: 'Add tests',
      author: 'Bob',
      date: seconds(new Date(2026, 4, 19, 10, 0, 0)),
      branchTags: [{ name: 'main', type: 'main' }]
    }),
    commit({
      hash: 'd'.repeat(40),
      shortHash: 'ddddddd',
      message: 'Old cleanup',
      author: 'Alice',
      date: seconds(new Date(2026, 3, 1, 10, 0, 0))
    })
  ];

  it('matches text and hash, case sensitive when requested', () => {
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, query: 'video' }, fixedNow).map(c => c.hash)
    ).toEqual([commits[0].hash]);
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, query: 'Video', caseSensitive: true }, fixedNow)
    ).toEqual([]);
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, query: 'eeeeeee' }, fixedNow).map(c => c.hash)
    ).toEqual([commits[1].hash]);
  });

  it('supports regex and treats an invalid regex as no matches', () => {
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, query: '^Add\\s', useRegex: true }, fixedNow).map(
        c => c.hash
      )
    ).toEqual([commits[1].hash]);
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, query: '[', useRegex: true }, fixedNow)
    ).toEqual([]);
  });

  it('filters by author, ref and relative date', () => {
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, author: 'Bob' }, fixedNow).map(c => c.hash)
    ).toEqual([commits[1].hash]);
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, refName: 'feature/video' }, fixedNow).map(
        c => c.hash
      )
    ).toEqual([commits[0].hash]);
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, date: 'today' }, fixedNow).map(c => c.hash)
    ).toEqual([commits[0].hash]);
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, date: 'week' }, fixedNow).map(c => c.hash)
    ).toEqual([commits[0].hash, commits[1].hash]);
    expect(
      filterCompareCommits(commits, { ...EMPTY_COMPARE_FILTER, date: 'month' }, fixedNow).map(c => c.hash)
    ).toEqual([commits[0].hash, commits[1].hash]);
  });

  it('computes date cutoffs from local midnight', () => {
    expect(dateFilterCutoff('all', fixedNow)).toBeNull();
    expect(dateFilterCutoff('today', fixedNow)).toBe(new Date(2026, 4, 20).getTime());
    expect(dateFilterCutoff('week', fixedNow)).toBe(fixedNow.getTime() - 7 * 86400000);
  });

  it('collects authors and refs once in first-seen order', () => {
    expect(collectCompareAuthors(commits)).toEqual(['Alice', 'Bob']);
    expect(collectCompareRefs(commits)).toEqual(['feature/video', 'main']);
  });
});

describe('Compare Branches file tree', () => {
  const files = [
    file('backend/app/api.py'),
    file('backend/tests/test_api.py'),
    file('backend/requirements.txt', 'M', 2, 1),
    file('frontend/src/App.tsx', 'A'),
    file('README.md')
  ];

  it('builds recursive directory counts and stable ordering', () => {
    const tree = buildCompareFileTree(files);
    expect(tree.dirs.map(d => d.name)).toEqual(['backend', 'frontend']);
    const backend = tree.dirs[0];
    expect(backend.fileCount).toBe(3);
    expect(backend.dirs.map(d => d.name)).toEqual(['app', 'tests']);
    expect(backend.dirs[0].fileCount).toBe(1);
    expect(tree.files.map(f => f.path)).toEqual(['README.md']);
    expect(allCompareDirPaths(tree)).toEqual([
      'backend',
      'backend/app',
      'backend/tests',
      'frontend',
      'frontend/src'
    ]);
    expect(flattenCompareFileTree(tree)).toEqual([
      'backend/app/api.py',
      'backend/tests/test_api.py',
      'backend/requirements.txt',
      'frontend/src/App.tsx',
      'README.md'
    ]);
  });

  it('walks range files in tree order, not git path order', () => {
    // 实际 Compare Branches 范围里 src/styles 没有变更；按路径序 App.tsx 会
    // 排在 services 前，但文件树把目录放在本层文件前，下一处跨文件必须到 App.tsx。
    const range = [
      file('src/services/tauriCommands.ts'),
      file('src/App.tsx'),
      file('src/services/tauriBridge.ts'),
      file('src/components/GhosttyTerminal.tsx'),
      file('src-tauri/src/pty.rs')
    ];
    const order = flattenCompareFileTree(buildCompareFileTree(range));
    expect(order).toEqual([
      'src/components/GhosttyTerminal.tsx',
      'src/services/tauriBridge.ts',
      'src/services/tauriCommands.ts',
      'src/App.tsx',
      'src-tauri/src/pty.rs'
    ]);
  });

  it('filters a file list by exact file or directory prefix', () => {
    expect(filterCompareFilesByPath(files, 'backend').map(f => f.path)).toEqual([
      'backend/app/api.py',
      'backend/tests/test_api.py',
      'backend/requirements.txt'
    ]);
    expect(filterCompareFilesByPath(files, 'backend/app/')).toEqual([files[0]]);
    expect(filterCompareFilesByPath(files, 'backend/app.py')).toEqual([]);
  });
});

describe('Compare Branches detail metadata', () => {
  it('only counts real branch refs, not tags or stashes', () => {
    const value = commit({
      branchTags: [
        { name: 'origin/main', type: 'remote' },
        { name: 'main', type: 'main' },
        { name: 'v1.0', type: 'tag' },
        { name: 'stash@{0}', type: 'stash' },
        { name: 'origin/main', type: 'remote' }
      ]
    });
    expect(branchNamesOf(value)).toEqual(['origin/main', 'main']);
    expect(branchNamesOf(null)).toEqual([]);
  });

  it('formats details dates as PyCharm YYYY/M/D at HH:mm', () => {
    const date = new Date(2026, 4, 19, 15, 16, 0);
    expect(formatCompareDetailsDate(String(Math.floor(date.getTime() / 1000)))).toBe('2026/5/19 at 15:16');
    expect(formatCompareDetailsDate('')).toBe('');
    expect(formatCompareDetailsDate('Yesterday 10:00')).toBe('Yesterday 10:00');
  });
});
