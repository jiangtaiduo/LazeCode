use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GhosttyInfo {
    pub installed: bool,
    pub version: String,
    pub binary_path: String,
    pub resources_dir: String,
    pub config_path: String,
    pub theme_count: usize,
    pub platform_supported: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GhosttyThemeDto {
    pub background: String,
    pub foreground: String,
    pub cursor_color: String,
    pub cursor_text: String,
    pub selection_background: String,
    pub selection_foreground: String,
    pub font_family: String,
    pub font_size: f64,
    pub theme_name: String,
    pub palette: Vec<String>,
    pub source: String,
}

// Ghostty 1.3.1 内置默认调色板 (One Dark)
const DEFAULT_PALETTE: [&str; 16] = [
    "#1d1f21", "#cc6666", "#b5bd68", "#f0c674", "#81a2be", "#b294bb", "#8abeb7", "#c5c8c6",
    "#666666", "#d54e53", "#b9ca4a", "#e7c547", "#7aa6da", "#c397d8", "#70c0b1", "#eaeaea",
];

impl Default for GhosttyThemeDto {
    fn default() -> Self {
        GhosttyThemeDto {
            background: "#282c34".to_string(),
            foreground: "#ffffff".to_string(),
            cursor_color: "#ffffff".to_string(),
            cursor_text: "#282c34".to_string(),
            selection_background: "#44475a".to_string(),
            selection_foreground: "#ffffff".to_string(),
            font_family: String::new(),
            font_size: 13.0,
            theme_name: "Ghostty Default (One Dark)".to_string(),
            palette: DEFAULT_PALETTE.iter().map(|c| c.to_string()).collect(),
            source: "bundled".to_string(),
        }
    }
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 有界执行本文件的探测命令：此前全部用无超时的 `.output()`。
///
/// `ghostty --version` 这类本该瞬间返回的命令一旦被用户 PATH 上的同名
/// 包装脚本/shim 按住，命令线程就永久卡死（Zed 的 clippy.toml 禁止的
/// 正是这种无界阻塞式 Command 调用）。一律走 `run_with_timeout_spawn`
///（管道后台排空 + 超时杀进程组），超时按「失败」处理 —— 各调用点本来
///就是 `ok()?` / `unwrap_or_default` / 空列表的失败语义，行为不变。
fn bounded_output(cmd: &mut Command, timeout: Duration) -> Result<std::process::Output, String> {
    crate::commands::run_with_timeout_spawn(cmd, timeout)
}

/// 定位 Ghostty 可执行文件：优先使用平台标准安装路径，回退到 PATH 查询。
///
/// 本文件的命令有意保持同步、且 `find_ghostty_binary` 不加缓存。已核对全部
/// 调用点的实际频率：
/// - `detect_ghostty` 只在 App 启动时调一次（`App.tsx` 挂载 effect）；
/// - `ghostty_list_themes` / `ghostty_show_config` / `ghostty_load_theme`
///   目前**没有前端调用点**（`services/ghostty.ts` 导出但无人 import）；
/// - `launch_ghostty` 由用户在项目树右键显式触发。
///
/// 因此把五个命令改 async + spawn_blocking 只会增加每处一次任务调度，
/// 换不来可观测收益。而 `OnceLock` 缓存二进制路径会让「用户在应用运行期间
/// 安装 Ghostty」这一场景探测失效（detect 必须重新查），并引入进程级可变
/// 状态。若将来 show_config/list_themes 变成热路径，两处一并再改。
pub fn find_ghostty_binary() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/Applications/Ghostty.app/Contents/MacOS/ghostty"),
            home_dir().join("Applications/Ghostty.app/Contents/MacOS/ghostty"),
            PathBuf::from("/opt/homebrew/bin/ghostty"),
            PathBuf::from("/usr/local/bin/ghostty"),
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            home_dir().join("scoop/shims/ghostty.exe"),
            PathBuf::from("C:/Program Files/Ghostty/ghostty.exe"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/bin/ghostty"),
            PathBuf::from("/usr/local/bin/ghostty"),
            PathBuf::from("/snap/bin/ghostty"),
            PathBuf::from("/var/lib/flatpak/exports/bin/com.mitchellh.ghostty"),
        ]
    };

    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // PATH 回退查询（Unix 用 sh，Windows 用 where）。5 秒上限：
    // 一个挂起的 shell profile 会让无超时的 `.output()` 永久卡住命令线程。
    let mut lookup_cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("where");
        c.arg("ghostty");
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-lc", "command -v ghostty"]);
        c
    };
    let lookup = bounded_output(&mut lookup_cmd, Duration::from_secs(5));

    if let Ok(output) = lookup {
        if output.status.success() {
            let found = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !found.is_empty() {
                return Some(PathBuf::from(found));
            }
        }
    }

    None
}

