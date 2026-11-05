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
            font_size: 12.5,
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
    pub font_size: f32,
    pub scrollback: usize,
    /// `None` means use `$SHELL` (or /bin/zsh).
    pub shell: Option<String>,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_size: 12.0,
            scrollback: 10_000,
            shell: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    pub language: Language,
    pub editor: EditorSettings,
    pub terminal: TerminalSettings,
    pub sidebar_width: f32,
    pub panel_height: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            language: Language::English,
            editor: EditorSettings::default(),
            terminal: TerminalSettings::default(),
            sidebar_width: 262.0,
            panel_height: 248.0,
        }
    }
}

impl Settings {
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
        self.editor.font_size = self.editor.font_size.clamp(9.0, 24.0);
        self.editor.tab_width = self.editor.tab_width.clamp(1, 8);
        self.terminal.font_size = self.terminal.font_size.clamp(9.0, 24.0);
        self.terminal.scrollback = self.terminal.scrollback.clamp(1_000, 200_000);
        self.sidebar_width = self.sidebar_width.clamp(180.0, 520.0);
        self.panel_height = self.panel_height.clamp(120.0, 900.0);
    }
}

/// Layout and recent-project state, persisted next to settings.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceFile {
    pub recent: Vec<PathBuf>,
    pub projects: Vec<PathBuf>,
    pub active_project: Option<PathBuf>,
    /// Focused worktree name per project root.
    pub active_worktrees: std::collections::BTreeMap<PathBuf, String>,
    pub sidebar_view: Option<String>,
    pub sidebar_visible: Option<bool>,
    pub sidebar_width: Option<f32>,
    pub panel_visible: Option<bool>,
    pub panel_height: Option<f32>,
    pub open_tabs: std::collections::BTreeMap<PathBuf, Vec<PathBuf>>,
    pub active_tabs: std::collections::BTreeMap<PathBuf, PathBuf>,
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
    fn round_trips_settings() {
        let mut settings = Settings::default();
        settings.editor.font_size = 15.0;
        settings.theme = ThemeMode::Light;
        let text = toml::to_string_pretty(&settings).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.editor.font_size, 15.0);
        assert_eq!(back.theme, ThemeMode::Light);
    }
}
