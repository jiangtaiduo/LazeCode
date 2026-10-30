// 终端单元格帧：Rust 与前端之间的唯一数据面
//
// 原生（Tauri）路径下，VT 解析在 Rust 侧完成，前端每帧只取一份紧凑缓冲：
// 120x40 约 38 KB / 帧，一次 memcpy，无 JSON、无逐格跨界调用。
//
// 线格式（全部小端），与 `src-tauri/src/vt.rs::encode_snapshot` 严格对应：
//
//   头部 56 字节
//     u32 magic        = 0x54564E31 ("VTN1")
//     u16 cols
//     u16 rows
//     u16 cursor_x
//     u16 cursor_y
//     u16 style_count
//     u16 flags         bit0 光标可见, bit1 待换行
//     i32 default_fg    (-1 = 未设置)
//     i32 default_bg
//     i32 cursor_color
//     u32 cursor_style  0=bar 1=block 2=underline 3=block_hollow
//     u64 scrollbar_total
//     u64 scrollbar_offset
//     u64 scrollbar_len
//   样式表 style_count × 12 字节
//     i32 fg         (-1 表示未设置)
//     i32 bg         (-1 表示未设置)
//     u32 flags
//   单元格 cols*rows × 8 字节（u64）
//     bit 0..20  codepoint
//     bit 21..22 wide      0=NARROW 1=WIDE 2=SPACER_TAIL 3=SPACER_HEAD
//     bit 23..38 style_idx
//     bit 39     has_text
//     bit 40..63 cluster_index + 1（0 = 该格无字素簇）
//   尾部（可选：旧帧没有，解码器按 magic 判断）
//     u32 magic = 0x56544631 ("VTF1")
//     u16 cluster_count
//     u16 row_flags_count
//     row_flags_count × u16（bit0 = OSC 133 prompt 行, bit1 = 续行）
//     cluster_count × (u32 byte_len + UTF-8 bytes)
//
//   字素簇只在「一格有多个码点」时才有条目（组合符、ZWJ emoji 序列）：
//   前端必须整簇绘制，否则 `e` + U+0301 会被画成两个字形。

export const WIRE_MAGIC = 0x54564e31;
export const WIRE_HEADER_LEN = 56;
export const WIRE_STYLE_LEN = 12;
export const WIRE_CELL_LEN = 8;
/** 全量帧尾部 magic（"VTF1"）。 */
export const WIRE_FOOTER_MAGIC = 0x56544631;
/** 增量帧尾部 magic（"VDF1"）。 */
export const WIRE_DIFF_FOOTER_MAGIC = 0x56444631;
/** 尾部固定头：u32 magic + u16 count + u16 count/reserved。 */
export const WIRE_FOOTER_LEN = 8;

/**
 * 增量帧 magic（"VTN3"）。与全量帧同族但语义不同：只带脏行。
 *
 * 为什么需要增量：全量帧每帧恒定 `cols*rows*8 + 56` 字节，与屏幕是否
 * 真的变了无关。终端多数帧只有一两行在动（光标闪烁、逐字回显），
 * 全量帧把整屏重编码、重拷贝、重解码一遍。增量帧只送引擎标记为脏的
 * 行，稳态下从 KB 级降到百字节级。
 *
 * VTN3 = VTN2 修掉了两个前端根本解不开的坑：cells 起点 4 字节对齐
 * （见 `WIRE_DIFF_ROW_HEADER_LEN`），以及取消「每行再抄一遍样式片段」
 * 的冗余（前端只读帧级表，逐行片段纯属线上浪费）。
 */
export const WIRE_DIFF_MAGIC = 0x54564e33;

/**
 * 增量帧头部 60 字节（按序，全部小端）：
 *   0..3    u32 magic
 *   4..5    u16 cols
 *   6..7    u16 rows
 *   8..9    u16 flags        bit0 光标可见, bit1 待换行
 *   10..13  i32 default_fg   (-1 = 未设置)
 *   14..17  i32 default_bg
 *   18..21  i32 cursor_color
 *   22..25  u32 cursor_style
 *   26..27  u16 cursor_x     （视口坐标）
 *   28..29  u16 cursor_y
 *   30..31  u16 style_count  （全帧共享的样式表条数）
 *   32..33  u16 row_count    （脏行数）
 *   34..35  i16 scroll_delta （相对上一帧的滚动位移）
 *   36..43  u64 scrollbar_total
 *   44..51  u64 scrollbar_offset
 *   52..59  u64 scrollbar_len
 * 之后是 `style_count × 12B` 的**帧级**样式表，再是逐条脏行：
 *   行头 8B + `cols × 8B` cells（style_idx 是帧级表的全局下标）。
 * 最后是帧尾（可选，按 magic 判断）：
 *   u32 0x56444631 ("VDF1") + u16 cluster_count + u16 reserved +
 *   cluster_count × (u32 byte_len + UTF-8)。
 */