/// 主题目录候选：用户自定义主题目录优先，其次 Ghostty 内置资源目录。
fn theme_search_dirs(resources_dir: &str) -> Vec<PathBuf> {
    let home = home_dir();
    let mut dirs: Vec<PathBuf> = Vec::new();

    if cfg!(target_os = "macos") {
        dirs.push(home.join("Library/Application Support/com.mitchellh.ghostty/themes"));
    }
    dirs.push(home.join(".config/ghostty/themes"));

    if !resources_dir.is_empty() {
        dirs.push(PathBuf::from(resources_dir).join("themes"));
    }

    dirs
}

/// 从可执行文件路径推导 Ghostty 资源目录（含 463 个内置主题）。
fn derive_resources_dir(binary: &Path) -> String {
    if let Some(contents) = binary.parent().and_then(|p| p.parent()) {
        let bundled = contents.join("Resources/ghostty");
        if bundled.exists() {
            return bundled.to_string_lossy().to_string();
        }
    }

    // Homebrew / Linux 安装布局
    for prefix in [
        "/opt/homebrew/share/ghostty",
        "/usr/local/share/ghostty",
        "/usr/share/ghostty",
    ] {
        if PathBuf::from(prefix).exists() {
            return prefix.to_string();
        }
    }

    String::new()
}

fn config_file_path() -> String {
    let home = home_dir();
    let candidates: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            home.join("Library/Application Support/com.mitchellh.ghostty/config.ghostty"),
            home.join("Library/Application Support/com.mitchellh.ghostty/config"),
            home.join(".config/ghostty/config"),
        ]
    } else {
        vec![home.join(".config/ghostty/config")]
    };

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }

    candidates
        .first()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 解析 Ghostty 配置文本为 `键 -> 多值` 映射（palette 可重复出现）。
pub fn parse_config_text(text: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            map.entry(key.trim().to_string())
                .or_default()
                .push(value.trim().to_string());
        }
    }

    map
}

/// 将 ANSI 调色板条目（形如 `0=#1d1f21`）解析为按索引排序的 16 色数组。
fn resolve_palette(values: &[String]) -> Vec<String> {
    let mut indexed: Vec<(usize, String)> = Vec::new();

    for entry in values {
        if let Some((idx, color)) = entry.split_once('=') {
            if let Ok(index) = idx.trim().parse::<usize>() {
                if index < 16 {
                    indexed.push((index, color.trim().to_string()));
                }
            }
        }
    }

    indexed.sort_by_key(|(idx, _)| *idx);

    let mut palette = DEFAULT_PALETTE
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<String>>();
    for (index, color) in indexed {
        palette[index] = color;
    }
    palette
}

