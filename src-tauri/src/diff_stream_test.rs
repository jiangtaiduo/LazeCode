// 实机链路校验：真实 PTY 流 → 原生解析 → 差分帧 → 合并，与全量快照逐格比对。
//
// 为什么单独一个文件：这条链路跨三个模块（pty / vt / 前端合并算法的镜像），
// 且驱动的是**真实 portable-pty + 真实 shell**，不是人造字节。放这里是为了
// 在没有显示器（锁屏、CI）时也能验证「实机运行」这一环 ——
// `pty::tests` 里已有等价物，但那覆盖不到差分路径。
//
// 合并算法（`Screen`）必须与前端 `src/ghostty-vt/cellFrame.ts::CellDiffCache`
// 同构：这里是 Rust 侧的独立实现，两边各自实现才能互相校验。
use crate::vt::{CellView, NativeTerminal};

#[test]
fn live_pty_stream_matches_full_snapshot() {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(portable_pty::PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty 失败");

    let mut cmd = portable_pty::cmdbuilder::CommandBuilder::new("/bin/zsh");
    cmd.arg("-c");
    cmd.arg(
        "printf '\\033[1;34m$\\033[0m ls -la\\r\\n'; \
         printf 'drwxr-xr-x  14 user   staff   448 Sep 28 13:55 .\\r\\n'; \
         printf '\\033[1;36mdrwxr-xr-x\\033[0m   9 user   staff   288 .audit\\r\\n'; \
         printf '\\033[38;2;10;200;90m-rw-r--r--\\033[0m   1 user   staff  1491 package.json\\r\\n'; \
         printf '中文测试宽字符\\r\\n'; \
         for i in $(seq 1 50); do echo \"line $i\"; done; \
         printf '\\033[2J\\033[Hredrawn\\r\\n'",
    );
    let mut child = pair.slave.spawn_command(cmd).expect("启动 shell 失败");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("获取 reader 失败");
    let mut term = NativeTerminal::new(120, 40).expect("创建终端失败");
    let mut screen = Screen::new(&mut term);

    let mut buf = [0u8; 8192];
    let mut diff_bytes = 0usize;
    let mut full_bytes = 0usize;
    let mut frames = 0usize;
    let mut checks = 0usize;

    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        // 按 256B 切片喂入并各拉一帧：模拟真实终端的逐块到达。
        // 一个 read 常吞下整条命令的输出，那是批量而非流式，测不出增量收益。
        let mut i = 0;
        while i < n {
            let end = (i + 256).min(n);
            term.write(&buf[i..end]);
            i = end;

            // 与前端 rAF 循环同构：脏了就拉一帧差分并合并
            let d = term.snapshot_wire_diff(false);
            diff_bytes += d.len();
            full_bytes += term.snapshot_wire().len();
            frames += 1;
            screen.merge(&d);

            if frames.is_multiple_of(40) {
                assert_screen_matches(&mut term, &screen);
                checks += 1;
            }
        }
    }
    let _ = child.wait();

    assert_screen_matches(&mut term, &screen);
    println!("实机校验通过：{frames} 帧，抽验 {checks}+1 次全部逐格一致");

    // 收益取决于「帧率 vs 输出速率」：批量冲刷时两者相当，
    // 交互态（闪烁/逐字回显/空闲）差分才大幅胜出。两种都测。
    let bulk = 100.0 * (1.0 - diff_bytes as f64 / full_bytes as f64);
    println!("批量输出态：差分 {diff_bytes}B vs 全量 {full_bytes}B -> 省 {bulk:.1}%");

    // 交互态：命令结束后的静止画面，只有光标在闪
    let mut idle_diff = 0usize;
    let mut idle_full = 0usize;
    for _ in 0..60 {
        let d = term.snapshot_wire_diff(false);
        idle_diff += d.len();
        idle_full += term.snapshot_wire().len();
        screen.merge(&d);
        assert_screen_matches(&mut term, &screen);
    }
    let idle = 100.0 * (1.0 - idle_diff as f64 / idle_full as f64);
    println!("交互静止态：差分 {idle_diff}B vs 全量 {idle_full}B -> 省 {idle:.1}%");

    assert!(bulk > -50.0, "批量态差分不应显著劣于全量：{bulk:.1}%");
    assert!(
        idle > 90.0,
        "静止态差分必须大幅胜出（每帧只闪光标）：{idle:.1}%"
    );
}

