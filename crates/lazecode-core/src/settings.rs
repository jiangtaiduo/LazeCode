use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

/// Curated colour theme: one designed dark/light pair.
///
/// Canonical ids are the settings contract and must stay stable. Legacy skin
/// ids are accepted as serde aliases so existing `settings.toml` files keep
/// loading; saving always writes the new canonical id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeSkin {
    /// 石墨 — cool neutral chrome with an azure accent (default).
    #[default]
    #[serde(alias = "studio")]
    Graphite,
    /// 宣纸 — warm paper by day, cool ink by night, cinnabar accent.
    RicePaper,
    /// 苔原 — grey-green surfaces with a moss accent.
    #[serde(alias = "garden", alias = "jade", alias = "lotus", alias = "sand-sage")]
    Tundra,
    /// 深海 — blue-black surfaces with a cyan accent.
    #[serde(
        alias = "hydrangea",
        alias = "nordic",
        alias = "riviera",
        alias = "teal-lemon"
    )]
    Abyss,
    /// 鸢尾 — violet-grey surfaces with an iris accent.
    #[serde(alias = "royal", alias = "violet-dusk")]
    Iris,
    /// 琥珀 — warm brown surfaces with a golden accent.
    #[serde(
        alias = "lantern",
        alias = "maple",
        alias = "cacao",
        alias = "reeds",
        alias = "moss-gold",
        alias = "wild-olive"
    )]
    Amber,
    /// 绯木 — rose-tinted surfaces with a crimson accent.
    #[serde(alias = "coral", alias = "hibiscus", alias = "plum-ridge")]
    Rosewood,
}

impl ThemeSkin {
    pub const ALL: [ThemeSkin; 7] = [
        ThemeSkin::Graphite,
        ThemeSkin::RicePaper,
        ThemeSkin::Tundra,
        ThemeSkin::Abyss,
        ThemeSkin::Iris,
        ThemeSkin::Amber,
        ThemeSkin::Rosewood,
    ];

