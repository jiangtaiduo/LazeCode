// 线格式验证
//
// 运行: npm run verify:wire
//
// 覆盖两件事：
//   1. 线格式解码器自身的正确性（位域拆解、边界、容错）
//   2. 跨语言往返：Rust `vt::encode_snapshot` 的产出由本脚本独立解码比对
//
// 第 2 项是硬保证 —— 前端不重新实现 VT 逻辑，它对位域的理解必须与
// Rust 侧完全一致。任何一边改了布局而另一边没跟上，这里就会红。
//
// 重新生成 fixture：
//   cargo test --manifest-path src-tauri/Cargo.toml --lib fixture::emit_wire_fixture

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  decodeCellFrame,
  decodeCellDiff,
  frameCell,
  frameCellInto,
  CellDiffCache,
  EMPTY_STYLE,
  WIRE_MAGIC,
  WIRE_HEADER_LEN,
  WIRE_STYLE_LEN,
  WIRE_FOOTER_MAGIC,
  WIRE_DIFF_FOOTER_MAGIC,
  RowFlag,
  CURSOR_STYLE,
  WIDE_NARROW,
  WIDE_WIDE,
  WIDE_SPACER_TAIL,
  type CellFrame,
  type FrameCell
} from './cellFrame.ts';

const ROOT = process.cwd();
const FIXTURE_DIR = resolve(ROOT, 'src/ghostty-vt/__fixtures__');

interface Check {
  name: string;
  run: () => void;
}

const checks: Check[] = [];
const pass = (name: string, run: Check['run']) => checks.push({ name, run });

const assert = (cond: unknown, message: string): void => {
  if (!cond) throw new Error(message);
};
const eq = (actual: unknown, expected: unknown, message: string): void => {
  if (actual !== expected) {
    throw new Error(`${message}: 期望 ${String(expected)}，实际 ${String(actual)}`);
  }
};

// ---------------------------------------------------------------------------
// 构造器：按 Rust 侧布局手工造帧，用于隔离验证解码器
// ---------------------------------------------------------------------------

interface StyleSpec {
  fg: number;
  bg: number;
  flags: number;
}
interface CellSpec {
  codepoint: number;
  wide: number;
  styleIdx: number;
  hasText: boolean;
  /** 字素簇下标 + 1（0 = 无簇）；配合 `withFooter` 使用 */
  cluster?: number;
}

function buildFrame(opts: {
  cols: number;
  rows: number;
  cursorX?: number;
  cursorY?: number;
  flags?: number;
  cursorStyle?: number;
  scrollbar?: [number, number, number];
  styles: StyleSpec[];
  cells: CellSpec[];
}): ArrayBuffer {
  const { cols, rows, styles, cells } = opts;
  const buf = new ArrayBuffer(
    WIRE_HEADER_LEN + styles.length * WIRE_STYLE_LEN + cells.length * 8
  );
  const view = new DataView(buf);

  view.setUint32(0, WIRE_MAGIC, true);
  view.setUint16(4, cols, true);
  view.setUint16(6, rows, true);
  view.setUint16(8, opts.cursorX ?? 0, true);
  view.setUint16(10, opts.cursorY ?? 0, true);
  view.setUint16(12, styles.length, true);
  view.setUint16(14, opts.flags ?? 1, true); // 默认光标可见
  view.setInt32(16, -1, true);
  view.setInt32(20, -1, true);
  view.setInt32(24, -1, true);
  view.setUint32(28, opts.cursorStyle ?? CURSOR_STYLE.BLOCK, true);
  const sb = opts.scrollbar ?? [0, 0, 0];
  view.setBigUint64(32, BigInt(sb[0]), true);
  view.setBigUint64(40, BigInt(sb[1]), true);
  view.setBigUint64(48, BigInt(sb[2]), true);

  let off = WIRE_HEADER_LEN;
  for (const s of styles) {
    view.setInt32(off, s.fg, true);
    view.setInt32(off + 4, s.bg, true);
    view.setUint32(off + 8, s.flags, true);
    off += WIRE_STYLE_LEN;
  }

  for (const c of cells) {
    const lo =
      (c.codepoint & 0x1fffff) | ((c.wide & 0b11) << 21) | ((c.styleIdx & 0x1ff) << 23);
    const hi =
      (c.styleIdx >>> 9) |
      (c.hasText ? 1 << 7 : 0) |
      (((c.cluster ?? 0) << 8) >>> 0);
    view.setUint32(off, lo, true);
    view.setUint32(off + 4, hi, true);
    off += 8;
  }

  return buf;
}

/**
 * 给全量帧追加尾部（`VTF1`）：行标记 + 字素簇表。
 *
 * 独立函数而不是 `buildFrame` 的选项：既有用例大多在验「没有尾部也能解」，
 * 默认带尾会掩盖掉那条兼容性。
 */
function withFooter(frame: ArrayBuffer, clusters: string[], rowFlags: number[]): ArrayBuffer {
  const enc = new TextEncoder();
  const encoded = clusters.map((c) => enc.encode(c));
  const size =
    frame.byteLength + 8 + rowFlags.length * 2 + encoded.reduce((n, b) => n + 4 + b.length, 0);
  const out = new ArrayBuffer(size);
  new Uint8Array(out).set(new Uint8Array(frame));
  const v = new DataView(out);
  let off = frame.byteLength;
  v.setUint32(off, WIRE_FOOTER_MAGIC, true);
  off += 4;
  v.setUint16(off, encoded.length, true);
  off += 2;
  v.setUint16(off, rowFlags.length, true);
  off += 2;
  for (const f of rowFlags) {
    v.setUint16(off, f, true);
    off += 2;
  }
  for (const bytes of encoded) {
    v.setUint32(off, bytes.length, true);
    off += 4;
    new Uint8Array(out, off, bytes.length).set(bytes);
    off += bytes.length;
  }
  return out;
}

const blanks = (n: number): CellSpec[] =>
  Array.from({ length: n }, () => ({ codepoint: 0, wide: 0, styleIdx: 0, hasText: false }));

const single = (): StyleSpec[] => [{ fg: -1, bg: -1, flags: 0 }];

// ---------------------------------------------------------------------------
// 1. 解码器
// ---------------------------------------------------------------------------

pass('拒绝 magic 不匹配的缓冲', () => {
  const bad = new ArrayBuffer(128);
  new DataView(bad).setUint32(0, 0xdeadbeef, true);
  let threw = false;
  try {
    decodeCellFrame(bad);
  } catch {
    threw = true;
  }
  assert(threw, '应拒绝错误的 magic');
});

pass('拒绝被截断的缓冲', () => {
  const full = buildFrame({ cols: 4, rows: 2, styles: single(), cells: blanks(8) });
  let threw = false;
  try {
    decodeCellFrame(full.slice(0, full.byteLength - 8));
  } catch {
    threw = true;
  }
  assert(threw, '应拒绝不完整的缓冲');
});

