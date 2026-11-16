//! Linked worktree management on `gix`, keyed by checkout path with the main
//! worktree first.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::anyhow;

use crate::model::WorktreeInfo;
use crate::{discover, Result};

/// Real path resolved through symlinks (falls back to the input).
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn branch_of_worktree(base: &Path) -> Option<String> {
    // Only HEAD is read here; a full snapshot per member made
    // `list_worktrees` cost one snapshot per worktree.
    crate::discover(base)
        .ok()?
        .head()
        .ok()?
        .referent_name()
        .map(|name| name.shorten().to_string())
}

/// Return the path of the repository's main worktree.
pub fn main_worktree(root: &Path) -> Result<PathBuf> {
    let repo = discover(root)?;
    let base = repo
        .main_repo()?
        .workdir()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("bare repositories are not supported"))?;
    Ok(canonical(&base))
}

/// List the repository's worktrees: main first, linked ones sorted by name.
pub fn list_worktrees(root: &Path) -> Result<Vec<WorktreeInfo>> {
    let repo = discover(root)?;
    let main = main_worktree(root)?;
    let main_branch = branch_of_worktree(&main);

    let mut linked: Vec<WorktreeInfo> = Vec::new();
    for proxy in repo.worktrees()? {
        let base = match proxy.base() {
            Ok(base) => base,
            Err(_) => continue, // prunable worktree; skip, matching `git worktree list`
        };
        let name = String::from_utf8_lossy(proxy.id()?).into_owned();
        linked.push(WorktreeInfo {
            name,
            path: canonical(&base),
            branch: branch_of_worktree(&base),
            locked: proxy.is_locked(),
            prunable: proxy.is_prunable(),
        });
    }

    let mut worktrees = vec![WorktreeInfo {
        name: "main".to_string(),
        path: main,
        branch: main_branch,
        locked: false,
        prunable: false,
    }];
    linked.sort_by(|a, b| a.name.cmp(&b.name));
    worktrees.extend(linked);
    Ok(worktrees)
}

/// Whether `root` is a linked worktree (not the main checkout).
pub fn is_linked_worktree(root: &Path) -> Result<bool> {
    Ok(discover(root)?.kind() == gix::repository::Kind::LinkedWorkTree)
}

/// Create a linked worktree at `destination` for `branch`, which must exist
/// and be unchecked out elsewhere; partial checkouts roll back.
pub fn add_worktree(root: &Path, destination: &Path, branch: &str) -> Result<PathBuf> {
    let repo = discover(root)?;
    let full = format!("refs/heads/{branch}");
    let head = gix::worktree::add::Head::Attached(
        full.as_str()
            .try_into()
            .map_err(|err| anyhow!("invalid branch '{branch}': {err}"))?,
    );
    let interrupt = AtomicBool::new(false);
    let (wt_repo, _) = repo.add_worktree(destination, head, gix::progress::Discard, &interrupt)?;
    Ok(canonical(
        wt_repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("created worktree has no workdir"))?
            .as_path(),
    ))
}

/// Create a linked worktree with a detached HEAD at the repository's current HEAD.
pub fn add_worktree_detached_at_head(root: &Path, destination: &Path) -> Result<PathBuf> {
    let repo = discover(root)?;
    let head_id = repo.head_id()?.detach();
    let head = gix::worktree::add::Head::Detached(head_id);
    let interrupt = AtomicBool::new(false);
    let (wt_repo, _) = repo.add_worktree(destination, head, gix::progress::Discard, &interrupt)?;
    Ok(canonical(
        wt_repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("created worktree has no workdir"))?
            .as_path(),
    ))
}

/// Remove a linked worktree; without `force` a dirty or locked one is refused.
pub fn remove_worktree(root: &Path, target: &Path, force: bool) -> Result<()> {
    let repo = discover(root)?;
    let level = if force {
        gix::worktree::remove::Force::DiscardChanges
    } else {
        gix::worktree::remove::Force::Never
    };
    repo.remove_worktree(target, level, gix::progress::Discard)
        .map_err(|err| anyhow!("cannot remove worktree: {err}"))
}

