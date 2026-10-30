export interface FileNode {
  name: string;
  path: string;
  isDirectory: boolean;
  children?: FileNode[];
  gitStatus?: 'M' | 'A' | 'D' | 'U' | 'S';
  isOpen?: boolean;
}

export interface GitGraphEdge {
  from: number;
  to: number;
}

export interface GitCommit {
  hash: string;
  shortHash: string;
  message: string;
  author: string;
  authorEmail: string;
  date: string;
  branchTags?: { name: string; type: 'head' | 'local' | 'remote' | 'main' | 'tag' | 'stash' }[];
  /** 本提交节点所在轨道 */
  graphTrack: number;
  /** 上一行边界汇入本节点的连线（源轨道 -> 节点） */
  graphIns?: number[];
  /** 本行中径直穿过的轨道 */
  graphStraight?: number[];
  /** 本节点出发到下一行边界的连线 */
  graphOuts?: GitGraphEdge[];
  isMerge?: boolean;
  /** 是否为当前 HEAD 祖先（含 HEAD）；false = 不在当前历史，行背景置灰（仅 git_log 提供） */
  inHead?: boolean;
  /** 父提交完整哈希（合并提交多于一个） */
  parents?: string[];
  changedFiles?: { path: string; status: 'M' | 'A' | 'D'; additions: number; deletions: number }[];
}

export interface GitBranch {
  name: string;
  isCurrent: boolean;
  isRemote: boolean;
  tracking?: string;
  ahead?: number;
  behind?: number;
  isFavorite?: boolean;
  /** 前端把 tag 也渲染进分支树时置 true；后端 git_branches 不会返回该字段。 */
  isTag?: boolean;
}

export interface GitRepoInfo {
  isRepo: boolean;
  root: string;
  branch: string;
}

export interface GitRemote {
  name: string;
  url: string;
}

export interface GitTag {
  name: string;
  shortHash: string;
  date: string;
  message: string;
}

export interface GitSubmodule {
  path: string;
  url: string;
  branch: string;
  status: string;
}

export interface GitWorktree {
  path: string;
  head: string;
  branch: string;
  detached: boolean;
  bare: boolean;
  locked: boolean;
}

export interface GitLfsStatus {
  installed: boolean;
  version: string;
  files: string[];
  error: string;
}

export interface GitCredentialStatus {
  helper: string;
  osxKeychainAvailable: boolean;
}

/** sequencer 状态（git_sequencer_state）：合并/拣选/变基是否在途中 */
export interface SequencerState {
  merging: boolean;
  cherryPicking: boolean;
  rebasing: boolean;
  /** MERGE_HEAD 短 hash：Conflicts 副标题与 Theirs 列头用 */
  mergeHead: string | null;
  /** 被捡提交短 hash / 作者 / 主题：副标题 "made by …" 用 */
  cherryPickHead: string | null;
  cherryPickAuthor: string | null;
  cherryPickSubject: string | null;
}

/** 冲突种类（git_conflict_kinds / git_merge_file_content kind 口径） */
export type ConflictKind = 'content' | 'deleted_by_us' | 'deleted_by_them' | 'resolved';

/** 三路标记文本（git_merge_file_marked）：合并工具按 diff3 标记切块 */
export interface MergeMarked {
  path: string;
  merged: string;
}

export interface GitCommitFile {
  path: string;
  status: string;
  additions: number;
  deletions: number;
}

export interface GitCompareResult {
  /** ref 上有而当前 HEAD 没有的提交 */
  commits: GitCommit[];
  /** HEAD 与 ref 的三点差异文件 */
  files: GitCommitFile[];
}

/** Compare Branches：左右两侧各自“另一侧没有”的提交 + 各自相对共同祖先的文件。 */
export interface GitCompareBranchesResult {
  leftCommits: GitCommit[];
  rightCommits: GitCommit[];
  leftFiles: GitCommitFile[];
  rightFiles: GitCommitFile[];
  /** 两分支共同祖先；无共同历史时为空 */
  mergeBase: string;
  leftTruncated: boolean;
  rightTruncated: boolean;
}

export interface BlameLine {
  /** 工作区文件中的行号（1 起） */
  line: number;
  hash: string;
  shortHash: string;
  author: string;
  date: string;
}

export interface GitStash {
  /** stash@{N} 中的 N */
  index: number;
  /** 完整引用名，如 "stash@{0}" */
  selector: string;
  hash: string;
  shortHash: string;
  /** 贮藏说明：自定义消息或默认 "WIP on ..." */
  message: string;
  /** 创建时的分支名（从消息解析，解析不出为空） */
  branch: string;
  date: string;
}

export interface GitChangedFile {
  path: string;
  status: 'M' | 'A' | 'D' | 'U' | 'C' | 'I' | 'R';
  /** index 相对 HEAD 有变化 */
  isStaged: boolean;
  /** 工作区相对 index 有变化（含未跟踪/冲突）；可与 isStaged 同时为 true */
  isUnstaged: boolean;
  additions: number;
  deletions: number;
}

export interface FileChangeHunk {
  startLine: number;
  endLine: number;
  kind: 'added' | 'modified' | 'removed';
}

export interface Shelf {
  name: string;
  date: string;
  /** 补丁涉及的文件路径（b 侧），Shelf 页据此渲染清单 */
  files: string[];
}

export interface ShelfPreviewDto {
  oldPath: string;
  newPath: string;
  title: string;
}

/** 本地变更列表（存于仓库 .git/ide-changelists.json） */
export interface Changelists {
  /** 变更列表名集合，首项恒为默认列表 Changes */
  lists: string[];
  /** 路径 → 列表名；不在表中的路径属于默认列表 */
  files: Record<string, string>;
}

export interface MergeFileContent {
  path: string;
  /** content=正文冲突；deleted_by_us=我删对改；deleted_by_them=对删我改；resolved=外部已解决 */
  kind: 'content' | 'deleted_by_us' | 'deleted_by_them' | 'resolved';
  base: string | null;
  ours: string;
  theirs: string;
  worktree: string;
}

export interface TerminalSession {
  id: string;
  title: string;
  status: 'running' | 'busy' | 'idle';
  processName: string;
}

export interface DiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  oldContent: string[];
  newContent: string[];
  isStaged?: boolean;
}
