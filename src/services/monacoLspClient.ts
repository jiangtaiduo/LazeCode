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
      // 处理带 id 的请求响应
      if (typeof parsed.id === 'number') {
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

      // 处理服务端主动推送的通知（如 diagnostics 诊断信息）
      if (parsed.method === 'textDocument/publishDiagnostics' && this.monacoInstance) {
        this.applyDiagnostics(parsed.params);
      }
    } catch {
      // 非法 JSON 丢弃
    }
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
    const nextVer = doc.version + 1;
    this.openDocs.set(filePath, { version: nextVer, language });
    const uri = this.pathToUri(filePath);
    try {
      await this.sendNotification(language, 'textDocument/didChange', {
        textDocument: { uri, version: nextVer },
        contentChanges: [{ text }],
      });
    } catch {
      // 失败静默
    }
  }

  public async notifyDocClose(filePath: string): Promise<void> {
    const doc = this.openDocs.get(filePath);
    if (!doc) return;
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
