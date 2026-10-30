import {
  EMPTY_STYLE,
  WIDE_NARROW,
  WIDE_SPACER_HEAD,
  WIDE_SPACER_TAIL,
  frameCellInto,
  type CellFrame,
  type FrameCell
} from './cellFrame';

/**
 * 把可见视口转成纯文本行，供屏幕阅读器镜像使用。
 *
 * 规则与 Rust 侧取文本的测试助手一致：
 * - 宽字符占两格，尾部 spacer 不产出任何字符，否则「中」会读成「中 」；
 * - 软换行行首的 spacer_head 同样没有字形，一并跳过；
 * - 空格格输出一个空格，行尾空格裁掉（终端右侧的空白没有语义）；
 * - 行与行用 `\n` 连接，空行保留，行数因此恒等于 `frame.rows`。
 *
 * 逐格读取复用同一个 `FrameCell`（`frameCellInto`），不为每格分配对象；
 * 每行只构造一次字符串，裁剪时用 `slice` 而非正则。
 */
export function frameToA11yRows(frame: CellFrame): string[] {
  if (frame.rows <= 0 || frame.cols <= 0) return [];

  const rows: string[] = new Array(frame.rows);
  const cell: FrameCell = {
    codepoint: 0,
    wide: WIDE_NARROW,
    hasText: false,
    cluster: null,
    style: EMPTY_STYLE
  };

  for (let y = 0; y < frame.rows; y++) {
    let row = '';
    for (let x = 0; x < frame.cols; x++) {
      frameCellInto(frame, x, y, cell);
      if (cell.wide === WIDE_SPACER_TAIL || cell.wide === WIDE_SPACER_HEAD) continue;
      row +=
        cell.hasText && (cell.cluster || cell.codepoint > 0)
          ? (cell.cluster ?? String.fromCodePoint(cell.codepoint))
          : ' ';
    }

    // 只裁 ASCII 空格：终端单元格的空白就是单个空格，不能误伤制表符等字符。
    let end = row.length;
    while (end > 0 && row.charCodeAt(end - 1) === 0x20) end--;
    rows[y] = end === row.length ? row : row.slice(0, end);
  }

  return rows;
}

/** 视口纯文本：`frameToA11yRows(frame).join('\n')`。 */
export function frameToA11yText(frame: CellFrame): string {
  return frameToA11yRows(frame).join('\n');
}
