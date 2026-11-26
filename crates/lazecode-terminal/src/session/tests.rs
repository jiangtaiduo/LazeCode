//! Session tests: PTY/spool/resize/selection/kitty behaviour.

use super::*;

#[test]
fn spawns_shell_and_renders_prompt() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 12,
        command: Some("printf 'hello-terminal\\n'".to_string()),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    let mut snap = Snapshot::default();
    let mut found = false;
    while std::time::Instant::now() < deadline {
        session.snapshot_into(&mut snap);
        let mut text = String::new();
        for row in 0..snap.rows {
            for col in 0..snap.cols {
                text.push(snap.cell(row, col).ch);
            }
            text.push('\n');
        }
        if text.contains("hello-terminal") {
            found = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(found, "terminal output not observed");
}

#[test]
fn synchronized_update_holds_presentation_until_esu() {
    let session = spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 12,
        command: Some(
            "printf '\\033[?2026h\\033[2J\\033[HPARTIAL'; sleep 0.4; \
             printf 'DONE\\033[?2026l'"
                .to_string(),
        ),
        ..Default::default()
    })
    .expect("spawn");

    assert!(
        wait_until(
            || session.sync_update_hold().is_some(),
            std::time::Duration::from_secs(4)
        ),
        "BSU was never observed"
    );
    // The grid already holds the partial frame; a renderer that snapshots
    // here would paint it. The hold is what keeps the last full frame.
    let mut snap = Snapshot::default();
    session.snapshot_into(&mut snap);
    assert!(snapshot_text(&snap).contains("PARTIAL"));
    assert!(
        wait_until(
            || session.sync_update_hold().is_none(),
            std::time::Duration::from_secs(4)
        ),
        "ESU was never observed"
    );
    session.snapshot_into(&mut snap);
    assert!(snapshot_text(&snap).contains("DONE"));
}

#[test]
fn absurd_geometry_is_clamped_and_snapshot_capacity_reclaimed() {
    let session = spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 12,
        command: Some("printf 'hi\\n'".to_string()),
        ..Default::default()
    })
    .expect("spawn");
    // 47,599 x 1,600 cells x 32-byte CellData is the 2.38 GB single
    // allocation observed in a long-running 0.4.5 process.
    session.resize(47_599, 1_600);
    let mut snap = Snapshot {
        // Simulate a stale oversized buffer from an earlier geometry.
        cells: Vec::with_capacity(1_000_000),
        ..Snapshot::default()
    };
    session.snapshot_into(&mut snap);
    assert!(snap.cols <= MAX_COLS as usize, "cols={}", snap.cols);
    assert!(snap.rows <= MAX_ROWS as usize, "rows={}", snap.rows);
    let needed = snap.cols * snap.rows;
    assert_eq!(snap.cells.len(), needed);
    assert!(
        snap.cells.capacity() <= needed * SNAPSHOT_SHRINK_FACTOR,
        "capacity {} for {needed} cells",
        snap.cells.capacity()
    );
}

#[test]
fn cjk_wide_character_attrs() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 12,
        command: Some("printf '这是一段测试\\n'".to_string()),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    let mut snap = Snapshot::default();
    let mut found = false;
    while std::time::Instant::now() < deadline {
        session.snapshot_into(&mut snap);
        for row in 0..snap.rows {
            if snap.cell(row, 0).ch == '这' {
                println!(
                    "cell(0, 0): ch='{}', attrs={:#b}",
                    snap.cell(row, 0).ch,
                    snap.cell(row, 0).attrs
                );
                println!(
                    "cell(0, 1): ch='{}', attrs={:#b}",
                    snap.cell(row, 1).ch,
                    snap.cell(row, 1).attrs
                );
                println!(
                    "cell(0, 2): ch='{}', attrs={:#b}",
                    snap.cell(row, 2).ch,
                    snap.cell(row, 2).attrs
                );
                found = true;
                break;
            }
        }
        if found {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(found);
}

#[test]
fn scrolls_into_history() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 60,
        rows: 5,
        command: Some("for i in $(seq 1 40); do echo row-$i; done".to_string()),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if !session.search("row-40").is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let mut snap = Snapshot::default();
    session.scroll_delta(20);
    session.snapshot_into(&mut snap);
    assert!(snap.display_offset > 0, "history should be scrolled");
    let mut text = String::new();
    for row in 0..snap.rows {
        for col in 0..snap.cols {
            text.push(snap.cell(row, col).ch);
        }
        text.push('\n');
    }
    assert!(
        text.contains("row-"),
        "scrolled viewport should show history: {text}"
    );
    session.scroll_bottom();
    session.snapshot_into(&mut snap);
    assert_eq!(snap.display_offset, 0);
}

#[test]
fn resize_keeps_scrolled_viewport_on_content() {
    // Each echo is 46-47 columns: it soft-wraps at 30 columns and fits on
    // one line at 60, so resizing rewrites how many grid lines the same
    // content occupies. A scroll offset that survives the resize as a raw
    // line count lands on different content — widening even shrinks
    // history until the offset clamps to the oldest line (the reported
    // "console jumps to the top" on window resize).
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 30,
        rows: 5,
        command: Some(
            "for i in $(seq 1 40); do echo row-$i-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done; echo END-MARKER"
                .to_string(),
        ),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if !session.search("END-MARKER").is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // Drain the shell prompt that trails the marker so no reader-thread
    // output races the resizes below.
    std::thread::sleep(std::time::Duration::from_millis(200));

    let mut snap = Snapshot::default();
    let viewport = |session: &Session, snap: &mut Snapshot| {
        session.snapshot_into(snap);
        let mut text = String::new();
        for row in 0..snap.rows {
            for col in 0..snap.cols {
                text.push(snap.cell(row, col).ch);
            }
            text.push('\n');
        }
        text
    };

    let hits = session.search("row-20-");
    assert!(!hits.is_empty(), "row-20 should be in scrollback");
    session.scroll_to_line(hits[0].line);
    assert!(
        viewport(&session, &mut snap).contains("row-20-"),
        "scrolled viewport should show row-20"
    );

    // Widen: the wrapped pairs unwrap and history shrinks. The line the
    // user was reading must stay at the top of the viewport.
    session.resize(60, 5);
    let text = viewport(&session, &mut snap);
    assert!(
        text.lines().next().unwrap_or_default().contains("row-20-"),
        "widening must keep the scrolled viewport on row-20: {text}"
    );

    // Narrow again: the lines re-wrap and history grows back.
    session.resize(30, 5);
    let text = viewport(&session, &mut snap);
    assert!(
        text.lines().next().unwrap_or_default().contains("row-20-"),
        "narrowing must keep the scrolled viewport on row-20: {text}"
    );
}