pass('尺寸与光标位置往返', () => {
  const f = decodeCellFrame(
    buildFrame({
      cols: 80,
      rows: 24,
      cursorX: 12,
      cursorY: 7,
      styles: single(),
      cells: blanks(80 * 24)
    })
  );
  eq(f.cols, 80, '列数');
  eq(f.rows, 24, '行数');
  eq(f.cursorX, 12, '光标列');
  eq(f.cursorY, 7, '光标行');
});

pass('光标可见性与待换行标志还原', () => {
  const visible = decodeCellFrame(
    buildFrame({ cols: 2, rows: 1, flags: 1, styles: single(), cells: blanks(2) })
  );
  eq(visible.cursorVisible, true, 'bit0 = 可见');

  const hiddenWrapping = decodeCellFrame(
    buildFrame({ cols: 2, rows: 1, flags: 0b10, styles: single(), cells: blanks(2) })
  );
  eq(hiddenWrapping.cursorVisible, false, 'bit0 清零应不可见');
  eq(hiddenWrapping.cursorPendingWrap, true, 'bit1 = 待换行');
});

pass('光标视觉形状还原', () => {
  for (const style of [CURSOR_STYLE.BAR, CURSOR_STYLE.BLOCK, CURSOR_STYLE.UNDERLINE]) {
    const f = decodeCellFrame(
      buildFrame({ cols: 2, rows: 1, cursorStyle: style, styles: single(), cells: blanks(2) })
    );
    eq(f.cursorStyle, style, `形状 ${style}`);
  }
});

pass('滚动条读数还原', () => {
  const f = decodeCellFrame(
    buildFrame({
      cols: 2,
      rows: 1,
      scrollbar: [1234, 567, 40],
      styles: single(),
      cells: blanks(2)
    })
  );
  eq(f.scrollbar.total, 1234, 'total');
  eq(f.scrollbar.offset, 567, 'offset');
  eq(f.scrollbar.len, 40, 'len');
});

pass('码点与 has_text 还原', () => {
  const f = decodeCellFrame(
    buildFrame({
      cols: 3,
      rows: 1,
      styles: single(),
      cells: [
        { codepoint: 0x41, wide: WIDE_NARROW, styleIdx: 0, hasText: true },
        { codepoint: 0, wide: WIDE_NARROW, styleIdx: 0, hasText: false },
        { codepoint: 0x4e2d, wide: WIDE_WIDE, styleIdx: 0, hasText: true }
      ]
    })
  );
  eq(frameCell(f, 0, 0).codepoint, 0x41, '首格码点');
  eq(frameCell(f, 0, 0).hasText, true, '首格有字形');
  eq(frameCell(f, 1, 0).hasText, false, '空白格无字形');
  eq(frameCell(f, 2, 0).codepoint, 0x4e2d, '宽字符码点');
});

pass('宽字符 spacer 标记还原', () => {
  const f = decodeCellFrame(
    buildFrame({
      cols: 2,
      rows: 1,
      styles: single(),
      cells: [
        { codepoint: 0x4e2d, wide: WIDE_WIDE, styleIdx: 0, hasText: true },
        { codepoint: 0, wide: WIDE_SPACER_TAIL, styleIdx: 0, hasText: false }
      ]
    })
  );
  eq(frameCell(f, 1, 0).wide, WIDE_SPACER_TAIL, 'spacer 标记');
});

pass('样式索引跨越 lo/hi 边界仍指向正确样式', () => {
  // 索引 16 位：低 9 位在 lo、高 7 位在 hi。511 与 512 恰好是两段的分界，
  // 同时覆盖可发现「只取一段」的错误。
  const styles: StyleSpec[] = Array.from({ length: 600 }, (_, i) => ({
    fg: 0x100000 + i,
    bg: -1,
    flags: 0
  }));
  styles[511] = { fg: 0xaabbcc, bg: -1, flags: 0 };
  styles[512] = { fg: 0x112233, bg: -1, flags: 0 };
  styles[599] = { fg: 0x445566, bg: -1, flags: 0 };

  const f = decodeCellFrame(
    buildFrame({
      cols: 3,
      rows: 1,
      styles,
      cells: [
        { codepoint: 0x41, wide: 0, styleIdx: 511, hasText: true },
        { codepoint: 0x42, wide: 0, styleIdx: 512, hasText: true },
        { codepoint: 0x43, wide: 0, styleIdx: 599, hasText: true }
      ]
    })
  );

  eq(frameCell(f, 0, 0).style.fg, 0xaabbcc, '索引 511');
  eq(frameCell(f, 1, 0).style.fg, 0x112233, '索引 512');
  eq(frameCell(f, 2, 0).style.fg, 0x445566, '索引 599');
});

pass('属性标志位与 null 颜色还原', () => {
  const f = decodeCellFrame(
    buildFrame({
      cols: 2,
      rows: 1,
      styles: [
        { fg: -1, bg: 0x283040, flags: 0b000001 },
        { fg: -1, bg: -1, flags: 0b100101 }
      ],
      cells: [
        { codepoint: 0x41, wide: 0, styleIdx: 0, hasText: true },
        { codepoint: 0x42, wide: 0, styleIdx: 1, hasText: true }
      ]
    })
  );

  const a = frameCell(f, 0, 0).style;
  eq(a.fg, null, '-1 前景应解码为 null');
  eq(a.bg, 0x283040, '背景色');
  eq(a.bold, true, '粗体');

  const b = frameCell(f, 1, 0).style;
  eq(b.bold, true, '粗体');
  eq(b.faint, true, '淡化');
  eq(b.strikethrough, true, '删除线');
  eq(b.inverse, false, '非反显');
});

pass('越界读取返回空槽而非崩溃', () => {
  const f = decodeCellFrame(
    buildFrame({ cols: 2, rows: 2, styles: single(), cells: blanks(4) })
  );
  const out = frameCell(f, 99, 99);
  eq(out.hasText, false, '越界应无字形');
  eq(out.codepoint, 0, '越界码点应为 0');
});

pass('全量帧尾部：字素簇与行标记还原', () => {
  const base = buildFrame({
    cols: 2,
    rows: 2,
    styles: single(),
    cells: [
      { codepoint: 0x65, wide: WIDE_NARROW, styleIdx: 0, hasText: true, cluster: 1 },
      { codepoint: 0, wide: WIDE_NARROW, styleIdx: 0, hasText: false },
      { codepoint: 0, wide: WIDE_NARROW, styleIdx: 0, hasText: false },
      { codepoint: 0, wide: WIDE_NARROW, styleIdx: 0, hasText: false }
    ]
  });
  const f = decodeCellFrame(withFooter(base, ['e\u0301'], [RowFlag.Prompt, 0]));

  eq(f.clusters.length, 1, '簇表条数');
  eq(f.clusters[0], 'e\u0301', '簇字符串');
  eq(f.rowFlags.length, 2, '行标记条数');
  eq(f.rowFlags[0], RowFlag.Prompt, '第 0 行 prompt');
  eq(f.rowFlags[1], 0, '第 1 行无标记');

  const out = makeScratchCell();
  frameCellInto(f, 0, 0, out);
  eq(out.cluster, 'e\u0301', 'frameCellInto 应解出簇');
  frameCellInto(f, 1, 0, out);
  eq(out.cluster, null, '无簇格应为 null');
});

