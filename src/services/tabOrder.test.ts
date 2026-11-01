import { describe, expect, it } from 'vitest';
import { applyTabReorder, orderProjectTabs } from './tabOrder';

describe('orderProjectTabs', () => {
  it('表内条目按表序排列，表外条目保持原序补在尾部', () => {
    const tabs = [{ path: '/a' }, { path: '/b' }, { path: '/c' }];
    expect(orderProjectTabs(tabs, ['/c', '/a'])).toEqual([{ path: '/c' }, { path: '/a' }, { path: '/b' }]);
  });

  it('空顺序表保持入参顺序（浏览器初始序 = 打开先后）', () => {
    const tabs = [{ path: '/x' }, { path: '/y' }];
    expect(orderProjectTabs(tabs, [])).toEqual(tabs);
  });

  it('顺序表里的陈旧路径（已关闭项目）不影响结果', () => {
    const tabs = [{ path: '/a' }, { path: '/b' }];
    expect(orderProjectTabs(tabs, ['/gone', '/b', '/a'])).toEqual([{ path: '/b' }, { path: '/a' }]);
  });

  it('不修改入参数组', () => {
    const tabs = [{ path: '/a' }, { path: '/b' }];
    orderProjectTabs(tabs, ['/b']);
    expect(tabs.map(t => t.path)).toEqual(['/a', '/b']);
  });
});

describe('applyTabReorder', () => {
  it('把 from 位置移动到 to 位置', () => {
    expect(applyTabReorder(['a', 'b', 'c', 'd'], 0, 2)).toEqual(['b', 'c', 'a', 'd']);
    expect(applyTabReorder(['a', 'b', 'c', 'd'], 3, 1)).toEqual(['a', 'd', 'b', 'c']);
  });

  it('同位移动返回等值副本，越界钳制到有效区间', () => {
    expect(applyTabReorder(['a', 'b'], 1, 1)).toEqual(['a', 'b']);
    expect(applyTabReorder(['a', 'b'], -5, 99)).toEqual(['b', 'a']);
  });

  it('不修改入参数组', () => {
    const list = ['a', 'b', 'c'];
    applyTabReorder(list, 0, 2);
    expect(list).toEqual(['a', 'b', 'c']);
  });
});