/// Drop stale worktree registrations whose checkouts no longer exist.
pub fn prune_worktrees(root: &Path) -> Result<usize> {
    let repo = discover(root)?;
    let prunable: Vec<PathBuf> = repo
        .worktrees()?
        .into_iter()
        .filter(|proxy| proxy.is_prunable())
        .filter_map(|proxy| proxy.base().ok())
        .collect();
    let main = main_worktree(root)?;
    for target in prunable.clone() {
        remove_worktree(&main, &target, true)?;
    }
    Ok(prunable.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.name", "Tester"]);
        git(dir.path(), &["config", "user.email", "tester@example.com"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);
        dir
    }

    #[test]
    fn lists_main_and_linked_worktrees() {
        let dir = init();
        let wt_path = dir.path().join("wt-feat");
        git(dir.path(), &["worktree", "add", "-b", "feat", "wt-feat"]);

        let listed = list_worktrees(dir.path()).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "main");
        assert_eq!(listed[0].branch.as_deref(), Some("main"));
        assert!(!listed[0].locked);
        let linked = &listed[1];
        assert_eq!(linked.name, "wt-feat");
        assert_eq!(linked.branch.as_deref(), Some("feat"));

        assert!(is_linked_worktree(linked.path.as_path()).unwrap());
        assert!(!is_linked_worktree(dir.path()).unwrap());

        assert_eq!(
            main_worktree(dir.path()).unwrap(),
            dir.path().canonicalize().unwrap()
        );
        assert!(dir.path().canonicalize().unwrap().is_dir()); // sanity
        assert_eq!(
            main_worktree(wt_path.as_path()).unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn add_worktree_creates_checkout() {
        let dir = init();
        let destination = dir.path().join("new-wt");
        git(dir.path(), &["branch", "side-branch"]);
        let path = add_worktree(dir.path(), &destination, "side-branch").unwrap();
        assert!(path.join("a.txt").is_file());
        assert!(is_linked_worktree(&path).unwrap());

        let listed = list_worktrees(dir.path()).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed
            .iter()
            .any(|info| info.path == destination.canonicalize().unwrap()));
    }

    #[test]
    fn add_worktree_rejects_checked_out_branch() {
        let dir = init();
        let err = add_worktree(dir.path(), &dir.path().join("dup"), "main").unwrap_err();
        assert!(
            err.to_string().contains("already checked out")
                || err.to_string().contains("Checked out")
                || err.to_string().contains("checked out"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn multiple_worktrees_require_distinct_branches() {
        let dir = init();
        git(dir.path(), &["branch", "side"]);
        git(dir.path(), &["branch", "feat"]);
        let side = dir.path().join("wt-side");
        let feat = dir.path().join("wt-feat");
        add_worktree(dir.path(), &side, "side").unwrap();
        add_worktree(dir.path(), &feat, "feat").unwrap();

        let listed = list_worktrees(dir.path()).unwrap();
        assert_eq!(listed.len(), 3);
        let branches: Vec<_> = listed
            .iter()
            .filter_map(|wt| wt.branch.as_deref())
            .collect();
        assert!(branches.contains(&"main"));
        assert!(branches.contains(&"side"));
        assert!(branches.contains(&"feat"));

        for branch in ["main", "side", "feat"] {
            let destination = dir.path().join(format!("reuse-{branch}"));
            let err = add_worktree(dir.path(), &destination, branch).unwrap_err();
            assert!(
                err.to_string().contains("already checked out")
                    || err.to_string().contains("Checked out")
                    || err.to_string().contains("checked out"),
                "branch {branch} was reused: {err}"
            );
            assert!(!destination.exists(), "failed add left a checkout behind");
        }
    }

    #[test]
    fn remove_worktree_removes_registration() {
        let dir = init();
        let destination = dir.path().join("temp-wt");
        git(dir.path(), &["branch", "side-branch"]);
        add_worktree(dir.path(), &destination, "side-branch").unwrap();

        remove_worktree(dir.path(), &destination, false).unwrap();
        assert!(!destination.exists());
        assert_eq!(list_worktrees(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn remove_dirty_worktree_requires_force_and_then_discards() {
        let dir = init();
        let destination = dir.path().join("dirty-wt");
        git(dir.path(), &["branch", "dirty-branch"]);
        let path = add_worktree(dir.path(), &destination, "dirty-branch").unwrap();
        std::fs::write(path.join("a.txt"), "uncommitted\n").unwrap();
        std::fs::write(path.join("untracked.txt"), "scratch\n").unwrap();

        let err = remove_worktree(dir.path(), &path, false).unwrap_err();
        assert!(
            err.to_string().contains("modified")
                || err.to_string().contains("untracked")
                || err.to_string().contains("changes")
                || err.to_string().contains("dirty"),
            "unexpected error: {err}"
        );
        assert!(path.exists(), "non-forced removal deleted the checkout");
        assert_eq!(list_worktrees(dir.path()).unwrap().len(), 2);

        remove_worktree(dir.path(), &path, true).unwrap();
        assert!(!path.exists());
        assert_eq!(list_worktrees(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn add_worktree_rejects_occupied_destination_without_registration() {
        let dir = init();
        git(dir.path(), &["branch", "side-branch"]);
        let destination = dir.path().join("occupied");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("keep.txt"), "keep\n").unwrap();

        let err = add_worktree(dir.path(), &destination, "side-branch").unwrap_err();
        assert!(!err.to_string().is_empty());
        assert_eq!(list_worktrees(dir.path()).unwrap().len(), 1);
        assert_eq!(
            std::fs::read_to_string(destination.join("keep.txt")).unwrap(),
            "keep\n"
        );
    }

    #[test]
    fn prune_removes_registration_when_checkout_vanished() {
        let dir = init();
        git(dir.path(), &["branch", "side-branch"]);
        let destination = dir.path().join("gone-wt");
        add_worktree(dir.path(), &destination, "side-branch").unwrap();
        std::fs::remove_dir_all(&destination).unwrap();

        assert_eq!(prune_worktrees(dir.path()).unwrap(), 1);
        let listed = list_worktrees(dir.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "main");
    }
}
