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
//! 一次就能拿全。四条路都试，任一路成功即返回。
//!
//! # 图片（截图 / 浏览器「复制图片」）
//!
//! 截图（Ctrl+Cmd+Shift+4）给 `public.tiff`，浏览器「复制图片」给
//! `public.png` —— 两者都**没有** file URL flavor，前三路全空。这里把图片
//! 数据原样落成一个临时文件（`$TMPDIR/lazecode-paste-<纳秒>.png|.tiff`），
//! 返回路径交给前端走同一条附件链路：omp 认得图片路径（扩展名白名单含
//! `.png`/`.tiff`，omp 18.8.3 二进制 strings 实测），会自己画缩略图 chip。
//! 临时文件不主动删：omp 读图发生在粘贴后的几分钟内，早于 macOS 对临时
//! 目录的任何回收周期。
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
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG, NSPasteboardTypeTIFF,
};
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

/// 临时落盘路径：系统临时目录 + 纳秒时间戳，同一次剪贴板内绝不重名。
#[cfg(target_os = "macos")]
fn image_temp_path(ext: &str) -> Option<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("lazecode-paste-{nanos}.{ext}"));
    Some(path)
}

/// 读系统剪贴板，返回其中的**文件绝对路径**（按剪贴板顺序去重）。
///
/// 剪贴板没有文件时返回空 `Vec`（不是 Err）—— 普通文本粘贴是常态。
/// 剪贴板是图片（截图 / 浏览器复制图片）时把图片数据落成临时文件、
/// 返回该临时路径 —— 见模块文档「图片」一节。
///
/// 四条路径的取舍见模块文档；顺序是「URL flavor → 文件名 flavor →
/// 逐 item 兜底 → 图片落盘」，任一路成功即返回。
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
        if !out.is_empty() {
            return out;
        }

        // 路 4：图片 flavor（PNG 优先 —— 浏览器「复制图片」；TIFF 兜底 ——
        // 系统截图）。图片数据写临时文件后返回路径，前端照常走 bracketed
        // paste；写盘失败就静默放弃，不阻塞普通文本粘贴。
        for (ty, ext) in [(NSPasteboardTypePNG, "png"), (NSPasteboardTypeTIFF, "tiff")] {
            let Some(data) = pb.dataForType(ty) else {
                continue;
            };
            let Some(path) = image_temp_path(ext) else {
                continue;
            };
            let ns_path = NSString::from_str(&path.to_string_lossy());
            if data.writeToFile_atomically(&ns_path, true) {
                out.push(path);
                break;
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
    use objc2_foundation::NSData;

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

    /// 路 4 回归：剪贴板里是一张 PNG（浏览器「复制图片」的等价写入）时，
    /// `clipboard_file_paths` 必须把图片落成 `$TMPDIR/lazecode-paste-*.png`
    /// 并返回该路径 —— 这是「终端粘贴截图」功能的根基。真实触发（截图 /
    /// 浏览器复制图片）走的是 public.tiff / public.png flavor，本测试写
    /// public.png 覆盖其一。
    ///
    /// **会覆盖用户剪贴板**（与 `clipboard_file_paths_roundtrip` 同理），
    /// 手动跑：
    /// `cargo test clipboard_image_paste_roundtrip -- --ignored --test-threads=1`
    #[test]
    #[ignore = "会覆盖用户剪贴板"]
    fn clipboard_image_paste_roundtrip() {
        // 1×1 PNG（合法 PNG 头，内容无关紧要：路由只透传字节）
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        unsafe {
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            let data = NSData::with_bytes(png);
            assert!(
                pb.setData_forType(Some(&data), NSPasteboardTypePNG),
                "写 PNG flavor 失败"
            );
        }
        let paths = clipboard_file_paths();
        assert_eq!(
            paths.len(),
            1,
            "图片剪贴板应返回恰好一个临时路径: {paths:?}"
        );
        let p = &paths[0];
        assert!(p.is_absolute());
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
        assert!(
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .starts_with("lazecode-paste-"),
            "落盘文件应带 lazecode-paste- 前缀: {p:?}"
        );
        assert_eq!(
            std::fs::read(p).unwrap_or_default(),
            png,
            "落盘字节必须与剪贴板一致"
        );
        let _ = std::fs::remove_file(p);
    }

    /// 图片临时落盘路径：系统临时目录内、带给定扩展名、两次调用不重名。
    #[test]
    fn image_temp_path_is_unique_and_in_temp_dir() {
        let a = image_temp_path("png").expect("临时目录可用");
        let b = image_temp_path("tiff").expect("临时目录可用");
        assert_eq!(a.extension().and_then(|e| e.to_str()), Some("png"));
        assert_eq!(b.extension().and_then(|e| e.to_str()), Some("tiff"));
        assert_ne!(a, b, "两次调用不得重名");
        assert!(a.starts_with(std::env::temp_dir()));
        assert!(a
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .starts_with("lazecode-paste-"));
    }
}
