//! Git 右键菜单功能测试：每个菜单项背后的命令在真实仓库上跑一遍。
//!
//! 菜单 → 命令对照（实现位置见各测试）：
//! - 项目树 Git 子菜单 → `git_add` / `git_discard_file_changes` / `git_file_history` /
//!   `git_blame` / `git_diff` / `git_commit_patch` / `git_diff_patch` /
//!   `git_checkout_file_revision`（`ProjectTreeContextMenu.tsx` + `App.tsx`）
//! - 变更抽屉菜单 → `git_commit(paths)` / `git_diff_patch_paths` + `git_shelve_save` /
//!   `git_apply_shelf` / `git_list_shelves` / `git_delete_shelf`（`CommitDrawer.tsx`）
//! - 分支菜单 → `git_checkout`（含远程 DWIM） / `git_create_branch` / `git_rename_branch` /
//!   `git_delete_branch` / `git_merge` / `git_rebase` / `git_push` / `git_pull` /
//!   `git_fetch` / `git_compare_refs`（`BranchContextMenu.tsx`）
//! - 提交菜单 → `git_cherry_pick` / `git_revert_commit` / `git_reset` / `git_amend_message` /
//!   `git_drop_commit` / `git_commit_patch` / `git_create_tag` / `git_rebase_interactive`
//!   （`CommitContextMenu.tsx`）
//! - 冲突对话框 → `git_merge_file_content` / `git_use_merge_side` / `git_stage_file`
//! - Stash 对话框 → `git_stash_push` / `git_stash_list` / `git_stash_files` /
//!   `git_stash_apply` / `git_stash_drop`
//!
//! 全部 hermetic：每个用例建独立 temp 仓库（`temp_dir/lazecode-gitmenu-<pid>-<用例>`），
//! 并行跑也不串。git 全局配置用空文件隔离，机器上的 `commit.gpgsign` 等不影响结果。

use crate::commands::*;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

static INIT: Once = Once::new();

