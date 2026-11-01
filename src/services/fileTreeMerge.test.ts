import { describe, expect, it } from 'vitest';
import { preserveExpandedChildren } from './fileTreeMerge';
import type { FileNode } from '../types';

const dir = (path: string, children?: FileNode[]): FileNode => ({
  name: path.split('/').pop() ?? path,
  path,
  isDirectory: true,
  ...(children !== undefined ? { children } : {})
});

const file = (path: string): FileNode => ({
  name: path.split('/').pop() ?? path,
  path,
  isDirectory: false
});

describe('preserveExpandedChildren', () => {
  const expanded = new Set(['/p/src', '/p/src/lib']);

  it('截断的展开目录沿用旧 children，未展开的保持截断', () => {
    const prev = [
      dir('/p/src', [
        dir('/p/src/lib', [file('/p/src/lib/old.ts')]),
        dir('/p/docs', [file('/p/docs/a.md')])
      ]), // docs 未展开但旧树里已加载
      file('/p/README.md')
    ];
    // 新读（maxDepth 截断：lib/docs 的 children 都是 undefined）
    const fresh = [dir('/p/src', [dir('/p/src/lib'), dir('/p/docs')]), file('/p/README.md')];
    const merged = preserveExpandedChildren(fresh, prev, expanded);
    const src = merged.find(n => n.path === '/p/src');
    const lib = src?.children?.find(n => n.path === '/p/src/lib');
    const docs = src?.children?.find(n => n.path === '/p/docs');
    expect(lib?.children).toEqual([file('/p/src/lib/old.ts')]);
    expect(docs?.children).toBeUndefined();
  });

  it('新读到的层继续向深处合并', () => {
    const prev = [
      dir('/p/src', [
        dir('/p/src/lib', [dir('/p/src/lib/deep', [file('/p/src/lib/deep/x.ts')])])
      ])
    ];
    // 新读：src 有 children（一层），lib 截断
    const fresh = [dir('/p/src', [dir('/p/src/lib'), file('/p/src/new.ts')])];
    const merged = preserveExpandedChildren(fresh, prev, expanded);
    const lib = merged[0].children?.find(n => n.path === '/p/src/lib');
    expect(lib?.children).toEqual(prev[0].children?.[0].children);
  });

  it('新读为空数组的目录保持为空（真空不沿用旧内容）', () => {
    const prev = [dir('/p/src', [dir('/p/src/lib', [file('/p/src/lib/a.ts')])])];
    const fresh = [dir('/p/src', [])]; // 外部清空了目录
    const merged = preserveExpandedChildren(fresh, prev, expanded);
    expect(merged[0].children).toEqual([]);
  });

  it('新增节点不受影响；prev 缺失时原样返回 fresh', () => {
    const fresh = [dir('/p/src', [dir('/p/src/newdir')]), file('/p/other.txt')];
    expect(preserveExpandedChildren(fresh, [], expanded)).toBe(fresh);
    expect(preserveExpandedChildren(fresh, undefined, expanded)).toBe(fresh);
  });
});