export const WIRE_DIFF_HEADER_LEN = 60;

/**
 * 每条脏行的行头 8 字节：`u16 row + u16 reserved + u16 row_flags + u16 pad`。
 *
 * 为什么必须是 8（VTN2 是 6）：cells 起点要 4 字节对齐，前端才能零拷贝
 * 建 `Uint32Array` 视图 —— `60 + 12×styles + 8×rows` 恒 `≡ 0 (mod 4)`。
 * 6 字节行头让每个 cells 起点 `≡ 2 (mod 4)`，建视图必抛 RangeError，
 * 增量通道当时是 100% 失效的（全量帧回退掩盖了它）。
 * `row_flags`（offset +4）承载 OSC 133 语义提示：bit0 = prompt 行，
 * bit1 = prompt 续行；`reserved`（+2）与 `pad`（+6）保持为 0。
 */
export const WIRE_DIFF_ROW_HEADER_LEN = 8;

/** `CellView::wide` 的取值 */
export const WIDE_NARROW = 0;
export const WIDE_WIDE = 1;
export const WIDE_SPACER_TAIL = 2;
export const WIDE_SPACER_HEAD = 3;

/**
 * 行标记位（全量帧尾部与增量帧行头共用）。
 * 来源是 OSC 133 语义提示：shell 用它标出「哪些行是提示符」。
 */
export const RowFlag = {
  /** prompt 主行（OSC 133 `A`） */
  Prompt: 1 << 0,
  /** prompt 续行（折行后的部分） */
  PromptContinuation: 1 << 1
} as const;

/**
 * 样式标志位。用常量对象而非 `const enum`：Node 的
 * `--experimental-strip-types` 会拒绝枚举语法，而验证脚本依赖它零构建运行。
 */
export const StyleFlag = {
  Bold: 1 << 0,
  Italic: 1 << 1,
  Faint: 1 << 2,
  Inverse: 1 << 3,
  Invisible: 1 << 4,
  Strikethrough: 1 << 5
} as const;

export interface FrameStyle {
  /** 0xRRGGBB，null 表示使用主题默认 */
  fg: number | null;
  bg: number | null;
  bold: boolean;
  italic: boolean;
  faint: boolean;
  inverse: boolean;
  invisible: boolean;
  strikethrough: boolean;
}

/** `GhosttyRenderStateCursorVisualStyle` */
export const CURSOR_STYLE = {
  BAR: 0,
  BLOCK: 1,
  UNDERLINE: 2,
  BLOCK_HOLLOW: 3
} as const;

/** 视口在滚动历史中的位置，驱动滚动条绘制 */
export interface FrameScrollbar {
  /** 总可滚动行数（含可见行） */
  total: number;
  /** 当前视口顶部在滚动空间中的偏移 */
  offset: number;
  /** 视口行数 */
  len: number;
}

export interface CellFrame {
  cols: number;
  rows: number;
  cursorX: number;
  cursorY: number;
  /** 终端默认前景色（0xRRGGBB），null 表示未设置，渲染器回退主题色 */
  defaultFg: number | null;
  defaultBg: number | null;
  cursorColor: number | null;
  /** 光标是否可见（DECTCEM） */
  cursorVisible: boolean;
  /** 光标是否处于待换行状态（位于行尾但未换行） */
  cursorPendingWrap: boolean;
  cursorStyle: number;
  scrollbar: FrameScrollbar;
  styles: FrameStyle[];
  /** 每格 2 个 u32：低字在前（小端 u64 拆分），避免 BigInt */
  words: Uint32Array;
  /** 去重后的完整字素簇表；`FrameCell.cluster` 从这里取值 */
  clusters: string[];
  /** 每行一个标记（`RowFlag` 位域）；长度等于 `rows` */
  rowFlags: Uint16Array;
}

/** 一格的解码结果：码点、宽度与已解析的样式 */
export interface FrameCell {
  codepoint: number;
  wide: number;
  hasText: boolean;
  /**
   * 完整字素簇（含基码点）；`null` = 该格没有簇（单码点或空白）。
   *
   * 声明为可选是为了兼容既有 `FrameCell` 字面量（渲染侧维护的
   * `GhosttyTerminal.tsx` 里有两个共享槽位），`frameCellInto` 每次都会
   * 写入该字段 —— 读到 `undefined` 与 `null` 等价。
   */
  cluster?: string | null;
  style: FrameStyle;
}

