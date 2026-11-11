//! History rewrite (merge/rebase/cherry-pick/revert/reset/drop/squash) and
//! sequencer-state introspection. Reset, merge and clean rebase are native
//! (gix refs/index/tree/merge); conflicted rebase, cherry-pick and revert still
//! fall back to the `git` porcelain. Every rewrite rejects a dirty worktree
//! first (matching main's safety model) unless the caller explicitly passes
//! `force`.

use std::path::Path;

use anyhow::anyhow;
use gix::hash::ObjectId;
use gix::refs::transaction::{PreviousValue, RefEdit};
use gix::refs::{FullName, Target};

use crate::ops::{
    open_index, remove_empty_parents, safe_worktree_join, write_index, CheckoutBackup,
};
use crate::tree::{
    commit_tree_map, head_tree_map, index_map, index_state_from_tree, materialize_tree,
    tree_has_gitlinks, tree_map,
};
use crate::{discover, workdir, Result};

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
    let mut cmd = crate::porcelain(root);
    cmd.args(args);
    cmd
}

fn run_ok(root: &Path, args: &[&str]) -> bool {
    builder(root, args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// A ref/committish that cannot be mistaken for a git option. Branch names
/// can legally start with `-` in some repositories; passing one raw to
/// `git merge`/`rebase` would execute `--exec=...`-style options instead.
fn validated_ref(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty()
        || name.starts_with('-')
        || name.contains("..")
        || name.contains('\0')
        || name.chars().any(char::is_whitespace)
    {
        anyhow::bail!("unsafe ref name '{name}'");
    }
    Ok(name)
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Shared safety gate for history rewrites (drop/squash): never rewrite a
/// merge commit, a commit outside HEAD's ancestry, or published history.
fn ensure_rewritable(root: &Path, hash: &str) -> Result<()> {
    let hash = validated_hash(hash)?;
    let parents = run(root, &["rev-list", "--parents", "-n", "1", hash])?;
    if parents.split_whitespace().count() > 2 {
        anyhow::bail!("cannot rewrite merge commit {hash}");
    }
    if !run_ok(root, &["merge-base", "--is-ancestor", hash, "HEAD"]) {
        anyhow::bail!("{hash} is not an ancestor of HEAD");
    }
    let published = run(
        root,
        &[
            "for-each-ref",
            "--contains",
            hash,
            "--format=%(refname)",
            "refs/remotes",
        ],
    )?;
    if !published.trim().is_empty() {
        anyhow::bail!(
            "{hash} is contained in a remote branch; refusing to rewrite published history"
        );
    }
    Ok(())
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
    let ref_name = validated_ref(ref_name)?;
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let target = repo
        .rev_parse_single(gix::bstr::BStr::new(ref_name))?
        .detach();
    let head = repo.head_id()?.detach();
    let base = repo.merge_base(head, target)?.map(|id| id.detach());
    if base == Some(head) {
        reset(root, &target.to_string(), ResetMode::Hard)?;
        return Ok("fast-forwarded".to_string());
    }
    if base == Some(target) {
        return Ok("Already up to date".to_string());
    }

    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: None,
        current: Some(gix::bstr::BStr::new("HEAD")),
        other: Some(gix::bstr::BStr::new(ref_name)),
    };
    let mut outcome =
        repo.merge_commits(head, target, labels, repo.tree_merge_options()?.into())?;
    let tree_id = outcome.tree_merge.tree.write()?.detach();
    let merged = tree_map(&repo, tree_id)?;
    let head_map = head_tree_map(&repo)?;
    let how = gix::merge::tree::TreatAsUnresolved::git();
    let unresolved = outcome.tree_merge.has_unresolved_conflicts(how);
    let message = format!("Merge branch '{ref_name}'");

    for path in merged.keys().chain(head_map.keys()) {
        safe_worktree_join(&workdir, path)?;
    }
    let backup = CheckoutBackup::capture(&repo, &workdir, merged.keys().chain(head_map.keys()))?;
    let old_index = std::fs::read(repo.index_path()).ok();
    let result = (|| -> Result<()> {
        for path in head_map.keys() {
            if merged.contains_key(path) {
                continue;
            }
            let relative = crate::ops::validate_repo_path(path)?;
            let absolute = workdir.join(&relative);
            match std::fs::symlink_metadata(&absolute) {
                Ok(metadata) if metadata.is_dir() => {
                    anyhow::bail!("refusing to remove directory {}", absolute.display());
                }
                Ok(_) => {
                    std::fs::remove_file(&absolute)?;
                    remove_empty_parents(&workdir, &relative);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        materialize_tree(&repo, &merged)?;

        let mut state = index_state_from_tree(&repo, &merged);
        if unresolved {
            gix::merge::tree::apply_index_entries(
                &outcome.tree_merge.conflicts,
                how,
                &mut state,
                gix::merge::tree::apply_index_entries::RemovalMode::Prune,
            );
        }
        write_index(gix::index::File::from_state(state, repo.index_path()))?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(restore_error) = backup.restore() {
            log::error!("merge rollback failed: {restore_error}");
        }
        match old_index {
            Some(bytes) => {
                let _ = lazecode_fs::file::write_atomic(&repo.index_path(), &bytes);
            }
            None => {
                let _ = std::fs::remove_file(repo.index_path());
            }
        }
        return Err(error);
    }
    backup.discard();

    if unresolved {
        let git_dir = repo.git_dir();
        lazecode_fs::file::write_atomic(
            &git_dir.join("MERGE_HEAD"),
            format!("{target}\n").as_bytes(),
        )?;
        lazecode_fs::file::write_atomic(
            &git_dir.join("ORIG_HEAD"),
            format!("{head}\n").as_bytes(),
        )?;
        lazecode_fs::file::write_atomic(
            &git_dir.join("MERGE_MSG"),
            format!("{message}\n").as_bytes(),
        )?;
        return Err(anyhow!("merge conflict: resolve the unmerged files"));
    }

    let commit = repo
        .new_commit(message, tree_id, [head, target])?
        .id()
        .detach();
    update_head(&repo, commit, "merge")?;
    Ok("merged".to_string())
}

pub fn merge_abort(root: &Path) -> Result<String> {
    run(root, &["merge", "--abort"]).map(|_| "merge aborted".to_string())
}

pub fn merge_continue(root: &Path) -> Result<String> {
    let output = builder(root, &["merge", "--continue"])
        .env("GIT_EDITOR", "true")
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok("merge finished".to_string())
}

// ---------------- rebase ----------------

pub fn rebase(root: &Path, onto: &str) -> Result<String> {
    require_clean(root)?;
    let onto_name = validated_ref(onto)?;
    let repo = discover(root)?;
    let onto_id = repo
        .rev_parse_single(gix::bstr::BStr::new(onto_name))?
        .detach();
    let head = repo.head_id()?.detach();
    let Some(base) = repo.merge_base(head, onto_id)?.map(|id| id.detach()) else {
        anyhow::bail!("cannot rebase: no common ancestor with {onto_name}");
    };
    if base == head {
        reset(root, &onto_id.to_string(), ResetMode::Hard)?;
        return Ok("fast-forwarded".to_string());
    }
    if base == onto_id {
        return Ok("Already up to date".to_string());
    }

    let mut commits: Vec<ObjectId> = repo
        .rev_walk([head])
        .with_hidden([base])
        .all()?
        .map(|info| Ok(info?.id))
        .collect::<Result<Vec<_>>>()?;
    commits.reverse();
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: None,
        current: Some(gix::bstr::BStr::new("onto")),
        other: Some(gix::bstr::BStr::new("commit")),
    };
    let how = gix::merge::tree::TreatAsUnresolved::git();
    let mut current = onto_id;
    for commit_id in commits {
        let commit = repo.find_commit(commit_id)?;
        let parent_tree = match commit.parent_ids().next() {
            Some(parent) => repo.find_commit(parent.detach())?.tree_id()?.detach(),
            None => repo.empty_tree().id,
        };
        let commit_tree = commit.tree_id()?.detach();
        let current_tree = repo.find_commit(current)?.tree_id()?.detach();
        let mut outcome = repo.merge_trees(
            parent_tree,
            current_tree,
            commit_tree,
            labels,
            repo.tree_merge_options()?,
        )?;
        if outcome.has_unresolved_conflicts(how) {
            // Keep the porcelain sequencer for conflicted rebases; nothing
            // was written to refs/index/worktree yet.
            return run(root, &["rebase", onto_name]).map(|_| "rebased".to_string());
        }
        let tree = outcome.tree.write()?.detach();
        let message = commit.message_raw()?.to_string();
        let author = commit.author()?;
        let committer = repo
            .committer()
            .ok_or_else(|| anyhow!("Committer identity is not configured"))??;
        current = repo
            .new_commit_as(committer, author, message, tree, [current])?
            .id()
            .detach();
    }
    reset(root, &current.to_string(), ResetMode::Hard)?;
    Ok("rebased".to_string())
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
    let onto = validated_ref(onto)?;
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
        .env(
            "GIT_SEQUENCE_EDITOR",
            format!("cp {}", shell_single_quote(&todo_path.to_string_lossy())),
        )
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
    let output = builder(root, &["cherry-pick", "--continue"])
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        // Do not silently `--skip`: that would discard a pick that failed
        // because of hooks, unmerged paths or a missing message. The user can
        // still abort or resolve in the terminal/conflict bar.
        return Err(anyhow!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
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
    let target_name = if target.trim().is_empty() {
        "HEAD"
    } else {
        validated_ref(target)?
    };
    let repo = discover(root)?;
    let target_id = repo
        .rev_parse_single(gix::bstr::BStr::new(target_name))
        .map_err(|err| anyhow!("cannot resolve '{target_name}': {err}"))?
        .detach();
    let message = format!("reset: moving to {target_name}");
    if mode == ResetMode::Soft {
        // Soft reset never touches the index, so unsupported index
        // extensions or submodule entries must not block it.
        update_head(&repo, target_id, &message)?;
        return Ok("branch reset (soft)".to_string());
    }
    let target_map = commit_tree_map(&repo, target_id)?;
    let target_tree = repo.find_commit(target_id)?.tree()?;
    if tree_has_gitlinks(&repo, target_tree)? {
        anyhow::bail!("reset target contains submodules; submodule support is not implemented");
    }
    let index = open_index(&repo)?;
    if index
        .entries()
        .iter()
        .any(|entry| entry.mode == gix::index::entry::Mode::COMMIT)
    {
        anyhow::bail!("cannot reset while the index contains submodules");
    }
    let old_map = index_map(&index);
    let workdir = workdir(&repo)?;
    match mode {
        ResetMode::Soft => unreachable!("handled above"),
        ResetMode::Mixed => {
            let old_index = std::fs::read(repo.index_path()).ok();
            write_index_from_state(index_state_from_tree(&repo, &target_map), &repo)?;
            if let Err(error) = update_head(&repo, target_id, &message) {
                restore_index(&repo, old_index);
                return Err(error);
            }
        }
        ResetMode::Hard => {
            for path in target_map.keys().chain(old_map.keys()) {
                safe_worktree_join(&workdir, path)?;
            }
            let backup =
                CheckoutBackup::capture(&repo, &workdir, old_map.keys().chain(target_map.keys()))?;
            let old_index = std::fs::read(repo.index_path()).ok();
            let result = (|| -> Result<()> {
                for path in old_map.keys() {
                    if target_map.contains_key(path) {
                        continue;
                    }
                    let relative = crate::ops::validate_repo_path(path)?;
                    let abs = workdir.join(&relative);
                    match std::fs::symlink_metadata(&abs) {
                        Ok(metadata) if metadata.is_dir() => {
                            anyhow::bail!(
                                "refusing to remove directory {} while resetting hard",
                                abs.display()
                            );
                        }
                        Ok(_) => {
                            std::fs::remove_file(&abs)?;
                            remove_empty_parents(&workdir, &relative);
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                        Err(err) => anyhow::bail!("cannot remove {}: {err}", abs.display()),
                    }
                }
                materialize_tree(&repo, &target_map)?;
                write_index_from_state(index_state_from_tree(&repo, &target_map), &repo)?;
                update_head(&repo, target_id, &message)?;
                Ok(())
            })();
            if let Err(error) = result {
                if let Err(restore_error) = backup.restore() {
                    log::error!("reset rollback failed: {restore_error}");
                }
                restore_index(&repo, old_index);
                return Err(error);
            }
            backup.discard();
        }
    }
    Ok(match mode {
        ResetMode::Soft => "branch reset (soft)".to_string(),
        ResetMode::Mixed => "branch reset (mixed)".to_string(),
        ResetMode::Hard => "branch reset (hard)".to_string(),
    })
}

fn write_index_from_state(state: gix::index::State, repo: &gix::Repository) -> Result<()> {
    write_index(gix::index::File::from_state(state, repo.index_path()))
}

fn restore_index(repo: &gix::Repository, old_index: Option<Vec<u8>>) {
    match old_index {
        Some(bytes) => {
            if let Err(error) = lazecode_fs::file::write_atomic(&repo.index_path(), &bytes) {
                log::error!("reset index rollback failed: {error}");
            }
        }
        None => {
            let _ = std::fs::remove_file(repo.index_path());
        }
    }
}

fn update_head(repo: &gix::Repository, target: ObjectId, message: &str) -> Result<()> {
    let head = repo.head()?;
    let name: FullName = match head.referent_name() {
        Some(referent) => referent.to_owned(),
        None => "HEAD".try_into()?,
    };
    let mut edit = RefEdit::update(
        name,
        Target::Object(target),
        PreviousValue::Any,
        message.to_string(),
    );
    edit.deref = false;
    repo.edit_reference(edit)?;
    Ok(())
}

/// Drop a single commit: HEAD via reset, otherwise rebase --onto with a
/// hash whitelist. The worktree must be clean.
pub fn drop_commit(root: &Path, hash: &str, force: bool) -> Result<String> {
    if !force {
        require_clean(root)?;
    }
    let hash = validated_hash(hash)?;
    ensure_rewritable(root, hash)?;
    // HEAD drop is a plain hard reset to HEAD~.
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
    let mut selected = Vec::with_capacity(hashes.len());
    for hash in hashes {
        selected.push(validated_hash(hash)?.to_string());
    }
    for hash in &selected {
        ensure_rewritable(root, hash)?;
    }

    // Find the oldest selected commit by ancestry.
    let mut oldest = selected[0].clone();
    for candidate in selected.iter().skip(1) {
        if run_ok(root, &["merge-base", "--is-ancestor", candidate, &oldest]) {
            oldest = candidate.clone();
        }
    }

    // Squashing must only fold the selected newest commits. Folding a range
    // that also contains unselected commits (or commits newer than the
    // selection) silently rewrites history the user did not ask for.
    let parent_probe = run(
        root,
        &["rev-parse", "--verify", "--quiet", &format!("{oldest}^")],
    );
    if parent_probe.is_err() || parent_probe.as_deref().unwrap_or("").is_empty() {
        anyhow::bail!("squashing the root commit is not supported");
    }
    let base_target = format!("{oldest}^");
    let range = format!("{base_target}..HEAD");
    let listed = run(root, &["rev-list", "--reverse", &range])?;
    let listed: Vec<String> = listed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let mut resolved = Vec::with_capacity(selected.len());
    for hash in &selected {
        resolved.push(run(root, &["rev-parse", hash])?);
    }
    let selected_set: std::collections::HashSet<&str> =
        resolved.iter().map(String::as_str).collect();
    if selected_set.len() != resolved.len()
        || listed.len() != resolved.len()
        || !listed
            .iter()
            .all(|hash| selected_set.contains(hash.as_str()))
    {
        anyhow::bail!("squash selection must be the newest contiguous commits on this branch");
    }

    // The tree after a soft reset to `oldest^` is exactly the folded tree.
    reset(root, &base_target, ResetMode::Soft)?;
    let subject = message.map(str::to_string).unwrap_or_else(|| {
        run(root, &["log", "-1", "--format=%s", &oldest])
            .unwrap_or_else(|_| format!("squash: {}", selected.len()))
    });
    let output = builder(root, &["commit", "-m", subject.as_str()])
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!("folded {} commits", selected.len()))
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
    let output = builder(root, &["status", "--porcelain=v1", "-z"])
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut out = Vec::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.len() < 4 {
            continue;
        }
        let code = String::from_utf8_lossy(&entry[..2]);
        let conflicted = code.contains('U') || code == "AA" || code == "DD";
        if !conflicted {
            continue;
        }
        let path = String::from_utf8_lossy(&entry[3..]).to_string();
        if path.is_empty() {
            continue;
        }
        out.push(ConflictEntry {
            path,
            kind: Some(code.to_string()),
        });
    }
    Ok(out)
}

/// Resolve a conflicted path by taking one side wholesale. For delete/modify
/// conflicts the requested side has no index stage, so the correct operation
/// is `git rm`, not an empty checkout.
pub fn resolve_take(root: &Path, path: &str, ours: bool) -> Result<()> {
    let relative = crate::ops::validate_repo_path(path)?;
    let path = relative.to_string_lossy();
    let stage = if ours { "2" } else { "3" };
    let output = builder(root, &["ls-files", "-u", "--", path.as_ref()])
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    let has_stage = String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.split_whitespace().nth(2) == Some(stage));
    if has_stage {
        let side = if ours { "--ours" } else { "--theirs" };
        run(root, &["checkout", side, "--", path.as_ref()])?;
        run(root, &["add", "--", path.as_ref()]).map(drop)
    } else {
        run(root, &["rm", "-f", "--", path.as_ref()]).map(drop)
    }
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
    let relative = crate::ops::validate_repo_path(path)?;
    let path = relative.to_string_lossy();
    let repo = discover(root)?;
    let workdir = crate::workdir(&repo)?;
    let full = crate::ops::safe_worktree_join(&workdir, &path)?;
    let ours = run(root, &["show", &format!(":2:{path}")]).unwrap_or_default();
    let theirs = run(root, &["show", &format!(":3:{path}")]).unwrap_or_default();
    let base = run(root, &["show", &format!(":1:{path}")]).ok();
    let worktree = std::fs::read(&full)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    Ok(MergeFile3Way {
        path: path.to_string(),
        ours,
        theirs,
        base,
        worktree,
    })
}

pub fn mark_conflict_resolved(root: &Path, path: &str, content: Option<&str>) -> Result<()> {
    let relative = crate::ops::validate_repo_path(path)?;
    let path = relative.to_string_lossy();
    if let Some(content) = content {
        let repo = discover(root)?;
        let workdir = crate::workdir(&repo)?;
        let full = crate::ops::safe_worktree_join(&workdir, &path)?;
        std::fs::write(full, content.as_bytes())?;
    }
    run(root, &["add", "--", path.as_ref()]).map(drop)
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
    let relative = crate::ops::validate_repo_path(path)?;
    let path = relative.to_string_lossy();
    let output = builder(root, &["checkout", revision, "--", path.as_ref()])
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
