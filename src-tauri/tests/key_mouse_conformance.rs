//! 键鼠编码一致性测试。
//!
//! 锁的是「一次按键/鼠标事件 → 写入 PTY 的确切字节」。这些字节由原生
//! Ghostty 编码器产生，前端 `domKeys.ts` 只负责填 (key, mods, action, utf8)
//! 四元组，因此这里直接调用 `NativeTerminal::encode_key` / `encode_mouse`，
//! 与真实输入路径同源。
//!
//! 期望值尽量硬编码为协议标准（xterm / Kitty / SGR-1006 / X10），而不是
//! 用引擎自己的输出反向生成 —— 否则引擎升级改坏序列时，测试会跟着一起错。

use lazecode_lib::vt::{key_action, mods, KeyInput, NativeTerminal};

/// `GhosttyKey` 常用值（取自 vendored key/event.h 的 GhosttyKey 枚举）。
const KEY_A: u32 = 20;
const KEY_C: u32 = 22;
const KEY_BACKSPACE: u32 = 53;
const KEY_ENTER: u32 = 58;
const KEY_TAB: u32 = 64;
const KEY_ARROW_DOWN: u32 = 75;
const KEY_ARROW_LEFT: u32 = 76;
const KEY_ARROW_RIGHT: u32 = 77;
const KEY_ARROW_UP: u32 = 78;
const KEY_ESCAPE: u32 = 120;
const KEY_F1: u32 = 121;

/// `GhosttyMouseAction` / `GhosttyMouseButton`（取自 mouse/event.h）。
const MOUSE_PRESS: u32 = 0;
const MOUSE_RELEASE: u32 = 1;
const MOUSE_LEFT: u32 = 1;
const MOUSE_RIGHT: u32 = 2;
const MOUSE_MIDDLE: u32 = 3;

fn press(key: u32, mods: u16, utf8: Option<&str>) -> KeyInput {
    press_with_codepoint(key, mods, utf8, 0)
}

/// `unshifted_codepoint` 是 Kitty 协议定位「未受修饰字符」的依据；
/// 非单字符键（Enter/方向键等）由引擎按逻辑键处理，填 0 即可。
fn press_with_codepoint(
    key: u32,
    mods: u16,
    utf8: Option<&str>,
    unshifted_codepoint: u32,
) -> KeyInput {
    KeyInput {
        key,
        mods,
        action: key_action::PRESS,
        utf8: utf8.map(|s| s.to_string()),
        unshifted_codepoint,
        composing: false,
    }
}

fn encode(term: &mut NativeTerminal, input: &KeyInput) -> Vec<u8> {
    term.encode_key(input).expect("按键编码失败")
}

/// 解析 `CSI <codepoint>;<mods> u`，同时验证完整形状（无多余参数）。
fn csi_u_params(bytes: &[u8]) -> (u32, u32) {
    let text = std::str::from_utf8(bytes).expect("CSI u 序列必须为 ASCII");
    let body = text
        .strip_prefix("\x1b[")
        .and_then(|s| s.strip_suffix('u'))
        .expect("不是 CSI ... u 序列");
    let mut parts = body.split(';');
    let codepoint = parts
        .next()
        .expect("缺少码点")
        .parse()
        .expect("码点不是数字");
    let modifier = parts
        .next()
        .expect("缺少修饰符参数")
        .parse()
        .expect("修饰符不是数字");
    assert!(parts.next().is_none(), "CSI u 序列参数过多: {body}");
    (codepoint, modifier)
}

// ---------------------------------------------------------------------------
// 键盘
// ---------------------------------------------------------------------------

/// 可打印字符与控制键：与前端 `domKeyToInput` 的填充方式保持一致
/// （控制键不带 utf8，可打印字符带 utf8）。
#[test]
fn printable_and_control_keys_use_standard_bytes() {
    let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");

    assert_eq!(encode(&mut term, &press(KEY_A, 0, Some("a"))), b"a");
    assert_eq!(encode(&mut term, &press(KEY_ENTER, 0, None)), b"\r");
    assert_eq!(encode(&mut term, &press(KEY_TAB, 0, None)), b"\t");
    assert_eq!(
        encode(&mut term, &press(KEY_BACKSPACE, 0, None)),
        vec![0x7f],
        "Backspace 必须是 DEL(0x7f)，不能是 BS(0x08)"
    );
    assert_eq!(encode(&mut term, &press(KEY_ESCAPE, 0, None)), vec![0x1b]);
}