const EMPTY_STYLE: FrameStyle = {
  fg: null,
  bg: null,
  bold: false,
  italic: false,
  faint: false,
  inverse: false,
  invisible: false,
  strikethrough: false
};

const EMPTY_FRAME: CellFrame = {
  cols: 0,
  rows: 0,
  cursorX: 0,
  cursorY: 0,
  defaultFg: null,
  defaultBg: null,
  cursorColor: null,
  cursorVisible: true,
  cursorPendingWrap: false,
  cursorStyle: CURSOR_STYLE.BLOCK,
  scrollbar: { total: 0, offset: 0, len: 0 },
  styles: [],
  words: new Uint32Array(0),
  clusters: [],
  rowFlags: new Uint16Array(0)
};

/**
 * 读帧尾的字素簇表（`count × (u32 byte_len + UTF-8)`）。
 *
 * `off` 指向第一条簇的长度字段。声明长度越界一律抛错：静默截断会让
 * 前端拿到半条簇，画出来的字形是错的且无从排查。
 */
function readFooterClusters(
  view: DataView,
  buffer: ArrayBuffer,
  off: number,
  count: number,
  where: string
): string[] {
  const clusters: string[] = new Array(count);
  const decoder = count > 0 ? new TextDecoder() : null;
  for (let i = 0; i < count; i++) {
    if (off + 4 > buffer.byteLength) {
      throw new Error(`${where} 簇 ${i} 长度字段越界`);
    }
    const len = view.getUint32(off, true);
    off += 4;
    if (off + len > buffer.byteLength) {
      throw new Error(
        `${where} 簇 ${i} 数据越界: 需要 ${off + len} 字节，实际 ${buffer.byteLength}`
      );
    }
    clusters[i] = decoder!.decode(new Uint8Array(buffer, off, len));
    off += len;
  }
  return clusters;
}

/**
 * 解码 Rust 侧的快照缓冲。
 *
 * 刻意避开 BigInt：`BigInt` 位运算在 V8 里比 32 位整数慢一个数量级，
 * 而 4800 格 × 每帧一次会把这点开销放大成主要成本。
 * 改用两个 u32 视图读取，位域全部落在 32 位内。
 */
export function decodeCellFrame(buffer: ArrayBuffer): CellFrame {
  if (buffer.byteLength < WIRE_HEADER_LEN) {
    throw new Error(`快照缓冲过短: ${buffer.byteLength} 字节`);
  }

  const view = new DataView(buffer);
  const magic = view.getUint32(0, true);
  if (magic !== WIRE_MAGIC) {
    throw new Error(
      `快照 magic 不匹配: 期望 0x${WIRE_MAGIC.toString(16)}，实际 0x${magic.toString(16)}`
    );
  }

  const cols = view.getUint16(4, true);
  const rows = view.getUint16(6, true);
  const cursorX = view.getUint16(8, true);
  const cursorY = view.getUint16(10, true);
  const styleCount = view.getUint16(12, true);
  const flags = view.getUint16(14, true);

  // 默认色随帧送达，使帧自足（原生路径下前端没有终端句柄可取）
  const readColor = (offset: number): number | null => {
    const v = view.getInt32(offset, true);
    return v < 0 ? null : v;
  };
  const defaultFg = readColor(16);
  const defaultBg = readColor(20);
  const cursorColor = readColor(24);
  const cursorStyle = view.getUint32(28, true);
  // 滚动条是 u64，但每帧只读 3 个，BigInt 开销可忽略（热点在逐格读取）
  const scrollbar = {
    total: Number(view.getBigUint64(32, true)),
    offset: Number(view.getBigUint64(40, true)),
    len: Number(view.getBigUint64(48, true))
  };

  const styles: FrameStyle[] = new Array(styleCount);
  let off = WIRE_HEADER_LEN;
  for (let i = 0; i < styleCount; i++) {
    const fg = view.getInt32(off, true);
    const bg = view.getInt32(off + 4, true);
    const flags = view.getUint32(off + 8, true);
    styles[i] = {
      fg: fg < 0 ? null : fg,
      bg: bg < 0 ? null : bg,
      bold: (flags & StyleFlag.Bold) !== 0,
      italic: (flags & StyleFlag.Italic) !== 0,
      faint: (flags & StyleFlag.Faint) !== 0,
      inverse: (flags & StyleFlag.Inverse) !== 0,
      invisible: (flags & StyleFlag.Invisible) !== 0,
      strikethrough: (flags & StyleFlag.Strikethrough) !== 0
    };
    off += WIRE_STYLE_LEN;
  }

  const cellCount = cols * rows;
  const expected = off + cellCount * WIRE_CELL_LEN;
  if (buffer.byteLength < expected) {
    throw new Error(
      `快照缓冲不完整: 需要 ${expected} 字节，实际 ${buffer.byteLength}`
    );
  }

  // 对齐到 4 字节后直接建视图，避免逐格 getUint32（4800 次调用）
  const words = new Uint32Array(buffer, off, cellCount * 2);

  // 帧尾可选：旧帧到 cells 结束就没了，此时簇表与行标记都是空。
  let clusters: string[] = [];
  let rowFlags = new Uint16Array(0);
  if (
    buffer.byteLength >= expected + WIRE_FOOTER_LEN &&
    view.getUint32(expected, true) === WIRE_FOOTER_MAGIC
  ) {
    const clusterCount = view.getUint16(expected + 4, true);
    const flagCount = view.getUint16(expected + 6, true);
    let tail = expected + WIRE_FOOTER_LEN;
    if (tail + flagCount * 2 > buffer.byteLength) {
      throw new Error(
        `快照帧尾行标记越界: 需要 ${tail + flagCount * 2} 字节，实际 ${buffer.byteLength}`
      );
    }
    rowFlags = new Uint16Array(flagCount);
    for (let i = 0; i < flagCount; i++) {
      rowFlags[i] = view.getUint16(tail, true);
      tail += 2;
    }
    clusters = readFooterClusters(view, buffer, tail, clusterCount, '快照帧尾');
  }

  return {
    cols,
    rows,
    cursorX,
    cursorY,
    defaultFg,
    defaultBg,
    cursorColor,
    cursorVisible: (flags & 1) !== 0,
    cursorPendingWrap: (flags & 2) !== 0,
    cursorStyle,
    scrollbar,
    styles,
    words,
    clusters,
    rowFlags
  };
}

