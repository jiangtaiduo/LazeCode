#!/usr/bin/env node
// 终端端到端管线基准：真实 PTY → 原生 libghostty-vt 解析 → 差分帧
// → 前端 CellDiffCache 合并。外加同一份负载下的进程峰值 RSS 对比。
//
// 链路与 `bench:terminal` 的区别：那边用合成字节流只比解析器；这里从
// `/bin/sh -c 'seq …'` 经真实 PTY 读出 >10MB，Rust 侧每 8KB 解析一次并
// 取一帧增量，Node 侧解码并合并进与生产同构的 CellDiffCache。
//
// **不包含**：Tauri IPC、React 调度、Canvas 绘制、WebView 与 UI 启动。
// 因此数字说明的是「引擎 + 线格式 + 合并算法」的成本，不是用户可见帧耗时。
//
// 用法：npm run bench:pipeline

import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import os from 'node:os';

import {
  CellDiffCache,
  decodeCellDiff,
  decodeCellFrame,
  WIRE_DIFF_MAGIC,
  WIRE_MAGIC
} from '../src/ghostty-vt/cellFrame.ts';

const require = createRequire(import.meta.url);
const xtermVersion = require('@xterm/headless/package.json').version;

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const TAURI_DIR = join(ROOT, 'src-tauri');
const BENCH_DIR = join(TAURI_DIR, 'target', 'bench');
const FRAME_PATH = join(BENCH_DIR, 'pipeline-frames.bin');
const PAYLOAD_PATH = join(BENCH_DIR, 'pipeline-payload.bin');
const RESULT_PATH = join(BENCH_DIR, 'pipeline.json');

const COLS = 120;
const ROWS = 40;
const CHUNK = 8 * 1024;
const SCROLLBACK = 20_000;

const MB = 1024 * 1024;
const TIME_BIN = process.platform === 'darwin' && existsSync('/usr/bin/time') ? '/usr/bin/time' : null;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function fnv1a32(buffer) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < buffer.length; i++) {
    hash ^= buffer[i];
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? ROOT,
    stdio: options.stdio ?? 'inherit',
    encoding: 'utf8',
    env: { ...process.env, ...(options.env ?? {}) },
    maxBuffer: options.maxBuffer ?? 64 * MB
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const detail = result.stderr?.trim() ? `\n${result.stderr.trim()}` : '';
    throw new Error(`${command} ${args.join(' ')} 退出码 ${result.status}${detail}`);
  }
  return result;
}

function buildNativeExample() {
  console.log('构建原生基准（release）…');
  run('cargo', ['build', '--release', '--example', 'bench_pipeline'], {
    cwd: TAURI_DIR,
    stdio: ['ignore', 'inherit', 'inherit']
  });
  const bin = join(TAURI_DIR, 'target', 'release', 'examples', 'bench_pipeline');
  assert(existsSync(bin), `找不到基准二进制：${bin}`);
  return bin;
}

function parseMaxRss(stderr) {
  const match = stderr.match(/^\s*(\d+)\s+maximum resident set size\s*$/m);
  return match ? Number(match[1]) : null;
}

/** 原生侧：跑真实 PTY 管线，落盘帧文件与负载文件。 */
function runNative(bin) {
  mkdirSync(BENCH_DIR, { recursive: true });
  const args = [FRAME_PATH, PAYLOAD_PATH];
  // /usr/bin/time -l 观测的是子进程峰值 RSS；没有它（非 macOS）就只跑基准。
  const result = TIME_BIN
    ? spawnSync(TIME_BIN, ['-l', bin, ...args], {
        cwd: TAURI_DIR,
        encoding: 'utf8',
        maxBuffer: 64 * MB
      })
    : spawnSync(bin, args, { cwd: TAURI_DIR, encoding: 'utf8', maxBuffer: 64 * MB });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`原生基准失败（退出码 ${result.status}）：\n${result.stderr ?? ''}`);
  }

  const line = result.stdout.trim().split('\n').filter(Boolean).at(-1);
  assert(line, '原生基准没有输出 JSON');
  return {
    metrics: JSON.parse(line),
    rssBytes: parseMaxRss(result.stderr ?? '')
  };
}

/**
 * xterm.js 的 RSS 单独起一个 Node 进程测：
 * 若在常驻进程里跑，脚本自身持有的 68MB 帧缓冲会被算进去，数字失真。
 */
const XTERM_RUNNER = `
const fs = require('node:fs');
const { Terminal } = require('@xterm/headless');
const payload = fs.readFileSync(process.env.BENCH_PAYLOAD);
const term = new Terminal({ cols: ${COLS}, rows: ${ROWS}, scrollback: ${SCROLLBACK}, convertEol: false });
const timer = setTimeout(() => {
  console.error('xterm 解析超时');
  process.exit(2);
}, 120000);
term.write(payload, () => {
  clearTimeout(timer);
  term.dispose();
  process.exit(0);
});
`;