/// 方向键在普通模式用 CSI，DECCKM（应用光标键模式）用 SS3。
#[test]
fn arrows_follow_cursor_key_mode() {
    let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");

    assert_eq!(encode(&mut term, &press(KEY_ARROW_UP, 0, None)), b"\x1b[A");
    assert_eq!(
        encode(&mut term, &press(KEY_ARROW_DOWN, 0, None)),
        b"\x1b[B"
    );
    assert_eq!(
        encode(&mut term, &press(KEY_ARROW_RIGHT, 0, None)),
        b"\x1b[C"
    );
    assert_eq!(
        encode(&mut term, &press(KEY_ARROW_LEFT, 0, None)),
        b"\x1b[D"
    );

    // DECCKM：应用模式必须能被编码器实时感知（不能缓存旧模式）。
    term.write(b"\x1b[?1h");
    assert_eq!(encode(&mut term, &press(KEY_ARROW_UP, 0, None)), b"\x1bOA");
    assert_eq!(
        encode(&mut term, &press(KEY_ARROW_DOWN, 0, None)),
        b"\x1bOB"
    );
    assert_eq!(
        encode(&mut term, &press(KEY_ARROW_RIGHT, 0, None)),
        b"\x1bOC"
    );
    assert_eq!(
        encode(&mut term, &press(KEY_ARROW_LEFT, 0, None)),
        b"\x1bOD"
    );
}

/// Ctrl 组合编码成 C0 控制字节；Alt 组合在 option-as-alt 配置下加 ESC 前缀。
#[test]
fn ctrl_and_alt_combinations() {
    let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");

    // Ctrl+C → ETX。前端对 Ctrl 组合不传 utf8（见 domKeyToInput）。
    assert_eq!(
        encode(&mut term, &press(KEY_C, mods::CTRL, None)),
        vec![0x03]
    );
    // Alt+a → ESC + "a"（编码器已配置 OPTION_AS_ALT）。
    assert_eq!(
        encode(&mut term, &press(KEY_A, mods::ALT, Some("a"))),
        b"\x1ba"
    );
}

/// F1 使用 xterm 的 SS3 序列。
///
/// xterm terminfo 中 `kf1=\EOP`（VT220 功能键 SS3 P），这是稳定标准，
/// 并非本引擎私有形式。F1 不受 DECCKM 影响。
#[test]
fn f1_matches_xterm_ss3_sequence() {
    let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");

    assert_eq!(
        encode(&mut term, &press(KEY_F1, 0, None)),
        b"\x1bOP",
        "F1 应为 xterm/VT220 的 SS3 P（kf1=\\EOP）"
    );

    term.write(b"\x1b[?1h");
    assert_eq!(
        encode(&mut term, &press(KEY_F1, 0, None)),
        b"\x1bOP",
        "DECCKM 只影响方向键，F1 仍为 SS3 P"
    );
}

/// Kitty 键盘协议：`CSI > 1 u`（disambiguate）之后，带修饰的可打印键
/// 改成 `CSI codepoint;mods u`，不再走传统控制字节。
///
/// 协议对照（kitty keyboard-protocol）：
///   - cis 参数 `mods = 1 + 位掩码`，shift=1、alt=2、ctrl=4；
///   - 码点使用「未受修饰」的小写字符（Ctrl+C → 99，不是 67）；
///   - 开启该标志后 Ctrl+C 不再产生 SIGINT/0x03。
///
/// 引擎细节：文本键的 CSI u 编码需要 `unshiftedCodepoint`；前端
/// `domKeyToInput` 对单字符键会填该字段。若为 0，编码器无法确定码点，
/// Ctrl+C 会输出空。Enter/Tab/Backspace 是协议保留的 legacy 例外，但带
/// 修饰时（如 Shift+Enter）仍编成 CSI u。
#[test]
fn kitty_protocol_reports_modified_keys_as_csi_u() {
    let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");
    term.write(b"\x1b[>1u"); // 0b1: disambiguate escape codes

    let ctrl_c = encode(
        &mut term,
        &press_with_codepoint(KEY_C, mods::CTRL, None, 'c' as u32),
    );
    assert_eq!(ctrl_c, b"\x1b[99;5u", "99 = 'c'，5 = 1 + ctrl(4)");
    assert_eq!(csi_u_params(&ctrl_c), (99, 5));

    // Enter 没有单字符 unshifted codepoint，走逻辑键分支；修饰后仍是 CSI u：
    // 13 = CR，2 = 1 + shift(1)。
    let shift_enter = encode(&mut term, &press(KEY_ENTER, mods::SHIFT, None));
    assert_eq!(shift_enter, b"\x1b[13;2u");
    assert_eq!(csi_u_params(&shift_enter), (13, 2));

    // 对照：未开启 Kitty 时同一按键仍是传统 C0 字节，确认上面的差异确实
    // 来自协议协商而不是编码器常驻行为。
    let mut legacy = NativeTerminal::new(80, 24).expect("创建终端失败");
    assert_eq!(
        encode(
            &mut legacy,
            &press_with_codepoint(KEY_C, mods::CTRL, None, 'c' as u32)
        ),
        vec![0x03]
    );
}

