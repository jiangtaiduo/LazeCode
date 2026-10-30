//! 终端行为级金样测试。
//!
//! `verifyWire` 锁的是跨语言线格式；这里锁的是“字节进、网格出”的语义：
//! SGR、光标/擦除、宽字符、备用屏、滚动区、模式、回滚、resize。
//! 金样是可见屏幕的规范化文本 + 非默认样式，任何引擎升级导致的语义变化
//! 都会在这里显式暴露，而不是悄悄改变用户看到的内容。
//!
//! 生成/更新金样：
//!   UPDATE_GOLDENS=1 cargo test --test terminal_conformance
//! 更新后必须逐个人工 review，禁止盲签。

use std::hint::black_box;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use lazecode_lib::vt::{CellView, NativeTerminal, Snapshot};

fn new_term(cols: u16, rows: u16) -> NativeTerminal {
    NativeTerminal::new(cols, rows).expect("创建终端失败")
}

fn row_text(snap: &Snapshot, y: u16) -> String {
    let mut out = String::new();
    for x in 0..snap.cols {
        let cell = &snap.cells[y as usize * snap.cols as usize + x as usize];
        if cell.is_spacer() {
            continue;
        }
        if cell.cluster_index > 0 {
            if let Some(cluster) = snap.clusters.get(cell.cluster_index as usize - 1) {
                out.push_str(cluster);
                continue;
            }
        }
        out.push(cell_char(cell));
    }
    out.trim_end().to_string()
}

fn cell_char(cell: &CellView) -> char {
    if !cell.has_text {
        return ' ';
    }
    match char::from_u32(cell.codepoint) {
        Some(ch) if !ch.is_control() => ch,
        Some(_) => '·',
        None => '�',
    }
}

fn style_code(cell: &CellView) -> Option<String> {
    let mut flags = String::new();
    if cell.bold {
        flags.push('B');
    }
    if cell.italic {
        flags.push('I');
    }
    if cell.faint {
        flags.push('F');
    }
    if cell.inverse {
        flags.push('V');
    }
    if cell.invisible {
        flags.push('H');
    }
    if cell.strikethrough {
        flags.push('S');
    }

    let has_color = cell.fg.is_some() || cell.bg.is_some();
    if flags.is_empty() && !has_color {
        return None;
    }

    let mut parts = Vec::new();
    if !flags.is_empty() {
        parts.push(flags);
    }
    if let Some(fg) = cell.fg {
        parts.push(format!("f={fg:06x}"));
    }
    if let Some(bg) = cell.bg {
        parts.push(format!("b={bg:06x}"));
    }
    Some(parts.join(","))
}

fn render(term: &NativeTerminal, snap: &Snapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "size={}x{} cursor={},{} visible={} style={} pending_wrap={} alt={} scrollback={} scrollbar={}/{}/{}\n",
        snap.cols,
        snap.rows,
        snap.cursor_x,
        snap.cursor_y,
        snap.cursor_visible,
        snap.cursor_style,
        snap.cursor_pending_wrap,
        term.alt_screen(),
        term.scrollback_rows(),
        snap.scrollbar.total,
        snap.scrollbar.offset,
        snap.scrollbar.len,
    ));

    for y in 0..snap.rows {
        let mut text = String::new();
        let mut styles = Vec::new();
        for x in 0..snap.cols {
            let cell = &snap.cells[y as usize * snap.cols as usize + x as usize];
            if cell.is_spacer() {
                continue;
            }
            if cell.cluster_index > 0 {
                if let Some(cluster) = snap.clusters.get(cell.cluster_index as usize - 1) {
                    text.push_str(cluster);
                } else {
                    text.push(cell_char(cell));
                }
            } else {
                text.push(cell_char(cell));
            }
            if let Some(code) = style_code(cell) {
                styles.push(format!("{x}:{code}"));
            }
        }
        out.push_str(&format!(
            "row={y:02}|{}|{}\n",
            text.trim_end(),
            styles.join(" ")
        ));
    }
    out
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/terminal")
}