pass('全量帧：缺少尾部时按空表处理（旧帧兼容）', () => {
  const f = decodeCellFrame(
    buildFrame({ cols: 2, rows: 1, styles: single(), cells: blanks(2) })
  );
  eq(f.clusters.length, 0, '旧帧无簇表');
  eq(f.rowFlags.length, 0, '旧帧无行标记');
  eq(frameCell(f, 0, 0).cluster, null, '旧帧格无簇');
});

pass('全量帧尾部：声明长度越界必须抛错', () => {
  const base = buildFrame({
    cols: 1,
    rows: 1,
    styles: single(),
    cells: [{ codepoint: 0x65, wide: 0, styleIdx: 0, hasText: true, cluster: 1 }]
  });
  const good = withFooter(base, ['e\u0301'], [1]);
  // 截掉簇数据的最后一个字节：声明的 byte_len 比实际长
  let threw = false;
  try {
    decodeCellFrame(good.slice(0, good.byteLength - 1));
  } catch {
    threw = true;
  }
  assert(threw, '簇数据被截断时应抛错');
});

// ---------------------------------------------------------------------------
// 2. 跨语言往返：Rust 编码 → 本脚本解码
// ---------------------------------------------------------------------------

interface ExpectedCell {
  codepoint: number;
  wide: number;
  hasText: boolean;
  fg: number | null;
  bg: number | null;
  bold: boolean;
  italic: boolean;
}
interface Expected {
  cols: number;
  rows: number;
  cursorX: number;
  cursorY: number;
  byteLength: number;
  defaultFg: number | null;
  defaultBg: number | null;
  cursorColor: number | null;
  cells: ExpectedCell[];
}

function loadFixture() {
  const bin = readFileSync(resolve(FIXTURE_DIR, 'snapshot.bin'));
  const ab = bin.buffer.slice(bin.byteOffset, bin.byteOffset + bin.byteLength) as ArrayBuffer;
  const expected: Expected = JSON.parse(
    readFileSync(resolve(FIXTURE_DIR, 'snapshot.expected.json'), 'utf8')
  );
  return { frame: decodeCellFrame(ab), expected, byteLength: bin.byteLength };
}

pass('跨语言：缓冲长度与 Rust 声明一致', () => {
  const { expected, byteLength } = loadFixture();
  eq(byteLength, expected.byteLength, '字节长度');
});

pass('跨语言：尺寸、光标与默认色一致', () => {
  const { frame, expected } = loadFixture();
  eq(frame.cols, expected.cols, '列数');
  eq(frame.rows, expected.rows, '行数');
  eq(frame.cursorX, expected.cursorX, '光标列');
  eq(frame.cursorY, expected.cursorY, '光标行');
  eq(frame.defaultFg, expected.defaultFg, '默认前景色');
  eq(frame.defaultBg, expected.defaultBg, '默认背景色');
  eq(frame.cursorColor, expected.cursorColor, '光标色');
});

pass('跨语言：逐格比对码点、宽度、字形与样式', () => {
  const { frame, expected } = loadFixture();

  for (let i = 0; i < expected.cells.length; i++) {
    const x = i % expected.cols;
    const y = Math.floor(i / expected.cols);
    const want = expected.cells[i];
    const got = frameCell(frame, x, y);
    const where = `格 [${i}] (${x},${y})`;

    eq(got.codepoint, want.codepoint, `${where} 码点`);
    eq(got.wide, want.wide, `${where} 宽度`);
    eq(got.hasText, want.hasText, `${where} 有字形`);
    eq(got.style.fg, want.fg, `${where} 前景色`);
    eq(got.style.bg, want.bg, `${where} 背景色`);
    eq(got.style.bold, want.bold, `${where} 粗体`);
    eq(got.style.italic, want.italic, `${where} 斜体`);
  }
});

pass('跨语言：索引色与真彩无损穿过样式表', () => {
  const { frame } = loadFixture();
  // cell 2 = \x1b[31m 下的 'C'，cell 4 = \x1b[38;2;10;200;90m 下的 'E'
  eq(frameCell(frame, 2, 0).style.fg, 0xcc6666, '索引色 1 → #cc6666');
  eq(frameCell(frame, 4, 0).style.fg, 0x0ac85a, '真彩 #0ac85a');
});

pass('跨语言：宽字符占两格且第二格为 spacer', () => {
  const { frame } = loadFixture();
  eq(frameCell(frame, 8, 0).codepoint, 0x4e2d, '中文首字');
  eq(frameCell(frame, 8, 0).wide, WIDE_WIDE, '应为宽字符');
  eq(frameCell(frame, 9, 0).wide, WIDE_SPACER_TAIL, '第二格应为 spacer');
  eq(frameCell(frame, 9, 0).hasText, false, 'spacer 无字形');
});

pass('跨语言：空白格为 NUL 且无样式覆写', () => {
  const { frame } = loadFixture();
  const blank = frameCell(frame, 0, 1);
  eq(blank.codepoint, 0, '空白格码点');
  eq(blank.hasText, false, '空白格不应标记有字形');
  eq(blank.style.fg, null, '空白格不应有前景色');
});

// ---------------------------------------------------------------------------
// 2b. 增量帧（VTN3）：跨语言往返 + 前端合并
// ---------------------------------------------------------------------------

interface DiffExpected {
  partialRowCells: ExpectedCell[];
  fullLength: number;
  partialLength: number;
  cells: ExpectedCell[];
}

function loadDiffFixture() {
  const full = readFileSync(resolve(FIXTURE_DIR, 'diff-full.bin'));
  const partial = readFileSync(resolve(FIXTURE_DIR, 'diff-partial.bin'));
  const expected: DiffExpected = JSON.parse(
    readFileSync(resolve(FIXTURE_DIR, 'diff.expected.json'), 'utf8')
  );
  const asAB = (b: Buffer) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) as ArrayBuffer;
  return {
    fullDiff: decodeCellDiff(asAB(full)),
    partialDiff: decodeCellDiff(asAB(partial)),
    fullLength: full.byteLength,
    partialLength: partial.byteLength,
    expected
  };
}

pass('增量帧：缓冲长度与 Rust 声明一致', () => {
  const { expected, fullLength, partialLength } = loadDiffFixture();
  eq(fullLength, expected.fullLength, '全量帧长度');
  eq(partialLength, expected.partialLength, '差分帧长度');
  assert(
    partialLength < fullLength,
    `差分帧 ${partialLength}B 应小于全量帧 ${fullLength}B —— 否则增量没有收益`
  );
});

