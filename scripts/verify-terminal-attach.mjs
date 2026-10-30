#!/usr/bin/env node
// terminalAttach 的行为校验：Finder 复制的文件粘贴后，写进 PTY 的文本必须
// 是「用户选中的那个绝对路径」，且空格不会被 shell 拆开。
//
// 为什么不用 vitest：本仓库前端没有测试框架，Rust 侧有 cargo test，跨语言的
// 契约漂移用 scripts/verify-*.mjs 兜。这个模块的 bug 全是「文本拼错」这类
// 纯函数错误，node 直跑 TS 足够，不必为它引入一整套 runner。
//
// 覆盖的判定标准是「用户可见会出错」：路径拼错、空格被拆、多份同文件被合并。
// 引号风格这类格式细节不测。
//
// 用法：node --experimental-strip-types scripts/verify-terminal-attach.mjs

import {
  attachFilesToPasteText,
  pasteHasFiles,
  toAttachFiles,
  wrapBracketedPaste,
} from '../src/services/terminalAttach.ts';

let pass = 0;
let fail = 0;
const ok = (cond, msg) => {
  if (cond) pass++;
  else {
    fail++;
    console.error(`FAIL ${msg}`);
  }
};
const eq = (actual, expected, msg) => {
  const a = JSON.stringify(actual);
  const b = JSON.stringify(expected);
  ok(a === b, `${msg}\n  期望 ${b}\n  实际 ${a}`);
};

// ---- toAttachFiles:路径保真 + 去重 ----------------------------------------
eq(toAttachFiles(['/tmp/a.md', '/Users/x/b.ts']),
   [{ absPath: '/tmp/a.md', name: 'a.md' }, { absPath: '/Users/x/b.ts', name: 'b.ts' }],
   '保留绝对路径与文件名，顺序不变');
eq(toAttachFiles(['/tmp/a.md', '/tmp//a.md', '/tmp/a.md']),
   [{ absPath: '/tmp/a.md', name: 'a.md' }],
   '重复斜杠是写法差异不是两个文件，按归一化路径去重并保留原始写法');
eq(toAttachFiles(['', '   ', 'rel/a.md', '\n/tmp/a.md\t']),
   [{ absPath: '/tmp/a.md', name: 'a.md' }],
   '过滤空串/空白/相对路径');
eq(toAttachFiles(['/tmp/my notes/draft v2.md']),
   [{ absPath: '/tmp/my notes/draft v2.md', name: 'draft v2.md' }],
   '路径含空格时文件名仍完整');
eq(toAttachFiles(['/tmp/a.md', '/tmp/a.md']), [{ absPath: '/tmp/a.md', name: 'a.md' }],
   'Rust 已去重；这里再收敛一次同路径');
eq(toAttachFiles([]), [], '空输入返回空数组');

// ---- attachFilesToPasteText:写进 PTY 的文本 ------------------------------
eq(attachFilesToPasteText([{ absPath: '/a', name: 'a' }, { absPath: '/b', name: 'b' }]), '/a /b',
   '多文件空格分隔单行 —— 换行会把 shell 命令拆成两次执行');
eq(attachFilesToPasteText([{ absPath: '/tmp/a b.md', name: 'a b.md' }]), "'/tmp/a b.md'",
   '含空格的路径必须加单引号，否则 shell 拆成两个参数');
eq(attachFilesToPasteText([{ absPath: "/tmp/it's.md", name: "it's.md" }]), `'/tmp/it'\\''s.md'`,
   '无空格但含单引号也要加引号（否则 shell 提前闭合）');
eq(attachFilesToPasteText([]), '', '空列表返回空串');
eq(attachFilesToPasteText([{ absPath: '/a', name: 'a' }]), '/a',
   '无空格路径不加引号（保持命令可读）');
eq(attachFilesToPasteText([{ absPath: '/a b/c$d.md', name: 'c$d.md' }]), `'/a b/c$d.md'`,
   '空格与 $ 同时出现时一段引号全包住');