// ---------------------------------------------------------------------------
// 鼠标
// ---------------------------------------------------------------------------

/// 固定格子尺寸（10x20 像素/格），让「0 基格坐标 → 协议 1 基坐标」可预期。
fn mouse_term() -> NativeTerminal {
    let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");
    term.resize(80, 24, 10, 20)
        .expect("设置终端尺寸与格子大小失败");
    term
}

/// SGR 1006：`CSI < Cb ; Cx ; Cy M/m`，坐标 1 基，按下/移动用 `M`，
/// 抬起用 `m`。Cb：左键 0、中键 1、右键 2；滚轮上 64、下 65；
/// 修饰位 shift=4、alt=8、ctrl=16。
#[test]
fn sgr_mouse_reports_press_release_and_wheel() {
    let mut term = mouse_term();

    assert!(
        term.encode_mouse(MOUSE_PRESS, MOUSE_LEFT, 2, 3, 0)
            .is_empty(),
        "未开启跟踪时不应产出任何字节（应回退到本地选区）"
    );

    term.write(b"\x1b[?1000h\x1b[?1006h");

    // 0 基 (2,3) → 协议 1 基 (3,4)。
    assert_eq!(
        term.encode_mouse(MOUSE_PRESS, MOUSE_LEFT, 2, 3, 0),
        b"\x1b[<0;3;4M"
    );
    assert_eq!(
        term.encode_mouse(MOUSE_RELEASE, MOUSE_LEFT, 2, 3, 0),
        b"\x1b[<0;3;4m",
        "SGR 抬起用固定按钮号 + 小写 m"
    );

    assert_eq!(
        term.encode_mouse(MOUSE_PRESS, MOUSE_RIGHT, 4, 5, 0),
        b"\x1b[<2;5;6M"
    );
    assert_eq!(
        term.encode_mouse(MOUSE_PRESS, MOUSE_MIDDLE, 4, 5, 0),
        b"\x1b[<1;5;6M"
    );
    // GhosttyMods ALT(1<<2) → SGR 修饰位 8。
    assert_eq!(
        term.encode_mouse(MOUSE_PRESS, MOUSE_LEFT, 2, 3, mods::ALT),
        b"\x1b[<8;3;4M"
    );

    // 滚轮以「按下」事件上报：上 64、下 65。
    assert_eq!(term.encode_wheel(-3, 2, 3, 0), b"\x1b[<64;3;4M");
    assert_eq!(term.encode_wheel(3, 2, 3, 0), b"\x1b[<65;3;4M");

    term.write(b"\x1b[?1000l\x1b[?1006l");
    assert!(
        term.encode_mouse(MOUSE_PRESS, MOUSE_LEFT, 2, 3, 0)
            .is_empty(),
        "关闭跟踪后必须立即停止产出"
    );
}

/// 关闭 SGR（1006）后回退到 X10：`CSI M Cb Cx Cy`，每个字节 +32。
/// Cb 释放固定为 3（按钮无法从编码中区分），滚轮上/下为 64/65。
#[test]
fn x10_mouse_encoding_when_sgr_is_off() {
    let mut term = mouse_term();
    term.write(b"\x1b[?1000h");

    // 0 基 (2,3) → 1 基 (3,4)；Cb=0+32、Cx=3+32、Cy=4+32。
    assert_eq!(
        term.encode_mouse(MOUSE_PRESS, MOUSE_LEFT, 2, 3, 0),
        vec![0x1b, b'[', b'M', 32, 35, 36]
    );
    assert_eq!(
        term.encode_mouse(MOUSE_RELEASE, MOUSE_LEFT, 2, 3, 0),
        vec![0x1b, b'[', b'M', 35, 35, 36],
        "X10 释放固定上报按钮 3（3+32=35）"
    );

    assert_eq!(
        term.encode_wheel(-3, 2, 3, 0),
        vec![0x1b, b'[', b'M', 96, 35, 36],
        "滚轮上：64+32=96"
    );
    assert_eq!(
        term.encode_wheel(3, 2, 3, 0),
        vec![0x1b, b'[', b'M', 97, 35, 36],
        "滚轮下：65+32=97"
    );
}