    /// Stable settings id (`kebab-case`, matches serde).
    pub fn id(self) -> &'static str {
        match self {
            ThemeSkin::Graphite => "graphite",
            ThemeSkin::RicePaper => "rice-paper",
            ThemeSkin::Tundra => "tundra",
            ThemeSkin::Abyss => "abyss",
            ThemeSkin::Iris => "iris",
            ThemeSkin::Amber => "amber",
            ThemeSkin::Rosewood => "rosewood",
        }
    }
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
    /// Soft per-console Kitty image cache target in MiB. Visible images are
    /// protected beyond this target up to four times the value, so a burst
    /// of pasted screenshots does not blank the oldest visible ones.
    pub graphics_cache_mb: usize,
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
            graphics_cache_mb: 64,
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
    /// Selected theme pair. `None` only while reading a legacy file that
    /// still stores `dark_skin` / `light_skin`.
    #[serde(rename = "skin", default)]
    pub theme_skin: Option<ThemeSkin>,
    /// Legacy per-mode skin keys. Read once by `migrate_skin`, never written.
    #[serde(default, skip_serializing)]
    pub dark_skin: Option<ThemeSkin>,
    #[serde(default, skip_serializing)]
    pub light_skin: Option<ThemeSkin>,
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
    /// Update manifest endpoint (HTTPS, `file://` or local path). Defaults to
    /// the signed `latest.json`; the manifest is verified before downloading.
    pub update_endpoint: String,
    /// Pinned Ed25519 public key for update manifests (base64 or hex).
    pub update_public_key: String,
    /// Check the endpoint once after startup; installing always needs a click.
    pub auto_check_updates: bool,
    /// Unix timestamp (seconds) of the last check attempt; `0` = never, also
    /// throttles the automatic check to once per day.
    pub last_update_check_time: i64,
    /// Version the user explicitly ignored. Background checks keep finding
    /// it, but no badge or toast is shown. Empty means no ignored version.
    pub ignored_update_version: String,
    /// 用户改过的快捷键覆盖表：动作 id（`files`/`git_log`…）→ 组合键串
    /// （`Cmd+1`/`Ctrl+P`…）。缺省动作走编译期默认键。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub keybindings: std::collections::BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            theme_skin: Some(ThemeSkin::Graphite),
            dark_skin: None,
            light_skin: None,
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
            last_update_check_time: 0,
            ignored_update_version: String::new(),
            keybindings: std::collections::BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Theme pair selected for both modes.
    pub fn skin(&self) -> ThemeSkin {
        self.theme_skin.unwrap_or_default()
    }

    pub fn set_skin(&mut self, skin: ThemeSkin) {
        self.theme_skin = Some(skin);
    }

    /// Fold legacy per-mode keys into the single paired selection. The active
    /// mode wins; if only the other mode was set, it is used as a fallback.
    fn migrate_skin(&mut self) {
        if self.theme_skin.is_none() {
            let (primary, fallback) = match self.theme {
                ThemeMode::Dark => (self.dark_skin, self.light_skin),
                ThemeMode::Light => (self.light_skin, self.dark_skin),
            };
            self.theme_skin = Some(primary.or(fallback).unwrap_or_default());
        }
        self.dark_skin = None;
        self.light_skin = None;
    }

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

    /// Fill endpoint/key defaults left empty by older settings so signed
    /// updates work without a hand edit.
    fn migrate_updater_defaults(&mut self) {
        if self.update_endpoint.trim().is_empty() {
            self.update_endpoint = Self::default().update_endpoint;
        }
        if self.update_public_key.trim().is_empty() {
            self.update_public_key = Self::default().update_public_key;
        }
    }

    /// `true` when both the endpoint and the pinned signing key are set.
    /// The UI disables every updater control while this is `false`.
    pub fn updates_configured(&self) -> bool {
        !self.update_endpoint.trim().is_empty() && !self.update_public_key.trim().is_empty()
    }

    pub fn config_dir() -> PathBuf {
        // Development / preview builds can run beside the stable app with an
        // isolated settings, workspace and update directory.
        if let Some(dir) = std::env::var_os("LAZECODE_CONFIG_DIR") {
            if !dir.is_empty() {
                return PathBuf::from(dir);
            }
        }
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
                    settings.migrate_updater_defaults();
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
        self.migrate_skin();
        self.ui_font = self.ui_font.trim().to_string();
        self.editor.font_family = self.editor.font_family.trim().to_string();
        self.terminal.font_family = self.terminal.font_family.trim().to_string();
        self.editor.font_size = self.editor.font_size.clamp(9.0, 24.0);
        self.editor.tab_width = self.editor.tab_width.clamp(1, 8);
        self.terminal.font_size = self.terminal.font_size.clamp(9.0, 24.0);
        self.terminal.scrollback = self.terminal.scrollback.clamp(1_000, 200_000);
        self.terminal.max_agent_consoles = self.terminal.max_agent_consoles.clamp(1, 100);
        self.terminal.graphics_cache_mb = self.terminal.graphics_cache_mb.clamp(8, 512);
        self.sidebar_width = self.sidebar_width.clamp(180.0, 520.0);
        self.commit_drawer_width = self.commit_drawer_width.clamp(240.0, 640.0);
        self.compare_drawer_width = self.compare_drawer_width.clamp(240.0, 640.0);
        self.panel_height = self.panel_height.clamp(120.0, 900.0);
        self.compare_split = self.compare_split.clamp(20.0, 80.0);
        self.compare_details_width = self.compare_details_width.clamp(24.0, 62.0);
        self.update_endpoint = self.update_endpoint.trim().to_string();
        self.update_public_key = self.update_public_key.trim().to_string();
        self.ignored_update_version = self.ignored_update_version.trim().to_string();
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
    /// Attached linked worktree names per project root. Needed before restoring
    /// open tabs so a side-worktree file is filed under the right checkout.
    pub open_worktrees: std::collections::BTreeMap<PathBuf, Vec<String>>,
    /// Pane layout per project root, keyed by worktree name. Compare sessions
    /// are not persisted, so their saved view falls back to the Git drawer.
    pub worktree_views: std::collections::BTreeMap<
        PathBuf,
        std::collections::BTreeMap<String, crate::model::WorktreeViewState>,
    >,
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
    /// Favorite branches keyed by `checkout root \u{1} branch`, decorated in
    /// the Git log branch tree. Bare legacy names migrate to the active repo.
    pub favorite_branches: Vec<String>,
    /// Uncommitted commit message draft per project root.
    pub commit_drafts: std::collections::BTreeMap<PathBuf, String>,
}