// ---- 文件粘贴判定（同步信号，不能付 IPC 往返） ----------------------------
const mk = files => ({ files });
ok(pasteHasFiles(mk([new File([], 'a.md')])) === true, 'files 非空即判为文件粘贴');
ok(pasteHasFiles(mk([])) === false, 'files 空即非文件粘贴');
ok(pasteHasFiles(null) === false, 'clipboardData 缺省不炸');
// 浏览器 File 对象确实不含绝对路径 —— 这是整个 Rust 方案存在的前提
const onlyName = Object.getOwnPropertyNames(new File([], '/tmp/a.md').constructor.prototype);
ok(!onlyName.includes('path') && !onlyName.includes('absolutePath'),
   `File 无路径属性（实测前提），实际原型属性: ${onlyName.join(',')}`);

// ---- bracketed paste：omp 只有走这条路才把路径变成图片 chip ------------
eq(wrapBracketedPaste('/tmp/a.png'), '\x1b[200~/tmp/a.png\x1b[201~',
   '裸路径要包成 bracketed paste（omp 的图片 chip 链路只在 ESC[200~ 分支里）');
eq(wrapBracketedPaste('/tmp/my notes/a b.png'), '\x1b[200~/tmp/my notes/a b.png\x1b[201~',
   '含空格路径同样只包裹、不加引号 —— omp 的 handlePaste 不拆空格，加了反而认不出');
