import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, waitFor, act } from '@testing-library/react';
import { GhosttyTerminal } from './GhosttyTerminal';

const mocks = vi.hoisted(() => ({
  invokeCommand: vi.fn()
}));

vi.mock('../services/tauriBridge', () => ({
  invokeCommand: mocks.invokeCommand
}));

const ptyMocks = vi.hoisted(() => ({
  spawnPty: vi.fn(),
  subscribeDirty: vi.fn(),
  subscribeExit: vi.fn(),
  subscribePwd: vi.fn()
}));

vi.mock('../services/pty', () => ({
  spawnPty: ptyMocks.spawnPty,
  subscribeDirty: ptyMocks.subscribeDirty,
  subscribeExit: ptyMocks.subscribeExit,
  subscribePwd: ptyMocks.subscribePwd,
  closePtyByCwd: vi.fn()
}));

describe('GhosttyTerminal busy polling', () => {
  beforeEach(() => {
    mocks.invokeCommand.mockReset();
    ptyMocks.spawnPty.mockReset();
    ptyMocks.subscribeDirty.mockReset();
    ptyMocks.subscribeExit.mockReset();
    ptyMocks.subscribePwd.mockReset();
    ptyMocks.spawnPty.mockResolvedValue({
      sessionId: 's1',
      shell: '/bin/zsh',
      cwd: '/repo',
      write: async () => {},
      resize: async () => {},
      close: async () => {}
    });
    ptyMocks.subscribeDirty.mockResolvedValue(() => {});
    ptyMocks.subscribeExit.mockResolvedValue(() => {});
    ptyMocks.subscribePwd.mockResolvedValue(() => {});
    mocks.invokeCommand.mockImplementation(async (command: string) => {
      if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
      return null;
    });
  });

  it('reports deduplicated running commands up to the parent', async () => {
    mocks.invokeCommand.mockImplementation(async (command: string, args?: { sessionIds?: string[] }) => {
      if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
      if (command === 'pty_sessions_busy') {
        // 组件按前端 pane id 请求：按请求回显
        return (args?.sessionIds ?? []).map(id => ({ sessionId: id, busy: true, commands: ['vite', 'vite', 'tsc'], lastOutputMs: 100 }));
      }
      return null;
    });
    const onBusyChange = vi.fn();
    render(<GhosttyTerminal visible projectKey="/repo" openProjects={[]} onBusyChange={onBusyChange} />);
    await waitFor(() => {
      expect(onBusyChange).toHaveBeenCalledWith(true, ['tsc', 'vite']);
    });
  });

  it('reports idle once no session reports busy', async () => {
    mocks.invokeCommand.mockImplementation(async (command: string, args?: { sessionIds?: string[] }) => {
      if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
      if (command === 'pty_sessions_busy') {
        return (args?.sessionIds ?? []).map(id => ({ sessionId: id, busy: false, commands: [], lastOutputMs: null }));
      }
      return null;
    });
    const onBusyChange = vi.fn();
    render(<GhosttyTerminal visible projectKey="/repo" openProjects={[]} onBusyChange={onBusyChange} />);
    await waitFor(() => {
      expect(onBusyChange).toHaveBeenCalledWith(false, []);
    });
  });

  it('reports per-project activity: 2s 内有输出 = outputting，久未输出 = 只 busy', async () => {
    mocks.invokeCommand.mockImplementation(async (command: string, args?: { sessionIds?: string[] }) => {
      if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
      if (command === 'pty_sessions_busy') {
        return (args?.sessionIds ?? []).map(id => ({ sessionId: id, busy: true, commands: ['vite'], lastOutputMs: 100 }));
      }
      return null;
    });
    const onProjectActivity = vi.fn();
    render(
      <GhosttyTerminal
        visible
        projectKey="/repo"
        openProjects={[]}
        onProjectActivity={onProjectActivity}
      />
    );
    await waitFor(() => {
      expect(onProjectActivity).toHaveBeenCalledWith(
        expect.objectContaining({ '/repo': { busy: true, outputting: true, commands: ['vite'] } })
      );
    });
  });

  it('输出的新鲜度超过 2s 时回落为不 outputting', async () => {
    mocks.invokeCommand.mockImplementation(async (command: string, args?: { sessionIds?: string[] }) => {
      if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
      if (command === 'pty_sessions_busy') {
        return (args?.sessionIds ?? []).map(id => ({ sessionId: id, busy: true, commands: ['sleep'], lastOutputMs: 5000 }));
      }
      return null;
    });
    const onProjectActivity = vi.fn();
    render(
      <GhosttyTerminal
        visible
        projectKey="/repo"
        openProjects={[]}
        onProjectActivity={onProjectActivity}
      />
    );
    await waitFor(() => {
      expect(onProjectActivity).toHaveBeenCalledWith(
        expect.objectContaining({ '/repo': { busy: true, outputting: false, commands: ['sleep'] } })
      );
    });
  });

  // 回归：切换到正在持续输出的项目时，切换会记一次 UI 事件（面板显隐重同步），
  // 下一次轮询若落在 500ms UI 宽限窗内，持续输出会被误判成「交互重绘」——
  // 跑马灯短暂变绿，再过一轮轮询又恢复红。持续输出中的会话不得被该事件压制。
  it('keeps outputting when switching onto a continuously producing console', async () => {
    vi.useFakeTimers();
    try {
      mocks.invokeCommand.mockImplementation(async (command: string, args?: { sessionIds?: string[] }) => {
        if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
        if (command === 'pty_sessions_busy') {
          return (args?.sessionIds ?? []).map(id => ({
            sessionId: id,
            busy: true,
            commands: ['tail'],
            // 每轮探测输出都是新鲜的：模拟持续输出的进程
            lastOutputMs: 50
          }));
        }
        return null;
      });
      const onProjectActivity = vi.fn();
      const view = render(
        <GhosttyTerminal visible projectKey="/alpha" openProjects={[]} onProjectActivity={onProjectActivity} />
      );
      // t≈0：alpha 自建会话，首轮探测 → alpha 红
      await act(async () => {
        await vi.advanceTimersByTimeAsync(50);
      });
      // 开第二个项目的控制台（lazy spawn），再切回：模拟两个都开过的状态
      view.rerender(<GhosttyTerminal visible projectKey="/beta" openProjects={['/alpha']} onProjectActivity={onProjectActivity} />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(100);
      });
      view.rerender(<GhosttyTerminal visible projectKey="/alpha" openProjects={['/beta']} onProjectActivity={onProjectActivity} />);
      await act(async () => {
        // 跨过 t=2000 的例行轮询：alpha、beta 都红（beta 的输出新鲜度已被记录）
        await vi.advanceTimersByTimeAsync(1900);
      });
      // t≈2050 切走；再走到 t≈3650 切回 beta（纯 revisit：无新 tab、无即时探测）
      view.rerender(<GhosttyTerminal visible projectKey="/alpha" openProjects={['/beta']} onProjectActivity={onProjectActivity} />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1600);
      });
      view.rerender(<GhosttyTerminal visible projectKey="/beta" openProjects={['/alpha']} onProjectActivity={onProjectActivity} />);
      // 切换刚发生：跑马灯本来就是红的
      expect(onProjectActivity).toHaveBeenLastCalledWith(
        expect.objectContaining({
          '/alpha': { busy: true, outputting: true, commands: ['tail'] },
          '/beta': { busy: true, outputting: true, commands: ['tail'] }
        })
      );
      // t≈3650 切到 beta，t=4000 的例行轮询落在切换后 500ms 内：
      // 持续输出是真实的，跑马灯必须保持红
      await act(async () => {
        await vi.advanceTimersByTimeAsync(400);
      });
      expect(onProjectActivity).toHaveBeenLastCalledWith(
        expect.objectContaining({
          '/alpha': { busy: true, outputting: true, commands: ['tail'] },
          '/beta': { busy: true, outputting: true, commands: ['tail'] }
        })
      );
      // 再过一轮轮询，同样保持红（不得出现 绿→红 的来回翻转）
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(onProjectActivity).toHaveBeenLastCalledWith(
        expect.objectContaining({
          '/alpha': { busy: true, outputting: true, commands: ['tail'] },
          '/beta': { busy: true, outputting: true, commands: ['tail'] }
        })
      );
      view.unmount();
    } finally {
      vi.useRealTimers();
    }
  });
});