fn assert_golden(name: &str, term: &NativeTerminal, snap: &Snapshot) {
    let actual = render(term, snap);
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).expect("创建金样目录失败");
    let path = dir.join(format!("{name}.golden"));

    if std::env::var("UPDATE_GOLDENS").as_deref() == Ok("1") {
        std::fs::write(&path, &actual).expect("写入金样失败");
        eprintln!("已更新金样：{}", path.display());
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "缺少金样 {}；运行 UPDATE_GOLDENS=1 cargo test --test terminal_conformance 生成后人工 review",
            path.display()
        )
    });

    if expected == actual {
        return;
    }

    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    for index in 0..expected_lines.len().max(actual_lines.len()) {
        let expected_line = expected_lines.get(index).copied().unwrap_or("<缺失>");
        let actual_line = actual_lines.get(index).copied().unwrap_or("<缺失>");
        if expected_line != actual_line {
            panic!(
                "金样不匹配：{}\n第 {} 行\n期望: {}\n实际: {}\n\n完整实际输出：\n{}",
                path.display(),
                index + 1,
                expected_line,
                actual_line,
                actual
            );
        }
    }
    panic!("金样不匹配：{}（仅行尾空行差异）", path.display());
}

#[test]
fn sgr_styles_and_truecolor() {
    let mut term = new_term(60, 2);
    term.write(
        b"plain \x1b[1mbold\x1b[0m \x1b[3mital\x1b[0m \x1b[7minv\x1b[0m \
          \x1b[9mstrike\x1b[0m \x1b[38;2;255;0;0mred\x1b[0m \
          \x1b[48;5;236mbg\x1b[0m",
    );
    let snap = term.snapshot();

    assert_eq!(row_text(&snap, 0), "plain bold ital inv strike red bg");

    let find = |needle: &str| -> (usize, usize) {
        let text = row_text(&snap, 0);
        let start = text
            .find(needle)
            .unwrap_or_else(|| panic!("找不到 {needle}"));
        let x = text[..start].chars().count();
        (x, 0)
    };

    let (bold_x, _) = find("bold");
    assert!(snap.cells[bold_x].bold, "bold 位未置位");

    let (italic_x, _) = find("ital");
    assert!(snap.cells[italic_x].italic, "italic 位未置位");

    let (inverse_x, _) = find("inv");
    assert!(snap.cells[inverse_x].inverse, "inverse 位未置位");

    let (strike_x, _) = find("strike");
    assert!(snap.cells[strike_x].strikethrough, "strikethrough 位未置位");

    let (red_x, _) = find("red");
    assert_eq!(snap.cells[red_x].fg, Some(0xff0000), "真彩前景色错误");

    let (bg_x, _) = find("bg");
    assert!(snap.cells[bg_x].bg.is_some(), "256 色背景未解析");

    assert_golden("sgr", &term, &snap);
}

#[test]
fn cursor_and_erase() {
    let mut term = new_term(10, 4);
    term.write(b"abcdef\r\nghijkl\r\nmnopqr");
    term.write(b"\x1b[2;3H\x1b[K");
    term.write(b"\x1b[3;3H\x1b[1K");
    let snap = term.snapshot();

    assert_eq!(row_text(&snap, 0), "abcdef");
    assert_eq!(row_text(&snap, 1), "gh");
    assert_eq!(row_text(&snap, 2), "   pqr");
    assert_eq!(row_text(&snap, 3), "");
    assert_golden("erase", &term, &snap);

    let mut cleared = new_term(10, 4);
    cleared.write(b"content\x1b[2J");
    let cleared_snap = cleared.snapshot();
    for y in 0..cleared_snap.rows {
        assert_eq!(row_text(&cleared_snap, y), "", "ED 2 未清空第 {y} 行");
    }
}

