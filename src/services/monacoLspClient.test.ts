import { beforeEach, describe, expect, it, vi } from 'vitest';

// invokeCommand 打桩：记录 lsp_server_send 的外发报文（应答服务端请求用）。
const invokeMock = vi.hoisted(() => vi.fn(async (_cmd: string, _args?: unknown) => undefined));
vi.mock('./tauriBridge', () => ({
  invokeCommand: invokeMock,
  isTauriEnvironment: () => false
}));

import { monacoLspBridge } from './monacoLspClient';

/** 直调私有分发入口（单例在测试环境无 Tauri 监听，消息只能这样喂）。 */
function feed(message: object): void {
  (monacoLspBridge as any).handleIncomingMessage({ language: 'python', message: JSON.stringify(message) });
}

/** 在客户端侧挂一个在途请求（id=5），捕获 resolve/reject。 */
function armPending(id: number): { resolved: unknown[]; rejected: unknown[] } {
  const resolved: unknown[] = [];
  const rejected: unknown[] = [];
  (monacoLspBridge as any).pending.set(id, {
    resolve: (v: unknown) => resolved.push(v),
    reject: (e: unknown) => rejected.push(e),
    timer: 0
  });
  return { resolved, rejected };
}

function sentMessages(): any[] {
  return invokeMock.mock.calls
    .filter(([cmd]) => cmd === 'lsp_server_send')
    .map(([, args]) => JSON.parse((args as any).message));
}

describe('monacoLspClient 消息分发', () => {
  beforeEach(() => {
    invokeMock.mockClear();
    (monacoLspBridge as any).pending.clear();
  });

  it('服务端→客户端请求不得按 id 误消费在途响应，且必须被应答', () => {
    // 客户端在途 hover 请求 id=5；服务端此时发来 id=5 的 configuration 请求
    //（两个 id 空间独立、天然重叠——旧实现先判 id，会把 hover 错误 resolve
    // 成 undefined 且 configuration 永远无人应答）
    const { resolved, rejected } = armPending(5);
    feed({ jsonrpc: '2.0', id: 5, method: 'workspace/configuration', params: { items: [{}, {}] } });

    // 在途请求不得被服务端请求误 resolve
    expect(resolved).toEqual([]);
    expect(rejected).toEqual([]);
    expect((monacoLspBridge as any).pending.get(5)).toBeDefined();

    const replies = sentMessages();
    expect(replies).toHaveLength(1);
    expect(replies[0].id).toBe(5);
    // configuration 按 items 数量逐项应答
    expect(replies[0].result).toEqual([null, null]);
  });

  it('客户端响应仍按 id 正确匹配 resolve', () => {
    const { resolved } = armPending(5);
    feed({ jsonrpc: '2.0', id: 5, result: { contents: 'hover text' } });
    expect(resolved).toEqual([{ contents: 'hover text' }]);
  });

  it('客户端错误响应走 reject 路径', () => {
    const { rejected } = armPending(7);
    feed({ jsonrpc: '2.0', id: 7, error: { code: -32603, message: 'boom' } });
    expect(rejected).toHaveLength(1);
  });

  it('applyEdit 服务端请求应答 applied:false', () => {
    armPending(99); // 不相关在途请求，不应被触碰
    feed({ jsonrpc: '2.0', id: 42, method: 'workspace/applyEdit', params: { label: 'x' } });
    const replies = sentMessages();
    expect(replies[0]).toMatchObject({ id: 42, result: { applied: false } });
  });

  it('服务端通知（publishDiagnostics）继续走诊断分发', () => {
    const fakeMonaco = {
      Uri: { parse: (u: string) => ({ u }) },
      editor: {
        getModel: (uri: { u: string }) => (uri.u === 'file:///a.py' ? { dummy: true } : null),
        setModelMarkers: vi.fn()
      },
      MarkerSeverity: { Error: 8, Warning: 4, Info: 2, Hint: 1 }
    };
    (monacoLspBridge as any).monacoInstance = fakeMonaco;
    try {
      feed({
        jsonrpc: '2.0',
        method: 'textDocument/publishDiagnostics',
        params: {
          uri: 'file:///a.py',
          diagnostics: [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } }, severity: 1, message: 'x' }]
        }
      });
      expect(fakeMonaco.editor.setModelMarkers).toHaveBeenCalledTimes(1);
      expect(invokeMock.mock.calls.filter(([cmd]) => cmd === 'lsp_server_send')).toHaveLength(0);
    } finally {
      (monacoLspBridge as any).monacoInstance = null;
    }
  });
});
