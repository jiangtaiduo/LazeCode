//! History rewrite (merge/rebase/cherry-pick/revert/reset/drop/squash) and
//! sequencer-state introspection. Reset, merge, rebase, cherry-pick, revert
//! and conflict resolution are native (gix refs/index/tree/merge); only the
//! interactive-rebase todo shim, drop/squash and a few read-only helpers still
//! invoke the `git` porcelain. Every rewrite rejects a dirty worktree first
//! (matching main's safety model) unless the caller explicitly passes `force`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::anyhow;
use gix::bstr::ByteSlice;
use gix::hash::ObjectId;
use gix::index::entry::{Flags, Mode, Stage, Stat};
use gix::refs::transaction::{PreviousValue, RefEdit};
use gix::refs::{FullName, Target};

use crate::ops::{
    open_conflicted_index, remove_empty_parents, safe_worktree_join, write_index, CheckoutBackup,
};
use crate::tree::{
    blob_bytes, commit_tree_map, head_tree_map, index_state_from_tree, materialize_tree,
    tree_has_gitlinks, tree_map, write_tree_from_index, TreeEntry, TreeMap,
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
                let _ = lazecode_fs::file::write_atomic_locked(&repo.index_path(), &bytes);
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

/// Native rebase state, stored under `.git/rebase-merge/`. The directory name
/// matches Git's merge backend so [`sequencer_state`] and the UI banner see an
/// in-flight rebase; the `lazecode` marker distinguishes it from a rebase that
/// the interactive porcelain started.
#[derive(Clone, Debug)]
struct RebaseMeta {
    dir: std::path::PathBuf,
    head_name: Option<String>,
    orig_head: ObjectId,
    onto: ObjectId,
    stopped: Option<ObjectId>,
    current: ObjectId,
    todo: Vec<ObjectId>,
}

impl RebaseMeta {
    fn load(repo: &gix::Repository) -> Result<Option<Self>> {
        let dir = repo.git_dir().join("rebase-merge");
        if !dir.join("lazecode").exists() {
            return Ok(None);
        }
        let read = |name: &str| -> Result<String> {
            std::fs::read_to_string(dir.join(name))
                .map(|text| text.trim().to_string())
                .map_err(|error| anyhow!("rebase state is corrupt: cannot read {name}: {error}"))
        };
        let parse = |name: &str, text: &str| -> Result<ObjectId> {
            ObjectId::from_hex(text.as_bytes())
                .map_err(|_| anyhow!("rebase state is corrupt: {name} is not a commit id"))
        };
        let stopped = read("stopped-sha")?;
        let todo = read("git-rebase-todo")?;
        Ok(Some(Self {
            head_name: {
                let name = read("head-name")?;
                (!name.is_empty()).then_some(name)
            },
            orig_head: parse("orig-head", &read("orig-head")?)?,
            onto: parse("onto", &read("onto")?)?,
            current: parse("current", &read("current")?)?,
            stopped: if stopped.is_empty() {
                None
            } else {
                Some(parse("stopped-sha", &stopped)?)
            },
            todo: todo
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| parse("git-rebase-todo", line))
                .collect::<Result<Vec<_>>>()?,
            dir,
        }))
    }

    fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.dir.join("lazecode"), b"1\n")?;
        std::fs::write(self.dir.join("orig-head"), format!("{}\n", self.orig_head))?;
        std::fs::write(self.dir.join("onto"), format!("{}\n", self.onto))?;
        std::fs::write(self.dir.join("current"), format!("{}\n", self.current))?;
        std::fs::write(
            self.dir.join("head-name"),
            match &self.head_name {
                Some(name) => format!("{name}\n"),
                None => String::new(),
            },
        )?;
        match self.stopped {
            Some(id) => std::fs::write(self.dir.join("stopped-sha"), format!("{id}\n"))?,
            None => {
                let _ = std::fs::remove_file(self.dir.join("stopped-sha"));
            }
        }
        let todo: String = self.todo.iter().map(|id| format!("{id}\n")).collect();
        std::fs::write(self.dir.join("git-rebase-todo"), todo)?;
        Ok(())
    }

    fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn head_name(repo: &gix::Repository) -> Result<Option<String>> {
    let head = repo.head()?;
    Ok(head.referent_name().map(|name| name.to_string()))
}

