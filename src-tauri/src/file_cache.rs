//! 文件内容读缓存 + 轻量探针（Zed 快照共享思想的平价版）。
//!
//! Zed 的 Rope 快照是 `Arc` 引用计数：N 个消费者（渲染/解析/保存/协作）
//! 共享同一份文本，后台线程拿快照零拷贝。LazeCode 是 Tauri IPC 边界：
//! 每次 `read_file_content` 都要走「读盘 → String → serde → JS string」
//! 全链路复制，同一文件在外部变更检测/重开标签/Diff 取侧时被反复全量拉取。
//!
//! 本模块做两件事（都不改变已有命令的语义，只省重复劳动）：
//! 1. `FileCache`：读穿缓存。key = 规范化绝对路径，命中条件 = 磁盘
//!    `(len, mtime)` 与缓存条目一致。命中省一次读盘 + 一次瞬时大分配；
//!    峰值被 `FILE_CACHE_MAX_*` 预算封顶（Zed 的教训：无预算的缓存就是泄漏）。
//! 2. `probe_of`：`file_probe` 命令的无状态实现——只 stat 不读内容，
//!    前端用它做变更检测的快路径（mtime+len 未变直接跳过全量读）。
//!
//! 缓存一致性：写入后由 `write_file_content_impl` 显式 `invalidate`；
//! 读-存之间的 TOCTOU 由调用方收敛——存入前复核 stat，不一致就丢弃这次存入
//! （下次读重新命中判断，不会返回错内容，只多一次读盘）。
//!
//! 锁毒化一律按"缓存失效"处理（返回 None / 跳过存入），不把 poison 传给调用方：
//! 缓存是纯优化层，它坏了最多退化成今天的行为，不能让读文件失败。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 缓存最多保留的文件数：FIFO 淘汰，超过即从最旧开始丢。
pub const FILE_CACHE_MAX_FILES: usize = 64;
/// 缓存总字节预算：超过即从最旧开始淘汰，直到装下新条目或清空。
pub const FILE_CACHE_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// 单文件进缓存上限：与 `read_file_content` 软上限同口径。`force=true` 的
/// 大文件（可达 256MB）不进缓存——一条就能吃掉全部预算。
pub const FILE_CACHE_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

struct CachedFile {
    len: u64,
    mtime_ms: Option<u64>,
    content: Arc<str>,
}

#[derive(Default)]
struct CacheInner {
    entries: HashMap<PathBuf, CachedFile>,
    /// 插入顺序（FIFO 淘汰用）。invalidate 时懒删除：弹出时对不上即跳过。
    order: VecDeque<PathBuf>,
    bytes: u64,
}

/// 读穿文件缓存（`tauri::State` 托管，见 lib.rs）。
#[derive(Default)]
pub struct FileCache {
    inner: Mutex<CacheInner>,
}