function runXtermRss() {
  if (!TIME_BIN) return { rssBytes: null, elapsedMs: null };
  const started = process.hrtime.bigint();
  const result = spawnSync(TIME_BIN, ['-l', process.execPath, '-e', XTERM_RUNNER], {
    cwd: ROOT,
    encoding: 'utf8',
    env: { ...process.env, BENCH_PAYLOAD: PAYLOAD_PATH },
    maxBuffer: 16 * MB
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`xterm.js RSS 运行失败（退出码 ${result.status}）：\n${result.stderr ?? ''}`);
  }
  return {
    rssBytes: parseMaxRss(result.stderr ?? ''),
    elapsedMs: Number(process.hrtime.bigint() - started) / 1e6
  };
}

function nowMs() {
  return Number(process.hrtime.bigint()) / 1e6;
}

function stats(values) {
  if (values.length === 0) return { p50: 0, p95: 0, max: 0, total: 0 };
  const sorted = [...values].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)];
  return {
    p50: at(0.5),
    p95: at(0.95),
    max: sorted[sorted.length - 1],
    total: sorted.reduce((sum, value) => sum + value, 0)
  };
}

/**
 * 逐帧解码 + 合并。帧文件每帧为 `u32 len`（小端）+ 帧字节；
 * 首帧是全量帧（VTN1），其余为增量帧（VTN3），顺序与生产前端一致。
 */
function replayFrames(frameBytes) {
  const file = readFileSync(FRAME_PATH);
  assert(file.length === frameBytes, `帧文件长度不符：${file.length} != ${frameBytes}`);

  const cache = new CellDiffCache();
  const decodeTimes = [];
  const applyTimes = [];
  let offset = 0;
  let fullFrames = 0;
  let diffFrames = 0;
  let decodedCells = 0;
  let fullCells = 0;
  let fullDecodeMs = 0;

  while (offset + 4 <= file.length) {
    const length = file.readUInt32LE(offset);
    offset += 4;
    const end = offset + length;
    assert(end <= file.length, `帧长度越界：${length}B @ ${offset}`);
    assert(length >= 4, `帧过短：${length}B`);

    // 复制出独立 ArrayBuffer：解码器按整个 buffer 校验长度，
    // 直接给大文件视图会把后续帧也算进本帧。
    const buffer = file.buffer.slice(file.byteOffset + offset, file.byteOffset + end);
    const magic = new DataView(buffer).getUint32(0, true);

    if (magic === WIRE_MAGIC) {
      const started = nowMs();
      const frame = decodeCellFrame(buffer);
      fullDecodeMs += nowMs() - started;
      cache.reset(frame);
      fullCells += frame.cols * frame.rows;
      fullFrames++;
    } else if (magic === WIRE_DIFF_MAGIC) {
      const decodeStart = nowMs();
      const diff = decodeCellDiff(buffer);
      decodeTimes.push(nowMs() - decodeStart);

      const applyStart = nowMs();
      cache.applyDiff(diff);
      applyTimes.push(nowMs() - applyStart);

      decodedCells += diff.rows.length * diff.cols;
      diffFrames++;
    } else {
      throw new Error(`未知帧 magic：0x${magic.toString(16)} @ ${offset}`);
    }

    offset = end;
  }

  assert(offset === file.length, `帧文件尾部有 ${file.length - offset}B 残留`);
  return {
    cache,
    fullFrames,
    diffFrames,
    decodedCells,
    fullCells,
    fullDecodeMs,
    decode: stats(decodeTimes),
    apply: stats(applyTimes)
  };
}

function formatMb(bytes) {
  return bytes === null || bytes === undefined ? 'null' : (bytes / MB).toFixed(1);
}

function formatMs(value) {
  return value < 1 ? value.toFixed(3) : value.toFixed(2);
}

