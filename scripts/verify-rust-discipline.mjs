#!/usr/bin/env node
// Rust 纪律门禁（对齐 Zed AGENTS.md / clippy.toml 的思路）。
//
// 为什么不用 clippy 全包：这些规则都是**位置敏感**的 —— 测试脚手架里
// `unwrap()` / `.status()` / `Command::new` 全是合法用法，clippy 的
// `disallowed-methods` 区分不了测试与生产上下文。全局能禁的只有一条
// （`serde_json::from_reader`，已进 `src-tauri/clippy.toml`），其余在这里
// 按「是否在 `#[cfg(test)]` 模块内」切分后做白名单门禁。
//
// 切分是真正的 Rust 词法扫描（字符串 / 字符 / 行注释 / 可嵌套块注释都跳过，
// 只数真实花括号），不是"第一个 #[cfg(test)] 之后全算测试" —— commands.rs
// / pty.rs 里测试模块与生产代码是交错排列的，粗切分会静默漏检生产代码。
//
// 规则（`#[cfg(test)]` 模块外 = 生产代码；测试文件 `diff_stream_test.rs` 整份跳过）：
//   R1 无 panic 面：`unwrap()` / `expect(` / `panic!` / `todo!` /
//      `unimplemented!` / `unreachable!` / `dbg!` 零容忍。
//      （`unwrap_or*` / `unwrap_err` 不算——它们不 panic。）
//   R2 无无界阻塞：`.output()` 零容忍。一切子进程输出必须走
//      `run_with_timeout_spawn`（管道后台排空 + 超时杀进程组）。
//   R3 `.status()` 只允许 `kill_process_group`（best-effort SIGKILL，
//      失败本就按 false 处理）。
//   R4 `let _ =` 只允许已注释的类别：emit（事件发不出去只能罢休）、
//      kill/wait/join（收尾）、remove_*/create_dir_all（best-effort 清理）、
//      watch/send/read_to_end（后台线程内）、以及两个显式未使用值。
//   R5 `from_reader` 全仓库零容忍（与 clippy.toml 双保险）。
//   R6 `Command::new` 只允许出现在 APPROVED_SPAWNERS 里的函数：
//      新起子进程的位置必须是有意为之的，顺手起一个就得改这张表并写理由。
//   R7 卫生：`#[tauri::command]` 不允许落在测试区内——命令若被划进测试区，
//      说明上面的切分假设已失效，本脚本会静默漏检，先修脚本。
//
// 行级说明：
// - 整行注释（`//` 开头，含 `///` / `//!`）不参与 R1–R6（文档里提一句
//   `.output()` 不该触发门禁）；代码行尾的注释不剥离，写注释时避开这些字面量。
// - `unwrap_or*` / `unwrap_err` 含 `unwrap` 子串但不匹配 `\.unwrap\(\)`，安全。
//
// 用法：node scripts/verify-rust-discipline.mjs（已接入 npm run check）

import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const srcDir = join(root, 'src-tauri/src');

// 整份测试代码、无生产逻辑，跳过（lib.rs 里 `#[cfg(test)] mod diff_stream_test` / `mod git_menu_test`）。
const TEST_ONLY_FILES = new Set(['diff_stream_test.rs', 'git_menu_test.rs']);

// R6：允许构造子进程的函数。新增位置时把名字加进来并在注释写理由。
const APPROVED_SPAWNERS = new Set([
  'git_cmd', // git 主入口：调用方一律走 run_with_timeout_spawn
  'network_git_cmd', // 经 git_cmd 构造，只加 env（实际无直接 new，留作文档）
  'fs_reveal', // open/explorer/xdg-open 经 spawn_and_reap（reaper 线程收尸）
  'open_url_in_browser', // 同上
  'kill_process_group', // best-effort SIGKILL，失败按 false 处理
  'find_ghostty_binary', // where/command -v 探测，5s 有界
  'run_ghostty_config', // ghostty +show-config，10s 有界
  'detect_ghostty', // ghostty --version，5s 有界
  'ghostty_list_themes', // ghostty +list-themes，10s 有界
  'launch_ghostty', // open -na / 直接起 GUI（reaper 线程收尸），15s 有界
  'probe_session_busy', // ps 探测终端占用，3s 有界
  'process_rss_kb', // ps -o rss= 采样 LSP 内存归因，3s 有界（run_with_timeout_spawn）
  'spawn_lsp_server', // 语言服务器长命进程：只 spawn 不 wait，句柄入注册表，stop/退出时 kill+reaper
  'ensure_macos_gui_environment', // macOS GUI 启动时从 login shell 获取真实 PATH，1.5s 有界
]);

