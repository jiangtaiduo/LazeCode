//! 原生 libghostty-vt 终端
//!
//! 与前端 wasm 版本的分工：
//!   - **本模块**：PTY 输出在这里解析（原生 SIMD，无沙箱边界）
//!   - 前端只拿一份紧凑的单元格缓冲用于绘制
//!
//! 关键：原生 ABI 与 wasm32 不同（指针 8 字节 vs 4 字节），
//! `GhosttyGridRef` 在原生下是 24 字节而非 12 字节。所有布局均以
//! `ghostty_type_json()` 的原生输出为准，不硬编码 wasm 时期的偏移。
//!
//! 结构体布局来源（原生 arm64 实测）：
//! ```text
//! GhosttyPoint     size=24 align=8   tag@0 value@8
//! GhosttyGridRef   size=24 align=8   size(u64)@0 node@8 x@16 y@18
//! GhosttyStyle     size=72 align=8   size@0 fg@8 bg@24 bold@56 .. underline(i32)@64
//! GhosttyStyleColor size=16 align=8  tag@0 value@8
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;

// ---------------------------------------------------------------------------
// FFI
// ---------------------------------------------------------------------------

/// `GhosttyPointTag::ACTIVE` —— 活动屏幕坐标（渲染应使用 VIEWPORT=1）
pub const POINT_TAG_VIEWPORT: u32 = 1;

/// `GhosttyTerminalData` 中需要的标量键
const DATA_COLS: u32 = 1;
const DATA_ROWS: u32 = 2;
const DATA_CURSOR_X: u32 = 3;
const DATA_CURSOR_Y: u32 = 4;
/// 待换行标志（DEC 自动折行挂起）。全量帧从 `read_flag(5)` 取，
/// 增量帧在帧头同义字段里透传。
const DATA_CURSOR_PENDING_WRAP: u32 = 5;
/// 默认前景 / 背景 / 光标色。渲染器需要它们才能回退未覆写的单元格，
/// 必须随帧一起送达 —— 否则原生路径下前端拿不到终端句柄。
const DATA_SCROLLBAR: u32 = 9;
/// `GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN`：0 = 主屏，1 = 备用屏（alt screen）
const DATA_ACTIVE_SCREEN: u32 = 6;
/// `GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING`：任一鼠标跟踪模式是否开启
const DATA_MOUSE_TRACKING: u32 = 11;
/// `GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS`：回滚历史行数
const DATA_SCROLLBACK_ROWS: u32 = 15;
/// `GHOSTTY_TERMINAL_DATA_MODE`：查询某个 ANSI/DEC 模式的当前值
/// （输入输出类型 `GhosttyTerminalModeConfig`，使用前必须填 mode）
const DATA_MODE: u32 = 37;
const DATA_COLOR_FOREGROUND: u32 = 18;

/// `GhosttyRenderStateData` —— 渲染状态是渲染器的正确数据源：
/// 光标视觉形状、可见性、闪烁、视口坐标都在这里，且与终端状态同步。
const RS_COLOR_CURSOR: u32 = 7;
const RS_COLOR_CURSOR_HAS_VALUE: u32 = 8;
const RS_CURSOR_VISUAL_STYLE: u32 = 10;
const RS_CURSOR_VISIBLE: u32 = 11;
const DATA_COLOR_BACKGROUND: u32 = 19;
const DATA_COLOR_CURSOR: u32 = 20;

/// `GhosttyStyleColorTag`
const STYLE_COLOR_NONE: u32 = 0;
const STYLE_COLOR_PALETTE: u32 = 1;
const STYLE_COLOR_RGB: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GhosttyPointCoordinate {
    pub x: u16,
    pub y: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union GhosttyPointValue {
    pub coordinate: GhosttyPointCoordinate,
    pub _padding: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GhosttyPoint {
    pub tag: u32,
    pub _pad: u32,
    pub value: GhosttyPointValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GhosttyGridRef {
    pub size: u64,
    pub node: *mut c_void,
    pub x: u16,
    pub y: u16,
}

/// ghostty_terminal_select_word 的入参（对应 C 侧 GhosttyTerminalSelectWordOptions）
#[repr(C)]
struct GhosttyTerminalSelectWordOptions {
    size: u64,
    grid_ref: GhosttyGridRef,
    boundary_codepoints: *const u32,
    boundary_codepoints_len: usize,
}

/// ghostty_terminal_select_line 的入参（对应 C 侧 GhosttyTerminalSelectLineOptions）
#[repr(C)]
struct GhosttyTerminalSelectLineOptions {
    size: u64,
    grid_ref: GhosttyGridRef,
    whitespace: *const u32,
    whitespace_len: usize,
    semantic_prompt_boundary: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GhosttyStyleColor {
    pub tag: u32,
    pub _pad: u32,
    pub value: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GhosttyStyle {
    pub size: u64,
    pub fg_color: GhosttyStyleColor,
    pub bg_color: GhosttyStyleColor,
    pub underline_color: GhosttyStyleColor,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: i32,
    pub _tail_pad: [u8; 4],
}

impl Default for GhosttyStyle {
    fn default() -> Self {
        // 安全：所有字段都是 POD，零初始化即合法
        unsafe { std::mem::zeroed() }
    }
}

/// `GhosttyMousePosition`（mouse/event.h）：表面坐标，单位像素。
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GhosttyMousePosition {
    pub x: f32,
    pub y: f32,
}

/// `GhosttyMouseEncoderSize`（mouse/encoder.h）：像素级格式（1016）换算用。
/// 首字段 `size` 必须等于结构体自身大小，故不用 `Default`，走 `new()` 构造。
#[repr(C)]
struct GhosttyMouseEncoderSize {
    size: usize,
    screen_width: u32,
    screen_height: u32,
    cell_width: u32,
    cell_height: u32,
    padding_top: u32,
    padding_bottom: u32,
    padding_right: u32,
    padding_left: u32,
}

impl GhosttyMouseEncoderSize {
    fn new(screen_w: u32, screen_h: u32, cell_w: u32, cell_h: u32) -> Self {
        Self {
            size: std::mem::size_of::<Self>(),
            screen_width: screen_w,
            screen_height: screen_h,
            cell_width: cell_w,
            cell_height: cell_h,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        }
    }
}

extern "C" {
    /// 引擎自省的 ABI 描述（结构体布局与枚举值）。
    /// 原生布局与 wasm32 不同，一切偏移与常量以此为准。
    /// 生产路径不调用，仅由 fixture 测试用来锁定 ABI。
    #[allow(dead_code)]
    fn ghostty_type_json() -> *const std::os::raw::c_char;
    fn ghostty_terminal_new(
        allocator: *const c_void,
        out: *mut *mut c_void,
        cols: u16,
        rows: u16,
    ) -> c_int;
    fn ghostty_terminal_free(terminal: *mut c_void);
    fn ghostty_terminal_resize(
        terminal: *mut c_void,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> c_int;
    fn ghostty_terminal_vt_write(terminal: *mut c_void, data: *const u8, len: usize);
    fn ghostty_terminal_get(terminal: *mut c_void, data: u32, out: *mut c_void) -> c_int;
    fn ghostty_terminal_set(terminal: *mut c_void, option: u32, value: *const c_void) -> c_int;

    // ── 鼠标编码（把滚轮/点击转成 PTY 程序认识的鼠标序列）──
    // 应用开启鼠标跟踪（omp/opencode/htop/vim 等 TUI）时，滚轮必须转发给
    // 程序自己滚动：备用屏没有本地 scrollback，本地回滚在 alt 屏里恒为空。
    fn ghostty_mouse_encoder_new(allocator: *const c_void, out: *mut *mut c_void) -> c_int;
    fn ghostty_mouse_encoder_free(encoder: *mut c_void);
    fn ghostty_mouse_encoder_setopt(encoder: *mut c_void, option: u32, value: *const c_void);
    fn ghostty_mouse_encoder_setopt_from_terminal(encoder: *mut c_void, terminal: *mut c_void);
    fn ghostty_mouse_encoder_encode(
        encoder: *mut c_void,
        event: *mut c_void,
        out_buf: *mut u8,
        out_buf_size: usize,
        out_len: *mut usize,
    ) -> c_int;
    fn ghostty_mouse_event_new(allocator: *const c_void, out: *mut *mut c_void) -> c_int;
    fn ghostty_mouse_event_free(event: *mut c_void);
    fn ghostty_mouse_event_set_action(event: *mut c_void, action: u32);
    fn ghostty_mouse_event_set_button(event: *mut c_void, button: u32);
    fn ghostty_mouse_event_set_mods(event: *mut c_void, mods: u16);
    fn ghostty_mouse_event_set_position(event: *mut c_void, position: GhosttyMousePosition);

    // ── Kitty 图形协议（内联图片） ──
    // 终端收到 `ESC_G`（APC G）后把图片解码进「图形存储」，前端按 placement
    // 的视口坐标把像素画到 canvas 上。PNG 解码由嵌入方提供（见 decode_png）。
    fn ghostty_sys_set(option: u32, value: *const c_void) -> c_int;
    fn ghostty_alloc(allocator: *const c_void, len: usize) -> *mut u8;
    fn ghostty_kitty_graphics_get(graphics: *mut c_void, data: u32, out: *mut c_void) -> c_int;
    fn ghostty_kitty_graphics_image(graphics: *mut c_void, image_id: u32) -> *mut c_void;
    fn ghostty_kitty_graphics_image_get(image: *mut c_void, data: u32, out: *mut c_void) -> c_int;
    fn ghostty_kitty_graphics_placement_iterator_new(
        allocator: *const c_void,
        out: *mut *mut c_void,
    ) -> c_int;
    fn ghostty_kitty_graphics_placement_iterator_free(iterator: *mut c_void);
    fn ghostty_kitty_graphics_placement_next(iterator: *mut c_void) -> bool;
    fn ghostty_kitty_graphics_placement_get(
        iterator: *mut c_void,
        data: u32,
        out: *mut c_void,
    ) -> c_int;
    /// 注意签名：除迭代器外还要给**图片句柄**与**终端句柄** —— 渲染几何
    /// 依赖图片像素尺寸与终端的格子尺寸。少传参数会直接段错误。
    fn ghostty_kitty_graphics_placement_render_info(
        iterator: *mut c_void,
        image: *mut c_void,
        terminal: *mut c_void,
        out: *mut GhosttyKittyPlacementRenderInfo,
    ) -> c_int;
    fn ghostty_search_new(
        allocator: *const c_void,
        out: *mut *mut c_void,
        terminal: *mut c_void,
    ) -> c_int;
    fn ghostty_search_free(search: *mut c_void);
    fn ghostty_search_set(search: *mut c_void, option: u32, value: *const c_void) -> c_int;
    fn ghostty_search_run(search: *mut c_void) -> c_int;
    fn ghostty_search_get(search: *mut c_void, data: u32, out: *mut c_void) -> c_int;

    fn ghostty_render_state_new(allocator: *const c_void, out: *mut *mut c_void) -> c_int;
    fn ghostty_render_state_free(state: *mut c_void);
    fn ghostty_render_state_update(state: *mut c_void, terminal: *mut c_void) -> c_int;
    fn ghostty_render_state_get(state: *mut c_void, data: u32, out: *mut c_void) -> c_int;

    /// 行级脏区迭代：增量帧的关键。`next_dirty` 只走变动的行，
    /// 把「每帧 40 行全送」降为「变了几行送几行」。
    fn ghostty_render_state_clean(state: *mut c_void) -> c_int;
    fn ghostty_render_state_set(state: *mut c_void, option: u32, value: *const c_void) -> c_int;
    fn ghostty_render_state_row_iterator_new(
        allocator: *const c_void,
        out: *mut *mut c_void,
    ) -> c_int;
    fn ghostty_render_state_row_iterator_free(iterator: *mut c_void);
    fn ghostty_render_state_row_iterator_next_dirty(iterator: *mut c_void, out_y: *mut u16)
        -> bool;
    /// 取当前行的数据（脏标记、原始行值、整行 cell 视图…）。
    fn ghostty_render_state_row_get(iterator: *mut c_void, data: u32, out: *mut c_void) -> c_int;
    /// 从打包的 cell 值里取字段。按值收 cell，无跨调用状态。
    fn ghostty_cell_get(cell: u64, data: u32, out: *mut c_void) -> c_int;

    /// 注意：`options` 按值传递（C ABI 下 24 字节结构体由调用方间接传入）。
    /// 声明成指针会让引擎从错误的地址读取字段。
    fn ghostty_terminal_selection_format_buf(
        terminal: *mut c_void,
        options: GhosttyTerminalSelectionFormatOptions,
        buf: *mut u8,
        buf_len: usize,
        out_written: *mut usize,
    ) -> c_int;
    /// 词/行选择：返回的是「选区快照」，需自行装回终端（见 select_word/select_line）
    fn ghostty_terminal_select_word(
        terminal: *mut c_void,
        options: *const GhosttyTerminalSelectWordOptions,
        out_selection: *mut GhosttySelection,
    ) -> c_int;
    fn ghostty_terminal_select_line(
        terminal: *mut c_void,
        options: *const GhosttyTerminalSelectLineOptions,
        out_selection: *mut GhosttySelection,
    ) -> c_int;
    fn ghostty_terminal_grid_ref(
        terminal: *mut c_void,
        point: *const GhosttyPoint,
        out: *mut GhosttyGridRef,
    ) -> c_int;
    /// 把 search 返回的 grid ref 换算成视口坐标；视口外返回 GHOSTTY_NO_VALUE。
    fn ghostty_terminal_point_from_grid_ref(
        terminal: *mut c_void,
        reference: *const GhosttyGridRef,
        tag: u32,
        out: *mut GhosttyPointCoordinate,
    ) -> c_int;
    fn ghostty_grid_ref_cell(grid_ref: *const GhosttyGridRef, out: *mut u64) -> c_int;
    /// 读取单元格的完整字素簇（基码点 + 组合码点）。
    /// 缓冲不够时返回 `GHOSTTY_OUT_OF_SPACE` 并写入所需码点数。
    fn ghostty_grid_ref_graphemes(
        grid_ref: *const GhosttyGridRef,
        buf: *mut u32,
        buf_len: usize,
        out_len: *mut usize,
    ) -> c_int;
    fn ghostty_grid_ref_style(grid_ref: *const GhosttyGridRef, out: *mut GhosttyStyle) -> c_int;
    /// 从 grid ref 取整行句柄（`GhosttyRow` 是不透明 u64）。
    fn ghostty_grid_ref_row(grid_ref: *const GhosttyGridRef, out_row: *mut u64) -> c_int;
    /// 读行级数据（语义提示、是否有字素簇等），键见 `GHOSTTY_ROW_DATA_*`。
    fn ghostty_row_get(row: u64, data: u32, out: *mut c_void) -> c_int;
    fn ghostty_terminal_scroll_viewport(
        terminal: *mut c_void,
        behavior: *const GhosttyTerminalScrollViewport,
    );

    fn ghostty_key_encoder_new(allocator: *const c_void, out: *mut *mut c_void) -> c_int;
    fn ghostty_key_encoder_free(encoder: *mut c_void);
    fn ghostty_key_encoder_setopt_from_terminal(encoder: *mut c_void, terminal: *mut c_void);
    fn ghostty_key_encoder_setopt(encoder: *mut c_void, option: u32, value: *const c_void);
    fn ghostty_key_encoder_encode(
        encoder: *mut c_void,
        event: *mut c_void,
        out_buf: *mut u8,
        out_buf_size: usize,
        out_len: *mut usize,
    ) -> c_int;

    fn ghostty_key_event_new(allocator: *const c_void, out: *mut *mut c_void) -> c_int;
    fn ghostty_key_event_free(event: *mut c_void);
    fn ghostty_key_event_set_action(event: *mut c_void, action: u32);
    fn ghostty_key_event_set_key(event: *mut c_void, key: u32);
    fn ghostty_key_event_set_mods(event: *mut c_void, mods: u16);
    fn ghostty_key_event_set_composing(event: *mut c_void, composing: bool);
    fn ghostty_key_event_set_utf8(event: *mut c_void, utf8: *const u8, len: usize);
    fn ghostty_key_event_set_unshifted_codepoint(event: *mut c_void, codepoint: u32);
}

/// `GhosttyMods`（u16 位域，取自 key/event.h）
pub mod mods {
    pub const SHIFT: u16 = 1 << 0;
    pub const CTRL: u16 = 1 << 1;
    pub const ALT: u16 = 1 << 2;
    pub const SUPER: u16 = 1 << 3;
    pub const CAPS_LOCK: u16 = 1 << 4;
    pub const NUM_LOCK: u16 = 1 << 5;
    pub const SHIFT_SIDE: u16 = 1 << 6;
    pub const CTRL_SIDE: u16 = 1 << 7;
}

/// `GhosttyKeyEncoderOption`
const ENCODER_OPT_ALT_ESC_PREFIX: u32 = 3;
const ENCODER_OPT_MACOS_OPTION_AS_ALT: u32 = 6;

/// `GhosttyOptionAsAlt`
const OPTION_AS_ALT_TRUE: i32 = 1;

/// `GhosttyKeyAction`
pub mod key_action {
    pub const RELEASE: u32 = 0;
    pub const PRESS: u32 = 1;
    pub const REPEAT: u32 = 2;
}

/// 一次按键的引擎表示。字段取自 DOM `KeyboardEvent`，
/// 由前端 `domKeyToInput` 填充。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInput {
    /// `GhosttyKey`
    pub key: u32,
    /// `GhosttyMods` 位域
    pub mods: u16,
    /// `GhosttyKeyAction`
    pub action: u32,
    /// 该键产生的 UTF-8 文本（普通输入用，控制键可为空）
    #[serde(default)]
    pub utf8: Option<String>,
    /// 未受 Shift 影响的码点，kitty 协议需要
    #[serde(default)]
    pub unshifted_codepoint: u32,
    #[serde(default)]
    pub composing: bool,
}

/// `GHOSTTY_TERMINAL_OPT_SELECTION` —— `ghostty_terminal_set` 的选项号。
/// 注意它属于 OPT 键（21），与同名的 DATA 键不是一回事；
/// 写成 1 会命中 `OPT_WRITE_PTY`，把结构体当成回调指针。
const OPT_SELECTION: u32 = 21;
/// `GHOSTTY_TERMINAL_OPT_COLOR_*`：默认前景/背景/光标
const OPT_COLOR_FOREGROUND: u32 = 11;
const OPT_COLOR_BACKGROUND: u32 = 12;
const OPT_COLOR_CURSOR: u32 = 13;

#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyColorRgb {
    r: u8,
    g: u8,
    b: u8,
}

/// `GhosttySelection`（原生 64 字节；wasm 下是 32 字节，含 GridRef 尺寸差异）
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GhosttySelection {
    pub size: u64,
    pub start: GhosttyGridRef,
    pub end: GhosttyGridRef,
    pub rectangle: bool,
    pub _pad: [u8; 7],
}

/// `GhosttyTerminalSelectionFormatOptions`（原生 24 字节）
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GhosttyTerminalSelectionFormatOptions {
    pub size: u64,
    pub emit: u32,
    pub unwrap: bool,
    pub trim: bool,
    pub _pad: [u8; 2],
    pub selection: *const GhosttySelection,
}

/// `GhosttyString { const uint8_t* ptr; size_t len; }`。
/// needle 需要传它的指针，而不是裸 `char*`。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GhosttyString {
    pub ptr: *const u8,
    pub len: usize,
}

/// `GhosttySearchOption`
const SEARCH_OPT_NEEDLE: u32 = 0;
const SEARCH_OPT_SELECT_NEXT: u32 = 1;
const SEARCH_OPT_SELECT_PREV: u32 = 2;

/// `GhosttySearchData`
const SEARCH_DATA_TOTAL_MATCHES: u32 = 2;
const SEARCH_DATA_SELECTED_INDEX: u32 = 3;
const SEARCH_DATA_SELECTED_MATCH: u32 = 4;

/// `GhosttyFormatterFormat::PLAIN`
const FORMATTER_FORMAT_PLAIN: u32 = 0;

/// `GhosttyTerminalScrollbar`（三个 u64）
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GhosttyTerminalScrollbar {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

/// `GhosttyTerminalScrollViewportTag`
pub const SCROLL_TOP: u32 = 0;
pub const SCROLL_BOTTOM: u32 = 1;
pub const SCROLL_DELTA: u32 = 2;
pub const SCROLL_ROW: u32 = 3;

/// `GhosttyTerminalScrollViewport`：tag(u32) + 4 字节填充 + union(16 字节)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GhosttyTerminalScrollViewport {
    pub tag: u32,
    pub _pad: u32,
    pub value: [u64; 2],
}

// ---------------------------------------------------------------------------
// 单元格位域解包（与 wasm 侧共用同一套 GhosttyCell 定义）
// ---------------------------------------------------------------------------

const CELL_CONTENT_TAG_MASK: u64 = 0b11;
const CELL_CODEPOINT_SHIFT: u32 = 2;
const CELL_CODEPOINT_MASK: u64 = (1 << 21) - 1;
const CELL_WIDE_SHIFT: u32 = 42;
const CELL_WIDE_MASK: u64 = 0b11;
/// 字素簇下标 + 1（0 = 无簇），线格式 bit 40..63。
const CELL_CLUSTER_SHIFT: u32 = 40;
const CELL_CLUSTER_MASK: u64 = (1 << 24) - 1;
/// 栈上字素簇缓冲的码点数。组合重音 / 常见 emoji 序列都在 8 以内，
/// 超出（长 ZWJ 序列）才走堆重试。
const CLUSTER_STACK_CODEPOINTS: usize = 8;

/// 一个单元格渲染所需的全部信息，可直接被前端消费。
#[derive(Debug, Clone, Copy, Default)]
pub struct CellView {
    pub codepoint: u32,
    /// `GhosttyCellWide`：0=NARROW 1=WIDE 2=SPACER_TAIL 3=SPACER_HEAD
    pub wide: u8,
    pub has_text: bool,
    /// 已解析为真彩的前景/背景（None 表示该单元格未覆写，用主题默认）
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    /// 完整字素簇在 `Snapshot::clusters` 中的下标 + 1（0 = 无字素簇，
    /// 即该格只有一个码点）。线格式里占 bit 40..63。
    pub cluster_index: u32,
}

impl CellView {
    /// 宽字符的首格（占用两列的左半）
    pub fn is_wide(&self) -> bool {
        self.wide == 1
    }

    /// 宽字符的尾格，渲染时应跳过
    pub fn is_spacer(&self) -> bool {
        self.wide == 2 || self.wide == 3
    }
}

/// 搜索时的选中方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchSelect {
    Next,
    Prev,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSelection {
    pub start_x: u16,
    pub start_y: u16,
    pub end_x: u16,
    pub end_y: u16,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub total: u64,
    pub selected: Option<u64>,
    /// 当前选中匹配的视口坐标；Find Bar 用它在前端画选区。
    pub selection: Option<SearchSelection>,
}

/// 一次快照：整屏单元格 + 光标 + 尺寸
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor_x: u16,
    pub cursor_y: u16,
    /// 终端默认前景色（0xRRGGBB），None 表示未设置
    pub default_fg: Option<u32>,
    pub default_bg: Option<u32>,
    pub cursor_color: Option<u32>,
    pub cursor_visible: bool,
    pub cursor_pending_wrap: bool,
    /// `GhosttyRenderStateCursorVisualStyle`（0=bar 1=block 2=underline 3=block_hollow）
    pub cursor_style: u32,
    pub scrollbar: GhosttyTerminalScrollbar,
    pub cells: Vec<CellView>,
    /// 去重后的完整字素簇表（只收 >1 码点的格）。
    /// `CellView::cluster_index` 是它的下标 + 1。
    pub clusters: Vec<String>,
    /// 每行一个字节：bit0 = OSC 133 prompt 行，bit1 = prompt 续行。
    pub row_flags: Vec<u8>,
}

// ---------------------------------------------------------------------------
// 终端会话
// ---------------------------------------------------------------------------

pub struct NativeTerminal {
    handle: *mut c_void,
    cols: u16,
    rows: u16,
    /// 复用的调色板缓冲。
    ///
    /// ABI 依据（768 = 256 × 3）：`GHOSTTY_TERMINAL_DATA_COLOR_PALETTE`
    /// （键 21）的输出类型是 `GhosttyColorRgb[256]`，而 `GhosttyColorRgb`
    /// 是 r/g/b 三个 `uint8_t`（无 alpha 字段，见
    /// vendor/ghostty-vt/include/ghostty/vt/color.h），故引擎整块内联写入
    /// 256 × 3 = 768 字节，此缓冲必须恒为 768 字节。
    palette: Vec<u8>,
    palette_ready: bool,
    /// 复用的 grid ref / style 暂存
    grid_ref: GhosttyGridRef,
    style: GhosttyStyle,
    /// 键盘编码器句柄。其模式（kitty 协议标志、光标键应用模式等）
    /// 由终端状态决定，每次编码前同步一次。
    key_encoder: *mut c_void,
    key_event: *mut c_void,
    /// 滚轮转发用的鼠标编码器/事件句柄（惰性创建，只有真用到鼠标协议时才付成本）
    mouse_encoder: *mut c_void,
    mouse_event: *mut c_void,
    /// 搜索句柄与其当前 needle。必须跨调用存活：`SELECT_NEXT` 的
    /// 选中位置是搜索对象自身持有的状态，每次重建会让「下一个」
    /// 永远返回第一个匹配。
    search: *mut c_void,
    search_needle: String,
    /// 渲染状态：光标形状/可见性等只能从这里正确取得。
    /// 注意 `GHOSTTY_TERMINAL_DATA_CURSOR_STYLE` 输出的是完整的
    /// `GhosttyStyle`（72 字节），当作 u32 读取会写穿栈。
    render_state: *mut c_void,
    /// 快照缓冲。rAF 每次重绘都要取一帧，每帧都新建 3 个大 Vec 会让
    /// 分配器成为热点（80x24 约 3.8KB cells + 30KB packed + 30KB out）。
    /// 提到结构体后 `clear` 保留容量复用，稳态下零分配。
    snapshot_cells: Vec<CellView>,
    snapshot_packed: Vec<u64>,
    snapshot_out: Vec<u8>,
    /// 快照的字素簇表与行标记缓冲，跨帧复用（`snapshot_wire` 归还）。
    snapshot_clusters: Vec<String>,
    snapshot_row_flags: Vec<u8>,
    /// 增量帧输出缓冲（跨帧复用）。
    diff_out: Vec<u8>,
    /// 增量帧暂存（脏行缓冲、样式表、单元格句柄）。
    diff_scratch: DiffScratch,
    /// 上一帧的滚动条 offset，用于算滚动位移。
    last_scroll_offset: u64,
    /// 样式表缓冲。原实现每帧 `StyleTable::default()`，即每帧新建一个
    /// Vec + HashMap（稳态下条目通常个位数，HashMap 的桶数组反而是主要
    /// 开销）。提到结构体后 `clear()` 保留容量复用。
    snapshot_styles: StyleTable,
    /// 引擎回调共享上下文（回包缓冲 + 尺寸报告几何）。
    ///
    /// `Box` 换**稳定地址**：回调里存着它的裸指针，而 `NativeTerminal`
    /// 构造完还要被搬进 `Arc<Mutex<…>>`（见 CallbackCtx 注释）。
    ctx: Box<CallbackCtx>,
}

// 终端句柄由 PTY 读线程与命令线程共用；引擎本身要求调用方自行序列化，
// 这里用 Mutex 在会话层保证（见 vt_session.rs）。
unsafe impl Send for NativeTerminal {}

// ───────────────────────────────────────────────────────────────────────────
// Kitty 图形协议（终端内联图片）
//
// 数据流：PTY 收到 `ESC_G`（APC G，base64 载荷）→ 引擎解码成 RGBA 存进
// 「图形存储」→ 前端按 placement 的视口坐标把像素画到 canvas。
//
// 两件必须由嵌入方做的事（缺任何一件引擎都会**拒绝**图片，终端里就只剩
// 一堆 U+10EEEE 占位格被画成豆腐块）：
//   1. `GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT` 设成非零；
//   2. 通过 `ghostty_sys_set(GHOSTTY_SYS_OPT_DECODE_PNG, …)` 装 PNG 解码器
//      （协议里 f=100 传的就是 PNG，引擎自己不实现 PNG 解码）。
// ───────────────────────────────────────────────────────────────────────────

/// `GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT`
const OPT_KITTY_IMAGE_STORAGE_LIMIT: u32 = 15;
/// `GHOSTTY_SYS_OPT_DECODE_PNG`
const SYS_OPT_DECODE_PNG: u32 = 1;
/// 图形存储上限：单个终端最多缓存 64 MiB 解码后像素，超出后引擎自行淘汰。
const KITTY_STORAGE_LIMIT: usize = 64 * 1024 * 1024;
/// `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES`：回滚历史行数上限（size_t*）
const OPT_SCROLLBACK_MAX_LINES: u32 = 28;
/// `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES`：回滚历史字节配额（size_t*）
const OPT_SCROLLBACK_MAX_BYTES: u32 = 27;
/// 回滚历史上限：20_000 行 + 128 MB 字节配额（跨项目多会话时的内存预算）。
///
/// **两个都必须设**：引擎默认的字节配额只有 10000，它才是真正的瓶颈 ——
/// 实测写 30 万行（100 列、45 字符行）在默认配置下只留 877 行可回看，
/// 行上限形同虚设。症状就是「在终端里跑 omp/opencode，输出一多就翻不回去」。
///
/// 实测（同一负载）：默认 → 877 行 / RSS +1 MB；50k 行 + 512 MB →
/// 49_699 行 / RSS +42 MB（100 列）、49_833 行 / RSS +84 MB（200 列）。
/// 行上限是面向用户的策略（约 1000 屏），字节配额只在极端内容密度
/// （超宽终端、重样式/组合字符）下才先于行数生效，作为内存安全阀。
const SCROLLBACK_MAX_LINES: usize = 20_000;
pub const SCROLLBACK_MAX_BYTES: usize = 128 * 1024 * 1024;
/// `GhosttyMouseAction`（mouse/event.h）
const MOUSE_ACTION_PRESS: u32 = 0;
const MOUSE_ACTION_MOTION: u32 = 2;
/// `GhosttyMouseButton`：滚轮上/下（xterm 约定 4/5）
const MOUSE_BUTTON_FOUR: u32 = 4;
const MOUSE_BUTTON_FIVE: u32 = 5;
/// `GhosttyMouseEncoderOption`
const MOUSE_ENCODER_OPT_SIZE: u32 = 2;
/// `GhosttyKey`：方向键（与前端 domKeys.ts 的 GHOSTTY_KEY 表同源）
const KEY_ARROW_DOWN: u32 = 75;
const KEY_ARROW_UP: u32 = 78;
/// 一次滚轮下发最多编码多少个事件：累积行数 ÷ 3 ≈ 真实滚轮「一格」，
/// 再封顶避免长按横扫时把成千上万条序列灌进 PTY。
const MAX_WHEEL_EVENTS: usize = 5;
/// `GHOSTTY_MODE_ALT_SCROLL`（DEC 1007）：备用屏里把滚轮转成上下方向键
const MODE_ALT_SCROLL: u16 = 1007;

/// 累积行数换算成鼠标事件/方向键个数：3 行 ≈ 真实滚轮一格，
/// 至少 1 个（否则小增量会丢），至多 `MAX_WHEEL_EVENTS` 个。
fn wheel_event_count(delta: i64) -> usize {
    (delta.unsigned_abs() as usize)
        .div_ceil(3)
        .clamp(1, MAX_WHEEL_EVENTS)
}

/// `GHOSTTY_TERMINAL_OPT_USERDATA`：回调的 userdata 指针（值就是指针本身）。
const OPT_USERDATA: u32 = 0;
/// `GHOSTTY_TERMINAL_OPT_SIZE`：XTWINOPS（`CSI 14/16/18 t`）与 in-band 尺寸报告
/// 的应答来源。TUI 靠 `CSI 16 t` 拿「一格多少像素」来算图片几何，不回它会退化。
const OPT_SIZE: u32 = 6;
/// `GHOSTTY_TERMINAL_OPT_PWD_CHANGED`：OSC 7 / OSC 9 / OSC 1337 改变 cwd 时回调
const OPT_PWD_CHANGED: u32 = 25;
/// `GHOSTTY_TERMINAL_DATA_PWD`：读取当前 pwd（回调内使用）
const DATA_PWD: u32 = 13;
/// `GHOSTTY_TERMINAL_OPT_WRITE_PTY`：把 VT 查询回包写回 PTY 的通道。
///
/// 与 `OPT_SELECTION`（结构体指针）不同：**回调类选项的 `value` 就是函数指针
/// 本身**，传 `&fn` 会让引擎把栈地址当代码跳转（实测 SIGBUS）。
const OPT_WRITE_PTY: u32 = 1;

/// kitty unicode 占位格码点：虚拟 placement 用它标记图片覆盖的格子，
/// 且这些格子的**前景色就是所属图片的 id**（协议规定如此）。
const PLACEHOLDER_CODEPOINT: u32 = 0x10EEEE;

/// `GHOSTTY_TERMINAL_DATA_KITTY_GRAPHICS`：取图形存储句柄。
const DATA_KITTY_GRAPHICS: u32 = 30;
/// `GHOSTTY_KITTY_GRAPHICS_DATA_GENERATION`
const KITTY_DATA_GENERATION: u32 = 2;
/// `GHOSTTY_KITTY_GRAPHICS_DATA_PLACEMENT_ITERATOR`
const KITTY_DATA_PLACEMENT_ITERATOR: u32 = 1;

/// `GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_*`
const PLACEMENT_DATA_IMAGE_ID: u32 = 1;
const PLACEMENT_DATA_PLACEMENT_ID: u32 = 2;
const PLACEMENT_DATA_IS_VIRTUAL: u32 = 3;
const PLACEMENT_DATA_Z: u32 = 12;

/// `GHOSTTY_KITTY_IMAGE_DATA_*`
const IMAGE_DATA_WIDTH: u32 = 3;
const IMAGE_DATA_HEIGHT: u32 = 4;
const IMAGE_DATA_FORMAT: u32 = 5;
const IMAGE_DATA_DATA_PTR: u32 = 7;
const IMAGE_DATA_DATA_LEN: u32 = 8;
const IMAGE_DATA_GENERATION: u32 = 9;

/// `GHOSTTY_KITTY_IMAGE_FORMAT_*`
const IMAGE_FORMAT_RGB: u32 = 0;
const IMAGE_FORMAT_RGBA: u32 = 1;
const IMAGE_FORMAT_GRAY_ALPHA: u32 = 3;
const IMAGE_FORMAT_GRAY: u32 = 4;

/// 引擎侧 `GhosttyKittyGraphicsPlacementRenderInfo` 的镜像。
/// 带 `size` 的 sized-struct ABI：调用前必须填 size。
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct GhosttyKittyPlacementRenderInfo {
    size: usize,
    pixel_width: u32,
    pixel_height: u32,
    grid_cols: u32,
    grid_rows: u32,
    viewport_col: i32,
    viewport_row: i32,
    viewport_visible: bool,
    source_x: u32,
    source_y: u32,
    source_width: u32,
    source_height: u32,
}

/// 引擎侧 `GhosttySysImage` 的镜像（PNG 解码回调的产物）。
///
/// 布局必须与 `ghostty/vt/sys.h` 完全一致：**没有** format 字段
/// （回调契约就是「解成 RGBA」）。多一个字段会让引擎把宽度当指针去 free ——
/// 直接 abort（malloc 报 "pointer being freed was not allocated"）。
#[repr(C)]
struct GhosttySysImage {
    width: u32,
    height: u32,
    data: *mut u8,
    data_len: usize,
}

/// 引擎侧 `GhosttySizeReportSize` 的镜像（见 vendor/.../size_report.h）：
/// `{ uint16_t rows; uint16_t columns; uint32_t cell_width; uint32_t cell_height; }`
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct GhosttySizeReportSize {
    rows: u16,
    columns: u16,
    cell_width: u32,
    cell_height: u32,
}

/// 引擎回调共享上下文。
///
/// 引擎只给一个 userdata 指针，而回包收集与尺寸报告都要用到终端状态；打包成一个
/// 结构体挂上去。用 `Box` 取**稳定地址**：`NativeTerminal` 构造完还要被搬进
/// `Arc<Mutex<…>>`，内联字段会随结构体移动让回调里的裸指针悬空。
struct CallbackCtx {
    /// 待写回 PTY 的 VT 查询回包（见 `collect_query_response`）
    responses: Vec<u8>,
    /// shell 通过 OSC 7 / OSC 9 / OSC 1337 报告的 cwd；None = 尚未报告
    pwd: Option<String>,
    /// 自上次 `take_pwd_change` 后是否发生过变化
    pwd_dirty: bool,
    /// 尺寸报告用几何：与终端当前 grid/像素尺寸保持一致
    cols: u16,
    rows: u16,
    cell_width_px: u32,
    cell_height_px: u32,
}

/// OSC 7 / OSC 9 / OSC 1337 改变 cwd：只把值记进上下文，事件由 PTY 读线程发出。
///
/// 回调发生在 `ghostty_terminal_vt_write` 内，此时调用方持有 terminal 锁；
/// 这里不能再回调进终端或做阻塞操作，只做一次 `ghostty_terminal_get` + 拷贝。
unsafe extern "C" fn note_pwd_change(terminal: *mut c_void, userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    let ctx = &mut *(userdata as *mut CallbackCtx);
    let mut value = GhosttyString {
        ptr: std::ptr::null(),
        len: 0,
    };
    let rc = unsafe {
        ghostty_terminal_get(
            terminal,
            DATA_PWD,
            &mut value as *mut GhosttyString as *mut c_void,
        )
    };
    if rc != 0 {
        return;
    }
    ctx.pwd = Some(if value.ptr.is_null() || value.len == 0 {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
        String::from_utf8_lossy(bytes).into_owned()
    });
    ctx.pwd_dirty = true;
}

/// XTWINOPS 尺寸查询的应答。
///
/// 格子像素尺寸未就绪（从未带像素尺寸 resize，例如无头测试）时返回 false，
/// 宁可不答也不回 0x0 的假几何 —— 有的 TUI 会拿它去算图片与布局。
unsafe extern "C" fn report_terminal_size(
    _terminal: *mut c_void,
    userdata: *mut c_void,
    out: *mut GhosttySizeReportSize,
) -> bool {
    if userdata.is_null() || out.is_null() {
        return false;
    }
    let ctx = &*(userdata as *const CallbackCtx);
    if ctx.cell_width_px == 0 || ctx.cell_height_px == 0 {
        return false;
    }
    *out = GhosttySizeReportSize {
        rows: ctx.rows,
        columns: ctx.cols,
        cell_width: ctx.cell_width_px,
        cell_height: ctx.cell_height_px,
    };
    true
}

/// VT 查询回包收集回调。
///
/// 引擎在 `ghostty_terminal_vt_write` 内同步调用：DA1/DA2（`CSI c`/`> c`）、
/// DECRQM（`CSI ? Ps $p`）、DSR（`CSI n`）、XTWINOPS（`CSI 14/16/18 t`），
/// 以及 kitty 图形协议握手（`a=q` → `i=<id>;OK`）。这些回包必须写回 PTY，
/// 否则在 TUI 眼里这就是个「哑终端」——omp 判定内联图片能力时会发
/// `CSI ? 2;1;0 S` 探测 sixel，250ms 收不到回包即认定不支持，图片直接不画。
///
/// 回调里不做 IO（引擎在写路径上同步等待）：只把字节攒进 `responses`，
/// 由 PTY 读线程统一 `take_responses()` 后写回子进程。
unsafe extern "C" fn collect_query_response(
    _terminal: *mut c_void,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if userdata.is_null() || data.is_null() || len == 0 {
        return;
    }
    let ctx = &mut *(userdata as *mut CallbackCtx);
    ctx.responses
        .extend_from_slice(std::slice::from_raw_parts(data, len));
}

/// PNG 解码回调：把协议里的 PNG 载荷解成 RGBA。
///
/// 输出缓冲必须用引擎给的 allocator 分配（引擎随后用同一个 allocator 释放），
/// 因此走 `ghostty_alloc`。解码失败返回 false，引擎会拒绝该图片并记日志。
unsafe extern "C" fn decode_png(
    _userdata: *mut c_void,
    allocator: *const c_void,
    data: *const u8,
    data_len: usize,
    out: *mut GhosttySysImage,
) -> bool {
    if data.is_null() || out.is_null() || data_len == 0 {
        return false;
    }
    let bytes = std::slice::from_raw_parts(data, data_len);
    let mut decoder = png::Decoder::new(bytes);
    // 统一到 8 位、带 alpha：EXPAND 展开调色板/低位深，ALPHA 补出不透明通道
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = match decoder.read_info() {
        Ok(d) => d,
        Err(e) => {
            log::warn!("PNG 解码失败（读头）: {e}");
            return false;
        }
    };
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = match reader.next_frame(&mut buf) {
        Ok(i) => i,
        Err(e) => {
            log::warn!("PNG 解码失败（像素）: {e}");
            return false;
        }
    };
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            let mut v = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in buf[..info.buffer_size()].chunks_exact(3) {
                v.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            v
        }
        png::ColorType::Grayscale => {
            let mut v = Vec::with_capacity((info.width * info.height * 4) as usize);
            for g in buf[..info.buffer_size()].iter() {
                v.extend_from_slice(&[*g, *g, *g, 255]);
            }
            v
        }
        png::ColorType::GrayscaleAlpha => {
            let mut v = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in buf[..info.buffer_size()].chunks_exact(2) {
                v.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            v
        }
        png::ColorType::Indexed => {
            // normalize_to_color8 已把索引色展开，这里兜底
            log::warn!("PNG 索引色未展开，跳过");
            return false;
        }
    };
    let len = pixels.len();
    let dst = ghostty_alloc(allocator, len);
    if dst.is_null() {
        log::warn!("PNG 解码：引擎 allocator 分配 {len} 字节失败");
        return false;
    }
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), dst, len);
    *out = GhosttySysImage {
        width: info.width,
        height: info.height,
        data: dst,
        data_len: len,
    };
    true
}

