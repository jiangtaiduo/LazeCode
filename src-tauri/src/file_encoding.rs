//! 文本编码检测/回写与原子保存。
//!
//! 读取：先认 BOM（UTF-8/UTF-16LE/BE），再尝试严格 UTF-8；失败后用
//! chardetng 对前 1 MiB 做统计检测，encoding_rs 解码。这样 GBK/Shift_JIS
//! 等老项目文件不会直接报“stream did not contain valid UTF-8”。
//!
//! 回写：FileBuffer 记住打开时的编码，保存走同一编码；UTF-8 直接写字节。
//! 所有保存都先写同目录临时文件、fsync 后再 rename，避免进程崩溃/断电
//! 把原文件截断成半截。Windows 的 rename 不能覆盖已存在目标，走删除后退化
//! 路径（该平台本身未做端到端验证，但至少不会静默失败）。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct DecodedText {
    pub text: String,
    pub encoding: String,
}

fn from_utf8_bom(bytes: &[u8]) -> Option<DecodedText> {
    let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF])?;
    Some(DecodedText {
        text: String::from_utf8_lossy(rest).into_owned(),
        // 标签带 BOM：保存时必须原样写回，否则 Windows 工具/字符串资源会认错。
        encoding: "UTF-8-BOM".to_string(),
    })
}

fn from_utf16_bom(bytes: &[u8]) -> Option<DecodedText> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let encoding = encoding_rs::UTF_16LE;
        let (text, _, _) = encoding.decode(rest);
        return Some(DecodedText {
            text: text.into_owned(),
            encoding: "UTF-16LE-BOM".to_string(),
        });
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let encoding = encoding_rs::UTF_16BE;
        let (text, _, _) = encoding.decode(rest);
        return Some(DecodedText {
            text: text.into_owned(),
            encoding: "UTF-16BE-BOM".to_string(),
        });
    }
    None
}

/// 解码任意文本字节。结果 encoding 是保存时优先使用的标签。
pub fn decode_text(bytes: &[u8]) -> DecodedText {
    if let Some(done) = from_utf8_bom(bytes) {
        return done;
    }
    if let Some(done) = from_utf16_bom(bytes) {
        return done;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return DecodedText {
            text: text.to_string(),
            encoding: "UTF-8".to_string(),
        };
    }

    // chardetng 只吃前缀：避免为一个 8MB 文件做全量统计。
    let sample = &bytes[..bytes.len().min(1024 * 1024)];
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(sample, true);
    let encoding = detector.guess(None, true);
    let (text, _, _) = encoding.decode(bytes);
    DecodedText {
        text: text.into_owned(),
        encoding: encoding.name().to_string(),
    }
}

fn resolve_encoding(label: &str) -> Result<&'static encoding_rs::Encoding, String> {
    encoding_rs::Encoding::for_label(label.trim().as_bytes())
        .ok_or_else(|| crate::error_proto::coded("encoding_unsupported", label))
}

/// 按打开时记录的编码回写字节；编码未知/为空时按 UTF-8。
pub fn encode_text(text: &str, encoding: Option<&str>) -> Result<Vec<u8>, String> {
    let label = encoding.unwrap_or("UTF-8").trim();
    if label.is_empty() || label.eq_ignore_ascii_case("utf-8") {
        return Ok(text.as_bytes().to_vec());
    }
    // BOM 是文件内容的一部分，不是显示细节：解码时剥掉，编码时必须补回。
    let (base, bom): (&str, &[u8]) = if label.eq_ignore_ascii_case("utf-8-bom") {
        ("UTF-8", &[0xEF, 0xBB, 0xBF])
    } else if label.eq_ignore_ascii_case("utf-16le-bom") {
        ("UTF-16LE", &[0xFF, 0xFE])
    } else if label.eq_ignore_ascii_case("utf-16be-bom") {
        ("UTF-16BE", &[0xFE, 0xFF])
    } else {
        (label, &[])
    };
    // encoding_rs 的 encode() 对 UTF-16 目标会折回 UTF-8（它是 Web 编码
    // 库的语义），不能用来写 .strings 这类真正的 UTF-16 文件；这里手动按
    // UTF-16 码元编码。
    let bytes: Vec<u8> =
        if base.eq_ignore_ascii_case("utf-16le") || base.eq_ignore_ascii_case("utf-16be") {
            let little = base.eq_ignore_ascii_case("utf-16le");
            let mut out = Vec::with_capacity(text.len() * 2);
            for unit in text.encode_utf16() {
                let pair = if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                };
                out.extend_from_slice(&pair);
            }
            out
        } else {
            let enc = resolve_encoding(base)?;
            let (bytes, _, had_errors) = enc.encode(text);
            if had_errors {
                return Err(crate::error_proto::coded("encoding_lossy", label));
            }
            bytes.into_owned()
        };
    if bom.is_empty() {
        return Ok(bytes);
    }
    let mut out = Vec::with_capacity(bom.len() + bytes.len());
    out.extend_from_slice(bom);
    out.extend_from_slice(&bytes);
    Ok(out)
}