// R4：`let _ =` 允许的类别（行内必须出现其中之一）。
const LET_UNDERSCORE_ALLOW = [
  /\.emit\(/, // 事件发不出去只能罢休（各处注释）
  /\.kill\(\)/, // 收尾杀进程
  /\.wait\(\)/, // reaper 线程收尸
  /\.join\(\)/, // reader 线程收尾（超时已放弃，只求同步）
  /remove_(file|dir)/, // best-effort 清理（Drop / 粘贴失败回滚）
  /create_dir_all/, // 配置目录不存在即建，失败由后续写盘报错
  /\.watch\(/, // 单个目录监听失败不连累其它文件
  /\.send\(/, // 后台线程回传（接收端已放弃即丢弃）
  /read_to_end/, // 管道排水中途失败即按已读处理
  /setPropertyList/, // objc 剪贴板写入（失败无补救动作）
  /let _ = target;/, // 非 Unix 平台显式未用参数
  /\(master, shell_pid\)/, // 探测元组只取占用结果
  /Command::new\("kill"\)/, // busybox 回退的 best-effort SIGKILL
  /writeln!\(/, // panic hook 里的 best-effort 落盘：hook 内无错误通道，只能罢休
];

// R1 特批：精确到「文件 + 函数 + 代码片段」，任一对不上即失效，
// 逼作者在改动该行时重新评估理由，而不是躺在白名单里腐烂。
// 当前唯一特批：进程启动失败（见该行注释）。
const PANIC_ALLOW = [
  {
    file: 'lib.rs',
    func: 'run',
    snippet: '.expect("error while building tauri application")',
    reason: 'startup-fatal',
  },
];

const fnRe = /^\s*(?:pub(?:\(crate\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_0-9]+)/;

/**
 * 标出每一行是否在 `#[cfg(test)]` 内联模块里。
 *
 * 轻量 Rust 词法扫描：跳过行注释 / 可嵌套块注释 / 普通字符串 /
 * raw 字符串 / 字符字面量后计数真实花括号。`#[cfg(test)]` 之后遇到的
 * 下一个 `{` 即打开测试区（本仓库全部形如 `#[cfg(test)]\\nmod x {`），
 * 遇到 `;` 则复位（防 `mod x;` 文件模块形态误开）。
 */
function testMask(src) {
  const lines = src.split('\n');
  const mask = new Array(lines.length).fill(false);
  let depth = 0;
  let pendingTest = false;
  const stack = [];
  const inTest = () => stack.length > 0;
  let i = 0;
  let line = 0;
  const N = src.length;
  const mark = () => {
    mask[line] = mask[line] || inTest();
  };
  while (i < N) {
    const c = src[i];
    if (c === '\n') {
      line += 1;
      i += 1;
      continue;
    }
    // 行注释：整段跳过（注释里的 #[cfg(test)] 不得触发状态）
    if (c === '/' && src[i + 1] === '/') {
      mark();
      while (i < N && src[i] !== '\n') i += 1;
      continue;
    }
    // 块注释（Rust 允许嵌套）
    if (c === '/' && src[i + 1] === '*') {
      let nest = 1;
      i += 2;
      mark();
      while (i < N && nest > 0) {
        if (src[i] === '\n') {
          line += 1;
          mark();
        }
        if (src[i] === '/' && src[i + 1] === '*') {
          nest += 1;
          i += 2;
        } else if (src[i] === '*' && src[i + 1] === '/') {
          nest -= 1;
          i += 2;
        } else {
          i += 1;
        }
      }
      continue;
    }
    // raw 字符串 r"..." / r#"..."# / r##"..."##
    if (c === 'r' && (src[i + 1] === '"' || src[i + 1] === '#')) {
      const m = src.slice(i, i + 8).match(/^r(#+)"/);
      const hashes = m ? m[1].length : 0;
      i += hashes === 0 ? 2 : 1 + hashes + 1;
      mark();
      const close = `"${'#'.repeat(hashes)}`;
      while (i < N && !src.startsWith(close, i)) {
        if (src[i] === '\n') {
          line += 1;
          mark();
        }
        i += 1;
      }
      i += close.length;
      continue;
    }
    // 普通字符串（含转义）
    if (c === '"') {
      i += 1;
      mark();
      while (i < N && src[i] !== '"') {
        if (src[i] === '\\') i += 1;
        if (src[i] === '\n') {
          line += 1;
          mark();
        }
        i += 1;
      }
      i += 1;
      continue;
    }
    // 字符字面量 'x' / '\n'；生命周期 'a 无闭合引号，直接忽略
    if (c === "'") {
      const m = src.slice(i, i + 6).match(/^'(\\.|[^'\\])'/);
      if (m) i += m[0].length;
      else i += 1;
      mark();
      continue;
    }
    // 属性：#[cfg(test)] / #[cfg(all(test, ...))] 等变体
    if (c === '#' && src[i + 1] === '[') {
      const m = src.slice(i, i + 24).match(/^#\[cfg\((test|all\(test)/);
      if (m) {
        pendingTest = true;
        i += m[0].length;
      } else {
        i += 2;
      }
      mark();
      continue;
    }
    if (c === ';' && pendingTest) {
      pendingTest = false; // `mod x;` 文件形态：不等花括号
      mark();
      i += 1;
      continue;
    }
    if (c === '{') {
      depth += 1;
      if (pendingTest) {
        stack.push(depth);
        pendingTest = false;
      }
      mark();
      i += 1;
      continue;
    }
    if (c === '}') {
      mark();
      depth -= 1;
      while (stack.length > 0 && depth < stack[stack.length - 1]) stack.pop();
      i += 1;
      continue;
    }
    mark();
    i += 1;
  }
  return mask;
}

let failures = 0;
const fail = (msg) => {
  console.error(`FAIL ${msg}`);
  failures += 1;
};

for (const f of readdirSync(srcDir).filter((x) => x.endsWith('.rs'))) {
  if (TEST_ONLY_FILES.has(f)) continue;
  const src = readFileSync(join(srcDir, f), 'utf8');
  const lines = src.split('\n');
  const mask = testMask(src);

  // R7：命令定义落在测试区 = 切分假设失效
  lines.forEach((line, idx) => {
    if (/^\s*#\[tauri::command\]/.test(line) && mask[idx]) {
      fail(`${f}:${idx + 1} 的 #[tauri::command] 被划进测试区，切分假设失效，先修 testMask`);
    }
  });

  let curFn = '?';
  lines.forEach((line, idx) => {
    if (mask[idx]) return; // 测试代码不受管
    if (/^\s*\/\//.test(line)) return; // 整行注释不参与 R1–R6
    const no = idx + 1;
    const m = line.match(fnRe);
    if (m) curFn = m[1];
    const tag = `${f}:${no} [${curFn}]`;

    // R1：panic 面（unwrap_or* / unwrap_err 不算）
    if (/\.unwrap\(\)/.test(line)) fail(`${tag} 生产代码禁用 unwrap()，用 ? / map_err + error_proto::coded：${line.trim()}`);
    if (/\.expect\(/.test(line)) {
      const allowed = PANIC_ALLOW.some(
        (a) => a.file === f && a.func === curFn && line.includes(a.snippet),
      );
      if (!allowed) fail(`${tag} 生产代码禁用 expect()，用 ? / map_err + error_proto::coded：${line.trim()}`);
    }
    const pm = line.match(/\b(panic!|todo!|unimplemented!|unreachable!|dbg!)\s*\(/);
    if (pm) fail(`${tag} 生产代码禁用 ${pm[1]}：${line.trim()}`);

    // R2：无界阻塞
    if (/\.output\(\)/.test(line)) {
      fail(`${tag} 禁止无超时的 .output()，走 run_with_timeout_spawn：${line.trim()}`);
    }

    // R3：.status() 特批（backtrace.status() 是 Backtrace 的方法，不是 Command 调用）
    if (/(?<!backtrace)\.status\(\)/.test(line) && curFn !== 'kill_process_group') {
      fail(`${tag} .status() 只允许在 kill_process_group 内：${line.trim()}`);
    }

    // R4：let _ = 白名单
    if (line.includes('let _ =') && !LET_UNDERSCORE_ALLOW.some((re) => re.test(line))) {
      fail(`${tag} let _ = 不在允许类别（emit/kill/wait/join/清理/watch/send/排空），要么处理错误要么扩展白名单并写理由：${line.trim()}`);
    }

    // R6：新起子进程的位置管制
    if (/Command::new/.test(line) && !APPROVED_SPAWNERS.has(curFn)) {
      fail(`${tag} Command::new 位置未审批，确认有界超时后加入 APPROVED_SPAWNERS 并写理由：${line.trim()}`);
    }
  });

  // R5 放整个文件（含测试与注释）：from_reader 全仓库不许出现
  lines.forEach((line, idx) => {
    if (line.includes('from_reader')) fail(`${f}:${idx + 1} 禁止 serde_json::from_reader（见 clippy.toml）：${line.trim()}`);
  });
}

if (failures > 0) process.exit(1);
console.log('ok Rust 纪律门禁通过（R1 无 panic 面 / R2 无无界阻塞 / R3 status 特批 / R4 let _= 白名单 / R5 无 from_reader / R6 spawner 审批 / R7 边界卫生）');
