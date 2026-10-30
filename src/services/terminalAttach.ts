/**
 * 终端粘贴增强：从 Finder Cmd+C 复制的文件，粘贴时把绝对路径送进 PTY。
 *
 * # 为什么不是纯前端
 *
 * 浏览器的 `ClipboardEvent.clipboardData.files` 是 `File` 对象，只有
 * name/type/size —— **绝对路径不存在**（实测 macOS 27）。而用户在 Finder 里
 * 选中的那个文件的完整路径在 NSPasteboard 的 fileURL/NSFilenames flavor 里，
 * 只有 Rust 侧读得到。所以流程是：
 *
 *   系统 Cmd+C → NSPasteboard(fileURL/NSFilenames) → Rust clipboard_file_paths
 *              ↘ clipboardData.files（只有文件名，不可用）
 *   终端 Cmd+V → onTextareaPaste 拿到「是否有文件」的信号 → 调 Rust 取路径
 *              → bracketed paste 包着绝对路径进 PTY
 *
 * # 预览由 omp 自己画，本模块不渲染
 *
 * omp 的编辑器认得 bracketed paste 里的图片路径，自己会生成 `img #N`
 * chip（缩略图 + `1176x1568` 尺寸，omp 18.8.3 实测）。LazeCode 若再画一层
 * 附件栏，一是重复，二是必然引出「删预览要不要回删 PTY」的同步问题。
 * **遵循 omp-tui 规格**：这里只负责把正确的字节送进 PTY，展示完全交给它。
 *
 * # 为什么这段逻辑独立成 service
 *
 * GhosttyTerminal.tsx 已经两千多行，粘贴路径里再塞路径归一、去重、shell
 * 引号转义、bracketed paste 包裹，会把文件改到不可维护。放 service 里可单测。
 *
 * # 与「普通文本粘贴」的关系
 *
 * `clipboard_file_paths` 只在剪贴板**确实有文件**时返回非空；普通文本粘贴
 * 拿到空数组，走原有逻辑原样下发。互不干扰。
 *
 * # 为什么写「裸路径」而不是 omp 的 `@path`
 *
 * omp（pi-coding-agent）的 `@file` 语法**只在 CLI 参数里解析**：
 * `dist/cli/args.js:173` 把 `@` 前缀参数收进 `fileArgs`，
 * `dist/main.js:100` → `dist/cli/file-processor.js:11` 才会读文件、
 * 生成 `<file name="…">` 引用与图片附件。
 *
 * 交互式 TUI 编辑器**不解析 `@`** —— 粘贴进去就是字面文本
 * （`dist/modes/interactive/interactive-mode.js:1624` 把粘贴原样包成
 * bracketed paste 送进编辑器）。所以用户日常手敲 `omp` 进终端的场景下，
 * 传 `@/path` 只会让模型看到一串带 @ 的字符，拿不到文件内容。
 *
 * 裸绝对路径则是唯一在两个场景都对的形式：模型认得路径、能自己 read；
 * 图片的缩略图由 omp 自己渲染，不需要我们转成任何标记。
 */

/** 一个待粘贴的文件：绝对路径 + 文件名（当前仅用于日志/调试）。 */
export interface TerminalAttachFile {
  /** 绝对路径，直接写进 PTY 的文本也是它 */
  absPath: string;
  /** 文件名（basename） */
  name: string;
}

/** basename：末段 `/` 后的内容。根路径 `/` 回落为 `/`（否则名字为空）。 */
function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, '');
  // 『全斜杠』被 trim 成空串（"/" 或 "///"）：文件系统根目录，名就是它自己
  if (trimmed === '') return '/';
  const i = trimmed.lastIndexOf('/');
  return i === -1 ? trimmed : trimmed.slice(i + 1);
}

/**
 * 把 Rust 返回的绝对路径数组规整为去重、保序的附件列表。
 *
 * `file://` URL 归一成 POSIX 路径：Rust 侧（clipboard.rs 走 NSURL.path()）
 * 返回的就是 POSIX 路径，正常不会到这里；但剪贴板源若直接给 file URL，
 * 早先的 `startsWith('/')` 过滤会**把它静默丢掉** —— 用户按一次 Cmd+V
 * 什么都没发生，这是最难查的一类 bug。宁可归一后照常处理。
 *
 * 去重是必要的：多个 flavor 可能描述同一个文件，Rust 侧已尽力去重，但路径
 * 字符串层面仍可能有 `/a/b` 与 `/a//b` 反复（不同来源写法不同）。这里再按
 * 「归一化后的字符串」收敛一次，**保留第一次出现的原始形态** —— 写进 PTY 的
 * 应该是用户看到的那个路径，不是我们改造过的。
 */
export function toAttachFiles(paths: readonly string[]): TerminalAttachFile[] {
  const seen = new Set<string>();
  const out: TerminalAttachFile[] = [];
  for (const raw of paths) {
    const p = normalizeAttachPath(raw);
    if (p === null) continue;
    const key = p.replace(/\/{2,}/g, '/');
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ absPath: p, name: basename(p) || p });
  }
  return out;
}

