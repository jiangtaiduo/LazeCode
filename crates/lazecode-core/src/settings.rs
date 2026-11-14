use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    English,
    Chinese,
}

impl Language {
    pub fn is_zh(self) -> bool {
        matches!(self, Language::Chinese)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Editor font family. Empty means the built-in monospace stack.
    pub font_family: String,
    pub font_size: f32,
    pub tab_width: usize,
    pub use_spaces: bool,
    pub show_line_numbers: bool,
    pub highlight_current_line: bool,
    pub show_minimap: bool,
    pub auto_save: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 13.5,
            tab_width: 4,
            use_spaces: true,
            show_line_numbers: true,
            highlight_current_line: true,
            show_minimap: true,
            auto_save: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    /// Terminal font family. Empty means "Match editor" when the editor
    /// has a custom family, otherwise the built-in monospace stack.
    pub font_family: String,
    pub font_size: f32,
    pub scrollback: usize,
    /// Maximum number of live agent consoles across every project and
    /// worktree. OMP sessions are large (hundreds of MB each); the cap keeps
    /// a multi-worktree workspace from exhausting physical memory.
    pub max_agent_consoles: usize,
    /// `None` means use `$SHELL` (or /bin/zsh).
    pub shell: Option<String>,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_family: String::new(),
            font_size: 13.0,
            scrollback: 10_000,
            max_agent_consoles: 20,
            shell: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AgentProfile {
    pub name: String,
    pub description: String,
    /// Shell command executed in a new agent console. Empty opens the
    /// user's login shell.
    pub command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub options: Vec<AgentProfile>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            options: vec![
                AgentProfile {
                    name: "Claude Code".into(),
                    description: "Anthropic's terminal agent".into(),
                    command: "claude".into(),
                },
                AgentProfile {
                    name: "OpenCode".into(),
                    description: "Open-source terminal agent".into(),
                    command: "opencode".into(),
                },
                AgentProfile {
                    name: "omp".into(),
                    description: "omp terminal coding agent".into(),
                    command: "omp".into(),
                },
                AgentProfile {
                    name: "Shell".into(),
                    description: "Start a plain login shell".into(),
                    command: String::new(),
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    pub language: Language,
    /// Application UI font family. Empty means the language-aware system
    /// stack (SF Pro for English, PingFang SC first for Chinese).
    pub ui_font: String,
    /// Optional language-server integration for the 11 supported languages.
    /// A missing or crashing server silently degrades to syntax highlighting.
    pub lsp_enabled: bool,
    pub editor: EditorSettings,
    pub terminal: TerminalSettings,
    pub agents: AgentSettings,
    pub sidebar_width: f32,
    /// Commit 抽屉宽度（稳定版 ide_commit_drawer_width，默认 330，240-640）。
    pub commit_drawer_width: f32,
    /// Compare Versions 抽屉宽度（默认 320，240-640）。
    pub compare_drawer_width: f32,
    pub panel_height: f32,
    /// 分支比较页上下两个 pane 的高度占比（left pane，20-80）。
    pub compare_split: f32,
    /// 分支比较页详情列宽度占比（两侧共用，24-62）。
    pub compare_details_width: f32,
    /// 最近提交信息历史（本机级，最新在前，上限 10 条；对齐稳定版的
    /// useCommitMessageHistory：提交成功后入史、历史项可回填可清除）。
    pub commit_history: Vec<String>,
    pub ignored_patterns: Vec<String>,
    /// Update manifest endpoint (HTTPS URL, `file://` URL or local path).
    /// Defaults to the signed `latest.json` published on the repository's
    /// main branch; the manifest is signature-verified before anything is
    /// downloaded.
    pub update_endpoint: String,
    /// Pinned Ed25519 public key for the update manifest, base64 or hex.
    /// Shipped with the release signing key's public half so updates verify
    /// out of the box; operators can repin their own key.
    pub update_public_key: String,
    /// Check the endpoint once shortly after startup (the check is one small
    /// manifest fetch; download and install always need the user's click in
    /// the update dialog).
    pub auto_check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            language: Language::English,
            ui_font: String::new(),
            lsp_enabled: false,
            editor: EditorSettings::default(),
            terminal: TerminalSettings::default(),
            agents: AgentSettings::default(),
            sidebar_width: 262.0,
            commit_drawer_width: 330.0,
            compare_drawer_width: 320.0,
            panel_height: 248.0,
            compare_split: 40.0,
            compare_details_width: 36.0,
            commit_history: Vec::new(),
            ignored_patterns: vec![
                ".git".into(),
                ".idea".into(),
                ".DS_Store".into(),
                "node_modules".into(),
                ".venv".into(),
                "__pycache__".into(),
                "target".into(),
            ],
            update_endpoint:
                "https://raw.githubusercontent.com/jiangtaiduo/LazeCode/main/latest.json"
                    .to_string(),
            update_public_key: "QpmhblRgWl9yUHzvNbbRcffNYA/qKNO4g1aBSVU18b0=".to_string(),
            auto_check_updates: true,
        }
    }
}

impl Settings {
    /// Push a successful commit message into the history (dedupe, cap 10).
    pub fn remember_commit_message(&mut self, message: &str) {
        let trimmed = message.trim();
        if trimmed.is_empty() {
            return;
        }
        self.commit_history.retain(|existing| existing != trimmed);
        self.commit_history.insert(0, trimmed.to_string());
        self.commit_history.truncate(10);
    }

    /// `true` when both the endpoint and the pinned signing key are set.
    /// The UI disables every updater control while this is `false`.
    pub fn updates_configured(&self) -> bool {
        !self.update_endpoint.trim().is_empty() && !self.update_public_key.trim().is_empty()
    }

    pub fn config_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("LazeCode")
    }

    pub fn path() -> PathBuf {
        Self::config_dir().join("settings.toml")
    }

    pub fn load() -> (Self, Option<String>) {
        let path = Self::path();
        if !path.exists() {
            return (Self::default(), None);
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Settings>(&text) {
                Ok(mut settings) => {
                    settings.normalise();
                    (settings, None)
                }
                Err(err) => (
                    Self::default(),
                    Some(format!("settings.toml is invalid ({err}); using defaults")),
                ),
            },
            Err(err) => (
                Self::default(),
                Some(format!("cannot read settings.toml ({err}); using defaults")),
            ),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let text = toml::to_string_pretty(self)?;
        let path = Self::path();
        lazecode_fs::file::write_atomic(&path, text.as_bytes())
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(())
    }

    fn normalise(&mut self) {
        self.ui_font = self.ui_font.trim().to_string();
        self.editor.font_family = self.editor.font_family.trim().to_string();
        self.terminal.font_family = self.terminal.font_family.trim().to_string();
        self.editor.font_size = self.editor.font_size.clamp(9.0, 24.0);
        self.editor.tab_width = self.editor.tab_width.clamp(1, 8);
        self.terminal.font_size = self.terminal.font_size.clamp(9.0, 24.0);
        self.terminal.scrollback = self.terminal.scrollback.clamp(1_000, 200_000);
        self.terminal.max_agent_consoles = self.terminal.max_agent_consoles.clamp(1, 100);
        self.sidebar_width = self.sidebar_width.clamp(180.0, 520.0);
        self.commit_drawer_width = self.commit_drawer_width.clamp(240.0, 640.0);
        self.compare_drawer_width = self.compare_drawer_width.clamp(240.0, 640.0);
        self.panel_height = self.panel_height.clamp(120.0, 900.0);
        self.compare_split = self.compare_split.clamp(20.0, 80.0);
        self.compare_details_width = self.compare_details_width.clamp(24.0, 62.0);
        self.update_endpoint = self.update_endpoint.trim().to_string();
        self.update_public_key = self.update_public_key.trim().to_string();
        for option in &mut self.agents.options {
            option.name = option.name.trim().to_string();
            option.description = option.description.trim().to_string();
            option.command = option.command.trim().to_string();
        }
        self.agents.options.retain(|option| !option.name.is_empty());
        self.agents.options.truncate(12);
        if self.agents.options.is_empty() {
            self.agents = AgentSettings::default();
        }
    }
}

/// Layout and recent-project state, persisted next to settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceFile {
    pub recent: Vec<PathBuf>,
    pub recent_files: Vec<PathBuf>,
    pub projects: Vec<PathBuf>,
    pub active_project: Option<PathBuf>,
    /// Files in the singleton ad-hoc editor workspace, in tab order.
    pub single_files: Vec<PathBuf>,
    pub active_single_file: Option<PathBuf>,
    pub single_files_active: bool,
    /// Focused worktree name per project root.
    pub active_worktrees: std::collections::BTreeMap<PathBuf, String>,
    pub sidebar_view: Option<String>,
    pub sidebar_visible: Option<bool>,
    pub sidebar_width: Option<f32>,
    pub commit_drawer_width: Option<f32>,
    pub panel_visible: Option<bool>,
    pub panel_height: Option<f32>,
    pub terminal_placement: Option<String>,
    pub terminal_width: Option<f32>,
    pub terminal_height: Option<f32>,
    pub open_tabs: std::collections::BTreeMap<PathBuf, Vec<PathBuf>>,
    pub active_tabs: std::collections::BTreeMap<PathBuf, PathBuf>,
    /// Favorite branches, decorated in the Git log branch tree.
    pub favorite_branches: Vec<String>,
    /// Uncommitted commit message draft per project root.
    pub commit_drafts: std::collections::BTreeMap<PathBuf, String>,
}

impl WorkspaceFile {
    pub fn path() -> PathBuf {
        Settings::config_dir().join("workspace.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&text) {
            Ok(workspace) => workspace,
            Err(err) => {
                log::warn!(
                    "{} is invalid ({err}); the next save will replace it",
                    path.display()
                );
                Self::default()
            }
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = Settings::config_dir();
        std::fs::create_dir_all(&dir)?;
        let text = toml::to_string_pretty(self)?;
        let path = Self::path();
        lazecode_fs::file::write_atomic(&path, text.as_bytes())
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        Ok(())
    }
}

/// Normalise a path for comparisons: absolute, no trailing slash.
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_settings_default_on_and_round_trip() {
        let settings = Settings::default();
        // Updates ship configured: the signed manifest lives next to the
        // repository and the release signing key's public half is pinned, so
        // the updater works out of the box.
        assert!(settings.update_endpoint.starts_with("https://"));
        assert!(!settings.update_public_key.is_empty());
        assert!(settings.auto_check_updates);
        assert!(settings.updates_configured());
        // The pinned key must parse as an Ed25519 public key.
        assert!(crate::updater::public_key_is_valid(
            &settings.update_public_key
        ));

        let settings = Settings {
            update_endpoint: "https://updates.example.test/manifest.json".into(),
            update_public_key: "Zm9vYmFy".into(),
            auto_check_updates: false,
            ..Settings::default()
        };
        let text = toml::to_string_pretty(&settings).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.update_endpoint, settings.update_endpoint);
        assert_eq!(back.update_public_key, settings.update_public_key);
        assert!(!back.auto_check_updates);
        assert!(back.updates_configured());
    }