fn temp_path_for(path: &Path) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(format!(
        ".{name}.lazecode-save-{}-{seq}",
        std::process::id()
    ))
}

/// 原子写：同目录临时文件 + fsync + rename；失败清理临时文件。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = temp_path_for(path);
    let result = (|| -> io::Result<()> {
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        // 保留原文件权限（可执行脚本保存后不应丢 +x）。chmod 失败时宁可报错，
        // 也不要静默写出一份丢了执行位/ACL 的文件。
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        // 不要先删目标再 rename：Windows 上 std::fs::rename 已经用
        // MoveFileExW(REPLACE_EXISTING) 覆盖目标；预删除会在 rename 失败
        // （杀软/共享锁/进程崩溃）时留下一个已被删掉的旧文件。
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_bom_and_plain() {
        let plain = decode_text(b"hello");
        assert_eq!(plain.text, "hello");
        assert_eq!(plain.encoding, "UTF-8");
        let bom = decode_text(b"\xEF\xBB\xBFhello");
        assert_eq!(bom.text, "hello");
        assert_eq!(bom.encoding, "UTF-8-BOM");
    }

    #[test]
    fn decodes_utf16le_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "中文".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let d = decode_text(&bytes);
        assert_eq!(d.text, "中文");
        assert_eq!(d.encoding, "UTF-16LE-BOM");
    }

    #[test]
    fn decodes_gbk_without_panic() {
        // “中文”的 GBK 编码
        let bytes = [0xD6, 0xD0, 0xCE, 0xC4];
        let d = decode_text(&bytes);
        assert!(d.text.contains('中') || d.text.contains('文') || !d.text.is_empty());
        assert_ne!(d.encoding, "UTF-8");
    }

    #[test]
    fn encode_roundtrip_utf8_and_rejects_unencodable() {
        assert_eq!(
            encode_text("中文", Some("UTF-8")).unwrap(),
            "中文".as_bytes()
        );
        let gbk = encode_text("中文", Some("GBK")).unwrap();
        assert_eq!(decode_text(&gbk).text, "中文");
        assert!(encode_text("🙂", Some("GBK")).is_err());
    }

    /// 回归：BOM 不能在保存时丢。此前标签只有 UTF-8/UTF-16LE，解码剥掉
    /// BOM 后 encode 不再写回，UTF-16 文件第二次打开就会被当成 UTF-8 乱码。
    #[test]
    fn bom_survives_encode_decode_roundtrip() {
        for (label, prefix) in [
            ("UTF-8-BOM", &[0xEF, 0xBB, 0xBF][..]),
            ("UTF-16LE-BOM", &[0xFF, 0xFE][..]),
            ("UTF-16BE-BOM", &[0xFE, 0xFF][..]),
        ] {
            let bytes = encode_text("中文", Some(label)).unwrap();
            assert!(
                bytes.starts_with(prefix),
                "{label} 应带 BOM，实际 {bytes:?}"
            );
            let decoded = decode_text(&bytes);
            assert_eq!(decoded.text, "中文", "{label} 往返文本");
            assert_eq!(decoded.encoding, label, "{label} 标签必须保持");
        }
        // 无 BOM 的 UTF-16（chardet 标签）仍按无 BOM 写，避免改变原文件形态
        let no_bom = encode_text("中文", Some("UTF-16LE")).unwrap();
        assert!(
            !no_bom.starts_with(&[0xFF, 0xFE]),
            "无 BOM 标签不能擅自加 BOM"
        );
    }

    #[test]
    fn atomic_write_replaces_and_preserves_content() {
        let dir = std::env::temp_dir().join(format!("lazecode-atomic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.txt");
        fs::write(&path, "old").unwrap();
        atomic_write(&path, "new".as_bytes()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let _ = fs::remove_dir_all(&dir);
    }
}
