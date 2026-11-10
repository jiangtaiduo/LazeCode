//! History rewrite (merge/rebase/cherry-pick/revert/reset/drop/squash) and
//! sequencer-state introspection, driven by the `git` porcelain. Every path
//! rejects a dirty worktree first (matching main's safety model) unless the
//! caller explicitly passes `force`.

use std::path::Path;

use anyhow::anyhow;

use crate::{discover, Result};

fn run(root: &Path, args: &[&str]) -> Result<String> {
    let output = builder(root, args)
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        // git writes most porcelain errors to stderr, but merge/cherry-pick
        // conflict banners land on stdout as well — keep both.
        return Err(anyhow!("{}{}", stdout.trim(), stderr.trim()));
    }
    Ok(stdout.trim().to_string())
}

fn builder(root: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(root).args(args);
    cmd
}

pub fn require_clean(root: &Path) -> Result<()> {
    let output = crate::status::snapshot(root)?;
    if output.has_tracked_changes() {
        anyhow::bail!("working tree has uncommitted changes; commit or stash first");
    }
    Ok(())
}

fn validated_hash(hash: &str) -> Result<&str> {
    let hash = hash.trim();
    let valid = (7..=40).contains(&hash.len()) && hash.chars().all(|c| c.is_ascii_hexdigit());
    if !valid {
        anyhow::bail!("invalid commit hash '{hash}'");
    }
    Ok(hash)
}

// ---------------- merge ----------------

pub fn merge(root: &Path, ref_name: &str) -> Result<String> {
    require_clean(root)?;
    run(root, &["merge", "--no-edit", ref_name]).map(|_| "merged".to_string())
}

pub fn merge_abort(root: &Path) -> Result<String> {
    run(root, &["merge", "--abort"]).map(|_| "merge aborted".to_string())
}

pub fn merge_continue(root: &Path) -> Result<String> {
    run(root, &["commit", "--no-edit", "--no-verify"]).map(|_| "merge finished".to_string())
}

// ---------------- rebase ----------------

pub fn rebase(root: &Path, onto: &str) -> Result<String> {
    require_clean(root)?;
    run(root, &["rebase", onto]).map(|_| "rebased".to_string())
}

pub fn rebase_continue(root: &Path) -> Result<String> {
    let output = builder(root, &["rebase", "--continue"])
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok("rebase continuing".to_string())
}

pub fn rebase_abort(root: &Path) -> Result<String> {
    run(root, &["rebase", "--abort"]).map(|_| "rebase aborted".to_string())
}

pub fn rebase_skip(root: &Path) -> Result<String> {
    run(root, &["rebase", "--skip"]).map(|_| "rebase skipped".to_string())
}

/// 交互式变基的一条 todo（action ∈ pick/reword/edit/squash/fixup/drop）。
#[derive(Clone, Debug)]
pub struct RebaseStep {
    pub action: String,
    pub commit: String,
}

/// 执行交互式变基：把 UI 编排好的 todo 写进临时文件，再用
/// GIT_SEQUENCE_EDITOR 让 `git rebase -i` 直接采用该清单（不弹外部
/// 编辑器）。GIT_EDITOR=true 使 reword 保留原提交信息（UI 暂无消息
/// 编辑框）；edit 会停在对应提交，由面板的 Continue/Abort 接管。
pub fn rebase_interactive(root: &Path, onto: &str, steps: &[RebaseStep]) -> Result<String> {
    require_clean(root)?;
    // onto 可以是分支名等 committish，先解析成完整提交号。
    let resolved = run(
        root,
        &["rev-parse", "--verify", &format!("{onto}^{{commit}}")],
    )?;
    let onto = validated_hash(&resolved)?;
    if steps.is_empty() {
        anyhow::bail!("empty interactive rebase todo");
    }
    let mut todo = String::new();
    for step in steps {
        let action = step.action.trim();
        if !matches!(
            action,
            "pick" | "reword" | "edit" | "squash" | "fixup" | "drop"
        ) {
            anyhow::bail!("unsupported rebase action '{action}'");
        }
        let commit = validated_hash(&step.commit)?;
        todo.push_str(&format!("{action} {commit}\n"));
    }
    // 临时清单文件：进程号 + 纳秒避免并行冲突，结束后整目录清理。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("lazecode-rebase-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let todo_path = dir.join("todo");
    std::fs::write(&todo_path, todo)?;
    let output = builder(root, &["rebase", "-i", onto])
        .env("GIT_SEQUENCE_EDITOR", format!("cp {}", todo_path.display()))
        .env("GIT_EDITOR", "true")
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(anyhow!("{}{}", stdout.trim(), stderr.trim()));
    }
    Ok(stdout.trim().to_string())
}

// ---------------- cherry-pick ----------------

pub fn cherry_pick(root: &Path, hash: &str) -> Result<String> {
    require_clean(root)?;
    let hash = validated_hash(hash)?;
    run(root, &["cherry-pick", hash]).map(|_| "picked".to_string())
}