pass('增量帧：全量帧逐格覆盖 Rust 真值', () => {
  const { fullDiff, expected } = loadDiffFixture();
  eq(fullDiff.rows.length, fullDiff.gridRows, '全量帧应覆盖每一行');

  for (const row of fullDiff.rows) {
    for (let x = 0; x < fullDiff.cols; x++) {
      const i = row.y * fullDiff.cols + x;
      const want = expected.cells[i];
      const where = `差分帧格 [${i}] (${x},${row.y})`;
      // cells 在帧缓冲里，用 frameCellInto 直接读
      const lo = new DataView(fullDiff.buffer).getUint32(row.cellsOffset + x * 8, true);
      const hi = new DataView(fullDiff.buffer).getUint32(row.cellsOffset + x * 8 + 4, true);
      const codepoint = lo & 0x1fffff;
      const wide = (lo >>> 21) & 0b11;
      const hasText = ((hi >>> 7) & 1) !== 0;
      const styleIdx = ((lo >>> 23) & 0x1ff) | ((hi & 0x7f) << 9);

      eq(codepoint, want.codepoint, `${where} 码点`);
      eq(wide, want.wide, `${where} 宽度`);
      eq(hasText, want.hasText, `${where} 有字形`);

      // cells 里的 style_idx 是**帧级**样式表的全局下标
      const style = fullDiff.styles[styleIdx];
      assert(style, `${where} 样式索引 ${styleIdx} 越界（表共 ${fullDiff.styles.length} 项）`);
      eq(style.fg, want.fg, `${where} 前景色`);
      eq(style.bg, want.bg, `${where} 背景色`);
      eq(style.bold, want.bold, `${where} 粗体`);
      eq(style.italic, want.italic, `${where} 斜体`);
    }
  }
});

pass('增量帧：差分帧只带变动行', () => {
  const { partialDiff } = loadDiffFixture();
  assert(
    partialDiff.rows.length > 0 && partialDiff.rows.length < partialDiff.gridRows,
    `只改一行时应只送部分行，实际 ${partialDiff.rows.length}/${partialDiff.gridRows} 行`
  );
  assert(
    partialDiff.rows.some(r => r.y === 1),
    '被修改的第 1 行必须在差分帧里'
  );
});

pass('增量帧：差分帧每行与 Rust 真值一致', () => {
  const { partialDiff, expected } = loadDiffFixture();
  const view = new DataView(partialDiff.buffer);
  for (const row of partialDiff.rows) {
    // 差分帧同时含「旧光标所在行」与「新写入的行」：前者在首帧时刻是 'A'，
    // 后者在差分时刻是 'x'。按行号取对应时刻的真值。
    // `partialRowCells` 是从第 1 行开始切的 24 项，故以 (y-1) 为基。
    const truth =
      row.y === 0
        ? expected.cells // 首帧时刻的第 0 行
        : expected.partialRowCells; // 差分时刻的第 1 行
    const base = row.y === 0 ? 0 : row.y * partialDiff.cols - partialDiff.cols;
    for (let x = 0; x < partialDiff.cols; x++) {
      const want = truth[base + x];
      const where = `差分帧行 ${row.y} 格 ${x}`;
      const lo = view.getUint32(row.cellsOffset + x * 8, true);
      eq(lo & 0x1fffff, want.codepoint, `${where} 码点`);
      eq((lo >>> 21) & 0b11, want.wide, `${where} 宽度`);
    }
  }
});

pass('增量帧：前端合并（CellDiffCache）逐格还原真值', () => {
  const { fullDiff, partialDiff, expected } = loadDiffFixture();
  const cache = new CellDiffCache();
  cache.reset(emptyFrameForMerge(fullDiff.cols, fullDiff.gridRows));
  // 首帧（整屏）→ 差分帧（只覆盖第 1 行），与运行时的两条路径一致
  cache.applyDiff(fullDiff);
  cache.applyDiff(partialDiff);
  assert(!cache.needsFullRebuild, '合并不应触发整屏重建');

  const view = cache.frameView({
    cursorX: 0,
    cursorY: 0,
    cursorVisible: true,
    cursorPendingWrap: false,
    defaultFg: null,
    defaultBg: null,
    cursorColor: null,
    cursorStyle: CURSOR_STYLE.BLOCK,
    scrollbar: { total: 0, offset: 0, len: 0 }
  });
  const cell = makeScratchCell();
  for (let y = 0; y < view.rows; y++) {
    for (let x = 0; x < view.cols; x++) {
      // 第 0 行取首帧真值（该行内容未动），第 1 行起取差分时刻真值
      const want =
        y === 0
          ? expected.cells[y * view.cols + x]
          : expected.partialRowCells[(y - 1) * view.cols + x];
      const where = `合并后格 (${x},${y})`;
      frameCellInto(view, x, y, cell);
      eq(cell.codepoint, want.codepoint, `${where} 码点`);
      eq(cell.wide, want.wide, `${where} 宽度`);
      eq(cell.hasText, want.hasText, `${where} 有字形`);
      eq(cell.style.fg, want.fg, `${where} 前景色`);
      eq(cell.style.bg, want.bg, `${where} 背景色`);
      eq(cell.style.bold, want.bold, `${where} 粗体`);
      eq(cell.style.italic, want.italic, `${where} 斜体`);
    }
  }
});

pass('增量帧：多脏行各自新样式按帧级下标着色（旧实现错位/丢色）', () => {
  // 手工构造一帧：帧级样式表 2 项，行 0 只用样式 0、行 1 只用样式 1，
  // 且两行都在本帧引入自己的样式。旧实现按「行内片段」重映射，
  // 第 2 行会取到第 1 行的样式（实测 fg 解析为 undefined）。
  const diff = decodeCellDiff(buildTwoStyleDiff());
  const cache = new CellDiffCache();
  cache.reset(emptyFrameForMerge(diff.cols, diff.gridRows));
  cache.applyDiff(diff);

  const view = cache.frameView({
    cursorX: 0,
    cursorY: 0,
    cursorVisible: true,
    cursorPendingWrap: false,
    defaultFg: null,
    defaultBg: null,
    cursorColor: null,
    cursorStyle: CURSOR_STYLE.BLOCK,
    scrollbar: { total: 0, offset: 0, len: 0 }
  });
  const cell = makeScratchCell();
  frameCellInto(view, 0, 0, cell);
  eq(cell.style.fg, 0xff0000, '第 0 行的前景色应为红');
  frameCellInto(view, 0, 1, cell);
  eq(cell.style.bg, 0x00ff00, '第 1 行的背景色应为绿');
});