/** 字素簇下标在 `hi`（u64 高 32 位）里的位置：bit 40..63 → hi 位 8..31。 */
function clusterOf(frame: CellFrame, hi: number): string | null {
  const idx = hi >>> 8;
  return idx === 0 ? null : (frame.clusters[idx - 1] ?? null);
}

/** 读取一格的解码结果。`words` 中每格占两个 u32。 */
export function frameCell(frame: CellFrame, x: number, y: number): FrameCell {
  const idx = (y * frame.cols + x) * 2;
  const lo = frame.words[idx];
  const hi = frame.words[idx + 1];

  if (lo === undefined || hi === undefined) {
    return {
      codepoint: 0,
      wide: WIDE_NARROW,
      hasText: false,
      cluster: null,
      style: EMPTY_STYLE
    };
  }

  // u64 位域 → 拆分到 (lo, hi) 后各段位置：
  //   codepoint  bit 0..20   → lo  位 0..20
  //   wide       bit 21..22  → lo  位 21..22
  //   style_idx  bit 23..38  → lo  位 23..31 (低 9 位) + hi 位 0..6 (高 7 位)
  //   has_text   bit 39      → hi  位 7
  //   cluster    bit 40..63  → hi  位 8..31（存「下标 + 1」）
  const codepoint = lo & 0x1fffff;
  const wide = (lo >>> 21) & 0b11;
  const styleIdx = ((lo >>> 23) & 0x1ff) | ((hi & 0x7f) << 9);
  const hasText = ((hi >>> 7) & 1) !== 0;

  return {
    codepoint,
    wide,
    hasText,
    cluster: clusterOf(frame, hi),
    style: frame.styles[styleIdx] ?? EMPTY_STYLE
  };
}

/**
 * 与 `frameCell` 相同，但写入调用方提供的对象而非新分配。
 *
 * 供绘制热循环使用：`frameCell` 每帧要为 4800 格各分配一个对象，
 * 60fps 下每秒 ~29 万个短命对象，GC 全落在帧预算上。调用方传入
 * 模块级共享槽位即可稳态零分配。
 */
export function frameCellInto(frame: CellFrame, x: number, y: number, out: FrameCell): FrameCell {
  const idx = (y * frame.cols + x) * 2;
  const lo = frame.words[idx];
  const hi = frame.words[idx + 1];

  if (lo === undefined || hi === undefined) {
    out.codepoint = 0;
    out.wide = WIDE_NARROW;
    out.hasText = false;
    out.cluster = null;
    out.style = EMPTY_STYLE;
    return out;
  }

  out.codepoint = lo & 0x1fffff;
  out.wide = (lo >>> 21) & 0b11;
  out.hasText = ((hi >>> 7) & 1) !== 0;
  out.cluster = clusterOf(frame, hi);
  out.style = frame.styles[((lo >>> 23) & 0x1ff) | ((hi & 0x7f) << 9)] ?? EMPTY_STYLE;
  return out;
}

