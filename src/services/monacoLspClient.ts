/**
 * Monaco 与 Tauri 原生 LSP 管道 (lsp.rs) 的适配桥接层。
 *
 * 架构：
 * 1. 监听 Tauri 事件 `lsp://message` 接收后端 pump 出的 JSON-RPC 响应与通知；
 * 2. 通过 `lsp_server_start` / `lsp_server_send` 与各语言的独立 LSP 进程通信；
 * 3. 向 Monaco 注册语言的 HoverProvider / DefinitionProvider / CompletionItemProvider；
 * 4. 监听文档 didOpen / didChange / didClose，维持与 LSP 服务端同步。
 *
 * 容错与防御：
 * - 超时防护：单个 RPC 请求 2.5 秒未响应自动超时，防止卡死 Monaco 主线程；
 * - 缺省服务容错：机器未安装该 server 二进制（如无 pylsp）时静默回退，Monaco 保留原生功能；
 * - 纯前端隔离：支持跨平台，在无 Tauri 环境（测试/Web预览）直接 no-op。
 */
import type * as monacoNs from 'monaco-editor';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { isTauriEnvironment, invokeCommand } from './tauriBridge';

interface LspWirePayload {
  language: string;
  message: string;
}

interface PendingRequest {
  resolve: (value: any) => void;
  reject: (reason: any) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface ServerConfig {
  command: string;
  args: string[];
}

const DEFAULT_SERVERS: Record<string, ServerConfig> = {
  python: { command: 'pylsp', args: [] },
  rust: { command: 'rust-analyzer', args: [] },
  typescript: { command: 'typescript-language-server', args: ['--stdio'] },
  javascript: { command: 'typescript-language-server', args: ['--stdio'] },
  go: { command: 'gopls', args: [] },
};

class MonacoLspBridge {
  private reqId = 1;
  private pending = new Map<number, PendingRequest>();
  private serverStatus = new Map<string, 'unstarted' | 'starting' | 'ready' | 'failed'>();
  private openDocs = new Map<string, { version: number; language: string }>();
  private registeredLangs = new Set<string>();
  /** didChange 防抖的暂存与定时器（按文件）。 */
  private pendingChanges = new Map<string, { language: string; text: string }>();
  private pendingTimers = new Map<string, number>();
  private monacoInstance: typeof monacoNs | null = null;
  private unlistener: UnlistenFn | null = null;
  private currentWorkspaceRoot = '';

  public dispose(): void {
    if (this.unlistener) {
      this.unlistener();
      this.unlistener = null;
    }
  }
  constructor() {
    if (isTauriEnvironment()) {
      void this.initEventListener();
    }
  }

  private async initEventListener(): Promise<void> {
    try {
      this.unlistener = await listen<LspWirePayload>('lsp://message', event => {
        this.handleIncomingMessage(event.payload);
      });
    } catch (err) {
      console.warn('LSP: failed to attach event listener', err);
    }
  }

  public setWorkspaceRoot(root: string): void {
    this.currentWorkspaceRoot = root;
  }

  public registerMonaco(monaco: typeof monacoNs): void {
    this.monacoInstance = monaco;
  }

  private handleIncomingMessage(payload: LspWirePayload): void {
    if (!payload?.message) return;
    try {
      const parsed = JSON.parse(payload.message);
      const hasId = parsed.id !== undefined && parsed.id !== null;

      // 服务端→客户端的**请求**（id + method 同时存在，如
      // workspace/configuration、window/workDoneProgress/create——pylsp /
      // rust-analyzer / tsserver 都会发）必须先于响应匹配处理：客户端与
      // 服务端的 id 空间各自独立且天然重叠，先按 id 匹配会把在途请求错误
      // resolve 成服务端请求的 undefined；服务端请求本身若无人应答，
      // 部分实现会挂起等 configuration。
      if (hasId && typeof parsed.method === 'string') {
        this.respondToServerRequest(payload.language, parsed);
        return;
      }

      // 客户端请求的响应（有 id、无 method）
      if (hasId) {
        const handler = this.pending.get(parsed.id);
        if (handler) {
          clearTimeout(handler.timer);
          this.pending.delete(parsed.id);
          if (parsed.error) {
            handler.reject(new Error(parsed.error.message || 'LSP error'));
          } else {
            handler.resolve(parsed.result);
          }
        }
        return;
      }

      // 服务端主动推送的通知（如 diagnostics 诊断信息）
      if (parsed.method === 'textDocument/publishDiagnostics' && this.monacoInstance) {
        this.applyDiagnostics(parsed.params);
      }
    } catch {
      // 非法 JSON 丢弃
    }
  }