impl WorkspaceFile {
    pub fn path() -> PathBuf {
        Settings::config_dir().join("workspace.toml")
    }

    /// A first launch has no workspace file yet. Everything after that has a
    /// saved file, even when it only holds defaults.
    pub fn exists() -> bool {
        Self::path().exists()
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
    fn terminal_defaults_match_the_settings_choices() {
        let terminal = TerminalSettings::default();
        assert_eq!(terminal.scrollback, 10_000);
        assert_eq!(terminal.graphics_cache_mb, 64);
    }

    #[test]
    fn updater_settings_default_on_and_round_trip() {
        let settings = Settings::default();
        // Updates ship configured: signed manifest endpoint plus pinned key,
        // so the updater works out of the box.
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
    fn empty_updater_fields_migrate_to_defaults() {
        // Pre-updater installs saved empty strings; promote them to defaults.
        let mut settings: Settings = toml::from_str(
            "update_endpoint = \"\"
update_public_key = \"\"\n",
        )
        .unwrap();
        settings.migrate_updater_defaults();
        assert!(settings.updates_configured());
        assert!(settings.update_endpoint.starts_with("https://"));
        assert!(!settings.update_public_key.is_empty());
        // An operator-set endpoint is left untouched.
        let mut settings: Settings =
            toml::from_str("update_endpoint = \"https://example.test/m.json\"\n").unwrap();
        settings.migrate_updater_defaults();
        assert_eq!(settings.update_endpoint, "https://example.test/m.json");
    }

    #[test]
    fn updater_check_metadata_round_trips_and_defaults() {
        let back: Settings = toml::from_str("theme = \"light\"\n").unwrap();
        assert_eq!(back.last_update_check_time, 0);
        assert!(back.ignored_update_version.is_empty());

        let settings = Settings {
            last_update_check_time: 1_700_000_000,
            ignored_update_version: "0.4.7".into(),
            ..Settings::default()
        };
        let text = toml::to_string_pretty(&settings).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.last_update_check_time, 1_700_000_000);
        assert_eq!(back.ignored_update_version, "0.4.7");
    }

    #[test]
    fn missing_updater_fields_fall_back_to_defaults() {
        let back: Settings = toml::from_str("theme = \"light\"\n").unwrap();
        assert!(back.ui_font.is_empty());
        assert!(back.editor.font_family.is_empty());
        assert!(back.terminal.font_family.is_empty());
        assert_eq!(back.terminal.max_agent_consoles, 20);
        assert_eq!(back.terminal.graphics_cache_mb, 64);
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
        settings.terminal.graphics_cache_mb = 128;
        settings.ui_font = "PingFang SC".into();
        settings.theme = ThemeMode::Light;
        settings.set_skin(ThemeSkin::Amber);
        let text = toml::to_string_pretty(&settings).unwrap();
        assert!(text.contains("skin = \"amber\""), "{text}");
        assert!(!text.contains("dark_skin"), "{text}");
        assert!(!text.contains("light_skin"), "{text}");
        let mut back: Settings = toml::from_str(&text).unwrap();
        back.normalise();
        assert_eq!(back.editor.font_size, 15.0);
        assert_eq!(back.editor.font_family, "Menlo");
        assert_eq!(back.terminal.font_family, "SF Mono");
        assert_eq!(back.terminal.max_agent_consoles, 40);
        assert_eq!(back.terminal.graphics_cache_mb, 128);
        assert_eq!(back.ui_font, "PingFang SC");
        assert_eq!(back.theme, ThemeMode::Light);
        assert_eq!(back.skin(), ThemeSkin::Amber);
    }

    #[test]
    fn default_skin_is_graphite() {
        assert_eq!(Settings::default().skin(), ThemeSkin::Graphite);
    }

    #[test]
    fn every_canonical_skin_id_round_trips() {
        for skin in ThemeSkin::ALL {
            let text = format!("skin = \"{}\"\n", skin.id());
            let mut back: Settings = toml::from_str(&text).unwrap();
            back.normalise();
            assert_eq!(back.skin(), skin, "{}", skin.id());
        }
    }

    #[test]
    fn legacy_skin_ids_migrate_to_paired_themes() {
        let cases = [
            ("studio", ThemeSkin::Graphite),
            ("hydrangea", ThemeSkin::Abyss),
            ("garden", ThemeSkin::Tundra),
            ("royal", ThemeSkin::Iris),
            ("nordic", ThemeSkin::Abyss),
            ("lantern", ThemeSkin::Amber),
            ("sand-sage", ThemeSkin::Tundra),
            ("plum-ridge", ThemeSkin::Rosewood),
            ("maple", ThemeSkin::Amber),
            ("reeds", ThemeSkin::Amber),
            ("violet-dusk", ThemeSkin::Iris),
            ("lotus", ThemeSkin::Tundra),
            ("jade", ThemeSkin::Tundra),
            ("riviera", ThemeSkin::Abyss),
            ("iris", ThemeSkin::Iris),
            ("cacao", ThemeSkin::Amber),
            ("moss-gold", ThemeSkin::Amber),
            ("coral", ThemeSkin::Rosewood),
            ("teal-lemon", ThemeSkin::Abyss),
            ("hibiscus", ThemeSkin::Rosewood),
            ("wild-olive", ThemeSkin::Amber),
        ];
        for (legacy, expected) in cases {
            let text =
                format!("theme = \"dark\"\ndark_skin = \"{legacy}\"\nlight_skin = \"studio\"\n");
            let mut back: Settings = toml::from_str(&text).unwrap();
            back.normalise();
            assert_eq!(back.skin(), expected, "{legacy}");
            assert!(back.dark_skin.is_none(), "{legacy}");
            assert!(back.light_skin.is_none(), "{legacy}");
        }
    }

    #[test]
    fn active_mode_wins_when_legacy_skins_differ() {
        let mut back: Settings = toml::from_str(
            "theme = \"light\"\ndark_skin = \"moss-gold\"\nlight_skin = \"sand-sage\"\n",
        )
        .unwrap();
        back.normalise();
        assert_eq!(back.skin(), ThemeSkin::Tundra);
    }

    #[test]
    fn missing_skin_fields_fall_back_to_graphite() {
        let mut back: Settings = toml::from_str("theme = \"dark\"\n").unwrap();
        back.normalise();
        assert_eq!(back.skin(), ThemeSkin::Graphite);
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

#[cfg(test)]
mod workspace_tests {
    use super::*;
    use crate::model::{PanelTab, SidebarView, WorktreeViewState};

    #[test]
    fn workspace_round_trips_per_worktree_views() {
        let view = WorktreeViewState {
            terminal_visible: true,
            panel_visible: false,
            sidebar_view: SidebarView::Git,
            panel_tab: PanelTab::Git,
            ..Default::default()
        };
        let mut file = WorkspaceFile::default();
        file.worktree_views
            .entry(PathBuf::from("/tmp/project"))
            .or_default()
            .insert("feature".to_string(), view);

        let text = toml::to_string_pretty(&file).unwrap();
        let back: WorkspaceFile = toml::from_str(&text).unwrap();
        let views = back
            .worktree_views
            .get(Path::new("/tmp/project"))
            .expect("project view map");
        assert_eq!(views.get("feature"), Some(&view));
    }
}
