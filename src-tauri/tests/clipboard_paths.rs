//! `clipboard_file_paths` 命令的端到端测试：把真实文件放进 NSPasteboard，
//! 再走前端调用的那条命令路径，验证返回的是「用户在 Finder 里选中的绝对路径」。
//!
//! 为什么单独放集成测试：单测在 `src/clipboard.rs` 里（覆盖归一化/去重等
//! 纯函数），这里只测「写进真实剪贴板 → 命令读回」这一条完整链路。
//! 这是整个功能的地基 —— 前端拿不到绝对路径（浏览器 File 对象只有
//! name/type/size），路径只能由 Rust 从系统剪贴板读。
//!
//! **这些用例会覆盖用户剪贴板**，默认标记 ignore；CI 里显式跑：
//! `cargo test --test clipboard_paths -- --ignored --test-threads=1`
//! 单线程是必须的 —— 两个线程同时读写 `NSPasteboard.general` 会互相抹掉状态。

#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 造一个带中文与空格的路径：这两类是 Windows/macOS 上最常见的「看起来对
/// 却拼错」的来源（编码被改写、空格被当分隔符）。
unsafe fn make_fixture_file() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("clip_paths_test_{nanos}"));
    fs::create_dir_all(&dir).expect("创建测试目录");
    let p = dir.join("笔记 草案 v2.md");
    fs::write(&p, "# 测试\n").expect("写测试文件");
    p
}

/// 写 Finder 单文件复制的等价内容：fileURL flavor + 文件名数组 flavor 都给
/// （Finder 就是这样，只写一个的话测不出「两条路都能读到」）。
unsafe fn put_file_on_pasteboard(path: &Path) {
    // 复用 src/clipboard.rs 的测试助手：它写 NSFilenamesPboardType。
    // fileURL 那条路由 Finder 覆盖；这里复用它写文件名 flavor（路径 2）。
    lazecode_lib::clipboard::set_clipboard_file_names_for_test(&[&path.to_string_lossy()]);
}

#[test]
#[ignore = "会覆盖用户剪贴板"]
fn command_returns_absolute_path_of_pasted_file() {
    let file = unsafe { make_fixture_file() };
    unsafe { put_file_on_pasteboard(&file) };

    // 与前端完全同一条路径：invoke("clipboard_file_paths")
    let got = tauri::async_runtime::block_on(lazecode_lib::commands::clipboard_file_paths())
        .expect("命令不应失败 —— 剪贴板没文件时返回空数组，不是 Err");

    assert_eq!(got.len(), 1, "只放了一个文件，却读到 {:?}", got);
    assert_eq!(
        got[0],
        file.to_string_lossy(),
        "必须返回 Finder 里那个绝对路径"
    );
    assert!(Path::new(&got[0]).exists(), "返回的路径应真实存在");

    let _ = fs::remove_dir_all(file.parent().expect("有父目录"));
}

#[test]
#[ignore = "会覆盖用户剪贴板"]
fn command_returns_empty_when_clipboard_has_no_file() {
    // 普通文本剪贴板：写字符串 flavor，不写任何文件 flavor。
    unsafe {
        use objc2_app_kit::NSPasteboard;
        use objc2_foundation::NSString;
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let s = NSString::from_str("echo hello");
        let _ = pb.setString_forType(&s, objc2_app_kit::NSPasteboardTypeString);
    }
    let got = tauri::async_runtime::block_on(lazecode_lib::commands::clipboard_file_paths())
        .expect("没有文件不是错误");
    assert!(got.is_empty(), "纯文本剪贴板必须返回空数组，实际 {:?}", got);
}