fn detach_head(repo: &gix::Repository, target: ObjectId) -> Result<()> {
    let mut edit = RefEdit::update(
        "HEAD".try_into()?,
        Target::Object(target),
        PreviousValue::Any,
        "rebase: detaching HEAD".to_string(),
    );
    edit.deref = false;
    repo.edit_reference(edit)?;
    Ok(())
}

fn attach_head(repo: &gix::Repository, name: &str) -> Result<()> {
    let full: FullName = name.try_into()?;
    let mut edit = RefEdit::update(
        "HEAD".try_into()?,
        Target::Symbolic(full),
        PreviousValue::Any,
        format!("rebase: returning to {name}"),
    );
    edit.deref = false;
    repo.edit_reference(edit)?;
    Ok(())
}

fn move_reference(repo: &gix::Repository, name: &str, target: ObjectId) -> Result<()> {
    let full: FullName = name.try_into()?;
    let mut edit = RefEdit::update(
        full,
        Target::Object(target),
        PreviousValue::Any,
        "rebase finished".to_string(),
    );
    edit.deref = false;
    repo.edit_reference(edit)?;
    Ok(())
}

pub fn rebase(root: &Path, onto: &str) -> Result<String> {
    require_clean(root)?;
    let onto_name = validated_ref(onto)?;
    let repo = discover(root)?;
    if repo.git_dir().join("rebase-merge").exists() || repo.git_dir().join("rebase-apply").exists()
    {
        anyhow::bail!("a rebase is already in progress");
    }
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
    write_git_file(&repo, "ORIG_HEAD", &format!("{head}\n"))?;
    let meta = RebaseMeta {
        dir: repo.git_dir().join("rebase-merge"),
        head_name: head_name(&repo)?,
        orig_head: head,
        onto: onto_id,
        stopped: None,
        current: onto_id,
        todo: commits,
    };
    rebase_replay(root, &repo, &meta)
}

