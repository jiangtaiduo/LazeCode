//! 窗口几何（位置 / 尺寸 / 最大化）的跨会话持久化。
//!
//! 需求：每次启动使用上次退出时的窗口位置与大小，首次启动才用配置里的默认值。
//!
//! ## 为什么做在 Rust 侧而不是前端 localStorage
//!
//! 1. 窗口在 `App.tsx` 挂载之前就已经由 tauri.conf.json 创建完成，前端 React
//!    渲染时窗口已在默认位置闪了一下 —— 「恢复上次位置」要的是「窗口出现时
//!    就在上次的位置」，只能在原生侧创建窗口的那一刻做。
//! 2. 位置/尺寸是**设备像素**（`outer_position` / `outer_size` 返回
//!    `PhysicalPosition<i32>` / `PhysicalSize<u32>`），写入前需按当前屏幕的
//!    scale factor 换算才能与 `.position(x, y)` / `.set_size(w, h)` 的
//!    逻辑像素参数对齐；这层换算只有原生侧顺手。
//!    采集与恢复都用 **outer** 口径（`outer_*` ↔ `set_size`），
//!    混用 inner 会引入 chrome 尺寸漂移，见 `capture` 的说明。
//! 3. 前端 localStorage 属 WebView，窗口关闭时可能已没有机会 flush。
//!
//! ## 存储位置与格式
//!
//! `{app_config_dir}/window-state.json`（macOS: `~/Library/Application Support/
//! io.github.jiangtaiduo.lazecode/`，Windows: `%APPDATA%\io.github.jiangtaiduo.lazecode\`）。不用
//! localStorage 也不用 `app_log_dir`（那是给日志的目录）。
//!
//! ```json
//! { "x": 120, "y": 80, "width": 1440, "height": 920, "maximized": false, "scaleFactor": 2.0 }
//! ```
//!
//! 全部字段可选：任何字段缺失/损坏/越界都只忽略该字段，退回配置默认值。
//!
//! ## 多显示器与缩放变化
//!
//! 物理像素写盘、读取时换算回逻辑像素。跨显示器（不同 DPI）、外接显示器
//! 拔掉、系统改缩放后，旧坐标可能落在所有显示器的工作区之外 —— 此时
//! `clamp_into_visible_monitors` 把窗口拉回可见区域中央，而不是让用户
//! 看到一个打不开的窗口（macOS 上窗口会整个不可见，只能重启）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 磁盘上的窗口几何（逻辑像素 + 当时的 scale factor）。
///
/// `#[serde(default)]` 让缺字段的旧文件也能解析：手工编辑 / 早期版本写入的
/// 内容不会让整份状态作废。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowState {
    /// 窗口外框左上角（逻辑像素，可跨桌面为负）
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// 客户区尺寸（逻辑像素）
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// 退出时是否最大化
    pub maximized: Option<bool>,
    /// 采集时的屏幕 scale factor（用于跨会话 DPI 归一）
    pub scale_factor: Option<f64>,
}

impl WindowState {
    /// 当前是否认为状态为空（没有任何可用字段）。
    pub fn is_empty(&self) -> bool {
        self.x.is_none()
            && self.y.is_none()
            && self.width.is_none()
            && self.height.is_none()
            && self.maximized.is_none()
    }
}

/// 状态文件路径：`{app_config_dir}/window-state-<label>.json`。
///
/// **每个窗口独立一份**。多窗口（`create_new_window` 开 `win_*`）下如果共用
/// 一个文件，任一窗口的移动都会覆盖别的窗口的几何，最后一次写（通常是
/// 某个窗口关闭时的 Destroyed 采集）决定全局，等于谁都恢复不对。
/// `label` 是 tauri 分配的窗口标识，无需额外配置。
pub fn state_file_path(app_config_dir: &Path, label: &str) -> PathBuf {
    // label 只可能是 tauri 生成的 "main" / "win_<millis>"，理论上无路径字符；
    // 仍需过滤：这是拼接进文件名的外部来源，防御纵深。
    let safe: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    app_config_dir.join(format!("window-state-{safe}.json"))
}

/// 兼容单窗口时代的旧路径（`window-state.json`）。
///
/// 首次启动时若它存在，按 main 的几何读出来 —— 否则升级到多窗口版本后
/// 用户会丢掉上次摆好的窗口位置，属于无谓的回退。
fn legacy_state_file_path(app_config_dir: &Path) -> PathBuf {
    app_config_dir.join("window-state.json")
}