    #[test]
    fn missing_updater_fields_fall_back_to_defaults() {
        let back: Settings = toml::from_str("theme = \"light\"\n").unwrap();
        assert!(back.ui_font.is_empty());
        assert!(back.editor.font_family.is_empty());
        assert!(back.terminal.font_family.is_empty());
        assert_eq!(back.terminal.max_agent_consoles, 20);
        assert!(back.update_endpoint.starts_with("https://"));
        assert!(back.auto_check_updates);
        assert!(back.updates_configured());
    }

    #[test]
    fn round_trips_settings() {
        let mut settings = Settings::default();
        settings.editor.font_size = 15.0;
        settings.editor.font_family = "Menlo".into();
        settings.terminal.font_family = "SF Mono".into();
        settings.terminal.max_agent_consoles = 40;
        settings.ui_font = "PingFang SC".into();
        settings.theme = ThemeMode::Light;
        let text = toml::to_string_pretty(&settings).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.editor.font_size, 15.0);
        assert_eq!(back.editor.font_family, "Menlo");
        assert_eq!(back.terminal.font_family, "SF Mono");
        assert_eq!(back.terminal.max_agent_consoles, 40);
        assert_eq!(back.ui_font, "PingFang SC");
        assert_eq!(back.theme, ThemeMode::Light);
    }
}

#[cfg(test)]
mod agent_settings_tests {
    use super::*;

    #[test]
    fn default_agents_keep_supported_entries_only() {
        let settings = Settings::default();
        let names: Vec<&str> = settings
            .agents
            .options
            .iter()
            .map(|option| option.name.as_str())
            .collect();
        assert_eq!(names, ["Claude Code", "OpenCode", "omp", "Shell"]);
        let shell = settings.agents.options.last().unwrap();
        assert!(shell.command.is_empty());
    }

    #[test]
    fn agent_scripts_round_trip_and_missing_field_uses_defaults() {
        let mut settings = Settings::default();
        settings.agents.options[0].command = "claude --dangerously-skip-permissions".into();
        let text = toml::to_string_pretty(&settings).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(
            back.agents.options[0].command,
            "claude --dangerously-skip-permissions"
        );

        let legacy: Settings = toml::from_str("theme = \"dark\"\n").unwrap();
        assert_eq!(legacy.agents.options.len(), 4);
        assert_eq!(legacy.agents.options[2].name, "omp");
    }
}