  /** 应答服务端→客户端的请求：已知能力给最小合法结果，未知请求回 null。
   *  丢包只影响服务端的可选功能（进度显示等），不能让它挂死。 */
  private respondToServerRequest(language: string, parsed: any): void {
    let result: unknown = null;
    if (parsed.method === 'workspace/configuration') {
      result = (parsed.params?.items ?? []).map(() => null);
    } else if (parsed.method === 'workspace/applyEdit') {
      result = { applied: false };
    }
    // window/workDoneProgress/create、client/registerCapability 等取默认 null
    const msg = JSON.stringify({ jsonrpc: '2.0', id: parsed.id, result });
    void invokeCommand('lsp_server_send', { language, message: msg }).catch(() => {
      // 服务端可能刚退出：应答丢失无补救动作
    });
  }

  private applyDiagnostics(params: any): void {
    if (!params?.uri || !Array.isArray(params.diagnostics) || !this.monacoInstance) return;
    const uri = this.monacoInstance.Uri.parse(params.uri);
    const model = this.monacoInstance.editor.getModel(uri);
    if (!model) return;

    const markers: monacoNs.editor.IMarkerData[] = params.diagnostics.map((d: any) => {
      const severity =
        d.severity === 1
          ? this.monacoInstance!.MarkerSeverity.Error
          : d.severity === 2
          ? this.monacoInstance!.MarkerSeverity.Warning
          : d.severity === 3
          ? this.monacoInstance!.MarkerSeverity.Info
          : this.monacoInstance!.MarkerSeverity.Hint;

      return {
        severity,
        startLineNumber: (d.range?.start?.line ?? 0) + 1,
        startColumn: (d.range?.start?.character ?? 0) + 1,
        endLineNumber: (d.range?.end?.line ?? 0) + 1,
        endColumn: (d.range?.end?.character ?? 0) + 1,
        message: d.message || '',
        source: d.source || 'LSP',
      };
    });

    this.monacoInstance.editor.setModelMarkers(model, 'lsp', markers);
  }

  public async ensureServer(language: string): Promise<boolean> {
    if (!isTauriEnvironment()) return false;
    const status = this.serverStatus.get(language);
    if (status === 'ready') return true;
    if (status === 'failed') return false;
    if (status === 'starting') {
      // 简单自旋等待最多 1 秒
      for (let i = 0; i < 10; i++) {
        await new Promise(r => setTimeout(r, 100));
        if (this.serverStatus.get(language) === 'ready') return true;
        if (this.serverStatus.get(language) === 'failed') return false;
      }
      return false;
    }

    const conf = DEFAULT_SERVERS[language];
    if (!conf) return false;

    this.serverStatus.set(language, 'starting');
    try {
      await invokeCommand('lsp_server_start', {
        language,
        command: conf.command,
        args: conf.args,
        root: this.currentWorkspaceRoot || '',
      });

      // 发送 initialize
      const rootUri = this.pathToUri(this.currentWorkspaceRoot || '/');
      await this.sendRequest(language, 'initialize', {
        processId: null,
        rootUri,
        capabilities: {
          textDocument: {
            hover: { contentFormat: ['markdown', 'plaintext'] },
            definition: { linkSupport: true },
            completion: {
              completionItem: { snippetSupport: true, documentationFormat: ['markdown', 'plaintext'] },
            },
            publishDiagnostics: { relatedInformation: true },
          },
        },
      });

      // 发送 initialized 通知
      await this.sendNotification(language, 'initialized', {});
      this.serverStatus.set(language, 'ready');
      return true;
    } catch {
      this.serverStatus.set(language, 'failed');
      return false;
    }
  }

  private sendRequest(language: string, method: string, params: any): Promise<any> {
    return new Promise((resolve, reject) => {
      const id = this.reqId++;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`LSP request timeout: ${method}`));
      }, 2500);

      this.pending.set(id, { resolve, reject, timer });