/// 读取指定窗口的状态。
///
/// 任何失败（文件不存在、JSON 损坏、权限）都返回 `None` 并记日志：
/// 首次启动 / 状态损坏都走「配置默认值」这条路径，不阻断启动。
pub fn load_state(app_config_dir: &Path, label: &str) -> Option<WindowState> {
    let path = state_file_path(app_config_dir, label);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 仅 main 尝试旧路径：其它窗口没有那一段历史。
            if label == "main" {
                read_to_string_tolerating_corrupt(&legacy_state_file_path(app_config_dir))?
            } else {
                return None;
            }
        }
        Err(e) => {
            log::warn!("读取窗口状态 {} 失败: {}", path.display(), e);
            return None;
        }
    };
    match serde_json::from_str::<WindowState>(&text) {
        Ok(state) => {
            log::debug!("已加载窗口 {} 状态: {:?}", label, state);
            Some(state)
        }
        Err(e) => {
            log::warn!(
                "窗口 {} 状态 {} 解析失败（将使用默认值）: {}",
                label,
                path.display(),
                e
            );
            None
        }
    }
}

/// 读旧路径：不存在/损坏都返回 None（纯兼容路径，不该有副作用）。
fn read_to_string_tolerating_corrupt(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// 写入窗口状态（原子：同目录临时文件 + rename）。
///
/// 与 `save_changelists` 同款做法：进程在「写一半」时被杀不会留下半个 JSON。
/// 临时名带纳秒时间戳：连续两次保存（拖动窗口 → 立即退出）不互相覆盖。
pub fn save_state(app_config_dir: &Path, label: &str, state: &WindowState) -> Result<(), String> {
    if let Some(parent) = app_config_dir.to_path_buf().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::create_dir_all(app_config_dir);
    let path = state_file_path(app_config_dir, label);
    let text =
        serde_json::to_string_pretty(state).map_err(|e| format!("序列化窗口状态失败: {e}"))?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = app_config_dir.join(format!(
        "window-state.json.tmp.{}-{}",
        std::process::id(),
        unique
    ));
    let write = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("写入窗口状态 {} 失败: {}", path.display(), e));
    }
    Ok(())
}

/// 一个可见的矩形区域（逻辑像素），来自某个 monitor 的 work area。
#[derive(Debug, Clone, Copy)]
struct VisibleRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl VisibleRect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }

    /// 把点拉回矩形内（给「窗口在但坐标在屏幕外」的场景兜底）。
    fn clamp_point(&self, x: f64, y: f64) -> (f64, f64) {
        (
            x.clamp(self.left, self.right),
            y.clamp(self.top, self.bottom),
        )
    }
}

/// 把物理像素坐标换算为逻辑像素。
pub fn to_logical(physical: i32, scale_factor: f64) -> f64 {
    let sf = if scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    physical as f64 / sf
}

/// 把逻辑像素坐标换算为物理像素（供采集侧写盘）。
pub fn to_physical(value: f64, scale_factor: f64) -> i64 {
    let sf = if scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (value * sf).round() as i64
}

/// 采集当前窗口几何。
///
/// 注意采集的**时机**：`outer_position` / `outer_size` / `is_maximized`
/// 是同步 getter（tauri 内部经主线程 event loop 完成），调用方必须已在
/// 主线程 —— `on_window_event` 的 Moved/Resized/Destroyed 回调满足这条。
///
/// 尺寸取 `outer_size` 而不是 `inner_size`：恢复侧用的是
/// `set_size`（语义 = outer size，tauri 文档 "Resizes this window"）。
/// 两侧必须同口径 —— 采 inner、设 outer 会让每次重启都按「上次的内容区
/// 尺寸」当外框来设，窗口被 chrome（标题栏/边框）吃掉一圈，
/// 恢复出来比用户摆好的小，且反复重启会持续收缩。
///
/// 两种状态跳过采集（由本函数判定，调用方无需关心）：
/// - **最小化**：位置是「最小化位置」，恢复后窗口跑到错误的地方；
/// - **原生全屏**：坐标与尺寸都是全屏 Space 的值，与「最大化」语义不同。
///   把全屏时的 1470×903 当普通尺寸写盘、再配合 maximized=true 恢复，
///   会得到一个尺寸被污染的「假最大化」。全屏状态本身不持久化 ——
///   退出全屏是用户主动行为，恢复成窗口化比强行拉回全屏更符合预期。
pub fn capture<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    scale_factor: f64,
) -> Option<WindowState> {
    if window.is_minimized().unwrap_or(false) {
        return None;
    }
    if window.is_fullscreen().unwrap_or(false) {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let maximized = window.is_maximized().unwrap_or(false);
    Some(WindowState {
        x: Some(to_logical(position.x, scale_factor)),
        y: Some(to_logical(position.y, scale_factor)),
        width: Some(to_logical(size.width as i32, scale_factor)),
        height: Some(to_logical(size.height as i32, scale_factor)),
        maximized: Some(maximized),
        scale_factor: Some(if scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        }),
    })
}

