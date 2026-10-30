// 提交信息草稿：只保存在内存里，按仓库根隔离。
// 关闭提交抽屉会卸载 CommitDrawer，若草稿只放在组件 state，任何一次
// 「切到日志看提交再切回来」都会丢字。这里用模块级表跨挂载保留，
// 不落盘（提交信息里可能粘过 token，与 recentMessages 的隐私策略一致）。
const drafts = new Map<string, string>();

export function getCommitDraft(repoPath: string): string {
  return drafts.get(repoPath) ?? '';
}

export function saveCommitDraft(repoPath: string, message: string): void {
  if (message) drafts.set(repoPath, message);
  else drafts.delete(repoPath);
}

export function clearCommitDraft(repoPath: string): void {
  drafts.delete(repoPath);
}