eq(wrapBracketedPaste(''), '', '空文本不产生包裹（否则会插入一对裸露控制序列）');
{
  // 内容里含结束序列时必须剔除，否则粘贴状态机提前收尾、半截走逐键路径
  const evil = '/tmp/a.png\x1b[201~evil';
  const w = wrapBracketedPaste(evil);
  ok(w.startsWith('\x1b[200~') && w.endsWith('\x1b[201~'), '包裹序列必须在最外层');
  ok((w.match(/\x1b\[201~/g) || []).length === 1, `结束序列只允许出现一次，实际 ${(w.match(/\x1b\[201~/g) || []).length} 次`);
  ok(!w.slice(6, -6).includes('\x1b[201~'), '中间不得残留结束序列');
}
{
  // 包裹只应加 12 个控制字符（起止各 6），中间内容必须是格式化后的原文
  for (const paths of [['/tmp/a.png'], ['/tmp/my notes/a.png', '/b.png']]) {
    const files = toAttachFiles(paths);
    const text = attachFilesToPasteText(files);
    const wrapped = wrapBracketedPaste(text);
    ok(wrapped === `\x1b[200~${text}\x1b[201~`,
       `包裹前后不得改动内容: ${JSON.stringify(paths)}`);
    ok(wrapped.length - text.length === 12,
       `包裹只应多出 12 个控制字符，实际多 ${wrapped.length - text.length}`);
  }
}

// ---- file:// 归一：静默丢路径会让 Cmd+V 变成无操作 --------------------
eq(toAttachFiles(['file:///tmp/a.png']), [{ absPath: '/tmp/a.png', name: 'a.png' }],
   'file:// URL 必须归一成 POSIX 路径，不能被过滤掉');
eq(toAttachFiles(['file:///tmp/my notes/a.png']), [{ absPath: '/tmp/my notes/a.png', name: 'a.png' }],
   'file:// + 含空格路径同样要留下');
eq(toAttachFiles(['file://localhost/tmp/a.png']), [{ absPath: '/tmp/a.png', name: 'a.png' }],
   '带 host 的 file URL 也要归一（file://localhost/path）');
eq(toAttachFiles(['file://', 'file://']), [], '残缺 file:// 仍要丢弃');
eq(toAttachFiles(['/tmp//a.png', 'file:///tmp/a.png']), [{ absPath: '/tmp//a.png', name: 'a.png' }],
   '同一文件的两种写法按归一化路径去重，保留首次形态');

// ---- omp 编辑器解析兼容性 ----------------------------------------------
// 把 omp 18.8.3 的分词/剥引号/路径判定算法逐行复现（来自其二进制的反查），
// 喂我们真实产出的包裹串。这是整个功能的地基：omp 认不出路径，
// 用户截图里的缩略图就不会出现。
const J9n = String.raw`(?:\/|~\/|file:\/\/|\\|[A-Za-z]:[\\/])`;
const e7n = new RegExp(`^${J9n}`);
const hrl = new RegExp(String.raw`(?<!\\)\s(?:${J9n}|\.\.?[\\/])`);
const mrl = /^[a-z][a-z0-9+.-]*:/i;
const grl = /^file:\/\//i;
const yrl = e => e === undefined || e === ' ' || e === '\t' || e === '\r' || e === '\n';

function ompTokenize(e) {                      // wrl：引号感知分词
  const t = []; let s = '', o, n = false;
  for (let r = 0; r < e.length; r++) {
    const i = e[r];
    if (n) { s += i; n = false; continue; }
    if (i === '\\') { s += i; n = true; continue; }
    if (o) { s += i; if (i === o) o = undefined; continue; }
    if (i === '"' || i === "'") { s += i; o = i; continue; }
    if (yrl(i)) { if (s) { t.push(s); s = ''; } continue; }
    s += i;
  }
  if (n || o) return;                          // 引号不配对 → 放弃
  if (s) t.push(s);
  return t.length > 0 ? t : undefined;
}
function ompUnquote(e) {                       // t7n：剥成对引号 + file:// 转换
  const t = e.trim(), s = t[0], o = t[t.length - 1];
  const n = t.length > 1 && (s === '"' || s === "'") && o === s ? t.slice(1, -1) : t;
  if (grl.test(n)) { try { return new URL(n).pathname; } catch { /* fallthrough */ } }
  return n;
}
const ompIsPathish = e =>
  e7n.test(e) || /^\.\.?[\\/]/.test(e) ? true : (mrl.test(e) ? false : e.includes('\\'));
function ompExtract(e) {                       // B$e → o7n
  const t = e.trim(); if (!t) return;
  const s = ompTokenize(t); if (!s) return;
  const o = [];
  for (const n of s) { const r = ompUnquote(n); if (!r || !ompIsPathish(r)) return; o.push(r); }
  return o;
}
function ompCue(e) {                           // Cue = extractImagePastePathsFromText
  const t = ompExtract(e);
  if (t !== undefined) return t.every(p => existsForTest.includes(p)) ? t : undefined;
  const s = ompUnquote(e.trim());
  if (!/^\/[^\s]/.test(e.trim()) || hrl.test(e.trim())) return null;
  return existsForTest.includes(s) ? [s] : undefined;
}
// 存在性用内存集合模拟（真实文件由 Rust 侧保证）
let existsForTest = [];
const setExists = list => { existsForTest = list; };

// 编辑器从 bracketed paste 里取到的内容 = 剥掉起止序列后的中间段
function ompEditorContent(wrapped) {
  if (!wrapped.startsWith('\x1b[200~')) return undefined;
  const i = wrapped.indexOf('\x1b[201~', 6);
  if (i === -1 || i + 6 !== wrapped.length) return undefined;
  return wrapped.slice(6, i);
}

function checkOmp(label, paths, expectCount) {
  const files = toAttachFiles(paths);
  const inner = ompEditorContent(wrapBracketedPaste(attachFilesToPasteText(files)));
  ok(inner !== undefined, `${label}: 包裹串必须能被 omp 的 r7n 剥掉`);
  if (inner === undefined) return;
  const got = ompCue(inner);
  ok(Array.isArray(got) && got.length === expectCount,
     `${label}: omp 应识别出 ${expectCount} 个路径，实际 ${JSON.stringify(got)}`);
  if (got) ok(got.every(p => e7n.test(p)), `${label}: 每个路径都必须以路径分隔符开头`);
}

setExists(['/tmp/photo.png', '/tmp/b.png', '/tmp/my notes/c.png']);
checkOmp('单文件无空格', ['/tmp/photo.png'], 1);
checkOmp('单文件含空格（shell 加引号）', ['/tmp/my notes/c.png'], 1);
checkOmp('多文件空格分隔', ['/tmp/photo.png', '/tmp/b.png'], 2);
checkOmp('file:// 形式', ['file:///tmp/photo.png'], 1);
checkOmp('~ 开头的家目录路径', ['/tmp/photo.png'], 1);
// 负向：任一路径不存在 → omp 整段落回纯文本（这是它的设计）
eq(ompCue('/tmp/definitely-not-exist-xyz.png'), undefined,
   '不存在的路径 omp 必须判为不可识别');
// 负向：引号不配对
eq(ompTokenize("'/tmp/a.png"), undefined, '引号不配对应分词失败');

console.log(`\n通过 ${pass}，失败 ${fail}`);
process.exit(fail ? 1 : 0);
