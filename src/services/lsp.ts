// Monaco ↔ 原生语言服务器桥。
//
// Rust 侧 lsp.rs 负责 spawn + Content-Length 分帧；这里实现 Monaco
// MonacoLspClient 需要的 IMessageTransport：send 走 lsp_server_send，
// 收到的 lsp://message 事件解析成 JSON-RPC message。
//
// 服务器按「工作区根 + language」缓存；切换项目时 dispose 前端 features
// 并停止 Rust 进程，避免 provider 重复注册与旧 root 串线。

import { listen } from '@tauri-apps/api/event';
import { invokeCommand, isTauriEnvironment } from './tauriBridge';

interface LspWireMessage {
  language: string;
  message: string;
}

interface Disposable {
  dispose(): void;
}
interface LspClient {
  dispose?(): void;
}
type LspClientCtor = new (transport: unknown) => LspClient;

/** MonacoLspClient 的 feature disposable 不对外暴露；用原型补丁捕获一次。 */
const featureStores = new WeakMap<object, Disposable>();
const patchedCtors = new WeakSet<object>();
function installFeatureCapture(ctor: LspClientCtor): void {
  if (patchedCtors.has(ctor)) return;
  patchedCtors.add(ctor);
  const proto = ctor.prototype as unknown as { createFeatures: () => Disposable };
  const original = proto.createFeatures;
  proto.createFeatures = function (this: object) {
    const created = original.call(this);
    featureStores.set(this, created);
    return created;
  };
}
interface LspNamespace {
  MonacoLspClient: LspClientCtor;
}
interface InitializeRequest {
  method: 'initialize';
  params: Record<string, unknown>;
}

interface LspCandidate {
  command: string;
  args: string[];
}

/**
 * 常用语言服务器。按顺序尝试，第一个能 spawn 的生效。
 * 未安装全部候选时只记一次日志，不阻塞编辑（Monaco 内置 TS/JSON/CSS 仍可用）。
 */
const CANDIDATES: Record<string, LspCandidate[]> = {
  python: [
    { command: 'pyright-langserver', args: ['--stdio'] },
    { command: 'basedpyright-langserver', args: ['--stdio'] },
    { command: 'pylsp', args: [] }
  ],
  go: [{ command: 'gopls', args: [] }],
  rust: [{ command: 'rust-analyzer', args: [] }],
  c: [{ command: 'clangd', args: [] }],
  cpp: [{ command: 'clangd', args: [] }],
  java: [{ command: 'jdtls', args: [] }],
  ruby: [{ command: 'solargraph', args: ['stdio'] }],
  php: [{ command: 'intelephense', args: ['--stdio'] }],
  kotlin: [{ command: 'kotlin-language-server', args: [] }],
  swift: [{ command: 'sourcekit-lsp', args: [] }],
  lua: [{ command: 'lua-language-server', args: [] }]
};

function pathToFileUri(path: string): string {
  const normalized = path.replace(/\\/g, '/');
  const withSlash = normalized.startsWith('/') ? normalized : `/${normalized}`;
  return `file://${encodeURI(withSlash).replace(/#/g, '%23').replace(/\?/g, '%3F')}`;
}

/** 一个工作区/语言一个客户端。dispose 时同时撤销 Monaco providers。 */
class ManagedClient {
  disposed = false;

  constructor(
    private readonly client: LspClient,
    private readonly transport: TauriLspTransport
  ) {}

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    try {
      featureStores.get(this.client)?.dispose();
      this.client.dispose?.();
    } catch {
      /* feature disposable 可能已失效 */
    }
    this.transport.dispose();
  }
}

/**
 * IMessageTransport 的 Tauri 实现。state 对 Monaco 客户端只读；这里始终
 * 报告 open，连接断开由服务器停止/重启流程重建。
 */
class TauriLspTransport {
  readonly state = {
    get value() {
      return { state: 'open' as const };
    },
    get onChange() {
      return (_listener: (value: unknown) => void) => ({ dispose: () => {} });
    }
  };
  private listener: ((message: unknown) => void) | undefined;
  private unlisten: (() => void) | undefined;
  private closed = false;

  constructor(
    private readonly language: string,
    private readonly root: string
  ) {}

  async start(): Promise<void> {
    const unlisten = await listen<LspWireMessage>('lsp://message', ev => {
      if (ev.payload?.language !== this.language || this.closed) return;
      try {
        this.listener?.(JSON.parse(ev.payload.message));
      } catch {
        // 非 JSON 帧忽略：server 日志类输出不应打断协议流
      }
    });
    this.unlisten = unlisten;
  }

  setListener(listener: ((message: unknown) => void) | undefined): void {
    this.listener = listener;
  }

  async send(message: unknown): Promise<void> {
    if (this.closed) throw new Error('LSP transport closed');
    // MonacoLspClient 固定传 rootUri:null；在发出 initialize 前补上真实根，
    // 否则 pyright/gopls 无法建立工作区索引。
    if (
      message &&
      typeof message === 'object' &&
      'method' in message &&
      message.method === 'initialize' &&
      'params' in message &&
      message.params &&
      typeof message.params === 'object'
    ) {
      const init = message as InitializeRequest;
      init.params.rootUri = pathToFileUri(this.root);
      init.params.rootPath = this.root;
      init.params.processId = null;
      init.params.workspaceFolders = [
        {
          uri: pathToFileUri(this.root),
          name: this.root.split(/[\\/]/).filter(Boolean).pop() || this.root
        }
      ];
    }
    await invokeCommand('lsp_server_send', {
      language: this.language,
      message: JSON.stringify(message)
    });
  }