#[test]
fn wide_chars_and_emoji() {
    let mut term = new_term(20, 3);
    // 2027 = grapheme clustering；不开时引擎会把 ZWJ 序列拆成多个格子（这是
    // Ghostty 的模式语义，不是前端缺陷）。测试显式开启，锁定“整簇穿线格式”。
    term.write("中A🙂\r\n\x1b[?2027h\x1b[2;1He\u{301}👨‍👩‍👧‍👦B".as_bytes());
    let snap = term.snapshot();

    assert!(snap.cells[0].is_wide(), "CJK 首格未标记为宽字符");
    assert!(snap.cells[1].is_spacer(), "CJK 尾格未标记为 spacer");
    assert!(snap.cells[3].is_wide(), "emoji 首格未标记为宽字符");
    assert!(snap.cells[4].is_spacer(), "emoji 尾格未标记为 spacer");
    assert!(row_text(&snap, 0).starts_with("中A🙂"));

    // 2027 模式下，组合附加符与 ZWJ 家族必须作为单个簇穿过线格式。
    assert_eq!(row_text(&snap, 1), "e\u{301}👨‍👩‍👧‍👦B");
    assert!(
        snap.cells[20].cluster_index > 0 || snap.cells.iter().any(|cell| cell.cluster_index > 0),
        "行内应至少有一个字素簇索引"
    );

    assert_golden("wide", &term, &snap);
}

#[test]
fn alt_screen_preserves_main_buffer() {
    let mut term = new_term(20, 3);
    term.write(b"main screen");
    assert!(!term.alt_screen(), "初始不应处于备用屏");

    term.write(b"\x1b[?1049h");
    assert!(term.alt_screen(), "1049h 未进入备用屏");
    // 1049 会保留光标位置；显式归位，测试只关心备用屏与主屏的内容隔离。
    term.write(b"\x1b[Halt screen");
    let alt_snap = term.snapshot();
    assert_eq!(row_text(&alt_snap, 0), "alt screen");
    assert_golden("alt", &term, &alt_snap);

    term.write(b"\x1b[?1049l");
    assert!(!term.alt_screen(), "1049l 未退出备用屏");
    let restored = term.snapshot();
    assert_eq!(row_text(&restored, 0), "main screen");
    assert_golden("alt-restored", &term, &restored);
}

#[test]
fn scroll_region_limits_scrolling() {
    let mut term = new_term(10, 5);
    term.write(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    term.write(b"\x1b[2;4r");
    term.write(b"\x1b[4;1H\n");
    let snap = term.snapshot();

    assert_eq!(row_text(&snap, 0), "one", "滚动区外的首行不应移动");
    assert_eq!(row_text(&snap, 1), "three");
    assert_eq!(row_text(&snap, 2), "four");
    assert_eq!(row_text(&snap, 3), "");
    assert_eq!(row_text(&snap, 4), "five", "滚动区外的末行不应移动");

    assert_golden("scroll-region", &term, &snap);
}

#[test]
fn modes_hyperlinks_and_osc52_are_side_effect_free() {
    let mut term = new_term(30, 2);
    term.write(b"\x1b[?2004h\x1b[?1000h\x1b[?1006h");
    assert!(term.mode_enabled(2004), "bracketed paste 未开启");
    assert!(term.mouse_tracking(), "鼠标跟踪未开启");

    // OSC 52 允许程序写系统剪贴板。本项目没有向 libghostty-vt 注册
    // 剪贴板回调，因此该序列必须被安全忽略：不能改屏幕、不能崩溃。
    term.write(b"\x1b]52;c;SGVsbG8=\x07");
    // OSC 8 超链接与 OSC 0 标题：文本仍要正常显示，序列本身不能泄漏到网格。
    term.write(b"\x1b]8;;https://example.com\x07link\x1b]8;;\x07");
    term.write(b"\x1b]0;window title\x07");

    let snap = term.snapshot();
    let text = row_text(&snap, 0);
    assert!(text.contains("link"), "OSC 8 之间的文本丢失：{text:?}");
    assert!(!text.contains("SGVsbG8"), "OSC 52 载荷泄漏到了屏幕");
    assert!(!text.contains("window title"), "OSC 0 标题泄漏到了屏幕");

    assert_golden("modes", &term, &snap);
}

#[test]
fn scrollback_preserves_history() {
    let mut term = new_term(10, 3);
    for index in 0..20 {
        term.write(format!("L{index:02}\r\n").as_bytes());
    }

    assert!(term.scrollback_rows() > 0, "历史输出没有进入 scrollback");
    assert!(term.viewport_is_at_bottom(), "初始视口应在底部");

    let bottom = row_text(&term.snapshot(), 1);
    term.scroll_delta(-5);
    let scrolled_snap = term.snapshot();
    assert!(!term.viewport_is_at_bottom(), "向上滚动后不应仍在底部");
    assert_ne!(row_text(&scrolled_snap, 1), bottom, "视口没有滚动");

    assert_golden("scrollback", &term, &scrolled_snap);

    term.scroll_to_bottom();
    assert!(
        term.viewport_is_at_bottom(),
        "scroll_to_bottom 未回到活动区"
    );
}

#[test]
fn resize_keeps_grid_consistent() {
    let mut term = new_term(20, 5);
    term.write(b"0123456789");
    term.resize(10, 3, 8, 16).expect("resize 失败");

    let snap = term.snapshot();
    assert_eq!((snap.cols, snap.rows), (10, 3));
    assert_eq!(snap.cells.len(), 30);
    assert!(snap.cursor_x < 10 && snap.cursor_y < 3);
    assert_golden("resize", &term, &snap);
}

#[cfg(unix)]
#[test]
fn pty_output_reaches_engine() {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty 失败");

    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", "printf '\\033[32mPTY_OK\\033[0m\\n'"]);
    let mut child = pair.slave.spawn_command(command).expect("spawn 失败");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader 失败");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => all.extend_from_slice(&buf[..n]),
            }
        }
        let _ = sender.send(all);
    });

    let bytes = match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = child.kill();
            panic!("PTY 读取超时：{error}");
        }
    };
    let _ = child.wait();
    assert!(!bytes.is_empty(), "PTY 没有输出");

    let mut term = new_term(80, 24);
    term.write(&bytes);
    let snap = term.snapshot();
    assert!(
        row_text(&snap, 0).starts_with("PTY_OK"),
        "PTY 输出未正确进入网格：{:?}",
        row_text(&snap, 0)
    );
    assert!(snap.cells[0].fg.is_some(), "PTY 输出的 SGR 颜色丢失");
}