/// 全局安装一次 PNG 解码器：`ghostty_sys_set` 是进程级设置。
fn ensure_png_decoder() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let f: unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const u8,
            usize,
            *mut GhosttySysImage,
        ) -> bool = decode_png;
        let rc = unsafe { ghostty_sys_set(SYS_OPT_DECODE_PNG, f as *const c_void) };
        if rc != 0 {
            log::warn!("安装 PNG 解码器失败（rc={rc}）：终端内联图片将不可用");
        }
    });
}

/// 一张已解码的图片（前端缓存用）。
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KittyImageDto {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    /// 内容代际：同 id 重新传输时变化，前端据此丢弃旧位图
    pub generation: u64,
}

/// 一次图片放置（几何按当前视口给出，滚动即变）。
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KittyPlacementDto {
    pub image_id: u32,
    pub placement_id: u32,
    /// 视口列/行（可为负 = 部分可见）
    pub x: i32,
    pub y: i32,
    pub cols: u32,
    pub rows: u32,
    /// 渲染像素尺寸
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// 源矩形（裁剪用）
    pub source_x: u32,
    pub source_y: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub z: i32,
    /// Unicode 占位格模式：格子里是 U+10EEEE，图片由终端画在格子上
    pub is_virtual: bool,
}

/// 一张图在视口里可见的占位格范围（格坐标，含端点）。
struct PlaceholderSpan {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

/// 迭代器读出的原始 placement（几何与元数据在 `images()` 里按需补齐）。
struct RawPlacement {
    image_id: u32,
    placement_id: u32,
    is_virtual: bool,
    z: i32,
    info: GhosttyKittyPlacementRenderInfo,
}

/// 图形存储快照：`generation` 不变即「放置集合与图片内容都没变」，
/// 前端可以整帧跳过图片查询（滚动只改几何，由 placement 的 y 体现）。
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KittyImagesDto {
    pub generation: u64,
    pub placements: Vec<KittyPlacementDto>,
    pub images: Vec<KittyImageDto>,
}

impl NativeTerminal {
    /// 打开图形存储：不设上限时引擎直接拒绝 `ESC_G`，终端里只剩占位格。
    fn enable_kitty_graphics(&mut self) {
        ensure_png_decoder();
        let limit: usize = KITTY_STORAGE_LIMIT;
        let rc = unsafe {
            ghostty_terminal_set(
                self.handle,
                OPT_KITTY_IMAGE_STORAGE_LIMIT,
                &limit as *const usize as *const c_void,
            )
        };
        if rc != 0 {
            log::warn!("启用 Kitty 图形存储失败（rc={rc}）：终端内联图片将不可用");
        }
    }

    /// 打开 VT 查询回包通道。
    ///
    /// 引擎已经把回包算好了（DA1 `ESC[?62;22c`、DECRQM `ESC[?<n>;2$y`、
    /// DSR `ESC[1;1R`、kitty 握手 `ESC_Gi=<id>;OK`…），缺的只是「谁写回 PTY」。
    /// 不开这一路，任何按标准探测能力的程序（TUI 的图片/同步输出/超链接能力
    /// 判定）都会把我们当哑终端。
    fn enable_query_responses(&mut self) {
        let userdata = &mut *self.ctx as *mut CallbackCtx as *const c_void;
        unsafe {
            ghostty_terminal_set(self.handle, OPT_USERDATA, userdata);
            let rc = ghostty_terminal_set(
                self.handle,
                OPT_WRITE_PTY,
                collect_query_response as *const c_void,
            );
            if rc != 0 {
                log::warn!("安装 WRITE_PTY 回调失败（rc={rc}）：终端将不回应急序查询");
            }
            let rc =
                ghostty_terminal_set(self.handle, OPT_SIZE, report_terminal_size as *const c_void);
            if rc != 0 {
                log::warn!("安装尺寸报告回调失败（rc={rc}）：XTWINOPS 尺寸查询将无应答");
            }
            let rc = ghostty_terminal_set(
                self.handle,
                OPT_PWD_CHANGED,
                note_pwd_change as *const c_void,
            );
            if rc != 0 {
                log::warn!("安装 PWD_CHANGED 回调失败（rc={rc}）：shell 集成无法更新 cwd");
            }
        }
    }

    /// 取走待写回 PTY 的回包（写入由 PTY 读线程负责）。
    ///
    /// 绝大多数帧没有任何查询，这里对空缓冲直接返回空 Vec，不做分配。
    pub fn take_responses(&mut self) -> Vec<u8> {
        if self.ctx.responses.is_empty() {
            return Vec::new();
        }
        std::mem::take(&mut self.ctx.responses)
    }

    /// 取走 shell 最近一次报告的 cwd 变化；没有变化返回 None。
    ///
    /// 空字符串表示 shell 主动清除了 pwd（OSC 7 空值），调用方按“无 cwd”处理。
    pub fn take_pwd_change(&mut self) -> Option<String> {
        if !self.ctx.pwd_dirty {
            return None;
        }
        self.ctx.pwd_dirty = false;
        self.ctx.pwd.take()
    }

    /// 图形存储句柄；未启用或引擎未编译该特性时为空。
    fn kitty_graphics(&self) -> *mut c_void {
        let mut out: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            ghostty_terminal_get(
                self.handle,
                DATA_KITTY_GRAPHICS,
                &mut out as *mut _ as *mut c_void,
            )
        };
        if rc == 0 {
            out
        } else {
            std::ptr::null_mut()
        }
    }

    /// 图形存储代际（0 = 从未写入）。
    fn kitty_generation(&self, graphics: *mut c_void) -> u64 {
        let mut gen: u64 = 0;
        let rc = unsafe {
            ghostty_kitty_graphics_get(
                graphics,
                KITTY_DATA_GENERATION,
                &mut gen as *mut u64 as *mut c_void,
            )
        };
        if rc == 0 {
            gen
        } else {
            0
        }
    }

    /// 当前视口的全部图片放置 + 图片元数据。
    ///
    /// 分两趟：先读迭代器，再（仅在存在虚拟 placement 时）扫格子反查它们的
    /// 左上角。原因见 `placeholder_origins`。
    pub fn images(&self) -> KittyImagesDto {
        let graphics = self.kitty_graphics();
        if graphics.is_null() {
            return KittyImagesDto {
                generation: 0,
                placements: Vec::new(),
                images: Vec::new(),
            };
        }
        let generation = self.kitty_generation(graphics);

        // 第一趟：迭代器只保证「当前位置」有效，必须先取出数据再决定几何
        let mut raw: Vec<RawPlacement> = Vec::new();
        let mut iterator: *mut c_void = std::ptr::null_mut();
        let rc = unsafe {
            ghostty_kitty_graphics_placement_iterator_new(std::ptr::null(), &mut iterator)
        };
        if rc == 0 && !iterator.is_null() {
            let loaded = unsafe {
                ghostty_kitty_graphics_get(
                    graphics,
                    KITTY_DATA_PLACEMENT_ITERATOR,
                    &mut iterator as *mut _ as *mut c_void,
                )
            };
            if loaded == 0 {
                while unsafe { ghostty_kitty_graphics_placement_next(iterator) } {
                    let mut image_id: u32 = 0;
                    let mut placement_id: u32 = 0;
                    let mut is_virtual = false;
                    let mut z: i32 = 0;
                    unsafe {
                        ghostty_kitty_graphics_placement_get(
                            iterator,
                            PLACEMENT_DATA_IMAGE_ID,
                            &mut image_id as *mut u32 as *mut c_void,
                        );
                        ghostty_kitty_graphics_placement_get(
                            iterator,
                            PLACEMENT_DATA_PLACEMENT_ID,
                            &mut placement_id as *mut u32 as *mut c_void,
                        );
                        ghostty_kitty_graphics_placement_get(
                            iterator,
                            PLACEMENT_DATA_IS_VIRTUAL,
                            &mut is_virtual as *mut bool as *mut c_void,
                        );
                        ghostty_kitty_graphics_placement_get(
                            iterator,
                            PLACEMENT_DATA_Z,
                            &mut z as *mut i32 as *mut c_void,
                        );
                    }
                    let image = unsafe { ghostty_kitty_graphics_image(graphics, image_id) };
                    if image.is_null() {
                        continue;
                    }
                    let mut info = GhosttyKittyPlacementRenderInfo {
                        size: std::mem::size_of::<GhosttyKittyPlacementRenderInfo>(),
                        ..Default::default()
                    };
                    let rc = unsafe {
                        ghostty_kitty_graphics_placement_render_info(
                            iterator,
                            image,
                            self.handle,
                            &mut info,
                        )
                    };
                    if rc != 0 {
                        continue;
                    }
                    raw.push(RawPlacement {
                        image_id,
                        placement_id,
                        is_virtual,
                        z,
                        info,
                    });
                }
            }
            unsafe { ghostty_kitty_graphics_placement_iterator_free(iterator) };
        }

        // 虚拟 placement 的左上角只能从占位格反查（引擎明确不给虚拟 placement 坐标）
        let wanted: Vec<u32> = raw
            .iter()
            .filter(|r| r.is_virtual)
            .map(|r| r.image_id)
            .collect();
        let origins = self.placeholder_origins(&wanted);

        let mut placements = Vec::new();
        let mut images: Vec<KittyImageDto> = Vec::new();
        for placement in raw {
            let RawPlacement {
                image_id,
                placement_id,
                is_virtual,
                z,
                info,
            } = placement;
            // 元数据与可见性无关：视口外的图片也要能被前端预取，
            // 否则滚动到它时才现拉像素（会闪一下空白）
            let dims = match images.iter().find(|m| m.id == image_id) {
                Some(m) => Some((m.width, m.height)),
                None => self.image_meta(graphics, image_id).map(|meta| {
                    let dims = (meta.width, meta.height);
                    images.push(meta);
                    dims
                }),
            };
            // 图片已被替换/删除：placement 无图可画
            let Some((img_width, img_height)) = dims else {
                continue;
            };

            // 虚拟 placement 的可见范围：占位格不在视口里就没有可渲染位置，
            // 这一帧跳过（等滚回来再画）
            let span = if is_virtual {
                origins.get(&image_id)
            } else {
                None
            };
            let (x, y) = if is_virtual {
                let Some(span) = span else {
                    continue;
                };
                (span.min_x, span.min_y)
            } else {
                if !info.viewport_visible {
                    continue;
                }
                (info.viewport_col, info.viewport_row)
            };

            // 采样源矩形：协议里 source 宽高为 0 表示「取整张图」
            let mut cols = info.grid_cols;
            let mut rows = info.grid_rows;
            let mut source_x = info.source_x;
            let mut source_y = info.source_y;
            let mut source_width = if info.source_width > 0 {
                info.source_width
            } else {
                img_width
            };
            let mut source_height = if info.source_height > 0 {
                info.source_height
            } else {
                img_height
            };
            let mut pixel_width = info.pixel_width;
            let mut pixel_height = info.pixel_height;

            // 虚拟 placement 的可见几何：图片往上/往左滚出视口时，被裁掉的部分
            // 必须从采样源矩形里扣掉。只给「左上角 + 整图」会让图片贴着视口顶边
            // 画出来，显示的还是最上面几行 —— 内容对不上。
            // 判据只用可见格数：块的左上角落在视口边界上（0）且可见格数不足时，
            // 差额就是被裁掉的部分。omp 这类 TUI 会把图片限制在视口内，故
            // 「块比视口还大」的歧义情况不会出现（真出现也只影响裁剪量）。
            if let Some(span) = span {
                let visible_cols = (span.max_x - span.min_x + 1) as u32;
                let visible_rows = (span.max_y - span.min_y + 1) as u32;
                let clip_top = if span.min_y == 0 && visible_rows < rows {
                    rows - visible_rows
                } else {
                    0
                };
                let clip_left = if span.min_x == 0 && visible_cols < cols {
                    cols - visible_cols
                } else {
                    0
                };
                if clip_top > 0 || clip_left > 0 {
                    let row_px = source_height / rows.max(1);
                    let col_px = source_width / cols.max(1);
                    source_y += clip_top * row_px;
                    source_x += clip_left * col_px;
                    source_height -= clip_top * row_px;
                    source_width -= clip_left * col_px;
                    let row_draw_px = pixel_height / rows.max(1);
                    let col_draw_px = pixel_width / cols.max(1);
                    pixel_height = (rows - clip_top) * row_draw_px;
                    pixel_width = (cols - clip_left) * col_draw_px;
                    rows -= clip_top;
                    cols -= clip_left;
                }
            }

            placements.push(KittyPlacementDto {
                image_id,
                placement_id,
                x,
                y,
                cols,
                rows,
                pixel_width,
                pixel_height,
                source_x,
                source_y,
                source_width,
                source_height,
                z,
                is_virtual,
            });
        }

        KittyImagesDto {
            generation,
            placements,
            images,
        }
    }

    /// 反查虚拟 placement（kitty unicode 占位格）在视口里覆盖的范围。
    ///
    /// 协议规定：占位格是 `U+10EEEE` + 组合附加符，**格子的前景色就是图片 id**；
    /// 而引擎对虚拟 placement 不给坐标（`render_info` 的 viewport_col/row 明确
    /// 「无意义」，`placement_rect` / `placement_viewport_pos` 对虚拟 placement
    /// 返回 NO_VALUE）。位置只能照协议从格子内容里反查 —— 谁是这张图的占位格，
    /// 图片就画在它们覆盖的矩形上。
    ///
    /// 返回每张图可见部分的边界（`PlaceholderSpan`）：聊天记录往上滚时图片顶部
    /// 会被视口裁掉，只给一个左上角会让整张图「贴」到视口顶端 —— 靠可见行数
    /// 反推被裁掉的行数，采样源矩形同步内缩（见 `images()`）。
    ///
    /// 只在存在虚拟 placement 时调用。前景色按真彩解析（协议要求 `38;2;R;G;B`），
    /// 不依赖调色板是否已刷新。
    fn placeholder_origins(&self, wanted: &[u32]) -> HashMap<u32, PlaceholderSpan> {
        let mut out: HashMap<u32, PlaceholderSpan> = HashMap::new();
        if wanted.is_empty() {
            return out;
        }
        let mut grid_ref = GhosttyGridRef {
            size: std::mem::size_of::<GhosttyGridRef>() as u64,
            node: std::ptr::null_mut(),
            x: 0,
            y: 0,
        };
        // 自带网格句柄：本方法只拿得到 &self，不能借用 self.grid_ref
        for y in 0..self.rows {
            for x in 0..self.cols {
                let point = GhosttyPoint {
                    tag: POINT_TAG_VIEWPORT,
                    _pad: 0,
                    value: GhosttyPointValue {
                        coordinate: GhosttyPointCoordinate { x, y: y as u32 },
                    },
                };
                grid_ref.size = std::mem::size_of::<GhosttyGridRef>() as u64;
                grid_ref.node = std::ptr::null_mut();
                let r = unsafe { ghostty_terminal_grid_ref(self.handle, &point, &mut grid_ref) };
                if r != 0 {
                    continue;
                }
                let mut packed: u64 = 0;
                if unsafe { ghostty_grid_ref_cell(&grid_ref, &mut packed) } != 0 {
                    continue;
                }
                let content_tag = packed & CELL_CONTENT_TAG_MASK;
                if content_tag != 0 && content_tag != 1 {
                    continue;
                }
                let codepoint = ((packed >> CELL_CODEPOINT_SHIFT) & CELL_CODEPOINT_MASK) as u32;
                if codepoint != PLACEHOLDER_CODEPOINT {
                    continue;
                }
                let mut style = GhosttyStyle {
                    size: std::mem::size_of::<GhosttyStyle>() as u64,
                    ..GhosttyStyle::default()
                };
                if unsafe { ghostty_grid_ref_style(&grid_ref, &mut style) } != 0 {
                    continue;
                }
                let Some(image_id) = self.resolve_color(&style.fg_color) else {
                    continue;
                };
                if !wanted.contains(&image_id) {
                    continue;
                }
                match out.get_mut(&image_id) {
                    Some(span) => {
                        span.min_x = span.min_x.min(x as i32);
                        span.min_y = span.min_y.min(y as i32);
                        span.max_x = span.max_x.max(x as i32);
                        span.max_y = span.max_y.max(y as i32);
                    }
                    None => {
                        out.insert(
                            image_id,
                            PlaceholderSpan {
                                min_x: x as i32,
                                min_y: y as i32,
                                max_x: x as i32,
                                max_y: y as i32,
                            },
                        );
                    }
                }
            }
        }
        out
    }

    fn image_meta(&self, graphics: *mut c_void, image_id: u32) -> Option<KittyImageDto> {
        let image = unsafe { ghostty_kitty_graphics_image(graphics, image_id) };
        if image.is_null() {
            return None;
        }
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        let mut generation: u64 = 0;
        unsafe {
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_WIDTH,
                &mut width as *mut u32 as *mut c_void,
            );
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_HEIGHT,
                &mut height as *mut u32 as *mut c_void,
            );
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_GENERATION,
                &mut generation as *mut u64 as *mut c_void,
            );
        }
        Some(KittyImageDto {
            id: image_id,
            width,
            height,
            generation,
        })
    }

    /// 取一张图片的 RGBA 像素（前端只在缓存缺失时拉一次）。
    ///
    /// 引擎保证存进来的数据已解码且无压缩；这里只做格式归一（RGB→RGBA）。
    pub fn image_rgba(&self, image_id: u32) -> Option<(u32, u32, Vec<u8>)> {
        let graphics = self.kitty_graphics();
        if graphics.is_null() {
            return None;
        }
        let image = unsafe { ghostty_kitty_graphics_image(graphics, image_id) };
        if image.is_null() {
            return None;
        }
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        let mut format: u32 = 0;
        let mut ptr: *const u8 = std::ptr::null();
        let mut len: usize = 0;
        unsafe {
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_WIDTH,
                &mut width as *mut u32 as *mut c_void,
            );
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_HEIGHT,
                &mut height as *mut u32 as *mut c_void,
            );
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_FORMAT,
                &mut format as *mut u32 as *mut c_void,
            );
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_DATA_PTR,
                &mut ptr as *mut *const u8 as *mut c_void,
            );
            ghostty_kitty_graphics_image_get(
                image,
                IMAGE_DATA_DATA_LEN,
                &mut len as *mut usize as *mut c_void,
            );
        }
        if ptr.is_null() || len == 0 || width == 0 || height == 0 {
            return None;
        }
        let raw = unsafe { std::slice::from_raw_parts(ptr, len) };
        let pixels = match format {
            IMAGE_FORMAT_RGBA => raw.to_vec(),
            IMAGE_FORMAT_RGB => {
                let mut v = Vec::with_capacity(raw.len() / 3 * 4);
                for px in raw.chunks_exact(3) {
                    v.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                v
            }
            IMAGE_FORMAT_GRAY => raw.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
            IMAGE_FORMAT_GRAY_ALPHA => raw
                .chunks_exact(2)
                .flat_map(|px| [px[0], px[0], px[0], px[1]])
                .collect(),
            _ => return None,
        };
        Some((width, height, pixels))
    }
}

