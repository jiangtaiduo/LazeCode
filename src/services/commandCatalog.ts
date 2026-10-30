// 命令目录（对齐 Zed `actions!` 把命令数据化的思想）。
//
// 此前命令只有名字（`TauriCommand` 联合类型防拼错，但"这条命令是干什么的、
// 有多危险"只存在于各调用点的注释里）。这里是全部 106 条命令的唯一清单：
// 标题（双语）+ 分类 + 数据风险。用途：
// - 遥测按 category 聚合（见 services/telemetry.ts）；
// - 预览 mock 覆盖门禁（scripts/verify-command-catalog.mjs）要求每条命令
//   在 `simulateCommand` 里有显式分支，`default:` 不再能静默吞掉新命令；
// - 将来的命令面板 / 快捷键表直接读这张表，不再各写一份。
//
// 约束：纯数据、零 import（node 验证脚本直接文本解析；运行时 import 也安全）。
// Rust 侧增删命令后：`verify-tauri-commands.mjs --write` 重生联合类型，
// 这里补一行，mock 补一个 case —— 三处对齐由门禁守着。
//
// risk 说明（只看数据风险）：
// - read：不改变任何状态；
// - write：新建/修改状态（含 UI 与外部进程副作用），可逆或新增对象；
// - destructive：可能丢数据（删文件、重写历史、 checkout 覆盖工作区文件）。

import type { TauriCommand } from './tauriCommands';

export type CommandCategory =
  | 'git'
  | 'files'
  | 'search'
  | 'terminal'
  | 'ghostty'
  | 'window'
  | 'system'
  | 'watch';

export type CommandRisk = 'read' | 'write' | 'destructive';

export interface CommandMeta {
  title: string;
  titleZh: string;
  category: CommandCategory;
  risk: CommandRisk;
}