#[test]
fn random_escape_corpus_keeps_grid_invariants() {
    // 固定种子：失败可复现。语料覆盖 CSI/OSC/模式切换/UTF-8/非法字节，
    // 每次随机切块，模拟 PTY 在任意位置截断转义序列。
    let corpus: &[&[u8]] = &[
        b"hello",
        b"\x1b[31m",
        b"\x1b[0m",
        b"\x1b[38;2;1;2;3m",
        b"\x1b[48;5;200m",
        b"\x1b[2J",
        b"\x1b[K",
        b"\x1b[1;1H",
        b"\x1b[5;5H",
        b"\x1b[3;7r",
        b"\x1b[r",
        b"\x1b[?1049h",
        b"\x1b[?1049l",
        b"\x1b[?2004h",
        b"\x1b[?1000h",
        b"\x1b[?1006h",
        b"\x1b]52;c;QQ==\x07",
        b"\x1b]0;title\x07",
        b"\x1b]8;;https://example.com\x07",
        b"\x1b]8;;\x07",
        b"\x1b[1;31;44mX\x1b[0m",
        b"\x1b[999;999H",
        b"\x1b[",
        b"\x1b]",
        b"\x07",
        "中文".as_bytes(),
        "🙂".as_bytes(),
        "e\u{301}".as_bytes(),
    ];

    let mut state: u32 = 0x5eed_1234;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };

    let mut term = new_term(40, 12);
    for _ in 0..1500 {
        let mut payload = Vec::new();
        let pieces = (next() % 8 + 1) as usize;
        for _ in 0..pieces {
            payload.extend_from_slice(corpus[next() as usize % corpus.len()]);
        }
        // 混入少量原始字节，覆盖非法 UTF-8 / 未完成序列。
        for _ in 0..(next() % 4) {
            payload.push(next() as u8);
        }

        let mut offset = 0;
        while offset < payload.len() {
            let take = (next() as usize % 17 + 1).min(payload.len() - offset);
            term.write(&payload[offset..offset + take]);
            offset += take;
        }

        if next() % 7 == 0 {
            black_box(term.snapshot_wire_diff(false));
        }
        let _ = term.take_responses();

        let snap = term.snapshot();
        assert_eq!(snap.cells.len(), 40 * 12, "网格长度被破坏");
        assert!(snap.cursor_x < 40, "光标 x 越界：{}", snap.cursor_x);
        assert!(snap.cursor_y < 12, "光标 y 越界：{}", snap.cursor_y);
    }
}