export { EMPTY_FRAME, EMPTY_STYLE };

// ---------------------------------------------------------------------------
// 增量帧（VTN3）
// ---------------------------------------------------------------------------

/** 一条脏行的索引：行号 + cells 在帧缓冲中的字节偏移。 */
export interface DiffRow {
  y: number;
  /** 该行 cells 在帧缓冲中的字节偏移（已 4 字节对齐，可直接建 Uint32Array） */
  cellsOffset: number;
}

/**
 * 增量帧的索引视图。
 *
 * 只解码头部与行索引，**不拷贝 cells** —— cells 仍留在原缓冲里，
 * 由 `CellDiffCache` 在合并时按需取用。这样每帧的解码成本是
 * O(row_count) 而非 O(cols*rows)。
 */
export interface CellDiff {
  cols: number;
  /** 网格行数（不等于 `rows.length`：后者是本帧的脏行数） */
  gridRows: number;
  cursorX: number;
  cursorY: number;
  cursorVisible: boolean;
  cursorPendingWrap: boolean;
  defaultFg: number | null;
  defaultBg: number | null;
  cursorColor: number | null;
  cursorStyle: number;
  /** 视口相对上一帧的滚动位移（前端据此挪本地选区端点） */
  scrollDelta: number;
  scrollbar: FrameScrollbar;
  /** 帧级共享样式表；cells 里的 style_idx 是它的全局下标 */
  styles: FrameStyle[];
  /** 帧级字素簇表；cells 里的 cluster 位是「下标 + 1」 */
  clusters: string[];
  /** 每行的语义提示标记（`RowFlag`）；非脏行在本帧里为 0 */
  rowFlags: Uint16Array;
  rows: DiffRow[];
  /** 源缓冲。调用方持有它，索引视图只在缓冲存活期间有效。 */
  buffer: ArrayBuffer;
}

/**
 * 解码增量帧的头部与行索引。
 *
 * 抛出：magic 不匹配、缓冲短于声明、行索引越界。
 * 这些都属于协议错位，静默容忍会让画面停在旧帧且无从排查。
 */