      const msg = JSON.stringify({ jsonrpc: '2.0', id, method, params });
      invokeCommand('lsp_server_send', { language, message: msg }).catch(err => {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(err);
      });
    });
  }

  private sendNotification(language: string, method: string, params: any): Promise<void> {
    const msg = JSON.stringify({ jsonrpc: '2.0', method, params });
    return invokeCommand('lsp_server_send', { language, message: msg });
  }

  public pathToUri(filePath: string): string {
    // 必须与 model 的创建形态严格一致：@monaco-editor/react 用
    // `Uri.parse(path)` 建 model，providers 发 `model.uri.toString()` ——
    // didOpen/didChange 用别的推导（不做百分号编码的裸拼接）时，含
    // `#`/`?`/`%` 的路径会被服务端当成两个不同文档，诊断与补全全失效。
    if (this.monacoInstance) {
      try {
        return this.monacoInstance.Uri.parse(filePath).toString();
      } catch {
        /* 非法路径退回纯字符串拼接 */
      }
    }
    const norm = filePath.replace(/\\/g, '/');
    return norm.startsWith('/') ? `file://${norm}` : `file:///${norm}`;
  }

  public async notifyDocOpen(filePath: string, language: string, text: string): Promise<void> {
    if (!isTauriEnvironment() || !DEFAULT_SERVERS[language]) return;
    const ready = await this.ensureServer(language);
    if (!ready) return;

    const uri = this.pathToUri(filePath);
    this.openDocs.set(filePath, { version: 1, language });
    try {
      await this.sendNotification(language, 'textDocument/didOpen', {
        textDocument: { uri, languageId: language, version: 1, text },
      });
    } catch {
      // 失败静默
    }
  }

  public async notifyDocChange(filePath: string, language: string, text: string): Promise<void> {
    if (!isTauriEnvironment() || !DEFAULT_SERVERS[language]) return;
    const doc = this.openDocs.get(filePath);
    if (!doc) {
      await this.notifyDocOpen(filePath, language, text);
      return;
    }
    // 全文同步（TextDocumentSyncKind.Full）下每个键都是一整篇文档的
    // JSON.stringify + IPC + 服务端整篇 re-parse。按文件尾随防抖：连续输入
    // 合并成一次 didChange（LSP 允许 version 跳变，只要单调），诊断延迟
    // 200ms 不可感知。close 时 flush（见 notifyDocClose），格式化/重命名
    // 读到的服务端缓冲不会落后于最后一次击键。
    const latest = { language, text };
    this.pendingChanges.set(filePath, latest);
    if (this.pendingTimers.has(filePath)) return;
    this.pendingTimers.set(
      filePath,
      window.setTimeout(() => {
        this.pendingTimers.delete(filePath);
        void this.flushDocChange(filePath);
      }, 200)
    );
  }

  /** 立即发送该文件暂存的 didChange（防抖出口与 close 前的 flush 共用）。 */
  private async flushDocChange(filePath: string): Promise<void> {
    const pending = this.pendingChanges.get(filePath);
    const timer = this.pendingTimers.get(filePath);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      this.pendingTimers.delete(filePath);
    }
    if (!pending) return;
    this.pendingChanges.delete(filePath);
    const doc = this.openDocs.get(filePath);
    if (!doc) return; // 已 close：丢弃（服务端马上也会收到 didClose）
    const nextVer = doc.version + 1;
    this.openDocs.set(filePath, { version: nextVer, language: doc.language });
    const uri = this.pathToUri(filePath);
    try {
      await this.sendNotification(doc.language, 'textDocument/didChange', {
        textDocument: { uri, version: nextVer },
        contentChanges: [{ text: pending.text }],
      });
    } catch {
      // 失败静默
    }
  }

  public async notifyDocClose(filePath: string): Promise<void> {
    const doc = this.openDocs.get(filePath);
    if (!doc) {
      // 没打开过也可能有挂着的防抖（open 后立刻 close）：清掉即可
      this.pendingChanges.delete(filePath);
      const t = this.pendingTimers.get(filePath);
      if (t !== undefined) {
        window.clearTimeout(t);
        this.pendingTimers.delete(filePath);
      }
      return;
    }
    // 先把防抖中的最后一次编辑发出去再 close：服务端缓冲不落后于最后一次击键
    await this.flushDocChange(filePath);
    this.openDocs.delete(filePath);
    const uri = this.pathToUri(filePath);
    try {
      await this.sendNotification(doc.language, 'textDocument/didClose', {
        textDocument: { uri },
      });
    } catch {
      // 失败静默
    }
  }

  /** 为指定语言向 Monaco 注册悬停、定义与补全能力 */
  public registerLanguageProviders(monaco: typeof monacoNs, language: string): void {
    if (this.registeredLangs.has(language) || !DEFAULT_SERVERS[language]) return;
    this.registeredLangs.add(language);
    this.monacoInstance = monaco;

    // 1. Hover Provider
    monaco.languages.registerHoverProvider(language, {
      provideHover: async (model, position) => {
        if (this.serverStatus.get(language) !== 'ready') return null;
        try {
          const res = await this.sendRequest(language, 'textDocument/hover', {
            textDocument: { uri: model.uri.toString() },
            position: { line: position.lineNumber - 1, character: position.column - 1 },
          });
          if (!res || !res.contents) return null;
          const contents = Array.isArray(res.contents)
            ? res.contents.map((c: any) => ({ value: typeof c === 'string' ? c : c.value || '' }))
            : [{ value: typeof res.contents === 'string' ? res.contents : res.contents.value || '' }];
          return { contents };
        } catch {
          return null;
        }
      },
    });

    // 2. Definition Provider
    monaco.languages.registerDefinitionProvider(language, {
      provideDefinition: async (model, position) => {
        if (this.serverStatus.get(language) !== 'ready') return null;
        try {
          const res = await this.sendRequest(language, 'textDocument/definition', {
            textDocument: { uri: model.uri.toString() },
            position: { line: position.lineNumber - 1, character: position.column - 1 },
          });
          if (!res) return null;
          const locs = Array.isArray(res) ? res : [res];
          return locs.map((loc: any) => ({
            uri: monaco.Uri.parse(loc.uri || loc.targetUri),
            range: {
              startLineNumber: ((loc.range || loc.targetRange)?.start?.line ?? 0) + 1,
              startColumn: ((loc.range || loc.targetRange)?.start?.character ?? 0) + 1,
              endLineNumber: ((loc.range || loc.targetRange)?.end?.line ?? 0) + 1,
              endColumn: ((loc.range || loc.targetRange)?.end?.character ?? 0) + 1,
            },
          }));
        } catch {
          return null;
        }
      },
    });

    // 3. Completion Provider
    monaco.languages.registerCompletionItemProvider(language, {
      triggerCharacters: ['.', ':', '(', '"', '/', '@'],
      provideCompletionItems: async (model, position) => {
        if (this.serverStatus.get(language) !== 'ready') return { suggestions: [] };
        try {
          const res = await this.sendRequest(language, 'textDocument/completion', {
            textDocument: { uri: model.uri.toString() },
            position: { line: position.lineNumber - 1, character: position.column - 1 },
          });
          const rawItems = Array.isArray(res) ? res : res?.items || [];
          const suggestions = rawItems.map((item: any) => ({
            label: item.label,
            kind: item.kind ? item.kind - 1 : monaco.languages.CompletionItemKind.Property,
            insertText: item.insertText || item.label,
            detail: item.detail,
            documentation: typeof item.documentation === 'string' ? item.documentation : item.documentation?.value,
            range: undefined as any,
          }));
          return { suggestions };
        } catch {
          return { suggestions: [] };
        }
      },
    });

    // 4. Document Formatting Provider (对齐 PyCharm Option+Cmd+L)
    monaco.languages.registerDocumentFormattingEditProvider(language, {
      provideDocumentFormattingEdits: async (model, options) => {
        if (this.serverStatus.get(language) !== 'ready') return [];
        try {
          const res = await this.sendRequest(language, 'textDocument/formatting', {
            textDocument: { uri: model.uri.toString() },
            options: {
              tabSize: options.tabSize,
              insertSpaces: options.insertSpaces,
            },
          });
          if (!Array.isArray(res)) return [];
          return res.map((edit: any) => ({
            range: {
              startLineNumber: (edit.range?.start?.line ?? 0) + 1,
              startColumn: (edit.range?.start?.character ?? 0) + 1,
              endLineNumber: (edit.range?.end?.line ?? 0) + 1,
              endColumn: (edit.range?.end?.character ?? 0) + 1,
            },
            text: edit.newText ?? '',
          }));
        } catch {
          return [];
        }
      },
    });
  }
}

export const monacoLspBridge = new MonacoLspBridge();