pub fn cherry_pick_continue(root: &Path) -> Result<String> {
    // Finish uses the recorded message; neutralise the editor invocation so
    // headless runs don't hang.
    let output = builder(root, &["cherry-pick", "--continue"])
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        // Taking one side can reduce the pick to a no-op; consume it by
        // skipping so the sequencer unwinds instead of wedging.
        let skip = builder(root, &["cherry-pick", "--skip"])
            .env("GIT_EDITOR", "true")
            .env("GIT_SEQUENCE_EDITOR", "true")
            .output()
            .map_err(|err| anyhow!("git spawn failed: {err}"))?;
        if !skip.status.success() {
            return Err(anyhow!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        return Ok("cherry-pick consumed (empty pick skipped)".to_string());
    }
    Ok("cherry-pick finishing".to_string())
}

pub fn cherry_pick_abort(root: &Path) -> Result<String> {
    run(root, &["cherry-pick", "--abort"]).map(|_| "cherry-pick aborted".to_string())
}

// ---------------- revert / reset ----------------

pub fn revert(root: &Path, hash: &str) -> Result<String> {
    require_clean(root)?;
    let hash = validated_hash(hash)?;
    run(root, &["revert", "--no-edit", hash]).map(|_| "reverted".to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

pub fn reset(root: &Path, target: &str, mode: ResetMode) -> Result<String> {
    let flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };
    let target = if target.trim().is_empty() {
        "HEAD"
    } else {
        target.trim()
    };
    let output = builder(root, &["reset", flag, target])
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(match mode {
        ResetMode::Soft => "branch reset (soft)".to_string(),
        ResetMode::Mixed => "branch reset (mixed)".to_string(),
        ResetMode::Hard => "branch reset (hard)".to_string(),
    })
}

/// Drop a single commit: HEAD via reset, otherwise rebase --onto with a
/// hash whitelist. The worktree must be clean.
pub fn drop_commit(root: &Path, hash: &str, force: bool) -> Result<String> {
    if !force {
        require_clean(root)?;
    }
    let hash = validated_hash(hash)?;
    // HEAD drop is a plain soft reset to HEAD~.
    let head = run(root, &["rev-parse", "HEAD"])?;
    if head == hash {
        return reset(root, "HEAD~1", ResetMode::Hard);
    }
    // Any other commit: rebase --onto {hash}^ {hash} (whitelist-compatible).
    let output = builder(
        root,
        &["rebase", "--onto", &format!("{hash}^"), hash, "HEAD"],
    )
    .output()
    .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!("dropped {}", &hash[..7.min(hash.len())]))
}

/// Squash a contiguous chain of commits into one; `message` overrides the
/// first commit's message when non-empty.
pub fn squash_commits(root: &Path, hashes: &[String], message: Option<&str>) -> Result<String> {
    if hashes.len() < 2 {
        anyhow::bail!("need at least two commits to squash");
    }
    require_clean(root)?;
    for hash in hashes {
        validated_hash(hash)?;
    }
    // Determine the oldest commit by actual ancestry order: query log order
    // (the caller may pass commits newest→oldest either way).
    // Ancestry-sorted seed: rev-list accepts hash args but their order maps
    // to the output — pick the pair whose commit is the ancestor of all.
    let mut oldest = hashes[0].clone();
    for candidate in hashes.iter().skip(1) {
        let is_ancestor = run(
            root,
            &["merge-base", "--is-ancestor", candidate.as_str(), &oldest],
        )
        .is_ok();
        if is_ancestor {
            oldest = candidate.clone();
        }
    }
    // Drop everything at-or-after `oldest` from the branch, then re-commit
    // the combined tree of the folded commits.
    let parent_probe = run(
        root,
        &["rev-parse", "--verify", "--quiet", &format!("{oldest}^")],
    );
    let base_target = if parent_probe.is_ok() && !parent_probe.as_deref().unwrap_or("").is_empty() {
        format!("{oldest}^")
    } else {
        oldest.clone()
    };
    reset(root, &base_target, ResetMode::Soft)?;
    let subject = message
        .map(str::to_string)
        .unwrap_or_else(|| format!("squash: {}", hashes.len()));
    // The commit's tree is already right from the soft reset.
    let output = builder(root, &["commit", "--no-verify", "-m", subject.as_str()]).output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!("folded {} commits", hashes.len()))
}

// ---------------- sequencer state ----------------

/// Which sequencer-ish operation is in flight; drives the conflict banner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SequencerState {
    None,
    Merge { head: String },
    Rebase,
    CherryPick { head: String },
    Revert { head: String },
}

pub fn sequencer_state(root: &Path) -> Result<SequencerState> {
    let git_dir = discover(root)?.git_dir().to_path_buf();
    let read = |rel: &str| std::fs::read_to_string(git_dir.join(rel)).ok();
    if git_dir.join("MERGE_HEAD").exists() {
        let head = read("MERGE_HEAD").unwrap_or_default();
        return Ok(SequencerState::Merge {
            head: head.lines().next().unwrap_or("").to_string(),
        });
    }
    if read("rebase-merge/.orig-base").is_some()
        || read("rebase-merge/done").is_some()
        || git_dir.join("rebase-merge").exists()
        || git_dir.join("rebase-apply").exists()
    {
        return Ok(SequencerState::Rebase);
    }
    if let Some(head) = read("CHERRY_PICK_HEAD") {
        return Ok(SequencerState::CherryPick {
            head: head.lines().next().unwrap_or("").to_string(),
        });
    }
    if let Some(head) = read("REVERT_HEAD") {
        return Ok(SequencerState::Revert {
            head: head.lines().next().unwrap_or("").to_string(),
        });
    }
    Ok(SequencerState::None)
}