export function decodeCellDiff(buffer: ArrayBuffer): CellDiff {
  if (buffer.byteLength < WIRE_DIFF_HEADER_LEN) {
    throw new Error(`增量帧缓冲过短: ${buffer.byteLength} 字节`);
  }
  const view = new DataView(buffer);
  const magic = view.getUint32(0, true);
  if (magic !== WIRE_DIFF_MAGIC) {
    throw new Error(
      `增量帧 magic 不匹配: 期望 0x${WIRE_DIFF_MAGIC.toString(16)}，实际 0x${magic.toString(16)}`
    );
  }

  const cols = view.getUint16(4, true);
  const rows = view.getUint16(6, true);
  const flags = view.getUint16(8, true);
  const readColor = (offset: number): number | null => {
    const v = view.getInt32(offset, true);
    return v < 0 ? null : v;
  };
  const defaultFg = readColor(10);
  const defaultBg = readColor(14);
  const cursorColor = readColor(18);
  const cursorStyle = view.getUint32(22, true);
  const cursorX = view.getUint16(26, true);
  const cursorY = view.getUint16(28, true);
  const styleCount = view.getUint16(30, true);
  const rowCount = view.getUint16(32, true);
  const scrollDelta = view.getInt16(34, true);
  // 滚动条是 u64，每帧只读 3 个，BigInt 开销可忽略
  const scrollbar = {
    total: Number(view.getBigUint64(36, true)),
    offset: Number(view.getBigUint64(44, true)),
    len: Number(view.getBigUint64(52, true))
  };

  const styles: FrameStyle[] = new Array(styleCount);
  let off = WIRE_DIFF_HEADER_LEN;
  for (let i = 0; i < styleCount; i++) {
    const fg = view.getInt32(off, true);
    const bg = view.getInt32(off + 4, true);
    const styleFlags = view.getUint32(off + 8, true);
    styles[i] = {
      fg: fg < 0 ? null : fg,
      bg: bg < 0 ? null : bg,
      bold: (styleFlags & StyleFlag.Bold) !== 0,
      italic: (styleFlags & StyleFlag.Italic) !== 0,
      faint: (styleFlags & StyleFlag.Faint) !== 0,
      inverse: (styleFlags & StyleFlag.Inverse) !== 0,
      invisible: (styleFlags & StyleFlag.Invisible) !== 0,
      strikethrough: (styleFlags & StyleFlag.Strikethrough) !== 0
    };
    off += WIRE_STYLE_LEN;
  }

  const rowsIndex: DiffRow[] = [];
  const rowFlags = new Uint16Array(rows);
  for (let i = 0; i < rowCount; i++) {
    if (off + WIRE_DIFF_ROW_HEADER_LEN > buffer.byteLength) {
      throw new Error(`增量帧行 ${i} 头部越界`);
    }
    const y = view.getUint16(off, true);
    // 行头 8B：row + reserved + row_flags + pad。cells 紧随其后，
    // 起点由 60 + 12×styles + 8×行号 保证 4 字节对齐。
    // row_flags 的 bit0/bit1 是 OSC 133 语义提示。
    const rowFlag = view.getUint16(off + 4, true);
    off += WIRE_DIFF_ROW_HEADER_LEN;
    const need = off + cols * WIRE_CELL_LEN;
    if (need > buffer.byteLength) {
      throw new Error(`增量帧行 ${i} cells 越界: 需要 ${need}，实际 ${buffer.byteLength}`);
    }
    if (y < rows) rowFlags[y] = rowFlag;
    rowsIndex.push({ y, cellsOffset: off });
    off = need;
  }

  // 帧尾（可选）：u32 magic + u16 cluster_count + u16 reserved + 簇表。
  let clusters: string[] = [];
  if (
    buffer.byteLength >= off + WIRE_FOOTER_LEN &&
    view.getUint32(off, true) === WIRE_DIFF_FOOTER_MAGIC
  ) {
    const clusterCount = view.getUint16(off + 4, true);
    clusters = readFooterClusters(
      view,
      buffer,
      off + WIRE_FOOTER_LEN,
      clusterCount,
      '增量帧尾'
    );
  }

  return {
    cols,
    gridRows: rows,
    cursorX,
    cursorY,
    cursorVisible: (flags & 1) !== 0,
    cursorPendingWrap: (flags & 2) !== 0,
    defaultFg,
    defaultBg,
    cursorColor,
    cursorStyle,
    scrollDelta,
    scrollbar,
    styles,
    clusters,
    rowFlags,
    rows: rowsIndex,
    buffer
  };
}

/**
 * 帧缓存：持有当前完整屏幕，增量帧就地覆盖脏行。
 *
 * 为什么要有它：增量帧只带脏行，绘制端需要「整屏可见」的视图。
 * 缓存就是那块屏幕 —— 全量帧重建它，增量帧逐行覆盖它。
 *
 * `words` 与 `styles` 的容量按需增长（resize 时）；稳态下 resize 不变，
 * 每帧只做 Uint32Array.set，无分配。
 */
export class CellDiffCache {
  cols = 0;
  rows = 0;
  words = new Uint32Array(0);
  styles: FrameStyle[] = [];
  /** 字素簇表：与 styles 同构，全量帧重建、增量帧按值并入。 */
  clusters: string[] = [];
  /** 每行语义提示标记（`RowFlag`）；增量帧只覆盖脏行。 */
  rowFlags = new Uint16Array(0);
  /** `w` 格是否被增量帧写过；全量帧重建时为 false。 */
  private filled = false;
  /**
   * 最近一次拿到的有效默认色。
   *
   * 增量帧的默认色字段在「未设置」时是 -1，直接采用会把主题色抹成回退色，
   * 因此缓存里留一份最近的有效值。
   */
  lastDefaultFg: number | null = null;
  lastDefaultBg: number | null = null;

  /** 样式值键 → 缓存样式表下标。`reset` 时按全量帧重建。 */
  private readonly styleIndex = new Map<string, number>();
  /** 「帧内下标 → 缓存下标」的映射缓冲，跨帧复用（避免每帧分配）。 */
  private remap = new Uint16Array(64);
  /** 簇值 → 缓存簇表下标；与样式表同构。 */
  private readonly clusterIndex = new Map<string, number>();
  /** 簇的「帧内下标 → 缓存下标」映射缓冲，跨帧复用。 */
  private clusterRemap = new Uint16Array(64);
  /** 16 位 style_idx 空间用尽：上层应立即整屏重建（见 `internStyle`）。 */
  private overflowed = false;

  /**
   * 是否需要立刻走一次全量重建。
   *
   * 两个来源：还没有任何一帧（首帧 / 解码失败后失效），或样式表把
   * 16 位下标空间用尽（增量合并已降级，不能再往上叠）。
   */
  get needsFullRebuild(): boolean {
    return !this.filled || this.overflowed;
  }