fn init_env() {
    INIT.call_once(|| {
        // 后端 spawn 的 git 继承测试进程环境：禁掉一切交互与机器全局配置
        std::env::set_var("GIT_TERMINAL_PROMPT", "0");
        std::env::set_var("GIT_EDITOR", "true");
        std::env::set_var("GIT_MERGE_AUTOEDIT", "no");
        let dir = std::env::temp_dir().join(format!("lazecode-gitmenu-env-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let empty = dir.join("empty.gitconfig");
        let _ = std::fs::write(&empty, "");
        std::env::set_var("GIT_CONFIG_GLOBAL", &empty);
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
    });
}

/// bare 远端不走这条（`git init --bare` 不需要身份），其余一律带 `-c` 身份。
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=LazeCode Test",
            "-c",
            "user.email=lazecode@test",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn fresh_dir(name: &str) -> PathBuf {
    init_env();
    let d = std::env::temp_dir().join(format!("lazecode-gitmenu-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn w(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).unwrap();
    }
    std::fs::write(p, content).unwrap();
}

fn r(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn repo(d: &Path) -> Option<String> {
    Some(d.to_string_lossy().into_owned())
}

fn block_on<F: Future>(f: F) -> F::Output {
    tauri::async_runtime::block_on(f)
}

/// 两笔提交的基线仓库：a.txt 改过一次，sub/b.txt 一次到位。
fn basic_repo(name: &str) -> PathBuf {
    let d = fresh_dir(name);
    git(&d, &["init", "-b", "main"]);
    w(&d, "a.txt", "line1\nline2\n");
    w(&d, "sub/b.txt", "b1\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "init"]);
    w(&d, "a.txt", "line1\nline2\nline3\n");
    git(&d, &["commit", "-am", "second"]);
    d
}

fn head_hash(d: &Path) -> String {
    git(d, &["rev-parse", "HEAD"]).trim().to_string()
}

fn status_of(d: &Path, rel: &str) -> Option<GitChangedFileDto> {
    block_on(git_status(repo(d)))
        .expect("git_status")
        .into_iter()
        .find(|f| f.path == rel)
}

// ── 项目树 Git 子菜单 ─────────────────────────────────────────────

#[test]
fn tree_add_to_vcs_stages_untracked_with_absolute_path() {
    // 前端项目树传的是绝对路径（`onAddToVcs={p => handleAddToVcs([p])}`）
    let d = basic_repo("tree-add");
    let abs = d.join("new-file.txt");
    w(&d, "new-file.txt", "hello\n");
    git_add(repo(&d), vec![abs.to_string_lossy().into_owned()]).expect("git_add");
    let st = status_of(&d, "new-file.txt").expect("应出现在 status 里");
    assert_eq!(st.status, "A");
    assert!(st.is_staged, "add 后应进暂存区");
}

#[test]
fn tree_rollback_file_restores_tracked() {
    // 变更抽屉传仓库相对路径，项目树传绝对路径：两者都要能回滚
    for (name, path_fn) in [
        (
            "rel",
            Box::new(|_d: &Path| "a.txt".to_string()) as Box<dyn Fn(&Path) -> String>,
        ),
        (
            "abs",
            Box::new(|d: &Path| d.join("a.txt").to_string_lossy().into_owned()),
        ),
    ] {
        let d = basic_repo(&format!("tree-rb-{name}"));
        w(&d, "a.txt", "TAMPERED\n");
        git_discard_file_changes(repo(&d), path_fn(&d)).expect("rollback");
        assert_eq!(r(&d, "a.txt"), "line1\nline2\nline3\n");
        assert!(status_of(&d, "a.txt").is_none(), "回滚后应干净");
    }
}

#[test]
fn tree_rollback_removes_untracked_and_staged_new() {
    let d = basic_repo("tree-rb-new");
    // 未跟踪文件：直接删盘
    w(&d, "scratch.txt", "x\n");
    git_discard_file_changes(repo(&d), "scratch.txt".to_string()).expect("rollback untracked");
    assert!(!d.join("scratch.txt").exists());
    // 已暂存的新文件：去暂存 + 删盘
    w(&d, "staged-new.txt", "y\n");
    git(&d, &["add", "staged-new.txt"]);
    git_discard_file_changes(repo(&d), "staged-new.txt".to_string()).expect("rollback staged-new");
    assert!(!d.join("staged-new.txt").exists());
    assert!(status_of(&d, "staged-new.txt").is_none());
}

#[test]
fn tree_rollback_dir_cleans_subtree() {
    // 前端目录回滚传绝对路径
    let d = basic_repo("tree-rb-dir");
    w(&d, "sub/b.txt", "MODIFIED\n");
    w(&d, "sub/extra-untracked.txt", "u\n");
    git_discard_file_changes(repo(&d), d.join("sub").to_string_lossy().into_owned())
        .expect("rollback dir");
    assert_eq!(r(&d, "sub/b.txt"), "b1\n");
    assert!(!d.join("sub/extra-untracked.txt").exists());
}

#[test]
fn tree_show_diff_and_history_and_blame() {
    let d = basic_repo("tree-info");
    w(&d, "a.txt", "line1\nline2\nline3\nline4\n");
    let abs = d.join("a.txt").to_string_lossy().into_owned();
    // Show Diff（工作区）：行数 > 0
    let diff = block_on(git_diff(repo(&d), abs.clone(), Some(false))).expect("git_diff");
    assert!(!diff.lines.is_empty(), "工作区 diff 应有内容");
    // 已暂存侧
    git(&d, &["add", "a.txt"]);
    let staged = block_on(git_diff(repo(&d), abs.clone(), Some(true))).expect("staged diff");
    assert!(!staged.lines.is_empty());
    // Show History：a.txt 有两笔提交（--follow）
    let hist = git_file_history(repo(&d), abs.clone(), Some(200)).expect("history");
    assert!(hist.len() >= 2, "应有 2 笔，实际 {}", hist.len());
    // Annotate：行数与文件一致，每行都有作者
    let blame = block_on(git_blame(repo(&d), abs)).expect("blame");
    assert_eq!(blame.len(), 4);
    assert!(blame.iter().all(|l| !l.author.is_empty()));
}

#[test]
fn tree_compare_hunks_and_patches() {
    let d = basic_repo("tree-patch");
    w(&d, "a.txt", "line1\nline2\nline3\nline4\n");
    let hunks = block_on(git_file_change_hunks(repo(&d), "a.txt".to_string())).expect("hunks");
    assert!(!hunks.is_empty(), "应有 hunk");
    let patch = git_diff_patch(repo(&d), Some("a.txt".to_string()), None).expect("local patch");
    assert!(patch.contains("line4"), "本地补丁应含新增行");
    let hash = head_hash(&d);
    let cp = git_commit_patch(repo(&d), hash).expect("commit patch");
    assert!(cp.contains("line3"), "提交补丁应含 second 的改动");
}

#[test]
fn tree_get_from_revision_overwrites_worktree() {
    let d = basic_repo("tree-getrev");
    w(&d, "a.txt", "TAMPERED\n");
    let hash = head_hash(&d);
    git_checkout_file_revision(repo(&d), hash, "a.txt".to_string()).expect("get from rev");
    assert_eq!(r(&d, "a.txt"), "line1\nline2\nline3\n");
}

// ── 变更抽屉：提交 / Shelve ──────────────────────────────────────

#[test]
fn drawer_commit_single_file_only() {
    // CommitDrawer 传仓库相对路径；只提交勾选文件
    let d = basic_repo("drawer-commit");
    w(&d, "a.txt", "line1\nline2\nline3\nAAA\n");
    w(&d, "sub/b.txt", "b1\nBBB\n");
    git_commit(
        repo(&d),
        "only a".to_string(),
        false,
        Some(vec!["a.txt".to_string()]),
    )
    .expect("partial commit");
    let log = git(&d, &["log", "--oneline"]);
    assert!(log.contains("only a"));
    assert!(status_of(&d, "sub/b.txt").is_some(), "b.txt 应保持未提交");
    assert!(status_of(&d, "a.txt").is_none());
}

#[test]
fn drawer_shelve_save_apply_delete_roundtrip() {
    let d = basic_repo("drawer-shelve");
    w(&d, "a.txt", "line1\nline2\nline3\nSHELVED\n");
    // Shelve Changes：先取补丁再存（与 handleShelve 一致）
    let patch = git_diff_patch_paths(repo(&d), vec!["a.txt".to_string()]).expect("paths patch");
    assert!(patch.contains("SHELVED"));
    git_shelve_save(repo(&d), "test-shelf".to_string(), patch).expect("shelve save");
    git_discard_file_changes(repo(&d), "a.txt".to_string()).expect("restore after shelve");
    assert!(status_of(&d, "a.txt").is_none());
    let shelves = git_list_shelves(repo(&d)).expect("list");
    assert!(shelves.iter().any(|s| s.name == "test-shelf"));
    git_apply_shelf(repo(&d), "test-shelf".to_string()).expect("unshelve");
    assert!(r(&d, "a.txt").contains("SHELVED"), "unshelve 应恢复内容");
    git_delete_shelf(repo(&d), "test-shelf".to_string()).expect("delete shelf");
    assert!(git_list_shelves(repo(&d)).expect("relist").is_empty());
}

// ── 分支菜单 ─────────────────────────────────────────────────────

#[test]
fn branch_create_rename_checkout() {
    let d = basic_repo("branch-crud");
    // New Branch from HEAD~1（startPoint）
    let parent = git(&d, &["rev-parse", "HEAD~1"]).trim().to_string();
    git_create_branch(repo(&d), "old-name".to_string(), Some(parent)).expect("create");
    git_rename_branch(repo(&d), "old-name".to_string(), "new-name".to_string()).expect("rename");
    let branches = block_on(git_branches(repo(&d))).expect("branches");
    assert!(branches
        .iter()
        .any(|b| b.name == "new-name" && !b.is_remote));
    assert!(!branches.iter().any(|b| b.name == "old-name"));
    git_checkout(repo(&d), "new-name".to_string()).expect("checkout");
    let cur = block_on(git_branches(repo(&d))).expect("relist");
    assert!(cur.iter().any(|b| b.name == "new-name" && b.is_current));
    // old-name 从 second 之前建：a.txt 只有两行
    assert_eq!(r(&d, "a.txt"), "line1\nline2\n");
}

#[test]
fn branch_delete_merged_then_force_for_unmerged() {
    let d = basic_repo("branch-del");
    git(&d, &["checkout", "-b", "merged-ok"]);
    w(&d, "m.txt", "m\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "m"]);
    git(&d, &["checkout", "main"]);
    git(&d, &["merge", "--no-edit", "merged-ok"]);
    git(&d, &["checkout", "-b", "unmerged"]);
    w(&d, "u.txt", "u\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "u"]);
    git(&d, &["checkout", "main"]);
    // 已合入：一次成功
    block_on(git_delete_branch(
        repo(&d),
        "merged-ok".to_string(),
        Some(false),
        Some(false),
    ))
    .expect("delete merged");
    // 未合入：先失败（前端据此弹二次确认），再强删
    let err = block_on(git_delete_branch(
        repo(&d),
        "unmerged".to_string(),
        Some(false),
        Some(false),
    ))
    .expect_err("未合入应拒绝");
    assert!(
        err.contains("not fully merged"),
        "应提示未合入，实际: {err}"
    );
    block_on(git_delete_branch(
        repo(&d),
        "unmerged".to_string(),
        Some(false),
        Some(true),
    ))
    .expect("force delete");
}

#[test]
fn branch_checkout_remote_creates_tracking_and_delete_remote() {
    let d = basic_repo("branch-remote");
    let bare = fresh_dir("branch-remote-bare");
    git(&bare, &["init", "--bare", "-b", "main"]);
    git(&d, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&d, &["checkout", "-b", "remote-only"]);
    w(&d, "ro.txt", "ro\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "ro"]);
    git(&d, &["push", "-u", "origin", "remote-only"]);
    git(&d, &["checkout", "main"]);
    git(&d, &["branch", "-D", "remote-only"]);
    git(&d, &["fetch", "origin"]);
    // Checkout 远程分支：应建本地跟踪分支而非 detached
    git_checkout(repo(&d), "origin/remote-only".to_string()).expect("checkout remote");
    let head_ref = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    assert_eq!(head_ref, "remote-only", "应落在新建的本地分支");
    assert!(d.join("ro.txt").exists());
    // Delete Remote Branch：远端引用消失
    git(&d, &["checkout", "main"]);
    git(&d, &["branch", "-D", "remote-only"]);
    block_on(git_delete_branch(
        repo(&d),
        "origin/remote-only".to_string(),
        Some(true),
        Some(false),
    ))
    .expect("delete remote");
    let ls = git(&d, &["ls-remote", "--heads", "origin"]);
    assert!(!ls.contains("remote-only"), "远端分支应已删除");
}

#[test]
fn branch_merge_clean_and_rebase_clean() {
    let d = basic_repo("merge-rebase");
    git(&d, &["checkout", "-b", "feat"]);
    w(&d, "feat.txt", "f\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "feat"]);
    git(&d, &["checkout", "main"]);
    git_merge(repo(&d), "feat".to_string()).expect("merge");
    assert!(d.join("feat.txt").exists());
    // rebase：feat2 基老 main，main 前进后变基
    git(&d, &["checkout", "-b", "feat2", "HEAD~1"]);
    w(&d, "feat2.txt", "f2\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "feat2"]);
    git_rebase(repo(&d), "main".to_string()).expect("rebase");
    assert!(d.join("feat.txt").exists(), "变基后应含 main 的推进");
    assert!(d.join("feat2.txt").exists());
}

#[test]
fn branch_merge_conflict_then_dialog_cmds_resolve() {
    let d = basic_repo("merge-conflict");
    git(&d, &["checkout", "-b", "side-a"]);
    w(&d, "a.txt", "line1\nA-SIDE\nline3\n");
    git(&d, &["commit", "-am", "side a"]);
    git(&d, &["checkout", "main"]);
    git(&d, &["checkout", "-b", "side-b"]);
    w(&d, "a.txt", "line1\nB-SIDE\nline3\n");
    git(&d, &["commit", "-am", "side b"]);
    git(&d, &["checkout", "side-a"]);
    // Merge：冲突失败（前端据 CONFLICT 弹冲突提示）
    let err = git_merge(repo(&d), "side-b".to_string()).expect_err("应冲突");
    assert!(err.contains("CONFLICT"), "实际: {err}");
    // 冲突对话框三件套
    let content =
        block_on(git_merge_file_content(repo(&d), "a.txt".to_string())).expect("merge content");
    assert!(content.base.is_some(), "有共同祖先应有 base");
    assert!(content.ours.contains("A-SIDE") && content.theirs.contains("B-SIDE"));
    assert!(content.worktree.contains("<<<<<<<"), "工作区应有冲突标记");
    block_on(git_use_merge_side(
        repo(&d),
        "a.txt".to_string(),
        "side".to_string(),
    ))
    .expect_err("非法 side");
    block_on(git_use_merge_side(
        repo(&d),
        "a.txt".to_string(),
        "ours".to_string(),
    ))
    .expect("use ours");
    assert_eq!(r(&d, "a.txt"), "line1\nA-SIDE\nline3\n");
    block_on(git_stage_file(repo(&d), "a.txt".to_string())).expect("stage resolved");
    git(&d, &["commit", "-m", "resolve with ours"]);
    assert!(block_on(git_status(repo(&d))).expect("status").is_empty());
}

#[test]
fn branch_compare_refs_and_commit_files() {
    let d = basic_repo("compare");
    git(&d, &["checkout", "-b", "feat"]);
    w(&d, "feat.txt", "f\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "feat"]);
    git(&d, &["checkout", "main"]);
    // Compare with Current（ref 独有提交 + 文件）
    let cmp =
        block_on(git_compare_refs(repo(&d), "feat".to_string(), Some(false))).expect("compare");
    assert!(!cmp.files.is_empty(), "应列出 feat 独有文件");
    // Show Diff with Working Tree
    let wt = block_on(git_compare_refs(repo(&d), "feat".to_string(), Some(true))).expect("wt");
    assert!(!wt.files.is_empty());
    // 提交详情文件清单 / show / 原文
    let hash = git(&d, &["rev-parse", "feat"]).trim().to_string();
    let files = block_on(git_commit_files(repo(&d), hash.clone())).expect("commit files");
    assert!(files.iter().any(|f| f.path == "feat.txt"));
    let show = block_on(git_show(
        repo(&d),
        hash.clone(),
        Some("feat.txt".to_string()),
    ))
    .expect("show file");
    assert!(!show.lines.is_empty());
    let text = block_on(git_file_at_rev(repo(&d), hash, "feat.txt".to_string())).expect("at rev");
    assert_eq!(text, "f\n");
}

#[test]
fn branch_push_fetch_pull_against_local_remote() {
    let d = basic_repo("net");
    let bare = fresh_dir("net-bare");
    git(&bare, &["init", "--bare", "-b", "main"]);
    git(&d, &["remote", "add", "origin", bare.to_str().unwrap()]);
    // Push 对话框：推 main 并建上游
    block_on(git_push(
        repo(&d),
        Some("origin".to_string()),
        Some("main".to_string()),
        Some(true),
        Some(false),
        None,
    ))
    .expect("push");
    assert!(git(&d, &["ls-remote", "--heads", "origin"]).contains("refs/heads/main"));
    // 远端前进（模拟他人推送）：第二克隆推一笔
    let other = fresh_dir("net-other");
    git(&other, &["clone", bare.to_str().unwrap(), "."]);
    w(&other, "teammate.txt", "t\n");
    git(&other, &["add", "."]);
    git(&other, &["commit", "-m", "teammate"]);
    git(&other, &["push", "origin", "HEAD:main"]);
    // Fetch：能看到但不合入
    block_on(git_fetch(repo(&d), Some("origin".to_string()))).expect("fetch");
    assert!(!d.join("teammate.txt").exists(), "fetch 不应改工作区");
    // Update Project（merge 策略）：合入后文件落地
    block_on(git_pull(
        repo(&d),
        Some("origin".to_string()),
        Some("main".to_string()),
        Some(false),
    ))
    .expect("pull");
    assert_eq!(r(&d, "teammate.txt"), "t\n");
}

// ── 提交菜单 ─────────────────────────────────────────────────────

#[test]
fn commit_cherry_pick_clean_and_conflict() {
    let d = basic_repo("cherry");
    git(&d, &["checkout", "-b", "donor"]);
    w(&d, "donor.txt", "d\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "donor change"]);
    let donor_hash = head_hash(&d);
    git(&d, &["checkout", "main"]);
    git_cherry_pick(repo(&d), donor_hash).expect("cherry-pick");
    assert!(d.join("donor.txt").exists());
    // 冲突版：同一行各改一边
    git(&d, &["checkout", "-b", "c-a"]);
    w(&d, "a.txt", "line1\nCA\nline3\n");
    git(&d, &["commit", "-am", "ca"]);
    let ca_hash = head_hash(&d);
    git(&d, &["checkout", "main"]);
    git(&d, &["checkout", "-b", "c-b"]);
    w(&d, "a.txt", "line1\nCB\nline3\n");
    git(&d, &["commit", "-am", "cb"]);
    let err = git_cherry_pick(repo(&d), ca_hash).expect_err("应冲突");
    assert!(err.contains("CONFLICT"), "实际: {err}");
    git(&d, &["cherry-pick", "--abort"]);
}

#[test]
fn commit_revert_and_checkout_revision() {
    let d = basic_repo("revert");
    w(&d, "gone.txt", "g\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "add gone"]);
    let hash = head_hash(&d);
    git_revert_commit(repo(&d), hash).expect("revert");
    assert!(!d.join("gone.txt").exists(), "revert 应删除该文件");
    assert!(git(&d, &["log", "--oneline"]).contains("Revert"));
    // Checkout Revision（detached HEAD）
    let first = git(&d, &["rev-parse", "HEAD~2"]).trim().to_string();
    git_checkout(repo(&d), first).expect("checkout rev");
    assert_eq!(
        git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "HEAD"
    );
}

#[test]
fn commit_reset_modes_and_undo_pattern() {
    // soft：提交撤销、改动留暂存区（Undo Commit 语义）
    let d = basic_repo("reset-soft");
    w(&d, "u.txt", "u\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "to undo"]);
    git_reset(repo(&d), "HEAD~1".to_string(), Some("soft".to_string())).expect("undo");
    assert!(!git(&d, &["log", "--oneline"]).contains("to undo"));
    let st = block_on(git_status(repo(&d))).expect("status");
    assert!(
        st.iter().any(|f| f.path == "u.txt" && f.is_staged),
        "soft 后应留暂存"
    );
    // mixed：落回工作区；hard：丢弃
    git_reset(repo(&d), "HEAD".to_string(), Some("mixed".to_string())).expect("mixed");
    let st = block_on(git_status(repo(&d))).expect("status2");
    assert!(st.iter().any(|f| f.path == "u.txt" && !f.is_staged));
    // hard：已跟踪修改被丢弃；未跟踪文件按 git 语义保留（PyCharm 的 Hard 同此行为）
    w(&d, "a.txt", "HARD-TAMPER\n");
    git_reset(repo(&d), "HEAD".to_string(), Some("hard".to_string())).expect("hard");
    assert_eq!(r(&d, "a.txt"), "line1\nline2\nline3\n");
    assert!(d.join("u.txt").exists(), "未跟踪文件不受 --hard 影响");
}

#[test]
fn commit_amend_message() {
    let d = basic_repo("amend");
    git_amend_message(repo(&d), "rewritten message".to_string()).expect("amend");
    assert!(git(&d, &["log", "-1", "--format=%s"]).contains("rewritten message"));
}

#[test]
fn commit_drop_middle_and_head_and_rejections() {
    let d = basic_repo("drop");
    for (f, msg) in [
        ("one.txt", "c-one"),
        ("two.txt", "c-two"),
        ("three.txt", "c-three"),
    ] {
        w(&d, f, "x\n");
        git(&d, &["add", "."]);
        git(&d, &["commit", "-m", msg]);
    }
    let two = git(&d, &["rev-parse", "HEAD~1"]).trim().to_string();
    // Drop 中间提交：它的文件消失，两侧保留
    git_drop_commit(repo(&d), two, None).expect("drop middle");
    assert!(!d.join("two.txt").exists());
    assert!(d.join("one.txt").exists() && d.join("three.txt").exists());
    // Drop HEAD：走 reset --hard 路径
    let head = head_hash(&d);
    git_drop_commit(repo(&d), head.clone(), None).expect("drop head");
    assert!(!d.join("three.txt").exists());
    // 非法哈希与合并提交：拒绝
    git_drop_commit(repo(&d), "zzzz".to_string(), None).expect_err("非法哈希");
    git(&d, &["checkout", "-b", "m1"]);
    w(&d, "m1.txt", "1\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "m1"]);
    git(&d, &["checkout", "main"]);
    git(&d, &["merge", "--no-ff", "--no-edit", "m1"]);
    let merge_hash = head_hash(&d);
    git_drop_commit(repo(&d), merge_hash, None).expect_err("合并提交不能摘");
}

#[test]
fn commit_new_tag_light_and_annotated() {
    let d = basic_repo("tag");
    git_create_tag(repo(&d), "v-light".to_string(), None, None).expect("light tag");
    git_create_tag(
        repo(&d),
        "v-note".to_string(),
        Some(head_hash(&d)),
        Some("release note".to_string()),
    )
    .expect("annotated tag");
    let tags = git(&d, &["tag", "-l"]);
    assert!(tags.contains("v-light") && tags.contains("v-note"));
}

#[test]
fn commit_interactive_rebase_reorders() {
    let d = basic_repo("rebase-i");
    for (f, msg) in [("ra.txt", "r-a"), ("rb.txt", "r-b")] {
        w(&d, f, "x\n");
        git(&d, &["add", "."]);
        git(&d, &["commit", "-m", msg]);
    }
    let ha = git(&d, &["rev-parse", "HEAD~1"]).trim().to_string();
    let hb = head_hash(&d);
    let base = git(&d, &["rev-parse", "HEAD~2"]).trim().to_string();
    // 交换两笔提交顺序（与交互式变基对话框「上移/下移」一致）
    git_rebase_interactive(
        repo(&d),
        base,
        vec![format!("pick {hb}"), format!("pick {ha}")],
    )
    .expect("reorder");
    let msgs = git(&d, &["log", "--format=%s", "-2"]);
    let lines: Vec<&str> = msgs.lines().collect();
    assert_eq!(lines, vec!["r-a", "r-b"], "顺序应对调，实际: {lines:?}");
    // 注入行：exec 必须被拒绝
    git_rebase_interactive(
        repo(&d),
        "HEAD~1".to_string(),
        vec!["exec echo pwned".to_string()],
    )
    .expect_err("exec 应拒绝");
}

// ── Stash 对话框 ─────────────────────────────────────────────────

#[test]
fn stash_push_list_files_apply_drop() {
    let d = basic_repo("stash");
    w(&d, "a.txt", "line1\nline2\nline3\nSTASHED\n");
    w(&d, "untracked-s.txt", "u\n");
    // Stash Dialog：含未跟踪
    git_stash_push(repo(&d), Some("test stash".to_string()), Some(true)).expect("stash push");
    assert!(block_on(git_status(repo(&d))).expect("clean").is_empty());
    let list = git_stash_list(repo(&d)).expect("list");
    assert_eq!(list.len(), 1);
    assert!(list[0].message.contains("test stash"));
    let files = block_on(git_stash_files(repo(&d), "stash@{0}".to_string())).expect("files");
    assert!(files.iter().any(|f| f.path == "a.txt"));
    assert!(files.iter().any(|f| f.path == "untracked-s.txt"));
    // Unstash：apply 后内容回来，drop 后清空
    git_stash_apply(repo(&d), "stash@{0}".to_string(), Some(false)).expect("apply");
    assert!(r(&d, "a.txt").contains("STASHED"));
    git_stash_drop(repo(&d), "stash@{0}".to_string()).expect("drop");
    assert!(git_stash_list(repo(&d)).expect("relist").is_empty());
}

// ── 面板基座：Log / 分支 / 仓库信息 / 初始化 ─────────────────────

#[test]
fn panels_log_branches_repo_info() {
    let d = basic_repo("panels");
    let log = block_on(git_log(
        repo(&d),
        Some(1),
        Some(0),
        Some(true),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
    .expect("log");
    assert_eq!(log.len(), 1);
    let page2 = block_on(git_log(
        repo(&d),
        Some(10),
        Some(1),
        Some(true),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
    .expect("p2");
    assert!(
        page2.iter().any(|c| c.message == "init"),
        "skip=1 应翻到 init"
    );
    let pending = block_on(git_pending_commits(repo(&d), Some(50))).expect("pending");
    assert!(!pending.is_empty());
    let branches = block_on(git_branches(repo(&d))).expect("branches");
    assert!(branches
        .iter()
        .any(|b| b.name == "main" && b.is_current && !b.is_remote));
    let info = block_on(git_repo_info(repo(&d))).expect("info");
    assert_eq!(info.branch, "main");
    assert!(info.root.ends_with("panels"));
    let remotes = git_remote_list(repo(&d)).expect("remotes");
    assert!(remotes.is_empty());
}

#[test]
fn panels_init_repo_on_empty_dir() {
    let d = fresh_dir("initrepo");
    git_init(repo(&d)).expect("init");
    assert!(d.join(".git").is_dir());
    assert!(block_on(git_status(repo(&d))).expect("status").is_empty());
}

// ── 远程管理（RemotesDialog.tsx → Git 菜单 Manage Remotes） ────────────────

#[test]
fn remotes_add_list_seturl_rename_remove_roundtrip() {
    let d = basic_repo("remotes-crud");
    assert!(git_remote_list(repo(&d)).expect("list").is_empty());
    // add ×2
    git_remote_add(
        repo(&d),
        "origin".to_string(),
        "https://example.com/a.git".to_string(),
    )
    .expect("add origin");
    git_remote_add(
        repo(&d),
        "upstream".to_string(),
        "git@example.com:b.git".to_string(),
    )
    .expect("add upstream");
    let remotes = git_remote_list(repo(&d)).expect("relist");
    assert_eq!(remotes.len(), 2);
    assert!(remotes
        .iter()
        .any(|r| r.name == "origin" && r.url == "https://example.com/a.git"));
    // 重复 add 同名：git 原生拒绝
    git_remote_add(
        repo(&d),
        "origin".to_string(),
        "https://example.com/other.git".to_string(),
    )
    .expect_err("同名应拒绝");
    // set-url
    git_remote_set_url(
        repo(&d),
        "origin".to_string(),
        "https://example.com/a2.git".to_string(),
    )
    .expect("set-url");
    let relist = git_remote_list(repo(&d)).expect("relist2");
    assert!(relist
        .iter()
        .any(|r| r.name == "origin" && r.url == "https://example.com/a2.git"));
    // rename（含跟踪引用改写不断言，只看列表）
    git_remote_rename(repo(&d), "upstream".to_string(), "mirror".to_string()).expect("rename");
    let relist = git_remote_list(repo(&d)).expect("relist3");
    assert!(relist
        .iter()
        .any(|r| r.name == "mirror" && r.url == "git@example.com:b.git"));
    assert!(!relist.iter().any(|r| r.name == "upstream"));
    // remove（含 origin 也允许删，删的是本地配置）
    git_remote_remove(repo(&d), "origin".to_string()).expect("remove origin");
    let relist = git_remote_list(repo(&d)).expect("relist4");
    assert_eq!(relist.len(), 1);
    assert_eq!(relist[0].name, "mirror");
    // 删不存在的：git 原生拒绝
    git_remote_remove(repo(&d), "nope".to_string()).expect_err("删不存在应失败");
    // set-url 到不存在的：git 原生拒绝
    git_remote_set_url(
        repo(&d),
        "nope".to_string(),
        "https://example.com/x.git".to_string(),
    )
    .expect_err("set-url 不存在应失败");
}

#[test]
fn remotes_reject_bad_names_urls_and_mask_token_in_errors() {
    let d = basic_repo("remotes-validate");
    // 空名 / 空 URL
    git_remote_add(
        repo(&d),
        "  ".to_string(),
        "https://example.com/a.git".to_string(),
    )
    .expect_err("空名应拒绝");
    git_remote_add(repo(&d), "origin".to_string(), "   ".to_string()).expect_err("空 URL 应拒绝");
    // 前导横杠（选项注入）
    git_remote_add(
        repo(&d),
        "--upload-pack=touch /tmp/pwned".to_string(),
        "https://example.com/a.git".to_string(),
    )
    .expect_err("前导横杠应拒绝");
    // 空白与 ref 通配符
    for bad in [
        "my remote",
        "a:b",
        "a?b",
        "a*b",
        "a[b",
        "a\\b",
        "@",
        "/lead",
        ".lead",
        "a..b",
    ] {
        git_remote_add(
            repo(&d),
            bad.to_string(),
            "https://example.com/a.git".to_string(),
        )
        .expect_err(&format!("远程名 {bad:?} 应拒绝"));
    }
    // rename 新名同样校验
    git_remote_add(
        repo(&d),
        "origin".to_string(),
        "https://example.com/a.git".to_string(),
    )
    .expect("add");
    git_remote_rename(repo(&d), "origin".to_string(), "has space".to_string())
        .expect_err("rename 坏名应拒绝");
    git_remote_set_url(
        repo(&d),
        "origin".to_string(),
        "not a url\nwith newline".to_string(),
    )
    .expect_err("含换行 URL 应拒绝");
    // 带 token 的 URL 触发我们自己的校验失败时，错误回显必须脱敏
    let err = git_remote_set_url(
        repo(&d),
        "origin".to_string(),
        "https://user:ghp_secrettoken123@example.com/a\ngit".to_string(),
    )
    .expect_err("含换行应拒绝");
    assert!(
        !err.contains("ghp_secrettoken123"),
        "错误回显不应泄露 token，实际: {err}"
    );
}

// ── 冲突解决（ConflictDialog.tsx）：删改冲突分级 + 缺 stage 的采用 ────────
// PyCharm 对 DU/UD 不开三窗格，只给"保留删除 / 保留修改"文件级选项。

/// 造一个 modify/delete 冲突：del_side 删除 f.txt，mod_side 修改 f.txt，
/// 最后在 main 上 merge，停在未解决态。返回仓库目录。
fn moddel_repo(name: &str, delete_on_main: bool) -> PathBuf {
    let d = fresh_dir(name);
    git(&d, &["init", "-b", "main"]);
    w(&d, "f.txt", "base\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "side"]);
    w(&d, "f.txt", "modified\n");
    git(&d, &["commit", "-am", "mod"]);
    git(&d, &["checkout", "-q", "main"]);
    if delete_on_main {
        git(&d, &["rm", "-q", "f.txt"]);
        git(&d, &["commit", "-qm", "del"]);
    } else {
        w(&d, "f.txt", "ours-mod\n");
        git(&d, &["commit", "-am", "ours"]);
    }
    // 合并必冲突：输出断言在调用方（DU vs UD 的 stderr 不同）
    let out = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args([
            "-c",
            "user.name=LazeCode Test",
            "-c",
            "user.email=lazecode@test",
            "-c",
            "commit.gpgsign=false",
            "merge",
            "side",
        ])
        .output()
        .expect("spawn merge");
    assert!(!out.status.success(), "merge 应冲突暂停");
    d
}

#[test]
fn conflict_deleted_by_us_kind_and_both_resolutions() {
    // DU：我方删除、对方修改 → stages 1+3，无 stage 2
    let d = moddel_repo("conf-du", true);
    let c = block_on(git_merge_file_content(repo(&d), "f.txt".to_string())).expect("content");
    assert_eq!(c.kind, "deleted_by_us");
    assert!(c.ours.is_empty(), "被删方无内容");
    assert!(c.theirs.contains("modified"), "存活方是对方修改版");
    assert!(c.base.as_deref().unwrap_or("").contains("base"));
    // Accept Theirs（保留修改版）：恢复 + 暂存，即解决（HEAD 本无此文件，
    // 解决后呈暂存新增 A，而非干净 —— "干净"只适用于内容与 HEAD 一致时）
    block_on(git_use_merge_side(
        repo(&d),
        "f.txt".to_string(),
        "theirs".to_string(),
    ))
    .expect("accept theirs");
    assert_eq!(r(&d, "f.txt"), "modified\n");
    let st = status_of(&d, "f.txt").expect("解决后应有暂存条目");
    assert_ne!(st.status, "C", "不应再是冲突态");
    assert!(st.is_staged, "解决结果应已暂存");

    // 重建，再 Accept Ours（确认删除）：文件消失 + 暂存，即解决
    let d = moddel_repo("conf-du-ours", true);
    block_on(git_use_merge_side(
        repo(&d),
        "f.txt".to_string(),
        "ours".to_string(),
    ))
    .expect("accept ours");
    assert!(!d.join("f.txt").exists(), "确认删除后工作区不应有文件");
    assert!(status_of(&d, "f.txt").is_none(), "解决后 status 应干净");
}

#[test]
fn conflict_deleted_by_them_kind_and_both_resolutions() {
    // UD：对方删除、我方修改 → stages 1+2，无 stage 3
    let e = fresh_dir("conf-ud2");
    git(&e, &["init", "-b", "main"]);
    w(&e, "f.txt", "base\n");
    git(&e, &["add", "."]);
    git(&e, &["commit", "-m", "base"]);
    git(&e, &["checkout", "-qb", "side"]);
    git(&e, &["rm", "-q", "f.txt"]);
    git(&e, &["commit", "-qm", "del"]);
    git(&e, &["checkout", "-q", "main"]);
    w(&e, "f.txt", "ours-mod\n");
    git(&e, &["commit", "-am", "ours"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(&e)
        .args([
            "-c",
            "user.name=LazeCode Test",
            "-c",
            "user.email=lazecode@test",
            "-c",
            "commit.gpgsign=false",
            "merge",
            "side",
        ])
        .output()
        .expect("spawn merge");
    assert!(!out.status.success(), "merge 应冲突暂停");

    let c = block_on(git_merge_file_content(repo(&e), "f.txt".to_string())).expect("content");
    assert_eq!(c.kind, "deleted_by_them");
    assert!(c.theirs.is_empty(), "被删方无内容");
    assert!(c.ours.contains("ours-mod"), "存活方是我方修改版");
    // Accept Ours（保留我方修改）：工作区已有 + 暂存，即解决
    block_on(git_use_merge_side(
        repo(&e),
        "f.txt".to_string(),
        "ours".to_string(),
    ))
    .expect("accept ours");
    assert_eq!(r(&e, "f.txt"), "ours-mod\n");
    assert!(status_of(&e, "f.txt").is_none());
    // 外部已解决后：kind=resolved，且再采用直接拒绝（绝不能 rm 正常文件）
    let c2 = block_on(git_merge_file_content(repo(&e), "f.txt".to_string())).expect("requery");
    assert_eq!(c2.kind, "resolved");
    block_on(git_use_merge_side(
        repo(&e),
        "f.txt".to_string(),
        "theirs".to_string(),
    ))
    .expect_err("已解决后采用应拒绝");
    assert_eq!(r(&e, "f.txt"), "ours-mod\n", "拒绝后文件必须原样保留");
}

#[test]
fn conflict_add_add_is_content_without_base() {
    // AA：双方各新增同名不同内容 → stages 2+3，无 base
    let d = fresh_dir("conf-aa");
    git(&d, &["init", "-b", "main"]);
    w(&d, "base.txt", "x\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "side"]);
    w(&d, "f.txt", "side version\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "side add"]);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "f.txt", "main version\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "main add"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args([
            "-c",
            "user.name=LazeCode Test",
            "-c",
            "user.email=lazecode@test",
            "-c",
            "commit.gpgsign=false",
            "merge",
            "side",
        ])
        .output()
        .expect("spawn merge");
    assert!(!out.status.success(), "merge 应冲突暂停");
    let c = block_on(git_merge_file_content(repo(&d), "f.txt".to_string())).expect("content");
    assert_eq!(c.kind, "content");
    assert!(c.base.is_none(), "add/add 无共同祖先");
    assert!(c.ours.contains("main version"));
    assert!(c.theirs.contains("side version"));
}

// ── sequencer：取消合并 / 捡选继续·放弃 ────────────────────────────────────

/// 带内容冲突的 merge 现场：main 与 side 改同一行，merge 停住。
fn conflicting_merge_repo(name: &str) -> PathBuf {
    let d = fresh_dir(name);
    git(&d, &["init", "-b", "main"]);
    w(&d, "f.txt", "line\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "side"]);
    w(&d, "f.txt", "side\n");
    git(&d, &["commit", "-am", "side"]);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "f.txt", "main\n");
    git(&d, &["commit", "-am", "main"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args([
            "-c",
            "user.name=LazeCode Test",
            "-c",
            "user.email=lazecode@test",
            "-c",
            "commit.gpgsign=false",
            "merge",
            "side",
        ])
        .output()
        .expect("spawn merge");
    assert!(!out.status.success(), "merge 应冲突暂停");
    d
}

#[test]
fn sequencer_merge_detect_and_abort_restores() {
    let d = conflicting_merge_repo("seq-merge");
    let st = git_sequencer_state(repo(&d)).expect("state");
    assert!(st.merging, "合并冲突中应 merging");
    assert!(!st.cherry_picking);
    let before = head_hash(&d);
    git_merge_abort(repo(&d)).expect("abort");
    let st = git_sequencer_state(repo(&d)).expect("state2");
    assert!(!st.merging && !st.cherry_picking && !st.rebasing);
    assert_eq!(head_hash(&d), before, "abort 不动 HEAD");
    assert_eq!(r(&d, "f.txt"), "main\n", "工作区回到合并前");
    assert!(status_of(&d, "f.txt").is_none());
    // 无合并在途再 abort：git 原文拒绝（前端不亮按钮，此为兜底）
    git_merge_abort(repo(&d)).expect_err("无合并应拒绝");
}

#[test]
fn sequencer_cherry_pick_conflict_continue_and_abort() {
    // 捡选冲突现场：feat 改的行 main 也改了
    let d = fresh_dir("seq-pick");
    git(&d, &["init", "-b", "main"]);
    w(&d, "f.txt", "line\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "feat"]);
    w(&d, "f.txt", "feat\n");
    git(&d, &["commit", "-am", "feat"]);
    let feat_hash = head_hash(&d);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "f.txt", "main\n");
    git(&d, &["commit", "-am", "main"]);
    let before = head_hash(&d);
    git_cherry_pick(repo(&d), feat_hash).expect_err("应冲突暂停");
    let st = git_sequencer_state(repo(&d)).expect("state");
    assert!(st.cherry_picking, "捡选冲突中应 cherryPicking");
    assert!(!st.merging);
    // 解决（采用对方）+ 暂存 → continue 落子
    block_on(git_use_merge_side(
        repo(&d),
        "f.txt".to_string(),
        "theirs".to_string(),
    ))
    .expect("accept");
    git_cherry_pick_continue(repo(&d)).expect("continue");
    let st = git_sequencer_state(repo(&d)).expect("state2");
    assert!(!st.cherry_picking, "continue 后 sequencer 应干净");
    assert_ne!(head_hash(&d), before, "continue 应产生新提交");
    assert_eq!(r(&d, "f.txt"), "feat\n");

    // 再来一次，这次 abort：HEAD 不动、工作区回捡选前
    git(&d, &["checkout", "-qb", "feat2"]);
    w(&d, "f.txt", "feat2\n");
    git(&d, &["commit", "-am", "feat2"]);
    let feat2_hash = head_hash(&d);
    git(&d, &["checkout", "-q", "main"]);
    git_cherry_pick(repo(&d), feat2_hash).expect("feat2 应干净捡入");
    // main 再改一行，让下一次捡选冲突
    w(&d, "f2.txt", "x\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "f2"]);
    git(&d, &["checkout", "-qb", "feat3"]);
    w(&d, "f.txt", "feat3\n");
    git(&d, &["commit", "-am", "feat3"]);
    let feat3_hash = head_hash(&d);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "f.txt", "main3\n");
    git(&d, &["commit", "-am", "main3"]);
    let before3 = head_hash(&d);
    git_cherry_pick(repo(&d), feat3_hash).expect_err("应冲突暂停");
    git_cherry_pick_abort(repo(&d)).expect("abort");
    assert_eq!(head_hash(&d), before3, "abort 不动 HEAD");
    assert_eq!(r(&d, "f.txt"), "main3\n", "工作区回捡选前");
    let st = git_sequencer_state(repo(&d)).expect("state3");
    assert!(!st.cherry_picking);
}

// ── Conflicts 对话框（PyCharm 式）：副标题 heads / 批量 kinds / 三路标记文本 ──

#[test]
fn sequencer_heads_feed_conflict_subtitle() {
    // 捡选冲突：副标题 "cherry-picking commit <h> made by <author>" 的数据
    let d = fresh_dir("seq-heads");
    git(&d, &["init", "-b", "main"]);
    w(&d, "f.txt", "line\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "feat"]);
    w(&d, "f.txt", "feat\n");
    git(&d, &["commit", "-am", "pick me"]);
    let feat_hash = head_hash(&d);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "f.txt", "main\n");
    git(&d, &["commit", "-am", "main"]);
    git_cherry_pick(repo(&d), feat_hash.clone()).expect_err("应冲突暂停");
    let st = git_sequencer_state(repo(&d)).expect("state");
    assert_eq!(
        st.cherry_pick_head.as_deref(),
        Some(&feat_hash[..7]),
        "短 hash 对上被捡提交"
    );
    assert_eq!(st.cherry_pick_author.as_deref(), Some("LazeCode Test"));
    assert_eq!(st.cherry_pick_subject.as_deref(), Some("pick me"));
    assert!(st.merge_head.is_none(), "非合并无 mergeHead");
    git_cherry_pick_abort(repo(&d)).expect("abort");
    let st = git_sequencer_state(repo(&d)).expect("clean");
    assert!(st.cherry_pick_head.is_none() && st.cherry_pick_author.is_none());

    // 合并冲突：mergeHead 对上被合分支头
    let d = conflicting_merge_repo("seq-heads-merge");
    let st = git_sequencer_state(repo(&d)).expect("mstate");
    let side_hash = git(&d, &["rev-parse", "side"]).trim().to_string();
    assert_eq!(st.merge_head.as_deref(), Some(&side_hash[..7]));
    assert!(st.cherry_pick_head.is_none());
    git_merge_abort(repo(&d)).expect("abort");
}

/// 正文冲突 + 删改冲突混在一起：一次调用点亮所有行的 Yours/Theirs 列。
#[test]
fn conflict_kinds_batch_cover_content_and_delete() {
    let d = fresh_dir("kinds");
    git(&d, &["init", "-b", "main"]);
    w(&d, "both.txt", "base\n");
    w(&d, "gone.txt", "gone\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "side"]);
    w(&d, "both.txt", "side\n");
    // side 删 gone.txt
    std::fs::remove_file(d.join("gone.txt")).unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-m", "side"]);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "both.txt", "main\n");
    w(&d, "gone.txt", "gone changed\n");
    git(&d, &["commit", "-am", "main"]);
    git_merge(repo(&d), "side".to_string()).expect_err("应冲突暂停");
    let kinds = git_conflict_kinds(repo(&d)).expect("kinds");
    assert_eq!(kinds.get("both.txt").map(String::as_str), Some("content"));
    // main 改、side 删 = deleted_by_them（对方删了我方改）
    assert_eq!(
        kinds.get("gone.txt").map(String::as_str),
        Some("deleted_by_them")
    );
    git_merge_abort(repo(&d)).expect("abort");
    let kinds = git_conflict_kinds(repo(&d)).expect("clean");
    assert!(kinds.is_empty(), "解决后无未合并条目");
}

/// 三路标记文本：非冲突部分已自动合好，冲突处留 diff3 标记给前端切块。
#[test]
fn merge_file_marked_splits_clean_and_conflict() {
    let d = fresh_dir("marked");
    git(&d, &["init", "-b", "main"]);
    w(&d, "f.txt", "same\nbase\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "side"]);
    w(&d, "f.txt", "same\nside\n");
    git(&d, &["commit", "-am", "side"]);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "f.txt", "same\nmain\n");
    git(&d, &["commit", "-am", "main"]);
    git_merge(repo(&d), "side".to_string()).expect_err("应冲突暂停");
    let m = block_on(git_merge_file_marked(repo(&d), "f.txt".to_string())).expect("marked");
    assert!(m.merged.starts_with("same\n"), "非冲突行已自动合好");
    assert!(m.merged.contains("<<<<<<< ours"), "冲突块有 ours 标记");
    assert!(m.merged.contains("||||||| base"), "diff3 带 base 段");
    assert!(m.merged.contains("======="), "有分隔");
    assert!(m.merged.contains(">>>>>>> theirs"), "有 theirs 标记");
    assert!(
        m.merged.contains("main\n") && m.merged.contains("side\n"),
        "双方内容都在块里"
    );
    git_merge_abort(repo(&d)).expect("abort");
}

/// 删改冲突也能开合并工具：缺失边按空文件合（对齐 PyCharm 的 Merge… 常亮）。
#[test]
fn merge_file_marked_supports_delete_conflict_with_empty_side() {
    let d = fresh_dir("marked-du");
    git(&d, &["init", "-b", "main"]);
    w(&d, "gone.txt", "gone\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "base"]);
    git(&d, &["checkout", "-qb", "side"]);
    std::fs::remove_file(d.join("gone.txt")).unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-m", "side del"]);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "gone.txt", "gone changed\n");
    git(&d, &["commit", "-am", "main change"]);
    git_merge(repo(&d), "side".to_string()).expect_err("应冲突暂停");
    let m = block_on(git_merge_file_marked(repo(&d), "gone.txt".to_string())).expect("DU 也能合");
    assert!(
        m.merged.contains("<<<<<<< ours"),
        "改/删分歧应留冲突块，原文：{}",
        m.merged
    );
    assert!(m.merged.contains("gone changed"), "存活方内容在块里");
    git_merge_abort(repo(&d)).expect("abort");
}

/// Drop Commit 必须先检查工作区：未提交改动存在时拒绝，避免 reset --hard 吞掉。
#[test]
fn drop_commit_refuses_dirty_worktree() {
    let d = basic_repo("drop-dirty");
    let head = head_hash(&d);
    w(&d, "a.txt", "uncommitted\n");
    let err = git_drop_commit(repo(&d), head.clone(), None).expect_err("脏工作区应拒绝");
    assert!(
        err.contains("worktree_dirty"),
        "应返回 worktree_dirty: {err}"
    );
    assert!(d.join("a.txt").exists(), "文件不能被删除");
}

/// 标签列表/删除：Branch 下拉的 Tags 区依赖这两个命令。
#[test]
fn tags_list_and_delete() {
    let d = basic_repo("tags");
    git_create_tag(repo(&d), "v1.0.0".to_string(), None, None).expect("create");
    let tags = git_list_tags(repo(&d)).expect("list");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v1.0.0");
    git_delete_tag(repo(&d), "v1.0.0".to_string()).expect("delete");
    assert!(git_list_tags(repo(&d)).expect("list2").is_empty());
}

/// 批量 Drop 走一次 rebase todo；逐个 reset 会因 hash 失效而失败。
#[test]
fn batch_drop_commits_rewrites_once() {
    let d = basic_repo("drop-many");
    let second = head_hash(&d);
    w(&d, "c.txt", "c\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "third"]);
    let third = head_hash(&d);
    git_drop_commits(repo(&d), vec![second, third]).expect("batch drop");
    let log = block_on(git_log(
        repo(&d),
        Some(10),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
    .expect("log");
    assert_eq!(
        log.len(),
        1,
        "只应剩 init: {:?}",
        log.iter().map(|c| &c.message).collect::<Vec<_>>()
    );
    assert_eq!(log[0].message, "init");
}

/// hunk 级暂存：补丁只含一个 hunk，--cached 后索引里出现新增行。
#[test]
fn apply_patch_stages_one_hunk() {
    let d = basic_repo("hunk-apply");
    let patch = "\
diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,4 @@
 line1
 line2
 line3
+line4
";
    git_apply_patch(repo(&d), patch.to_string(), Some(true), Some(false)).expect("apply");
    let cached = git(&d, &["diff", "--cached", "--", "a.txt"]);
    assert!(cached.contains("+line4"), "索引应包含新增行: {cached}");
}

/// 已推到远程的提交不能从历史里摘除：后端必须独立拦截（菜单状态可能过期）。
#[test]
fn drop_commit_rejects_published_commit() {
    let d = basic_repo("drop-published");
    let head = head_hash(&d);
    git(&d, &["update-ref", "refs/remotes/origin/main", &head]);
    let err = git_drop_commit(repo(&d), head.clone(), None).expect_err("远程已包含的提交应拒绝");
    assert!(
        err.contains("commit_published"),
        "应返回 commit_published: {err}"
    );
    assert_eq!(head_hash(&d), head, "拒绝时 HEAD 不能移动");
}

/// 范围内有合并提交时批量 Drop 必须在启动 rebase 前失败，不能留下半完成状态。
#[test]
fn batch_drop_rejects_merge_in_range_before_rebase() {
    let d = basic_repo("drop-merge");
    let old = head_hash(&d);
    git(&d, &["checkout", "-qb", "side"]);
    w(&d, "side.txt", "side\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "side"]);
    git(&d, &["checkout", "-q", "main"]);
    w(&d, "main.txt", "main\n");
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "main"]);
    git(&d, &["merge", "--no-ff", "side", "-m", "merge side"]);
    let head = head_hash(&d);

    let err = git_drop_commits(repo(&d), vec![old.clone()]).expect_err("范围内有 merge 应拒绝");
    assert!(
        err.contains("merge_commit_drop"),
        "应返回 merge_commit_drop: {err}"
    );
    assert_eq!(head_hash(&d), head, "拒绝时 HEAD 不能移动");
    assert!(
        !d.join(".git/rebase-merge").exists(),
        "不能留下 rebase-merge"
    );
    assert!(
        !d.join(".git/rebase-apply").exists(),
        "不能留下 rebase-apply"
    );
}