/// Replay `meta.todo` on top of `meta.current`. On conflict the merged tree is
/// checked out with index stages and the sequencer stops; on success the branch
/// is moved and HEAD reattached.
fn rebase_replay(root: &Path, repo: &gix::Repository, meta: &RebaseMeta) -> Result<String> {
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: None,
        current: Some(gix::bstr::BStr::new("onto")),
        other: Some(gix::bstr::BStr::new("commit")),
    };
    let how = gix::merge::tree::TreatAsUnresolved::git();
    let mut current = meta.current;
    for (position, commit_id) in meta.todo.iter().enumerate() {
        let commit = repo.find_commit(*commit_id)?;
        let parent_tree = parent_tree_of(repo, &commit)?;
        let commit_tree = commit.tree_id()?.detach();
        let current_tree = tree_of(repo, current)?;
        let mut outcome = repo.merge_trees(
            parent_tree,
            current_tree,
            commit_tree,
            labels,
            repo.tree_merge_options()?,
        )?;
        if outcome.has_unresolved_conflicts(how) {
            let tree_id = outcome.tree.write()?.detach();
            let base_map = tree_map(repo, current_tree)?;
            detach_head(repo, current)?;
            write_conflict_state(repo, &base_map, tree_id, &outcome.conflicts)?;
            let stopped = RebaseMeta {
                stopped: Some(*commit_id),
                current,
                todo: meta.todo[position + 1..].to_vec(),
                ..meta.clone()
            };
            stopped.save()?;
            return Err(anyhow!("rebase conflict: resolve the unmerged files"));
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
    if let Some(name) = &meta.head_name {
        move_reference(repo, name, current)?;
        attach_head(repo, name)?;
    }
    reset(root, &current.to_string(), ResetMode::Hard)?;
    meta.clear();
    Ok("rebased".to_string())
}

pub fn rebase_continue(root: &Path) -> Result<String> {
    let repo = discover(root)?;
    let Some(mut meta) = RebaseMeta::load(&repo)? else {
        // Interactive rebase state is owned by the git porcelain.
        return run(root, &["rebase", "--continue"]).map(|_| "rebase continuing".to_string());
    };
    let stopped = meta
        .stopped
        .ok_or_else(|| anyhow!("rebase has no stopped commit to continue"))?;
    let file = open_conflicted_index(&repo)?;
    let (state, _) = file.into_parts();
    if state
        .entries()
        .iter()
        .any(|entry| entry.stage() != Stage::Unconflicted)
    {
        anyhow::bail!("rebase still has unresolved conflicts; resolve them first");
    }
    if state
        .entries()
        .iter()
        .any(|entry| entry.mode == Mode::COMMIT)
    {
        anyhow::bail!("cannot continue rebase: the index contains submodules");
    }
    let tree = write_tree_from_index(&repo, &state)?;

    let stopped_commit = repo.find_commit(stopped)?;
    let message = stopped_commit.message_raw()?.to_string();
    let author = stopped_commit.author()?;
    let committer = repo
        .committer()
        .ok_or_else(|| anyhow!("Committer identity is not configured"))??;
    let current = repo.head_id()?.detach();
    let new_id = repo
        .new_commit_as(committer, author, message, tree, [current])?
        .id()
        .detach();
    detach_head(&repo, new_id)?;
    meta.stopped = None;
    meta.current = new_id;
    meta.save()?;
    rebase_replay(root, &repo, &meta)
}

pub fn rebase_abort(root: &Path) -> Result<String> {
    let repo = discover(root)?;
    let Some(meta) = RebaseMeta::load(&repo)? else {
        return run(root, &["rebase", "--abort"]).map(|_| "rebase aborted".to_string());
    };
    reset(root, &meta.orig_head.to_string(), ResetMode::Hard)?;
    if let Some(name) = &meta.head_name {
        attach_head(&repo, name)?;
    }
    meta.clear();
    Ok("rebase aborted".to_string())
}

pub fn rebase_skip(root: &Path) -> Result<String> {
    let repo = discover(root)?;
    let Some(meta) = RebaseMeta::load(&repo)? else {
        return run(root, &["rebase", "--skip"]).map(|_| "rebase skipped".to_string());
    };
    // Discard the stopped commit: restore the tree of the last replayed commit
    // so the conflict markers and stages are gone, then continue with the todo.
    reset(root, &meta.current.to_string(), ResetMode::Hard)?;
    rebase_replay(root, &repo, &meta)
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

// ---------------- sequencer helpers ----------------

fn git_file(repo: &gix::Repository, name: &str) -> std::path::PathBuf {
    repo.git_dir().join(name)
}

fn write_git_file(repo: &gix::Repository, name: &str, content: &str) -> Result<()> {
    lazecode_fs::file::write_atomic(&git_file(repo, name), content.as_bytes())
        .map_err(|error| anyhow!("cannot write {}: {error}", git_file(repo, name).display()))
}

fn read_git_file(repo: &gix::Repository, name: &str) -> Option<String> {
    std::fs::read_to_string(git_file(repo, name)).ok()
}

fn remove_git_file(repo: &gix::Repository, name: &str) {
    let _ = std::fs::remove_file(git_file(repo, name));
}

fn resolve_committish(repo: &gix::Repository, name: &str) -> Result<ObjectId> {
    repo.rev_parse_single(gix::bstr::BStr::new(name))
        .map(|id| id.detach())
        .map_err(|error| anyhow!("cannot resolve '{name}': {error}"))
}

fn tree_of(repo: &gix::Repository, commit: ObjectId) -> Result<ObjectId> {
    Ok(repo.find_commit(commit)?.tree_id()?.detach())
}

fn parent_tree_of(repo: &gix::Repository, commit: &gix::Commit<'_>) -> Result<ObjectId> {
    match commit.parent_ids().next() {
        Some(parent) => tree_of(repo, parent.detach()),
        None => Ok(repo.empty_tree().id),
    }
}

/// Write a merged, conflicted tree into the worktree and index: conflict
/// markers land in the files and stages 1/2/3 land in the index. `base_map` is
/// the tree that was checked out before the operation, used to delete paths the
/// merge drops. Everything is rolled back when any step fails.
fn write_conflict_state(
    repo: &gix::Repository,
    base_map: &TreeMap,
    merged_tree: ObjectId,
    conflicts: &[gix::merge::tree::Conflict],
) -> Result<()> {
    let workdir = workdir(repo)?;
    let merged = tree_map(repo, merged_tree)?;
    // A conflicted rebase can stop before the worktree was moved to the last
    // replayed commit, so consider both the base tree and the current index
    // when deciding which paths to delete.
    let index = open_conflicted_index(repo)?;
    let mut old_paths: BTreeSet<String> = base_map.keys().cloned().collect();
    for entry in index.entries() {
        old_paths.insert(entry.path(&index).to_string());
    }
    for path in merged.keys().chain(old_paths.iter()) {
        safe_worktree_join(&workdir, path)?;
    }
    let backup = CheckoutBackup::capture(repo, &workdir, old_paths.iter().chain(merged.keys()))?;
    let old_index = std::fs::read(repo.index_path()).ok();
    let result = (|| -> Result<()> {
        for path in &old_paths {
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
        materialize_tree(repo, &merged)?;
        let mut state = index_state_from_tree(repo, &merged);
        gix::merge::tree::apply_index_entries(
            conflicts,
            gix::merge::tree::TreatAsUnresolved::git(),
            &mut state,
            gix::merge::tree::apply_index_entries::RemovalMode::Prune,
        );
        write_index(gix::index::File::from_state(state, repo.index_path()))?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(restore_error) = backup.restore() {
            log::error!("conflict checkout rollback failed: {restore_error}");
        }
        restore_index(repo, old_index);
        return Err(error);
    }
    backup.discard();
    Ok(())
}

/// Commit whatever the user staged after resolving a cherry-pick/revert
/// conflict, preserving the original author for cherry-picks.
fn finish_sequencer(root: &Path, marker: &str, op: &str, preserve_author: bool) -> Result<String> {
    let repo = discover(root)?;
    // A multi-commit cherry-pick/revert sequence started by the git CLI keeps
    // its remaining todo in `.git/sequencer`; let the porcelain finish it.
    if repo.git_dir().join("sequencer").exists() {
        return run(root, &[op, "--continue"]).map(|_| format!("{op} continuing"));
    }
    let picked_text = read_git_file(&repo, marker).ok_or_else(|| anyhow!("no {op} in progress"))?;
    let picked = ObjectId::from_hex(picked_text.trim().as_bytes())
        .map_err(|_| anyhow!("{op} state is corrupt: {marker} is not a commit id"))?;
    let file = open_conflicted_index(&repo)?;
    let (state, _) = file.into_parts();
    if state
        .entries()
        .iter()
        .any(|entry| entry.stage() != Stage::Unconflicted)
    {
        anyhow::bail!("{op} still has unresolved conflicts; resolve them first");
    }
    if state
        .entries()
        .iter()
        .any(|entry| entry.mode == Mode::COMMIT)
    {
        anyhow::bail!("cannot continue {op}: the index contains submodules");
    }
    let tree = write_tree_from_index(&repo, &state)?;

    let picked_commit = repo.find_commit(picked)?;
    let message = read_git_file(&repo, "MERGE_MSG")
        .map(|text| text.trim_end().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| picked_commit.message_raw().unwrap_or_default().to_string());
    let head = repo.head_id()?.detach();
    let commit = if preserve_author {
        let committer = repo
            .committer()
            .ok_or_else(|| anyhow!("Committer identity is not configured"))??;
        repo.new_commit_as(committer, picked_commit.author()?, message, tree, [head])?
    } else {
        repo.new_commit(message, tree, [head])?
    };
    update_head(&repo, commit.id().detach(), op)?;
    remove_git_file(&repo, marker);
    remove_git_file(&repo, "MERGE_MSG");
    Ok(format!("{op} finished"))
}

fn abort_sequencer(root: &Path, marker: &str, op: &str) -> Result<String> {
    let repo = discover(root)?;
    if repo.git_dir().join("sequencer").exists() {
        return run(root, &[op, "--abort"]).map(|_| format!("{op} aborted"));
    }
    if !git_file(&repo, marker).exists() {
        anyhow::bail!("no {op} in progress");
    }
    let orig = read_git_file(&repo, "ORIG_HEAD")
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| anyhow!("cannot abort {op}: ORIG_HEAD is missing"))?;
    reset(root, &orig, ResetMode::Hard)?;
    remove_git_file(&repo, marker);
    remove_git_file(&repo, "MERGE_MSG");
    Ok(format!("{op} aborted"))
}

// ---------------- cherry-pick / revert ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayKind {
    Pick,
    Revert,
}

impl ReplayKind {
    fn op(self) -> &'static str {
        match self {
            ReplayKind::Pick => "cherry-pick",
            ReplayKind::Revert => "revert",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            ReplayKind::Pick => "CHERRY_PICK_HEAD",
            ReplayKind::Revert => "REVERT_HEAD",
        }
    }
}

pub fn cherry_pick(root: &Path, hash: &str) -> Result<String> {
    require_clean(root)?;
    let hash = validated_hash(hash)?;
    replay_commit(root, hash, ReplayKind::Pick)
}

pub fn cherry_pick_continue(root: &Path) -> Result<String> {
    finish_sequencer(root, "CHERRY_PICK_HEAD", "cherry-pick", true)
}

pub fn cherry_pick_abort(root: &Path) -> Result<String> {
    abort_sequencer(root, "CHERRY_PICK_HEAD", "cherry-pick")
}

pub fn revert(root: &Path, hash: &str) -> Result<String> {
    require_clean(root)?;
    let hash = validated_hash(hash)?;
    replay_commit(root, hash, ReplayKind::Revert)
}

pub fn revert_continue(root: &Path) -> Result<String> {
    finish_sequencer(root, "REVERT_HEAD", "revert", false)
}

pub fn revert_abort(root: &Path) -> Result<String> {
    abort_sequencer(root, "REVERT_HEAD", "revert")
}

fn replay_commit(root: &Path, hash: &str, kind: ReplayKind) -> Result<String> {
    let repo = discover(root)?;
    let picked = resolve_committish(&repo, hash)?;
    let commit = repo.find_commit(picked)?;
    if commit.parent_ids().count() > 1 {
        anyhow::bail!(
            "cannot {} merge commit {hash}: a mainline parent is required",
            kind.op()
        );
    }
    let head = repo.head_id()?.detach();
    let head_tree = tree_of(&repo, head)?;
    let commit_tree = commit.tree_id()?.detach();
    let parent_tree = parent_tree_of(&repo, &commit)?;
    let (ancestor, theirs) = match kind {
        ReplayKind::Pick => (parent_tree, commit_tree),
        ReplayKind::Revert => (commit_tree, parent_tree),
    };

    let raw_message = commit.message_raw()?.to_string();
    let message = match kind {
        ReplayKind::Pick => raw_message.clone(),
        ReplayKind::Revert => {
            let subject = raw_message.lines().next().unwrap_or("").trim();
            format!("Revert \"{subject}\"\n\nThis reverts commit {picked}.\n")
        }
    };

    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: None,
        current: Some(gix::bstr::BStr::new("HEAD")),
        other: Some(gix::bstr::BStr::new(kind.op())),
    };
    let how = gix::merge::tree::TreatAsUnresolved::git();
    let mut outcome = repo.merge_trees(
        ancestor,
        head_tree,
        theirs,
        labels,
        repo.tree_merge_options()?,
    )?;
    let unresolved = outcome.has_unresolved_conflicts(how);
    let tree_id = outcome.tree.write()?.detach();

    // Git records the pre-operation tip for both `--abort` and reflog clarity.
    write_git_file(&repo, "ORIG_HEAD", &format!("{head}\n"))?;

    if unresolved {
        let base_map = tree_map(&repo, head_tree)?;
        write_conflict_state(&repo, &base_map, tree_id, &outcome.conflicts)?;
        write_git_file(&repo, kind.marker(), &format!("{picked}\n"))?;
        write_git_file(&repo, "MERGE_MSG", &message)?;
        return Err(anyhow!(
            "{} conflict: resolve the unmerged files",
            kind.op()
        ));
    }

    let new_commit = match kind {
        ReplayKind::Pick => {
            let committer = repo
                .committer()
                .ok_or_else(|| anyhow!("Committer identity is not configured"))??;
            repo.new_commit_as(committer, commit.author()?, message, tree_id, [head])?
        }
        ReplayKind::Revert => repo.new_commit(message, tree_id, [head])?,
    };
    let new_id = new_commit.id().detach();
    reset(root, &new_id.to_string(), ResetMode::Hard)?;
    Ok(match kind {
        ReplayKind::Pick => "picked".to_string(),
        ReplayKind::Revert => "reverted".to_string(),
    })
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
    // `reset` is also how an aborted cherry-pick/rebase restores the pre-
    // operation state, so unmerged stages must be accepted here: they are
    // discarded along with the rest of the old index.
    let index = open_conflicted_index(&repo)?;
    if index
        .entries()
        .iter()
        .any(|entry| entry.mode == gix::index::entry::Mode::COMMIT)
    {
        anyhow::bail!("cannot reset while the index contains submodules");
    }
    let mut old_paths: BTreeSet<String> = BTreeSet::new();
    for entry in index.entries() {
        old_paths.insert(entry.path(&index).to_string());
    }
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
            for path in target_map.keys().chain(old_paths.iter()) {
                safe_worktree_join(&workdir, path)?;
            }
            let backup = CheckoutBackup::capture(
                &repo,
                &workdir,
                old_paths.iter().chain(target_map.keys()),
            )?;
            let old_index = std::fs::read(repo.index_path()).ok();
            let result = (|| -> Result<()> {
                for path in &old_paths {
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
            // index.lock semantics: a concurrent stage/commit job must not be
            // silently overwritten by this rollback.
            if let Err(error) = lazecode_fs::file::write_atomic_locked(&repo.index_path(), &bytes) {
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

/// One conflicted path, reconstructed from the index's unmerged stages.
#[derive(Clone, Debug)]
pub struct ConflictEntry {
    pub path: String,
    /// Git's two-letter status: `UU`, `AA`, `UD`, `DU` or `DD`.
    pub kind: Option<String>,
}

fn conflict_kind(stages: [bool; 3]) -> &'static str {
    match stages {
        [_, true, true] if stages[0] => "UU",
        [_, true, true] => "AA",
        [true, true, false] => "UD",
        [true, false, true] => "DU",
        [true, false, false] => "DD",
        _ => "UU",
    }
}

fn index_conflicts(index: &gix::index::State) -> BTreeMap<String, [bool; 3]> {
    let mut conflicts: BTreeMap<String, [bool; 3]> = BTreeMap::new();
    for entry in index.entries() {
        let slot = match entry.stage() {
            Stage::Base => 0,
            Stage::Ours => 1,
            Stage::Theirs => 2,
            Stage::Unconflicted => continue,
        };
        conflicts
            .entry(entry.path(index).to_string())
            .or_insert([false; 3])[slot] = true;
    }
    conflicts
}

pub fn conflict_list(root: &Path) -> Result<Vec<ConflictEntry>> {
    let repo = discover(root)?;
    let index = repo.index_or_empty()?;
    Ok(index_conflicts(&index)
        .into_iter()
        .map(|(path, stages)| ConflictEntry {
            path,
            kind: Some(conflict_kind(stages).to_string()),
        })
        .collect())
}

/// Resolve a conflicted path by taking one side wholesale. For delete/modify
/// conflicts the requested side has no index stage, so the file must be
/// removed from the worktree and index instead of checked out empty.
pub fn resolve_take(root: &Path, path: &str, ours: bool) -> Result<()> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let relative = crate::ops::validate_repo_path(path)?;
    let path = relative.to_string_lossy().into_owned();
    let stage = if ours { Stage::Ours } else { Stage::Theirs };

    let file = open_conflicted_index(&repo)?;
    let index_path = file.path().to_path_buf();
    let (mut state, _) = file.into_parts();
    let conflicted = state.entries().iter().any(|entry| {
        entry.stage() != Stage::Unconflicted
            && entry.path(&state).to_str().ok() == Some(path.as_str())
    });
    if !conflicted {
        anyhow::bail!("{path} is not conflicted");
    }
    let side = state
        .entry_by_path_and_stage(path.as_str().into(), stage)
        .map(|entry| (entry.id, entry.mode, entry.stat));
    if let Some((_, mode, _)) = side {
        if mode == Mode::COMMIT {
            anyhow::bail!("cannot resolve {path}: submodule entries are not supported");
        }
    }
    state.remove_entries(|_, candidate, _| candidate.to_str().ok() == Some(path.as_str()));

    match side {
        Some((id, mode, _)) => {
            let mut map = TreeMap::new();
            map.insert(
                path.clone(),
                TreeEntry {
                    oid: id,
                    mode: mode.bits(),
                },
            );
            materialize_tree(&repo, &map)?;
            let absolute = safe_worktree_join(&workdir, &path)?;
            let stat = gix::index::fs::Metadata::from_path_no_follow(&absolute)
                .ok()
                .and_then(|metadata| Stat::from_fs(&metadata).ok())
                .unwrap_or_default();
            state.dangerously_push_entry(stat, id, Flags::empty(), mode, path.as_str().into());
        }
        None => {
            let absolute = safe_worktree_join(&workdir, &path)?;
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
    }

    state.remove_tree();
    state.sort_entries();
    write_index(gix::index::File::from_state(state, index_path))?;
    Ok(())
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
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let relative = crate::ops::validate_repo_path(path)?;
    let path = relative.to_string_lossy().into_owned();
    let index = repo.index_or_empty()?;
    let read_stage = |stage: Stage| -> Result<Option<String>> {
        match index.entry_by_path_and_stage(path.as_str().into(), stage) {
            Some(entry) => Ok(Some(
                String::from_utf8_lossy(&blob_bytes(&repo, entry.id)?).into_owned(),
            )),
            None => Ok(None),
        }
    };
    let ours = read_stage(Stage::Ours)?.unwrap_or_default();
    let theirs = read_stage(Stage::Theirs)?.unwrap_or_default();
    let base = read_stage(Stage::Base)?;
    let full = safe_worktree_join(&workdir, &path)?;
    let worktree = std::fs::read(&full)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    Ok(MergeFile3Way {
        path,
        ours,
        theirs,
        base,
        worktree,
    })
}

pub fn mark_conflict_resolved(root: &Path, path: &str, content: Option<&str>) -> Result<()> {
    let relative = crate::ops::validate_repo_path(path)?;
    let path = relative.to_string_lossy().into_owned();
    if let Some(content) = content {
        let repo = discover(root)?;
        let workdir = workdir(&repo)?;
        let full = safe_worktree_join(&workdir, &path)?;
        if let Ok(metadata) = std::fs::symlink_metadata(&full) {
            if !metadata.is_file() {
                std::fs::remove_file(&full)?;
            }
        }
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, content.as_bytes())?;
    }
    crate::ops::stage_resolving_conflicts(root, &[path])
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

/// Checkout an arbitrary ref (tag, branch, commit) detached. Unlike
/// `checkout_revision`, this accepts human-readable ref names but resolves
/// them to a commit hash before invoking checkout, so option-like input can
/// never reach the Git command line.
pub fn checkout_ref(root: &Path, spec: &str) -> Result<String> {
    let spec = spec.trim();
    if spec.is_empty() || spec.starts_with('-') {
        anyhow::bail!("unsuitable revision '{spec}'");
    }
    require_clean(root)?;
    let resolved = run(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{spec}^{{commit}}"),
        ],
    )?;
    let hash = resolved.trim().lines().next().unwrap_or("");
    validated_hash(hash)?;
    let output = builder(root, &["checkout", "--detach", hash])
        .output()
        .map_err(|e| anyhow!("git checkout failed: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!("checked out {spec}"))
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