function main() {
  const bin = buildNativeExample();
  const native = runNative(bin);
  const metrics = native.metrics;

  const payload = readFileSync(PAYLOAD_PATH);
  assert(payload.length === metrics.payload.bytes, '原生与 Node 的负载长度不一致');
  assert(fnv1a32(payload) === metrics.payload.fnv1a32, '原生与 Node 的负载哈希不一致');

  const xterm = runXtermRss();
  const frames = replayFrames(metrics.frames.file_bytes);
  assert(
    frames.diffFrames === metrics.frames.diff_frames,
    `增量帧数不一致：${frames.diffFrames} != ${metrics.frames.diff_frames}`
  );
  assert(!frames.cache.needsFullRebuild, 'CellDiffCache 终态仍要求全量重建');
  assert(frames.cache.cols === COLS && frames.cache.rows === ROWS, 'CellDiffCache 尺寸不符');

  const applyAll = frames.apply.total;
  const summary = {
    generatedAt: new Date().toISOString(),
    environment: {
      platform: `${os.platform()} ${os.release()} ${os.arch()}`,
      cpu: os.cpus()[0]?.model ?? 'unknown',
      node: process.version,
      xterm: xtermVersion,
      rustc: spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout?.trim() ?? null
    },
    config: {
      cols: COLS,
      rows: ROWS,
      chunkBytes: CHUNK,
      scrollback: SCROLLBACK,
      command: metrics.payload.command,
      frameFile: FRAME_PATH,
      payloadFile: PAYLOAD_PATH
    },
    payload: {
      bytes: payload.length,
      mb: payload.length / MB,
      fnv1a32: metrics.payload.fnv1a32
    },
    native: metrics,
    frontend: {
      fullFrames: frames.fullFrames,
      fullFrameCells: frames.fullCells,
      fullFrameDecodeMs: frames.fullDecodeMs,
      diffFrames: frames.diffFrames,
      decodedCells: frames.decodedCells,
      decodeMs: frames.decode,
      applyMs: frames.apply,
      cellsPerSecond: frames.decodedCells / (applyAll / 1000),
      cacheNeedsFullRebuild: frames.cache.needsFullRebuild
    },
    rss: {
      nativeBytes: native.rssBytes,
      xtermBytes: xterm.rssBytes,
      xtermElapsedMs: xterm.elapsedMs,
      note:
        '进程峰值 RSS（macOS /usr/bin/time -l），含各自运行时与分配器；' +
        '原生含 libghostty-vt 的 Zig allocator，xterm 含 Node.js 运行时。' +
        '非 macOS 或缺少 /usr/bin/time 时为 null。'
    },
    scope:
      '端到端指引擎 + 线格式 + 前端合并；不含 Canvas 绘制、Tauri IPC、' +
      'React 调度与 UI 启动。'
  };

  writeFileSync(RESULT_PATH, `${JSON.stringify(summary, null, 2)}\n`);

  const nativeRss = native.rssBytes === null ? 'null' : `${formatMb(native.rssBytes)} MB`;
  const xtermRss = xterm.rssBytes === null ? 'null' : `${formatMb(xterm.rssBytes)} MB`;

  console.log('');
  console.log('终端端到端管线（真实 PTY → 原生解析 → 差分帧 → 前端合并）');
  console.log('─'.repeat(72));
  console.log(`负载                : ${(payload.length / MB).toFixed(2)} MB / ${metrics.frames.diff_frames} 块（每块 ${CHUNK / 1024} KB）`);
  console.log(`PTY 读取            : ${formatMs(metrics.pty.drain_ms)} ms（read 阻塞 ${formatMs(metrics.pty.read_ms)} ms）`);
  console.log(
    `原生解析+差分       : ${formatMs(metrics.parse_diff.total_ms)} ms 合计，` +
      `${(metrics.parse_diff.mb_per_s).toFixed(1)} MB/s`
  );
  console.log(
    `  每块 p50/p95/max  : ${formatMs(metrics.parse_diff.p50_ms)} / ${formatMs(metrics.parse_diff.p95_ms)} / ${formatMs(metrics.parse_diff.max_ms)} ms`
  );
  console.log(
    `差分帧              : ${metrics.frames.diff_frames} 帧 / ${(metrics.frames.diff_bytes / MB).toFixed(1)} MB` +
      `（另有 1 帧全量 ${metrics.frames.full_frame_bytes} B）`
  );
  console.log(
    `前端解码            : p50 ${formatMs(frames.decode.p50)} / p95 ${formatMs(frames.decode.p95)} / max ${formatMs(frames.decode.max)} ms（${frames.diffFrames} 帧）`
  );
  console.log(
    `前端合并            : p50 ${formatMs(frames.apply.p50)} / p95 ${formatMs(frames.apply.p95)} / max ${formatMs(frames.apply.max)} ms，` +
      `共 ${(frames.decodedCells / 1e6).toFixed(2)}M 格`
  );
  console.log(`缓存终态            : needsFullRebuild=${frames.cache.needsFullRebuild}`);
  console.log(`峰值 RSS            : 原生 ${nativeRss} / xterm.js ${xtermRss}`);
  console.log('');
  console.log(`结果 → ${RESULT_PATH}`);
  console.log(`帧文件 → ${FRAME_PATH}`);
  console.log('注意：不含 Canvas 绘制、Tauri IPC、React 调度与 UI 启动；不能当作端到端 UI 帧耗时。');
}

main();
