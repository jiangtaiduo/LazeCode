//! 应用级偏好：界面语言。
//!
//! ## 为什么 Rust 侧要存一份语言
//!
//! 原生菜单在 `setup()` 里构造，**早于前端任何代码执行**。此时若不知道用户
//! 上次选的语言，菜单只能用硬编码的英文 —— 用户把语言切成中文，重启后
//! 菜单变回英文，要等前端挂载后再重建一次才纠正（可见的闪烁 + 短时间内
//! 两套语言并存）。
//!
//! 前端语言存 `localStorage`（随 WebView，读它是 DOM 侧的事），Rust 读不到。
//! 因此这里维护一份 `{app_config_dir}/language`，由 `set_app_menu_language`
//! 在切换时同步写入。
//!
//! ## 一致性策略
//!
//! 两份存储（localStorage + 本文件）**可能短暂不一致**：隐私模式下前端
//! 写 localStorage 失败，但 Rust 这份仍写成功。定优先级为
//! **localStorage 优先**：它才是界面文案（菜单之外的全部内容）的来源，
//! 且用户能通过设置面板看到并修正它。本文件只是「让首屏菜单正确」的缓存。
//!
//! 这意味着隐私模式下重启会出现「界面英文、菜单中文」——但隐私模式本身就是
//! 「本次会话不记住设置」，用户下一次改动就会重新同步两边。

use std::path::Path;

use crate::menu::{labels_for, normalize_menu_language, LANG_EN};

/// 语言文件名。放在 app_config_dir 下与 window-state-*.json 同级。
const LANGUAGE_FILE: &str = "language";

/// 读取上次保存的语言；任何失败（不存在 / 不可读 / 无权限 / 内容非法）
/// 一律回落英文。
///
/// 不报错的理由：这是纯增强项。为了它让启动链路失败不可接受。
pub fn load_language(app_config_dir: &Path) -> &'static str {
    let path = app_config_dir.join(LANGUAGE_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => normalize_menu_language(text.trim()),
        Err(_) => LANG_EN,
    }
}

/// 保存语言。失败只返回错误，由调用方决定是否记录。
///
/// 不做原子写（临时文件 + rename）：文件只有一个词，且写坏的后果只是下次
/// 启动菜单语言回落英文，不值得多一次 fsync 的复杂度。`save_state` 对窗口
/// 几何用原子写是因为那份内容长、被截断的概率与后果都更高。
pub fn save_language(app_config_dir: &Path, language: &str) -> std::io::Result<()> {
    let normalized = normalize_menu_language(language);
    if let Some(parent) = app_config_dir.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::create_dir_all(app_config_dir);
    // 只写归一化后的值：落盘的即查表的依据，避免下次读到 "zh_CN" 之类
    // 再归一化一次（虽然 normalize 也能处理，但落盘规范化更易排查）。
    std::fs::write(app_config_dir.join(LANGUAGE_FILE), normalized)
}

/// 由前端设置命令调用：落盘 + 返回归一化后的语言（供调用方确认实际生效值）。
pub fn set_language(app_config_dir: &Path, language: &str) -> &'static str {
    let normalized = normalize_menu_language(language);
    if let Err(e) = save_language(app_config_dir, normalized) {
        log::warn!("保存界面语言失败（{language} -> {normalized}）: {e}");
    }
    normalized
}

/// 当前语言的菜单文案表（便捷入口，主要给 lib.rs 的 setup 用）。
pub fn menu_labels(app_config_dir: &Path) -> crate::menu::MenuLabels {
    labels_for(load_language(app_config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 落盘 -> 读回必须往返一致，且是归一化后的值。
    #[test]
    fn round_trips_and_normalizes() {
        let dir = temp_dir("lang_rt");
        save_language(&dir, "zh_CN").expect("写盘失败");
        assert_eq!(load_language(&dir), crate::menu::LANG_ZH_CN);
        // 读到的就是归一化形式（不是原始的 zh_CN）
        let raw = std::fs::read_to_string(dir.join(LANGUAGE_FILE)).expect("读原文件失败");
        assert_eq!(raw, "zh-CN");
    }

    /// 文件不存在时回落英文（首次启动的主路径）。
    #[test]
    fn missing_file_falls_back_to_english() {
        let dir = temp_dir("lang_missing").join("does-not-exist");
        assert_eq!(load_language(&dir), LANG_EN);
    }

    /// 目录不可创建 / 不可写时保存失败但不 panic，
    /// 且读回仍是上一次的有效值（不会被半截内容污染）。
    #[test]
    fn save_failure_leaves_previous_value() {
        let dir = temp_dir("lang_fail");
        save_language(&dir, "zh-CN").expect("先写一份成功值");
        // 指向一个必然创建失败的路径：用一个**文件**当父目录，
        // create_dir_all 会在其上失败。
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"not a dir").expect("建 blocker 文件失败");
        let bad_parent = blocker.join("sub");
        let result = save_language(&bad_parent, "en");
        assert!(result.is_err(), "应当失败而不是 panic");
        // 旧值未被动过
        assert_eq!(load_language(&dir), crate::menu::LANG_ZH_CN);
    }

    /// 非法内容（空串 / 乱码）必须回落英文而不是把菜单弄空白。
    #[test]
    fn garbage_content_falls_back_to_english() {
        let dir = temp_dir("lang_garbage");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LANGUAGE_FILE), b"   ").unwrap();
        assert_eq!(load_language(&dir), LANG_EN);
        std::fs::write(dir.join(LANGUAGE_FILE), "日本語").unwrap();
        assert_eq!(load_language(&dir), LANG_EN);
        std::fs::write(dir.join(LANGUAGE_FILE), "french").unwrap();
        assert_eq!(load_language(&dir), LANG_EN);
    }

    /// 大小写与空白都要吃下（手改文件的人不会讲究大小写）。
    #[test]
    fn tolerant_of_case_and_whitespace() {
        let dir = temp_dir("lang_case");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(LANGUAGE_FILE), "  ZH-cn \n").unwrap();
        assert_eq!(load_language(&dir), crate::menu::LANG_ZH_CN);
    }

    /// 测试辅助：唯一临时目录。
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "lazecode-langtest-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