/// 用恢复出的状态修正窗口位置：把窗口拉回所有可见显示器的工作区之内。
///
/// 规则（与 VS Code / Chrome 的恢复行为一致）：
/// 1. 原始坐标落在任一显示器工作区内 → 原样使用；
/// 2. 否则（拔掉外接屏、改了分辨率/缩放）→ 拉回主显示器工作区，居中偏左上。
///
/// 只在**多显示器/坐标越界**时才动窗口：正常路径零 API 调用。
pub fn clamp_into_visible_monitors(
    x: f64,
    y: f64,
    monitors: &[(f64, f64, f64, f64)],
    primary: Option<(f64, f64, f64, f64)>,
) -> (f64, f64) {
    // 只判断窗口**左上角**是否落在某块屏内。窗口尺寸未知（老调用方）时
    // 只能这样；新的恢复流程请用 clamp_size_into_monitors + clamp_position_into_monitors，
    // 那两个会把整块窗口拉进屏幕。
    let rects: Vec<VisibleRect> = monitors
        .iter()
        .map(|&(left, top, right, bottom)| VisibleRect {
            left,
            top,
            right,
            bottom,
        })
        .collect();
    if rects.is_empty() {
        // 没有显示器信息（无头环境）：保持原值，交给系统兜底
        return (x, y);
    }
    if rects.iter().any(|r| r.contains(x, y)) {
        return (x, y);
    }
    match primary {
        Some((left, top, right, bottom)) => {
            let rect = VisibleRect {
                left,
                top,
                right,
                bottom,
            };
            rect.clamp_point(x, y)
        }
        None => {
            // 无主显示器信息：选第一个可见区域
            let rect = rects[0];
            rect.clamp_point(x, y)
        }
    }
}

/// 窗口尺寸下限：与 `tauri.conf.json` 的 minWidth/minHeight 保持一致。
const MIN_W: f64 = 1024.0;
const MIN_H: f64 = 700.0;

/// 把窗口尺寸夹到「最大的那块显示器工作区」以内。
///
/// 换过分辨率 / 缩放 / 拔掉外接屏之后，存盘时的尺寸可能超过现在所有屏幕。
/// 尺寸不夹的后果是位置夹了也没用：窗口左上角落在屏内就算「可见」，
/// 但大半截内容在屏幕外，用户看到的是标题栏下面一片空白。
pub fn clamp_size_into_monitors(
    w: f64,
    h: f64,
    monitors: &[(f64, f64, f64, f64)],
    primary: Option<(f64, f64, f64, f64)>,
) -> (f64, f64) {
    // 取所有显示器（含主屏）里最大的工作区作为上限：窗口跨屏摆放时也能保住
    // 原尺寸，只有确实无处可放时才缩。
    let mut area = primary;
    for m in monitors {
        let (lw, lh) = (m.2 - m.0, m.3 - m.1);
        area = match area {
            Some((l, t, r, b)) if (r - l) * (b - t) >= lw * lh => Some((l, t, r, b)),
            _ => Some((m.0, m.1, m.2, m.3)),
        };
    }
    let Some((left, top, right, bottom)) = area else {
        return (w, h);
    };
    let aw = (right - left).max(1.0);
    let ah = (bottom - top).max(1.0);
    // 屏幕比下限还小时以屏幕为准，不能反向撑大窗口
    (w.clamp(MIN_W.min(aw), aw), h.clamp(MIN_H.min(ah), ah))
}

/// 窗口矩形与 `r` 的重叠面积（用于挑选落点显示器）。
fn overlap_area(r: &VisibleRect, x: f64, y: f64, w: f64, h: f64) -> f64 {
    let ox = (x + w).min(r.right) - x.max(r.left);
    let oy = (y + h).min(r.bottom) - y.max(r.top);
    if ox <= 0.0 || oy <= 0.0 {
        0.0
    } else {
        ox * oy
    }
}

