// 自动保存/淘汰写盘的准入规则（从 App.tsx 抽出，便于单测）。
//
// externalChanged：外部修改尚未由用户表态。自动保存绝不能替用户选择
// “本地覆盖磁盘”，否则冲突横幅还没看就丢了外部内容。显式 ⌘S 不走这里。

export interface AutoSaveCandidate {
  dirty: boolean;
  language: string;
  error: string | null;
  externalChanged: boolean;
}

export function shouldAutoSave(buffer: AutoSaveCandidate): boolean {
  return buffer.dirty && buffer.language !== 'image' && !buffer.error && !buffer.externalChanged;
}