pass('增量帧：行标记来自行头，簇表来自帧尾', () => {
  const diff = decodeCellDiff(buildClusterDiff(['e\u0301'], 1, RowFlag.Prompt));
  eq(diff.clusters.length, 1, '帧级簇表条数');
  eq(diff.clusters[0], 'e\u0301', '帧级簇表内容');
  eq(diff.rowFlags.length, diff.gridRows, '行标记按网格行数');
  eq(diff.rowFlags[0], RowFlag.Prompt, '第 0 行行头带 prompt 标记');
  eq(diff.rowFlags[1], 0, '第 1 行无标记');

  const row0 = diff.rows.find((r) => r.y === 0);
  assert(row0, '第 0 行应在帧里');
  const hi = new DataView(diff.buffer).getUint32(row0.cellsOffset + 4, true);
  eq(hi >>> 8, 1, '格内簇下标应为 1');
});

pass('增量帧：缓存合并重映射簇下标并保留行标记', () => {
  const diff1 = decodeCellDiff(buildClusterDiff(['e\u0301'], 1, RowFlag.Prompt));
  const cache = new CellDiffCache();
  cache.reset(emptyFrameForMerge(diff1.cols, diff1.gridRows));
  cache.applyDiff(diff1);

  const cursor = {
    cursorX: 0,
    cursorY: 0,
    cursorVisible: true,
    cursorPendingWrap: false,
    defaultFg: null,
    defaultBg: null,
    cursorColor: null,
    cursorStyle: CURSOR_STYLE.BLOCK,
    scrollbar: { total: 0, offset: 0, len: 0 }
  };
  const cell = makeScratchCell();
  const view1 = cache.frameView(cursor);
  frameCellInto(view1, 0, 0, cell);
  eq(cell.cluster, 'e\u0301', '合并后应解出簇');
  eq(view1.rowFlags[0], RowFlag.Prompt, '合并后行标记');

  // 第二帧引入**另一个**簇：缓存必须把帧内下标重映射到自己的表尾，
  // 否则新簇会覆盖旧下标、未脏行指向错的簇。
  const diff2 = decodeCellDiff(buildClusterDiff(['x\u0308'], 1, 0));
  cache.applyDiff(diff2);
  const view2 = cache.frameView(cursor);
  frameCellInto(view2, 0, 0, cell);
  eq(cell.cluster, 'x\u0308', '第二帧簇应重映射到缓存表');
  eq(view2.rowFlags[0], 0, '行标记随脏行更新');
  assert(cache.clusters.includes('e\u0301'), '旧簇仍应留在缓存表里');
});

/**
 * 手写一帧 VTN3：cols=3 rows=2，1 条样式，行 0 首格带簇、行头带标记，
 * 行 1 全空白，帧尾带簇表。
 */
function buildClusterDiff(clusters: string[], row0Cluster: number, row0Flags: number): ArrayBuffer {
  const cols = 3;
  const gridRows = 2;
  const enc = new TextEncoder();
  const encoded = clusters.map((c) => enc.encode(c));
  const rowFlags = [row0Flags, 0];
  const size =
    60 +
    12 +
    gridRows * (8 + cols * 8) +
    8 +
    encoded.reduce((n, b) => n + 4 + b.length, 0);
  const buf = new ArrayBuffer(size);
  const v = new DataView(buf);

  v.setUint32(0, 0x54564e33, true); // VTN3
  v.setUint16(4, cols, true);
  v.setUint16(6, gridRows, true);
  v.setUint16(8, 1, true); // 光标可见
  v.setInt32(10, -1, true);
  v.setInt32(14, -1, true);
  v.setInt32(18, -1, true);
  v.setUint32(22, CURSOR_STYLE.BLOCK, true);
  v.setUint16(26, 0, true);
  v.setUint16(28, 0, true);
  v.setUint16(30, 1, true); // style_count
  v.setUint16(32, gridRows, true);
  v.setInt16(34, 0, true);
  v.setBigUint64(36, 0n, true);
  v.setBigUint64(44, 0n, true);
  v.setBigUint64(52, 0n, true);

  let off = 60;
  v.setInt32(off, -1, true);
  v.setInt32(off + 4, -1, true);
  v.setUint32(off + 8, 0, true);
  off += 12;

  for (let y = 0; y < gridRows; y++) {
    v.setUint16(off, y, true);
    v.setUint16(off + 2, 0, true);
    v.setUint16(off + 4, rowFlags[y], true);
    v.setUint16(off + 6, 0, true);
    off += 8;
    for (let x = 0; x < cols; x++) {
      const codepoint = y === 0 && x === 0 ? 0x65 : 0;
      const hasText = codepoint !== 0;
      const cluster = hasText ? row0Cluster : 0;
      const lo = codepoint & 0x1fffff;
      const hi = (hasText ? 1 << 7 : 0) | ((cluster << 8) >>> 0);
      v.setUint32(off, lo, true);
      v.setUint32(off + 4, hi, true);
      off += 8;
    }
  }

  v.setUint32(off, WIRE_DIFF_FOOTER_MAGIC, true);
  off += 4;
  v.setUint16(off, encoded.length, true);
  off += 2;
  v.setUint16(off, 0, true); // reserved
  off += 2;
  for (const bytes of encoded) {
    v.setUint32(off, bytes.length, true);
    off += 4;
    new Uint8Array(buf, off, bytes.length).set(bytes);
    off += bytes.length;
  }
  eq(off, buf.byteLength, '构造器长度自洽');
  return buf;
}

/** 空的 CellFrame（供 `CellDiffCache.reset` 建立尺寸与缓冲）。 */
function emptyFrameForMerge(cols: number, rows: number): CellFrame {
  return {
    cols,
    rows,
    cursorX: 0,
    cursorY: 0,
    cursorVisible: true,
    cursorPendingWrap: false,
    defaultFg: null,
    defaultBg: null,
    cursorColor: null,
    cursorStyle: CURSOR_STYLE.BLOCK,
    scrollbar: { total: 0, offset: 0, len: 0 },
    styles: [],
    words: new Uint32Array(cols * rows * 2),
    clusters: [],
    rowFlags: new Uint16Array(rows)
  };
}

function makeScratchCell(): FrameCell {
  return { codepoint: 0, wide: WIDE_NARROW, hasText: false, style: EMPTY_STYLE };
}

// ---------------------------------------------------------------------------
// 2c. 字素簇 / 行标记：跨语言往返
// ---------------------------------------------------------------------------

interface GraphemeExpected {
  cols: number;
  rows: number;
  clusters: string[];
  rowFlags: number[];
  cells: { codepoint: number; wide: number; hasText: boolean; clusterIndex: number }[];
  diffLength: number;
}

function loadGraphemeFixture() {
  const asAB = (b: Buffer) =>
    b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength) as ArrayBuffer;
  const full = readFileSync(resolve(FIXTURE_DIR, 'grapheme.bin'));
  const diff = readFileSync(resolve(FIXTURE_DIR, 'grapheme-diff.bin'));
  const expected: GraphemeExpected = JSON.parse(
    readFileSync(resolve(FIXTURE_DIR, 'grapheme.expected.json'), 'utf8')
  );
  return {
    frame: decodeCellFrame(asAB(full)),
    diff: decodeCellDiff(asAB(diff)),
    diffLength: diff.byteLength,
    expected
  };
}

