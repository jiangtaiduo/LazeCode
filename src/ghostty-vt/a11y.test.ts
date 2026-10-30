import { describe, expect, it } from 'vitest';
import {
  EMPTY_STYLE,
  WIDE_NARROW,
  WIDE_SPACER_HEAD,
  WIDE_SPACER_TAIL,
  type CellFrame,
  type FrameStyle
} from './cellFrame';
import { frameToA11yRows, frameToA11yText } from './a11y';

interface CellSpec {
  cp?: number;
  wide?: number;
  /** 默认 true；spacer / 空白格显式传 false */
  text?: boolean;
}

const BLANK: CellSpec = { cp: 0, wide: WIDE_NARROW, text: false };

const STYLES: FrameStyle[] = [EMPTY_STYLE];

/** 按 wire 位域把测试格打包进 `words`，与 Rust 侧 pack 布局一致。 */
function packCell(spec: CellSpec, styleIdx = 0): [number, number] {
  const cp = spec.cp ?? 0;
  const wide = spec.wide ?? WIDE_NARROW;
  const hasText = spec.text !== false;
  const lo = (cp & 0x1fffff) | ((wide & 0b11) << 21) | ((styleIdx & 0x1ff) << 23);
  const hi = ((styleIdx >>> 9) & 0x7f) | ((hasText ? 1 : 0) << 7);
  return [lo >>> 0, hi >>> 0];
}

function buildFrame(rows: CellSpec[][], cols = 0): CellFrame {
  const width = cols || Math.max(1, ...rows.map((row) => row.length));
  const words = new Uint32Array(width * rows.length * 2);

  rows.forEach((row, y) => {
    for (let x = 0; x < width; x++) {
      const [lo, hi] = packCell(row[x] ?? BLANK);
      const idx = (y * width + x) * 2;
      words[idx] = lo;
      words[idx + 1] = hi;
    }
  });

  return {
    cols: width,
    rows: rows.length,
    cursorX: 0,
    cursorY: 0,
    defaultFg: null,
    defaultBg: null,
    cursorColor: null,
    cursorVisible: true,
    cursorPendingWrap: false,
    cursorStyle: 1,
    scrollbar: { total: rows.length, offset: 0, len: rows.length },
    styles: STYLES,
    words,
    clusters: [],
    rowFlags: new Uint16Array(0)
  };
}

const char = (ch: string): CellSpec => ({ cp: ch.codePointAt(0) ?? 0 });

describe('frameToA11yText', () => {
  it('宽字符只输出一次，跳过 spacer tail', () => {
    const frame = buildFrame([
      [
        char('中'),
        { cp: 0, wide: WIDE_SPACER_TAIL, text: false },
        char('文'),
        { cp: 0, wide: WIDE_SPACER_TAIL, text: false },
        char('a')
      ]
    ]);
    expect(frameToA11yText(frame)).toBe('中文a');
  });

  it('跳过软换行行首的 spacer head', () => {
    const frame = buildFrame([
      [char('a'), { cp: 0, wide: WIDE_SPACER_HEAD, text: false }],
      [char('中'), { cp: 0, wide: WIDE_SPACER_TAIL, text: false }, char('b')]
    ]);
    expect(frameToA11yText(frame)).toBe('a\n中b');
  });

  it('空白格保留为空格，行尾空格裁掉', () => {
    const frame = buildFrame([
      [char('a'), BLANK, char('b'), BLANK, BLANK],
      [char('c'), BLANK, BLANK, BLANK, BLANK]
    ]);
    expect(frameToA11yText(frame)).toBe('a b\nc');
  });

  it('多行按 \\n 连接，空行与末尾空行都保留', () => {
    const frame = buildFrame([
      [char('x'), BLANK, BLANK],
      [BLANK, BLANK, BLANK],
      [char('y'), char('z'), BLANK],
      [BLANK, BLANK, BLANK]
    ]);
    expect(frameToA11yText(frame)).toBe('x\n\nyz\n');
  });

  it('宽字符位于行尾时光标占位不被读成多余空格', () => {
    const frame = buildFrame([
      [char('a'), char('中'), { cp: 0, wide: WIDE_SPACER_TAIL, text: false }]
    ]);
    expect(frameToA11yText(frame)).toBe('a中');
  });

  it('空帧返回空字符串', () => {
    expect(frameToA11yText(buildFrame([]))).toBe('');
  });

  it('确定性：同一帧多次调用结果相同且不改写帧数据', () => {
    const frame = buildFrame([
      [char('a'), BLANK, char('中'), { cp: 0, wide: WIDE_SPACER_TAIL, text: false }]
    ]);
    const before = frame.words.slice();
    const first = frameToA11yText(frame);
    const second = frameToA11yText(frame);
    expect(second).toBe(first);
    expect(frame.words).toEqual(before);
  });
});

describe('frameToA11yRows', () => {
  it('返回与可见行一一对应的数组', () => {
    const frame = buildFrame([
      [char('a'), BLANK],
      [BLANK, char('b')]
    ]);
    expect(frameToA11yRows(frame)).toEqual(['a', ' b']);
  });
});