/// 与前端 `CellDiffCache` 同构的屏幕镜像。
struct Screen {
    cols: usize,
    cells: Vec<CellView>,
}

impl Screen {
    fn new(t: &mut NativeTerminal) -> Self {
        let s = t.snapshot();
        Self {
            cols: s.cols as usize,
            cells: s.cells,
        }
    }

    fn merge(&mut self, frame: &[u8]) {
        let rd_u16 = |o: usize| u16::from_le_bytes([frame[o], frame[o + 1]]) as usize;
        let rd_i32 =
            |o: usize| i32::from_le_bytes([frame[o], frame[o + 1], frame[o + 2], frame[o + 3]]);
        let rd_u32 =
            |o: usize| u32::from_le_bytes([frame[o], frame[o + 1], frame[o + 2], frame[o + 3]]);
        assert_eq!(rd_u32(0), 0x5456_4e33, "增量帧 magic");

        let cols = rd_u16(4);
        let style_count = rd_u16(30);
        let row_count = rd_u16(32);

        let mut off = 60;
        let mut styles: Vec<CellView> = Vec::with_capacity(style_count);
        for _ in 0..style_count {
            let fg = rd_i32(off);
            let bg = rd_i32(off + 4);
            let fl = rd_u32(off + 8);
            styles.push(CellView {
                codepoint: 0,
                wide: 0,
                has_text: false,
                fg: (fg >= 0).then_some(fg as u32),
                bg: (bg >= 0).then_some(bg as u32),
                bold: fl & 1 != 0,
                italic: fl & 2 != 0,
                faint: fl & 4 != 0,
                inverse: fl & 8 != 0,
                invisible: fl & 16 != 0,
                strikethrough: fl & 32 != 0,
                cluster_index: 0,
            });
            off += 12;
        }

        for _ in 0..row_count {
            let y = rd_u16(off);
            // 行头 8B（row + reserved + row_flags + pad）：cells 4 字节对齐
            off += 8;
            for x in 0..cols {
                let mut b = [0u8; 8];
                b.copy_from_slice(&frame[off..off + 8]);
                let packed = u64::from_le_bytes(b);
                off += 8;
                let si = ((packed >> 23) & 0xffff) as usize;
                let mut c = styles[si.min(styles.len().saturating_sub(1))];
                c.codepoint = (packed & 0x1f_ffff) as u32;
                c.wide = ((packed >> 21) & 0b11) as u8;
                c.has_text = (packed >> 39) & 1 == 1;
                let idx = y * self.cols + x;
                if idx < self.cells.len() {
                    self.cells[idx] = c;
                }
            }
        }
    }
}

fn assert_screen_matches(t: &mut NativeTerminal, screen: &Screen) {
    let truth = t.snapshot();
    for i in 0..truth.cells.len().min(screen.cells.len()) {
        let a = &truth.cells[i];
        let b = &screen.cells[i];
        assert_eq!(a.codepoint, b.codepoint, "格 {i} 码点");
        assert_eq!(a.wide, b.wide, "格 {i} 宽度");
        assert_eq!(a.has_text, b.has_text, "格 {i} 有字形");
        assert_eq!(a.fg, b.fg, "格 {i} 前景色");
        assert_eq!(a.bg, b.bg, "格 {i} 背景色");
        assert_eq!(a.bold, b.bold, "格 {i} 粗体");
        assert_eq!(a.italic, b.italic, "格 {i} 斜体");
    }
}