pass('跨语言：字素簇与行标记穿过全量帧', () => {
  const { frame, expected } = loadGraphemeFixture();
  eq(JSON.stringify(frame.clusters), JSON.stringify(expected.clusters), '簇表');
  eq(
    JSON.stringify([...frame.rowFlags]),
    JSON.stringify(expected.rowFlags),
    '行标记'
  );

  const cell = makeScratchCell();
  for (let i = 0; i < expected.cells.length; i++) {
    const x = i % expected.cols;
    const y = Math.floor(i / expected.cols);
    const want = expected.cells[i];
    const where = `格 [${i}] (${x},${y})`;
    frameCellInto(frame, x, y, cell);
    eq(cell.codepoint, want.codepoint, `${where} 码点`);
    eq(cell.wide, want.wide, `${where} 宽度`);
    eq(cell.hasText, want.hasText, `${where} 有字形`);
    const wantCluster =
      want.clusterIndex === 0 ? null : expected.clusters[want.clusterIndex - 1];
    eq(cell.cluster, wantCluster, `${where} 簇`);
  }

  // 防止夹具退化成空表后「全 null」仍然全绿
  assert(frame.clusters.includes('e\u0301'), '应含 e + U+0301 单簇');
  assert(frame.clusters.includes('👨‍👩‍👧‍👦'), '应含家庭 emoji 整簇');
  eq(frame.rowFlags[0], RowFlag.Prompt, '第 0 行应是 prompt 行');
});

pass('跨语言：增量帧携带同一簇与行标记', () => {
  const { diff, diffLength, expected } = loadGraphemeFixture();
  eq(diffLength, expected.diffLength, '增量帧长度');
  eq(JSON.stringify(diff.clusters), JSON.stringify(expected.clusters), '帧级簇表');
  eq(diff.rowFlags[0], RowFlag.Prompt, '第 0 行行头应带 prompt 标记');
  eq(diff.rowFlags[1], 0, '第 1 行不应带 prompt 标记');

  const row0 = diff.rows.find((r) => r.y === 0);
  assert(row0, '第 0 行应在增量帧里');
  // 夹具第 0 行是 `$ e` + U+0301：簇在第 2 格
  const hi = new DataView(diff.buffer).getUint32(row0.cellsOffset + 2 * 8 + 4, true);
  eq(hi >>> 8, 1, '第 2 格应指向簇 1');
  eq(diff.clusters[(hi >>> 8) - 1], 'e\u0301', '簇 1 应是 e + U+0301');
});

/**
 * 手写一帧 VTN3：cols=4 rows=2，帧级样式表 [红前景, 绿背景]，
 * 行 0 全部格用样式 0，行 1 全部格用样式 1。
 */
function buildTwoStyleDiff(): ArrayBuffer {
  const cols = 4;
  const gridRows = 2;
  const styles: Array<[number, number]> = [
    [0xff0000, -1],
    [-1, 0x00ff00]
  ];
  const rowStyles: number[] = [0, 1];
  const buf = new ArrayBuffer(60 + styles.length * 12 + rowStyles.length * (8 + cols * 8));
  const v = new DataView(buf);
  v.setUint32(0, 0x54564e33, true); // VTN3
  v.setUint16(4, cols, true);
  v.setUint16(6, gridRows, true);
  v.setUint16(8, 1, true); // 光标可见
  v.setInt32(10, -1, true);
  v.setInt32(14, -1, true);
  v.setInt32(18, -1, true);
  v.setUint32(22, CURSOR_STYLE.BLOCK, true);
  v.setUint16(26, 0, true);
  v.setUint16(28, 0, true);
  v.setUint16(30, styles.length, true);
  v.setUint16(32, rowStyles.length, true);
  v.setInt16(34, 0, true);
  v.setBigUint64(36, 0n, true);
  v.setBigUint64(44, 0n, true);
  v.setBigUint64(52, 0n, true);

  let off = 60;
  for (const [fg, bg] of styles) {
    v.setInt32(off, fg, true);
    v.setInt32(off + 4, bg, true);
    v.setUint32(off + 8, 0, true);
    off += 12;
  }
  rowStyles.forEach((styleIdx, y) => {
    v.setUint16(off, y, true);
    v.setUint16(off + 2, 0, true);
    v.setUint16(off + 4, 0, true);
    v.setUint16(off + 6, 0, true);
    off += 8;
    for (let x = 0; x < cols; x++) {
      const lo = 0x41 | ((styleIdx & 0x1ff) << 23); // 'A'
      const hi = ((styleIdx >>> 9) & 0x7f) | (1 << 7);
      v.setUint32(off + x * 8, lo >>> 0, true);
      v.setUint32(off + x * 8 + 4, hi, true);
    }
    off += cols * 8;
  });
  return buf;
}

// ---------------------------------------------------------------------------
// 3. 键盘键值：前端生成表 vs 原生枚举
// ---------------------------------------------------------------------------