fn first_value(map: &HashMap<String, Vec<String>>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(|values| values.first())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn build_theme_dto(
    map: &HashMap<String, Vec<String>>,
    theme_name: &str,
    source: &str,
) -> GhosttyThemeDto {
    let fallback = GhosttyThemeDto::default();

    let background = first_value(map, "background").unwrap_or(fallback.background);
    let foreground = first_value(map, "foreground").unwrap_or(fallback.foreground);
    let cursor_color = first_value(map, "cursor-color").unwrap_or_else(|| foreground.clone());
    let cursor_text = first_value(map, "cursor-text").unwrap_or_else(|| background.clone());
    let selection_background =
        first_value(map, "selection-background").unwrap_or(fallback.selection_background);
    let selection_foreground =
        first_value(map, "selection-foreground").unwrap_or(fallback.selection_foreground);

    let font_family = first_value(map, "font-family").unwrap_or_default();
    let font_size = first_value(map, "font-size")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(fallback.font_size);

    let palette = match map.get("palette") {
        Some(values) if !values.is_empty() => resolve_palette(values),
        _ => fallback.palette,
    };

    GhosttyThemeDto {
        background,
        foreground,
        cursor_color,
        cursor_text,
        selection_background,
        selection_foreground,
        font_family,
        font_size,
        theme_name: theme_name.to_string(),
        palette,
        source: source.to_string(),
    }
}

fn run_ghostty_config(binary: &PathBuf, default_only: bool) -> Option<String> {
    let mut cmd = Command::new(binary);
    cmd.arg("+show-config");
    if default_only {
        cmd.arg("--default");
    }

    let output = bounded_output(&mut cmd, Duration::from_secs(10)).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
pub fn detect_ghostty() -> GhosttyInfo {
    let Some(binary) = find_ghostty_binary() else {
        return GhosttyInfo {
            installed: false,
            platform_supported: !cfg!(target_os = "windows"),
            ..GhosttyInfo::default()
        };
    };

    let mut version_cmd = Command::new(&binary);
    version_cmd.arg("--version");
    let version = bounded_output(&mut version_cmd, Duration::from_secs(5))
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .unwrap_or_default();

    let resources_dir = derive_resources_dir(&binary);
    let theme_count = if resources_dir.is_empty() {
        0
    } else {
        std::fs::read_dir(PathBuf::from(&resources_dir).join("themes"))
            .map(|entries| entries.flatten().count())
            .unwrap_or(0)
    };

    GhosttyInfo {
        installed: true,
        version,
        binary_path: binary.to_string_lossy().to_string(),
        resources_dir,
        config_path: config_file_path(),
        theme_count,
        // Ghostty 官方 Windows 后端仍在开发中
        platform_supported: !cfg!(target_os = "windows"),
    }
}

#[tauri::command]
pub fn ghostty_list_themes() -> Vec<String> {
    let Some(binary) = find_ghostty_binary() else {
        return Vec::new();
    };

    let mut themes_cmd = Command::new(&binary);
    themes_cmd.args(["+list-themes", "--plain"]);
    let Ok(output) = bounded_output(&mut themes_cmd, Duration::from_secs(10)) else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            line.trim()
                .trim_end_matches("(resources)")
                .trim()
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

#[tauri::command]
pub fn ghostty_load_theme(name: String) -> GhosttyThemeDto {
    let info = detect_ghostty();
    let candidates = theme_search_dirs(&info.resources_dir);

    for dir in candidates {
        let path = dir.join(&name);
        if let Ok(text) = std::fs::read_to_string(&path) {
            let map = parse_config_text(&text);
            return build_theme_dto(&map, &name, "theme-file");
        }
    }

    GhosttyThemeDto {
        theme_name: name,
        source: "not-found".to_string(),
        ..GhosttyThemeDto::default()
    }
}

/// 读取用户当前生效的 Ghostty 配置（默认值 + 用户覆盖合并）。
#[tauri::command]
pub fn ghostty_show_config() -> GhosttyThemeDto {
    let Some(binary) = find_ghostty_binary() else {
        return GhosttyThemeDto::default();
    };

    let mut effective = match run_ghostty_config(&binary, true) {
        Some(text) => parse_config_text(&text),
        None => return GhosttyThemeDto::default(),
    };

    // 用户显式修改的配置覆盖默认值
    if let Some(changes_text) = run_ghostty_config(&binary, false) {
        for (key, values) in parse_config_text(&changes_text) {
            effective.entry(key).or_default().extend(values);
        }
    }

    let theme_label = first_value(&effective, "theme")
        .unwrap_or_else(|| "Ghostty Default (One Dark)".to_string());

    // 用户在配置中指定了主题名时，以主题文件调色板为准
    if let Some(theme_name) = first_value(&effective, "theme") {
        let loaded = ghostty_load_theme(theme_name);
        if loaded.source == "theme-file" {
            return loaded;
        }
    }

    build_theme_dto(&effective, &theme_label, "user-config")
}

/// 在独立的原生 Ghostty 窗口中打开当前工作目录。
/// macOS 不支持从 CLI 直接启动终端，必须经由 `open -na`。
///
/// 刻意不提供「启动时执行某命令」的参数：该参数前端完全可控，
/// macOS 上会经 `open --args -e <cmd>` 启动任意程序。当前唯一调用方
/// （项目树右键 Open In > Terminal）只需要 cwd，去掉参数即消除该攻击面。
#[tauri::command]
pub fn launch_ghostty(cwd: String) -> Result<String, String> {
    let workdir = if cwd.is_empty() { ".".to_string() } else { cwd };

    if cfg!(target_os = "macos") {
        let args: Vec<String> = vec![
            "-na".to_string(),
            "Ghostty.app".to_string(),
            "--args".to_string(),
            format!("--working-directory={}", workdir),
        ];
        // `open -na` 正常情况瞬间返回；15 秒上限防同名 open 被按住。
        // 超时错误直接透给前端（与启动失败同文案，调用点语义不变）。
        let mut open_cmd = Command::new("open");
        open_cmd.args(&args);
        let output = bounded_output(&mut open_cmd, Duration::from_secs(15))
            .map_err(|e| format!("启动 Ghostty 失败: {}", e))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        return Ok("已在原生 Ghostty 窗口打开".to_string());
    }

    if cfg!(target_os = "windows") {
        return Err("Ghostty 官方 Windows 后端尚未发布，敬请期待".to_string());
    }

    let binary = find_ghostty_binary().ok_or_else(|| "未找到 Ghostty 可执行文件".to_string())?;
    let mut cmd = Command::new(binary);
    cmd.arg(format!("--working-directory={}", workdir));

    // Ghostty 是长命的 GUI 进程，不能 wait 阻塞命令线程；但直接丢下不管
    // 会在它退出后留一个僵尸。交给 reaper 线程接手，命令立即返回。
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 Ghostty 失败: {}", e))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok("已在 Ghostty 中打开".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_lines_and_repeated_palette_entries() {
        let text = "\
# comment line
background = #282a36

palette = 0=#21222c
palette = 1=#ff5555
font-size = 14
";
        let map = parse_config_text(text);
        assert_eq!(first_value(&map, "background").unwrap(), "#282a36");
        assert_eq!(first_value(&map, "font-size").unwrap(), "14");
        // palette 重复键必须全部收集
        assert_eq!(map.get("palette").unwrap().len(), 2);
    }

    #[test]
    fn resolves_sparse_palette_over_defaults() {
        let entries = vec!["1=#ff5555".to_string(), "0=#21222c".to_string()];
        let palette = resolve_palette(&entries);
        assert_eq!(palette.len(), 16);
        assert_eq!(palette[0], "#21222c");
        assert_eq!(palette[1], "#ff5555");
        // 未声明的索引保留 Ghostty 默认值
        assert_eq!(palette[2], DEFAULT_PALETTE[2]);
    }

    #[test]
    fn bundled_default_matches_ghostty_one_dark() {
        let theme = GhosttyThemeDto::default();
        assert_eq!(theme.background, "#282c34");
        assert_eq!(theme.foreground, "#ffffff");
        assert_eq!(theme.palette.len(), 16);
        assert_eq!(theme.palette[4], "#81a2be");
    }

    // 以下测试依赖本机真实安装的 Ghostty，未安装时自动跳过
    #[test]
    fn detects_installed_ghostty_and_enumerates_themes() {
        let info = detect_ghostty();
        if !info.installed {
            eprintln!("跳过：本机未安装 Ghostty");
            return;
        }

        assert!(
            info.version.contains("Ghostty"),
            "版本串异常: {}",
            info.version
        );
        assert!(info.theme_count > 100, "主题数量异常: {}", info.theme_count);

        let themes = ghostty_list_themes();
        assert!(
            themes.contains(&"Dracula".to_string()),
            "主题清单缺少 Dracula"
        );
    }

    #[test]
    fn loads_real_theme_file_palette() {
        let theme = ghostty_load_theme("Dracula".to_string());
        if theme.source != "theme-file" {
            eprintln!("跳过：本机无可读取的 Ghostty 主题目录");
            return;
        }

        assert_eq!(theme.background, "#282a36");
        assert_eq!(theme.foreground, "#f8f8f2");
        assert_eq!(theme.palette[1], "#ff5555");
        assert_eq!(theme.palette[4], "#bd93f9");
        assert_eq!(theme.cursor_color, "#f8f8f2");
    }

    #[test]
    fn missing_theme_falls_back_without_panicking() {
        let theme = ghostty_load_theme("__definitely_not_a_theme__".to_string());
        assert_eq!(theme.source, "not-found");
        assert_eq!(theme.palette.len(), 16);
    }

    #[test]
    fn effective_config_exposes_a_full_palette() {
        let theme = ghostty_show_config();
        assert_eq!(theme.palette.len(), 16);
        for color in &theme.palette {
            assert!(color.starts_with('#'), "非法颜色值: {}", color);
            assert_eq!(color.len(), 7, "颜色值长度异常: {}", color);
        }
        assert!(theme.background.starts_with('#'));
    }
}