  /**
   * 导出为 `CellFrame` 视图供 paint 消费。
   *
   * 复用缓存内的 `words` / `styles`（零拷贝）—— paint 只读它们。
   * `cols`/`rows` 与光标等标量由调用方按最近一帧覆写。
   */
  frameView(cursor: {
    cursorX: number;
    cursorY: number;
    cursorVisible: boolean;
    cursorPendingWrap: boolean;
    defaultFg: number | null;
    defaultBg: number | null;
    cursorColor: number | null;
    cursorStyle: number;
    scrollbar: FrameScrollbar;
  }): CellFrame {
    return {
      cols: this.cols,
      rows: this.rows,
      ...cursor,
      styles: this.styles,
      words: this.words,
      clusters: this.clusters,
      rowFlags: this.rowFlags
    };
  }

  /** 全量帧重建缓存。尺寸变化时重分配缓冲。 */
  reset(frame: CellFrame): void {
    if (this.cols !== frame.cols || this.rows !== frame.rows || this.words.length === 0) {
      this.cols = frame.cols;
      this.rows = frame.rows;
      this.words = new Uint32Array(frame.cols * frame.rows * 2);
      this.rowFlags = new Uint16Array(frame.rows);
    }
    this.words.set(frame.words);
    // 复制而非引用：增量帧合并会往表尾追加新样式，不能改到全量帧对象
    // 持有的那张表（lastFrame 还在引用它）。表只有几十项，成本可忽略。
    this.styles = frame.styles.slice();
    this.styleIndex.clear();
    for (let i = 0; i < this.styles.length; i++) {
      this.styleIndex.set(styleKeyOf(this.styles[i]), i);
    }
    // 簇表同理：增量帧按值并入时也会追加。
    this.clusters = frame.clusters.slice();
    this.clusterIndex.clear();
    for (let i = 0; i < this.clusters.length; i++) {
      this.clusterIndex.set(this.clusters[i], i);
    }
    // 行标记按行整体替换；旧帧没有帧尾时 `rowFlags` 为空，全部清零。
    this.rowFlags.fill(0);
    this.rowFlags.set(frame.rowFlags.subarray(0, Math.min(frame.rowFlags.length, this.rows)));
    if (frame.defaultFg !== null) this.lastDefaultFg = frame.defaultFg;
    if (frame.defaultBg !== null) this.lastDefaultBg = frame.defaultBg;
    this.filled = true;
    this.overflowed = false;
  }

  /**
   * 把缓存标为失效：下一帧必须走全量重建。
   *
   * 用于解码失败 / IPC 超时 —— 此时缓存内容可能与引擎真实状态脱节，
   * 继续在它上面做增量合并会把错的状态一直传下去。
   */
  invalidate(): void {
    this.filled = false;
  }

