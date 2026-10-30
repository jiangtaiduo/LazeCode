//! 剪贴板文件路径读取：Finder Cmd+C 复制的文件，在终端里 Cmd+V 时粘贴绝对路径。
//!
//! # 为什么必须在 Rust 侧做
//!
//! 浏览器的 `ClipboardEvent.clipboardData.files` 给的是 `File` 对象，
//! **只有 name/type/size，没有绝对路径**（实测：macOS 27 上 `File` 无
//! `webkitRelativePath`、无任何路径属性）。而 NSPasteboard 单文件时
//! `public.file-url` / `NSFilenamesPboardType` 都带完整 POSIX 路径 ——
//! 信息在 OS 侧有，前端拿不到。前端能拿到的只有文件名，拼不出选中项。
//!
//! # 多文件时的坑
//!
//! 实测（`osascript` 写的多文件 list）：NSPasteboard 的 `types` 只有一个
//! `dyn.ah62d4rv4gk8024pxsu`（动态 UTI，即「list」flavor），
//! `NSFilenamesPboardType` 与 `public.file-url` 都是 nil，`pasteboardItems`
//! 也只有一个带 `dyn.*` 类型的 item —— **标准 flavor 全部读不到**。
//! 好消息是那只是 AppleScript 的写法：Finder 真实 Cmd+C 多选时会为每个
//! 文件写一个 `NSPasteboardTypeFileURL` item，`readObjects(forClasses:)`
//! 一次就能拿全。三条路都试，任一路成功即返回。
//!
//! # 边界
//!
//! - 目录也算「文件」：Finder 复制的文件夹同样给绝对路径，前端按目录渲染。
//! - 剪贴板里**没有**文件时返回空 Vec，不是错误 —— 普通文本粘贴是常态。
//! - 路径可能不存在（复制后被删）：只做存在性探测，不阻止粘贴。
//! - 路径不做授权过滤：这是用户主动粘贴，不是仓库内容诱导。

#[cfg(target_os = "macos")]
use objc2::ClassType;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSString, NSURL};
use std::path::PathBuf;

/// macOS 上 Finder 等写入的文件名数组 flavor。取值是 POSIX 路径字符串数组。
#[cfg(target_os = "macos")]
const NSFILENAMES_PBOARD_TYPE: &str = "NSFilenamesPboardType";

/// 归一化剪贴板里的路径字符串。
///
/// 去掉首尾空白；AppleScript `set the clipboard to list` 的量词模式会给每项
/// 包一层引号，一并剥掉。**不**做 `canonicalize` —— 软链接/`..` 交给调用方
/// 按需判断，这里只保证不把明显非路径的垃圾当文件返回（要求以 `/` 开头）。
#[cfg(target_os = "macos")]
fn normalize_path_string(raw: &str) -> Option<PathBuf> {
    let s = raw.trim();
    let unquoted = if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].trim().to_string()
    } else {
        s.to_string()
    };
    if unquoted.is_empty() || !unquoted.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(unquoted))
}

/// 从 NSURL 取文件路径。非 file scheme（http:/data:）返回 None。
#[cfg(target_os = "macos")]
fn path_from_nsurl(url: &NSURL) -> Option<PathBuf> {
    if !url.isFileURL() {
        return None;
    }
    let s = url.path()?;
    normalize_path_string(&s.to_string())
}

/// 读系统剪贴板，返回其中的**文件绝对路径**（按剪贴板顺序去重）。
///
/// 剪贴板没有文件时返回空 `Vec`（不是 Err）—— 普通文本粘贴是常态。
///
/// 三条路径的取舍见模块文档；顺序是「URL flavor → 文件名 flavor →
/// 逐 item 兜底」，任一路成功即返回。
#[cfg(target_os = "macos")]
pub fn clipboard_file_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    unsafe {
        let pb = NSPasteboard::generalPasteboard();

        // 路 1：Finder 单/多文件复制 —— 每个文件一个 NSPasteboardTypeFileURL
        // item，readObjects(forClasses: [NSURL.self]) 一次拿全。
        let classes = NSArray::from_slice(&[NSURL::class()]);
        if let Some(objs) = pb.readObjectsForClasses_options(&classes, None) {
            for obj in objs.iter() {
                if let Some(url) = obj.downcast_ref::<NSURL>() {
                    if let Some(p) = path_from_nsurl(url) {
                        if !out.contains(&p) {
                            out.push(p);
                        }
                    }
                }
            }
        }
        if !out.is_empty() {
            return out;
        }

        // 路 2：NSFilenamesPboardType —— POSIX 路径字符串数组。
        let key = NSString::from_str(NSFILENAMES_PBOARD_TYPE);
        if let Some(plist) = pb.propertyListForType(&key) {
            if let Ok(arr) = plist.downcast::<NSArray>() {
                for item in arr.iter() {
                    if let Some(s) = item.downcast_ref::<NSString>() {
                        if let Some(p) = normalize_path_string(&s.to_string()) {
                            if !out.contains(&p) {
                                out.push(p);
                            }
                        }
                    }
                }
            }
        }
        if !out.is_empty() {
            return out;
        }

        // 路 3：逐 item 兜底。item 类型必须是 NSPasteboardTypeFileURL；
        // 动态 UTI（AppleScript list）一律解析不出，跳过。
        if let Some(items) = pb.pasteboardItems() {
            for item in items.iter() {
                if let Some(s) = item.stringForType(NSPasteboardTypeFileURL) {
                    if let Some(p) = normalize_path_string(&s.to_string()) {
                        if !out.contains(&p) {
                            out.push(p);
                        }
                    }
                }
            }
        }
    }
    out
}

