import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
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
});
