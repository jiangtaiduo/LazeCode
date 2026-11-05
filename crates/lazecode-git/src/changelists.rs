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
    root: PathBuf,
    data: Stored,
}

impl Changelists {
    pub fn open(root: &Path) -> Self {
        let path = root.join(".git").join(FILE_NAME);
        let data: Stored = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let mut data = data;
        if data.lists.is_empty() {
            data.lists.push(DEFAULT_LIST.to_string());
        }
        Self {
            root: root.to_path_buf(),
            data,
        }
    }

    pub fn lists(&self) -> &[String] {
        &self.data.lists
    }

    pub fn assign(&mut self, path: &str, list: &str) {
        self.data.files.insert(path.to_string(), list.to_string());
        self.flush();
    }

    pub fn create_list(&mut self, name: &str) {
        let name = name.trim();
        if !name.is_empty() && !self.data.lists.iter().any(|existing| existing == name) {
            self.data.lists.push(name.to_string());
            self.flush();
        }
    }

    /// Removes a custom list; the files that belonged to it fall back to
    /// `Default` (implicit membership). Returns the paths that were re-homed.
    pub fn remove_list(&mut self, name: &str) -> Vec<String> {
        let orphans = self.files_of(name);
        if remove_list(&mut self.data, name) {
            self.flush();
        }
        orphans
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

    fn flush(&self) {
        let dir = self.root.join(".git");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.data) {
            let _ = lazecode_fs::file::write_atomic(&dir.join(FILE_NAME), text.as_bytes());
        }
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