  /**
   * 合并一帧增量帧：先按帧级样式表建「帧内下标 → 缓存下标」的映射，
   * 再逐行就地覆盖 cells。
   *
   * 为什么必须按整张帧表建映射：cells 里的 `style_idx` 是**帧级表的全局
   * 下标**，不是行内序号。旧实现按 `[0, styleCount)` 取行内片段再相加，
   * 只要同一帧有第二条脏行引入了新样式，第二行起的颜色就会错位或丢失
   * （实测：第 2 行 fg 解析为 undefined，颜色回退默认值）。
   *
   * 行号越界（resize 与读线程并发的瞬态）时忽略该行：旧网格的脏标记
   * 对上不齐当前尺寸，写进去反而污染缓存。
   */
  applyDiff(diff: CellDiff): void {
    // 尺寸闸：增量帧的 cols/gridRows 必须与缓存完全一致。不匹配说明
    // 引擎 resize 与这次读取并发（两个 IPC 无顺序保证），此时脏行的
    // 行号是按新网格算的，写进旧网格就是整屏错位。
    //
    // 旧实现只判了 `row.y >= this.rows`（行越界）并用 Math.min 取较小
    // 列数：diff.cols 大时右侧内容被静默丢弃，小时右侧残留上一帧的
    // 陈旧字符 —— 都是自愈前可见的数据错乱。这里直接判失效，让上层
    // 走全量重建（GhosttyTerminal 的 refresh 读 needsFullRebuild）。
    if (diff.cols !== this.cols || diff.gridRows !== this.rows) {
      this.invalidate();
      return;
    }
    const styleCount = diff.styles.length;
    if (this.remap.length < styleCount) {
      this.remap = new Uint16Array(Math.max(styleCount, 64));
    }
    for (let i = 0; i < styleCount; i++) {
      this.remap[i] = this.internStyle(diff.styles[i]);
    }
    // 簇与样式同构：cells 里的簇下标是**帧级簇表**的全局下标，
    // 必须逐帧重映射到缓存簇表，否则第二帧起会指向错的簇。
    const clusterCount = diff.clusters.length;
    if (this.clusterRemap.length < clusterCount) {
      this.clusterRemap = new Uint16Array(Math.max(clusterCount, 64));
    }
    for (let i = 0; i < clusterCount; i++) {
      this.clusterRemap[i] = this.internCluster(diff.clusters[i]);
    }

    const cols = Math.min(this.cols, diff.cols);
    for (const row of diff.rows) {
      if (row.y >= this.rows) continue;
      // 行标记随行头到达：只覆盖本帧脏行，未脏的行保留旧值。
      this.rowFlags[row.y] = diff.rowFlags[row.y] ?? 0;
      const base = row.y * this.cols * 2;
      // cells 起点 4 字节对齐（VTN3 的行头 8B 保证），零拷贝建视图。
      const cells = new Uint32Array(diff.buffer, row.cellsOffset, cols * 2);
      for (let x = 0; x < cols; x++) {
        const lo = cells[x * 2];
        const hi = cells[x * 2 + 1];
        const styleIdx = ((lo >>> 23) & 0x1ff) | ((hi & 0x7f) << 9);
        const newIdx = this.remap[styleIdx] ?? 0;
        // 簇下标在 hi 位 8..31（u64 的 bit 40..63），0 表示无簇。
        // 线格式存「下标 + 1」，缓存表是 0 基，重映射后要加回去。
        const clusterIdx = hi >>> 8;
        const newCluster = clusterIdx === 0 ? 0 : (this.clusterRemap[clusterIdx - 1] ?? 0) + 1;
        const out = base + x * 2;
        this.words[out] = (lo & ~(0x1ff << 23)) | ((newIdx & 0x1ff) << 23);
        // has_text（hi 位 7）保留；style 高 7 位与簇下标重写。
        this.words[out + 1] =
          (hi & 0x80) | ((newIdx >>> 9) & 0x7f) | ((newCluster << 8) >>> 0);
      }
    }
  }

  /**
   * 把一条帧级样式并入缓存样式表，返回它在缓存表里的下标。
   *
   * 按「值」去重而不是按区间：单条样式 O(1) 命中，且表不会随时间无限
   * 增长（旧实现按区间扫描 + 追加，最坏 O(表长 × 片段长)，且可能把
   * 16 位 style_idx 空间耗尽）。
   */
  private internStyle(style: FrameStyle): number {
    const key = styleKeyOf(style);
    const hit = this.styleIndex.get(key);
    if (hit !== undefined) return hit;
    if (this.styles.length >= 0xffff) {
      // 16 位下标空间用尽：本帧起颜色会不准，必须让上层立刻整屏重建
      // （reset 会把表换成引擎给的干净快照）。这种量级（6.5 万种不同
      // 样式）只可能出现在长跑的高彩输出里，属可自愈的降级。
      this.overflowed = true;
      return 0;
    }
    const idx = this.styles.length;
    this.styles.push(style);
    this.styleIndex.set(key, idx);
    return idx;
  }

  /**
   * 把一条帧级字素簇并入缓存簇表，返回它在缓存表里的下标。
   *
   * 与 `internStyle` 同样的取舍：按值去重、O(1) 命中、表不随时间增长。
   * 表满（16 位下标空间）时标记降级，让上层整屏重建 —— 簇下标在
   * 缓存里是 16 位（`clusterRemap` 与 style 同宽）。
   */
  private internCluster(cluster: string): number {
    const hit = this.clusterIndex.get(cluster);
    if (hit !== undefined) return hit;
    if (this.clusters.length >= 0xffff) {
      this.overflowed = true;
      return 0;
    }
    const idx = this.clusters.length;
    this.clusters.push(cluster);
    this.clusterIndex.set(cluster, idx);
    return idx;
  }
}

/** 样式的值键：fg/bg 用 -1 表示未设置，flags 打包成 6 位。 */
function styleKeyOf(s: FrameStyle): string {
  const flags =
    (s.bold ? 1 : 0) |
    (s.italic ? 2 : 0) |
    (s.faint ? 4 : 0) |
    (s.inverse ? 8 : 0) |
    (s.invisible ? 16 : 0) |
    (s.strikethrough ? 32 : 0);
  return `${s.fg ?? -1},${s.bg ?? -1},${flags}`;
}