pass('键盘键值与原生枚举一致', () => {
  // 数值由 scripts/gen-key-table.mjs 从 event.h 生成；这里再与 fixture
  // 逐项对账，防止生成脚本或 fixture 单方面漂移（症状都是「按键无反应」：
  // 本项目曾因 MINUS/PERIOD/QUOTE/SEMICOLON/SLASH 各偏 1 而丢掉输入）。
  const nativeText = readFileSync(resolve(FIXTURE_DIR, 'native-keys.txt'), 'utf8');
  const native = new Map<string, number>();
  for (const line of nativeText.split('\n').slice(1)) {
    const m = /^\s*(\w+) = (\d+)$/.exec(line.trim());
    if (m) native.set(m[1], Number(m[2]));
  }
  assert(native.size > 100, `原生键表应含百余项，实际 ${native.size}`);

  const domSrc = readFileSync(resolve(ROOT, 'src/ghostty-vt/domKeys.ts'), 'utf8');
  assert(
    /\.\.\.GHOSTTY_KEY_VALUES/.test(domSrc),
    'domKeys.ts 应展开生成的 GHOSTTY_KEY_VALUES，而不是手写数字'
  );

  const src = readFileSync(resolve(ROOT, 'src/ghostty-vt/keyValues.generated.ts'), 'utf8');
  const block = /export const GHOSTTY_KEY_VALUES = \{([\s\S]*?)\} as const;/.exec(src);
  assert(block, '未找到 keyValues.generated.ts 中的 GHOSTTY_KEY_VALUES 表');

  const entries = [...block[1].matchAll(/(\w+):\s*(\d+)/g)];
  assert(entries.length > 20, `键表过小，仅 ${entries.length} 项`);

  const mismatched: string[] = [];
  for (const [, name, value] of entries) {
    const expected = native.get(name);
    if (expected === undefined) {
      mismatched.push(`${name} 不在原生枚举中`);
    } else if (expected !== Number(value)) {
      mismatched.push(`${name} 代码=${value} 原生=${expected}`);
    }
  }
  assert(mismatched.length === 0, `键值不符: ${mismatched.join('; ')}`);

  // 反向对账：native 表里「前端应当支持」的键，必须在 TS 表中存在。
  // 单向遍历只能发现「值写错」，抓不到「整组丢失」—— 本项目曾因整个
  // JIS 三键缺失而丢掉日文键盘输入，当时 CI 全绿。
  // 判据不是 native 全部 175 项（大量按键没有对应 DOM code），而是
  // 与 DOM KeyboardEvent.code 有稳定对应的标准键组。
  const REQUIRED_GROUPS: Record<string, RegExp> = {
    '小键盘数字': /^NUMPAD_[0-9]$/,
    '小键盘运算符': /^NUMPAD_(ADD|SUBTRACT|MULTIPLY|DIVIDE|DECIMAL|EQUAL|COMMA|PAREN_LEFT|PAREN_RIGHT)$/,
    '小键盘控制': /^NUMPAD_ENTER$/,
    '功能键': /^F([1-9]|1[0-9]|2[0-4])$/,
    '修饰键': /^(SHIFT|CONTROL|ALT|META)_(LEFT|RIGHT)$/,
    'JIS 键盘': /^INTL_|^CONVERT$|^KANA_MODE$|^NON_CONVERT$/,
    '方向与导航': /^ARROW_(UP|DOWN|LEFT|RIGHT)$|^PAGE_(UP|DOWN)$|^HOME$|^END$|^INSERT$|^DELETE$|^BACKSPACE$|^ESCAPE$|^ENTER$|^TAB$|^SPACE$/,
    '锁定与系统': /^CAPS_LOCK$|^NUM_LOCK$|^SCROLL_LOCK$|^PRINT_SCREEN$|^CONTEXT_MENU$/,
    '符号与标点': /^(MINUS|PERIOD|QUOTE|SEMICOLON|SLASH|EQUAL|COMMA|BACKQUOTE|BACKSLASH|BRACKET_LEFT|BRACKET_RIGHT)$/
  };
  const tsNames: Record<string, true> = {};
  for (const [, n] of entries) tsNames[n] = true;
  const missing: string[] = [];
  for (const [group, pattern] of Object.entries(REQUIRED_GROUPS)) {
    for (const name of native.keys()) {
      if (pattern.test(name) && !(name in tsNames)) {
        missing.push(`${group}/${name}`);
      }
    }
  }
  assert(missing.length === 0, `键表缺少必需分组: ${missing.join(', ')}`);

  // 锁定条目数下限：表只能增长，减少即失败（防止误删整组）。
  // 原有 44 + Numpad 括号 2 + 修饰键 8 + 系统/锁定 4 + JIS 3 + F1..F24 的
  // 23 个新增 = 84 项。
  assert(
    entries.length >= 84,
    `GHOSTTY_KEY 表至少 84 项（当前 ${entries.length}）—— 减少意味着有键组被删`
  );
});

// ---------------------------------------------------------------------------
// 修饰键位与按键动作：前端硬编码值 vs 原生头文件
// ---------------------------------------------------------------------------

/** 解析 `NAME = N` 形态的 fixture 行 */
function parseNativeFixture(text: string): Record<string, number> {
  const out: Record<string, number> = {};
  for (const line of text.split('\n').slice(1)) {
    const m = /^\s*(\w+) = (\d+)$/.exec(line.trim());
    if (m) out[m[1]] = Number(m[2]);
  }
  return out;
}

pass('MODS 位域与原生头文件一致', () => {
  // CAPS_LOCK 位偏 1 会被当成 SHIFT 上报、action 值错位会让 RELEASE 被
  // 当成 PRESS —— 按键行为全乱。这些常量与 GHOSTTY_KEY 一样是硬编码，
  // 必须同样对账（此前只有 GHOSTTY_KEY 有 fixture 对账）。
  const nativeText = readFileSync(resolve(FIXTURE_DIR, 'native-mods.txt'), 'utf8');
  const native = parseNativeFixture(nativeText);
  assert(Object.keys(native).length >= 6, `MODS fixture 至少 6 项，实际 ${Object.keys(native).length}`);

  const src = readFileSync(resolve(ROOT, 'src/ghostty-vt/domKeys.ts'), 'utf8');
  const tsBlock = /export const MODS = \{([\s\S]*?)\} as const;/.exec(src);
  assert(tsBlock, '未找到 domKeys.ts 中的 MODS 表');

  // fixture 里的 GHOSTTY_MODS_SHIFT 对应 TS 的 SHIFT
  const mismatched: string[] = [];
  for (const [nativeName, nativeValue] of Object.entries(native)) {
    const tsName = nativeName.replace(/^GHOSTTY_MODS_/, '');
    const tsValue = evalBit(tsBlock[1], tsName);
    if (tsValue === undefined) {
      // SIDE 位（SHIFT_SIDE 等）前端尚未使用：不算缺失，只要求值一致
      if (!/_SIDE$/.test(nativeName)) mismatched.push(`${tsName} 不在前端 MODS 表中`);
      continue;
    }
    if (tsValue !== nativeValue) {
      mismatched.push(`${tsName} 前端=${tsValue} 原生=${nativeValue}`);
    }
  }
  assert(mismatched.length === 0, `MODS 位域不符: ${mismatched.join('; ')}`);
});

pass('KEY_ACTION 值与原生枚举一致', () => {
  const nativeText = readFileSync(resolve(FIXTURE_DIR, 'native-keyaction.txt'), 'utf8');
  const native = parseNativeFixture(nativeText);
  assert(Object.keys(native).length >= 3, `KEY_ACTION fixture 至少 3 项，实际 ${Object.keys(native).length}`);

  const src = readFileSync(resolve(ROOT, 'src/ghostty-vt/domKeys.ts'), 'utf8');
  const tsBlock = /export const KEY_ACTION = \{([\s\S]*?)\} as const;/.exec(src);
  assert(tsBlock, '未找到 domKeys.ts 中的 KEY_ACTION 表');

  const mismatched: string[] = [];
  for (const [tsName, tsValue] of Object.entries(evalEntryMap(tsBlock[1]))) {
    const expected = native[tsName];
    if (expected === undefined) {
      mismatched.push(`${tsName} 不在原生枚举中`);
    } else if (expected !== tsValue) {
      mismatched.push(`${tsName} 前端=${tsValue} 原生=${expected}`);
    }
  }
  assert(mismatched.length === 0, `KEY_ACTION 值不符: ${mismatched.join('; ')}`);
});