#[test]
fn search_regex_classes_match_in_scrollback() {
    // The search box feeds the rio DFA directly: character classes and
    // case-folding work without a lowercase round-trip.
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 60,
        rows: 6,
        command: Some(
            "for i in 1 2 3 4 5 6 7 8 9 10; do echo line-$i; done; echo done-marker".to_string(),
        ),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if !session.search("done-marker").is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let hits = session.search("line-[7-9]");
    assert!(!hits.is_empty(), "regex class should match line-7..9");
    // rio-vt smart-case (Alacritty/less convention): a lowercase query is
    // case-insensitive, an uppercase query matches case-sensitively.
    let folded = session.search("done-mark");
    assert!(!folded.is_empty(), "lowercase query should fold case");
    let sensitive = session.search("done-MARK");
    assert!(
        sensitive.is_empty(),
        "uppercase query is case-sensitive: must NOT hit lowercase text"
    );
}

#[test]
fn resize_keeps_anchor_when_viewport_top_is_a_wrap_continuation() {
    // Dragging the window wider merges soft-wrapped rows. When the
    // viewport top is the *continuation* row of a wrapped line, its text
    // moves into the middle of the merged row after the reflow — a
    // row-start signature can never match it again, and the merged rows
    // above the viewport shift it far past the old ±24 scan window. The
    // viewport must still land on that logical line after one widen.
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 30,
        rows: 6,
        command: Some(
            "for i in $(seq 1 300); do echo row-$i-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done; echo END-MARKER"
                .to_string(),
        ),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if !session.search("END-MARKER").is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));

    let mut snap = Snapshot::default();
    let viewport = |session: &Session, snap: &mut Snapshot| {
        session.snapshot_into(snap);
        let mut text = String::new();
        for row in 0..snap.rows {
            for col in 0..snap.cols {
                text.push(snap.cell(row, col).ch);
            }
            text.push('\n');
        }
        text
    };

    // Put the continuation row of `row-220` at the very top of the
    // viewport: its first row wraps at 30 columns, so the next grid row
    // starts with the wrapped `aaaa…` fragment.
    let hits = session.search("row-220-");
    assert!(!hits.is_empty(), "row-220 should be in scrollback");
    session.scroll_to_line(hits[0].line + 1);
    let text = viewport(&session, &mut snap);
    assert!(
        text.lines()
            .next()
            .unwrap_or_default()
            .trim_start()
            .starts_with("aaaa"),
        "viewport top should be the wrap continuation of row-220: {text}"
    );

    // One widening step: every continuation merges into its first row and
    // history halves — hundreds of rows above the viewport shift.
    session.resize(60, 6);
    let text = viewport(&session, &mut snap);
    assert!(
        text.lines().next().unwrap_or_default().contains("row-220"),
        "widening must keep the viewport on the merged row-220: {text}"
    );
}

#[test]
fn resize_drag_sequence_keeps_scrolled_anchor() {
    // A drag fires a burst of small resizes. Each widening step merges
    // rows above the viewport; when one step shifts more than the scan
    // window, the old code kept rio's clamped position and the drift
    // compounded until the view pinned at the oldest line. Every step of
    // the sequence must keep the anchor line at the viewport top.
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 30,
        rows: 6,
        command: Some(
            "for i in $(seq 1 300); do echo row-$i-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done; echo END-MARKER"
                .to_string(),
        ),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if !session.search("END-MARKER").is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));

    let mut snap = Snapshot::default();
    let viewport = |session: &Session, snap: &mut Snapshot| {
        session.snapshot_into(snap);
        let mut text = String::new();
        for row in 0..snap.rows {
            for col in 0..snap.cols {
                text.push(snap.cell(row, col).ch);
            }
            text.push('\n');
        }
        text
    };

    let hits = session.search("row-260-");
    assert!(!hits.is_empty(), "row-260 should be in scrollback");
    session.scroll_to_line(hits[0].line);
    let text = viewport(&session, &mut snap);
    assert!(
        text.lines().next().unwrap_or_default().contains("row-260"),
        "scrolled viewport should show row-260 at the top: {text}"
    );

    for cols in [36u16, 42, 48, 54, 60] {
        session.resize(cols, 6);
        let text = viewport(&session, &mut snap);
        assert!(
            text.lines().next().unwrap_or_default().contains("row-260"),
            "widening to {cols} must keep row-260 at the viewport top: {text}"
        );
    }
}

#[test]
fn search_lowercase_length_change_does_not_panic() {
    // İ lowercases to i̇ (byte growth); the snapshot haystack path must
    // not slice by byte offsets that no longer align with chars.
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 40,
        rows: 6,
        command: Some("printf 'İ marker\n'".to_string()),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        let hits = session.search("marker");
        if !hits.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("marker not found");
}

