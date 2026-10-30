// 文件探针快路径（变更检测的零读盘判定，见 file_cache.rs / fileWatch.ts）。
//
// 本模块**零运行时 import**（与 telemetry.ts 同一可验证性约束）：
// 纯逻辑放这里，node 验证脚本可直跑；副作用（file_probe invoke、事件订阅）
// 留在 fileWatch.ts。fileWatch 重导出，保持调用方 import 位置不变。

/** 决策所需的最小缓冲视图：FileBuffer 的结构子集，避免对 App.tsx 的循环依赖 */
export interface WatchedBufferView {
  content: string;
  diskHash: string;
  dirty: boolean;
  externalChanged: boolean;
  /**
   * 磁盘基线探针（与 diskHash 同一次读盘/写盘时记录的 `(len, mtime)`）。
   * null/undefined = 基线未知（旧会话恢复、图片占位），探针快路径不启用，
   * 退化为今天的全量读行为。
   */
  diskLen?: number | null;
  diskMtimeMs?: number | null;
}

/** 与 Rust FileProbeDto 对应（serde camelCase）：只 stat，不读内容 */
export interface FileProbe {
  exists: boolean;
  isFile: boolean;
  len: number;
  /**
   * 磁盘 mtime（毫秒）。null = 文件系统给不出——此时**必须**走全量读，
   * 不能拿"未知"当"未变"（与 Rust FileCache::get 同一约定）。
   */
  mtimeMs: number | null;
}

/**
 * 探针快路径判定（纯函数，可独立测试）：
 * - `'unchanged'`：`(len, mtimeMs)` 与基线完全一致 → 磁盘内容 == 基线内容，
 *   等价于 `resolveExternalChange` 的 unchanged 分支（调用方据 externalChanged
 *   决定 noop 还是 resolved），**零读盘**；
 * - `'changed'`：探针与基线对不上（或基线/mtime 未知）→ 调用方继续全量读，
 *   走今天的哈希精确判定；
 * - `'missing'`：探针说文件没了 → 调用方走既有的"已删除/不可读"分支。
 *
 * 别名风险（已接受，见 file_cache.rs）：同一 mtime 粒度内等长改写会撞过
 * 快路径。撞过只延迟到下一次事件才发现，不丢数据、不误覆盖——外部改了但
 * 本轮没看到，最坏是横幅晚出现一次。
 */
export function probeFastPath(
  buffer: WatchedBufferView,
  probe: FileProbe
): 'unchanged' | 'changed' | 'missing' {
  if (!probe.exists) return 'missing';
  const baselineKnown =
    buffer.diskHash !== '' &&
    buffer.diskLen != null &&
    buffer.diskMtimeMs != null &&
    probe.mtimeMs != null;
  if (
    baselineKnown &&
    probe.len === buffer.diskLen &&
    probe.mtimeMs === buffer.diskMtimeMs
  ) {
    return 'unchanged';
  }
  return 'changed';
}