/** 从 `NAME: 1 << 2,` 片段里取某个名字的位值 */
function evalBit(block: string, name: string): number | undefined {
  const m = new RegExp(`\\b${name}\\s*:\\s*([0-9]+)\\s*<<\\s*([0-9]+)`).exec(block);
  if (m) return (Number(m[1]) << Number(m[2])) >>> 0;
  const plain = new RegExp(`\\b${name}\\s*:\\s*(\\d+)`).exec(block);
  return plain ? Number(plain[1]) : undefined;
}

/** 解析 `NAME: N` 片段为查找表 */
function evalEntryMap(block: string): Record<string, number> {
  const out: Record<string, number> = {};
  for (const m of block.matchAll(/(\w+)\s*:\s*([0-9]+)\s*<<\s*([0-9]+)/g)) {
    out[m[1]] = (Number(m[2]) << Number(m[3])) >>> 0;
  }
  for (const m of block.matchAll(/(\w+)\s*:\s*(\d+)/g)) {
    if (!(m[1] in out)) out[m[1]] = Number(m[2]);
  }
  return out;
}

// ---------------------------------------------------------------------------
// 终端网格尺寸：宿主 content-box → cols/rows
// ---------------------------------------------------------------------------

pass('终端网格由宿主 content-box 推导且不重复扣 padding', () => {
  // 复现过的 bug：ResizeObserver 走 contentRect（已扣 padding），spawn 后的
  // 兜底却用 getBoundingClientRect() - 16 再扣一遍。同一宿主算出两套行列，
  // PTY 停在窄网格上：prompt 按旧列数折行、光标被挤到第二行行首，
  // 光标之下的整片区域永远不渲染内容（截图 3 的红框区域）。
  const src = readFileSync(
    resolve(ROOT, 'src/components/GhosttyTerminal.tsx'),
    'utf8'
  );

  assert(
    /export function gridSizeFromHost\(/.test(src),
    '未找到 GhosttyTerminal.tsx 的 gridSizeFromHost'
  );

  // 宿主 823×398（截图 3 实测比例）、单元格 7.8×18（13px 字号 * 1.38）
  // 与生产实现同口径：content-box 直接除，不再扣 16px padding
  const grid = { cols: Math.max(20, Math.floor(823 / 7.8)), rows: Math.max(4, Math.floor(398 / 18)) };
  assert(
    grid.cols === 105 && grid.rows === 22,
    `宿主 823×398 / 格 7.8×18 应为 105×22，实际 ${grid.cols}×${grid.rows}`
  );

  // 同一宿主若再扣一遍 16px，列数/行数各少 2 —— 画面比视口窄一截，
  // 而 shell 已按宽网格输出，prompt 就会折行、光标错位
  const narrow = { cols: Math.max(20, Math.floor((823 - 16) / 7.8)), rows: Math.max(4, Math.floor((398 - 16) / 18)) };
  assert(
    narrow.cols !== grid.cols && narrow.rows !== grid.rows,
    'content-box 尺寸不应再减一遍 padding（否则网格小于视口）'
  );

  // 代码里不允许再出现「宿主尺寸 - 16」的手工扣 padding（注释里的历史说明除外）
  const codeLines = src
    .split('\n')
    .filter(l => !/^\s*(\/\/|\*|\/\*)/.test(l))
    .join('\n');
  const rectSubtract = /(rect|getBoundingClientRect\(\))[\s\S]{0,80}-\s*16/.test(codeLines);
  assert(!rectSubtract, '不应再出现宿主尺寸 - 16 的手工扣 padding');

  // applyHostSize 是唯一推导入口：RO 回调、spawn 兜底、rAF 自愈都走它
  const applyUsesHelper = /applyHostSize[\s\S]{0,500}gridSizeFromHost\(/.test(src);
  assert(applyUsesHelper, 'applyHostSize 必须调用 gridSizeFromHost');
  assert(
    /const ro = new ResizeObserver\(\(\) => \{\s*applyHostSize\(\);/.test(src),
    'ResizeObserver 回调必须走 applyHostSize'
  );

  // rAF 自愈：RO 漏回调（面板从 display:none 恢复 / 抽屉开关 / 最大化切换）时，
  // 渲染循环按尺寸签名重算，否则 PTY 永久停在旧网格
  assert(/lastHostSig/.test(src), '渲染循环需要 lastHostSig 尺寸签名');
  assert(
    /if \(hostSig !== lastHostSig\.current\)[\s\S]{0,120}applyHostSize\(\)/.test(src),
    '尺寸签名变化时必须调用 applyHostSize'
  );

  // 隐藏面板不应缩放网格：宿主 <4px 直接返回。
  // 用 clientWidth/clientHeight（content-box）而不是 offsetWidth（含 padding）：
  // 含 padding 的尺寸会多算一列/一行，canvas 超出内容区被 overflow 裁掉。
  assert(
    /clientWidth < 4 \|\| host\.clientHeight < 4/.test(src),
    '宿主不可见时必须跳过同步（避免 PTY 被缩到最小行列）'
  );

  // 格宽/格高必须是整数：pty_resize 的 cell_*_px 是 u32，小数会让整条
  // IPC 反序列化失败（"invalid type: floating point"），PTY 停在 spawn 网格，
  // canvas 只画旧行数、下方露出宿主底色（「内容只占上半屏」）。
  assert(
    /cellW\.current = Math\.max\(1, Math\.round\(/.test(src),
    'cellW 必须取整：小数会被 pty_resize 的 u32 拒绝'
  );
  assert(
    /cellH\.current = Math\.max\(1, Math\.round\(/.test(src),
    'cellH 必须取整：小数会被 pty_resize 的 u32 拒绝'
  );

  // resize 不能与前端 state 比对来决定是否下发：state 可能在 spawn 前就被
  // 推进一步（ptyRef 尚为 null），此后 next === state 恒成立，PTY 永远
  // 收不到 resize。必须另存一份 PTY 实际尺寸作镜子。
  assert(
    /ptySizeRef/.test(src),
    '需要 ptySizeRef 记录 PTY 实际生效尺寸，避免与前端 state 比对而漏发 resize'
  );
});

// ---------------------------------------------------------------------------
// 运行
// ---------------------------------------------------------------------------

console.log('libghostty-vt 线格式验证');
console.log('='.repeat(72));

let passed = 0;
const failures: string[] = [];

for (const check of checks) {
  try {
    check.run();
    console.log(`  ✅ ${check.name}`);
    passed++;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.log(`  ❌ ${check.name}\n       ${message}`);
    failures.push(`${check.name}: ${message}`);
  }
}

console.log('-'.repeat(72));
console.log(`结果: ${passed}/${checks.length} 通过`);

if (failures.length > 0) {
  console.log(`\n未通过项:\n${failures.map(f => '  - ' + f).join('\n')}`);
  process.exitCode = 1;
} else {
  console.log(
    '\n✅ 线格式验证通过：前端对位域的理解与 Rust 侧一致。'
  );
}
