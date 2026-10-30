/**
 * Compare Branches 的纯数据层。
 *
 * 这一屏不是 Compare Versions 的文件 diff：两侧各是一条独立的提交日志，
 * 因此过滤、文件树、详情日期都必须按“某一条分支独有提交”的语义处理。
 * 抽成纯函数便于锁定边界（正则、大小写、相对日期、目录计数），
 * 组件只负责渲染与交互。
 */

import type { GitCommit, GitCommitFile } from '../types';

export type CompareDateFilter = 'all' | 'today' | 'week' | 'month';

export interface CompareCommitFilter {
  query: string;
  useRegex: boolean;
  caseSensitive: boolean;
  author: string;
  date: CompareDateFilter;
  /** 按提交携带的分支/标签名过滤；空串 = 全部引用 */
  refName: string;
}

export interface CompareFileTreeNode {
  name: string;
  path: string;
  dirs: CompareFileTreeNode[];
  files: GitCommitFile[];
  /** 递归文件数（目录行显示 "N files"） */
  fileCount: number;
}

export const EMPTY_COMPARE_FILTER: CompareCommitFilter = {
  query: '',
  useRegex: false,
  caseSensitive: false,
  author: '',
  date: 'all',
  refName: ''
};

const SECONDS_RE = /^\d{9,}$/;

/** 提交日期可能是 git %ct 的秒级字符串，也可能是别处传入的普通字符串。 */
export function commitTimestampMs(raw: string | undefined | null): number | null {
  if (!raw) return null;
  const text = String(raw).trim();
  if (SECONDS_RE.test(text)) {
    const ms = Number(text) * 1000;
    return Number.isFinite(ms) ? ms : null;
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : null;
}

function startOfToday(now: Date): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
}

/** 与 PyCharm 的 All / Today / Last 7 days / Last 30 days 对齐。 */
export function dateFilterCutoff(kind: CompareDateFilter, now: Date = new Date()): number | null {
  if (kind === 'all') return null;
  if (kind === 'today') return startOfToday(now);
  const days = kind === 'week' ? 7 : 30;
  return now.getTime() - days * 24 * 60 * 60 * 1000;
}

function makeTextMatcher(query: string, useRegex: boolean, caseSensitive: boolean) {
  const needle = query.trim();
  if (!needle) return () => true;
  if (useRegex) {
    try {
      const re = new RegExp(needle, caseSensitive ? '' : 'i');
      return (text: string) => re.test(text);
    } catch {
      // 非法正则不应该把异常抛到 React 渲染里；显示为空结果更接近搜索工具行为。
      return () => false;
    }
  }
  const normalized = caseSensitive ? needle : needle.toLowerCase();
  return (text: string) => {
    const value = caseSensitive ? text : text.toLowerCase();
    return value.includes(normalized);
  };
}

/** 日志搜索只针对提交正文/哈希/作者/引用，不触碰文件内容。 */
export function filterCompareCommits(
  commits: GitCommit[],
  filter: CompareCommitFilter,
  now: Date = new Date()
): GitCommit[] {
  const matchesText = makeTextMatcher(filter.query, filter.useRegex, filter.caseSensitive);
  const cutoff = dateFilterCutoff(filter.date, now);

  return commits.filter(commit => {
    if (filter.author && commit.author !== filter.author) return false;
    if (filter.refName) {
      const tags = commit.branchTags ?? [];
      if (!tags.some(tag => tag.name === filter.refName)) return false;
    }
    if (cutoff !== null) {
      const ms = commitTimestampMs(commit.date);
      if (ms === null || ms < cutoff) return false;
    }
    if (!filter.query.trim()) return true;
    const refs = (commit.branchTags ?? []).map(tag => tag.name).join(' ');
    return matchesText(
      [commit.message, commit.shortHash, commit.hash, commit.author, commit.authorEmail, refs].join('\n')
    );
  });
}

/** 作者下拉候选：当前两侧日志里出现过的作者，保持首次出现顺序。 */
export function collectCompareAuthors(commits: GitCommit[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const commit of commits) {
    if (!commit.author || seen.has(commit.author)) continue;
    seen.add(commit.author);
    out.push(commit.author);
  }
  return out;
}