export const COMMAND_CATALOG: Record<TauriCommand, CommandMeta> = {
  changelist_assign: { title: 'Assign changelist', titleZh: '分配变更列表', category: 'git', risk: 'write' },
  changelist_create: { title: 'Create changelist', titleZh: '新建变更列表', category: 'git', risk: 'write' },
  changelist_delete: { title: 'Delete changelist', titleZh: '删除变更列表', category: 'git', risk: 'write' },
  changelists_load: { title: 'Load changelists', titleZh: '加载变更列表', category: 'git', risk: 'read' },
  clipboard_file_paths: { title: 'Clipboard file paths', titleZh: '剪贴板文件路径', category: 'system', risk: 'read' },
  close_window: { title: 'Close window', titleZh: '关闭窗口', category: 'window', risk: 'write' },
  cancel_app_exit: { title: 'Cancel app exit', titleZh: '取消退出', category: 'window', risk: 'write' },
  create_new_window: { title: 'New window', titleZh: '新窗口', category: 'window', risk: 'write' },
  finish_app_exit: { title: 'Finish app exit', titleZh: '完成退出', category: 'window', risk: 'write' },
  detect_ghostty: { title: 'Detect Ghostty', titleZh: '探测 Ghostty', category: 'ghostty', risk: 'read' },
  focus_window: { title: 'Focus window', titleZh: '聚焦窗口', category: 'window', risk: 'write' },
  fs_create_directory: { title: 'Create directory', titleZh: '新建目录', category: 'files', risk: 'write' },
  fs_create_file: { title: 'Create file', titleZh: '新建文件', category: 'files', risk: 'write' },
  fs_delete: { title: 'Delete files', titleZh: '删除文件', category: 'files', risk: 'destructive' },
  fs_diff_files: { title: 'Diff files', titleZh: '比较文件', category: 'files', risk: 'read' },
  fs_paste: { title: 'Paste files', titleZh: '粘贴文件', category: 'files', risk: 'write' },
  fs_path_exists: { title: 'Path exists', titleZh: '路径存在检查', category: 'files', risk: 'read' },
  file_probe: { title: 'Probe file', titleZh: '文件探针', category: 'files', risk: 'read' },
  fs_rename: { title: 'Rename entry', titleZh: '重命名', category: 'files', risk: 'write' },
  fs_reveal: { title: 'Reveal in Finder', titleZh: '在文件管理器中显示', category: 'files', risk: 'write' },
  fs_watch_dirs: { title: 'Watch directories', titleZh: '监听目录', category: 'watch', risk: 'read' },
  fs_watch_files: { title: 'Watch files', titleZh: '监听文件', category: 'watch', risk: 'read' },
  get_conflict_repo_path: { title: 'Get conflict repo path', titleZh: '获取冲突仓库路径', category: 'window', risk: 'read' },
  get_merge_tool_params: { title: 'Get merge tool params', titleZh: '获取合并工具参数', category: 'window', risk: 'read' },
  ghostty_list_themes: { title: 'List Ghostty themes', titleZh: '列出 Ghostty 主题', category: 'ghostty', risk: 'read' },
  ghostty_load_theme: { title: 'Load Ghostty theme', titleZh: '加载 Ghostty 主题', category: 'ghostty', risk: 'read' },
  ghostty_show_config: { title: 'Show Ghostty config', titleZh: '查看 Ghostty 配置', category: 'ghostty', risk: 'read' },
  git_add: { title: 'Git add', titleZh: 'Git 添加', category: 'git', risk: 'write' },
  git_apply_patch: { title: 'Apply hunk patch', titleZh: '应用块补丁', category: 'git', risk: 'destructive' },
  git_clone: { title: 'Clone repository', titleZh: '克隆仓库', category: 'git', risk: 'write' },
  git_credential_status: { title: 'Credential helper status', titleZh: '凭据助手状态', category: 'git', risk: 'read' },
  git_credential_set_helper: { title: 'Set credential helper', titleZh: '设置凭据助手', category: 'git', risk: 'write' },
  git_lfs_status: { title: 'Git LFS status', titleZh: 'Git LFS 状态', category: 'git', risk: 'read' },
  git_lfs_pull: { title: 'Git LFS pull', titleZh: 'Git LFS 拉取', category: 'git', risk: 'write' },
  git_lfs_track: { title: 'Git LFS track', titleZh: 'Git LFS 跟踪', category: 'git', risk: 'write' },
  git_submodule_list: { title: 'List submodules', titleZh: '列出子模块', category: 'git', risk: 'read' },
  git_submodule_add: { title: 'Add submodule', titleZh: '添加子模块', category: 'git', risk: 'write' },
  git_submodule_update: { title: 'Update submodules', titleZh: '更新子模块', category: 'git', risk: 'write' },
  git_submodule_sync: { title: 'Sync submodules', titleZh: '同步子模块', category: 'git', risk: 'write' },
  git_worktree_list: { title: 'List worktrees', titleZh: '列出工作树', category: 'git', risk: 'read' },
  git_worktree_add: { title: 'Add worktree', titleZh: '添加工作树', category: 'git', risk: 'write' },
  git_worktree_remove: { title: 'Remove worktree', titleZh: '删除工作树', category: 'git', risk: 'destructive' },
  git_amend_message: { title: 'Amend commit message', titleZh: '修改提交信息', category: 'git', risk: 'write' },
  git_apply_shelf: { title: 'Apply shelf', titleZh: '应用搁置', category: 'git', risk: 'write' },
  git_apply_shelf_partial: { title: 'Apply shelf partially', titleZh: '部分应用搁置', category: 'git', risk: 'write' },
  git_blame: { title: 'Git blame', titleZh: 'Git 追溯', category: 'git', risk: 'read' },
  git_branches: { title: 'List branches', titleZh: '列出分支', category: 'git', risk: 'read' },
  git_checkout: { title: 'Checkout branch', titleZh: '切换分支', category: 'git', risk: 'write' },
  git_checkout_file_revision: { title: 'Checkout file revision', titleZh: '检出文件历史版本', category: 'git', risk: 'destructive' },
  git_cherry_pick: { title: 'Cherry-pick', titleZh: '拣选提交', category: 'git', risk: 'write' },
  git_cherry_pick_abort: { title: 'Abort cherry-pick', titleZh: '放弃拣选', category: 'git', risk: 'destructive' },
  git_cherry_pick_continue: { title: 'Continue cherry-pick', titleZh: '继续拣选', category: 'git', risk: 'write' },
  git_commit: { title: 'Commit', titleZh: '提交', category: 'git', risk: 'write' },
  git_commit_files: { title: 'Commit files', titleZh: '提交的文件', category: 'git', risk: 'read' },
  git_commits_diff: { title: 'Selected commits diff', titleZh: '所选提交差异', category: 'git', risk: 'read' },
  git_commits_files: { title: 'Selected commits files', titleZh: '所选提交的文件', category: 'git', risk: 'read' },
  git_commit_patch: { title: 'Commit patch', titleZh: '提交补丁', category: 'git', risk: 'read' },
  git_compare_branches: { title: 'Compare branches', titleZh: '比较分支', category: 'git', risk: 'read' },
  git_compare_commits: { title: 'Compare commits', titleZh: '对比提交', category: 'git', risk: 'read' },
  git_compare_refs: { title: 'Compare refs', titleZh: '比较引用', category: 'git', risk: 'read' },
  git_create_branch: { title: 'Create branch', titleZh: '新建分支', category: 'git', risk: 'write' },
  git_create_tag: { title: 'Create tag', titleZh: '创建标签', category: 'git', risk: 'write' },
  git_list_tags: { title: 'List tags', titleZh: '列出标签', category: 'git', risk: 'read' },
  git_delete_tag: { title: 'Delete tag', titleZh: '删除标签', category: 'git', risk: 'destructive' },
  git_delete_branch: { title: 'Delete branch', titleZh: '删除分支', category: 'git', risk: 'destructive' },
  git_delete_shelf: { title: 'Delete shelf', titleZh: '删除搁置', category: 'git', risk: 'destructive' },
  git_delete_shelf_file: { title: 'Delete shelf file', titleZh: '删除搁置中的文件', category: 'git', risk: 'destructive' },
  git_delete_shelf_files: { title: 'Delete shelf files', titleZh: '批量删除搁置文件', category: 'git', risk: 'destructive' },
  git_diff: { title: 'Git diff', titleZh: 'Git 差异', category: 'git', risk: 'read' },
  git_diff_commits: { title: 'Diff commits', titleZh: '提交差异', category: 'git', risk: 'read' },
  git_diff_patch: { title: 'Diff patch', titleZh: '补丁差异', category: 'git', risk: 'read' },
  git_diff_patch_paths: { title: 'Diff patch paths', titleZh: '补丁路径列表', category: 'git', risk: 'read' },
  git_diff_ref_worktree: { title: 'Diff ref to worktree', titleZh: '引用与工作区差异', category: 'git', risk: 'read' },
  git_discard_file_changes: { title: 'Discard file changes', titleZh: '丢弃文件改动', category: 'git', risk: 'destructive' },
  git_drop_commit: { title: 'Drop commit', titleZh: '丢弃提交', category: 'git', risk: 'destructive' },
  git_drop_commits: { title: 'Drop commits', titleZh: '批量丢弃提交', category: 'git', risk: 'destructive' },
  git_squash_commits: { title: 'Squash commits', titleZh: '批量压缩提交', category: 'git', risk: 'write' },
  git_fetch: { title: 'Git fetch', titleZh: 'Git 拉取', category: 'git', risk: 'write' },
  git_file_at_rev: { title: 'File at revision', titleZh: '历史版本文件内容', category: 'git', risk: 'read' },
  git_file_change_hunks: { title: 'File change hunks', titleZh: '文件改动块', category: 'git', risk: 'read' },
  git_file_history: { title: 'File history', titleZh: '文件历史', category: 'git', risk: 'read' },
  git_init: { title: 'Init repository', titleZh: '初始化仓库', category: 'git', risk: 'write' },
  git_list_shelves: { title: 'List shelves', titleZh: '列出搁置', category: 'git', risk: 'read' },
  git_list_deleted_shelves: { title: 'List deleted shelves', titleZh: '列出已删除搁置', category: 'git', risk: 'read' },
  git_log: { title: 'Git log', titleZh: '提交记录', category: 'git', risk: 'read' },
  git_merge: { title: 'Merge', titleZh: '合并', category: 'git', risk: 'write' },
  git_merge_abort: { title: 'Abort merge', titleZh: '取消合并', category: 'git', risk: 'destructive' },
  git_merge_continue: { title: 'Continue merge', titleZh: '继续合并', category: 'git', risk: 'write' },
  git_merge_file_content: { title: 'Merge file content', titleZh: '合并文件内容', category: 'git', risk: 'read' },
  git_conflict_kinds: { title: 'Conflict kinds', titleZh: '冲突种类', category: 'git', risk: 'read' },
  git_merge_file_marked: { title: 'Merge file marked text', titleZh: '三路合并文本', category: 'git', risk: 'read' },
  git_pending_commits: { title: 'Pending commits', titleZh: '待推送提交', category: 'git', risk: 'read' },
  git_pull: { title: 'Git pull', titleZh: 'Git 拉取合并', category: 'git', risk: 'write' },
  git_push: { title: 'Git push', titleZh: 'Git 推送', category: 'git', risk: 'write' },
  git_rebase: { title: 'Rebase', titleZh: '变基', category: 'git', risk: 'destructive' },
  git_rebase_abort: { title: 'Abort rebase', titleZh: '取消变基', category: 'git', risk: 'destructive' },
  git_rebase_continue: { title: 'Continue rebase', titleZh: '继续变基', category: 'git', risk: 'write' },
  git_rebase_skip: { title: 'Skip rebase commit', titleZh: '跳过变基提交', category: 'git', risk: 'destructive' },
  git_rebase_interactive: { title: 'Interactive rebase', titleZh: '交互式变基', category: 'git', risk: 'destructive' },
  git_remote_list: { title: 'List remotes', titleZh: '列出远端', category: 'git', risk: 'read' },
  git_remote_add: { title: 'Add remote', titleZh: '添加远端', category: 'git', risk: 'write' },
  git_remote_remove: { title: 'Remove remote', titleZh: '删除远端', category: 'git', risk: 'destructive' },
  git_remote_rename: { title: 'Rename remote', titleZh: '重命名远端', category: 'git', risk: 'write' },
  git_remote_set_url: { title: 'Set remote URL', titleZh: '修改远端地址', category: 'git', risk: 'write' },
  git_rename_branch: { title: 'Rename branch', titleZh: '重命名分支', category: 'git', risk: 'write' },
  git_repo_info: { title: 'Repository info', titleZh: '仓库信息', category: 'git', risk: 'read' },
  git_reset: { title: 'Reset', titleZh: '重置', category: 'git', risk: 'destructive' },
  git_revert_commit: { title: 'Revert commit', titleZh: '回滚提交', category: 'git', risk: 'write' },
  git_shelve_save: { title: 'Shelve changes', titleZh: '搁置改动', category: 'git', risk: 'write' },
  git_import_shelf_patch: { title: 'Import shelf patch', titleZh: '导入补丁到搁置', category: 'git', risk: 'write' },
  git_read_shelf_patch: { title: 'Read shelf patch', titleZh: '读取搁置补丁', category: 'git', risk: 'read' },
  git_rename_shelf: { title: 'Rename shelf', titleZh: '重命名搁置', category: 'git', risk: 'write' },
  git_restore_shelf: { title: 'Restore shelf', titleZh: '恢复已删除搁置', category: 'git', risk: 'write' },
  git_shelf_diff_preview: { title: 'Shelf diff preview', titleZh: '搁置差异预览', category: 'git', risk: 'read' },
  git_sequencer_state: { title: 'Sequencer state', titleZh: '合并/拣选状态', category: 'git', risk: 'read' },
  git_show: { title: 'Show commit', titleZh: '查看提交', category: 'git', risk: 'read' },
  git_stage_file: { title: 'Stage file', titleZh: '暂存文件', category: 'git', risk: 'write' },
  git_stash_apply: { title: 'Apply stash', titleZh: '应用贮藏', category: 'git', risk: 'write' },
  git_stash_drop: { title: 'Drop stash', titleZh: '丢弃贮藏', category: 'git', risk: 'destructive' },
  git_stash_files: { title: 'Stash files', titleZh: '贮藏的文件', category: 'git', risk: 'read' },
  git_stash_list: { title: 'List stashes', titleZh: '列出贮藏', category: 'git', risk: 'read' },
  git_stash_push: { title: 'Push stash', titleZh: '贮藏改动', category: 'git', risk: 'write' },
  git_status: { title: 'Git status', titleZh: 'Git 状态', category: 'git', risk: 'read' },
  git_use_merge_side: { title: 'Resolve conflict by side', titleZh: '按一方解决冲突', category: 'git', risk: 'destructive' },
  global_search: { title: 'Search contents', titleZh: '全文搜索', category: 'search', risk: 'read' },
  replace_in_files: { title: 'Replace in files', titleZh: '在文件中替换', category: 'search', risk: 'destructive' },
  restart_app: { title: 'Restart app', titleZh: '重启应用', category: 'system', risk: 'write' },
  is_conflict_window_open: { title: 'Is conflict window open', titleZh: '冲突窗口开启状态', category: 'window', risk: 'read' },
  launch_ghostty: { title: 'Launch Ghostty', titleZh: '启动 Ghostty', category: 'ghostty', risk: 'write' },
  list_extensions: { title: 'List extensions', titleZh: '列出扩展', category: 'system', risk: 'read' },
  list_other_windows: { title: 'List windows', titleZh: '列出窗口', category: 'window', risk: 'read' },
  lsp_server_start: { title: 'Start language server', titleZh: '启动语言服务器', category: 'system', risk: 'write' },
  lsp_server_send: { title: 'Send LSP message', titleZh: '发送 LSP 消息', category: 'system', risk: 'write' },
  lsp_server_status: { title: 'Language server status', titleZh: '语言服务器状态', category: 'system', risk: 'read' },
  lsp_server_stop: { title: 'Stop language server', titleZh: '停止语言服务器', category: 'system', risk: 'write' },
  open_conflict_window: { title: 'Open conflict window', titleZh: '打开冲突解决窗口', category: 'window', risk: 'write' },
  open_merge_tool_window: { title: 'Open merge tool window', titleZh: '打开三路合并窗口', category: 'window', risk: 'write' },
  open_external_url: { title: 'Open external URL', titleZh: '打开外部链接', category: 'system', risk: 'write' },
  pty_close: { title: 'Close terminal', titleZh: '关闭终端', category: 'terminal', risk: 'write' },
  pty_close_by_cwd: { title: 'Close terminals by directory', titleZh: '按目录关闭终端', category: 'terminal', risk: 'write' },
  pty_resize: { title: 'Resize terminal', titleZh: '调整终端尺寸', category: 'terminal', risk: 'write' },
  pty_session_busy: { title: 'Terminal busy probe', titleZh: '终端占用探测', category: 'terminal', risk: 'read' },
  pty_spawn: { title: 'Spawn terminal', titleZh: '启动终端', category: 'terminal', risk: 'write' },
  pty_write: { title: 'Write to terminal', titleZh: '向终端写入', category: 'terminal', risk: 'write' },
  pick_path: { title: 'Pick file or directory', titleZh: '选择文件或目录', category: 'files', risk: 'read' },
  read_directory: { title: 'Read directory', titleZh: '读取目录', category: 'files', risk: 'read' },
  read_file_content: { title: 'Read file', titleZh: '读取文件', category: 'files', risk: 'read' },
  read_file_text: { title: 'Read text with encoding', titleZh: '读取文本与编码', category: 'files', risk: 'read' },
  read_file_range: { title: 'Read file range', titleZh: '分块读文件', category: 'files', risk: 'read' },
  read_image_bytes: { title: 'Read image bytes', titleZh: '读取图片', category: 'files', risk: 'read' },
  register_workspace: { title: 'Register workspace', titleZh: '登记工作区', category: 'system', risk: 'write' },
  search_files: { title: 'Search files', titleZh: '搜索文件', category: 'search', risk: 'read' },
  set_app_menu_language: { title: 'Set menu language', titleZh: '设置菜单语言', category: 'system', risk: 'write' },
  set_window_title: { title: 'Set window title', titleZh: '设置窗口标题', category: 'window', risk: 'write' },
  unregister_workspace: { title: 'Unregister workspace', titleZh: '注销工作区', category: 'system', risk: 'write' },
  vt_clear_selection: { title: 'Clear terminal selection', titleZh: '清除终端选区', category: 'terminal', risk: 'write' },
  vt_encode_key: { title: 'Encode terminal key', titleZh: '编码终端按键', category: 'terminal', risk: 'read' },
  vt_engine_info: { title: 'Terminal engine info', titleZh: '终端引擎信息', category: 'terminal', risk: 'read' },
  vt_image_data: { title: 'Terminal image data', titleZh: '终端图片数据', category: 'terminal', risk: 'read' },
  vt_images: { title: 'Terminal images', titleZh: '终端图片列表', category: 'terminal', risk: 'read' },
  vt_scroll: { title: 'Scroll terminal', titleZh: '滚动终端', category: 'terminal', risk: 'write' },
  vt_search: { title: 'Search terminal', titleZh: '搜索终端内容', category: 'terminal', risk: 'read' },
  vt_select_line: { title: 'Select terminal line', titleZh: '选中终端行', category: 'terminal', risk: 'write' },
  vt_select_word: { title: 'Select terminal word', titleZh: '选中终端单词', category: 'terminal', risk: 'write' },
  vt_selected_text: { title: 'Terminal selected text', titleZh: '终端选中文本', category: 'terminal', risk: 'read' },
  vt_set_selection: { title: 'Set terminal selection', titleZh: '设置终端选区', category: 'terminal', risk: 'write' },
  vt_snapshot: { title: 'Terminal snapshot', titleZh: '终端快照', category: 'terminal', risk: 'read' },
  vt_snapshot_diff: { title: 'Terminal snapshot diff', titleZh: '终端快照差异', category: 'terminal', risk: 'read' },
  vt_wheel: { title: 'Terminal wheel', titleZh: '终端滚轮', category: 'terminal', risk: 'write' },
  vt_mouse: { title: 'Terminal mouse event', titleZh: '终端鼠标事件', category: 'terminal', risk: 'write' },
  vt_mouse_tracking: { title: 'Terminal mouse tracking', titleZh: '终端鼠标跟踪状态', category: 'terminal', risk: 'read' },
  vt_set_colors: { title: 'Set terminal colors', titleZh: '设置终端颜色', category: 'terminal', risk: 'write' },
  write_file_content: { title: 'Write file', titleZh: '写入文件', category: 'files', risk: 'write' },
  write_file_text: { title: 'Write text with encoding', titleZh: '按编码写入文本', category: 'files', risk: 'write' },
};

/** 按分类取命令（遥测聚合、面板分组用）。 */
export function commandsByCategory(category: CommandCategory): TauriCommand[] {
  return (Object.keys(COMMAND_CATALOG) as TauriCommand[]).filter((c) => COMMAND_CATALOG[c].category === category);
}

/** 破坏性命令清单（审计 / 二次确认用）。 */
export function destructiveCommands(): TauriCommand[] {
  return (Object.keys(COMMAND_CATALOG) as TauriCommand[]).filter((c) => COMMAND_CATALOG[c].risk === 'destructive');
}
