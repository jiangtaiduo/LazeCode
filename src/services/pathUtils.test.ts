import { describe, expect, it } from 'vitest';
import { lspRootFor, parentOfPath } from './pathUtils';

describe('lspRootFor', () => {
  it('目录工作区使用工作区本身', () => {
    expect(lspRootFor('/Users/tester/work/demo', false, '')).toBe('/Users/tester/work/demo');
  });

  it('非 Git 单文件工作区使用父目录，不能把文件当 cwd/root', () => {
    expect(parentOfPath('/Users/tester/work/demo/main.go')).toBe('/Users/tester/work/demo');
    expect(lspRootFor('/Users/tester/work/demo/main.go', true, '')).toBe('/Users/tester/work/demo');
  });

  it('Git 仓库根优先于工作区/文件父目录', () => {
    expect(lspRootFor('/repo/sub/main.go', true, '/repo')).toBe('/repo');
  });
});