/// 把窗口**整块**拉进某块显示器：`clamp_size_into_monitors` 之后调用，
/// 保证尺寸 ≤ 目标屏工作区，因此这里一定能落进去。
pub fn clamp_position_into_monitors(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    monitors: &[(f64, f64, f64, f64)],
    primary: Option<(f64, f64, f64, f64)>,
) -> (f64, f64) {
    if monitors.is_empty() {
        return (x, y);
    }
    // 落点显示器 = 与窗口重叠面积最大的那块；全都重叠为 0（窗口整个在屏外）
    // 就用主屏，其次第一块。
    let mut best: Option<(f64, VisibleRect)> = None;
    for m in monitors {
        let rect = VisibleRect {
            left: m.0,
            top: m.1,
            right: m.2,
            bottom: m.3,
        };
        let area = overlap_area(&rect, x, y, w, h);
        if best.as_ref().is_none_or(|(a, _)| area > *a) {
            best = Some((area, rect));
        }
    }
    let picked = match best {
        Some((a, rect)) if a > 0.0 => rect,
        _ => primary
            .map(|(l, t, r, b)| VisibleRect {
                left: l,
                top: t,
                right: r,
                bottom: b,
            })
            .unwrap_or(VisibleRect {
                left: monitors[0].0,
                top: monitors[0].1,
                right: monitors[0].2,
                bottom: monitors[0].3,
            }),
    };
    let max_x = (picked.right - w).max(picked.left);
    let max_y = (picked.bottom - h).max(picked.top);
    (x.clamp(picked.left, max_x), y.clamp(picked.top, max_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "ide_window_state_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn reads_back_written_state() {
        let dir = temp_dir("roundtrip");
        let state = WindowState {
            x: Some(120.0),
            y: Some(80.0),
            width: Some(1440.0),
            height: Some(920.0),
            maximized: Some(false),
            scale_factor: Some(2.0),
        };
        save_state(&dir, "main", &state).expect("写入应成功");
        let loaded = load_state(&dir, "main").expect("应能读回");
        assert_eq!(loaded.x, Some(120.0));
        assert_eq!(loaded.y, Some(80.0));
        assert_eq!(loaded.width, Some(1440.0));
        assert_eq!(loaded.height, Some(920.0));
        assert_eq!(loaded.maximized, Some(false));
        assert_eq!(loaded.scale_factor, Some(2.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_yields_none() {
        let dir = temp_dir("missing");
        // 目录存在但文件不存在
        assert!(load_state(&dir, "main").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_file_yields_none_not_panic() {
        let dir = temp_dir("corrupt");
        let path = state_file_path(&dir, "main");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(load_state(&dir, "main").is_none(), "损坏文件必须安全降级");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_state_deserializes_with_missing_fields() {
        let dir = temp_dir("partial");
        let path = state_file_path(&dir, "main");
        // 只写一部分字段：旧版本 / 手工编辑的文件
        std::fs::write(&path, r#"{"width": 1600}"#).unwrap();
        let state = load_state(&dir, "main").expect("部分字段也应能解析");
        assert_eq!(state.width, Some(1600.0));
        assert_eq!(state.x, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coordinate_conversions_round_trip() {
        // 物理 ↔ 逻辑必须互逆（同一 scale factor 下）。
        // 浮点坐标在物理侧会被取整成整数像素，因此往返误差上限是 1 物理像素。
        for &sf in &[1.0, 1.25, 1.5, 2.0] {
            for &logical in &[0.0, 13.0, 100.5, 1440.0, -120.0] {
                let physical = to_physical(logical, sf);
                let back = to_logical(physical as i32, sf);
                let drift = (back - logical).abs() * sf;
                assert!(
                    drift <= 1.0,
                    "逻辑 {logical} 经 {sf} 往返得到 {back}（漂移 {drift} 物理像素）"
                );
            }
        }
        // scale 1.0 时物理即逻辑，浮点应精确可逆
        assert_eq!(to_logical(to_physical(100.5, 1.0) as i32, 1.0), 101.0);
        assert_eq!(to_logical(to_physical(-120.0, 1.0) as i32, 1.0), -120.0);
    }

    #[test]
    fn scale_factor_zero_falls_back_to_one() {
        // scale factor 为 0（异常采集）不能除零
        assert_eq!(to_logical(100, 0.0), 100.0);
        assert_eq!(to_physical(100.0, 0.0), 100);
    }

    #[test]
    fn position_inside_any_monitor_is_kept() {
        // 副屏在左（负坐标）：窗口在副屏上时必须原样保留
        let monitors = vec![(-1920.0, 0.0, 0.0, 1080.0), (0.0, 0.0, 2560.0, 1440.0)];
        let primary = Some((0.0, 0.0, 2560.0, 1440.0));
        let (x, y) = clamp_into_visible_monitors(-100.0, 50.0, &monitors, primary);
        assert_eq!((x, y), (-100.0, 50.0), "副屏坐标必须保留");
    }

    #[test]
    fn position_outside_all_monitors_is_clamped_to_primary() {
        // 拔掉外接屏后旧坐标落在 (-5000, -5000)：必须拉回可见区
        let monitors = vec![(0.0, 0.0, 2560.0, 1440.0)];
        let primary = Some((0.0, 0.0, 2560.0, 1440.0));
        let (x, y) = clamp_into_visible_monitors(-5000.0, -5000.0, &monitors, primary);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn position_just_outside_right_edge_is_clamped() {
        // 窗口大部分在屏内、x 超出右缘几个像素（缩放变化常见）→ 钳到右缘
        let monitors = vec![(0.0, 0.0, 1440.0, 900.0)];
        let primary = Some((0.0, 0.0, 1440.0, 900.0));
        let (x, y) = clamp_into_visible_monitors(1500.0, 100.0, &monitors, primary);
        assert_eq!(x, 1440.0);
        assert_eq!(y, 100.0);
    }

    #[test]
    fn size_larger_than_screen_is_clamped_down() {
        // 真实故障：1720x720 的屏幕上恢复了 1377x923 的窗口，内容大半截在屏外
        let monitors = vec![(0.0, 0.0, 1720.0, 720.0)];
        let primary = Some((0.0, 0.0, 1720.0, 720.0));
        let (w, h) = clamp_size_into_monitors(1377.0, 923.0, &monitors, primary);
        assert_eq!((w, h), (1377.0, 720.0), "高度必须被夹到屏幕内");
    }

    #[test]
    fn size_that_fits_is_kept() {
        let monitors = vec![(0.0, 0.0, 2560.0, 1440.0)];
        let primary = Some((0.0, 0.0, 2560.0, 1440.0));
        assert_eq!(
            clamp_size_into_monitors(1377.0, 923.0, &monitors, primary),
            (1377.0, 923.0)
        );
    }

    #[test]
    fn size_uses_largest_monitor_not_primary() {
        // 主屏 1280x800，右侧 4K 副屏：窗口放在副屏上就不该被主屏尺寸夹小
        let monitors = vec![(0.0, 0.0, 1280.0, 800.0), (1280.0, 0.0, 3840.0, 2160.0)];
        let primary = Some((0.0, 0.0, 1280.0, 800.0));
        assert_eq!(
            clamp_size_into_monitors(2000.0, 1200.0, &monitors, primary),
            (2000.0, 1200.0)
        );
    }

    #[test]
    fn no_monitor_info_keeps_size() {
        assert_eq!(
            clamp_size_into_monitors(1440.0, 920.0, &[], None),
            (1440.0, 920.0)
        );
    }

    #[test]
    fn position_pulls_whole_window_into_screen() {
        // 真实故障：y=518 的窗口高 923，屏幕高 720 —— 左上角在屏内但大半截在外面
        let monitors = vec![(0.0, 0.0, 1720.0, 720.0)];
        let primary = Some((0.0, 0.0, 1720.0, 720.0));
        let (w, h) = clamp_size_into_monitors(1377.0, 923.0, &monitors, primary);
        let (x, y) = clamp_position_into_monitors(333.0, 518.0, w, h, &monitors, primary);
        assert_eq!(y, 0.0, "整窗必须落在屏幕内");
        assert_eq!(x, 333.0, "横向本来就在屏内，不该被挪动");
    }

    #[test]
    fn position_on_secondary_monitor_is_kept() {
        // 副屏在左（负坐标），窗口完全在副屏内时原样保留
        let monitors = vec![(-1920.0, 0.0, 0.0, 1080.0), (0.0, 0.0, 2560.0, 1440.0)];
        let primary = Some((0.0, 0.0, 2560.0, 1440.0));
        assert_eq!(
            clamp_position_into_monitors(-1500.0, 100.0, 800.0, 600.0, &monitors, primary),
            (-1500.0, 100.0)
        );
    }

    #[test]
    fn window_entirely_offscreen_falls_back_to_primary() {
        // 拔掉外接屏后旧坐标落在 (-5000,-5000)
        let monitors = vec![(0.0, 0.0, 2560.0, 1440.0)];
        let primary = Some((0.0, 0.0, 2560.0, 1440.0));
        assert_eq!(
            clamp_position_into_monitors(-5000.0, -5000.0, 1440.0, 920.0, &monitors, primary),
            (0.0, 0.0)
        );
    }

    #[test]
    fn position_no_monitor_info_keeps_original() {
        assert_eq!(
            clamp_position_into_monitors(-5000.0, -5000.0, 1440.0, 920.0, &[], None),
            (-5000.0, -5000.0)
        );
    }

    #[test]
    fn right_overflow_is_pulled_back() {
        let monitors = vec![(0.0, 0.0, 1440.0, 900.0)];
        let primary = Some((0.0, 0.0, 1440.0, 900.0));
        // 宽 800 的窗口 x=1500 → 右缘 2300 超出，应拉到 640
        assert_eq!(
            clamp_position_into_monitors(1500.0, 100.0, 800.0, 600.0, &monitors, primary),
            (640.0, 100.0)
        );
    }

    #[test]
    fn no_monitor_info_keeps_original() {
        let (x, y) = clamp_into_visible_monitors(-5000.0, -5000.0, &[], None);
        assert_eq!((x, y), (-5000.0, -5000.0));
    }

    #[test]
    fn empty_state_is_detected() {
        assert!(WindowState::default().is_empty());
        assert!(!WindowState {
            x: Some(1.0),
            ..WindowState::default()
        }
        .is_empty());
    }

    /// 多窗口下每个窗口的几何必须互不覆盖。
    ///
    /// 单文件实现时：`main` 位于 (0,0)，用户把 `win_1` 拖到 (800,400)，
    /// 后一次写盘会把 main 的位置也变成 (800,400)。回归点是「另一个窗口的
    /// 写盘不得影响本窗口读回的值」。
    #[test]
    fn each_window_label_has_its_own_state_file() {
        let dir = temp_dir("perlabel");
        let main = WindowState {
            x: Some(0.0),
            y: Some(0.0),
            width: Some(1440.0),
            height: Some(920.0),
            maximized: Some(false),
            scale_factor: Some(2.0),
        };
        let other = WindowState {
            x: Some(800.0),
            y: Some(400.0),
            width: Some(1200.0),
            height: Some(700.0),
            maximized: Some(false),
            scale_factor: Some(2.0),
        };
        save_state(&dir, "main", &main).unwrap();
        save_state(&dir, "win_1757", &other).unwrap();

        // 两份文件、两种内容
        assert_ne!(
            state_file_path(&dir, "main"),
            state_file_path(&dir, "win_1757")
        );
        assert_eq!(load_state(&dir, "main").unwrap().x, Some(0.0));
        assert_eq!(load_state(&dir, "win_1757").unwrap().x, Some(800.0));

        // 单个窗口的状态缺失不影响另一个
        assert!(load_state(&dir, "win_never_created").is_none());
        assert_eq!(load_state(&dir, "main").unwrap().width, Some(1440.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// label 参与文件名拼接，必须不能把 ../ 带出去（防御纵深：
    /// tauri 自己生成的 label 没这个问题，但函数是 pub 的）。
    #[test]
    fn label_cannot_escape_config_dir() {
        let dir = std::env::temp_dir().join("ide_ws_label_escape");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let evil = "../../../../etc/passwd";
        let path = state_file_path(&dir, evil);
        assert!(
            path.starts_with(&dir),
            "被勒索的 label 仍须落在配置目录内: {}",
            path.display()
        );
        // 落盘也不能成功写到外部
        let state = WindowState {
            x: Some(1.0),
            ..WindowState::default()
        };
        assert!(save_state(&dir, evil, &state).is_ok());
        assert!(path.starts_with(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 单窗口时代的 `window-state.json` 必须仍能被 main 读回，
    /// 否则升级到多窗口版本后用户丢一次窗口位置。
    #[test]
    fn legacy_single_window_file_is_still_read_for_main() {
        let dir = temp_dir("legacy");
        let old = 340.0;
        std::fs::write(legacy_state_file_path(&dir), format!(r#"{{"x":{old}}}"#)).unwrap();
        let loaded = load_state(&dir, "main").expect("main 应能读旧格式");
        assert_eq!(loaded.x, Some(old));
        // 其它窗口没有那一段历史，不该去读
        assert!(load_state(&dir, "win_1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
