//! 文件分块读（大文件分页预览的后端一半）。
//!
//! 背景：`read_file_content` 整文件读入，8MB 软上限 / 256MB 硬上限 ——
//! 超限文件在编辑器里只能看到报错 +「强制打开」按钮。Zed 用 `Rope`
//!（分块 cord）让大文件可局部加载；本模块是零依赖版的第一步：
//! 按行号区间读（`start_line`/`max_lines`），输出有界，内存不随文件涨。
//!
//! v1 取舍（有意）：
//! - 不缓存行索引：每次调用扫一遍（单遍流式，无逐行分配）。256MB 上限内
//!   单次扫描几百毫秒，blocking 池执行；分页预览是低频操作，可接受。
//! - 上限与 `read_file_content(force=false)` 的硬上限同值
//!  （`MAX_READ_FILE_BYTES_FORCE`）：再大的文件连分块也不读 ——
//!   那种体积基本是磁盘镜像/模型权重，不是"文本预览"的目标。
//! - 非法 UTF-8 用 lossy 解码（预览不断行），与编辑器整文件读的严格语义不同，
//!   DTO 里不承诺往返写盘（只读预览用）。
//!
//! 前端调用方（后续项）：超限面板的「只读预览前 N 行」按钮。
//! 当前调用方：测试。命令已注册，可直接调。

use serde::Serialize;
use tauri::State;

use crate::commands::{authorize_workspace_path, WorkspaceRegistry};

/// 单次返回的最大行数：预览分页的页大小上限。调用方传更大即钳制，
/// 防 `max_lines = usize::MAX` 把整文件序列化回前端。
const MAX_RANGE_LINES: usize = 2000;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRangeDto {
    pub path: String,
    pub start_line: usize,
    pub lines: Vec<String>,
    /// 文件总行数（末行无换行也计一行，与 `str::lines` 同口径）
    pub total_lines: usize,
    /// 后面还有（`start + 返回行数 < 总行数`）
    pub has_more: bool,
}

/// 按行号区间读文件（1-based `start_line`，越界/非法参数即报错）。
///
/// 授权、体积守门、错误形态与 `read_file_content` 同口径（见 commands.rs），
/// 不各自发明：`file_too_large` 码 + 人类可读体积，读失败原文透出。
#[tauri::command]
pub async fn read_file_range(
    path: String,
    start_line: usize,
    max_lines: Option<usize>,
    registry: State<'_, WorkspaceRegistry>,
) -> Result<FileRangeDto, String> {
    read_file_range_impl(&path, start_line, max_lines, &registry).await
}

async fn read_file_range_impl(
    path: &str,
    start_line: usize,
    max_lines: Option<usize>,
    registry: &WorkspaceRegistry,
) -> Result<FileRangeDto, String> {
    if start_line == 0 {
        return Err("start_line 从 1 起".to_string());
    }
    let want = max_lines.unwrap_or(200).min(MAX_RANGE_LINES);
    if want == 0 {
        return Err("max_lines 必须大于 0".to_string());
    }
    let authorized = authorize_workspace_path(registry, path, false)?;
    // 预检（与 read_file_content_impl 同口径，含 TOCTOU 说明见彼处）
    if let Ok(meta) = std::fs::metadata(&authorized) {
        if meta.len() > crate::commands::MAX_READ_FILE_BYTES_FORCE {
            return Err(crate::error_proto::coded(
                "file_too_large",
                format!("{} ({})", path, bytes_len(meta.len())),
            ));
        }
    }
    let owned = authorized.to_string_lossy().to_string();
    let path_owned = path.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(meta) = std::fs::metadata(&owned) {
            if meta.len() > crate::commands::MAX_READ_FILE_BYTES_FORCE {
                return Err(crate::error_proto::coded(
                    "file_too_large",
                    format!("{} ({})", path_owned, bytes_len(meta.len())),
                ));
            }
        }
        let bytes =
            std::fs::read(&owned).map_err(|e| format!("Failed to read {}: {}", path_owned, e))?;
        // lossy：预览不断行（见模块头）；行切分走 slice_lines（与测试同一函数）。
        let text = String::from_utf8_lossy(&bytes);
        let (lines, total_lines, has_more) = slice_lines(&text, start_line, want);
        Ok(FileRangeDto {
            path: path_owned,
            start_line,
            has_more,
            total_lines,
            lines,
        })
    })
    .await
    .map_err(|e| format!("read_file_range 任务失败: {e}"))?
}