#[test]
fn busy_parser_ignores_shell_and_dedupes_commands() {
    let info = parse_busy_output(
        "  100 /bin/zsh\n 101 /usr/local/bin/vite\n 102 vite\n 103 node\n",
        Some(100),
        "/bin/zsh",
    );
    assert!(info.busy);
    assert_eq!(info.commands, ["vite", "node"]);
    assert!(!parse_busy_output("  100 /bin/zsh\n", Some(100), "/bin/zsh").busy);
    let exec = parse_busy_output("  100 omp\n", Some(100), "/bin/zsh");
    assert!(exec.busy);
    assert_eq!(exec.commands, ["omp"]);
}

#[test]
fn busy_parser_distinguishes_login_shell_from_exec_d_shell() {
    // macOS reports the idle login shell with a dash (argv[0]); an exec'd
    // command over the same pid has no dash. `exec fish` previously read
    // as idle because any shell name matched.
    assert!(
        !parse_busy_output("  100 -zsh\n", Some(100), "/bin/zsh").busy,
        "dash-form login shell is idle"
    );
    assert!(
        !parse_busy_output("  100 -/bin/zsh\n", Some(100), "/bin/zsh").busy,
        "dash-path login shell is idle"
    );
    let exec_fish = parse_busy_output("  100 fish\n", Some(100), "/bin/zsh");
    assert!(exec_fish.busy, "exec'd fish over the shell pid is busy");
    assert_eq!(exec_fish.commands, ["fish"]);
    // A plain configured-shell name stays idle (non-login spawn forms).
    assert!(!parse_busy_output("  100 /bin/zsh\n", Some(100), "/bin/zsh").busy);
    // macOS 15 `ps` can report the shell as `(bash)`, sometimes under a
    // pid other than the one recorded at spawn.
    let paren = parse_busy_output("  100 (bash)\n", Some(100), "/bin/bash");
    assert!(!paren.busy, "parenthesised shell is idle: {paren:?}");
    let reexec = parse_busy_output("  200 (bash)\n", Some(100), "/bin/bash");
    assert!(!reexec.busy, "shell matches by name, not pid: {reexec:?}");
    // Documented residual: `exec zsh` over a zsh session is
    // indistinguishable from the idle prompt by `ps` alone.
    assert!(!parse_busy_output("  100 zsh\n", Some(100), "/bin/zsh").busy);
}

#[test]
fn busy_probe_batches_distinct_ttys() {
    let mut busy_options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 60,
        rows: 6,
        command: Some("sleep 30".to_string()),
        ..Default::default()
    };
    busy_options.scrollback = 32;
    let busy = spawn(busy_options).unwrap();
    let mut idle_options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 60,
        rows: 6,
        ..Default::default()
    };
    idle_options.scrollback = 32;
    let idle = spawn(idle_options).unwrap();
    let targets = [busy.busy_target(), idle.busy_target()];
    // Wait for the *exec'd command*, not merely "busy": a login shell can
    // spawn transient children while sourcing dotfiles, and any of those
    // legitimately counts as a command for one probe round.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let infos = loop {
        let probed = probe_busy_many(&targets);
        let saw_sleep = probed
            .first()
            .is_some_and(|info| info.commands.iter().any(|command| command == "sleep"));
        if saw_sleep {
            break probed;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "sleep was never reported on the busy tty: {probed:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(infos.len(), 2);
    assert!(infos[0].busy, "exec'd sleep must be busy: {infos:?}");
    // The idle pane's shell may still be sourcing dotfiles while the busy
    // one is checked; wait for it to settle.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let idle_info = probe_busy_many(&targets).swap_remove(1);
        if !idle_info.busy {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "idle shell never settled: {idle_info:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    busy.kill();
    idle.kill();
}

#[test]
fn search_finds_text_in_scrollback() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 60,
        rows: 6,
        command: Some(
            "for i in 1 2 3 4 5 6 7 8; do echo line-$i; done; echo marker-xyz".to_string(),
        ),
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        let hits = session.search("marker-xyz");
        if !hits.is_empty() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("marker not found");
}

fn test_image(bytes: usize, age_secs: u64) -> rio_vt::ansi::graphics::StoredImage {
    rio_vt::ansi::graphics::StoredImage {
        data: rio_graphics::GraphicData {
            id: rio_graphics::GraphicId(1),
            width: 1,
            height: 1,
            color_type: rio_graphics::ColorType::Rgba,
            pixels: vec![0; bytes],
            is_opaque: true,
            resize: None,
            display_width: None,
            display_height: None,
            transmit_time: rio_vt::time::Instant::now(),
        },
        transmission_time: rio_vt::time::Instant::now() - std::time::Duration::from_secs(age_secs),
    }
}

fn test_virtual_placement(image_id: u32) -> rio_vt::ansi::graphics::VirtualPlacement {
    rio_vt::ansi::graphics::VirtualPlacement {
        image_id,
        placement_id: 1,
        columns: 1,
        rows: 1,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    }
}

#[test]
fn graphics_trim_keeps_visible_and_recent_images() {
    let mut graphics = Graphics::default();
    graphics.kitty_images.insert(1, test_image(60_000, 10));
    graphics
        .kitty_virtual_placements
        .insert((1, 1), test_virtual_placement(1));
    graphics.kitty_images.insert(2, test_image(60_000, 10));
    graphics.total_bytes = 120_000;

    trim_graphics_to_soft(&mut graphics, 100_000);
    assert!(graphics.kitty_images.contains_key(&1));
    assert!(!graphics.kitty_images.contains_key(&2));
    assert!(graphics.total_bytes <= 100_000);

    // A just-transmitted image with no placement yet must survive a
    // snapshot between transmission and placement.
    graphics.kitty_images.insert(2, test_image(60_000, 0));
    graphics.total_bytes = 120_000;
    trim_graphics_to_soft(&mut graphics, 100_000);
    assert!(graphics.kitty_images.contains_key(&1));
    assert!(graphics.kitty_images.contains_key(&2));
}

