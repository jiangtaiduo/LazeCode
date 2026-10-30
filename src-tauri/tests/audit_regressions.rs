//! 审计修复回归测试（集成测试，走 lazecode_lib 公开 API）。
//!
//! 覆盖两项 P0/P1 修复：
//! - `find_ci`：字节安全的大小写不敏感查找（旧实现遇非 ASCII 字符 panic）；
//! - `git_status_impl`：porcelain 行解析（旧实现按字节切片，中文路径会 panic）。
//! - `git_discard_file_changes`：仓库外路径安全闸（旧实现可能删工作区外文件）。
//!
//! 命令内部行为的具体测试留在 src/commands.rs 的 #[cfg(test)] 模块里，
//! 这里只放需要跨 crate 可见性的回归用例。

use lazecode_lib::commands::{find_ci, git_discard_file_changes, git_status_impl};

// ============ find_ci（P0：非 ASCII panic 回归） ============

#[test]
fn find_ci_matches_ascii_case_insensitively() {
    assert_eq!(find_ci("Hello, World", "world"), Some(7));
    // 两侧都做小写化：needle 大小写不限（global_search 会预降小写，
    // 但函数契约不再依赖调用方预处理）
    assert_eq!(find_ci("Hello", "HELLO"), Some(0));
    assert_eq!(find_ci("no match here", "xyz"), None);
}

#[test]
fn find_ci_survives_multibyte_haystack() {
    // 修复前：滑窗按字节偏移做 &haystack[i..]，多字节边界直接 panic
    // 每个汉字 3 字节：「世界」从字节 6 开始
    assert_eq!(find_ci("你好世界", "世界"), Some(6));
    // 带音标字符（é = 2 字节）："café " 共 6 字节，"Déjà" 从字节 6 起
    assert_eq!(find_ci("café Déjà Vu", "déjà"), Some(6));
    assert_eq!(find_ci("ÄÖÜ Short", "äöü"), Some(0));
    // 中文 + ASCII 混排
    assert_eq!(find_ci("文件 FILE.txt 说明", "file"), Some(7));
}

#[test]
fn find_ci_handles_non_ascii_needle() {
    assert_eq!(find_ci("Hello 世界 hello", "世界"), Some(6));
    // 非 ASCII needle 在纯 ASCII haystack 中找不到
    assert_eq!(find_ci("plain ascii", "中文"), None);
    // emoji（4 字节 astral 字符）
    assert_eq!(find_ci("a 🎉 b", "🎉"), Some(2));
}

#[test]
fn find_ci_needle_longer_than_haystack() {
    assert_eq!(find_ci("ab", "abcdef"), None);
    assert_eq!(find_ci("中文", "中文中文"), None);
    assert_eq!(find_ci("", "a"), None);
    // 字节数 needle 更长但 char 数相同：仍按找不到处理（严格匹配）
    assert_eq!(find_ci("€", "€€"), None);
}

#[test]
fn find_ci_empty_needle_matches_at_start() {
    assert_eq!(find_ci("abc", ""), Some(0));
    assert_eq!(find_ci("", ""), Some(0));
    assert_eq!(find_ci("中文", ""), Some(0));
}

#[test]
fn find_ci_returns_byte_offset_sliceable() {
    // 契约：返回值是字节偏移，调用方（search_dir_internal）用它定位
    // 高亮区间，必须能直接切原串而不 panic
    let haystack = "中文 find_me 尾部";
    let pos = find_ci(haystack, "FIND_ME").expect("混排行应命中");
    assert_eq!(&haystack[pos..pos + "find_me".len()], "find_me");

    let cjk = "你好世界";
    let pos = find_ci(cjk, "世界").expect("中文应命中");
    assert_eq!(
        &cjk[pos..pos + "世界".len()],
        "世界",
        "中文场景返回的也必须是原串字节偏移"
    );
}

// ============ git_status_impl（P1：中文路径解析回归） ============

/// 与 src 内测试同风格的 git 调用助手：失败即 panic 并带 stderr。
fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("启动 git 失败");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn temp_repo(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ide_audit_reg_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    dir
}