/// 人类可读体积（file_range 内部用；commands 的 format_bytes 是私有的，
/// 不为一行显示函数开 pub(crate) 口——重复十行比跨模块耦合便宜）。
fn bytes_len(n: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;
    const GB: u64 = 1024 * MB;
    if n >= GB {
        format!("{:.1} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

/// 测试直调的同步内核（行切分语义）：生产路径与测试走同一函数，
/// 切分口径（`str::lines`，末行无换行也计一行）只此一处。
fn slice_lines(text: &str, start_line: usize, want: usize) -> (Vec<String>, usize, bool) {
    let total = text.lines().count();
    let lines: Vec<String> = text
        .lines()
        .skip(start_line.saturating_sub(1))
        .take(want)
        .map(|l| l.to_string())
        .collect();
    let has_more = start_line.saturating_sub(1) + lines.len() < total;
    (lines, total, has_more)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::register_workspace_dir;
    use std::path::{Path, PathBuf};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("file_range_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 注册表桩：把 dir 登记为唯一授权根（走命令同款语义，不断言私有字段）。
    fn registry_for(dir: &Path) -> WorkspaceRegistry {
        let reg = WorkspaceRegistry::default();
        register_workspace_dir(&reg, dir.to_str().unwrap(), false).unwrap();
        reg
    }

    #[test]
    fn slice_middle_page() {
        let text = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, total, has_more) = slice_lines(&text, 4, 3);
        assert_eq!(lines, vec!["line4", "line5", "line6"]);
        assert_eq!(total, 10);
        assert!(has_more);
    }

    #[test]
    fn slice_tail_has_no_more() {
        let text = "a\nb\nc";
        let (lines, total, has_more) = slice_lines(text, 3, 10);
        assert_eq!(lines, vec!["c"]);
        assert_eq!(total, 3);
        assert!(!has_more);
    }

    #[test]
    fn slice_beyond_eof_is_empty() {
        let (lines, total, has_more) = slice_lines("a\nb", 99, 10);
        assert!(lines.is_empty());
        assert_eq!(total, 2);
        assert!(!has_more);
    }

    #[test]
    fn range_reads_end_to_end() {
        let dir = temp_dir("e2e");
        let file = dir.join("big.txt");
        let content = (1..=50)
            .map(|i| format!("L{i:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&file, &content).unwrap();
        let reg = registry_for(&dir);

        let dto = tauri::async_runtime::block_on(read_file_range_impl(
            file.to_str().unwrap(),
            11,
            Some(5),
            &reg,
        ))
        .expect("分块读");
        assert_eq!(dto.lines, vec!["L11", "L12", "L13", "L14", "L15"]);
        assert_eq!(dto.total_lines, 50);
        assert!(dto.has_more);

        // 参数守门
        assert!(tauri::async_runtime::block_on(read_file_range_impl(
            file.to_str().unwrap(),
            0,
            Some(5),
            &reg
        ))
        .is_err());
        assert!(tauri::async_runtime::block_on(read_file_range_impl(
            file.to_str().unwrap(),
            1,
            Some(0),
            &reg
        ))
        .is_err());
        // 越界路径被授权拒绝（不在登记根内）
        assert!(tauri::async_runtime::block_on(read_file_range_impl(
            "/etc/hosts",
            1,
            Some(5),
            &reg
        ))
        .is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn range_of_missing_file_errors_like_sibling() {
        let dir = temp_dir("missing");
        let reg = registry_for(&dir);
        let err = tauri::async_runtime::block_on(read_file_range_impl(
            dir.join("nope.txt").to_str().unwrap(),
            1,
            None,
            &reg,
        ))
        .unwrap_err();
        assert!(
            err.contains("Failed to read"),
            "与 read_file_content 同口径，实际: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