impl NativeTerminal {
    pub fn new(cols: u16, rows: u16) -> Result<Self, String> {
        let mut handle: *mut c_void = std::ptr::null_mut();
        let rc = unsafe { ghostty_terminal_new(std::ptr::null(), &mut handle, cols, rows) };
        if rc != 0 || handle.is_null() {
            return Err(crate::error_proto::coded_display(
                "ffi_terminal_new_failed",
                rc,
            ));
        }

        let mut term = Self {
            handle,
            cols,
            rows,
            palette: vec![0u8; 256 * 3],
            palette_ready: false,
            grid_ref: GhosttyGridRef {
                size: std::mem::size_of::<GhosttyGridRef>() as u64,
                node: std::ptr::null_mut(),
                x: 0,
                y: 0,
            },
            style: GhosttyStyle::default(),
            key_encoder: std::ptr::null_mut(),
            key_event: std::ptr::null_mut(),
            mouse_encoder: std::ptr::null_mut(),
            mouse_event: std::ptr::null_mut(),
            render_state: std::ptr::null_mut(),
            search: std::ptr::null_mut(),
            search_needle: String::new(),
            snapshot_cells: Vec::new(),
            snapshot_packed: Vec::new(),
            snapshot_out: Vec::new(),
            snapshot_clusters: Vec::new(),
            snapshot_row_flags: Vec::new(),
            diff_out: Vec::new(),
            diff_scratch: DiffScratch::default(),
            last_scroll_offset: 0,
            snapshot_styles: StyleTable::default(),
            ctx: Box::new(CallbackCtx {
                responses: Vec::new(),
                pwd: None,
                pwd_dirty: false,
                cols,
                rows,
                cell_width_px: 0,
                cell_height_px: 0,
            }),
        };
        // 终端内联图片：不打开图形存储，协议里的图片会被引擎直接拒绝
        term.enable_kitty_graphics();
        // 查询回包通道：不开的话终端对一切能力探测装聋（见 collect_query_response）
        term.enable_query_responses();
        // scrollback 上限：**行数与字节配额都要设**。引擎默认的字节配额只有
        // 10000，单独调行数完全不起作用（实测 30 万行只剩 877 行可回看），
        // 表现为「TUI 输出一多就翻不回去」。数值依据见常量注释。
        // 失败只记日志：上限是保护项，没设上也不该让终端创建失败。
        unsafe {
            let lines = SCROLLBACK_MAX_LINES;
            if ghostty_terminal_set(
                term.handle,
                OPT_SCROLLBACK_MAX_LINES,
                &lines as *const usize as *const c_void,
            ) != 0
            {
                log::warn!("设置 scrollback 行数上限失败：历史输出将按引擎默认值截断");
            }
            let bytes = SCROLLBACK_MAX_BYTES;
            if ghostty_terminal_set(
                term.handle,
                OPT_SCROLLBACK_MAX_BYTES,
                &bytes as *const usize as *const c_void,
            ) != 0
            {
                log::warn!("设置 scrollback 字节配额失败：历史输出将按引擎默认值截断");
            }
        }
        Ok(term)
    }

