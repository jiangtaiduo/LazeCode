// 退出前写盘的纯计划层（便于单测）。
//
// 当前项目 + 所有项目快照都可能有未保存缓冲；退出时必须全部落盘，否则
// 后台项目的内容随进程消失。此函数只做「哪些该写、哪些被外部修改挡住」，
// 真正的 IPC 写盘由 App.flushAllDirtyBuffers 执行。

export interface ExitFlushBuffer {
  dirty: boolean;
  language: string;
  error: string | null;
  externalChanged: boolean;
  content: string;
  encoding?: string;
}

export interface ExitFlushWrite {
  path: string;
  content: string;
  encoding?: string;
}

export interface ExitFlushPlan {
  writes: ExitFlushWrite[];
  /** 检测到外部修改、不能自动覆盖的文件数；>0 时调用方应取消退出。 */
  blocked: number;
}

/**
 * 生成退出写盘计划。
 *
 * - groups[0] 是当前项目，后续是快照；同一路径只保留最先出现的版本
 *   （当前项目在用户眼前，优先于历史快照）；
 * - 图片 / 读取失败 / 未修改的缓冲跳过；
 * - externalChanged 计入 blocked，绝不自动覆盖磁盘上的外部修改。
 */
export function planExitFlush(groups: Record<string, ExitFlushBuffer>[]): ExitFlushPlan {
  const seen = new Set<string>();
  const writes: ExitFlushWrite[] = [];
  let blocked = 0;
  for (const buffers of groups) {
    for (const [path, buffer] of Object.entries(buffers)) {
      if (!buffer.dirty || buffer.language === 'image' || buffer.error) continue;
      if (buffer.externalChanged) {
        blocked += 1;
        continue;
      }
      if (seen.has(path)) continue;
      seen.add(path);
      writes.push({ path, content: buffer.content, encoding: buffer.encoding });
    }
  }
  return { writes, blocked };
}