#[test]
fn graphics_trim_does_not_overshoot_into_visible_images() {
    let mut graphics = Graphics::default();
    graphics.kitty_images.insert(1, test_image(100_000, 10));
    graphics
        .kitty_virtual_placements
        .insert((1, 1), test_virtual_placement(1));
    graphics.kitty_images.insert(2, test_image(30_000, 10));
    graphics.kitty_images.insert(3, test_image(30_000, 10));
    graphics.total_bytes = 160_000;

    trim_graphics_to_soft(&mut graphics, 110_000);
    assert!(
        graphics.kitty_images.contains_key(&1),
        "the visible image must survive trimming"
    );
    assert!(graphics.total_bytes <= 110_000);
}

#[test]
fn graphics_trim_leaves_visible_images_over_the_soft_target() {
    let mut graphics = Graphics::default();
    graphics.kitty_images.insert(1, test_image(120_000, 10));
    graphics
        .kitty_virtual_placements
        .insert((1, 1), test_virtual_placement(1));
    graphics.total_bytes = 120_000;

    trim_graphics_to_soft(&mut graphics, 100_000);
    assert!(graphics.kitty_images.contains_key(&1));
    assert_eq!(graphics.total_bytes, 120_000);
}

fn snapshot_text(snap: &Snapshot) -> String {
    let mut text = String::new();
    for row in 0..snap.rows {
        for col in 0..snap.cols {
            let ch = snap.cell(row, col).ch;
            text.push(if ch == '\0' { ' ' } else { ch });
        }
        text.push('\n');
    }
    text
}

fn wait_until(mut predicate: impl FnMut() -> bool, duration: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + duration;
    while std::time::Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    false
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let value = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                let bits = (value >> (18 - 6 * index)) & 0x3f;
                out.push(ALPHABET[bits as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn kitty_apc(action: &str, id: u32, width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    kitty_apc_extra(action, id, width, height, pixels, "")
}

fn kitty_apc_extra(
    action: &str,
    id: u32,
    width: u32,
    height: u32,
    pixels: &[u8],
    extra: &str,
) -> Vec<u8> {
    let payload = base64(pixels);
    // `q=2` silences the terminal's reply so the echo session used by the
    // tests cannot bounce responses back and forth.
    format!("\x1b_Ga={action}{extra},q=2,f=32,s={width},v={height},i={id};{payload}\x1b\\")
        .into_bytes()
}

/// Wait until `cat` is actually echoing, so protocol sequences written by
/// the test are processed exactly once.
fn await_echo(session: &Session, snap: &mut Snapshot) {
    let ready = wait_until(
        || {
            session.write(b"LZREADY");
            session.snapshot_into(snap);
            snapshot_text(snap).contains("LZREADY")
        },
        std::time::Duration::from_secs(6),
    );
    assert!(ready, "echo session never became ready");
}

#[test]
fn cold_spool_extends_history_and_rehydrates() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 40,
        rows: 8,
        command: Some("for i in $(seq 1 2000); do echo row-$i; done".to_string()),
        // Small hot budget so most history has to live on disk.
        scrollback: 32,
        cold_lines: 10_000,
        cold_bytes: 8 << 20,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let mut snap = Snapshot::default();
    let done = wait_until(
        || {
            session.snapshot_into(&mut snap);
            snap.cold_lines > 1_500 && session.search("row-2000").iter().any(|m| m.line >= 0)
        },
        std::time::Duration::from_secs(15),
    );
    assert!(
        done,
        "spooled history did not fill: {:?}",
        session.history_stats()
    );

    let stats = session.history_stats();
    // Hot memory plateaus at the configured budget; the rest is on disk.
    assert!(stats.hot_lines <= 32, "hot lines grew: {stats:?}");
    assert!(
        snap.hot_history_size <= 32,
        "snapshot hot lines grew: {stats:?}"
    );
    assert!(stats.cold_lines > 1_500, "cold lines missing: {stats:?}");
    assert!(stats.cold_bytes > 0, "cold bytes missing: {stats:?}");
    assert!(stats.total_lines >= 1_990, "history lost: {stats:?}");
    assert!(session.cold_path().is_some_and(|path| path.exists()));

    // Search sees the cold rows: their absolute line is older than the
    // hot window's oldest line.
    let hot = snap.hot_history_size as i32;
    let hits = session.search("row-1500");
    let cold_hit = hits
        .iter()
        .find(|hit| hit.line < -hot)
        .unwrap_or_else(|| panic!("expected a cold hit, got {hits:?} (hot = {hot})"))
        .to_owned();
    // Jumping to a cold hit places that row at the top of the viewport.
    session.scroll_to_line(cold_hit.line);
    session.snapshot_into(&mut snap);
    let landed = snapshot_text(&snap);
    assert!(
        landed.starts_with("row-1500"),
        "cold hit navigation landed on: {landed:?}"
    );

    // Scrolling to the top rehydrates the oldest page from disk.
    session.scroll_top();
    session.snapshot_into(&mut snap);
    assert!(snap.display_offset > snap.hot_history_size);
    assert_eq!(snap.cold_lines, stats.cold_lines);
    let text = snapshot_text(&snap);
    assert!(
        text.starts_with("row-1"),
        "cold page not rehydrated: {text:?}"
    );
    session.scroll_bottom();
    session.snapshot_into(&mut snap);
    assert_eq!(snap.display_offset, 0);
    assert!(snapshot_text(&snap).contains("row-2000"));

    // Explicit clear drops both tiers.
    session.clear_scrollback();
    assert_eq!(session.history_stats().cold_lines, 0);
    let mut snap = Snapshot::default();
    session.snapshot_into(&mut snap);
    assert_eq!(snap.history_size, 0);
}

#[test]
fn cold_spool_file_is_removed_on_close() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 30,
        rows: 4,
        command: Some("for i in $(seq 1 400); do echo row-$i; done".to_string()),
        scrollback: 16,
        cold_lines: 10_000,
        cold_bytes: 8 << 20,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    assert!(
        wait_until(
            || session.history_stats().cold_lines > 0,
            std::time::Duration::from_secs(10)
        ),
        "nothing was spooled"
    );
    let path = session.cold_path().expect("spool enabled");
    assert!(path.exists());
    drop(session);
    // The reader thread owns a clone of the spool, so the file disappears
    // when that thread notices the cancel flag and exits, not when
    // `drop` returns. Wait for it instead of racing the teardown.
    assert!(
        wait_until(|| !path.exists(), std::time::Duration::from_secs(10)),
        "closing the session must delete the spool"
    );
}

fn echo_session(rows: u16) -> Session {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 40,
        rows,
        command: Some("stty raw -echo; cat".to_string()),
        ..Default::default()
    };
    spawn(options).expect("spawn")
}