    /// 解析一段 VT 字节流。PTY 可能在多字节 UTF-8 或转义序列中间切分，
    /// 引擎负责跨调用拼合，因此这里必须原样传入、不做任何解码。
    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        unsafe { ghostty_terminal_vt_write(self.handle, bytes.as_ptr(), bytes.len()) };
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) -> Result<(), String> {
        let rc = unsafe { ghostty_terminal_resize(self.handle, cols, rows, cell_w, cell_h) };
        if rc != 0 {
            return Err(crate::error_proto::coded_display(
                "ffi_terminal_resize_failed",
                rc,
            ));
        }
        self.cols = cols;
        self.rows = rows;
        // 尺寸报告必须与终端实际几何一致（TUI 用 CSI 16 t 拿格子像素算图片布局）
        self.ctx.cols = cols;
        self.ctx.rows = rows;
        self.ctx.cell_width_px = cell_w;
        self.ctx.cell_height_px = cell_h;
        Ok(())
    }

    fn get_u16(&self, key: u32) -> Option<u16> {
        let mut out: u16 = 0;
        let rc =
            unsafe { ghostty_terminal_get(self.handle, key, &mut out as *mut u16 as *mut c_void) };
        (rc == 0).then_some(out)
    }

    /// 应用是否请求了鼠标跟踪（DEC 1000/1002/1003 任一）。为真时滚轮必须转发给
    /// PTY 程序由它自己滚动 —— 这类 TUI 通常还占着备用屏，而备用屏没有本地
    /// scrollback，本地回滚等于什么都不做。
    pub fn mouse_tracking(&self) -> bool {
        self.read_flag(DATA_MOUSE_TRACKING)
    }

    /// 当前是否处于备用屏（alt screen）。
    pub fn alt_screen(&self) -> bool {
        let mut out: u32 = 0;
        let rc = unsafe {
            ghostty_terminal_get(
                self.handle,
                DATA_ACTIVE_SCREEN,
                &mut out as *mut u32 as *mut c_void,
            )
        };
        rc == 0 && out == 1
    }

    /// 查询 DEC/ANSI 模式当前值。输入输出是 `GhosttyTerminalModeConfig`，
    /// 使用前必须填好要查的模式号，否则引擎返回 GHOSTTY_INVALID_VALUE。
    pub fn mode_enabled(&self, mode: u16) -> bool {
        #[repr(C)]
        struct ModeConfig {
            mode: u16,
            value: bool,
        }
        let mut cfg = ModeConfig { mode, value: false };
        let rc = unsafe {
            ghostty_terminal_get(
                self.handle,
                DATA_MODE,
                &mut cfg as *mut ModeConfig as *mut c_void,
            )
        };
        rc == 0 && cfg.value
    }

    /// 回滚历史当前保留的行数（不含视口）。
    pub fn scrollback_rows(&self) -> usize {
        let mut out: usize = 0;
        let rc = unsafe {
            ghostty_terminal_get(
                self.handle,
                DATA_SCROLLBACK_ROWS,
                &mut out as *mut usize as *mut c_void,
            )
        };
        if rc == 0 {
            out
        } else {
            0
        }
    }

    /// 运行时调整滚动历史字节配额（供全局内存预算使用）。
    ///
    /// 返回是否成功。失败只意味着配额没变，终端仍可用；调用方应保留原配额
    /// 继续工作，不要把失败当成终端故障。收缩后引擎会按页裁剪历史。
    pub fn set_scrollback_bytes(&mut self, bytes: usize) -> bool {
        let rc = unsafe {
            ghostty_terminal_set(
                self.handle,
                OPT_SCROLLBACK_MAX_BYTES,
                &bytes as *const usize as *const c_void,
            )
        };
        rc == 0
    }

    /// 设置终端默认前景/背景/光标色（0xRRGGBB），让原生 VT 的默认色与
    /// 当前 IDE 主题一致，而不是停在引擎内置的 One Dark。OSC 10/11/12
    /// 后续覆盖仍然优先（由引擎处理）。
    pub fn set_default_colors(&mut self, fg: u32, bg: u32, cursor: u32) -> Result<(), String> {
        let to_rgb = |v: u32| GhosttyColorRgb {
            r: ((v >> 16) & 0xff) as u8,
            g: ((v >> 8) & 0xff) as u8,
            b: (v & 0xff) as u8,
        };
        for (key, value) in [
            (OPT_COLOR_FOREGROUND, fg),
            (OPT_COLOR_BACKGROUND, bg),
            (OPT_COLOR_CURSOR, cursor),
        ] {
            let rgb = to_rgb(value);
            let rc = unsafe {
                ghostty_terminal_set(
                    self.handle,
                    key,
                    &rgb as *const GhosttyColorRgb as *const c_void,
                )
            };
            if rc != 0 {
                return Err(crate::error_proto::coded_display(
                    "ffi_set_color_failed",
                    rc,
                ));
            }
        }
        Ok(())
    }

    /// 把一次滚轮增量编码成鼠标序列（按终端当前的跟踪模式与输出格式）。
    /// `delta` < 0 = 向上（按钮 4），> 0 = 向下（按钮 5）。未开启鼠标跟踪、
    /// 或编码失败时返回空 —— 调用方据此回退到本地回滚。
    pub fn encode_wheel(&mut self, delta: i64, col: u16, row: u16, mods: u16) -> Vec<u8> {
        if delta == 0 {
            return Vec::new();
        }
        if self.mouse_encoder.is_null() {
            let rc =
                unsafe { ghostty_mouse_encoder_new(std::ptr::null(), &mut self.mouse_encoder) };
            if rc != 0 || self.mouse_encoder.is_null() {
                log::warn!("鼠标编码器创建失败：滚轮无法转发给 PTY 程序");
                return Vec::new();
            }
        }
        if self.mouse_event.is_null() {
            let rc = unsafe { ghostty_mouse_event_new(std::ptr::null(), &mut self.mouse_event) };
            if rc != 0 || self.mouse_event.is_null() {
                log::warn!("鼠标事件创建失败：滚轮无法转发给 PTY 程序");
                return Vec::new();
            }
        }
        let button = if delta < 0 {
            MOUSE_BUTTON_FOUR
        } else {
            MOUSE_BUTTON_FIVE
        };
        // 像素几何：1016（SGR-Pixels）需要真像素，其余格式由引擎按同一份
        // size 折算格坐标。resize 之前 cell 尺寸为 0，此时跳过 SIZE 选项
        // （1016 会编码失败，其余格式照常工作）。
        let cw = self.ctx.cell_width_px;
        let ch = self.ctx.cell_height_px;
        let has_size = cw > 0 && ch > 0;
        let size = GhosttyMouseEncoderSize::new(
            cw.saturating_mul(self.cols as u32),
            ch.saturating_mul(self.rows as u32),
            cw,
            ch,
        );
        let position = GhosttyMousePosition {
            // 取单元格中心：引擎折算回格坐标时不会落到边界外
            x: col as f32 * cw as f32 + cw as f32 / 2.0,
            y: row as f32 * ch as f32 + ch as f32 / 2.0,
        };
        let mut out = Vec::new();
        unsafe {
            // 每次编码前从终端同步一次：跟踪模式与输出格式随时可能被 TUI 改掉
            ghostty_mouse_encoder_setopt_from_terminal(self.mouse_encoder, self.handle);
            if has_size {
                ghostty_mouse_encoder_setopt(
                    self.mouse_encoder,
                    MOUSE_ENCODER_OPT_SIZE,
                    &size as *const GhosttyMouseEncoderSize as *const c_void,
                );
            }
            ghostty_mouse_event_set_action(self.mouse_event, MOUSE_ACTION_PRESS);
            ghostty_mouse_event_set_button(self.mouse_event, button);
            ghostty_mouse_event_set_mods(self.mouse_event, mods);
            ghostty_mouse_event_set_position(self.mouse_event, position);
            let mut buf = [0u8; 64];
            for _ in 0..wheel_event_count(delta) {
                let mut len = 0usize;
                let rc = ghostty_mouse_encoder_encode(
                    self.mouse_encoder,
                    self.mouse_event,
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut len,
                );
                // 模式不允许该事件（未开跟踪等）时引擎返回成功但零字节
                if rc != 0 || len == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..len.min(buf.len())]);
            }
        }
        out
    }

    /// 把鼠标按下/抬起/移动编码成终端鼠标序列。
    ///
    /// 与滚轮共用 encoder/event；未开启鼠标跟踪或格式不支持时返回空，
    /// 调用方据此回退到本地选区。
    pub fn encode_mouse(
        &mut self,
        action: u32,
        button: u32,
        col: u16,
        row: u16,
        mods: u16,
    ) -> Vec<u8> {
        if action > MOUSE_ACTION_MOTION {
            return Vec::new();
        }
        if self.mouse_encoder.is_null() {
            let rc =
                unsafe { ghostty_mouse_encoder_new(std::ptr::null(), &mut self.mouse_encoder) };
            if rc != 0 || self.mouse_encoder.is_null() {
                return Vec::new();
            }
        }
        if self.mouse_event.is_null() {
            let rc = unsafe { ghostty_mouse_event_new(std::ptr::null(), &mut self.mouse_event) };
            if rc != 0 || self.mouse_event.is_null() {
                return Vec::new();
            }
        }
        let cw = self.ctx.cell_width_px;
        let ch = self.ctx.cell_height_px;
        let has_size = cw > 0 && ch > 0;
        let size = GhosttyMouseEncoderSize::new(
            cw.saturating_mul(self.cols as u32),
            ch.saturating_mul(self.rows as u32),
            cw,
            ch,
        );
        let position = GhosttyMousePosition {
            x: col as f32 * cw as f32 + cw as f32 / 2.0,
            y: row as f32 * ch as f32 + ch as f32 / 2.0,
        };
        let mut buf = [0u8; 64];
        let mut len = 0usize;
        unsafe {
            ghostty_mouse_encoder_setopt_from_terminal(self.mouse_encoder, self.handle);
            if has_size {
                ghostty_mouse_encoder_setopt(
                    self.mouse_encoder,
                    MOUSE_ENCODER_OPT_SIZE,
                    &size as *const GhosttyMouseEncoderSize as *const c_void,
                );
            }
            ghostty_mouse_event_set_action(self.mouse_event, action);
            ghostty_mouse_event_set_button(self.mouse_event, button);
            ghostty_mouse_event_set_mods(self.mouse_event, mods);
            ghostty_mouse_event_set_position(self.mouse_event, position);
            let rc = ghostty_mouse_encoder_encode(
                self.mouse_encoder,
                self.mouse_event,
                buf.as_mut_ptr(),
                buf.len(),
                &mut len,
            );
            if rc != 0 || len == 0 {
                return Vec::new();
            }
        }
        buf[..len.min(buf.len())].to_vec()
    }

    /// 备用屏 + 1007（备用屏滚轮转方向键）时，把滚轮增量编码成上下方向键。
    /// 走键盘编码器：应用光标键模式（DECCKM）与 kitty 协议标志由引擎决定。
    pub fn encode_arrow_scroll(&mut self, delta: i64) -> Vec<u8> {
        if delta == 0 {
            return Vec::new();
        }
        let key = if delta < 0 {
            KEY_ARROW_UP
        } else {
            KEY_ARROW_DOWN
        };
        let mut out = Vec::new();
        for _ in 0..wheel_event_count(delta) {
            match self.encode_key(&KeyInput {
                key,
                mods: 0,
                action: key_action::PRESS,
                utf8: None,
                unshifted_codepoint: 0,
                composing: false,
            }) {
                Ok(bytes) => out.extend_from_slice(&bytes),
                Err(e) => {
                    log::warn!("方向键编码失败：滚轮无法转发给备用屏程序（{e}）");
                    break;
                }
            }
        }
        out
    }

    /// 处理一次滚轮：需要转发时返回写给 PTY 的字节（鼠标序列或方向键），
    /// 否则就地回滚本地视口并返回 `None`。
    ///
    /// 优先级：程序开了鼠标跟踪 → 鼠标协议（它自己滚）；备用屏 + DEC 1007 →
    /// 上下方向键；其余 → 本地回滚。**备用屏必须走前两条**：alt 屏没有本地
    /// scrollback，本地回滚在 alt 屏里恒为空（这正是「TUI 里滚不回去」的根因）。
    pub fn scroll_wheel(&mut self, delta: i64, col: u16, row: u16, mods: u16) -> Option<Vec<u8>> {
        if delta == 0 {
            return None;
        }
        if self.mouse_tracking() {
            let bytes = self.encode_wheel(delta, col, row, mods);
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
        if self.alt_screen() && self.mode_enabled(MODE_ALT_SCROLL) {
            let bytes = self.encode_arrow_scroll(delta);
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
        self.scroll_delta(delta);
        None
    }

    pub fn cols(&self) -> u16 {
        self.get_u16(DATA_COLS).unwrap_or(self.cols)
    }

    pub fn rows(&self) -> u16 {
        self.get_u16(DATA_ROWS).unwrap_or(self.rows)
    }

    pub fn cursor(&self) -> (u16, u16) {
        (
            self.get_u16(DATA_CURSOR_X).unwrap_or(0),
            self.get_u16(DATA_CURSOR_Y).unwrap_or(0),
        )
    }

    /// 读取布尔类数据键
    fn read_flag(&self, key: u32) -> bool {
        let mut out = false;
        let rc =
            unsafe { ghostty_terminal_get(self.handle, key, &mut out as *mut bool as *mut c_void) };
        rc == 0 && out
    }

    /// 滚动条状态（total / offset / len）。三个 u64 由引擎内联写入。
    fn read_scrollbar(&self) -> GhosttyTerminalScrollbar {
        let mut sb = GhosttyTerminalScrollbar::default();
        let rc = unsafe {
            ghostty_terminal_get(
                self.handle,
                DATA_SCROLLBAR,
                &mut sb as *mut GhosttyTerminalScrollbar as *mut c_void,
            )
        };
        if rc == 0 {
            sb
        } else {
            GhosttyTerminalScrollbar::default()
        }
    }

    /// 按视口坐标取 grid ref（选区与单元格读取共用）
    fn grid_ref_at(&self, x: u16, y: u16) -> Option<GhosttyGridRef> {
        let point = GhosttyPoint {
            tag: POINT_TAG_VIEWPORT,
            _pad: 0,
            value: GhosttyPointValue {
                coordinate: GhosttyPointCoordinate { x, y: y as u32 },
            },
        };
        let mut reference = GhosttyGridRef {
            size: std::mem::size_of::<GhosttyGridRef>() as u64,
            node: std::ptr::null_mut(),
            x: 0,
            y: 0,
        };
        let rc = unsafe { ghostty_terminal_grid_ref(self.handle, &point, &mut reference) };
        (rc == 0).then_some(reference)
    }

    /// 视口行的语义提示标记 + 「本行是否有字素簇」标记。
    ///
    /// 行句柄只能从 grid ref 拿：`ghostty_grid_ref_row` 给出不透明 u64，
    /// 再用 `ghostty_row_get` 读字段。同一行的任意列都能取到同一句柄，
    /// 这里固定用第 0 列。
    ///
    /// 第二个返回值让调用方在「整行都没有字素簇」时跳过逐格的
    /// `grid_ref_graphemes`（绝大多数行都是这种情况）。
    fn row_info(&self, y: u16) -> (u8, bool) {
        let Some(reference) = self.grid_ref_at(0, y) else {
            return (0, false);
        };
        let mut row: u64 = 0;
        if unsafe { ghostty_grid_ref_row(&reference, &mut row) } != 0 {
            return (0, false);
        }

        let mut prompt: i32 = 0;
        let prompt_ok = unsafe {
            ghostty_row_get(
                row,
                ROW_DATA_SEMANTIC_PROMPT,
                &mut prompt as *mut i32 as *mut c_void,
            )
        } == 0;
        let flags = if prompt_ok {
            match prompt {
                ROW_SEMANTIC_PROMPT => row_flags::PROMPT,
                ROW_SEMANTIC_PROMPT_CONTINUATION => row_flags::PROMPT_CONTINUATION,
                _ => 0,
            }
        } else {
            0
        };

        let mut grapheme = false;
        let grapheme_ok = unsafe {
            ghostty_row_get(
                row,
                ROW_DATA_GRAPHEME,
                &mut grapheme as *mut bool as *mut c_void,
            )
        } == 0;
        (flags, grapheme_ok && grapheme)
    }

    /// 设置选区（拖动选择/双击选词等都由前端算好端点后传入）。
    pub fn set_selection(
        &mut self,
        start_x: u16,
        start_y: u16,
        end_x: u16,
        end_y: u16,
        rectangle: bool,
    ) -> Result<(), String> {
        let start = self
            .grid_ref_at(start_x, start_y)
            .ok_or_else(|| format!("选区起点越界: ({start_x},{start_y})"))?;
        let end = self
            .grid_ref_at(end_x, end_y)
            .ok_or_else(|| format!("选区终点越界: ({end_x},{end_y})"))?;

        let selection = GhosttySelection {
            size: std::mem::size_of::<GhosttySelection>() as u64,
            start,
            end,
            rectangle,
            _pad: [0; 7],
        };

        let rc = unsafe {
            ghostty_terminal_set(
                self.handle,
                OPT_SELECTION,
                &selection as *const GhosttySelection as *const c_void,
            )
        };
        if rc != 0 {
            return Err(crate::error_proto::coded_display(
                "ffi_set_selection_failed",
                rc,
            ));
        }
        Ok(())
    }

    /// 清除选区。传 NULL 即为清除，无需构造空选区。
    pub fn clear_selection(&mut self) -> Result<(), String> {
        let rc = unsafe { ghostty_terminal_set(self.handle, OPT_SELECTION, std::ptr::null()) };
        if rc != 0 {
            return Err(crate::error_proto::coded_display(
                "ffi_clear_selection_failed",
                rc,
            ));
        }
        Ok(())
    }

    /// 把「选区快照」装成当前选区，并返回端点（视口坐标，闭区间）。
    /// 端点顺序按选区方向保留（可能反向），交给前端绘制时自行归一化。
    fn install_selection(
        &mut self,
        selection: &GhosttySelection,
    ) -> Result<(u16, u16, u16, u16), String> {
        let rc = unsafe {
            ghostty_terminal_set(
                self.handle,
                OPT_SELECTION,
                selection as *const GhosttySelection as *const c_void,
            )
        };
        if rc != 0 {
            return Err(crate::error_proto::coded_display(
                "ffi_set_selection_failed",
                rc,
            ));
        }
        Ok((
            selection.start.x,
            selection.start.y,
            selection.end.x,
            selection.end.y,
        ))
    }

    /// 取点击落点所属字符（跳过宽字符尾格）：返回 (起始列, 码点)
    fn word_char_at(&self, x: u16, y: u16) -> Option<(u16, u32)> {
        let mut c = x;
        loop {
            if let Some(cp) = self.cell_codepoint(c, y) {
                return Some((c, cp));
            }
            if c == 0 {
                return None;
            }
            c -= 1;
        }
    }

    /// 取某格的首码点（空单元格返回 None）
    fn cell_codepoint(&self, x: u16, y: u16) -> Option<u32> {
        let reference = self.grid_ref_at(x, y)?;
        let mut buf = [0u32; 4];
        let mut len = 0usize;
        let rc = unsafe {
            ghostty_grid_ref_graphemes(&reference, buf.as_mut_ptr(), buf.len(), &mut len)
        };
        if rc != 0 || len == 0 {
            return None;
        }
        Some(buf[0])
    }

    /// CJK 回退扫描：引擎的词边界不认 CJK 字符（中文上直接返回「无可选内容」），
    /// 这里按「非空白、非标点」的连续区间取一段词。英文/代码路径仍走引擎规则。
    ///
    /// 宽度处理：宽字符（中文）占两格，第二格是没有码点的尾格 —— 先把整行摊平成
    /// 「字符 → 起始列」，扫描时跨过尾格，选区终点取到该字符的尾格，
    /// 高亮才不会只盖住半个字。
    fn scan_cjk_word(&mut self, x: u16, y: u16) -> Result<Option<(u16, u16, u16, u16)>, String> {
        let cols = self.cols;
        let mut chars: Vec<(u16, u32)> = Vec::with_capacity(cols as usize);
        for c in 0..cols {
            if let Some(cp) = self.cell_codepoint(c, y) {
                chars.push((c, cp));
            }
        }
        // 点击落点所属字符：起始列不超过点击列的那个（点在宽字符尾格时归到该字符）
        let idx = match chars.iter().rposition(|(c, _)| *c <= x) {
            Some(i) => i,
            None => return Ok(None),
        };
        if !is_word_char(chars[idx].1) {
            return Ok(None);
        }
        let mut lo = idx;
        while lo > 0 && is_word_char(chars[lo - 1].1) {
            lo -= 1;
        }
        let mut hi = idx;
        while hi + 1 < chars.len() && is_word_char(chars[hi + 1].1) {
            hi += 1;
        }
        let start = chars[lo].0;
        // 终点 = 下一个字符之前的最后一格（含宽字符尾格）
        let end = if hi + 1 < chars.len() {
            chars[hi + 1].0.saturating_sub(1)
        } else {
            cols.saturating_sub(1)
        };
        self.set_selection(start, y, end, y, false)?;
        Ok(Some((start, y, end, y)))
    }

    /// 双击选词：由引擎按词边界（Unicode 词边界规则）算出区间并装入。
    /// 返回端点供前端绘制高亮；点在空白等无可选内容处返回 None。
    pub fn select_word(&mut self, x: u16, y: u16) -> Result<Option<(u16, u16, u16, u16)>, String> {
        // 中文（CJK）不走引擎：引擎把每个汉字当成独立的「词」，双击只能选到一个字。
        // 中文习惯是取一段连续的字（词语），改由本地扫描给出区间。
        if let Some((_, cp)) = self.word_char_at(x, y) {
            if is_cjk_codepoint(cp) {
                return self.scan_cjk_word(x, y);
            }
        }
        let reference = match self.grid_ref_at(x, y) {
            Some(r) => r,
            None => return Ok(None),
        };
        let options = GhosttyTerminalSelectWordOptions {
            size: std::mem::size_of::<GhosttyTerminalSelectWordOptions>() as u64,
            grid_ref: reference,
            boundary_codepoints: std::ptr::null(),
            boundary_codepoints_len: 0,
        };
        let mut selection: GhosttySelection = unsafe { std::mem::zeroed() };
        selection.size = std::mem::size_of::<GhosttySelection>() as u64;
        let rc = unsafe { ghostty_terminal_select_word(self.handle, &options, &mut selection) };
        // -4 = GHOSTTY_NO_VALUE：引擎认为该处没有可选词。中文（CJK）字符不在
        // 引擎的词字符集里，会走到这条分支 —— 交给本地扫描兜底，
        // 保证「双击选一个词语」在中文下同样可用。
        if rc == -4 {
            return self.scan_cjk_word(x, y);
        }
        if rc != 0 {
            return Err(crate::error_proto::coded_display(
                "ffi_word_select_failed",
                rc,
            ));
        }
        self.install_selection(&selection).map(Some)
    }

    /// 三击选整行：由引擎按行内容（含语义提示边界）算出区间并装入。
    pub fn select_line(&mut self, y: u16) -> Result<Option<(u16, u16, u16, u16)>, String> {
        // 行选择只看行号，取该行首列作为参照
        let reference = match self.grid_ref_at(0, y) {
            Some(r) => r,
            None => return Ok(None),
        };
        let options = GhosttyTerminalSelectLineOptions {
            size: std::mem::size_of::<GhosttyTerminalSelectLineOptions>() as u64,
            grid_ref: reference,
            whitespace: std::ptr::null(),
            whitespace_len: 0,
            semantic_prompt_boundary: true,
        };
        let mut selection: GhosttySelection = unsafe { std::mem::zeroed() };
        selection.size = std::mem::size_of::<GhosttySelection>() as u64;
        let rc = unsafe { ghostty_terminal_select_line(self.handle, &options, &mut selection) };
        if rc == -4 {
            return Ok(None);
        }
        if rc != 0 {
            return Err(crate::error_proto::coded_display(
                "ffi_line_select_failed",
                rc,
            ));
        }
        self.install_selection(&selection).map(Some)
    }

    /// 取选区文本（纯文本、去除尾随空白）。无选区时返回空串。
    pub fn selected_text(&mut self) -> Result<String, String> {
        let options = GhosttyTerminalSelectionFormatOptions {
            size: std::mem::size_of::<GhosttyTerminalSelectionFormatOptions>() as u64,
            emit: FORMATTER_FORMAT_PLAIN,
            unwrap: false,
            trim: true,
            _pad: [0; 2],
            selection: std::ptr::null(),
        };

        // 选区可能很长（整屏 + 滚动历史），先按 8 KB 试，不够则倍增
        let mut cap = 8192usize;
        loop {
            let mut buf = vec![0u8; cap];
            let mut written: usize = 0;
            let rc = unsafe {
                ghostty_terminal_selection_format_buf(
                    self.handle,
                    options,
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut written,
                )
            };
            // GHOSTTY_OUT_OF_SPACE(-3) 时 out_written 是所需长度
            if rc == -3 {
                if written > cap && written <= 4 * 1024 * 1024 {
                    cap = written;
                    continue;
                }
                if cap >= 4 * 1024 * 1024 {
                    return Err("选区文本超过 4 MB".to_string());
                }
                cap *= 2;
                continue;
            }
            if rc == -4 {
                // GHOSTTY_NO_VALUE：无活动选区
                return Ok(String::new());
            }
            if rc != 0 {
                return Err(crate::error_proto::coded_display(
                    "ffi_read_selection_failed",
                    rc,
                ));
            }
            buf.truncate(written);
            return String::from_utf8(buf).map_err(|e| format!("选区文本非 UTF-8: {e}"));
        }
    }

    /// 同步渲染状态并返回句柄。每次取快照前调用一次。
    fn sync_render_state(&mut self) -> Option<*mut c_void> {
        if self.render_state.is_null() {
            let rc = unsafe { ghostty_render_state_new(std::ptr::null(), &mut self.render_state) };
            if rc != 0 || self.render_state.is_null() {
                return None;
            }
        }
        let rc = unsafe { ghostty_render_state_update(self.render_state, self.handle) };
        (rc == 0).then_some(self.render_state)
    }

    fn rs_flag(&self, state: *mut c_void, key: u32) -> bool {
        let mut out = false;
        let rc =
            unsafe { ghostty_render_state_get(state, key, &mut out as *mut bool as *mut c_void) };
        rc == 0 && out
    }

    fn rs_u32(&self, state: *mut c_void, key: u32) -> Option<u32> {
        let mut out: u32 = 0;
        let rc =
            unsafe { ghostty_render_state_get(state, key, &mut out as *mut u32 as *mut c_void) };
        (rc == 0).then_some(out)
    }

    /// 同 `read_rgb`，但走渲染状态
    fn rs_rgb(&self, state: *mut c_void, key: u32) -> Option<u32> {
        let mut out = [0u8; 4];
        let rc = unsafe { ghostty_render_state_get(state, key, out.as_mut_ptr() as *mut c_void) };
        if rc != 0 {
            return None;
        }
        Some(((out[0] as u32) << 16) | ((out[1] as u32) << 8) | out[2] as u32)
    }

    /// 在滚动历史中搜索。返回匹配总数与当前选中项序号。
    ///
    /// `select` 决定是否移动选中位置。搜索对象跨调用存活，
    /// 因此连续 `Next` 会依次走到各匹配；换 needle 时重建。
    pub fn search(
        &mut self,
        needle: &str,
        select: Option<SearchSelect>,
    ) -> Result<SearchResult, String> {
        if needle.is_empty() {
            self.discard_search();
            return Ok(SearchResult {
                total: 0,
                selected: None,
                selection: None,
            });
        }

        // needle 变化则重建：旧结果无从复用
        if self.search.is_null() || self.search_needle != needle {
            self.discard_search();
            let mut fresh: *mut c_void = std::ptr::null_mut();
            let rc = unsafe { ghostty_search_new(std::ptr::null(), &mut fresh, self.handle) };
            if rc != 0 || fresh.is_null() {
                return Err(crate::error_proto::coded_display(
                    "ffi_search_create_failed",
                    rc,
                ));
            }
            self.search = fresh;
            self.search_needle = needle.to_string();

            let needle_str = GhosttyString {
                ptr: needle.as_ptr(),
                len: needle.len(),
            };
            let rc = unsafe {
                ghostty_search_set(
                    self.search,
                    SEARCH_OPT_NEEDLE,
                    &needle_str as *const GhosttyString as *const c_void,
                )
            };
            if rc != 0 {
                self.discard_search();
                return Err(crate::error_proto::coded_display(
                    "ffi_search_set_term_failed",
                    rc,
                ));
            }
        }

        // run = feed + tick 直到追平终端（阻塞，调用方在命令线程上）
        let rc = unsafe { ghostty_search_run(self.search) };
        if rc != 0 {
            return Err(crate::error_proto::coded_display(
                "ffi_search_run_failed",
                rc,
            ));
        }

        match select {
            Some(SearchSelect::Next) => {
                unsafe {
                    ghostty_search_set(self.search, SEARCH_OPT_SELECT_NEXT, std::ptr::null())
                };
            }
            Some(SearchSelect::Prev) => {
                unsafe {
                    ghostty_search_set(self.search, SEARCH_OPT_SELECT_PREV, std::ptr::null())
                };
            }
            None => {}
        }

        let mut total: u64 = 0;
        unsafe {
            ghostty_search_get(
                self.search,
                SEARCH_DATA_TOTAL_MATCHES,
                &mut total as *mut u64 as *mut c_void,
            )
        };

        let mut selected: u64 = 0;
        let rc = unsafe {
            ghostty_search_get(
                self.search,
                SEARCH_DATA_SELECTED_INDEX,
                &mut selected as *mut u64 as *mut c_void,
            )
        };

        let mut match_selection: GhosttySelection = unsafe { std::mem::zeroed() };
        match_selection.size = std::mem::size_of::<GhosttySelection>() as u64;
        let sel_rc = unsafe {
            ghostty_search_get(
                self.search,
                SEARCH_DATA_SELECTED_MATCH,
                &mut match_selection as *mut GhosttySelection as *mut c_void,
            )
        };
        // SEARCH_DATA_SELECTED_MATCH 的两端是 grid ref 的页内坐标，不是
        // 视口坐标。直接当视口用会把高亮画到无关行（搜索滚到 scrollback
        // 后尤其明显）。按 ghostty 文档换算成 VIEWPORT；任一端在视口外
        // （例如引擎选择不滚动）就不画高亮，而不是画错位置。
        let mut view_start = GhosttyPointCoordinate::default();
        let mut view_end = GhosttyPointCoordinate::default();
        let start_ok = sel_rc == 0
            && unsafe {
                ghostty_terminal_point_from_grid_ref(
                    self.handle,
                    &match_selection.start,
                    POINT_TAG_VIEWPORT,
                    &mut view_start,
                )
            } == 0;
        let end_ok = start_ok
            && unsafe {
                ghostty_terminal_point_from_grid_ref(
                    self.handle,
                    &match_selection.end,
                    POINT_TAG_VIEWPORT,
                    &mut view_end,
                )
            } == 0;
        let selection = (start_ok && end_ok).then_some(SearchSelection {
            start_x: view_start.x,
            start_y: view_start.y.min(u16::MAX as u32) as u16,
            end_x: view_end.x,
            end_y: view_end.y.min(u16::MAX as u32) as u16,
        });

        Ok(SearchResult {
            total,
            selected: (rc == 0).then_some(selected),
            selection,
        })
    }

    fn discard_search(&mut self) {
        if !self.search.is_null() {
            unsafe { ghostty_search_free(self.search) };
            self.search = std::ptr::null_mut();
        }
        self.search_needle.clear();
    }

    /// 读取终端默认色。`GhosttyColorRgb` = r/g/b 三个字节（无 alpha），
    /// 引擎写入 3 字节；缓冲给 4 字节是超配，只读前 3 字节。
    fn read_rgb(&self, key: u32) -> Option<u32> {
        let mut out = [0u8; 4];
        let rc = unsafe { ghostty_terminal_get(self.handle, key, out.as_mut_ptr() as *mut c_void) };
        if rc != 0 {
            return None;
        }
        Some(((out[0] as u32) << 16) | ((out[1] as u32) << 8) | out[2] as u32)
    }

    /// 刷新 256 色调色板。引擎把 `GhosttyColorRgb[256]` **内联写入**调用方缓冲，
    /// 不是返回指针。
    ///
    /// 768 字节的由来：`GhosttyColorRgb` 为 r/g/b 三个 `uint8_t`（无 alpha），
    /// 256 项 × 3 字节 = 768 字节，恰为 `self.palette` 的容量；
    /// `palette_entry` 按 `index * 3` 取字节，与该布局一一对应。
    fn refresh_palette(&mut self) {
        let rc = unsafe {
            ghostty_terminal_get(
                self.handle,
                21, // GHOSTTY_TERMINAL_DATA_COLOR_PALETTE
                self.palette.as_mut_ptr() as *mut c_void,
            )
        };
        self.palette_ready = rc == 0;
    }

    fn palette_entry(&self, index: u8) -> Option<u32> {
        if !self.palette_ready {
            return None;
        }
        let base = index as usize * 3;
        let (r, g, b) = (
            self.palette[base],
            self.palette[base + 1],
            self.palette[base + 2],
        );
        Some(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    /// 解析一个 `GhosttyStyleColor` 为 0xRRGGBB
    fn resolve_color(&self, color: &GhosttyStyleColor) -> Option<u32> {
        match color.tag {
            STYLE_COLOR_NONE => None,
            STYLE_COLOR_RGB => {
                let bytes = color.value.to_le_bytes();
                Some(((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32)
            }
            STYLE_COLOR_PALETTE => {
                let index = color.value.to_le_bytes()[0];
                self.palette_entry(index)
            }
            _ => None,
        }
    }

    /// 取全屏快照。
    ///
    /// 整个遍历在 Rust 内完成 —— 这正是相对 wasm 版的核心收益：
    /// wasm 版每个单元格都要跨 JS↔wasm 边界 3 次，这里是零跨界。
    ///
    /// cells 走复用缓冲：`clear()` 保留容量，稳态下零分配。
    pub fn snapshot(&mut self) -> Snapshot {
        self.refresh_palette();

        // 渲染状态是光标信息的正确来源。终端数据键 10 是「光标处 SGR 样式」
        // （完整 GhosttyStyle），不是视觉形状；混用会写穿调用方缓冲。
        let rs = self.sync_render_state();
        let cursor_visible = match rs {
            Some(state) => self.rs_flag(state, RS_CURSOR_VISIBLE),
            None => true,
        };
        let cursor_style = match rs {
            Some(state) => self.rs_u32(state, RS_CURSOR_VISUAL_STYLE).unwrap_or(1),
            None => 1,
        };
        let rs_cursor_color = rs.and_then(|state| {
            if self.rs_flag(state, RS_COLOR_CURSOR_HAS_VALUE) {
                self.rs_rgb(state, RS_COLOR_CURSOR)
            } else {
                None
            }
        });

        let cols = self.cols();
        let rows = self.rows();
        self.snapshot_cells.clear();
        self.snapshot_cells.reserve(cols as usize * rows as usize);

        // 字素簇表与行标记缓冲同样跨帧复用（`snapshot_wire` 用完整归还）
        let mut clusters = std::mem::take(&mut self.snapshot_clusters);
        clusters.clear();
        let mut flags_out = std::mem::take(&mut self.snapshot_row_flags);
        flags_out.clear();
        let mut cluster_stack = [0u32; CLUSTER_STACK_CODEPOINTS];
        // 超过栈容量的长簇才会用到这块堆缓冲（极少）
        let mut cluster_heap: Vec<u32> = Vec::new();

        // 样式解析缓存。一屏里同样式连续成段是常态（一段同色文本、
        // 一片空白、一条 prompt），而 `resolve_color` 每次都要做
        // 标签分支 + 调色板索引。把上一次的原始 `GhosttyStyle`
        // 按字节缓存，逐格相同就直接复用已解析结果。
        let mut cached_raw = [0u8; std::mem::size_of::<GhosttyStyle>()];
        let mut cached: (Option<u32>, Option<u32>, bool, bool, bool, bool, bool, bool) =
            (None, None, false, false, false, false, false, false);
        // 只有 sr==0 的格才允许命中缓存；取格失败或样式读取失败时失效
        let mut cache_valid = false;

        for y in 0..rows {
            // 行级标记（OSC 133 语义提示）与「本行是否有字素簇」：后者让
            // 绝大多数行完全跳过逐格的 graphemes FFI。
            let (row_flag, has_grapheme) = self.row_info(y);
            flags_out.push(row_flag);
            for x in 0..cols {
                let point = GhosttyPoint {
                    tag: POINT_TAG_VIEWPORT,
                    _pad: 0,
                    value: GhosttyPointValue {
                        coordinate: GhosttyPointCoordinate { x, y: y as u32 },
                    },
                };

                // 每次调用前重置尺寸头，等价于 C 侧 GHOSTTY_INIT_SIZED
                self.grid_ref.size = std::mem::size_of::<GhosttyGridRef>() as u64;
                self.grid_ref.node = std::ptr::null_mut();

                let r =
                    unsafe { ghostty_terminal_grid_ref(self.handle, &point, &mut self.grid_ref) };
                if r != 0 {
                    self.snapshot_cells.push(CellView::default());
                    cache_valid = false;
                    continue;
                }

                let mut packed: u64 = 0;
                let r = unsafe { ghostty_grid_ref_cell(&self.grid_ref, &mut packed) };
                if r != 0 {
                    self.snapshot_cells.push(CellView::default());
                    cache_valid = false;
                    continue;
                }

                let content_tag = packed & CELL_CONTENT_TAG_MASK;
                let has_text = content_tag == 0 || content_tag == 1;
                let wide = ((packed >> CELL_WIDE_SHIFT) & CELL_WIDE_MASK) as u8;

                self.style = GhosttyStyle::default();
                self.style.size = std::mem::size_of::<GhosttyStyle>() as u64;
                let sr = unsafe { ghostty_grid_ref_style(&self.grid_ref, &mut self.style) };

                let (fg, bg, bold, italic, faint, inverse, invisible, strikethrough) = if sr == 0 {
                    // 与上一格样式逐字节相同则直接复用解析结果
                    let raw = unsafe {
                        std::slice::from_raw_parts(
                            &self.style as *const GhosttyStyle as *const u8,
                            std::mem::size_of::<GhosttyStyle>(),
                        )
                    };
                    if cache_valid && raw == cached_raw.as_slice() {
                        cached
                    } else {
                        let resolved = (
                            self.resolve_color(&self.style.fg_color),
                            self.resolve_color(&self.style.bg_color),
                            self.style.bold,
                            self.style.italic,
                            self.style.faint,
                            self.style.inverse,
                            self.style.invisible,
                            self.style.strikethrough,
                        );
                        cached_raw.copy_from_slice(raw);
                        cached = resolved;
                        cache_valid = true;
                        resolved
                    }
                } else {
                    // 样式读取失败：这一格的样式不可信，缓存一并失效
                    cache_valid = false;
                    (None, None, false, false, false, false, false, false)
                };

                let codepoint = if has_text {
                    ((packed >> CELL_CODEPOINT_SHIFT) & CELL_CODEPOINT_MASK) as u32
                } else {
                    0
                };

                // 完整字素簇：只有「有码点」且整行确有簇时才问引擎。
                // `self.grid_ref` 此刻正指向本格，不需要再定位一次。
                let mut cluster_index = 0u32;
                if codepoint != 0 && has_grapheme {
                    if let Some((len, in_heap)) =
                        read_cluster(&self.grid_ref, &mut cluster_stack, &mut cluster_heap)
                    {
                        if len > 1 {
                            let cps = cluster_slice(&cluster_stack, &cluster_heap, len, in_heap);
                            cluster_index = intern_cluster(&mut clusters, &cluster_utf8(cps));
                        }
                    }
                }

                self.snapshot_cells.push(CellView {
                    codepoint,
                    wide,
                    // 空白格在引擎里是 CODEPOINT 标签 + 码点 0（而非空格），
                    // 因此「是否有可见字形」只能以码点判定；沿用内容标签会让
                    // 每个空白格都带上有字形标记。
                    has_text: codepoint != 0,
                    fg,
                    bg,
                    bold,
                    italic,
                    faint,
                    inverse,
                    invisible,
                    strikethrough,
                    cluster_index,
                });
            }
        }

        let (cursor_x, cursor_y) = self.cursor();
        Snapshot {
            cols,
            rows,
            cursor_x,
            cursor_y,
            default_fg: self.read_rgb(DATA_COLOR_FOREGROUND),
            default_bg: self.read_rgb(DATA_COLOR_BACKGROUND),
            cursor_color: rs_cursor_color.or_else(|| self.read_rgb(DATA_COLOR_CURSOR)),
            cursor_visible,
            cursor_pending_wrap: self.read_flag(5),
            cursor_style,
            scrollbar: self.read_scrollbar(),
            cells: std::mem::take(&mut self.snapshot_cells),
            clusters,
            row_flags: flags_out,
        }
    }

    /// 解析 `(x, y)` 格的样式为 `(fg, bg, style_flags)`。
    ///
    /// 与 `snapshot()` 用的是同一条路（`grid_ref` + `grid_ref_style`），
    /// 但增加了「与上一格逐字节相同就直接复用」的缓存：一行里同样式
    /// 连续成段是常态，而 `resolve_color` 每次都要做标签分支 + 调色板索引。
    ///
    /// 调色板必须已刷新（`refresh_palette`）才能解析索引色。
    pub(crate) fn resolve_cell_style(
        &mut self,
        x: u16,
        y: u16,
        cache: &mut StyleCache,
    ) -> (Option<u32>, Option<u32>, u32) {
        // 每次调用前重置尺寸头，等价于 C 侧 GHOSTTY_INIT_SIZED
        self.grid_ref.size = std::mem::size_of::<GhosttyGridRef>() as u64;
        self.grid_ref.node = std::ptr::null_mut();
        let point = GhosttyPoint {
            tag: POINT_TAG_VIEWPORT,
            _pad: 0,
            value: GhosttyPointValue {
                coordinate: GhosttyPointCoordinate { x, y: y as u32 },
            },
        };
        let r = unsafe { ghostty_terminal_grid_ref(self.handle, &point, &mut self.grid_ref) };
        if r != 0 {
            cache.valid = false;
            return (None, None, 0);
        }

        self.style = GhosttyStyle::default();
        self.style.size = std::mem::size_of::<GhosttyStyle>() as u64;
        let sr = unsafe { ghostty_grid_ref_style(&self.grid_ref, &mut self.style) };
        if sr != 0 {
            // 样式读取失败：这一格的样式不可信，缓存一并失效
            cache.valid = false;
            return (None, None, 0);
        }

        // 与上一格样式逐字节相同则直接复用解析结果
        let raw = unsafe {
            std::slice::from_raw_parts(
                &self.style as *const GhosttyStyle as *const u8,
                std::mem::size_of::<GhosttyStyle>(),
            )
        };
        if cache.valid && raw == cache.raw.as_slice() {
            return cache.resolved;
        }

        let resolved = (
            self.resolve_color(&self.style.fg_color),
            self.resolve_color(&self.style.bg_color),
            style_flags_of(&self.style),
        );
        cache.raw.copy_from_slice(raw);
        cache.resolved = resolved;
        cache.valid = true;
        resolved
    }

    /// 直接取一帧并编码为线格式，全程复用缓冲。
    ///
    /// `snapshot()` + `encode_snapshot()` 的组合每帧要新建 3 个大 Vec
    /// （cells / packed / out）。生产路径（`vt_snapshot` 命令）只消费字节，
    /// 因此把三个缓冲都提到结构体上，`clear()` 后原地填充 —— 稳态下
    /// 每帧零堆分配，只有最终 `clone()` 出的响应体一次。
    pub fn snapshot_wire(&mut self) -> Vec<u8> {
        let snap = self.snapshot();
        // snapshot() 把 cells take 走了，编码时再借回来，用完归还
        let mut out = std::mem::take(&mut self.snapshot_out);
        let mut packed = std::mem::take(&mut self.snapshot_packed);
        let mut styles = std::mem::take(&mut self.snapshot_styles);
        encode_snapshot_scratch(&snap, &mut out, &mut packed, &mut styles);
        self.snapshot_cells = snap.cells;
        self.snapshot_clusters = snap.clusters;
        self.snapshot_row_flags = snap.row_flags;
        self.snapshot_packed = packed;
        self.snapshot_out = out.clone();
        self.snapshot_styles = styles;
        out
    }

    /// 只把上次快照后**变动的行**编码成增量帧（VTN3）。
    ///
    /// 与 `snapshot_wire` 的分工：全量帧每帧恒定 `cols*rows` 格，与屏幕
    /// 是否真的变了无关；本方法只送引擎标记为脏的行。前端首帧 / resize /
    /// 帧缓存失效时用 `full=true` 退化为整屏。
    ///
    /// 脏标记生命周期：`sync_render_state` → `render_state_update` 累积，
    /// 本函数出口 `clean()` 归还。漏掉 clean 会让脏标记一直累加，下一帧
    /// 把很久以前的行又送一遍。
    pub fn snapshot_wire_diff(&mut self, full: bool) -> Vec<u8> {
        if full {
            // 整屏标脏：帧格式不变（仍是逐行），前端据此重建帧缓存。
            let dirty = RS_DIRTY_FULL;
            if !self.render_state.is_null() {
                unsafe {
                    ghostty_render_state_set(
                        self.render_state,
                        RS_OPTION_DIRTY,
                        &dirty as *const u32 as *const c_void,
                    );
                }
            }
        }

        let cols = self.cols();
        let rows = self.rows();
        let scrollbar = self.read_scrollbar();
        let default_fg = self.read_rgb(DATA_COLOR_FOREGROUND);
        let default_bg = self.read_rgb(DATA_COLOR_BACKGROUND);
        // 光标色只在显式设置时才有意义；未设置就让前端用默认前景。
        let cursor_color = self
            .rs_cursor_rgb()
            .or_else(|| self.read_rgb(DATA_COLOR_CURSOR));
        // 滚动位移 = 视口相对上一帧的位移。前端靠它把本地选区端点
        // 跟随内容一起挪（选区端点存在前端，引擎侧选区在滚动条坐标里）。
        let scroll_delta = scrollbar.offset.saturating_sub(self.last_scroll_offset) as i16;
        self.last_scroll_offset = scrollbar.offset;

        // 逐格样式解析要走调色板（索引色 → RGB），必须先刷新。
        // `snapshot()` 里有同样一步，两条路径的样式解析才一致。
        self.refresh_palette();

        // render state 建不出来时退回全量：正确性优先于省带宽。
        let state = match self.sync_render_state() {
            Some(s) => s,
            None => return self.snapshot_wire(),
        };

        let mut out = std::mem::take(&mut self.diff_out);
        let mut scratch = std::mem::take(&mut self.diff_scratch);
        let meta = DiffFrameMeta {
            cols,
            rows,
            default_fg,
            default_bg,
            cursor_color,
            scrollbar,
            scroll_delta,
        };
        let result = encode_diff_scratch(self, state, &meta, &mut out, &mut scratch);
        self.diff_scratch = scratch;
        // 编码失败必须清空：残留半帧会让前端按合法帧头解出行号越界。
        if result.is_err() {
            out.clear();
        }
        self.diff_out = out.clone();
        // 归还脏标记：否则下一帧会把这次已消费的行再送一遍。
        unsafe { ghostty_render_state_clean(self.render_state) };
        out
    }

    /// render state 里的显式光标色（已 set 才有值）。
    fn rs_cursor_rgb(&self) -> Option<u32> {
        let state = self.render_state;
        if state.is_null() {
            return None;
        }
        if !self.rs_flag(state, RS_COLOR_CURSOR_HAS_VALUE) {
            return None;
        }
        self.rs_rgb(state, RS_COLOR_CURSOR)
    }

    /// 滚动视口。`delta` 为负表示向上（看历史）。
    pub fn scroll_delta(&mut self, delta: i64) {
        let req = GhosttyTerminalScrollViewport {
            tag: SCROLL_DELTA,
            _pad: 0,
            value: [delta as u64, 0],
        };
        unsafe { ghostty_terminal_scroll_viewport(self.handle, &req) };
    }

    /// 滚动到底部（回到活动区）
    pub fn scroll_to_bottom(&mut self) {
        let req = GhosttyTerminalScrollViewport {
            tag: SCROLL_BOTTOM,
            _pad: 0,
            value: [0, 0],
        };
        unsafe { ghostty_terminal_scroll_viewport(self.handle, &req) };
    }

    /// 视口是否停在底部（未滚入历史）
    ///
    /// 注意不能用 `ghostty_terminal_get` 的键 6：那是 ACTIVE_SCREEN
    ///（指针输出），把它按 u16 读出来毫无意义。正确判据是滚动条
    /// offset + len 是否到达 total。
    pub fn viewport_is_at_bottom(&self) -> bool {
        let sb = self.read_scrollbar();
        sb.offset + sb.len >= sb.total
    }

    /// 供基准测试使用：仅解析，不读取网格
    pub fn parse_only(&mut self, bytes: &[u8]) {
        self.write(bytes);
    }

    /// 把一次按键编码为写入 PTY 的字节。
    ///
    /// 编码前必须与终端同步模式：kitty 键盘协议、光标键应用模式、
    /// 修饰键处理方式都随终端状态变化，缓存下来会发出错误的序列。
    pub fn encode_key(&mut self, input: &KeyInput) -> Result<Vec<u8>, String> {
        // 键值范围防御：GhosttyKey 枚举在 vendored 头文件里共 176 个成员
        // （GHOSTTY_KEY_UNIDENTIFIED=0 … GHOSTTY_KEY_PASTE=175），C 侧用枚举值
        // 直接索引定长表。越界值会读到保护区，实测偶发 SIGBUS/SIGSEGV 直接杀进程
        // （是否崩取决于地址布局）。这里先拦下，让它确定性地返回错误。
        if input.key > MAX_GHOSTTY_KEY {
            return Err(format!(
                "非法键值 {}（合法范围 0..={}）",
                input.key, MAX_GHOSTTY_KEY
            ));
        }
        if self.key_encoder.is_null() {
            let rc = unsafe { ghostty_key_encoder_new(std::ptr::null(), &mut self.key_encoder) };
            if rc != 0 || self.key_encoder.is_null() {
                return Err(crate::error_proto::coded_display(
                    "ffi_encoder_create_failed",
                    rc,
                ));
            }
        }
        if self.key_event.is_null() {
            let rc = unsafe { ghostty_key_event_new(std::ptr::null(), &mut self.key_event) };
            if rc != 0 || self.key_event.is_null() {
                return Err(crate::error_proto::coded_display(
                    "ffi_event_create_failed",
                    rc,
                ));
            }
        }

        unsafe {
            // 先取终端模式（kitty 协议标志、光标键应用模式、DEC 1036）
            ghostty_key_encoder_setopt_from_terminal(self.key_encoder, self.handle);

            // 再覆盖宿主偏好。顺序不可颠倒：setopt_from_terminal 会重置选项集，
            // 先设偏好会被它抹掉；而这属于宿主配置，终端状态无权决定。
            let option_as_alt = OPTION_AS_ALT_TRUE;
            let alt_esc = true;
            ghostty_key_encoder_setopt(
                self.key_encoder,
                ENCODER_OPT_MACOS_OPTION_AS_ALT,
                &option_as_alt as *const i32 as *const c_void,
            );
            ghostty_key_encoder_setopt(
                self.key_encoder,
                ENCODER_OPT_ALT_ESC_PREFIX,
                &alt_esc as *const bool as *const c_void,
            );

            ghostty_key_event_set_action(self.key_event, input.action);
            ghostty_key_event_set_key(self.key_event, input.key);
            ghostty_key_event_set_mods(self.key_event, input.mods);
            ghostty_key_event_set_composing(self.key_event, input.composing);
            ghostty_key_event_set_unshifted_codepoint(self.key_event, input.unshifted_codepoint);

            match input.utf8.as_deref() {
                Some(text) if !text.is_empty() => {
                    ghostty_key_event_set_utf8(self.key_event, text.as_ptr(), text.len())
                }
                _ => ghostty_key_event_set_utf8(self.key_event, std::ptr::null(), 0),
            }
        }

        // 转义序列都很短；给足余量后若仍不够则说明编码器行为异常
        let mut buf = [0u8; 128];
        let mut out_len: usize = 0;
        let rc = unsafe {
            ghostty_key_encoder_encode(
                self.key_encoder,
                self.key_event,
                buf.as_mut_ptr(),
                buf.len(),
                &mut out_len,
            )
        };
        if rc != 0 {
            return Err(format!(
                "编码按键失败: {rc}（key={} mods={} action={}）",
                input.key, input.mods, input.action
            ));
        }
        if out_len > buf.len() {
            return Err(crate::error_proto::coded_display(
                "encode_overflow",
                out_len,
            ));
        }

        Ok(buf[..out_len].to_vec())
    }
}

impl Drop for NativeTerminal {
    fn drop(&mut self) {
        self.discard_search();
        if !self.render_state.is_null() {
            unsafe { ghostty_render_state_free(self.render_state) };
            self.render_state = std::ptr::null_mut();
        }
        if !self.key_event.is_null() {
            unsafe { ghostty_key_event_free(self.key_event) };
            self.key_event = std::ptr::null_mut();
        }
        if !self.mouse_event.is_null() {
            unsafe { ghostty_mouse_event_free(self.mouse_event) };
            self.mouse_event = std::ptr::null_mut();
        }
        if !self.mouse_encoder.is_null() {
            unsafe { ghostty_mouse_encoder_free(self.mouse_encoder) };
            self.mouse_encoder = std::ptr::null_mut();
        }
        if !self.key_encoder.is_null() {
            unsafe { ghostty_key_encoder_free(self.key_encoder) };
            self.key_encoder = std::ptr::null_mut();
        }
        if !self.handle.is_null() {
            unsafe { ghostty_terminal_free(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 宽字符占两格：首格携带码点，第二格是 spacer。取文本时跳过 spacer，
    /// 否则「中文」会被读成「中 文」。
    fn row_text(snap: &Snapshot, y: u16) -> String {
        let base = y as usize * snap.cols as usize;
        snap.cells[base..base + snap.cols as usize]
            .iter()
            .filter(|c| c.wide != 2)
            .map(|c| {
                if c.has_text && c.codepoint > 0 {
                    char::from_u32(c.codepoint).unwrap_or('?')
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn writes_and_reads_text() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        t.write(b"hello native\r\nsecond");
        let snap = t.snapshot();
        assert_eq!(row_text(&snap, 0), "hello native");
        assert_eq!(row_text(&snap, 1), "second");
    }

    #[test]
    fn handles_cup_positioning() {
        let mut t = NativeTerminal::new(20, 5).unwrap();
        t.write(b"\x1b[3;5HCUP");
        let snap = t.snapshot();
        assert_eq!(row_text(&snap, 2), "    CUP");
    }

    #[test]
    fn decodes_wide_cjk_with_spacers() {
        let mut t = NativeTerminal::new(10, 2).unwrap();
        t.write("中文".as_bytes());
        let snap = t.snapshot();
        assert_eq!(row_text(&snap, 0), "中文");

        assert_eq!(snap.cells[0].codepoint, 0x4e2d);
        assert_eq!(snap.cells[0].wide, 1, "首字应为 WIDE");
        assert_eq!(snap.cells[1].wide, 2, "第二格应为 SPACER_TAIL");
    }

    #[test]
    fn resolves_palette_and_truecolor() {
        let mut t = NativeTerminal::new(10, 2).unwrap();
        t.write(b"\x1b[38;5;1mA\x1b[38;2;10;200;90mB");
        let snap = t.snapshot();

        // One Dark 调色板第 1 项 = #cc6666
        assert_eq!(snap.cells[0].fg, Some(0xcc6666), "索引色应解析为调色板值");
        assert_eq!(snap.cells[1].fg, Some(0x0ac85a), "真彩应精确保留");
    }

    #[test]
    fn reads_text_attributes() {
        let mut t = NativeTerminal::new(10, 2).unwrap();
        t.write(b"\x1b[1mB\x1b[3mI\x1b[0mN");
        let snap = t.snapshot();
        assert!(snap.cells[0].bold);
        assert!(snap.cells[1].italic);
        assert!(!snap.cells[2].bold && !snap.cells[2].italic);
    }

    #[test]
    fn viewport_follows_scrollback() {
        let mut t = NativeTerminal::new(10, 3).unwrap();
        for i in 1..=8 {
            t.write(format!("L{i}\r\n").as_bytes());
        }
        let snap = t.snapshot();
        // 最后一次写入是 "L8\r\n"，光标停在新的空行。
        // 3 行视口贴着底部，因此应是 L7 / L8 / 空行。
        assert_eq!(row_text(&snap, 0), "L7");
        assert_eq!(row_text(&snap, 1), "L8");
        assert_eq!(row_text(&snap, 2), "");
    }

    #[test]
    fn resize_changes_grid() {
        let mut t = NativeTerminal::new(10, 3).unwrap();
        t.write(b"abc");
        t.resize(30, 6, 8, 16).unwrap();
        let snap = t.snapshot();
        assert_eq!(snap.cols, 30);
        assert_eq!(snap.rows, 6);
        assert_eq!(row_text(&snap, 0), "abc");
    }

    #[test]
    fn handles_utf8_split_across_writes() {
        let mut t = NativeTerminal::new(10, 2).unwrap();
        // 「中」的 UTF-8 是 E4 B8 AD，逐字节喂入
        for byte in "中".as_bytes() {
            t.write(&[*byte]);
        }
        let snap = t.snapshot();
        assert_eq!(row_text(&snap, 0), "中", "引擎应跨调用拼合 UTF-8");
    }
}

// ---------------------------------------------------------------------------
// 基准：原生 vs wasm
// ---------------------------------------------------------------------------

/// 基准负载的唯一来源。
///
/// `npm run bench:terminal` 会把这份字节流落盘，同一份字节同时喂给
/// 原生引擎与 xterm.js headless，并用 FNV-1a 校验两边一致。
/// 该流刻意偏向 print-heavy：大量彩色日志行 + 光标定位 + 进度条重绘，
/// 正是解析器的热路径。
pub fn engine_bench_stream(lines: usize) -> Vec<u8> {
    const LEVELS: [&str; 4] = [
        "\x1b[32mINFO \x1b[0m",
        "\x1b[33mWARN \x1b[0m",
        "\x1b[36mDEBG\x1b[0m",
        "\x1b[31mERR! \x1b[0m",
    ];

    let mut out = Vec::with_capacity(lines * 160);

    for i in 0..lines {
        let level = LEVELS[i % 4];
        let id = (i as u64 * 2654435761) % 4294967296;
        let latency = (i as u64 * 7919) % 997;
        let pct = i % 101;

        out.extend_from_slice(level.as_bytes());
        out.extend_from_slice(
            format!(
                "\x1b[90m[{:05}]\x1b[0m worker=\x1b[35m{}\x1b[0m id=\x1b[36m{}\x1b[0m latency=\x1b[{}m{}ms\x1b[0m \x1b[34mpayload=\x1b[0m{:04x}\r\n",
                i,
                i % 32,
                id,
                if latency > 500 { "31" } else { "32" },
                latency,
                id % 65536
            )
            .as_bytes(),
        );

        if i % 16 == 0 {
            let filled = (pct as f64 / 4.0).round() as usize;
            out.extend_from_slice(
                format!(
                    "\x1b[1G\x1b[36m[{}{}]\x1b[0m {:>3}%\r",
                    "#".repeat(filled),
                    ".".repeat(25 - filled),
                    pct
                )
                .as_bytes(),
            );
        }

        if i % 64 == 0 {
            out.extend_from_slice(
                "\x1b[1;3;4m\x1b[38;2;120;200;255m真彩粗斜体下划线\x1b[0m \
\x1b[48;5;236m\x1b[38;5;214m256 色对\x1b[0m \x1b[7m反显\x1b[0m\r\n"
                    .as_bytes(),
            );
        }
    }

    out
}

#[cfg(test)]
mod bench {
    use super::*;
    use std::time::Instant;

    fn mb(bytes: usize) -> f64 {
        bytes as f64 / (1024.0 * 1024.0)
    }

    /// 纯解析吞吐：只喂字节，不读网格
    #[test]
    #[ignore = "基准测试，手动运行"]
    fn bench_parse_throughput() {
        const COLS: u16 = 120;
        const ROWS: u16 = 40;
        const CHUNK: usize = 8192;
        // 约 12 万行 ≈ 16 MB，与 wasm 侧使用完全相同的行数
        let payload = engine_bench_stream(120_000);
        println!(
            "负载: {:.2} MB ({} 行, 与 wasm 侧同源)",
            mb(payload.len()),
            120_000
        );

        let mut t = NativeTerminal::new(COLS, ROWS).unwrap();
        // 预热
        for c in payload.chunks(CHUNK).take(64) {
            t.parse_only(c);
        }

        let start = Instant::now();
        for chunk in payload.chunks(CHUNK) {
            t.parse_only(chunk);
        }
        let elapsed = start.elapsed();

        let throughput = mb(payload.len()) / elapsed.as_secs_f64();
        let chunks = payload.len() / CHUNK;
        println!(
            "原生解析: {:.1} MB/s  ({} 块 8KB, 用时 {:.3}s)",
            throughput,
            chunks,
            elapsed.as_secs_f64()
        );
        println!(
            "  每块耗时 {:.2} µs",
            elapsed.as_micros() as f64 / chunks as f64
        );
    }

    /// 全屏快照速率：这是渲染循环实际做的事
    #[test]
    #[ignore = "基准测试，手动运行"]
    fn bench_snapshot_rate() {
        const COLS: u16 = 120;
        const ROWS: u16 = 40;
        let payload = engine_bench_stream(8_000);

        let mut t = NativeTerminal::new(COLS, ROWS).unwrap();
        for chunk in payload.chunks(8192) {
            t.parse_only(chunk);
        }

        const ITERS: usize = 2000;
        // 预热
        for _ in 0..100 {
            std::hint::black_box(t.snapshot());
        }

        let start = Instant::now();
        for _ in 0..ITERS {
            std::hint::black_box(t.snapshot());
        }
        let elapsed = start.elapsed();

        let per_snap = elapsed.as_secs_f64() / ITERS as f64;
        let fps = 1.0 / per_snap;
        let cells = COLS as usize * ROWS as usize;
        println!(
            "原生全屏快照 {COLS}x{ROWS} ({cells} 格): {:.3} ms/帧  → {:.0} FPS",
            per_snap * 1000.0,
            fps
        );
        println!("  每格 {:.2} µs", per_snap * 1e6 / cells as f64);
    }

    /// 拆解快照成本：定位真正的瓶颈在哪一步
    #[test]
    #[ignore = "基准测试，手动运行"]
    fn bench_snapshot_breakdown() {
        const COLS: u16 = 120;
        const ROWS: u16 = 40;
        const ITERS: usize = 2000;
        let mut t = NativeTerminal::new(COLS, ROWS).unwrap();
        t.write(b"\x1b[31mhello\x1b[0m world");

        // 仅 grid_ref
        let start = Instant::now();
        for _ in 0..ITERS {
            for y in 0..ROWS {
                for x in 0..COLS {
                    let point = GhosttyPoint {
                        tag: POINT_TAG_VIEWPORT,
                        _pad: 0,
                        value: GhosttyPointValue {
                            coordinate: GhosttyPointCoordinate { x, y: y as u32 },
                        },
                    };
                    t.grid_ref.size = std::mem::size_of::<GhosttyGridRef>() as u64;
                    t.grid_ref.node = std::ptr::null_mut();
                    unsafe { ghostty_terminal_grid_ref(t.handle, &point, &mut t.grid_ref) };
                }
            }
        }
        let grid_ref_only = start.elapsed();

        // 完整快照作为对照
        let start = Instant::now();
        for _ in 0..ITERS {
            std::hint::black_box(t.snapshot());
        }
        let full = start.elapsed();

        let cells = (COLS as usize * ROWS as usize * ITERS) as f64;
        println!(
            "grid_ref 定位: {:.3} ms/帧 ({:.3} µs/格)",
            grid_ref_only.as_secs_f64() * 1000.0 / ITERS as f64,
            grid_ref_only.as_secs_f64() * 1e6 / cells
        );
        println!(
            "完整快照:     {:.3} ms/帧 ({:.3} µs/格)",
            full.as_secs_f64() * 1000.0 / ITERS as f64,
            full.as_secs_f64() * 1e6 / cells
        );
        println!(
            "  grid_ref 占完整快照的 {:.0}%",
            grid_ref_only.as_secs_f64() / full.as_secs_f64() * 100.0
        );
    }
}

// ---------------------------------------------------------------------------
// 二进制线格式：Rust → 前端的单缓冲协议
// ---------------------------------------------------------------------------
//
// 不走 JSON：u64 单元格在 JSON 里超过 2^53 会丢精度，且 4800 格的
// 对象数组序列化开销远大于一次 memcpy。改用 Tauri 的原始字节响应，
// 前端用 DataView 零拷贝读取。
//
// 布局（全部小端）：
//   头部 56 字节
//     u32 magic        = 0x54564E31 ("VTN1")
//     u16 cols
//     u16 rows
//     u16 cursor_x
//     u16 cursor_y
//     u16 style_count
//     u16 flags         bit0 光标可见, bit1 待换行
//     i32 default_fg    (-1 表示未设置)
//     i32 default_bg
//     i32 cursor_color
//     u32 cursor_style  0=bar 1=block 2=underline 3=block_hollow
//     u64 scrollbar_total
//     u64 scrollbar_offset
//     u64 scrollbar_len
//
//   默认色随帧送达，使帧自足：渲染器无需再访问终端句柄，
//   原生路径下前端本来也拿不到句柄。
//   样式表 style_count × 12 字节
//     i32 fg         (-1 表示未设置)
//     i32 bg         (-1 表示未设置)
//     u32 flags      bit0 bold, bit1 italic, bit2 faint, bit3 inverse,
//                    bit4 invisible, bit5 strikethrough
//   单元格 cols*rows × 8 字节（u64 小端）
//     bit 0..20  codepoint (21)
//     bit 21..22 wide      (2)
//     bit 23..38 style_idx (16)
//     bit 39     has_text  (1)
//     bit 40..63 cluster_index + 1 (24, 0 = 无字素簇)
//
//   尾部（**可选**，旧帧没有；解码器必须先看 magic 再决定是否解析）
//     u32 magic        = 0x56544631 ("VTF1")
//     u16 cluster_count
//     u16 row_flags_count
//     row_flags_count × u16 row_flags   bit0 OSC 133 prompt, bit1 续行
//     cluster_count × (u32 byte_len + UTF-8 bytes)
//
//   字素簇表只在「一格有多个码点」时才有条目：`e` + U+0301 这样的组合、
//   ZWJ emoji 序列必须整簇送达，前端才能画出引擎看到的同一个字形。
//   单码点格不占表项（`cluster_index == 0`），普通 ASCII 屏幕零开销。

pub const WIRE_MAGIC: u32 = 0x54564E31;
pub const WIRE_HEADER_LEN: usize = 56;
pub const WIRE_STYLE_LEN: usize = 12;
pub const WIRE_CELL_LEN: usize = 8;
/// 全量帧尾部 magic（"VTF1"）。见上方布局注释。
pub const WIRE_FOOTER_MAGIC: u32 = 0x56544631;
/// 增量帧尾部 magic（"VDF1"）。
pub const WIRE_DIFF_FOOTER_MAGIC: u32 = 0x56444631;

/// 增量帧 magic（"VTN3"）。与全量帧同族但语义不同：只带脏行。
///
/// 为什么需要增量：全量帧每帧恒定 `cols*rows*8 + 56` 字节，与屏幕是否
/// 真的变了无关。终端多数帧只有一两行在动（光标闪烁、逐字回显），
/// 全量帧把 40 行 × 8 字节全部重编码、重拷贝、重解码一遍。增量帧只送
/// 引擎标记为脏的行，稳态下从 KB 级降到百字节级。
pub const WIRE_DIFF_MAGIC: u32 = 0x54564E33;
/// 增量帧头部长度。
///
/// 布局（全部小端，按序）：
///   0..3    u32 magic
///   4..5    u16 cols
///   6..7    u16 rows
///   8..9    u16 flags        bit0 光标可见, bit1 待换行
///   10..13  i32 default_fg
///   14..17  i32 default_bg
///   18..21  i32 cursor_color
///   22..25  u32 cursor_style
///   26..27  u16 cursor_x
///   28..29  u16 cursor_y
///   30..31  u16 style_count
///   32..33  u16 row_count
///   34..35  i16 scroll_delta
///   36..43  u64 scrollbar_total
///   44..51  u64 scrollbar_offset
///   52..59  u64 scrollbar_len
/// 之后是 style_count × 12B 的**帧级**样式表，再是逐条脏行：
///   行头 8B（u16 row + u16 reserved + u16 row_flags + u16 pad）+ cols × 8B cells。
/// 行头 `row_flags` 的 bit0 = OSC 133 prompt 行，bit1 = prompt 续行。
/// 最后是帧尾（解码器按 magic 判断，缺尾视为空表）：
///   u32 magic = 0x56444631 ("VDF1"), u16 cluster_count, u16 reserved,
///   cluster_count × (u32 byte_len + UTF-8 bytes)。
///
/// 为什么行头是 8 字节（VTN2 是 6）：cells 必须落在 4 字节边界上，
/// 前端才能零拷贝建 `Uint32Array` 视图。6 字节行头让每个 cells 起点
/// `≡ 2 (mod 4)`，前端建视图时必抛 RangeError（实测：增量通道 100% 失效）。
/// 多出的 2 字节同时给 pad 留位，且省掉了 VTN2 里「每行再抄一遍样式表」
/// 的冗余 —— 那时前端只读帧级表，逐行片段纯属线上浪费。
pub const WIRE_DIFF_HEADER_LEN: usize = 60;
/// 每条脏行记录的行头：u16 row + u16 reserved + u16 row_flags + u16 pad。
pub const WIRE_DIFF_ROW_HEADER_LEN: usize = 8;

/// `GHOSTTY_RENDER_STATE_DATA_*`（render.h）。
const RS_DATA_CURSOR: u32 = 18;
const RS_DATA_COLORS: u32 = 19;
/// 把一个**新创建**的行迭代器壳绑到渲染状态。
///
/// 缺了这一步，迭代器内部字段全是未初始化值，任何 next/get 都会
/// 读野指针（实测 SIGSEGV）。依据 `ghostty_render_state_row_iterator_new`
/// 的文档：「All fields except the allocator are left undefined until
/// populated」。
const RS_DATA_ROW_ITERATOR: u32 = 4;
/// 行级数据：整行的原始 cell 值数组（一次调用拿完整行）。
const RS_ROW_DATA_CELLS_RAW: u32 = 5;
/// `GHOSTTY_RENDER_STATE_OPTION_DIRTY`：整屏标脏（首帧 / resize 后全量重绘）。
const RS_OPTION_DIRTY: u32 = 0;
/// `GhosttyRenderStateDirty::FULL`
const RS_DIRTY_FULL: u32 = 2;
/// `GHOSTTY_CELL_DATA_*`（screen.h）。值经 `ghostty_type_json` 核对。
const CELL_DATA_CODEPOINT: u32 = 1;
const CELL_DATA_WIDE: u32 = 3;
const CELL_DATA_HAS_TEXT: u32 = 4;
/// `GhosttyCellsView { const GhosttyCell* ptr; size_t len; }`。
/// `row_get(iter, CELLS_RAW, &view)` 的产出：借用视图，只在下一次
/// render state update 之前有效。
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct GhosttyCellsView {
    ptr: *const u64,
    len: usize,
}
/// `GhosttyRenderStateCursor`（render.h，sized-struct ABI）。
#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyRenderStateCursor {
    size: usize,
    viewport_has_value: bool,
    viewport_x: u16,
    viewport_y: u16,
    wide_tail: bool,
    visible: bool,
    blinking: bool,
    password_input: bool,
    visual_style: u32,
}
impl Default for GhosttyRenderStateCursor {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
/// `GhosttyRenderStateColors`（render.h，sized-struct ABI，含 256 色调色板）。
#[repr(C)]
#[derive(Clone, Copy)]
struct GhosttyRenderStateColors {
    size: usize,
    background: [u8; 3],
    foreground: [u8; 3],
    cursor: [u8; 3],
    cursor_has_value: bool,
    palette: [[u8; 3]; 256],
}
impl Default for GhosttyRenderStateColors {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

pub mod style_flags {
    pub const BOLD: u32 = 1 << 0;
    pub const ITALIC: u32 = 1 << 1;
    pub const FAINT: u32 = 1 << 2;
    pub const INVERSE: u32 = 1 << 3;
    pub const INVISIBLE: u32 = 1 << 4;
    pub const STRIKETHROUGH: u32 = 1 << 5;
}

/// 行标记位（`Snapshot::row_flags` / 增量帧行头 `row_flags` 共用）。
pub mod row_flags {
    /// OSC 133 `A` 标记的 prompt 行
    pub const PROMPT: u8 = 1 << 0;
    /// prompt 的续行（折行后的部分）
    pub const PROMPT_CONTINUATION: u8 = 1 << 1;
}

/// `GHOSTTY_ROW_DATA_*`（screen.h）。
const ROW_DATA_GRAPHEME: u32 = 3;
const ROW_DATA_SEMANTIC_PROMPT: u32 = 6;
/// `GhosttyRowSemanticPrompt`（C 侧枚举以 int 为底层类型）。
const ROW_SEMANTIC_PROMPT: i32 = 1;
const ROW_SEMANTIC_PROMPT_CONTINUATION: i32 = 2;
/// `GHOSTTY_OUT_OF_SPACE`：缓冲不足，`out_len` 已写入所需长度。
const GHOSTTY_OUT_OF_SPACE: i32 = -3;

/// 样式去重表：同一屏内重复样式极多，去重后样式表通常只有个位数条目。
///
/// 索引必须是插入顺序（线格式里前端按索引取样式），因此
/// `entries` 保留顺序、`index` 只做 O(1) 查找。
///
/// `pub(crate)`：`encode_snapshot_scratch` 是 crate 可见的跨帧暂存入口，
/// 需要把缓冲的所有权交给调用方（`NativeTerminal`）持有。
/// 原先 `entries.iter().position()` 是 O(n) 线性扫：每格一次，
/// 80x24 就是 1920 × 条目数。条目通常个位数时无所谓，但一旦某个
/// 输出把颜色铺满（渐变、语法高亮 diff），条目涨到几十就会看到。
#[derive(Default)]
pub(crate) struct StyleTable {
    entries: Vec<(Option<u32>, Option<u32>, u32)>,
    index: std::collections::HashMap<(Option<u32>, Option<u32>, u32), u16>,
}

impl StyleTable {
    /// 复用前清空：保留 entries/index 的容量，稳态下零分配。
    fn reset(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    fn intern(&mut self, cell: &CellView) -> u16 {
        self.intern_style(cell.fg, cell.bg, style_bits(cell))
    }

    /// 按已解析的样式键去重。增量帧直接拿到 (fg, bg, flags)，
    /// 不需要先造一个 `CellView`。
    fn intern_style(&mut self, fg: Option<u32>, bg: Option<u32>, flags: u32) -> u16 {
        if let Some(&idx) = self.index.get(&(fg, bg, flags)) {
            return idx;
        }
        let idx = self.entries.len() as u16;
        self.entries.push((fg, bg, flags));
        self.index.insert((fg, bg, flags), idx);
        idx
    }
}

/// `CellView` 的样式位 → 线格式 flags。
fn style_bits(cell: &CellView) -> u32 {
    style_flags_of_cell(
        cell.bold,
        cell.italic,
        cell.faint,
        cell.inverse,
        cell.invisible,
        cell.strikethrough,
    )
}

/// 解析后的样式在增量帧里的缓存槽。
///
/// 「与上一格逐字节相同就复用」的判据是原始 `GhosttyStyle` 字节串：
/// 解析（调色板索引 → RGB）有成本，而一行里同样式连续成段是常态。
pub(crate) struct StyleCache {
    /// 上一格的原始 `GhosttyStyle` 字节
    raw: [u8; std::mem::size_of::<GhosttyStyle>()],
    resolved: (Option<u32>, Option<u32>, u32),
    valid: bool,
}

impl Default for StyleCache {
    fn default() -> Self {
        Self {
            raw: [0u8; std::mem::size_of::<GhosttyStyle>()],
            resolved: (None, None, 0),
            valid: false,
        }
    }
}

/// `GhosttyStyle` → 线格式 flags。
fn style_flags_of(style: &GhosttyStyle) -> u32 {
    style_flags_of_cell(
        style.bold,
        style.italic,
        style.faint,
        style.inverse,
        style.invisible,
        style.strikethrough,
    )
}

fn style_flags_of_cell(
    bold: bool,
    italic: bool,
    faint: bool,
    inverse: bool,
    invisible: bool,
    strikethrough: bool,
) -> u32 {
    let mut flags = 0u32;
    if bold {
        flags |= style_flags::BOLD;
    }
    if italic {
        flags |= style_flags::ITALIC;
    }
    if faint {
        flags |= style_flags::FAINT;
    }
    if inverse {
        flags |= style_flags::INVERSE;
    }
    if invisible {
        flags |= style_flags::INVISIBLE;
    }
    if strikethrough {
        flags |= style_flags::STRIKETHROUGH;
    }
    flags
}

/// 把快照编码为线格式。返回的缓冲可直接作为 Tauri 原始响应体。
pub fn encode_snapshot(snap: &Snapshot) -> Vec<u8> {
    let mut out = Vec::new();
    encode_snapshot_into(snap, &mut out);
    out
}

/// `encode_snapshot` 的缓冲复用版本：追加写入调用方缓冲。
///
/// `vt_snapshot` 每帧都会编码一次，每次新建 Vec 会让分配器在 60fps
/// 下持续抖动。调用方持有缓冲跨帧复用，`clear()` 后原地填充。
/// packed_cells 也走同一缓冲：80x24 = 1920 × 8B = 15KB，每帧一次分配。
pub fn encode_snapshot_into(snap: &Snapshot, out: &mut Vec<u8>) {
    encode_snapshot_scratch(snap, out, &mut Vec::new(), &mut StyleTable::default())
}

/// 带外部暂存区的编码入口。`packed` 与 `table` 都会被 `clear()` 后原地填充，
/// 调用方可跨帧持有以复用容量。
pub(crate) fn encode_snapshot_scratch(
    snap: &Snapshot,
    out: &mut Vec<u8>,
    packed_cells: &mut Vec<u64>,
    table: &mut StyleTable,
) {
    let cell_count = snap.cells.len();
    table.reset();

    // 先建样式表，再写单元格
    packed_cells.clear();
    packed_cells.reserve(cell_count);
    for cell in &snap.cells {
        let style_idx = table.intern(cell);
        packed_cells.push(pack_wire(
            cell.codepoint,
            cell.wide,
            cell.has_text,
            style_idx,
            cell.cluster_index,
        ));
    }

    let style_count = table.entries.len();
    let cluster_count = snap.clusters.len().min(u16::MAX as usize);
    let row_flags_count = snap.row_flags.len().min(u16::MAX as usize);
    let footer_len = 8
        + row_flags_count * 2
        + snap.clusters[..cluster_count]
            .iter()
            .map(|c| 4 + c.len())
            .sum::<usize>();
    out.clear();
    out.reserve(
        WIRE_HEADER_LEN + style_count * WIRE_STYLE_LEN + cell_count * WIRE_CELL_LEN + footer_len,
    );

    out.extend_from_slice(&WIRE_MAGIC.to_le_bytes());
    out.extend_from_slice(&snap.cols.to_le_bytes());
    out.extend_from_slice(&snap.rows.to_le_bytes());
    out.extend_from_slice(&snap.cursor_x.to_le_bytes());
    out.extend_from_slice(&snap.cursor_y.to_le_bytes());
    out.extend_from_slice(&(style_count as u16).to_le_bytes());

    let mut flags = 0u16;
    if snap.cursor_visible {
        flags |= 1;
    }
    if snap.cursor_pending_wrap {
        flags |= 1 << 1;
    }
    out.extend_from_slice(&flags.to_le_bytes());

    for color in [snap.default_fg, snap.default_bg, snap.cursor_color] {
        out.extend_from_slice(&color.map(|v| v as i32).unwrap_or(-1).to_le_bytes());
    }
    out.extend_from_slice(&snap.cursor_style.to_le_bytes());
    out.extend_from_slice(&snap.scrollbar.total.to_le_bytes());
    out.extend_from_slice(&snap.scrollbar.offset.to_le_bytes());
    out.extend_from_slice(&snap.scrollbar.len.to_le_bytes());

    for (fg, bg, flags) in &table.entries {
        out.extend_from_slice(&fg.map(|v| v as i32).unwrap_or(-1).to_le_bytes());
        out.extend_from_slice(&bg.map(|v| v as i32).unwrap_or(-1).to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
    }

    for packed in packed_cells.iter() {
        out.extend_from_slice(&packed.to_le_bytes());
    }

    // 帧尾：行标记 + 字素簇表。旧解码器读到 cells 末尾就停，多出的字节
    // 不影响它们；新解码器按 magic 判断是否存在。
    out.extend_from_slice(&WIRE_FOOTER_MAGIC.to_le_bytes());
    out.extend_from_slice(&(cluster_count as u16).to_le_bytes());
    out.extend_from_slice(&(row_flags_count as u16).to_le_bytes());
    for flag in &snap.row_flags[..row_flags_count] {
        out.extend_from_slice(&(*flag as u16).to_le_bytes());
    }
    for cluster in &snap.clusters[..cluster_count] {
        out.extend_from_slice(&(cluster.len() as u32).to_le_bytes());
        out.extend_from_slice(cluster.as_bytes());
    }
}

// ---------------------------------------------------------------------------
// 增量线格式（VTN3）：只带脏行
// ---------------------------------------------------------------------------
//
// 与全量帧共用单元格/样式编码，差别在行维度：只送 render state 标记为
// 脏的行。终端多数帧只有一两行在动（光标闪烁、逐字回显、一次快捷键），
// 全量帧把整屏重编码一遍，增量帧稳态下从 KB 级降到百字节级。
//
// 脏标记由 render state 持有，`ghostty_render_state_update` 累积、
// `ghostty_render_state_clean` 清空。因此每帧必须「update → 消费 → clean」，
// 否则脏标记一直累加，下一帧把很久以前的行又送一遍。
//
// 滚动条仍整帧送达（3 个 u64）：前端靠 offset 变化把本地选区端点跟随
// 内容位移，必须每次都知道真实位置。
//
// ## 增量帧的样式从哪来
//
// render state 的单元格 API **不解析样式** —— 它只给原始 cell 值与
// 「显式 RGB 背景」。SGR 属性（粗体/斜体）与前景色必须走 terminal 的
// `grid_ref` + `grid_ref_style`，与 `snapshot()` 用的是同一条路径。
//
// 代价是每格两次 FFI（`ghostty_terminal_grid_ref` + `grid_ref_style`），
// 但只对**脏行**做：40 行的屏幕改 1 行就是 1×cols 次，而不是 cols×rows。
// 且样式解析带「与上一格逐字节相同就复用」的缓存，一行里同样式连续
// 成段是常态（一段同色文本、一片空白），实际解析次数远低于格数。

/// 增量帧暂存缓冲（`NativeTerminal` 持有，跨帧复用）。
#[derive(Default)]
pub(crate) struct DiffScratch {
    /// 所有脏行的行体（行头 8B + cols×8B），按行顺序拼接。跨帧复用，
    /// 不按行新建 Vec。
    body: Vec<u8>,
    /// 整行原始 cell 的视图（`CELLS_RAW` 一次 FFI 的结果）。
    view: GhosttyCellsView,
    /// 光标读取槽。
    cursor: GhosttyRenderStateCursor,
    /// 行间共享的样式表（整帧一张，cells 里存它的全局下标）。
    table: DiffStyleTable,
    /// 逐格样式解析的跨格缓存。
    style_cache: StyleCache,
    /// 帧级字素簇表（整帧一张，cells 里存下标 + 1）。
    clusters: ClusterTable,
    /// 超过栈容量的长字素簇重试缓冲。
    cluster_heap: Vec<u32>,
}

/// 帧级字素簇去重表：`entries` 保序（线格式下标就是它的下标），
/// `index` 只做 O(1) 查找。
///
/// 与样式表同构：一帧里重复的簇（同一个 emoji 出现多次）只占一条，
/// cells 里存 `下标 + 1`（0 表示无簇）。
#[derive(Default)]
struct ClusterTable {
    entries: Vec<String>,
    index: std::collections::HashMap<String, u16>,
}

impl ClusterTable {
    fn reset(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    /// 并入一个簇，返回「下标 + 1」。表满（u16 上限）时返回 0。
    fn intern(&mut self, cluster: &str) -> u32 {
        if let Some(&idx) = self.index.get(cluster) {
            return idx as u32 + 1;
        }
        if self.entries.len() >= u16::MAX as usize {
            return 0;
        }
        let idx = self.entries.len() as u16;
        let owned = cluster.to_owned();
        self.entries.push(owned.clone());
        self.index.insert(owned, idx);
        idx as u32 + 1
    }
}

/// 增量帧用的样式去重表：整帧一张，cells 里的 `style_idx` 是它的全局下标。
/// 相邻行通常共享大部分样式，跨行共用一张表比每行一张省得多（VTN2 曾把
/// 每行的片段也抄到线上，前端从不读，纯浪费）。
#[derive(Default)]
struct DiffStyleTable {
    entries: Vec<(Option<u32>, Option<u32>, u32)>,
    index: std::collections::HashMap<(Option<u32>, Option<u32>, u32), u16>,
}

impl DiffScratch {
    fn table_len(&self) -> usize {
        self.table.entries.len()
    }

    fn table_reset(&mut self) {
        self.table.entries.clear();
        self.table.index.clear();
    }

    /// 把整表写进已预留的空间（帧头之后一次性布局）。
    fn write_table(&self, out: &mut [u8]) {
        let mut off = 0;
        for (fg, bg, flags) in &self.table.entries {
            out[off..off + 4].copy_from_slice(&fg.map(|v| v as i32).unwrap_or(-1).to_le_bytes());
            out[off + 4..off + 8]
                .copy_from_slice(&bg.map(|v| v as i32).unwrap_or(-1).to_le_bytes());
            out[off + 8..off + 12].copy_from_slice(&flags.to_le_bytes());
            off += WIRE_STYLE_LEN;
        }
    }

    fn intern(&mut self, fg: Option<u32>, bg: Option<u32>, flags: u32) -> u16 {
        if let Some(&idx) = self.table.index.get(&(fg, bg, flags)) {
            return idx;
        }
        let idx = self.table.entries.len() as u16;
        self.table.entries.push((fg, bg, flags));
        self.table.index.insert((fg, bg, flags), idx);
        idx
    }
}

/// 读一个单元格的完整字素簇（基码点 + 组合码点）。
///
/// 先用 8 码点的栈缓冲：组合重音、带修饰的 emoji 等常见情况都在里面。
/// 引擎缓冲不足时返回 `GHOSTTY_OUT_OF_SPACE`，并把所需码点数写进
/// `out_len`；此时用调用方复用的 `heap` 重试（长 ZWJ 序列才会走到）。
///
/// 返回 `(码点数, 是否落在 heap)`；读取失败返回 None。
fn read_cluster(
    reference: &GhosttyGridRef,
    stack: &mut [u32; CLUSTER_STACK_CODEPOINTS],
    heap: &mut Vec<u32>,
) -> Option<(usize, bool)> {
    let mut len = 0usize;
    let rc =
        unsafe { ghostty_grid_ref_graphemes(reference, stack.as_mut_ptr(), stack.len(), &mut len) };
    if rc == 0 {
        return Some((len.min(stack.len()), false));
    }
    if rc != GHOSTTY_OUT_OF_SPACE || len == 0 {
        return None;
    }
    heap.clear();
    heap.resize(len, 0);
    let rc =
        unsafe { ghostty_grid_ref_graphemes(reference, heap.as_mut_ptr(), heap.len(), &mut len) };
    if rc != 0 {
        return None;
    }
    Some((len.min(heap.len()), true))
}

/// `read_cluster` 的结果切片（栈 / 堆二选一）。
fn cluster_slice<'a>(stack: &'a [u32], heap: &'a [u32], len: usize, in_heap: bool) -> &'a [u32] {
    if in_heap {
        &heap[..len]
    } else {
        &stack[..len]
    }
}

/// 码点 → UTF-8。非法码点（代理区等）退化为 U+FFFD，绝不 panic。
fn cluster_utf8(codepoints: &[u32]) -> String {
    let mut s = String::with_capacity(codepoints.len() * 4);
    for &cp in codepoints {
        s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
    }
    s
}

/// 把字素簇并入去重表，返回线格式下标（即「表下标 + 1」）。
///
/// 表长在线格式里是 u16（帧尾 `cluster_count`），超出后无法表达，
/// 此时返回 0（该格退化为只有基码点）。
fn intern_cluster(clusters: &mut Vec<String>, s: &str) -> u32 {
    if let Some(i) = clusters.iter().position(|c| c.as_str() == s) {
        return i as u32 + 1;
    }
    if clusters.len() >= u16::MAX as usize {
        return 0;
    }
    clusters.push(s.to_owned());
    clusters.len() as u32
}

fn pack_wire(codepoint: u32, wide: u8, has_text: bool, style_idx: u16, cluster_index: u32) -> u64 {
    (codepoint as u64) & CELL_CODEPOINT_MASK
        | ((wide as u64) & CELL_WIDE_MASK) << 21
        | (style_idx as u64) << 23
        | (has_text as u64) << 39
        | ((cluster_index as u64) & CELL_CLUSTER_MASK) << CELL_CLUSTER_SHIFT
}

fn rgb_packed(c: &[u8; 3]) -> u32 {
    ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32
}

/// 增量帧头的一次性读数值。合成一个类型而不是传 5 个平铺参数：
/// 它们同源于 `snapshot_wire_diff` 的同一批 render-state 读取，语义上是
/// 「这一帧的元数据」这个整体，分开传容易在调用点上配错顺序。
pub(crate) struct DiffFrameMeta {
    pub cols: u16,
    pub rows: u16,
    pub default_fg: Option<u32>,
    pub default_bg: Option<u32>,
    pub cursor_color: Option<u32>,
    pub scrollbar: GhosttyTerminalScrollbar,
    pub scroll_delta: i16,
}

/// 把 render state 的脏行编码成增量帧（VTN3）。
///
/// 前置条件：调用方刚对本终端的 `render_state` 做过 update。
/// 本函数**不** clean，由调用方在命令出口统一 clean。
///
/// `meta.cols` / `meta.rows` 用调用方传的网格尺寸（与 render state 自身读数
/// 一致），避免在持锁期间再取一次。
///
/// `term` 同时用于两处：待换行标志（`ghostty_terminal_get`）与逐格样式
/// 解析（`grid_ref_style`）。render state 不解析单元格样式，所以样式
/// 增量帧只能走这一路。
pub(crate) fn encode_diff_scratch(
    term: &mut NativeTerminal,
    state: *mut c_void,
    meta: &DiffFrameMeta,
    out: &mut Vec<u8>,
    scratch: &mut DiffScratch,
) -> Result<(), String> {
    out.clear();
    out.reserve(WIRE_DIFF_HEADER_LEN);

    // ---- 帧头：脏行数与样式条数要遍历后才知道，先占位再回填 ----
    out.extend_from_slice(&WIRE_DIFF_MAGIC.to_le_bytes());
    out.extend_from_slice(&meta.cols.to_le_bytes());
    out.extend_from_slice(&meta.rows.to_le_bytes());
    let flags_at = out.len();
    out.extend_from_slice(&0u16.to_le_bytes());
    let fg_at = out.len();
    out.extend_from_slice(&0i32.to_le_bytes());
    let bg_at = out.len();
    out.extend_from_slice(&0i32.to_le_bytes());
    let cursor_color_at = out.len();
    out.extend_from_slice(&0i32.to_le_bytes());
    let cursor_style_at = out.len();
    out.extend_from_slice(&0u32.to_le_bytes());
    let cursor_x_at = out.len();
    out.extend_from_slice(&0u16.to_le_bytes());
    let cursor_y_at = out.len();
    out.extend_from_slice(&0u16.to_le_bytes());
    let style_count_at = out.len();
    out.extend_from_slice(&0u16.to_le_bytes());
    let row_count_at = out.len();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&meta.scroll_delta.to_le_bytes());
    out.extend_from_slice(&meta.scrollbar.total.to_le_bytes());
    out.extend_from_slice(&meta.scrollbar.offset.to_le_bytes());
    out.extend_from_slice(&meta.scrollbar.len.to_le_bytes());

    // ---- 光标与默认色 ----
    let mut cursor = scratch.cursor;
    cursor.size = std::mem::size_of::<GhosttyRenderStateCursor>();
    let cursor_ok = unsafe {
        ghostty_render_state_get(
            state,
            RS_DATA_CURSOR,
            &mut cursor as *mut GhosttyRenderStateCursor as *mut c_void,
        )
    } == 0;
    let mut flags = 0u16;
    if cursor_ok && cursor.visible {
        flags |= 1;
    }
    // 待换行标志：终端数据键 5，与全量帧同源
    if term.read_flag(DATA_CURSOR_PENDING_WRAP) {
        flags |= 1 << 1;
    }
    out[flags_at..flags_at + 2].copy_from_slice(&flags.to_le_bytes());
    out[fg_at..fg_at + 4].copy_from_slice(
        &meta
            .default_fg
            .map(|v| v as i32)
            .unwrap_or(-1)
            .to_le_bytes(),
    );
    out[bg_at..bg_at + 4].copy_from_slice(
        &meta
            .default_bg
            .map(|v| v as i32)
            .unwrap_or(-1)
            .to_le_bytes(),
    );
    out[cursor_color_at..cursor_color_at + 4].copy_from_slice(
        &meta
            .cursor_color
            .or_else(|| cursor_color_of(state))
            .map(|v| v as i32)
            .unwrap_or(-1)
            .to_le_bytes(),
    );
    out[cursor_style_at..cursor_style_at + 4]
        .copy_from_slice(&(if cursor_ok { cursor.visual_style } else { 1 }).to_le_bytes());
    // 光标坐标：render state 给的是视口坐标，正是前端绘制所需。
    // 不可信（光标在历史区 / alt screen）时给 0,0 —— 光标行一定是脏行，
    // 前端会在下一帧拿到正确值，不会永久错位。
    let cx = if cursor_ok && cursor.viewport_has_value {
        cursor.viewport_x
    } else {
        0
    };
    let cy = if cursor_ok && cursor.viewport_has_value {
        cursor.viewport_y
    } else {
        0
    };
    out[cursor_x_at..cursor_x_at + 2].copy_from_slice(&cx.to_le_bytes());
    out[cursor_y_at..cursor_y_at + 2].copy_from_slice(&cy.to_le_bytes());

    // ---- 脏行 ----
    // 两步创建迭代器：`new` 只分配空壳，内部字段未初始化；必须再用
    // RS_DATA_ROW_ITERATOR 把它绑到本 render state。跳过第二步直接
    // next_dirty 会读野指针（实测 SIGSEGV）。
    let mut iterator: *mut c_void = std::ptr::null_mut();
    let rc = unsafe { ghostty_render_state_row_iterator_new(std::ptr::null(), &mut iterator) };
    if rc != 0 || iterator.is_null() {
        return Err(crate::error_proto::coded_display(
            "ffi_row_iter_create_failed",
            rc,
        ));
    }
    let bind = unsafe {
        ghostty_render_state_get(
            state,
            RS_DATA_ROW_ITERATOR,
            &mut iterator as *mut *mut c_void as *mut c_void,
        )
    };
    if bind != 0 {
        unsafe { ghostty_render_state_row_iterator_free(iterator) };
        return Err(crate::error_proto::coded_display(
            "ffi_row_iter_bind_failed",
            bind,
        ));
    }
    let walk = collect_dirty_rows(term, iterator, meta.cols, meta.rows, out, scratch);
    unsafe { ghostty_render_state_row_iterator_free(iterator) };
    let row_count = walk?;

    // ---- 回填计数 ----
    out[style_count_at..style_count_at + 2]
        .copy_from_slice(&(scratch.table_len() as u16).to_le_bytes());
    out[row_count_at..row_count_at + 2].copy_from_slice(&(row_count as u16).to_le_bytes());
    Ok(())
}

/// 遍历脏行，返回写入的行数。
///
/// 行体（行头 + cells）先写进 `scratch.body` 这块跨帧复用的缓冲，最后整块
/// 拷进 `out` —— 帧级样式表必须排在所有行体之前，而表长要遍历完才知道，
/// 所以不能边遍历边直接写 `out`。也正因如此，行体不能按行新建 Vec：
/// 满屏刷新时那是每帧 40 次堆分配（实测过的旧写法）。
fn collect_dirty_rows(
    term: &mut NativeTerminal,
    iterator: *mut c_void,
    cols: u16,
    rows: u16,
    out: &mut Vec<u8>,
    scratch: &mut DiffScratch,
) -> Result<u16, String> {
    scratch.body.clear();
    scratch.table_reset();
    scratch.clusters.reset();

    let mut count: u16 = 0;
    let mut y: u16 = 0;
    while unsafe { ghostty_render_state_row_iterator_next_dirty(iterator, &mut y) } {
        if y >= rows {
            // resize 与读线程并发的瞬态：脏标记属于旧网格尺寸。
            // 跳过而不是写穿前端的帧缓存。
            continue;
        }
        encode_dirty_row(term, iterator, cols, y, scratch)?;
        count = count.saturating_add(1);
        if count == u16::MAX {
            break;
        }
    }

    // 样式表统一放在行数据之前（跨行共享），cells 里存的是它的全局下标。
    let table_start = out.len();
    out.resize(table_start + scratch.table_len() * WIRE_STYLE_LEN, 0);
    scratch.write_table(&mut out[table_start..]);
    out.extend_from_slice(&scratch.body);

    // 帧尾：字素簇表。行标记已随各行行头送达，这里只补簇。
    let cluster_count = scratch.clusters.len().min(u16::MAX as usize);
    out.extend_from_slice(&WIRE_DIFF_FOOTER_MAGIC.to_le_bytes());
    out.extend_from_slice(&(cluster_count as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    for cluster in &scratch.clusters.entries[..cluster_count] {
        out.extend_from_slice(&(cluster.len() as u32).to_le_bytes());
        out.extend_from_slice(cluster.as_bytes());
    }
    Ok(count)
}

/// 编码一条脏行。
///
/// 内容走 render state 的 `CELLS_RAW` 视图（一次 FFI 取整行），样式走
/// `grid_ref_style`（与 `snapshot()` 同一套解析）。两条路都要是因为
/// render state **不解析单元格样式** —— 它只给原始 cell 值与显式 RGB
/// 背景，SGR 属性（粗体/斜体）与前景色必须走 terminal 的 grid ref。
fn encode_dirty_row(
    term: &mut NativeTerminal,
    iterator: *mut c_void,
    cols: u16,
    y: u16,
    scratch: &mut DiffScratch,
) -> Result<(), String> {
    // 行头先落位：8 字节（row + reserved + row_flags + pad）保证 cells 起点
    // 在 4 字节边界上，前端才能零拷贝建 Uint32Array 视图。
    // row_flags 的 bit0/bit1 是 OSC 133 语义提示，随行头一起送达。
    let (row_flag, has_grapheme) = term.row_info(y);
    scratch.body.extend_from_slice(&y.to_le_bytes());
    scratch.body.extend_from_slice(&0u16.to_le_bytes()); // reserved
    scratch
        .body
        .extend_from_slice(&(row_flag as u16).to_le_bytes());
    scratch.body.extend_from_slice(&0u16.to_le_bytes()); // pad：把行头撑到 8 字节

    // 一次 FFI 取整行的原始 cell 数组。视图只在「下一次 render state
    // update 之前」有效，因此必须在同一个 update 周期内消费完 ——
    // 本函数正是那么做的（拿完立刻逐格解码）。
    let mut view = scratch.view;
    let rc = unsafe {
        ghostty_render_state_row_get(
            iterator,
            RS_ROW_DATA_CELLS_RAW,
            &mut view as *mut GhosttyCellsView as *mut c_void,
        )
    };
    if rc != 0 || view.ptr.is_null() {
        return Err(crate::error_proto::coded(
            "ffi_row_view_failed",
            format!("row {y}: {rc}"),
        ));
    }

    let n = (view.len as u16).min(cols);
    let mut cluster_stack = [0u32; CLUSTER_STACK_CODEPOINTS];
    for x in 0..n {
        let raw = unsafe { *view.ptr.add(x as usize) };
        let (codepoint, wide, has_text) = unpack_cell(raw)?;
        // 样式按格解析，但带「与上一格逐字节相同就复用」的缓存：
        // 一行里同样式连续成段是常态（一段同色文本、一片空白），
        // 而 resolve_color 每次都要做标签分支 + 调色板索引。
        let style = term.resolve_cell_style(x, y, &mut scratch.style_cache);
        let style_idx = scratch.intern(style.0, style.1, style.2);

        // 字素簇：复用 `resolve_cell_style` 已经定位好的 grid ref，
        // 每格只多一次 `grid_ref_graphemes`（且整行无簇时一次都没有）。
        //
        // 没用 render state 的 row cells API（`GRAPHEMES_LEN` +
        // `GRAPHEMES_UTF8`）：那条路每格要 select/next + len + utf8 三次
        // FFI，还多一个句柄生命周期，比这里贵。行级 `GRAPHEME` 标记
        // 已经把普通文本行的成本降到 0。
        let mut cluster_index = 0u32;
        if codepoint != 0 && has_grapheme {
            if let Some((len, in_heap)) = read_cluster(
                &term.grid_ref,
                &mut cluster_stack,
                &mut scratch.cluster_heap,
            ) {
                if len > 1 {
                    let cluster = {
                        let cps =
                            cluster_slice(&cluster_stack, &scratch.cluster_heap, len, in_heap);
                        cluster_utf8(cps)
                    };
                    cluster_index = scratch.clusters.intern(&cluster);
                }
            }
        }

        scratch.body.extend_from_slice(
            &pack_wire(codepoint, wide, has_text, style_idx, cluster_index).to_le_bytes(),
        );
    }
    // 行尾补齐：尺寸变化瞬间行可能短于 cols，缺格补空白，
    // 否则前端帧缓存里残留上一行的旧内容。
    let blank_idx = scratch.intern(None, None, 0);
    let blank = pack_wire(0, 0, false, blank_idx, 0).to_le_bytes();
    for _ in n..cols {
        scratch.body.extend_from_slice(&blank);
    }
    Ok(())
}

/// 从打包 cell 值取 (codepoint, wide, has_text)。
///
/// 走 `ghostty_cell_get` 而非手拆位域：头文件明确不保证 ABI，
/// 位域位置以 `ghostty_type_json` 为准。
fn unpack_cell(raw: u64) -> Result<(u32, u8, bool), String> {
    let mut codepoint: u32 = 0;
    let mut wide: u32 = 0;
    let mut has_text = false;
    let rc = unsafe {
        ghostty_cell_get(
            raw,
            CELL_DATA_CODEPOINT,
            &mut codepoint as *mut u32 as *mut c_void,
        )
    }
    .max(unsafe { ghostty_cell_get(raw, CELL_DATA_WIDE, &mut wide as *mut u32 as *mut c_void) })
    .max(unsafe {
        ghostty_cell_get(
            raw,
            CELL_DATA_HAS_TEXT,
            &mut has_text as *mut bool as *mut c_void,
        )
    });
    if rc != 0 {
        return Err(crate::error_proto::coded_display(
            "ffi_cell_read_failed",
            rc,
        ));
    }
    Ok((codepoint, wide as u8, has_text))
}

/// render state 的显式光标色（未设置时 None）。
fn cursor_color_of(state: *mut c_void) -> Option<u32> {
    let mut colors = GhosttyRenderStateColors {
        size: std::mem::size_of::<GhosttyRenderStateColors>(),
        ..GhosttyRenderStateColors::default()
    };
    let rc = unsafe {
        ghostty_render_state_get(
            state,
            RS_DATA_COLORS,
            &mut colors as *mut GhosttyRenderStateColors as *mut c_void,
        )
    };
    if rc != 0 || !colors.cursor_has_value {
        return None;
    }
    Some(rgb_packed(&colors.cursor))
}

/// 引擎自身信息，用于界面展示与诊断。
///
/// 数据来自引擎自省，而非编译期常量 —— 换库版本时界面会跟着变。
pub fn engine_info() -> Result<serde_json::Value, String> {
    let ptr = unsafe { ghostty_type_json() };
    if ptr.is_null() {
        return Err("引擎未提供类型清单".to_string());
    }
    let json = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| format!("类型清单非 UTF-8: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("类型清单非 JSON: {e}"))?;
    Ok(serde_json::json!({
        "libraryVersion": v["library_version"],
        "commit": v["commit"],
        "target": v["abi"]["target"],
        "pointerSize": v["abi"]["pointer_size"],
        "endian": v["abi"]["endian"],
        "backend": "native",
    }))
}

#[cfg(test)]
mod engine_info_tests {
    use super::*;

    #[test]
    fn reports_native_engine_identity() {
        let info = engine_info().expect("应能读取引擎信息");
        assert_eq!(info["backend"], "native");
        assert_eq!(info["pointerSize"], 8, "原生应为 64 位指针");
        assert!(
            info["libraryVersion"]
                .as_str()
                .is_some_and(|v| !v.is_empty()),
            "应报告库版本"
        );

        // 与 wasm 的关键差异：指针宽度必须如实反映，前端据此判断后端类型
        assert_ne!(info["pointerSize"], 4, "4 字节指针意味着仍在 wasm 后端");
    }
}

#[cfg(test)]
mod wire {
    use super::*;

    struct Decoded {
        cols: u16,
        rows: u16,
        cursor_x: u16,
        cursor_y: u16,
        flags: u16,
        cursor_style: u32,
        scrollbar: (u64, u64, u64),
        styles: Vec<(i32, i32, u32)>,
        cells: Vec<u64>,
        /// 帧尾的字素簇表（旧帧没有帧尾时为空）
        clusters: Vec<String>,
        /// 帧尾的每行标记（旧帧没有帧尾时为空）
        row_flags: Vec<u8>,
    }

    /// 前端解码逻辑的 Rust 镜像，用于验证协议自洽（`Decoded`）。
    /// 引擎的 cursor_x 必须等于「已写入字符占据的格数」。
    ///
    /// 这条不变量是前端逐格绘制的前提：绘制侧按 `col * cellW` 锚定每个字形，
    /// 光标按 `cursorX * cellW` 画。两者一致，文字末端才与光标格缘重合。
    /// 曾经前端把整 run 合并成一次 fillText（按字体的自然步进排列），
    /// 而格宽是取整值（JetBrains Mono 13px：7.83 vs 8），一行 44 列累计
    /// 7.6px，表现为「行尾文字与光标之间多出一个空格」。
    #[test]
    fn cursor_x_matches_written_column_count() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        t.write(b"user@host demo % hello");
        let snap = t.snapshot();
        assert_eq!(snap.cursor_x as usize, "user@host demo % hello".len());
        assert!(!snap.cursor_pending_wrap, "未写满行，pending wrap 不该置位");
    }

    /// 宽字按两格计：4 个 CJK 后 cursor_x 必须是 8，不是 4。
    ///
    /// 前端把宽字画在 `col * cellW`、占两个格宽；cursor_x 少算一半就会让
    /// 光标落在一个宽字中间。
    #[test]
    fn cursor_x_counts_wide_chars_as_two_cells() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        t.write("中文测试".as_bytes());
        assert_eq!(t.cursor(), (8, 0));
    }

    /// 混合 ASCII/宽字后 cursor_x 也必须等于总占格数。
    #[test]
    fn cursor_x_counts_mixed_widths() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        t.write("hello中文".as_bytes());
        assert_eq!(
            t.cursor(),
            (9, 0),
            "5 个 ASCII + 2 个宽字（各占 2 格）= 5 + 4"
        );
    }

    fn decode(buf: &[u8]) -> Decoded {
        let rd_u32 = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        let rd_u16 = |o: usize| u16::from_le_bytes(buf[o..o + 2].try_into().unwrap());
        let rd_i32 = |o: usize| i32::from_le_bytes(buf[o..o + 4].try_into().unwrap());

        assert_eq!(rd_u32(0), WIRE_MAGIC, "magic 不匹配");

        let cols = rd_u16(4);
        let rows = rd_u16(6);
        let cursor_x = rd_u16(8);
        let cursor_y = rd_u16(10);
        let style_count = rd_u16(12) as usize;
        let flags = rd_u16(14);

        let mut styles = Vec::with_capacity(style_count);
        let mut off = WIRE_HEADER_LEN;
        for _ in 0..style_count {
            styles.push((rd_i32(off), rd_i32(off + 4), rd_u32(off + 8)));
            off += WIRE_STYLE_LEN;
        }

        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for _ in 0..(cols as usize * rows as usize) {
            cells.push(u64::from_le_bytes(buf[off..off + 8].try_into().unwrap()));
            off += WIRE_CELL_LEN;
        }

        // 帧尾是可选的：旧帧到 cells 结束就没了。必须容忍，
        // 否则一次灰度升级期间新旧两侧无法互通。
        let mut clusters = Vec::new();
        let mut row_flags = Vec::new();
        if buf.len() >= off + 8 && rd_u32(off) == WIRE_FOOTER_MAGIC {
            let cluster_count = rd_u16(off + 4) as usize;
            let flag_count = rd_u16(off + 6) as usize;
            off += 8;
            for _ in 0..flag_count {
                row_flags.push(rd_u16(off) as u8);
                off += 2;
            }
            for _ in 0..cluster_count {
                let len = rd_u32(off) as usize;
                off += 4;
                clusters.push(String::from_utf8(buf[off..off + len].to_vec()).unwrap());
                off += len;
            }
        }
        assert_eq!(off, buf.len(), "缓冲长度与头部声明不符");

        Decoded {
            cols,
            rows,
            cursor_x,
            cursor_y,
            flags,
            cursor_style: rd_u32(28),
            scrollbar: (
                u64::from_le_bytes(buf[32..40].try_into().unwrap()),
                u64::from_le_bytes(buf[40..48].try_into().unwrap()),
                u64::from_le_bytes(buf[48..56].try_into().unwrap()),
            ),
            styles,
            cells,
            clusters,
            row_flags,
        }
    }

    fn cell_codepoint(packed: u64) -> u32 {
        (packed & CELL_CODEPOINT_MASK) as u32
    }

    #[test]
    fn roundtrips_text_and_dimensions() {
        let mut t = NativeTerminal::new(12, 3).unwrap();
        t.write(b"hi wire");
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        assert_eq!((d.cols, d.rows), (12, 3));
        assert_eq!(d.cells.len(), 36);
        assert_eq!(cell_codepoint(d.cells[0]), 'h' as u32);
        assert_eq!(cell_codepoint(d.cells[1]), 'i' as u32);
        assert_eq!(d.cursor_x, 7);
        assert_eq!(d.cursor_y, 0);
        // 前端对这三位各有还原检查（verifyWire 的 flags/scrollbar 段），
        // Rust 侧的镜像解码曾把它们解析出来却不断言——改了偏移也发现不了。
        // 这里对齐前端：默认终端不可见待换行、滚动读数为零占位。
        assert_eq!(d.flags & 0b1, 1, "bit0 = cursor visible，默认应置位");
        assert_eq!(d.flags & 0b10, 0, "bit1 = pending wrap，默认应清零");
        assert_eq!(
            d.cursor_style, 1,
            "默认光标形状为 block(1)，见线格式注释 0=bar 1=block"
        );
        // 12×3 视口只有 3 行缓冲区，没有可滚动余量，total == len == 3
        assert_eq!(
            d.scrollbar,
            (3, 0, 3),
            "视口等于缓冲时 total == len，offset 为 0"
        );
    }

    #[test]
    fn roundtrips_cursor_and_scrollbar_readouts() {
        let mut t = NativeTerminal::new(6, 4).unwrap();
        // 写入超过一屏的行，制造真实滚动读数
        for i in 0..12 {
            t.write(format!("line{i}\r\n").as_bytes());
        }
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        // 12 行内容进 4 行视口：total/len 必然非零且 len 不超过 total
        assert!(d.scrollbar.0 > 0, "总行数应为正，实际 {:?}", d.scrollbar);
        assert!(
            d.scrollbar.2 > 0 && d.scrollbar.2 <= d.scrollbar.0,
            "可见行数应介于 0 与总行数之间，实际 {:?}",
            d.scrollbar
        );
        // 光标停在最后一行
        assert_eq!(d.cursor_y, 3, "写满后光标应落在末行");
    }

    #[test]
    fn mirrors_pending_wrap_flag() {
        let mut t = NativeTerminal::new(4, 2).unwrap();
        // 恰好写满一行但不换行：光标停在行尾且 pending-wrap 置位
        t.write(b"abcd");
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        assert_eq!(
            d.flags & 0b10,
            0b10,
            "bit1 = pending wrap，写满一行后应置位"
        );
        assert_eq!(d.cursor_x, 3, "pending wrap 时 x 停在最后一格");
    }

    #[test]
    fn preserves_truecolor_through_style_table() {
        let mut t = NativeTerminal::new(8, 2).unwrap();
        t.write(b"\x1b[38;2;10;200;90mZ");
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        let style_idx = ((d.cells[0] >> 23) & 0xffff) as usize;
        let (fg, _bg, _flags) = d.styles[style_idx];
        assert_eq!(fg, 0x0ac85a, "真彩应穿过样式表无损到达前端");
    }

    #[test]
    fn deduplicates_repeated_styles() {
        let mut t = NativeTerminal::new(20, 2).unwrap();
        // 20 个字符同一颜色
        t.write(b"\x1b[31mAAAAAAAAAAAAAAAAAAAA");
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        // 40 格里只有两种样式：红色前景 与 默认（空白行）
        assert_eq!(d.styles.len(), 2, "样式表应被压缩到实际种类数");

        // 去重的实质：同一颜色的 20 格必须指向同一个样式索引
        let idx0 = (d.cells[0] >> 23) & 0xffff;
        for i in 1..20 {
            assert_eq!((d.cells[i] >> 23) & 0xffff, idx0, "同色格应共享样式索引");
        }

        let (fg, _bg, _flags) = d.styles[idx0 as usize];
        assert_eq!(fg, 0xcc6666, "共享索引应解析出正确前景色");
    }

    #[test]
    fn carries_attribute_flags() {
        let mut t = NativeTerminal::new(8, 2).unwrap();
        t.write(b"\x1b[1;3;4mX");
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        let style_idx = ((d.cells[0] >> 23) & 0xffff) as usize;
        let (_fg, _bg, flags) = d.styles[style_idx];
        assert!(flags & style_flags::BOLD != 0);
        assert!(flags & style_flags::ITALIC != 0);
        // 下划线在 flags 中不单列（渲染用不到），此处只验证粗体/斜体透传
    }

    #[test]
    fn encodes_wide_char_spacers() {
        let mut t = NativeTerminal::new(8, 2).unwrap();
        t.write("中文".as_bytes());
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        let wide0 = (d.cells[0] >> 21) & 0b11;
        let wide1 = (d.cells[1] >> 21) & 0b11;
        assert_eq!(wide0, 1, "首字应标记 WIDE");
        assert_eq!(wide1, 2, "第二格应标记 SPACER_TAIL");
    }

    #[test]
    fn has_text_bit_survives_encoding() {
        let mut t = NativeTerminal::new(8, 2).unwrap();
        t.write(b"A");
        let snap = t.snapshot();
        let d = decode(&encode_snapshot(&snap));

        assert_eq!((d.cells[0] >> 39) & 1, 1, "有字形的格应置位");
        assert_eq!((d.cells[1] >> 39) & 1, 0, "空白格不应置位");
    }

    /// 增量帧解码器：前端 `decodeCellDiff` 的 Rust 镜像。
    /// 两侧必须同步改，否则协议自洽而两端对不上。
    struct DiffDecoded {
        cols: u16,
        grid_rows: u16,
        cursor_x: u16,
        cursor_y: u16,
        cursor_visible: bool,
        row_count: u16,
        scrollbar: (u64, u64, u64),
        /// 帧级样式表：(fg, bg, flags)
        styles: Vec<(i32, i32, u32)>,
        rows: Vec<(u16, Vec<u64>)>,
        /// 行号 → 行头 row_flags
        row_flags: Vec<(u16, u16)>,
        /// 帧尾的字素簇表（旧帧没有帧尾时为空）
        clusters: Vec<String>,
    }

    fn decode_diff(buf: &[u8]) -> DiffDecoded {
        let rd_u32 = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        let rd_u16 = |o: usize| u16::from_le_bytes(buf[o..o + 2].try_into().unwrap());

        assert_eq!(rd_u32(0), WIRE_DIFF_MAGIC, "增量帧 magic 不匹配");
        let cols = rd_u16(4);
        let grid_rows = rd_u16(6);
        let flags = rd_u16(8);
        let cursor_x = rd_u16(26);
        let cursor_y = rd_u16(28);
        let style_count = rd_u16(30) as usize;
        let row_count = rd_u16(32);

        let rd_i32 = |o: usize| i32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        let mut styles_out = Vec::with_capacity(style_count);
        for i in 0..style_count {
            let base = WIRE_DIFF_HEADER_LEN + i * WIRE_STYLE_LEN;
            styles_out.push((rd_i32(base), rd_i32(base + 4), rd_u32(base + 8)));
        }

        let mut rows_out = Vec::new();
        let mut flags_out = Vec::new();
        let mut off = WIRE_DIFF_HEADER_LEN + style_count * WIRE_STYLE_LEN;
        for _ in 0..row_count {
            let y = rd_u16(off);
            let reserved = rd_u16(off + 2);
            assert_eq!(reserved, 0, "行头 reserved 应为 0");
            flags_out.push((y, rd_u16(off + 4)));
            off += WIRE_DIFF_ROW_HEADER_LEN;
            let mut cells = Vec::with_capacity(cols as usize);
            for _ in 0..cols {
                cells.push(u64::from_le_bytes(buf[off..off + 8].try_into().unwrap()));
                off += WIRE_CELL_LEN;
            }
            rows_out.push((y, cells));
        }

        // 帧尾（可选，旧帧没有）：u32 magic + u16 cluster_count + u16 reserved
        let mut clusters = Vec::new();
        if buf.len() >= off + 8 && rd_u32(off) == WIRE_DIFF_FOOTER_MAGIC {
            let cluster_count = rd_u16(off + 4) as usize;
            let reserved = rd_u16(off + 6);
            assert_eq!(reserved, 0, "帧尾 reserved 应为 0");
            off += 8;
            for _ in 0..cluster_count {
                let len = rd_u32(off) as usize;
                off += 4;
                clusters.push(String::from_utf8(buf[off..off + len].to_vec()).unwrap());
                off += len;
            }
        }
        assert_eq!(off, buf.len(), "增量帧长度与声明不符");

        DiffDecoded {
            cols,
            grid_rows,
            cursor_x,
            cursor_y,
            cursor_visible: flags & 1 == 1,
            row_count,
            scrollbar: (
                u64::from_le_bytes(buf[36..44].try_into().unwrap()),
                u64::from_le_bytes(buf[44..52].try_into().unwrap()),
                u64::from_le_bytes(buf[52..60].try_into().unwrap()),
            ),
            styles: styles_out,
            rows: rows_out,
            row_flags: flags_out,
            clusters,
        }
    }

    /// 全量差分帧（full=true）必须与 `snapshot_wire` 完全一致。
    ///
    /// 含样式索引：两条路径走同一套样式解析（`grid_ref_style` +
    /// 同样的样式表去重），因此 packed u64 应当逐位相同。这条断言是
    /// 增量路径正确性的最强证明 —— 内容、样式、去重顺序全对齐。
    #[test]
    fn full_diff_matches_snapshot_wire() {
        let mut t = NativeTerminal::new(10, 3).unwrap();
        t.write(b"\x1b[31mred\x1b[0m \x1b[1;3mbold italic\x1b[0m");
        let d_full = decode_diff(&t.snapshot_wire_diff(true));
        let d_whole = decode(&t.snapshot_wire());

        assert_eq!(
            (d_full.cols, d_full.grid_rows),
            (d_whole.cols, d_whole.rows)
        );
        assert_eq!(d_full.row_count, 3, "全量帧应覆盖每一行");
        assert_eq!(d_full.rows.len(), 3);

        for (row, (y, cells)) in d_full.rows.iter().enumerate() {
            assert_eq!(*y as usize, row, "行号应按序");
            for (x, packed) in cells.iter().enumerate() {
                assert_eq!(
                    *packed,
                    d_whole.cells[row * d_whole.cols as usize + x],
                    "({x},{row}) 两条路径编码不一致"
                );
            }
        }
    }

    /// 增量帧必须带样式：否则彩色输出在增量更新后掉色。
    ///
    /// 这条测试盯着一个真实的取舍 —— render state 不解析单元格样式，
    /// 增量路径必须自己走 `grid_ref_style`。没有它，前端会先把红字画对，
    /// 下一帧增量更新后变成默认色。
    #[test]
    fn diff_carries_styles_for_dirty_rows() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        let _ = t.snapshot_wire_diff(true);
        // 第二行写红字
        t.write(b"\x1b[2;1H\x1b[31mRED\x1b[0m");
        let d = decode_diff(&t.snapshot_wire_diff(false));
        let target = d.rows.iter().find(|(y, _)| *y == 1).expect("第 1 行应脏");
        let idx = ((target.1[0] >> 23) & 0xffff) as usize;
        let (fg, _bg, _flags) = d.styles[idx];
        assert_eq!(fg, 0xcc6666, "增量帧必须解析索引色为调色板值");
        assert!(!d.styles.is_empty(), "脏行必须附带样式表");
    }

    /// 核心收益：只送变动的行。
    ///
    /// 注意不能断言「恰好一行」：光标从 (0,0) 移到 (7,2) 时，**旧光标
    /// 所在行也 genuinely 变了**（那格的字符要被还原）。引擎标记两行是
    /// 正确行为，所以这里断言的是「远少于总行数」。
    #[test]
    fn diff_emits_only_dirty_rows() {
        let mut t = NativeTerminal::new(10, 5).unwrap();
        assert_eq!(decode_diff(&t.snapshot_wire_diff(true)).row_count, 5);

        t.write(b"\x1b[3;1Hchanged");
        let d = decode_diff(&t.snapshot_wire_diff(false));
        assert!(
            d.row_count < 5,
            "只改一处时应远少于全部行数，实际 {} 行",
            d.row_count
        );
        assert!(
            d.rows.iter().any(|(y, _)| *y == 2),
            "被修改的第 2 行必须在帧里"
        );
        for (y, cells) in &d.rows {
            assert_eq!(cells.len(), 10, "行 {y} 应送整行宽度");
        }

        // 差分帧已消费，再来一次应为空：否则脏标记没被归还
        let empty = decode_diff(&t.snapshot_wire_diff(false));
        assert_eq!(empty.row_count, 0, "已消费的帧不应重复发送");
    }

    /// 帧大小即收益的量度。
    #[test]
    fn diff_is_much_smaller_than_full() {
        let mut t = NativeTerminal::new(120, 40).unwrap();
        t.write(b"start");
        let whole = t.snapshot_wire().len();
        let _ = t.snapshot_wire_diff(true);
        t.write(b"x"); // 回显一个字符 = 一行脏
        let diff = t.snapshot_wire_diff(false).len();
        assert!(diff < whole / 8, "差分帧 {diff}B 应远小于全量帧 {whole}B");
    }

    /// 光标位置必须随输入前移（否则回显与闪烁不更新）。
    #[test]
    fn diff_tracks_cursor_movement() {
        let mut t = NativeTerminal::new(20, 4).unwrap();
        let _ = t.snapshot_wire_diff(true);
        t.write(b"ab");
        let d = decode_diff(&t.snapshot_wire_diff(false));
        assert!(d.cursor_visible);
        assert_eq!(d.cursor_x, 2);
        assert_eq!(d.cursor_y, 0);
    }

    /// 滚动到历史区后帧仍自洽，脏行号不越界。
    ///
    /// 不断言 total > len：输出刚好等于一屏时视口贴底，offset + len == total。
    #[test]
    fn diff_survives_scrollback() {
        let mut t = NativeTerminal::new(10, 3).unwrap();
        let _ = t.snapshot_wire_diff(true);
        for i in 1..=8 {
            t.write(format!("L{i}\r\n").as_bytes());
        }
        let d = decode_diff(&t.snapshot_wire_diff(false));
        assert!(
            d.scrollbar.0 >= d.scrollbar.2,
            "total 应不小于 len（total={} len={}）",
            d.scrollbar.0,
            d.scrollbar.2
        );
        for (y, _) in &d.rows {
            assert!(*y < d.grid_rows, "脏行号不应越界: {y} >= {}", d.grid_rows);
        }
    }

    /// 连续多帧差分后内容仍与全量帧一致：增量合并不能漂移。
    #[test]
    fn repeated_diffs_stay_consistent() {
        let mut t = NativeTerminal::new(12, 4).unwrap();
        let _ = t.snapshot_wire_diff(true);
        for chunk in [b"ab".as_slice(), b"\r\n", b"cd", b"\r\n", b"ef"] {
            t.write(chunk);
            let d = decode_diff(&t.snapshot_wire_diff(false));
            // 每帧都要能自洽解码（长度、行号）
            for (y, cells) in &d.rows {
                assert!(*y < d.grid_rows);
                assert_eq!(cells.len(), 12);
            }
        }
        // 末态与全量比对本帧的脏行
        let mut merged = decode_diff(&t.snapshot_wire_diff(true));
        let whole = decode(&t.snapshot_wire());
        let content = |v: u64| v & 0x00ff_ff80_007f_ffff;
        for (y, cells) in std::mem::take(&mut merged.rows) {
            for (x, packed) in cells.iter().enumerate() {
                assert_eq!(
                    content(*packed),
                    content(whole.cells[y as usize * whole.cols as usize + x]),
                    "({x},{y}) 增量合并后与全量不一致"
                );
            }
        }
    }

    /// 滚轮翻看历史后，增量帧必须带着滚动后的视口内容。
    ///
    /// 这是「终端滚不动 / 滚动后下半屏空白」的根因：`scroll_viewport`
    /// 把视口移进历史区，但 render state 的脏标记只由「内容变化」累积，
    /// 滚动本身不产生脏行。前端收到 dirty 后拉一帧增量，帧里一行都没有，
    /// 画面就永久停在上次的内容上 —— 用户看见屏幕下半部分不再刷新。
    ///
    /// 滚动条位移也只是前端用来挪本地选区端点，不触发任何行重绘。
    #[test]
    fn scroll_viewport_marks_rows_dirty() {
        let mut t = NativeTerminal::new(10, 4).unwrap();
        let _ = t.snapshot_wire_diff(true);
        for i in 1..=12 {
            t.write(format!("L{i}\r\n").as_bytes());
        }
        // 基准：同一串内容未滚动时的视口 offset
        let mut fresh = NativeTerminal::new(10, 4).unwrap();
        for i in 1..=12 {
            fresh.write(format!("L{i}\r\n").as_bytes());
        }
        let at_bottom = fresh.snapshot().scrollbar.offset;

        // 消费掉写入产生的脏行，只留滚动本身
        let _ = t.snapshot_wire_diff(false);

        t.scroll_delta(-3);
        let d = decode_diff(&t.snapshot_wire_diff(false));
        assert_ne!(
            d.scrollbar.1, at_bottom,
            "滚动后视口 offset 应离开底部（{} vs {at_bottom}）",
            d.scrollbar.1
        );
        assert_eq!(
            d.row_count, d.grid_rows,
            "滚动后必须整屏重绘：引擎不会因视口移动标脏，增量帧只送脏行 \
             （row_count={} rows={}）",
            d.row_count, d.grid_rows
        );
    }

    /// `e` + U+0301 必须作为**一个**字素簇送达：基码点仍是 `e`，
    /// 组合符在簇里；否则前端会画成两个字形（一个孤立的组合符）。
    #[test]
    fn combining_mark_roundtrips_as_one_cluster() {
        let mut t = NativeTerminal::new(8, 2).unwrap();
        t.write("e\u{301}".as_bytes());
        let snap = t.snapshot();

        assert_eq!(snap.cells[0].codepoint, 'e' as u32, "基码点应是 e");
        assert_eq!(snap.cells[0].cluster_index, 1, "该格应指向第 1 个簇");
        assert_eq!(snap.clusters, vec!["e\u{301}".to_string()]);

        // 线格式往返：簇表与格内下标都要原样到达
        let d = decode(&encode_snapshot(&snap));
        assert_eq!(d.clusters, vec!["e\u{301}".to_string()]);
        let idx = ((d.cells[0] >> CELL_CLUSTER_SHIFT) & CELL_CLUSTER_MASK) as usize;
        assert_eq!(idx, 1, "线格式 cluster 位应存「下标 + 1」");
        assert_eq!(d.clusters[idx - 1], "e\u{301}");
    }

    /// 家庭 emoji（7 码点 ZWJ 序列）必须整簇送达，不能只剩第一个码点。
    #[test]
    fn family_emoji_is_one_cluster() {
        let mut t = NativeTerminal::new(8, 2).unwrap();
        t.write("\x1b[?2027h👨‍👩‍👧‍👦".as_bytes());
        let snap = t.snapshot();
        assert_eq!(
            snap.clusters,
            vec!["👨‍👩‍👧‍👦".to_string()],
            "整簇应只有一条（宽字尾格不重复）"
        );
        assert_eq!(snap.cells[0].codepoint, 0x1F468, "基码点应是首位成员");
        assert_eq!(snap.cells[0].cluster_index, 1);

        let d = decode(&encode_snapshot(&snap));
        assert_eq!(d.clusters, vec!["👨‍👩‍👧‍👦".to_string()]);
    }

    /// OSC 133 `A` 标记的 prompt 行必须置行标记 bit0，并穿过帧尾。
    #[test]
    fn osc133_marks_prompt_row() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        t.write(b"\x1b]133;A\x07$ ls\r\n");
        let snap = t.snapshot();

        assert_eq!(snap.row_flags.len(), 3, "每行一个标记字节");
        assert!(
            snap.row_flags[0] & row_flags::PROMPT != 0,
            "第 0 行应是 prompt 行，实际 {:?}",
            snap.row_flags
        );
        assert_eq!(
            snap.row_flags[1] & row_flags::PROMPT,
            0,
            "输出行不应带 prompt 标记"
        );

        let d = decode(&encode_snapshot(&snap));
        assert_eq!(d.row_flags, snap.row_flags, "行标记应原样穿过帧尾");
    }

    /// OSC 7 改变 cwd：回调把原始值存进上下文，`take_pwd_change` 只取一次。
    #[test]
    fn osc7_reports_pwd_change_once() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        t.write(b"\x1b]7;file://localhost/tmp/lazecode\x07");
        assert_eq!(
            t.take_pwd_change().as_deref(),
            Some("file://localhost/tmp/lazecode"),
            "OSC 7 的原始 URI 应原样上报"
        );
        assert_eq!(t.take_pwd_change(), None, "同一次变化只能被消费一次");

        // 空 OSC 7 表示 shell 清除了 pwd，也要作为一次变化上报。
        t.write(b"\x1b]7;\x07");
        assert_eq!(t.take_pwd_change().as_deref(), Some(""));
    }

    /// 增量帧的行头 row_flags 与字素簇必须和全量帧一致。
    ///
    /// 增量路径的簇来自 `grid_ref_graphemes`（复用样式解析定位好的
    /// grid ref），行标记来自行句柄；两者都不能因为「只送脏行」而丢失。
    #[test]
    fn diff_carries_cluster_and_row_flags() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        // 第 0 行：prompt 标记 + 一个带组合符的格（放在第 1 格，
        // 稍后只覆盖第 0 格，脏行里必须仍带这个簇）
        t.write("\x1b]133;A\x07xe\u{301}\r\n".as_bytes());

        let full = decode_diff(&t.snapshot_wire_diff(true));
        assert_eq!(full.clusters, vec!["e\u{301}".to_string()], "整屏帧应带簇");
        let row0 = full
            .rows
            .iter()
            .find(|(y, _)| *y == 0)
            .expect("第 0 行应脏");
        assert_eq!(
            (row0.1[1] >> CELL_CLUSTER_SHIFT) & CELL_CLUSTER_MASK,
            1,
            "第 1 格应指向簇 1"
        );
        assert!(
            full.row_flags
                .iter()
                .any(|(y, f)| *y == 0 && f & row_flags::PROMPT as u16 != 0),
            "整屏帧第 0 行应带 prompt 标记"
        );

        // 只改第 0 格：脏行仍须带同样的簇与行标记
        t.write(b"\x1b[1;1Hy");
        let diff = decode_diff(&t.snapshot_wire_diff(false));
        let dirty = diff
            .rows
            .iter()
            .find(|(y, _)| *y == 0)
            .expect("第 0 行应脏");
        assert_eq!(
            (dirty.1[0] >> CELL_CLUSTER_SHIFT) & CELL_CLUSTER_MASK,
            0,
            "被覆盖的格已无簇"
        );
        assert_eq!(
            (dirty.1[1] >> CELL_CLUSTER_SHIFT) & CELL_CLUSTER_MASK,
            1,
            "脏行里剩余的组合簇仍应带下标"
        );
        assert_eq!(diff.clusters, vec!["e\u{301}".to_string()]);
        assert!(
            diff.row_flags
                .iter()
                .any(|(y, f)| *y == 0 && f & row_flags::PROMPT as u16 != 0),
            "脏行行头必须带 prompt 标记"
        );
    }
}

/// 键盘编码：产出的字节直接写进 PTY，错了就是 shell 收到错误输入。
#[cfg(test)]
mod query_and_placeholder {
    use super::*;

    /// 引擎算好的查询回包必须落到 responses 缓冲里（PTY 读线程随后写回子进程）。
    ///
    /// 旧实现根本没装 WRITE_PTY：DA1/DECRQM/DSR 与 kitty 握手全部被丢弃，
    /// 任何按标准探测能力的程序都把我们当哑终端（omp 的内联图片探测就是这样超时的）。
    #[test]
    fn query_responses_are_collected_and_drained() {
        let mut term = NativeTerminal::new(80, 24).unwrap();

        term.write(b"\x1b[c");
        assert_eq!(
            String::from_utf8_lossy(&term.take_responses()),
            "\x1b[?62;22c",
            "DA1 应有回包"
        );
        assert!(term.take_responses().is_empty(), "取走后缓冲必须清空");

        term.write(b"\x1b[6n");
        assert_eq!(String::from_utf8_lossy(&term.take_responses()), "\x1b[1;1R");

        term.write(b"\x1b[?2026$p");
        assert_eq!(
            String::from_utf8_lossy(&term.take_responses()),
            "\x1b[?2026;2$y",
            "DECRQM 应按自身模式表作答"
        );

        // kitty 图形握手：TUI 用它确认终端真的支持图形协议
        term.write(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
        assert_eq!(
            String::from_utf8_lossy(&term.take_responses()),
            "\x1b_Gi=31;OK\x1b\\"
        );

        // 普通输出不产生任何回包
        term.write(b"hello world\r\n");
        assert!(term.take_responses().is_empty());
    }

    /// XTWINOPS 尺寸查询应按 resize 时给出的真实几何作答（TUI 靠 CSI 16 t
    /// 拿格子像素尺寸算图片布局）。
    #[test]
    fn size_queries_report_cell_geometry() {
        let mut term = NativeTerminal::new(80, 24).unwrap();
        term.write(b"\x1b[16t");
        assert!(
            term.take_responses().is_empty(),
            "格子像素未知时不应回 0x0 的假几何"
        );

        term.resize(80, 24, 9, 18).unwrap();
        term.write(b"\x1b[16t");
        assert_eq!(
            String::from_utf8_lossy(&term.take_responses()),
            "\x1b[6;18;9t"
        );
        term.write(b"\x1b[18t");
        assert_eq!(
            String::from_utf8_lossy(&term.take_responses()),
            "\x1b[8;24;80t"
        );
    }

    const PNG_2X2: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAIAAAD91JpzAAAAEElEQVR4nGP4z8AARAwQCgAf7gP9i18U1AAAAABJRU5ErkJggg==";

    /// TUI 常用 unicode 占位格贴图（omp 即如此）：先 `a=t` 传图，再 `a=p,U=1`
    /// 建虚拟 placement，最后在网格里铺 U+10EEEE 占位格（前景色 = 图片 id）。
    ///
    /// 引擎对虚拟 placement 不给坐标（render_info 的 col/row 明确无意义），旧实现
    /// 又按 `viewport_visible` 把它整条过滤掉 —— 于是这类图片永远画不出来。
    /// 这条测试锁住「从占位格反查左上角」这条链路。
    #[test]
    fn virtual_placement_position_comes_from_placeholder_cells() {
        let mut term = NativeTerminal::new(20, 6).unwrap();
        term.resize(20, 6, 9, 18).unwrap();

        let mut seq = format!("\x1b_Ga=t,f=100,q=2,i=7;{PNG_2X2}\x1b\\");
        seq.push_str("\x1b_Ga=p,U=1,q=2,i=7,c=2,r=1\x1b\\");
        // 第 4 行第 6 列（1 基）起两个占位格，前景色编码图片 id=7
        seq.push_str("\x1b[4;6H\x1b[38;2;0;0;7m\u{10EEEE}\u{10EEEE}");
        term.write(seq.as_bytes());

        let images = term.images();
        assert_eq!(images.images.len(), 1, "图片应已解码入库");
        assert_eq!((images.images[0].width, images.images[0].height), (2, 2));

        let p = images
            .placements
            .first()
            .expect("虚拟 placement 不应再被丢弃");
        assert!(p.is_virtual, "该 placement 应标记为虚拟");
        assert_eq!((p.x, p.y), (5, 3), "坐标应由占位格反查得到（0 基）");
        assert_eq!((p.cols, p.rows), (2, 1));
        assert!(term.image_rgba(7).is_some(), "像素应可取到");
    }

    const PNG_2X8: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAAICAIAAABcT7kVAAAAD0lEQVR4nGNgqPgPQqRSAICmF3G/zyrNAAAAAElFTkSuQmCC";

    /// 图片往上滚出视口时，被裁掉的顶部必须从采样源矩形里扣掉：
    /// 否则整张图会贴到视口顶边，显示的还是最上面几行（内容错位）。
    #[test]
    fn scrolled_virtual_placement_clips_source_rect() {
        let mut term = NativeTerminal::new(20, 4).unwrap();
        term.resize(20, 4, 9, 18).unwrap();

        // 2x8 像素的图，铺成 2 列 4 行（每行格 2 像素高）
        let mut seq = format!("\x1b_Ga=t,f=100,q=2,i=11;{PNG_2X8}\x1b\\");
        seq.push_str("\x1b_Ga=p,U=1,q=2,i=11,c=2,r=4\x1b\\");
        // 逐行绝对定位写占位格：尾随换行会把视口顶掉一行，干扰期望值
        for row in 0..4 {
            seq.push_str(&format!(
                "\x1b[{};1H\x1b[38;2;0;0;11m{}{}",
                row + 1,
                '\u{10EEEE}',
                '\u{10EEEE}'
            ));
        }
        term.write(seq.as_bytes());

        let full = term.images().placements.first().cloned().expect("整图可见");
        assert_eq!((full.x, full.y, full.rows), (0, 0, 4));
        assert_eq!(
            (full.source_y, full.source_height),
            (0, 8),
            "未裁剪应取整图"
        );

        // 再滚 2 行：顶部的 2 行占位格被推出视口
        term.write(b"\r\n\r\n");
        let clipped = term
            .images()
            .placements
            .first()
            .cloned()
            .expect("底部两行仍可见");
        assert_eq!(clipped.y, 0, "可见部分从视口第 0 行开始");
        assert_eq!(clipped.rows, 2, "只剩往下 2 行可见");
        assert_eq!(
            (clipped.source_y, clipped.source_height),
            (4, 4),
            "采样源矩形应扣掉被推出视口的上半张图"
        );
    }

    /// 占位格滚出视口后不该凭 0,0 画到左上角：没有占位格就没有可渲染位置。
    #[test]
    fn virtual_placement_without_visible_placeholders_is_not_emitted() {
        let mut term = NativeTerminal::new(20, 3).unwrap();
        term.resize(20, 3, 9, 18).unwrap();
        let mut seq = format!("\x1b_Ga=t,f=100,q=2,i=9;{PNG_2X2}\x1b\\");
        seq.push_str("\x1b_Ga=p,U=1,q=2,i=9,c=2,r=2\x1b\\");
        seq.push_str("\x1b[1;1H\x1b[38;2;0;0;9m\u{10EEEE}\u{10EEEE}");
        term.write(seq.as_bytes());

        let images = term.images();
        assert_eq!(images.placements.len(), 1, "视口内应能看到该虚拟 placement");
        // 把占位格顶出屏幕（3 行高，推 6 行）
        term.write(b"\r\n\r\n\r\n\r\n\r\n\r\n");
        let images = term.images();
        assert!(
            images.placements.is_empty(),
            "占位格不在视口时不应下发凭空坐标: {:?}",
            images.placements
        );
        assert_eq!(images.images.len(), 1, "图片元数据仍应下发以便前端预取");
    }
}

#[cfg(test)]
mod key_encoding {
    use super::*;
    use crate::vt::{key_action, mods};

    /// GhosttyKey 常用值（取自原生枚举清单）
    const KEY_A: u32 = 20;
    const KEY_C: u32 = 22;
    const KEY_ENTER: u32 = 58;
    const KEY_ESCAPE: u32 = 120;
    const KEY_ARROW_UP: u32 = 78;
    const KEY_TAB: u32 = 64;
    const KEY_BACKSPACE: u32 = 53;
    const KEY_SPACE: u32 = 63;
    const KEY_F1: u32 = 121;

    fn press(key: u32, mods: u16, utf8: Option<&str>) -> KeyInput {
        KeyInput {
            key,
            mods,
            action: key_action::PRESS,
            utf8: utf8.map(|s| s.to_string()),
            unshifted_codepoint: 0,
            composing: false,
        }
    }

    fn encoded(key: u32, mods: u16, utf8: Option<&str>) -> Vec<u8> {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        t.encode_key(&press(key, mods, utf8)).expect("编码失败")
    }

    #[test]
    fn encodes_plain_characters_verbatim() {
        assert_eq!(encoded(KEY_A, 0, Some("a")), b"a");
    }

    #[test]
    fn encodes_enter_tab_backspace() {
        assert_eq!(encoded(KEY_ENTER, 0, Some("\r")), b"\r");
        assert_eq!(encoded(KEY_TAB, 0, Some("\t")), b"\t");
        assert_eq!(encoded(KEY_BACKSPACE, 0, None), vec![0x7f]);
    }

    #[test]
    fn encodes_ctrl_combinations_as_control_bytes() {
        // Ctrl+C 必须产出 0x03，否则中断信号传不到 shell
        assert_eq!(encoded(KEY_C, mods::CTRL, Some("c")), vec![0x03]);
    }

    #[test]
    fn encodes_escape_alone() {
        assert_eq!(encoded(KEY_ESCAPE, 0, None), vec![0x1b]);
    }

    #[test]
    fn encodes_arrows_in_normal_mode() {
        // 非应用模式：CSI 形式
        assert_eq!(encoded(KEY_ARROW_UP, 0, None), b"\x1b[A");
    }

    #[test]
    fn encodes_f1_function_key() {
        assert_eq!(encoded(KEY_F1, 0, None), b"\x1bOP");
    }

    #[test]
    fn encodes_alt_prefix_for_alt_combinations() {
        // Alt+a 在默认配置下应产出 ESC 前缀
        assert_eq!(encoded(KEY_A, mods::ALT, Some("a")), b"\x1ba");
    }

    #[test]
    fn space_produces_a_space() {
        assert_eq!(encoded(KEY_SPACE, 0, Some(" ")), b" ");
    }

    /// 模式变化必须被感知：应用光标键模式改变方向键的编码形式
    #[test]
    fn arrow_encoding_follows_terminal_mode() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        let normal = t.encode_key(&press(KEY_ARROW_UP, 0, None)).unwrap();
        assert_eq!(normal, b"\x1b[A", "默认应为 CSI 形式");

        // DECCKM（应用光标键模式）→ SS3 形式
        t.write(b"\x1b[?1h");
        let application = t.encode_key(&press(KEY_ARROW_UP, 0, None)).unwrap();
        assert_eq!(
            application, b"\x1bOA",
            "应用模式下应为 SS3 形式——编码器必须与终端状态同步"
        );
    }

    /// key release 只在终端协商了 kitty 事件上报之后才产出字节。
    ///
    /// 这条用例锁住两个事实（实测本引擎行为，前端 keyup 的安全性依赖它）：
    ///   1. 没协商时 RELEASE 一个字节都不产出 —— 普通应用（绝大多数）
    ///      不会因为前端转发抬起事件而收到任何多余输入；
    ///   2. 协商之后（`CSI > 2 u`：report event types），特殊键的抬起
    ///      编成 `CSI 1;1:3 A` 这类序列，应用才能拿到按下/抬起边沿。
    ///      纯文本键（'a'）的抬起仍不产出 —— 引擎按 kitty 规范只对
    ///      会走转义形式的键上报。
    #[test]
    fn release_encodes_nothing_unless_terminal_requests_events() {
        let release = |key: u32, utf8: Option<&str>| KeyInput {
            key,
            mods: 0,
            action: key_action::RELEASE,
            utf8: utf8.map(|s| s.to_string()),
            unshifted_codepoint: 0,
            composing: false,
        };

        let mut plain = NativeTerminal::new(80, 24).unwrap();
        assert!(
            plain
                .encode_key(&release(KEY_A, Some("a")))
                .unwrap()
                .is_empty(),
            "未协商时文本键的 RELEASE 不应产出字节"
        );
        assert!(
            plain
                .encode_key(&release(KEY_ARROW_UP, None))
                .unwrap()
                .is_empty(),
            "未协商时特殊键的 RELEASE 不应产出字节"
        );

        let mut kitty = NativeTerminal::new(80, 24).unwrap();
        kitty.write(b"\x1b[>2u"); // push flags：bit1(值 2) = report event types
        assert_eq!(
            kitty.encode_key(&release(KEY_ARROW_UP, None)).unwrap(),
            b"\x1b[1;1:3A",
            "协商后特殊键的抬起应编成带事件类型的 CSI 序列"
        );
        assert!(
            kitty
                .encode_key(&release(KEY_A, Some("a")))
                .unwrap()
                .is_empty(),
            "文本键的抬起仍不产出（引擎按 kitty 规范只上报转义形式的键）"
        );
    }

    /// 符号键必须原样产出其文本。
    ///
    /// 前端曾把 MINUS..SLASH 五个键值各写偏 1，症状是这些键「无反应」：
    /// 编码器收到的是另一个键且没有文本，于是什么也不输出。
    #[test]
    fn encodes_symbol_keys_with_their_text() {
        // 与前端的 GHOSTTY_KEY 表同源（原生枚举）
        let symbols: &[(u32, &str, &str)] = &[
            (46, "-", "连字符"),
            (47, ".", "句点"),
            (48, "'", "单引号"),
            (49, ";", "分号"),
            (50, "/", "斜杠"),
            (5, ",", "逗号"),
            (6, "0", "数字 0"),
            (19, "=", "等号"),
            (20, "a", "字母 a"),
        ];

        for (key, text, label) in symbols {
            let encoded = encoded(*key, 0, Some(text));
            assert_eq!(
                encoded,
                text.as_bytes(),
                "{label}（key={key}）应原样输出 {text:?}，实际 {:?}",
                String::from_utf8_lossy(&encoded)
            );
        }
    }

    #[test]
    fn rejects_invalid_key_without_panicking() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        // 越界键值应返回错误而非崩溃
        let bad = KeyInput {
            key: 999_999,
            mods: 0,
            action: key_action::PRESS,
            utf8: None,
            unshifted_codepoint: 0,
            composing: false,
        };
        assert!(
            t.encode_key(&bad).is_err(),
            "越界键值必须返回错误：C 侧按枚举索引定长表，越界会读到保护区并偶发 SIGBUS"
        );
    }
}

/// 是否 CJK（含假名/谚文/汉字扩展区）：这些字符在引擎里被当作「一字一词」，
/// 需要走本地的连续区间扫描。
fn is_cjk_codepoint(cp: u32) -> bool {
    matches!(
        cp,
        0x3040..=0x30FF      // 平假名 / 片假名
            | 0x3400..=0x4DBF // 汉字扩展 A
            | 0x4E00..=0x9FFF // 汉字基本区
            | 0xF900..=0xFAFF // 兼容汉字
            | 0xAC00..=0xD7AF // 谚文音节
            | 0xFF66..=0xFF9D // 半角片假名
            | 0x20000..=0x2FA1F // 汉字扩展 B~F
    )
}

/// 词字符判定（CJK 回退扫描用）：非空白、非标点。
/// 覆盖常见 CJK 标点与全角标点 —— 它们应当切开词，而不是并进词里。
fn is_word_char(cp: u32) -> bool {
    if cp == 0 {
        return false;
    }
    // 空白类
    if matches!(cp, 0x20 | 0x09 | 0xA0 | 0x3000) {
        return false;
    }
    // CJK / 全角标点
    if matches!(
        cp,
        0x3001
            | 0x3002
            | 0xFF0C
            | 0xFF01
            | 0xFF1F
            | 0xFF1A
            | 0xFF1B
            | 0xFF08
            | 0xFF09
            | 0x3010
            | 0x3011
            | 0x300A
            | 0x300B
            | 0x201C
            | 0x201D
            | 0x2018
            | 0x2019
            | 0x2026
            | 0x2014
            | 0x00B7
            | 0xFF5B
            | 0xFF5D
            | 0xFF3B
            | 0xFF3D
            | 0xFF0F
            | 0xFF3C
    ) {
        return false;
    }
    // ASCII 标点/符号（英文路径通常已被引擎处理，这里保持一致）
    if cp < 0x80 && !(cp as u8 as char).is_ascii_alphanumeric() && cp != b'_' as u32 {
        return false;
    }
    true
}

/// GhosttyKey 枚举的最大合法值（= GHOSTTY_KEY_PASTE）。
/// 依据 vendor/ghostty-vt/include/ghostty/vt/key/event.h：枚举自 0 起连续编号，
/// 共 176 个成员，末位 GHOSTTY_KEY_PASTE = 175。超出即越界访问 C 侧定长表。
const MAX_GHOSTTY_KEY: u32 = 175;

/// 选区：IDE 的复制能力完全依赖它
#[cfg(test)]
mod selection {
    use super::*;

    fn term_with(text: &str, cols: u16, rows: u16) -> NativeTerminal {
        let mut t = NativeTerminal::new(cols, rows).unwrap();
        t.write(text.as_bytes());
        t
    }

    #[test]
    fn selects_a_single_row_span() {
        let mut t = term_with("hello world", 20, 3);
        t.set_selection(0, 0, 4, 0, false).unwrap();
        assert_eq!(t.selected_text().unwrap(), "hello");
    }

    #[test]
    fn selection_spans_multiple_rows() {
        let mut t = term_with("first line\r\nsecond line\r\nthird", 20, 5);
        t.set_selection(0, 0, 5, 1, false).unwrap();
        let text = t.selected_text().unwrap();
        assert!(text.contains("first line"), "应含首行: {text:?}");
        assert!(text.contains("second"), "应含次行: {text:?}");
    }

    #[test]
    fn trims_trailing_whitespace() {
        let mut t = term_with("abc", 20, 3);
        // 选到行尾，尾部空白应被裁掉
        t.set_selection(0, 0, 19, 0, false).unwrap();
        assert_eq!(t.selected_text().unwrap(), "abc");
    }

    /// 端点是闭区间：(0,0)→(0,0) 选中一格，而非空
    #[test]
    fn degenerate_selection_covers_one_cell() {
        let mut t = term_with("abc", 20, 3);
        t.set_selection(0, 0, 0, 0, false).unwrap();
        assert_eq!(t.selected_text().unwrap(), "a");
    }

    #[test]
    fn select_word_handles_identifier_and_cjk_run() {
        // 英文/代码：沿用引擎规则（点号、连字符、斜杠都在词内）
        let mut t = term_with("foo.bar baz", 60, 3);
        assert_eq!(t.select_word(1, 0).unwrap(), Some((0, 0, 6, 0)));
        assert_eq!(t.selected_text().unwrap(), "foo.bar");

        let mut t = term_with("src/components/App.tsx", 60, 3);
        assert_eq!(t.selected_text().unwrap(), ""); // 先清空状态无关：新实例
        assert_eq!(t.select_word(4, 0).unwrap(), Some((0, 0, 21, 0)));
        assert_eq!(t.selected_text().unwrap(), "src/components/App.tsx");

        // 中文：双击取一段连续的字（词语），而不是单字
        let mut t = term_with("中文测试 end", 60, 3);
        let range = t.select_word(1, 0).unwrap().expect("点中文应有选区");
        assert_eq!(t.selected_text().unwrap(), "中文测试");
        assert_eq!(range.0, 0, "起点应是首字所在列");

        // 中文标点断开词语
        let mut t = term_with("你好，世界", 60, 3);
        t.select_word(0, 0).unwrap();
        assert_eq!(t.selected_text().unwrap(), "你好");

        // 中文与英文连续时一并选中
        let mut t = term_with("中文abc test", 60, 3);
        t.select_word(0, 0).unwrap();
        assert_eq!(t.selected_text().unwrap(), "中文abc");

        // 三击选整行（行首行尾空白被裁掉）
        let mut t = term_with("  indented line  ", 40, 3);
        t.select_line(0).unwrap();
        assert_eq!(t.selected_text().unwrap(), "indented line");
    }

    #[test]
    fn clearing_selection_yields_empty_text() {
        let mut t = term_with("abc", 20, 3);
        t.set_selection(0, 0, 2, 0, false).unwrap();
        assert_eq!(t.selected_text().unwrap(), "abc");

        t.clear_selection().unwrap();
        assert_eq!(t.selected_text().unwrap(), "", "清除后不应再有选区文本");
    }

    #[test]
    fn selection_reflects_subsequent_writes() {
        let mut t = term_with("abc", 20, 3);
        t.set_selection(0, 0, 2, 0, false).unwrap();
        assert_eq!(t.selected_text().unwrap(), "abc");

        // 覆盖同一区域后，选区文本应随之更新
        t.write(b"\x1b[1;1HXYZ");
        assert_eq!(t.selected_text().unwrap(), "XYZ");
    }

    #[test]
    fn rejects_out_of_range_endpoints() {
        let mut t = term_with("abc", 20, 3);
        let err = t.set_selection(0, 0, 99, 99, false);
        assert!(err.is_err(), "越界端点应返回错误而非崩溃");
    }

    #[test]
    fn handles_wide_characters() {
        let mut t = term_with("中文测试", 20, 3);
        t.set_selection(0, 0, 3, 0, false).unwrap();
        let text = t.selected_text().unwrap();
        assert!(text.starts_with("中文"), "宽字符选区应完整: {text:?}");
    }
}

/// 帧元数据：光标与滚动条是渲染器判断如何绘制光标的依据
#[cfg(test)]
mod frame_metadata {
    use super::*;

    /// 回归：`NativeTerminal::new` 之后再 resize 到更大的行数，引擎的视口
    /// 长度（滚动条 len）必须跟着变，不能停在构造时的行数。
    ///
    /// 这条链路是「控制台内容只占上半屏、下半空白」的直接成因：前端按宿主
    /// 高度算出 42 行并下发 resize，若引擎视口仍是启动时的 24 行，
    /// `vt_snapshot_diff` 只会填 24 行，前端 canvas 画 42 行的高度，
    /// 余下 18 行是空的 —— 用户看到上半有内容、下面大片空白。
    #[test]
    fn resize_updates_engine_viewport_rows() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        let initial = t.read_scrollbar();
        assert_eq!(initial.len, 24, "新终端视口应为构造时的 24 行");

        // 前端 applyHostSize 的真实调用形状：宿主变高 → 行数变大 → 同步引擎
        t.resize(98, 42, 8, 18).expect("resize 应成功");

        let grown = t.read_scrollbar();
        assert_eq!(
            grown.len, 42,
            "resize 后引擎视口长度必须跟上，实际 {grown:?}"
        );
        assert_eq!(t.rows(), 42, "rows() 也应返回新值");
        assert_eq!(t.cols(), 98, "cols() 也应返回新值");

        // 缩小同样要跟上：dock 被拖矮时视口必须跟着缩
        t.resize(98, 16, 8, 18).expect("缩小 resize 应成功");
        let shrunk = t.read_scrollbar();
        assert_eq!(shrunk.len, 16, "缩小后视口长度必须跟上，实际 {shrunk:?}");
    }

    #[test]
    fn cursor_starts_visible_at_origin() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        let snap = t.snapshot();
        assert!(snap.cursor_visible, "新终端光标应可见");
        assert_eq!((snap.cursor_x, snap.cursor_y), (0, 0));
    }

    #[test]
    fn cursor_hidden_when_dec_mode_25_disabled() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        t.write(b"\x1b[?25l");
        assert!(!t.snapshot().cursor_visible, "DECTCEM 关闭后光标应隐藏");

        t.write(b"\x1b[?25h");
        assert!(t.snapshot().cursor_visible, "重新开启后应恢复可见");
    }

    #[test]
    fn cursor_style_follows_decscusr() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        // DECSCUSR 1/2=block, 3/4=underline, 5/6=bar
        t.write(b"\x1b[5 q");
        let bar = t.snapshot().cursor_style;
        t.write(b"\x1b[2 q");
        let block = t.snapshot().cursor_style;
        assert_ne!(bar, block, "不同 DECSCUSR 应给出不同视觉形状");
    }

    #[test]
    fn scrollbar_reports_growth_with_scrollback() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        let empty = t.snapshot().scrollbar;
        // total 含可见行，故初始即为屏幕行数
        assert_eq!(empty.total, 3, "初始 total 应为屏幕行数");

        for i in 0..20 {
            t.write(format!("line {i}\r\n").as_bytes());
        }
        let full = t.snapshot().scrollbar;
        assert!(full.total > 0, "滚动历史应增长，实际 {}", full.total);
        assert_eq!(full.len, 3, "视口长度应为屏幕行数");
    }

    /// 长输出必须留得下足够的历史：引擎默认的字节配额（10000）会把 30 万行
    /// 砍到只剩 ~900 行，那正是「TUI 输出一多就翻不回去」的根因。这里同时保护
    /// 两个方向：留太少（字节配额又把人限死）与行上限失效（无界增长）。
    #[test]
    fn scrollback_keeps_long_history() {
        let mut t = NativeTerminal::new(100, 30).unwrap();
        for i in 0..120_000 {
            t.write(format!("line {i:06} some realistic payload text here\r\n").as_bytes());
        }
        let rows = t.scrollback_rows();
        assert!(
            rows > 10_000,
            "历史保留过少（字节配额压死了行上限）: rows={rows}"
        );
        assert!(rows < 30_000, "行上限失效: rows={rows}");
    }

    /// 全局预算依赖「运行时收缩配额能裁剪已有历史」；若引擎只在创建时认
    /// 配额，这个测试会失败，届时退回 spawn-time quota（见 pty.rs 注释）。
    #[test]
    fn scrollback_quota_can_shrink_at_runtime() {
        let mut t = NativeTerminal::new(100, 30).unwrap();
        for i in 0..40_000 {
            t.write(format!("line {i:06} some realistic payload text here\r\n").as_bytes());
        }
        let before = t.scrollback_rows();
        assert!(before > 5_000, "前置条件：应有足够历史，实际 {before}");
        assert!(t.set_scrollback_bytes(1024 * 1024), "运行时设置配额失败");
        // 有的引擎在 set 时立即裁剪，有的在下一帧写入时裁剪；都接受。
        t.write(b"after shrink\r\n");
        let after = t.scrollback_rows();
        assert!(after < before, "运行时收缩未生效：{before} -> {after}");
    }

    /// 滚轮转发：开启鼠标跟踪后必须产出鼠标序列（SGR 格式），
    /// 关闭时必须什么都不产出，交给本地回滚。位置是 1 基的格坐标。
    #[test]
    fn wheel_events_follow_mouse_protocol() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        t.resize(80, 24, 9, 18).unwrap();
        assert!(
            t.encode_wheel(-3, 5, 5, 0).is_empty(),
            "未开跟踪不应产出字节"
        );
        t.write(b"\x1b[?1002h\x1b[?1006h");
        assert!(t.mouse_tracking());
        let up = t.encode_wheel(-3, 5, 5, 0);
        assert_eq!(String::from_utf8_lossy(&up), "\x1b[<64;6;6M");
        let down = t.encode_wheel(3, 0, 0, 0);
        assert_eq!(String::from_utf8_lossy(&down), "\x1b[<65;1;1M");
        // 修饰键：GhosttyMods（ALT = 1<<2）经编码器换算成协议位（SGR 里 alt = 8）
        let with_alt = t.encode_wheel(-3, 1, 2, 4);
        assert_eq!(String::from_utf8_lossy(&with_alt), "\x1b[<72;2;3M");
        // 关闭跟踪后立刻停止产出（TUI 退出时不能再往 shell 灌鼠标序列）
        t.write(b"\x1b[?1002l\x1b[?1006l");
        assert!(!t.mouse_tracking());
        assert!(t.encode_wheel(-3, 5, 5, 0).is_empty());

        // 按下/抬起（SGR）：不再只有滚轮能转发
        t.write(b"\x1b[?1002h\x1b[?1006h");
        let press = t.encode_mouse(MOUSE_ACTION_PRESS, 1, 0, 0, 0);
        assert!(!press.is_empty(), "左键按下应产出鼠标序列");
        let release = t.encode_mouse(1, 1, 0, 0, 0);
        assert!(!release.is_empty(), "左键抬起应产出鼠标序列");
        t.write(b"\x1b[?1002l\x1b[?1006l");
        assert!(t.encode_mouse(MOUSE_ACTION_PRESS, 1, 0, 0, 0).is_empty());
    }

    /// 备用屏 + DEC 1007：滚轮转方向键（alt 屏没有本地 scrollback，只能靠这个）。
    #[test]
    fn alt_screen_wheel_turns_into_arrows() {
        let mut t = NativeTerminal::new(80, 24).unwrap();
        assert!(!t.alt_screen());
        t.write(b"\x1b[?1049h\x1b[?1007h");
        assert!(t.alt_screen());
        assert!(t.mode_enabled(MODE_ALT_SCROLL));
        assert_eq!(t.encode_arrow_scroll(-3), b"\x1b[A");
        assert_eq!(t.encode_arrow_scroll(3), b"\x1b[B");
        assert_eq!(
            t.encode_arrow_scroll(-30),
            b"\x1b[A".repeat(5),
            "一次下发要有封顶"
        );
        // 退出备用屏后模式复位，转发条件不再成立
        t.write(b"\x1b[?1049l");
        assert!(!t.alt_screen());
    }

    #[test]
    fn scrollbar_offset_moves_with_viewport() {
        let mut t = NativeTerminal::new(20, 3).unwrap();
        for i in 0..20 {
            t.write(format!("line {i}\r\n").as_bytes());
        }
        let at_bottom = t.snapshot().scrollbar;
        t.scroll_delta(-5);
        let scrolled = t.snapshot().scrollbar;

        assert_ne!(
            scrolled.offset, at_bottom.offset,
            "滚动后偏移量应变化（{scrolled:?} vs {at_bottom:?}）"
        );
    }
}

/// 搜索：在滚动历史里定位文本
#[cfg(test)]
mod search {
    use super::*;

    fn term_with_lines(n: usize) -> NativeTerminal {
        let mut t = NativeTerminal::new(40, 5).unwrap();
        for i in 0..n {
            let marker = if i % 7 == 3 { " NEEDLE" } else { "" };
            t.write(format!("line {i}{marker}\r\n").as_bytes());
        }
        t
    }

    /// 回归：search 返回的 grid ref 是页内坐标，必须换算成视口坐标；
    /// 否则 scrollback 命中会把高亮画到 rows 之外/无关行。
    #[test]
    fn selection_is_reported_in_viewport_coordinates() {
        let mut t = term_with_lines(70);
        let r = t.search("NEEDLE", Some(SearchSelect::Next)).unwrap();
        let sel = r.selection.expect("scrollback 命中应有选区");
        let rows = t.snapshot().rows;
        assert!(sel.start_y < rows, "start_y 越界: {sel:?}, rows={rows}");
        assert!(sel.end_y < rows, "end_y 越界: {sel:?}, rows={rows}");
        assert!(sel.start_x < 40 && sel.end_x < 40, "x 越界: {sel:?}");
    }

    #[test]
    fn finds_matches_in_scrollback() {
        let mut t = term_with_lines(70);
        // 0..70 中 i % 7 == 3 的有 3,10,17,24,31,38,45,52,59,66 → 10 个
        let r = t.search("NEEDLE", None).unwrap();
        assert_eq!(r.total, 10, "应找到全部匹配");
    }

    #[test]
    fn empty_needle_matches_nothing() {
        let mut t = term_with_lines(20);
        let r = t.search("", None).unwrap();
        assert_eq!(r.total, 0);
        assert_eq!(r.selected, None);
    }

    #[test]
    fn missing_needle_yields_zero() {
        let mut t = term_with_lines(20);
        let r = t.search("NOT-PRESENT-ANYWHERE", None).unwrap();
        assert_eq!(r.total, 0);
    }

    #[test]
    fn selects_first_match_and_advances() {
        let mut t = term_with_lines(70);

        let first = t.search("NEEDLE", Some(SearchSelect::Next)).unwrap();
        assert_eq!(first.total, 10);
        let idx0 = first.selected.expect("应选中一项");

        let second = t.search("NEEDLE", Some(SearchSelect::Next)).unwrap();
        let idx1 = second.selected.expect("应选中一项");
        assert_ne!(idx0, idx1, "再次前进应换到下一个匹配");
    }

    #[test]
    fn backward_selection_wraps_differently_than_forward() {
        let mut t = term_with_lines(70);
        let fwd = t
            .search("NEEDLE", Some(SearchSelect::Next))
            .unwrap()
            .selected;
        let back = t
            .search("NEEDLE", Some(SearchSelect::Prev))
            .unwrap()
            .selected;
        assert!(fwd.is_some() && back.is_some());
        assert_ne!(fwd, back, "前进与后退应落在不同匹配");
    }

    #[test]
    fn repeated_search_is_stable() {
        let mut t = term_with_lines(40);
        let a = t.search("line", None).unwrap();
        let b = t.search("line", None).unwrap();
        assert_eq!(a.total, b.total, "同一搜索词重复执行结果应一致");
        assert!(a.total >= 40, "每行都含 line，实际 {}", a.total);
    }

    #[test]
    fn searches_after_new_output() {
        let mut t = term_with_lines(10);
        let before = t.search("FRESH", None).unwrap().total;
        assert_eq!(before, 0);

        t.write(b"FRESH content here\r\n");
        let after = t.search("FRESH", None).unwrap().total;
        assert_eq!(after, 1, "新输出应可被搜索到");
    }
}

/// 空白格的表示是渲染器依赖的契约，锁进回归测试。
#[cfg(test)]
mod blank_cell_contract {
    use super::*;

    #[test]
    fn blank_cells_are_nul_codepoints_not_spaces() {
        let mut t = NativeTerminal::new(6, 2).unwrap();
        t.write(b"\x1b[31mA");
        let snap = t.snapshot();

        let a = snap.cells[0];
        assert_eq!(a.codepoint, 'A' as u32);
        assert!(a.has_text, "有字形的格应为 true");
        assert_eq!(a.fg, Some(0xcc6666), "红 A 应带前景色");

        for idx in [1, 5, 6] {
            let blank = snap.cells[idx];
            assert_eq!(blank.codepoint, 0, "空白格码点应为 NUL（idx {idx}）");
            assert!(!blank.has_text, "空白格不应标记有字形（idx {idx}）");
            assert!(!blank.is_wide() && blank.wide == 0, "空白格不应是宽字符");
            assert_eq!(blank.fg, None, "空白格不应有前景色（idx {idx}）");
        }
    }
}

//

/// 跨语言往返验证：把真实快照编码成线格式并落盘，
/// 由 TS 侧 `cellFrame.crossLanguage.test.ts` 解码比对。
///
/// 单语言的往返测试无法发现「两边对位域理解不一致」的问题 ——
/// 只有让 Rust 产出字节、TS 独立解码，才能锁死协议。
#[cfg(test)]
mod fixture {
    use super::*;
    use std::fmt::Write as _;

    /// 导出原生枚举值，供前端对齐键盘常量（不靠猜）
    #[test]
    fn dump_native_enums() {
        let ptr = unsafe { ghostty_type_json() };
        let json = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().unwrap();
        let v: serde_json::Value = serde_json::from_str(json).unwrap();

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("src/ghostty-vt/__fixtures__");
        std::fs::create_dir_all(&dir).unwrap();

        println!(
            "顶层键: {:?}",
            v.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        if let Some(abi) = v.get("abi").and_then(|a| a.as_object()) {
            println!("abi 键: {:?}", abi.keys().collect::<Vec<_>>());
        }

        let wanted = [
            "GhosttyMods",
            "GhosttyKeyAction",
            "GhosttyKeyEncoderOption",
            "GhosttyKey",
        ];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("src/ghostty-vt/__fixtures__");
        std::fs::create_dir_all(&dir).unwrap();

        if let Some(all) = v["types"]["GhosttyKey"]["values"].as_object() {
            let mut keys: Vec<_> = all.iter().collect();
            keys.sort_by_key(|(_, val)| val.as_u64().unwrap_or(0));
            let mut lines = String::from("GhosttyKey (按值排序):\n");
            for (name, val) in keys {
                if name == "MAX_VALUE" {
                    continue;
                }
                lines.push_str(&format!("  {} = {}\n", name, val));
            }
            std::fs::write(dir.join("native-keys.txt"), &lines).unwrap();
            println!("键表已导出: {}", all.len());
        }

        // 选区相关结构体的原生布局（与 wasm 不同：内含指针与 GridRef）
        for name in [
            "GhosttySelection",
            "GhosttyTerminalSelectionFormatOptions",
            "GhosttyTerminalScrollbar",
            "GhosttyRenderStateCursorVisualStyle",
        ] {
            if let Some(t) = v["types"].get(name) {
                println!("--- {name}: size={} align={}", t["size"], t["align"]);
                if let Some(fields) = t["fields"].as_object() {
                    for (fname, f) in fields {
                        println!(
                            "      {fname:16} off={:3} size={:2} {}",
                            f["offset"], f["size"], f["type"]
                        );
                    }
                }
            }
        }
        // 修饰键位与按键动作值导出：前端 domKeys.ts 的 MODS / KEY_ACTION 是
        // 硬编码，与 GHOSTTY_KEY 一样需要对账（此前只断言枚举存在，值无人校验）。
        // GhosttyMods 是 C 头文件里的 #define 位域宏（不是 enum），
        // type_json 不导出它，因此直接从 vendor 头文件解析。
        for name in ["GhosttyKeyAction"] {
            if let Some(values) = v["types"][name]["values"].as_object() {
                let mut rows: Vec<_> = values.iter().collect();
                rows.sort_by_key(|(_, val)| val.as_u64().unwrap_or(0));
                let mut lines = format!("{name} (按值排序):\n");
                for (mname, val) in rows {
                    if mname == "MAX_VALUE" {
                        continue;
                    }
                    lines.push_str(&format!("  {} = {}\n", mname, val));
                }
                let out = name.strip_prefix("Ghostty").unwrap().to_lowercase();
                std::fs::write(dir.join(format!("native-{}.txt", out)), &lines).unwrap();
                println!("{} 表已导出: {}", name, values.len());
            }
        }

        // MODS 位域宏：从 event.h 解析 `GHOSTTY_MODS_* (1 << N)`
        let header = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("vendor/ghostty-vt/include/ghostty/vt/key/event.h");
        if let Ok(text) = std::fs::read_to_string(&header) {
            let mut rows: Vec<(String, u32)> = Vec::new();
            for line in text.lines() {
                let t = line.trim();
                let Some(rest) = t.strip_prefix("#define GHOSTTY_MODS_") else {
                    continue;
                };
                // 形如 `#define GHOSTTY_MODS_SHIFT (1 << 0)`
                let open = match rest.find('(') {
                    Some(i) => i,
                    None => continue,
                };
                let name = rest[..open].trim().to_string();
                let inside = rest[open + 1..].trim_end_matches(')');
                if let Some(n) = inside.strip_prefix("1 << ") {
                    if let Ok(bit) = n.trim().parse::<u32>() {
                        rows.push((format!("GHOSTTY_MODS_{}", name), 1u32 << bit));
                    }
                }
            }
            rows.sort_by_key(|(_, v)| *v);
            rows.dedup_by_key(|(_, v)| *v);
            let mut lines = String::from("GhosttyMods (按值排序, 来自 event.h):\n");
            for (name, val) in &rows {
                lines.push_str(&format!("  {} = {}\n", name, val));
            }
            std::fs::write(dir.join("native-mods.txt"), &lines).unwrap();
            println!("GhosttyMods 表已导出: {}", rows.len());
        }

        // 仅断言这些枚举确实存在于清单中；具体取值由各自的测试锁定，
        // 键表则以 native-keys.txt 的形式供前端 verify:wire 比对。
        for name in wanted {
            assert!(v["types"].get(name).is_some(), "原生清单缺少枚举 {name}");
        }
        println!("已确认 {} 个枚举存在", wanted.len());
    }

    #[test]
    fn emit_wire_fixture() {
        // 刻意覆盖：普通文本、索引色、真彩、粗体+斜体、宽字符+spacer、空白格
        let mut t = NativeTerminal::new(12, 2).unwrap();
        t.write(b"AB");
        t.write(b"\x1b[31mCD\x1b[0m");
        t.write(b"\x1b[38;2;10;200;90mEF\x1b[0m");
        t.write(b"\x1b[1;3mGH\x1b[0m");
        t.write("中文".as_bytes());

        let snap = t.snapshot();
        let buffer = encode_snapshot(&snap);

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("src/ghostty-vt/__fixtures__");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("snapshot.bin"), &buffer).unwrap();

        // 期望值：直接取自 Rust 侧解码，作为 TS 的独立比对基准
        let mut json = String::new();
        json.push_str("{\n");
        let _ = writeln!(json, "  \"cols\": {},", snap.cols);
        let _ = writeln!(json, "  \"rows\": {},", snap.rows);
        let _ = writeln!(json, "  \"cursorX\": {},", snap.cursor_x);
        let _ = writeln!(json, "  \"cursorY\": {},", snap.cursor_y);
        let _ = writeln!(json, "  \"byteLength\": {},", buffer.len());
        let _ = writeln!(
            json,
            "  \"defaultFg\": {},",
            snap.default_fg
                .map(|v| v.to_string())
                .unwrap_or("null".into())
        );
        let _ = writeln!(
            json,
            "  \"defaultBg\": {},",
            snap.default_bg
                .map(|v| v.to_string())
                .unwrap_or("null".into())
        );
        let _ = writeln!(
            json,
            "  \"cursorColor\": {},",
            snap.cursor_color
                .map(|v| v.to_string())
                .unwrap_or("null".into())
        );

        json.push_str("  \"cells\": [\n");
        for (i, c) in snap.cells.iter().enumerate() {
            let comma = if i + 1 == snap.cells.len() { "" } else { "," };
            let _ = writeln!(
                json,
                "    {{ \"codepoint\": {}, \"wide\": {}, \"hasText\": {}, \"fg\": {}, \"bg\": {}, \"bold\": {}, \"italic\": {} }}{}",
                c.codepoint,
                c.wide,
                c.has_text,
                c.fg.map(|v| v.to_string()).unwrap_or("null".into()),
                c.bg.map(|v| v.to_string()).unwrap_or("null".into()),
                c.bold,
                c.italic,
                comma
            );
        }
        json.push_str("  ]\n}\n");

        std::fs::write(dir.join("snapshot.expected.json"), json).unwrap();

        println!("线格式缓冲 {} 字节，{} 格", buffer.len(), snap.cells.len());
    }

    /// 增量帧的跨语言 fixture。
    ///
    /// 与 `emit_wire_fixture` 同一用意：让 TS 的 `decodeCellDiff` 与
    /// Rust 的编码器有一份共同的基准。两边对位域、行索引、样式区间
    /// 的理解必须一致，否则前端会把样式贴到错的格子上。
    ///
    /// 顺序要紧：先取首帧的期望真值，再做第二次写入、取差分帧。
    /// 期望值必须与**它校验的那一帧**同状态。
    #[test]
    fn emit_diff_fixture() {
        // 覆盖：彩色文本、粗斜体、宽字符、多行、只改一行后的差分
        let mut t = NativeTerminal::new(12, 3).unwrap();
        t.write(b"AB");
        t.write(b"\x1b[31mCD\x1b[0m");
        t.write(b"\x1b[38;2;10;200;90mEF\x1b[0m");
        t.write(b"\x1b[1;3mGH\x1b[0m");
        t.write("中文".as_bytes());

        // 第 2、3 行各自带**不同**颜色：整屏帧因此覆盖「同一帧多条脏行各自
        // 引入新样式」这条路径（cells 里的 style_idx 是帧级表的全局下标，
        // 前端若不按帧表重映射，第 2 行起会拿到第 1 行的样式 → 丢色/串色）。
        t.write(b"\x1b[2;1H\x1b[32mxy\x1b[0m");
        t.write(b"\x1b[3;1H\x1b[34;44mzw\x1b[0m");

        // 首帧（整屏脏）与它对应时刻的真值
        let full = t.snapshot_wire_diff(true);
        let full_truth = t.snapshot();

        // 只改第 2 行（光标从第 3 行移回第 2 行 + 写入新内容）：脏行 = {1, 2}，
        // 仍小于整屏，保证「增量确实更小」这条断言站得住。
        t.write(b"\x1b[2;1H\x1b[35mPQ\x1b[0m");

        let partial = t.snapshot_wire_diff(false);
        let partial_truth = t.snapshot();

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("src/ghostty-vt/__fixtures__");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("diff-full.bin"), &full).unwrap();
        std::fs::write(dir.join("diff-partial.bin"), &partial).unwrap();

        // 期望值：Rust 侧逐格解出的真值，TS 侧独立解码后逐格比对
        let mut json = String::new();
        json.push_str("{\n");
        let _ = writeln!(json, "  \"fullLength\": {},", full.len());
        let _ = writeln!(json, "  \"partialLength\": {},", partial.len());
        json.push_str("  \"cells\": [\n");
        for (i, c) in full_truth.cells.iter().enumerate() {
            let comma = if i + 1 == full_truth.cells.len() {
                ""
            } else {
                ","
            };
            let _ = writeln!(
                json,
                "    {{ \"codepoint\": {}, \"wide\": {}, \"hasText\": {}, \"fg\": {}, \"bg\": {}, \"bold\": {}, \"italic\": {} }}{}",
                c.codepoint,
                c.wide,
                c.has_text,
                c.fg.map(|v| v.to_string()).unwrap_or("null".into()),
                c.bg.map(|v| v.to_string()).unwrap_or("null".into()),
                c.bold,
                c.italic,
                comma
            );
        }
        json.push_str("  ],\n");
        // 差分帧的真值：TS 只比对差分帧里的行，用「该行在差分时刻的内容」
        json.push_str("  \"partialRowCells\": [\n");
        let partial_cells = partial_truth
            .cells
            .iter()
            .skip(12)
            .take(24)
            .collect::<Vec<_>>();
        for (i, c) in partial_cells.iter().enumerate() {
            let comma = if i + 1 == partial_cells.len() {
                ""
            } else {
                ","
            };
            let _ = writeln!(
                json,
                "    {{ \"codepoint\": {}, \"wide\": {}, \"hasText\": {}, \"fg\": {}, \"bg\": {}, \"bold\": {}, \"italic\": {} }}{}",
                c.codepoint,
                c.wide,
                c.has_text,
                c.fg.map(|v| v.to_string()).unwrap_or("null".into()),
                c.bg.map(|v| v.to_string()).unwrap_or("null".into()),
                c.bold,
                c.italic,
                comma
            );
        }
        json.push_str("  ]\n}\n");
        std::fs::write(dir.join("diff.expected.json"), json).unwrap();

        println!(
            "增量帧 fixture: full={}B partial={}B",
            full.len(),
            partial.len()
        );
    }
}
