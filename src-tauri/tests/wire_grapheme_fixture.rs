//! 字素簇 / 行标记的跨语言线格式夹具。
//!
//! 与 `vt::fixture::emit_wire_fixture` 同一用意：Rust 侧编码一份含组合符、
//! ZWJ emoji 与 OSC 133 prompt 标记的帧，TS 的 `decodeCellFrame` /
//! `decodeCellDiff` 独立解码后逐项比对。任何一边改了簇表布局或行标记位，
//! `npm run verify:wire` 都会红。
//!
//! 生成/更新夹具（幂等，重复运行写出同样的字节）：
//!   cargo test --test wire_grapheme_fixture
//!
//! 为什么单独一个测试目标：`src-tauri/src/vt.rs` 的 fixture 模块只覆盖
//! 基础单元格，字素簇需要 mode 2027 才能让 ZWJ 序列整簇进一格。

use std::path::Path;

use lazecode_lib::vt::{encode_snapshot, NativeTerminal};

#[test]
fn emit_grapheme_fixture() {
    let mut t = NativeTerminal::new(8, 3).unwrap();
    // 第 0 行：OSC 133 A（prompt 行）+ `e` + U+0301（组合符）
    t.write("\x1b]133;A\x07$ e\u{301}".as_bytes());
    // 第 1 行：mode 2027 打开字素聚类后，ZWJ 家庭 emoji 整簇占一格（宽 2）
    t.write("\x1b[?2027h\x1b[2;1H👨‍👩‍👧‍👦".as_bytes());

    let snap = t.snapshot();
    let full = encode_snapshot(&snap);
    // 整屏增量帧：同一屏内容走另一条编码路径（逐行行头 + 帧尾簇表）
    let diff = t.snapshot_wire_diff(true);

    // 夹具自检：内容不对时宁可红，也不要写出无意义的基准
    assert_eq!(
        snap.clusters,
        vec!["e\u{301}".to_string(), "👨‍👩‍👧‍👦".to_string()],
        "夹具应包含组合符与家庭 emoji 两个簇"
    );
    assert_eq!(
        snap.row_flags[0],
        lazecode_lib::vt::row_flags::PROMPT,
        "第 0 行应是 OSC 133 prompt 行"
    );
    assert_eq!(snap.row_flags[1], 0, "第 1 行不应带 prompt 标记");

    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src/ghostty-vt/__fixtures__");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("grapheme.bin"), &full).unwrap();
    std::fs::write(dir.join("grapheme-diff.bin"), &diff).unwrap();

    let cells: Vec<serde_json::Value> = snap
        .cells
        .iter()
        .map(|c| {
            serde_json::json!({
                "codepoint": c.codepoint,
                "wide": c.wide,
                "hasText": c.has_text,
                "clusterIndex": c.cluster_index,
            })
        })
        .collect();
    let expected = serde_json::json!({
        "cols": snap.cols,
        "rows": snap.rows,
        "clusters": snap.clusters,
        "rowFlags": snap.row_flags,
        "cells": cells,
        "diffLength": diff.len(),
    });
    let mut json = serde_json::to_string_pretty(&expected).unwrap();
    json.push('\n');
    std::fs::write(dir.join("grapheme.expected.json"), json).unwrap();

    println!(
        "字素夹具: full={}B diff={}B clusters={:?}",
        full.len(),
        diff.len(),
        snap.clusters
    );
}