/// 供测试：把给定路径数组写进系统剪贴板（Finder 单文件复制的等价实现）。
///
/// 只写 `NSFilenamesPboardType` —— 这是唯一不依赖拖放/外部程序、只用一个
/// flavor 就能表达「一组文件」的接口，覆盖路径 2。
///
/// `pub` 而非 `pub(crate)`：集成测试（`tests/clipboard_paths.rs`）要复用它
/// 构造「剪贴板里有文件」这一前置状态。产物里是死代码 —— 发布二进制不跑
/// 测试，链接器会连同其唯一调用方一起消除。
///
/// # Safety
///
/// 会**清空并覆盖用户剪贴板**，因此调用方必须确认当前没有正在使用的剪贴板
/// 内容（测试场景应 `--test-threads=1` 串行跑）。
#[cfg(target_os = "macos")]
pub unsafe fn set_clipboard_file_names_for_test(paths: &[&str]) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let items_obj: Vec<objc2::rc::Retained<NSString>> =
        paths.iter().map(|p| NSString::from_str(p)).collect();
    let arr = NSArray::from_retained_slice(&items_obj);
    let _ = pb.setPropertyList_forType(&arr, &NSString::from_str(NSFILENAMES_PBOARD_TYPE));
}

/// 非 macOS：没有等价的原生剪贴板文件 flavor 实现，返回空让前端按纯文本粘贴。
#[cfg(not(target_os = "macos"))]
pub fn clipboard_file_paths() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_plain_absolute_path() {
        assert_eq!(
            normalize_path_string("/tmp/a.md"),
            Some(PathBuf::from("/tmp/a.md"))
        );
    }

    #[test]
    fn normalize_rejects_relative_and_empty() {
        assert_eq!(normalize_path_string(""), None);
        assert_eq!(normalize_path_string("   "), None);
        assert_eq!(normalize_path_string("relative/a.md"), None);
    }

    /// AppleScript `set the clipboard to list` 的量词模式会把每项包在引号里。
    #[test]
    fn normalize_strips_quotes_from_applescript_list_mode() {
        assert_eq!(
            normalize_path_string("\"/tmp/sample note.md\""),
            Some(PathBuf::from("/tmp/sample note.md"))
        );
        assert_eq!(
            normalize_path_string("'/tmp/x.md'"),
            Some(PathBuf::from("/tmp/x.md"))
        );
    }

    #[test]
    fn normalize_keeps_spaces_inside_path() {
        assert_eq!(
            normalize_path_string("/tmp/a b/c d.md"),
            Some(PathBuf::from("/tmp/a b/c d.md")),
            "路径内含空格必须保留"
        );
    }

    /// 往系统剪贴板写文件名数组再读回 —— 证明路径 2 在真实 NSPasteboard 上可用。
    ///
    /// **会覆盖用户剪贴板**。测试并行跑时另一个线程可能同时动剪贴板，
    /// 故默认 `#[ignore]`；改完本模块后手动跑：
    /// `cargo test clipboard_file_paths_roundtrip -- --ignored --test-threads=1`
    #[test]
    #[ignore = "会覆盖用户剪贴板"]
    fn clipboard_file_paths_roundtrip() {
        unsafe { set_clipboard_file_names_for_test(&["/tmp/a.md", "/tmp/b c.md"]) };
        assert_eq!(
            clipboard_file_paths(),
            vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/b c.md")]
        );
    }

    /// 直接读当前真实剪贴板：不 panic、返回的路径全部绝对。
    ///
    /// 不断言为空 —— 剪贴板里恰有文件时返回非空也是正确行为。
    #[test]
    fn clipboard_file_paths_never_panics_and_returns_absolute() {
        for p in clipboard_file_paths() {
            assert!(p.is_absolute(), "返回的必须是绝对路径: {p:?}");
        }
    }
}
