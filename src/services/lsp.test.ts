import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invokeCommand, listen } = vi.hoisted(() => ({
  invokeCommand: vi.fn(),
  listen: vi.fn()
}));

vi.mock('./tauriBridge', () => ({
  isTauriEnvironment: () => true,
  invokeCommand
}));

vi.mock('@tauri-apps/api/event', () => ({ listen }));

vi.mock('monaco-editor', () => {
  class MonacoLspClient {
    constructor(_transport: unknown) {}
    createFeatures() {
      return { dispose: vi.fn() };
    }
    dispose() {}
  }
  return { lsp: { MonacoLspClient } };
});

const calls: string[] = [];

/** 每次测试重新加载 lsp 模块：清空 generation / 队列 / clients 等模块级状态。 */
async function loadLsp() {
  vi.resetModules();
  return import('./lsp');
}

const startArgs = () =>
  invokeCommand.mock.calls
    .filter(([cmd]) => cmd === 'lsp_server_start')
    .map(([, args]) =>
      args && typeof args === 'object' && 'root' in args ? String(args.root) : ''
    );

describe('LSP lifecycle queue', () => {
  beforeEach(() => {
    calls.length = 0;
    invokeCommand.mockReset();
    invokeCommand.mockImplementation(async (cmd: string) => {
      calls.push(cmd);
      return undefined;
    });
    listen.mockReset();
    listen.mockResolvedValue(() => {});
  });

  it('start 后 reset 按顺序执行 stop', async () => {
    const lsp = await loadLsp();
    await lsp.ensureLspForLanguage('go', '/ws/a');
    expect(calls).toEqual(['lsp_server_start']);

    await lsp.resetLsp();
    expect(calls).toEqual(['lsp_server_start', 'lsp_server_stop']);
  });

  it('reset 之后的新 ensure 排在 stop 之后，不会误杀', async () => {
    const lsp = await loadLsp();
    await lsp.ensureLspForLanguage('go', '/ws/a');

    const reset = lsp.resetLsp();
    const ensureB = lsp.ensureLspForLanguage('go', '/ws/b');
    await Promise.all([reset, ensureB]);

    expect(calls).toEqual([
      'lsp_server_start',
      'lsp_server_stop',
      'lsp_server_start'
    ]);
    expect(startArgs()).toEqual(['/ws/a', '/ws/b']);
  });

  it('isCurrent 失效时不启动（快速切换的旧代次）', async () => {
    const lsp = await loadLsp();
    await lsp.ensureLspForLanguage('go', '/ws/a', () => false);
    expect(invokeCommand).not.toHaveBeenCalled();
  });

  it('在途 start 遇到 reset 会自停，新 root 的 start 最终生效', async () => {
    const lsp = await loadLsp();
    let releaseStart!: () => void;
    const gate = new Promise<void>(resolve => {
      releaseStart = resolve;
    });
    let startedResolve!: () => void;
    const started = new Promise<void>(resolve => {
      startedResolve = resolve;
    });

    invokeCommand.mockImplementation(async (cmd: string) => {
      calls.push(cmd);
      if (cmd === 'lsp_server_start' && startArgs().length === 1) {
        startedResolve();
        await gate;
      }
      return undefined;
    });

    const first = lsp.ensureLspForLanguage('go', '/ws/a');
    await started;

    const reset = lsp.resetLsp();
    const second = lsp.ensureLspForLanguage('go', '/ws/b');
    releaseStart();
    await Promise.all([first, reset, second]);

    // 旧 root 的启动被至少一次 stop 覆盖，最后一次启动属于新 root
    expect(calls.filter(c => c === 'lsp_server_stop').length).toBeGreaterThanOrEqual(1);
    expect(startArgs().at(-1)).toBe('/ws/b');
    expect(calls.lastIndexOf('lsp_server_start')).toBeGreaterThan(
      calls.lastIndexOf('lsp_server_stop')
    );
  });
});
