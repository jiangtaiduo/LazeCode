//! Changelist persistence: JSON under `.git/lazecode-changelists.json`,
//! mapping each file path to a named changelist. Port of main's
//! `.git/ide-changelists.json` shape: `{ "lists": ["Default"], "files":
//! { "<path>": "Default" } }`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "lazecode-changelists.json";
pub const DEFAULT_LIST: &str = "Default";

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct Stored {
    lists: Vec<String>,
    files: BTreeMap<String, String>,
}

pub struct Changelists {
    git_dir: PathBuf,
    data: Stored,
}

impl Changelists {
    pub fn open(root: &Path) -> Self {
        let git_dir = match crate::discover(root) {
            // 关联 worktree / submodule 的 `.git` 是文件而非目录，必须经
            // discover 解析出真实 git-dir；直接拼 `.git` 会让这些仓库的
            // 变更列表静默失效（shelf.rs 同样遵循此约定）。
            Ok(repo) => repo.git_dir().to_path_buf(),
            Err(_) => root.join(".git"),
        };
        let path = git_dir.join(FILE_NAME);
        let mut data: Stored = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(data) => data,
                Err(_) => {
                    // Keep the unreadable file for the user instead of
                    // silently overwriting their changelist assignments.
                    let backup = git_dir.join(format!("{FILE_NAME}.corrupt.bak"));
                    let _ = std::fs::rename(&path, backup);
                    Stored::default()
                }
            },
            Err(_) => Stored::default(),
        };
        if data.lists.is_empty() {
            data.lists.push(DEFAULT_LIST.to_string());
        }
        Self { git_dir, data }
    }

    pub fn lists(&self) -> &[String] {
        &self.data.lists
    }

    pub fn assign(&mut self, path: &str, list: &str) -> anyhow::Result<()> {
        self.data.files.insert(path.to_string(), list.to_string());
        self.flush()
    }

    pub fn create_list(&mut self, name: &str) -> anyhow::Result<()> {
        let name = name.trim();
        if !name.is_empty() && !self.data.lists.iter().any(|existing| existing == name) {
            self.data.lists.push(name.to_string());
            self.flush()?;
        }
        Ok(())
    }

    /// `create_list` 的可判错版本：重名/空名返回 `Err`，供需要把失败
    /// 原因回显给用户的调用方（如 Unshelve 目标列表）使用。
    pub fn create_list_checked(&mut self, name: &str) -> anyhow::Result<()> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("empty changelist name");
        }
        if self.data.lists.iter().any(|existing| existing == name) {
            anyhow::bail!("changelist '{name}' already exists");
        }
        self.create_list(name)
    }

    /// Batch-assign many paths in one flush (unshelve-to-changelist path).
    pub fn assign_many(&mut self, paths: &[String], list: &str) -> anyhow::Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        for path in paths {
            self.data.files.insert(path.clone(), list.to_string());
        }
        self.flush()
    }

    /// Removes a custom list; the files that belonged to it fall back to
    /// `Default` (implicit membership). Returns the paths that were re-homed.
    pub fn remove_list(&mut self, name: &str) -> anyhow::Result<Vec<String>> {
        let orphans = self.files_of(name);
        if remove_list(&mut self.data, name) {
            self.flush()?;
        }
        Ok(orphans)
    }

    pub fn files_of(&self, list: &str) -> Vec<String> {
        files_of(&self.data.files, list)
    }

    pub fn list_of(&self, path: &str) -> String {
        self.data
            .files
            .get(path)
            .cloned()
            .unwrap_or_else(|| DEFAULT_LIST.to_string())
    }

    fn flush(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.git_dir)?;
        let text = serde_json::to_string_pretty(&self.data)?;
        lazecode_fs::file::write_atomic(&self.git_dir.join(FILE_NAME), text.as_bytes())?;
        Ok(())
    }
}

/// Paths assigned to `list`. Untracked files belong to `Default`.
fn files_of(files: &BTreeMap<String, String>, list: &str) -> Vec<String> {
    files
        .iter()
        .filter(|&(_, assigned)| assigned == list)
        .map(|(path, _)| path.clone())
        .collect()
}

fn remove_list(data: &mut Stored, name: &str) -> bool {
    if name == DEFAULT_LIST {
        return false;
    }
    let before = data.lists.len();
    data.lists.retain(|existing| existing != name);
    if data.lists.len() != before {
        data.files.retain(|_, assigned| assigned != name);
        true
    } else {
        false
    }
}