/** `file://host/path` → `/path`。
 *
 * **必须 `decodeURIComponent`**：`new URL(...).pathname` 会把空格编成
 * `%20`，中文编成 `%E7%AC%94...`。那不是磁盘上真实存在的路径 —— omp 侧
 * 的存在性检查失败，整段粘贴会退化成纯文本，缩略图消失。
 *
 * 裸 `file://` 解析出 `/`：那是 URL 的空路径而非用户选中的文件，必须丢弃
 * （否则会多出一个指向根目录的条目）。
 */
function normalizeAttachPath(raw: string): string | null {
  const p = raw.trim();
  if (p === '') return null;
  if (!/^file:\/\//i.test(p)) return p.startsWith('/') ? p : null;
  let decoded: string;
  try {
    decoded = decodeURIComponent(new URL(p).pathname);
  } catch {
    // 畸形 URL：剥掉 scheme 与 host 后再解码
    const stripped = p.replace(/^file:\/\//i, '').replace(/^[^/]+/, '');
    try {
      decoded = decodeURIComponent(stripped);
    } catch {
      return null;
    }
  }
  // '/' 要么来自裸 file://，要么来自 file:///；没有文件名就不是可粘贴的文件
  if (!decoded.startsWith('/') || decoded === '/') return null;
  return decoded;
}

/**
 * 附件写入 PTY 的文本格式。
 *
 * 多文件时用空格分隔单行 —— 既能在 shell 里直接当一条命令的参数
 * （`cat /a /b`），也是 omp 编辑器一次粘贴的常规形态。换行会把 shell 的
 * 命令拆成两次执行（用户还没看预览就跑了），必须避免。
 *
 * 路径只要含 shell 元字符就加单引号：空格会被拆词，单引号会提前闭合，
 * 两者都让命令语义偏离用户选中的那个文件。已含单引号的路径按 `'\''`
 * 转义（闭合、转义一个字面量单引号、重开），保持 shell 语义正确。
 *
 * omp 侧加引号是安全的：它的分词器引号感知，会把成对的 `'`/`"` 剥掉
 * （`wrl` → `t7n`），所以 shell 与 omp 两个场景同时满足。
 */
export function attachFilesToPasteText(files: readonly TerminalAttachFile[]): string {
  if (files.length === 0) return '';
  return files
    .map(f => (needsShellQuote(f.absPath) ? shellQuote(f.absPath) : f.absPath))
    .join(' ');
}

/** bracketed paste 的起止序列（DECSET 2004）。对 readline/zsh/omp 而言语义
 *  一致：中间内容是一整块粘贴，勿逐键处理。 */
const BRACKET_PASTE_START = '\x1b[200~';
const BRACKET_PASTE_END = '\x1b[201~';

/**
 * 把附件文本包成一次 bracketed paste 写进 PTY。
 *
 * **这不是可选的优化，是功能成立的前提**（omp 18.8.3 二进制实测）：
 * omp 编辑器只在 `handleInput` 收到 `ESC[200~` 后才进入粘贴状态机，
 * `#E.process(e)` 返回 `handled` 才可能走到
 * `Cue(pasteContent) → onPasteImagePath → t.attach({type:"image"})`
 * 这条**把已存在的图片路径变成 [Image #N] chip（带缩略图）**的链路。
 * 裸 `pty.write()` 的字节走逐键分支，`Cue` 永不被调用 —— 路径只会变成
 * 编辑器里的一串普通字符，用户看到的缩略图不会出现。
 *
 * 附带收益（同样是 omp 编辑器实测行为）：裸写时每个字符会触发
 * `updateAutocomplete()` 路径补全浮层（键入 `/tmp/ph` 弹候选），
 * 且每个字符都入撤销栈；包起来则整段一次插入、一次撤销。
 *
 * shell 侧：bash/zsh 默认开启 bracketed paste（readline 的
 * `enable-bracketed-paste`），中间内容会原样进命令行，**但其中的换行不再
 * 触发执行**。本格式化保证单行，所以两种模式下结果一致；换行的风险由
 * `attachFilesToPasteText` 的空格连接策略从源头消掉，不依赖这个包裹。
 */
export function wrapBracketedPaste(text: string): string {
  if (text === '') return '';
  // 中间内容里的结束序列必须剔除：否则用户选中的文件名里若含这三个字符
  // （极端但可能），粘贴状态机会提前收尾，剩下半截走逐键路径。
  const safe = text.split(BRACKET_PASTE_END).join('');
  return `${BRACKET_PASTE_START}${safe}${BRACKET_PASTE_END}`;
}

/** 空格与 shell 元字符：任一项出现就必须整段加引号。 */
const SHELL_UNSAFE = /[\s'"$`\\|&;<>(){}*?[\]!#~]/;

function needsShellQuote(p: string): boolean {
  return SHELL_UNSAFE.test(p);
}

function shellQuote(p: string): string {
  return `'${p.replace(/'/g, "'\\''")}'`;
}

/**
 * 粘贴事件里「有文件」的判定。
 *
 * `clipboardData.files` 在有文件时非空（哪怕拿不到路径），是**唯一可靠的
 * 同步信号** —— 不能反过来调 Rust 探测，那会让每次普通文本粘贴都付一次
 * IPC 往返。files 空 ⇒ 一定不是文件粘贴（Finder 复制一定带 files）。
 */
export function pasteHasFiles(data: DataTransfer | null): boolean {
  if (!data) return false;
  return data.files.length > 0;
}