#[test]
fn scrolled_viewport_stays_pinned_while_output_continues() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 40,
        rows: 8,
        command: Some("stty raw -echo; cat".to_string()),
        // Force most of the history onto disk so the pin spans tiers.
        scrollback: 32,
        cold_lines: 10_000,
        cold_bytes: 8 << 20,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let mut snap = Snapshot::default();
    await_echo(&session, &mut snap);

    let mut blob = String::new();
    for line in 1..=200 {
        blob.push_str(&format!("row-{line}\r\n"));
    }
    session.write(blob.as_bytes());
    let arrived = wait_until(
        || {
            session.snapshot_into(&mut snap);
            session
                .search("row-200")
                .iter()
                .any(|hit| hit.line >= -(snap.rows as i32))
        },
        std::time::Duration::from_secs(10),
    );
    assert!(
        arrived,
        "no row-200: hits={:?} text={:?} stats={:?}",
        session.search("row-200"),
        snapshot_text(&snap),
        session.history_stats()
    );

    session.scroll_delta(40);
    session.snapshot_into(&mut snap);
    assert_eq!(snap.display_offset, 40);
    let pinned = snapshot_text(&snap);
    let first_line = pinned.lines().next().unwrap_or("").to_string();
    assert!(first_line.starts_with("row-"), "pinned line: {pinned:?}");

    let mut more = String::new();
    for line in 201..=300 {
        more.push_str(&format!("row-{line}\r\n"));
    }
    session.write(more.as_bytes());
    assert!(wait_until(
        || {
            session.snapshot_into(&mut snap);
            session
                .search("row-300")
                .iter()
                .any(|hit| hit.line >= -(snap.rows as i32))
        },
        std::time::Duration::from_secs(10)
    ));
    assert!(
        snap.display_offset > 40,
        "the viewport must follow the history growth: {}",
        snap.display_offset
    );
    assert_eq!(
        snapshot_text(&snap).lines().next().unwrap_or(""),
        first_line,
        "a scrolled viewport must stay pinned to its content"
    );
    assert!(session.history_stats().cold_lines > 0);
}

#[test]
fn kitty_png_payload_decodes() {
    let session = echo_session(4);
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ])
            .expect("png data");
    }
    let mut sequence = format!("\x1b_Ga=t,f=100,q=2,i=9;{}\x1b\\", base64(&png)).into_bytes();
    sequence.extend_from_slice(b"\x1b_Ga=p,U=1,i=9,p=1,c=2,r=1,q=2\x1b\\");
    sequence.extend_from_slice(b"\x1b[38;2;0;0;9m\x1b[58;2;0;0;1m");
    sequence
        .extend_from_slice(rio_vt::ansi::kitty_virtual::encode_placeholder(0, 0, None).as_bytes());
    sequence
        .extend_from_slice(rio_vt::ansi::kitty_virtual::encode_placeholder(0, 1, None).as_bytes());
    sequence.extend_from_slice(b"\x1b[0m");
    session.write(&sequence);
    let mut updates = Vec::new();
    let mut snap = Snapshot::default();
    let received = wait_until(
        || {
            updates.extend(session.take_graphics_updates());
            session.snapshot_into(&mut snap);
            updates.iter().any(|image| image.id == 9) && !snap.virtual_graphics.is_empty()
        },
        std::time::Duration::from_secs(6),
    );
    assert!(
        received,
        "png image / virtual run missing: images={:?} runs={} text={:?}",
        snap.graphics_images,
        snap.virtual_graphics.len(),
        snapshot_text(&snap)
    );
}