/** 引用下拉候选：提交上携带的分支/标签名。 */
export function collectCompareRefs(commits: GitCommit[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const commit of commits) {
    for (const tag of commit.branchTags ?? []) {
      if (!tag.name || seen.has(tag.name)) continue;
      seen.add(tag.name);
      out.push(tag.name);
    }
  }
  return out;
}

/** 提交详情里 "In N branches" 只数分支引用，不把 tag/stash 算成 branch。 */
export function branchNamesOf(commit: GitCommit | null | undefined): string[] {
  if (!commit) return [];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const tag of commit.branchTags ?? []) {
    if (tag.type === 'tag' || tag.type === 'stash') continue;
    if (!tag.name || seen.has(tag.name)) continue;
    seen.add(tag.name);
    out.push(tag.name);
  }
  return out;
}

/** 目录树：目录计数按递归文件数，顺序按名称稳定排序（不依赖文件输入顺序）。 */
export function buildCompareFileTree(files: GitCommitFile[]): CompareFileTreeNode {
  const root: CompareFileTreeNode = { name: '', path: '', dirs: [], files: [], fileCount: 0 };
  for (const file of files) {
    const parts = file.path.replace(/\\/g, '/').split('/').filter(Boolean);
    let current = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const name = parts[i];
      const path = parts.slice(0, i + 1).join('/');
      let next = current.dirs.find(dir => dir.name === name);
      if (!next) {
        next = { name, path, dirs: [], files: [], fileCount: 0 };
        current.dirs.push(next);
      }
      current = next;
    }
    current.files.push(file);
  }

  const countAndSort = (node: CompareFileTreeNode): number => {
    node.dirs.sort((a, b) => a.name.localeCompare(b.name));
    node.files.sort((a, b) => a.path.localeCompare(b.path));
    let count = node.files.length;
    for (const dir of node.dirs) count += countAndSort(dir);
    node.fileCount = count;
    return count;
  };
  countAndSort(root);
  return root;
}

/** 目录路径全集：Collapse All 需要把所有层级都标记为折叠。 */
export function allCompareDirPaths(node: CompareFileTreeNode): string[] {
  const out: string[] = [];
  const walk = (current: CompareFileTreeNode) => {
    for (const dir of current.dirs) {
      out.push(dir.path);
      walk(dir);
    }
  };
  walk(node);
  return out;
}

/**
 * 按文件树的实际渲染顺序展开路径：每个节点先递归目录，再输出本层文件。
 * Diff 的“下一处差异”跨文件跳转必须用这个顺序；直接用 `git diff` 的
 * 路径序会在目录之间跳错文件。
 */
export function flattenCompareFileTree(node: CompareFileTreeNode): string[] {
  const out: string[] = [];
  const walk = (current: CompareFileTreeNode) => {
    for (const dir of current.dirs) walk(dir);
    for (const file of current.files) out.push(file.path);
  };
  walk(node);
  return out;
}

/** Paths 过滤：支持目录前缀（`backend` 命中 `backend/...`），不做 glob 展开。 */
export function filterCompareFilesByPath(files: GitCommitFile[], path: string): GitCommitFile[] {
  const needle = path.trim().replace(/\\/g, '/').replace(/\/+$/, '');
  if (!needle) return files;
  return files.filter(file => {
    const value = file.path.replace(/\\/g, '/');
    return value === needle || value.startsWith(`${needle}/`);
  });
}

/** 详情底部时间：PyCharm 使用 `YYYY/M/D at HH:mm`。 */
export function formatCompareDetailsDate(raw: string | undefined | null): string {
  if (!raw) return '';
  const ms = commitTimestampMs(raw);
  if (ms === null) return String(raw);
  const date = new Date(ms);
  if (Number.isNaN(date.getTime())) return String(raw);
  const hh = String(date.getHours()).padStart(2, '0');
  const mm = String(date.getMinutes()).padStart(2, '0');
  return `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()} at ${hh}:${mm}`;
}