#[test]
fn git_status_parses_chinese_paths_without_panic() {
    let dir = temp_repo("cjk");
    std::fs::create_dir_all(dir.join("中文目录")).unwrap();
    std::fs::write(dir.join("中文目录/文件.txt"), "hello\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "init"]);

    // 修改已跟踪的中文路径文件 → 工作区 M
    std::fs::write(dir.join("中文目录/文件.txt"), "changed\n").unwrap();
    // 未跟踪的中文路径文件 → ??（status 用 -uall 展开成具体文件）
    std::fs::write(dir.join("未跟踪.txt"), "new\n").unwrap();

    let files = git_status_impl(Some(dir.to_string_lossy().to_string())).unwrap();

    let modified = files
        .iter()
        .find(|f| f.path == "中文目录/文件.txt")
        .expect("中文路径的修改项应被解析（旧实现在此 panic/乱码）");
    assert_eq!(modified.status, "M", "已跟踪修改应为 M");
    assert!(!modified.is_staged, "只改工作区不应标记 staged");

    let untracked = files
        .iter()
        .find(|f| f.path == "未跟踪.txt")
        .expect("中文路径的未跟踪项应被解析");
    assert_eq!(untracked.status, "U", "未跟踪应归一化为 U");
    assert!(!untracked.is_staged);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn git_status_parses_rename_arrow_line() {
    let dir = temp_repo("rename");
    std::fs::write(dir.join("old.txt"), "content\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "init"]);

    // git mv 的 rename 落在暂存区，porcelain 行形如 "R  old -> new"
    git(&dir, &["mv", "old.txt", "new.txt"]);

    let files = git_status_impl(Some(dir.to_string_lossy().to_string())).unwrap();

    let renamed = files
        .iter()
        .find(|f| f.path == "new.txt")
        .expect("rename 的新路径应被解析");
    assert_eq!(renamed.status, "R", "暂存的 rename 应保持 R");
    assert!(renamed.is_staged, "rename 在暂存区，应标记 staged");
    assert!(
        !files.iter().any(|f| f.path.contains("->")),
        "「->」不应残留在路径字段里"
    );
    assert!(
        !files.iter().any(|f| f.path == "old.txt"),
        "rename 的旧路径不应单独成条目"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn git_status_skips_malformed_lines() {
    // 直接种一个格式异常的行没有低成本入口（解析器吃 git 真实输出），
    // 这里验证解析对「短行」的健壮性：空仓库只产生空输出，不 panic。
    let dir = temp_repo("empty");
    let files = git_status_impl(Some(dir.to_string_lossy().to_string())).unwrap();
    assert!(files.is_empty(), "空仓库不应有变更条目");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn git_discard_rejects_paths_outside_repo() {
    // 回归：strip_prefix 失败时旧实现把仓库外绝对路径原样返回，
    // Path::join 遇绝对路径整体替换 → git rm / 磁盘删除可删工作区外文件。
    let dir = temp_repo("outside");
    let outside_dir =
        std::env::temp_dir().join(format!("ide_audit_outside_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside_dir);
    std::fs::create_dir_all(&outside_dir).unwrap();
    let victim = outside_dir.join("victim.txt");
    std::fs::write(&victim, "precious\n").unwrap();

    // 仓库外绝对路径必须报错
    let err = git_discard_file_changes(
        Some(dir.to_string_lossy().to_string()),
        victim.to_string_lossy().to_string(),
    )
    .unwrap_err();
    assert!(!err.is_empty(), "应给出明确错误: {err}");
    assert!(victim.exists(), "仓库外文件绝不能被删除");

    // 含 .. 段的相对路径同样必须被安全闸拦下
    let err = git_discard_file_changes(
        Some(dir.to_string_lossy().to_string()),
        "../victim.txt".into(),
    )
    .unwrap_err();
    assert!(err.contains(".."), "错误信息应指向 .. 段: {err}");
    assert!(victim.exists(), ".. 逃逸路径绝不能被删除");

    let _ = std::fs::remove_dir_all(&outside_dir);
    let _ = std::fs::remove_dir_all(&dir);
}