#[test]
fn kitty_virtual_placeholders_resolve_to_runs() {
    let session = echo_session(4);
    session.set_cell_size(8.0, 16.0);
    let pixels = vec![0xffu8; 16];
    let mut seq = kitty_apc("t", 7, 2, 2, &pixels);
    seq.extend_from_slice(b"\x1b_Ga=p,U=1,i=7,p=1,c=2,r=1,q=2\x1b\\");
    seq.extend_from_slice(b"\x1b[38;2;0;0;7m\x1b[58;2;0;0;1m");
    seq.extend_from_slice(rio_vt::ansi::kitty_virtual::encode_placeholder(0, 0, None).as_bytes());
    seq.extend_from_slice(rio_vt::ansi::kitty_virtual::encode_placeholder(0, 1, None).as_bytes());
    seq.extend_from_slice(b"\x1b[0m");
    session.write(&seq);
    let mut snap = Snapshot::default();
    let found = wait_until(
        || {
            session.snapshot_into(&mut snap);
            !snap.virtual_graphics.is_empty()
        },
        std::time::Duration::from_secs(6),
    );
    assert!(
        found,
        "no virtual runs: images={:?} text={:?}",
        snap.graphics_images,
        snapshot_text(&snap)
    );
    let run = &snap.virtual_graphics[0];
    assert_eq!(run.image_id, 7);
    assert!(run.columns > 0.0 && run.rows > 0.0);
    assert_eq!(run.source_uv, [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn kitty_graphics_are_parsed_placed_and_deleted() {
    let session = echo_session(10);
    session.set_cell_size(8.0, 16.0);
    let pixels = vec![
        0xff, 0x00, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, //
        0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    // An explicit placement id makes a retransmission replace the same
    // placement instead of stacking another one (rio allocates a fresh
    // internal id for `p=0`).
    let sequence = kitty_apc_extra("T", 7, 2, 2, &pixels, ",p=1");
    let mut updates = Vec::new();
    let received = wait_until(
        || {
            session.write(&sequence);
            updates.extend(session.take_graphics_updates());
            updates.iter().any(|image| image.id == 7)
        },
        std::time::Duration::from_secs(6),
    );
    assert!(received, "kitty image was not decoded");
    let image = updates.iter().find(|image| image.id == 7).expect("image 7");
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(image.rgba.len(), 16);
    assert_eq!(&image.rgba[..4], &pixels[..4]);

    let mut snap = Snapshot::default();
    session.snapshot_into(&mut snap);
    let placement = snap
        .graphics
        .iter()
        .find(|placement| placement.image_id == 7)
        .expect("placement for image 7");
    assert!(placement.columns >= 1 && placement.rows >= 1);
    assert_eq!(placement.source, (0, 0, 0, 0));
    assert!(snap.graphics_images.contains(&7));

    // A second image with a higher z-index is painted on top.
    let second = vec![0x40u8; 16];
    let received = wait_until(
        || {
            session.write(&kitty_apc_extra("T", 8, 2, 2, &second, ",p=2,z=5"));
            session.snapshot_into(&mut snap);
            snap.graphics_images.contains(&8)
        },
        std::time::Duration::from_secs(6),
    );
    assert!(received, "second image missing: {:?}", snap.graphics_images);
    let z_order: Vec<i32> = snap
        .graphics
        .iter()
        .map(|placement| placement.z_index)
        .collect();
    assert_eq!(z_order, vec![0, 5], "placements must be z-ordered");

    // `d=A` (uppercase) drops every placement *and* the image data.
    session.write(b"\x1b_Ga=d,d=A\x1b\\");
    let deleted = wait_until(
        || {
            session.snapshot_into(&mut snap);
            snap.graphics_images.is_empty() && snap.graphics.is_empty()
        },
        std::time::Duration::from_secs(6),
    );
    assert!(deleted, "images were not deleted");
}

#[test]
fn kitty_graphics_reject_undecodable_png_payloads_safely() {
    let session = echo_session(8);
    session.set_cell_size(8.0, 16.0);
    let mut snap = Snapshot::default();
    await_echo(&session, &mut snap);
    // `f=100` is the PNG format code; without the `graphics` feature this
    // payload cannot be decoded, so drop the request without wedging the
    // parser.
    let apc = format!(
        "\x1b_Ga=T,q=2,f=100,i=22;{}\x1b\\",
        base64(b"definitely not a png")
    );
    session.write(apc.as_bytes());

    // A raw image sent afterwards still decodes, proving the parser and
    // the graphics state survived.
    let pixels = vec![0x7fu8; 16];
    session.write(&kitty_apc_extra("T", 23, 2, 2, &pixels, ",p=1"));
    let mut images = Vec::new();
    let recovered = wait_until(
        || {
            images.extend(session.take_graphics_updates());
            images.iter().any(|image| image.id == 23)
        },
        std::time::Duration::from_secs(6),
    );
    assert!(recovered, "session stopped parsing after a bad png payload");
    session.snapshot_into(&mut snap);
    assert!(snap.graphics_images.contains(&23));
    assert!(!snap.graphics_images.contains(&22));
}

#[test]
fn kitty_graphics_chunked_transmission_is_reassembled() {
    let session = echo_session(8);
    session.set_cell_size(8.0, 16.0);
    let mut snap = Snapshot::default();
    await_echo(&session, &mut snap);

    let pixels: Vec<u8> = (0..16u8).map(|value| value.wrapping_mul(16)).collect();
    let payload = base64(&pixels);
    let (head, tail) = payload.split_at(payload.len() / 2);
    // First chunk: `m=1` keeps the accumulator open. The continuation
    // repeats the id and omits the transmission metadata.
    let first = format!("\x1b_Ga=T,m=1,q=2,f=32,s=2,v=2,i=11;{head}\x1b\\");
    let second = format!("\x1b_Gm=0,q=2,i=11;{tail}\x1b\\");
    session.write(first.as_bytes());
    session.write(second.as_bytes());

    let mut images = Vec::new();
    let received = wait_until(
        || {
            images.extend(session.take_graphics_updates());
            images.iter().any(|image| image.id == 11)
        },
        std::time::Duration::from_secs(6),
    );
    assert!(received, "chunked image was not reassembled");
    let image = images
        .iter()
        .find(|image| image.id == 11)
        .expect("image 11");
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(image.rgba.as_ref(), pixels.as_slice());
}

#[test]
fn kitty_graphics_ignore_oversized_images() {
    let session = echo_session(8);
    session.set_cell_size(8.0, 16.0);
    // 20000 columns exceeds the protocol's 10000 limit.
    let oversized = b"\x1b_Ga=T,f=32,s=20000,v=2,i=9;AAAA\x1b\\";
    let mut snap = Snapshot::default();
    let ignored = wait_until(
        || {
            session.write(oversized);
            session.snapshot_into(&mut snap);
            !snap.graphics_images.contains(&9)
        },
        std::time::Duration::from_secs(3),
    );
    assert!(ignored);
    // A malformed command must not wedge the parser either.
    session.write(b"\x1b_Gnot-a-command\x1b\\");
    assert!(wait_until(
        || {
            session.snapshot_into(&mut snap);
            snap.rows > 0 && !snap.graphics_images.contains(&9)
        },
        std::time::Duration::from_secs(2)
    ));
}

#[test]
fn kitty_graphics_cache_is_bounded_by_the_byte_cap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first_path = dir.path().join("first.apc");
    let second_path = dir.path().join("second.apc");
    // Two 200x200 RGBA images are 160 KB each, more than the 256 KB cap.
    std::fs::write(
        &first_path,
        kitty_apc("T", 1, 200, 200, &vec![0x10u8; 200 * 200 * 4]),
    )
    .expect("write");
    std::fs::write(
        &second_path,
        kitty_apc("T", 2, 200, 200, &vec![0x20u8; 200 * 200 * 4]),
    )
    .expect("write");
    let options = SpawnOptions {
        cwd: dir.path().to_path_buf(),
        cols: 40,
        rows: 8,
        command: Some(format!(
            "cat '{}' '{}'",
            first_path.display(),
            second_path.display()
        )),
        graphics_bytes: 256 * 1024,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    session.set_cell_size(8.0, 16.0);
    let mut snap = Snapshot::default();
    let bounded = wait_until(
        || {
            let _ = session.take_graphics_updates();
            session.snapshot_into(&mut snap);
            snap.graphics_images.contains(&2)
        },
        std::time::Duration::from_secs(10),
    );
    assert!(bounded, "second image missing: {:?}", snap.graphics_images);
    // Visible placements are protected above the soft target; both
    // images stay until the hard cap (4x the soft target) is reached.
    assert!(
        snap.graphics_images.contains(&1),
        "visible image should not be evicted: {:?}",
        snap.graphics_images
    );
    let total: usize = snap.graphics_images.len() * 200 * 200 * 4;
    assert!(total <= 4 * 256 * 1024);
}

#[test]
fn pending_graphics_queue_is_bounded_when_never_drained() {
    // A background console never drains `take_graphics_updates`, so the
    // pending queue caps itself at the session's graphics budget.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut files = Vec::new();
    for index in 1..=10u32 {
        let path = dir.path().join(format!("img{index}.apc"));
        std::fs::write(
            &path,
            kitty_apc("T", index, 200, 200, &vec![index as u8; 200 * 200 * 4]),
        )
        .expect("write");
        files.push(path);
    }
    let mut command = String::from("cat");
    for path in &files {
        command.push_str(&format!(" '{}'", path.display()));
    }
    let options = SpawnOptions {
        cwd: dir.path().to_path_buf(),
        cols: 40,
        rows: 8,
        command: Some(command),
        graphics_bytes: 256 * 1024,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    session.set_cell_size(8.0, 16.0);
    let mut snap = Snapshot::default();
    let parsed = wait_until(
        || {
            session.snapshot_into(&mut snap);
            snap.graphics_images.contains(&10)
        },
        std::time::Duration::from_secs(10),
    );
    assert!(
        parsed,
        "tenth image never parsed: {:?}",
        snap.graphics_images
    );
    // The pending queue holds at most one budget of the newest payloads.
    let taken = session.take_graphics_updates();
    let total: usize = taken.iter().map(|image| image.rgba.len()).sum();
    assert!(
        total <= 256 * 1024,
        "pending queue exceeded the budget: {} bytes in {} images",
        total,
        taken.len()
    );
    assert!(
        !taken.is_empty(),
        "the newest image must still be delivered"
    );
}

#[test]
fn keyboard_protocol_stack_and_modify_other_keys_are_parsed() {
    let session = echo_session(8);
    let mut snap = Snapshot::default();
    await_echo(&session, &mut snap);
    session.write(b"\x1b[>1u");
    assert!(wait_until(
        || session.modes().kitty.bits() == 1,
        std::time::Duration::from_secs(5)
    ));
    let modes = session.modes();
    assert!(modes.kitty_keyboard);
    assert_eq!(
        crate::input::encode_key(
            crate::input::KeyStroke::plain(crate::input::TerminalKey::Char('a'))
                .with_event(crate::input::KeyEvent::Release),
            &modes
        ),
        None,
        "release needs the event-types flag"
    );

    session.write(b"\x1b[>3u");
    assert!(wait_until(
        || session.modes().kitty.bits() == 3,
        std::time::Duration::from_secs(5)
    ));
    let modes = session.modes();
    let release = crate::input::KeyStroke::plain(crate::input::TerminalKey::Char('a'))
        .with_event(crate::input::KeyEvent::Release);
    assert_eq!(
        crate::input::encode_key(release, &modes).expect("encoded"),
        b"\x1b[97;1:3u".to_vec()
    );

    session.write(b"\x1b[<u");
    assert!(wait_until(
        || session.modes().kitty.bits() == 1,
        std::time::Duration::from_secs(5)
    ));
    session.write(b"\x1b[<u");
    assert!(wait_until(
        || !session.modes().kitty.is_active(),
        std::time::Duration::from_secs(5)
    ));

    session.write(b"\x1b[>4;2m");
    assert!(wait_until(
        || session.modes().modify_other_keys == 2,
        std::time::Duration::from_secs(5)
    ));
    let modes = session.modes();
    let ctrl_enter = crate::input::KeyStroke {
        key: crate::input::TerminalKey::Enter,
        ctrl: true,
        alt: false,
        shift: false,
        event: crate::input::KeyEvent::Press,
    };
    assert_eq!(
        crate::input::encode_key(ctrl_enter, &modes).expect("encoded"),
        b"\x1b[27;5;13~".to_vec()
    );
    session.write(b"\x1b[>4m");
    assert!(wait_until(
        || session.modes().modify_other_keys == 0,
        std::time::Duration::from_secs(5)
    ));
}

#[test]
fn alt_screen_does_not_rewind_or_clear_cold_history() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 40,
        rows: 6,
        command: Some(
            "for i in $(seq 1 600); do echo pre-$i; done; \
             printf '\\033[?1049h'; for i in $(seq 1 300); do echo alt-$i; done; \
             printf '\\033[?1049l'; echo POST-MARKER; sleep 30"
                .to_string(),
        ),
        scrollback: 24,
        cold_lines: 10_000,
        cold_bytes: 8 << 20,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let mut snap = Snapshot::default();
    assert!(
        wait_until(
            || {
                session.snapshot_into(&mut snap);
                session
                    .search("POST-MARKER")
                    .iter()
                    .any(|hit| hit.line >= 0)
                    && snap.cold_lines > 100
            },
            std::time::Duration::from_secs(15)
        ),
        "alt screen round trip did not finish: {:?}",
        session.history_stats()
    );
    let hot = snap.hot_history_size as i32;
    let before = session.history_stats();
    assert!(before.cold_lines > 100, "no cold history: {before:?}");
    let pre = session.search("pre-100");
    assert!(
        pre.iter().any(|hit| hit.line < -hot),
        "pre-alt rows must survive the screen swap: {pre:?} (hot = {hot})"
    );
    assert!(
        !session.search("alt-250").iter().any(|hit| hit.line < -hot),
        "alt-screen rows must not be archived into main-screen history"
    );
    assert!(
        session.history_stats().cold_lines >= before.cold_lines,
        "leaving alt screen must not clear the cold store"
    );
    session.clear_scrollback();
    assert_eq!(session.history_stats().cold_lines, 0);
}

#[test]
fn cold_search_matches_hot_smart_case_regex() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 60,
        rows: 6,
        command: Some(
            "printf 'COLD-CASE-MARKER\\n'; for i in $(seq 1 1200); do echo cold-row-$i; done; sleep 20"
                .to_string(),
        ),
        scrollback: 16,
        cold_lines: 10_000,
        cold_bytes: 8 << 20,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let mut snap = Snapshot::default();
    assert!(
        wait_until(
            || {
                session.snapshot_into(&mut snap);
                snap.cold_lines > 500 && !session.search("cold-row-1190").is_empty()
            },
            std::time::Duration::from_secs(15)
        ),
        "cold history did not fill: {:?}",
        session.history_stats()
    );
    let hot = snap.hot_history_size as i32;
    let regex_hits = session.search("cold-row-1[0-9]{2}");
    assert!(
        regex_hits.iter().any(|hit| hit.line < -hot),
        "regex classes must match in the cold tier: {regex_hits:?} (hot = {hot})"
    );
    let folded = session.search("cold-case-marker");
    assert!(
        folded.iter().any(|hit| hit.line < -hot),
        "lowercase queries fold case in the cold tier: {folded:?}"
    );
    let sensitive = session.search("cold-case-MARKER");
    assert!(
        sensitive.is_empty(),
        "uppercase queries are case-sensitive in the cold tier: {sensitive:?}"
    );
}

#[test]
fn real_ris_clears_cold_history_but_not_a_csi_c_query() {
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 40,
        rows: 6,
        command: Some(
            "for i in $(seq 1 300); do echo before-$i; done; \
             printf '\\033[c'; echo CSI-QUERY-KEPT; sleep 1; \
             printf '\\033c'; echo AFTER-RIS; sleep 20"
                .to_string(),
        ),
        scrollback: 24,
        cold_lines: 10_000,
        cold_bytes: 8 << 20,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let mut snap = Snapshot::default();
    assert!(
        wait_until(
            || {
                session.snapshot_into(&mut snap);
                snap.cold_lines > 100 && !session.search("CSI-QUERY-KEPT").is_empty()
            },
            std::time::Duration::from_secs(15)
        ),
        "rows before the device query were not spooled: {:?}",
        session.history_stats()
    );
    assert!(
        wait_until(
            || !session.search("AFTER-RIS").is_empty(),
            std::time::Duration::from_secs(5)
        ),
        "real RIS output was not observed"
    );
    // The device query `CSI c` must not reset anything: rows before it stay
    // addressable, while the following real RIS drops the cold tier.
    assert_eq!(
        session.history_stats().cold_lines,
        0,
        "stats: {:?}",
        session.history_stats()
    );
    let mut snap = Snapshot::default();
    session.snapshot_into(&mut snap);
    assert_eq!(snap.cold_lines, 0);
}

#[test]
fn scrolled_viewport_keeps_its_distance_when_cold_rows_are_evicted() {
    // A hidden console keeps producing output. The bounded cold tier evicts
    // its oldest rows, so a viewport that keeps counting them outgrows the
    // addressable window and the next snapshot clamps it to the oldest
    // retained row — the console jumps to the top when its project is shown
    // again. The offset must follow the live window instead.
    let options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 40,
        rows: 6,
        command: Some("stty raw -echo; cat".to_string()),
        scrollback: 16,
        cold_lines: 40,
        cold_bytes: 8 << 20,
        ..Default::default()
    };
    let session = spawn(options).expect("spawn");
    let mut snap = Snapshot::default();
    await_echo(&session, &mut snap);

    let mut blob = String::new();
    for line in 1..=100 {
        blob.push_str(&format!("row-{line}\r\n"));
    }
    session.write(blob.as_bytes());
    assert!(wait_until(
        || !session.search("row-100").is_empty(),
        std::time::Duration::from_secs(10)
    ));

    session.scroll_delta(10);
    session.snapshot_into(&mut snap);
    assert_eq!(snap.display_offset, 10);

    // No snapshots in between: the pane belongs to another project.
    let mut more = String::new();
    for line in 101..=400 {
        more.push_str(&format!("row-{line}\r\n"));
    }
    session.write(more.as_bytes());
    assert!(wait_until(
        || !session.search("row-400").is_empty(),
        std::time::Duration::from_secs(15)
    ));

    session.snapshot_into(&mut snap);
    assert!(
        snap.display_offset <= 12,
        "viewport must keep its distance from live output: offset={} history={}",
        snap.display_offset,
        snap.history_size
    );
    let text = snapshot_text(&snap);
    let first = text.lines().next().unwrap_or("");
    assert!(
        first.contains("row-38") || first.contains("row-39"),
        "viewport should show recent history, not the oldest retained row: {first:?}"
    );
}