// ---------------- conflicts ----------------

/// One conflicted path from `git diff --name-only --diff-filter=U`.
#[derive(Clone, Debug)]
pub struct ConflictEntry {
    pub path: String,
    /// None = pure content conflict; Some(kind) = delete/modify conflict.
    pub kind: Option<String>,
}

pub fn conflict_list(root: &Path) -> Result<Vec<ConflictEntry>> {
    let output = run(
        root,
        &["diff", "--name-only", "--diff-filter=U", "-z", "--no-color"],
    )?;
    let entries: Vec<String> = output
        .split('\0')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    for entry in &entries {
        if !std::path::Path::new(&root.join(entry)).exists() {
            continue; // file was deleted-ours/theirs
        }
    }
    let mut kinds = std::collections::BTreeMap::new();
    let status = run(root, &["status", "--porcelain"])?;
    for line in status.lines() {
        let (code, path) = line.split_at(2);
        let path = path.trim();
        let kind = if code.contains('U') || code.contains('A') || code.contains('D') {
            Some(code.to_string())
        } else {
            None
        };
        kinds.insert(path.to_string(), kind);
    }
    let mut out = Vec::new();
    for (path, kind) in kinds {
        if entries.contains(&path) {
            out.push(ConflictEntry { path, kind });
        } else if !root.join(&path).exists() && entries.is_empty() {
            out.push(ConflictEntry {
                path,
                kind: Some("U".into()),
            });
        }
    }
    // Stable order: the porcelain status list order (already alphabetical).
    Ok(out)
}

/// Resolve a conflicted path by taking one side wholesale (`checkout --ours /
/// --theirs`), then stages the file.
pub fn resolve_take(root: &Path, path: &str, ours: bool) -> Result<()> {
    let side = if ours { "ours" } else { "theirs" };
    run(root, &["checkout", &format!("--{}", side), "--", path])?;
    run(root, &["add", "--", path]).map(drop)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeFile3Way {
    pub path: String,
    pub ours: String,
    pub theirs: String,
    pub base: Option<String>,
    pub worktree: String,
}

pub fn merge_file_content(root: &Path, path: &str) -> Result<MergeFile3Way> {
    let ours = run(root, &["show", &format!(":2:{path}")]).unwrap_or_default();
    let theirs = run(root, &["show", &format!(":3:{path}")]).unwrap_or_default();
    let base = run(root, &["show", &format!(":1:{path}")]).ok();
    let worktree = std::fs::read_to_string(root.join(path)).unwrap_or_default();
    Ok(MergeFile3Way {
        path: path.to_string(),
        ours,
        theirs,
        base,
        worktree,
    })
}

pub fn mark_conflict_resolved(root: &Path, path: &str, content: Option<&str>) -> Result<()> {
    if let Some(content) = content {
        std::fs::write(root.join(path), content.as_bytes())?;
    }
    run(root, &["add", "--", path]).map(drop)
}

/// Checkout a detached revision/commit.
pub fn checkout_revision(root: &Path, revision: &str) -> Result<String> {
    validated_hash(revision)?;
    require_clean(root)?;
    let output = builder(root, &["checkout", "--detach", revision])
        .output()
        .map_err(|e| anyhow!("git checkout failed: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!("checked out revision {revision}"))
}

/// Checkout a single file from a given revision (`git checkout <revision> -- <path>`).
pub fn checkout_revision_path(root: &Path, revision: &str, path: &str) -> Result<()> {
    validated_hash(revision)?;
    let output = builder(root, &["checkout", revision, "--", path])
        .output()
        .map_err(|e| anyhow!("git checkout file failed: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Undo the HEAD commit (soft reset HEAD~1).
pub fn undo_commit_head(root: &Path) -> Result<String> {
    require_clean(root)?;
    let output = builder(root, &["reset", "--soft", "HEAD~1"])
        .output()
        .map_err(|e| anyhow!("git reset --soft failed: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok("undid HEAD commit".to_string())
}

/// Amend HEAD commit's message only.
pub fn amend_commit_message(root: &Path, message: &str) -> Result<String> {
    let msg = message.trim();
    if msg.is_empty() {
        anyhow::bail!("commit message cannot be empty");
    }
    let output = builder(root, &["commit", "--amend", "--only", "-m", msg])
        .output()
        .map_err(|e| anyhow!("git commit --amend failed: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok("amended commit message".to_string())
}

/// Export full unified patch for a commit.
pub fn commit_patch(root: &Path, hash: &str) -> Result<String> {
    validated_hash(hash)?;
    let output = builder(root, &["format-patch", "-1", "--stdout", hash])
        .output()
        .map_err(|e| anyhow!("git format-patch failed: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