  toString(): string {
    return `TauriLspTransport(${this.language})`;
  }

  dispose(): void {
    this.closed = true;
    this.unlisten?.();
    this.unlisten = undefined;
    this.listener = undefined;
  }
}

// 生命周期按「窗口 + language」唯一：Rust 侧的 key 是 `window::language`
// （lsp_server_start/stop/send 都不带 root），所以前端也按 language 缓存。
// 一个窗口同一语言只允许一个 root；工作区切换必须 reset 后才能换 root。
//
// 所有 start/stop 都排进同一条串行队列，并用 generation 让在途启动失效：
// 旧实现 `void resetLsp()` 后立刻 `ensure` 时，迟到的 stop 会把新进程杀掉，
// 或反过来留下旧 root 的进程。队列 + generation 把时序变成确定行为。
const clients = new Map<string, ManagedClient>();
/** language -> 失败时的 root：同 root 不重试，换 root（切项目）允许再试。 */
const failedRoots = new Map<string, string>();
/** 已成功拉起过的 language：reset 时按执行时的集合逐个 stop。 */
const knownLanguages = new Set<string>();

let generation = 0;
let lifecycle: Promise<void> = Promise.resolve();

/** 排进串行队列；失败也继续执行后续任务（避免一个坏 server 卡死整条链）。 */
function enqueue(task: () => Promise<void>): Promise<void> {
  lifecycle = lifecycle.then(task, task);
  return lifecycle;
}

export function isLspLanguage(language: string): boolean {
  return language in CANDIDATES;
}

/**
 * 确保语言服务器已启动且 Monaco provider 已注册。
 *
 * `isCurrent` 是工作区代次守卫：调用方在切换项目的同步路径里递增
 * `workspaceSeqRef`，因此排队期间已经过期的启动会在执行前/启动后被丢弃，
 * 不会把旧项目的语言服务器带进新项目。
 *
 * 失败是正常情况（用户没装 gopls/pyright）：只记一次日志，不阻塞编辑。
 */
export async function ensureLspForLanguage(
  language: string,
  root: string,
  isCurrent: () => boolean = () => true
): Promise<void> {
  if (!isTauriEnvironment() || !root || !isLspLanguage(language)) return;
  const gen = generation;
  return enqueue(async () => {
    // 排队期间工作区已切换：这个启动属于旧项目，直接放弃。
    if (gen !== generation || !isCurrent()) return;
    if (clients.has(language) || failedRoots.get(language) === root) return;

    failedRoots.set(language, root); // 先占位，防并发重复 spawn；成功后删除。

    // 动态 import 例外：monaco 内核约 4MB，必须保持懒加载（见 monacoSetup.ts）。
    const monaco = await import('monaco-editor');
    if (gen !== generation || !isCurrent()) return;
    // monaco-editor 的 index 把 LSP client 挂在 `lsp` 命名空间；类型声明在
    // 外部包中，这里用具名 unknown cast 收口一次。
    const monacoWithLsp = monaco as unknown as { lsp?: LspNamespace };
    const lsp = monacoWithLsp.lsp;
    if (!lsp) return;

    for (const candidate of CANDIDATES[language]) {
      const transport = new TauriLspTransport(language, root);
      try {
        await transport.start();
        await invokeCommand('lsp_server_start', {
          language,
          command: candidate.command,
          args: candidate.args,
          root
        });
        // 从这里开始 Rust 侧已有进程：reset 的 stop 任务必须知道。
        knownLanguages.add(language);
        // 捕获 createFeatures 返回的 disposable，切工作区时才能真正撤销 provider。
        installFeatureCapture(lsp.MonacoLspClient);
        const client = new lsp.MonacoLspClient(transport);
        const managed = new ManagedClient(client, transport);
        // 启动期间工作区又切走了：立即自停，别把旧 root 的进程留给新项目。
        if (gen !== generation || !isCurrent()) {
          managed.dispose();
          try {
            await invokeCommand('lsp_server_stop', { language });
          } catch {
            // 进程可能已自己退出
          }
          return;
        }
        clients.set(language, managed);
        failedRoots.delete(language);
        console.info(`[lsp] ${language} started with ${candidate.command}`);
        return;
      } catch (err) {
        transport.dispose();
        console.debug(`[lsp] ${language} candidate ${candidate.command} failed`, err);
      }
    }
    console.info(`[lsp] no language server available for ${language}`);
  });
}

/**
 * 工作区切换/关闭：撤销 Monaco providers 并停止 Rust 进程。
 *
 * 同步清空前端表、递增 generation，stop 任务排队执行。这样：
 * - 任何在途 start 完成后会发现 generation 已变，自己停掉；
 * - 任何 reset 之后发起的 ensure 都排在本轮 stop 之后，不会被误杀。
 */
export function resetLsp(): Promise<void> {
  generation += 1;
  const entries = [...clients.values()];
  clients.clear();
  failedRoots.clear();
  return enqueue(async () => {
    for (const managed of entries) managed.dispose();
    // 执行时再取 known：排队期间完成的 start 也会被这一轮 stop 覆盖。
    for (const language of [...knownLanguages]) {
      try {
        await invokeCommand('lsp_server_stop', { language });
      } catch {
        // 进程可能已自己退出：stop 返回错误是正常清理路径
      }
    }
    knownLanguages.clear();
  });
}
