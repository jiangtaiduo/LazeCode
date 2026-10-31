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
    mocks.invokeCommand.mockImplementation(async (command: string) => {
      if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
      if (command === 'pty_session_busy') return { busy: true, commands: ['vite', 'vite', 'tsc'] };
      return null;
    });
    const onBusyChange = vi.fn();
    render(<GhosttyTerminal visible projectKey="/repo" openProjects={[]} onBusyChange={onBusyChange} />);
    await waitFor(() => {
      expect(onBusyChange).toHaveBeenCalledWith(true, ['tsc', 'vite']);
    });
  });

  it('reports idle once no session reports busy', async () => {
    mocks.invokeCommand.mockImplementation(async (command: string) => {
      if (command === 'pty_spawn') return { sessionId: 's1', shell: '/bin/zsh' };
      if (command === 'pty_session_busy') return { busy: false, commands: [] };
      return null;
    });
    const onBusyChange = vi.fn();
    render(<GhosttyTerminal visible projectKey="/repo" openProjects={[]} onBusyChange={onBusyChange} />);
    await waitFor(() => {
      expect(onBusyChange).toHaveBeenCalledWith(false, []);
    });
  });
});