/// `metadata.modified()` → 自 UNIX_EPOCH 的毫秒数。拿不到（罕见文件系统）
/// 给 None：调用方把 None 当"未知"，命中判断要求两侧同时已知才放行。
pub fn mtime_ms_of(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

impl FileCache {
    /// 命中当且仅当：条目存在 **且** `(len, mtime)` 与当前 stat 完全一致。
    /// mtime 只要有一侧未知就不命中（宁可多读一次，不返回疑似过期内容）。
    pub fn get(&self, canonical: &Path, len: u64, mtime_ms: Option<u64>) -> Option<Arc<str>> {
        let inner = self.inner.lock().ok()?;
        let entry = inner.entries.get(canonical)?;
        if entry.len != len {
            return None;
        }
        match (entry.mtime_ms, mtime_ms) {
            (Some(a), Some(b)) if a == b => Some(Arc::clone(&entry.content)),
            _ => None,
        }
    }

    /// 存入（读穿未命中后由调用方调）。调用方**必须**在读盘成功后复核一次
    /// stat 并把复核值传进来——预检与读取之间的改动会导致存入错版本，
    /// 复核不一致时这里直接丢弃（返回 false），保证缓存里永远是自洽版本。
    pub fn insert(
        &self,
        canonical: PathBuf,
        len: u64,
        mtime_ms: Option<u64>,
        content: Arc<str>,
    ) -> bool {
        if len > FILE_CACHE_MAX_FILE_BYTES {
            return false;
        }
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        // 同路径复写：先减掉旧账，避免 bytes 虚增。
        if let Some(old) = inner.entries.remove(&canonical) {
            inner.bytes = inner.bytes.saturating_sub(old.len);
        }
        // 预算收敛：FIFO 淘汰直到装下（单文件已保证 ≤ 单文件上限，
        // 极端下清空整个缓存也装得下，循环必终止）。
        while (inner.entries.len() >= FILE_CACHE_MAX_FILES
            || inner.bytes.saturating_add(len) > FILE_CACHE_MAX_BYTES)
            && !inner.entries.is_empty()
        {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            if let Some(evicted) = inner.entries.remove(&oldest) {
                inner.bytes = inner.bytes.saturating_sub(evicted.len);
            }
        }
        inner.bytes = inner.bytes.saturating_add(len);
        inner.order.push_back(canonical.clone());
        inner.entries.insert(
            canonical,
            CachedFile {
                len,
                mtime_ms,
                content,
            },
        );
        true
    }

    /// 写盘/删除后调用：无条件丢弃该路径条目（不需要 stat）。
    pub fn invalidate(&self, canonical: &Path) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if let Some(old) = inner.entries.remove(canonical) {
            inner.bytes = inner.bytes.saturating_sub(old.len);
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().map(|i| i.entries.len()).unwrap_or(0)
    }
}

/// 轻量探针结果（只 stat，不读内容）。
pub struct FileProbe {
    pub exists: bool,
    pub is_file: bool,
    pub len: u64,
    pub mtime_ms: Option<u64>,
}

/// 对已授权路径做探针：跟随软链的 `metadata`（与读路径一致——读得到什么，
/// 探针就量什么；`fs_path_exists` 的 symlink_metadata 是存在性语义，两者不同）。
pub fn probe_of(canonical: &Path) -> FileProbe {
    match std::fs::metadata(canonical) {
        Ok(meta) => FileProbe {
            exists: true,
            is_file: meta.is_file(),
            len: meta.len(),
            mtime_ms: mtime_ms_of(&meta),
        },
        Err(_) => FileProbe {
            exists: false,
            is_file: false,
            len: 0,
            mtime_ms: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_file(name: &str, content: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ide_filecache_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn hit_requires_len_and_mtime_match() {
        let cache = FileCache::default();
        let p = tmp_file("hit.txt", b"hello");
        let meta = std::fs::metadata(&p).unwrap();
        let content: Arc<str> = Arc::from("hello");
        assert!(cache.insert(
            p.clone(),
            meta.len(),
            mtime_ms_of(&meta),
            Arc::clone(&content)
        ));

        // 未变化 → 命中
        let meta2 = std::fs::metadata(&p).unwrap();
        let hit = cache.get(&p, meta2.len(), mtime_ms_of(&meta2));
        assert!(hit.is_some(), "未改动的文件应命中");
        assert_eq!(hit.unwrap().as_ref(), "hello");

        // 内容变（len 变）→ 不命中
        std::fs::write(&p, "hello world, longer").unwrap();
        let meta3 = std::fs::metadata(&p).unwrap();
        assert!(cache.get(&p, meta3.len(), mtime_ms_of(&meta3)).is_none());

        // 同长度改写也可能撞 len，但 mtime 前进 → 不命中
        std::fs::write(&p, "hello world, LONGEr").unwrap();
        let meta4 = std::fs::metadata(&p).unwrap();
        assert_eq!(meta4.len(), meta3.len());
        // mtime 粒度在部分文件系统是秒级：不断言此处一定不命中（由调用方复核兜底），
        // 只断言 invalidate 后必不命中。
        cache.invalidate(&p);
        assert!(cache.get(&p, meta4.len(), mtime_ms_of(&meta4)).is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn unknown_mtime_never_hits() {
        let cache = FileCache::default();
        let p = tmp_file("nomtime.txt", b"x");
        assert!(cache.insert(p.clone(), 1, None, Arc::from("x")));
        assert!(
            cache.get(&p, 1, Some(123)).is_none(),
            "存入时 mtime 未知则永不命中"
        );
        assert!(cache.get(&p, 1, None).is_none(), "两侧都未知也不命中");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn oversize_single_file_rejected_and_budget_bounded() {
        let cache = FileCache::default();
        let big: Arc<str> = Arc::from("x".repeat(16).as_str());
        assert!(
            !cache.insert(
                PathBuf::from("/tmp/ide_filecache_big"),
                FILE_CACHE_MAX_FILE_BYTES + 1,
                Some(1),
                big
            ),
            "超单文件上限拒绝存入"
        );
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn fifo_evicts_oldest_when_full() {
        let cache = FileCache::default();
        // 每次 1MB，70 个 > 64 文件上限，触发 FIFO 淘汰
        let chunk: Arc<str> = Arc::from("y".repeat(1024 * 1024).as_str());
        for i in 0..70 {
            cache.insert(
                PathBuf::from(format!("/tmp/ide_filecache_fifo_{i}")),
                chunk.len() as u64,
                Some(i as u64),
                Arc::clone(&chunk),
            );
        }
        assert!(
            cache.len() <= FILE_CACHE_MAX_FILES,
            "文件数被上限封顶，实际 {}",
            cache.len()
        );
        // 最旧的已被淘汰，最新的还在
        assert!(cache
            .get(
                &PathBuf::from("/tmp/ide_filecache_fifo_0"),
                chunk.len() as u64,
                Some(0)
            )
            .is_none());
        assert!(cache
            .get(
                &PathBuf::from("/tmp/ide_filecache_fifo_69"),
                chunk.len() as u64,
                Some(69)
            )
            .is_some());
    }

    #[test]
    fn probe_reports_stat_without_reading() {
        let p = tmp_file("probe.txt", b"12345");
        let probe = probe_of(&p);
        assert!(probe.exists && probe.is_file);
        assert_eq!(probe.len, 5);
        assert!(probe.mtime_ms.is_some());
        let missing = probe_of(&p.with_file_name("no-such-file-xyz.txt"));
        assert!(!missing.exists);
        let _ = std::fs::remove_file(&p);
    }
}
