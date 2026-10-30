// Tauri 2.0 Native IPC Bridge with Browser Fallbacks

import { invoke } from '@tauri-apps/api/core';
import { describeResult, formatCommand, logIpc } from './ipcLog';
import { t } from './i18n';
import type { TauriCommand } from './tauriCommands';
import { COMMAND_CATALOG } from './commandCatalog';
import { trackCommand } from './telemetry';

// Tauri internal marker is only present at runtime in desktop webview
export const isTauriEnvironment = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 调用原生命令。
 *
 * 桌面端（Tauri 运行时）里的失败一律向上抛：原生命令报错必须让调用方看到，
 * 否则界面会静默显示空数据或示意数据，把真实故障伪装成“正常工作”。
 * 浏览器预览（npm run dev）没有原生后端，才退回 simulateCommand 的示意数据。
 */
export async function invokeCommand<T>(cmd: TauriCommand, args?: Record<string, unknown>): Promise<T> {
  const label = formatCommand(cmd, args);

  if (isTauriEnvironment()) {
    const started = performance.now();
    try {
      const result = await invoke<T>(cmd, args);
      // 全命令埋点（遥测）：分类来自命令目录，高频轮询由 trackCommand 跳过。
      // 与下面的 git/write 抽样日志是两回事 —— 那是给人看的 Console，
      // 这是给聚合看的耗时/成败（只存本机）。
      trackCommand(cmd, performance.now() - started, true, COMMAND_CATALOG[cmd]?.category ?? 'unknown');
      if (cmd.startsWith('git_') || cmd === 'write_file_content') {
        const elapsed = Math.round(performance.now() - started);
        logIpc(`${label} → ${describeResult(result)}（${elapsed}ms）`);
      }
      return result;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      trackCommand(cmd, performance.now() - started, false, COMMAND_CATALOG[cmd]?.category ?? 'unknown');
      logIpc(`${label} 失败：${message}`, 'error');
      throw err instanceof Error ? err : new Error(message);
    }
  }

  logIpc(`${label} → 浏览器预览模式（示意数据）`);
  return simulateCommand<T>(cmd, args);
}

interface SimCommitFile {
  path: string;
  status: string;
  additions: number;
  deletions: number;
}

const SIM_COMMIT_FILES: Record<string, SimCommitFile[]> = {
  a1f0c2a8b7e4: [
    { path: 'service/api/v1/catalog.py', status: 'M', additions: 12, deletions: 3 },
    { path: 'service/services/catalog/catalog_pipeline.py', status: 'A', additions: 30, deletions: 0 },
    { path: 'web/src/App.tsx', status: 'M', additions: 4, deletions: 1 },
    { path: 'docs/spec.md', status: 'A', additions: 20, deletions: 0 }
  ],
  a2f0c2a8b7e4: [
    { path: 'service/services/catalog/catalog_service.py', status: 'A', additions: 40, deletions: 0 },
    { path: 'service/services/catalog/planner.py', status: 'M', additions: 8, deletions: 2 },
    { path: 'service/tests/test_catalog.py', status: 'M', additions: 18, deletions: 6 }
  ],
  b1f0c2a8b7e4: [
    { path: 'service/queue/dispatcher.py', status: 'M', additions: 5, deletions: 0 },
    { path: 'service/queue/worker_pipeline.py', status: 'A', additions: 25, deletions: 0 },
    { path: 'README.md', status: 'M', additions: 1, deletions: 0 }
  ]
};

/** 浏览器预览环境的示意数据：与真实文件系统无关，仅用于 UI 联调 */
async function simulateCommand<T>(cmd: TauriCommand, args?: Record<string, unknown>): Promise<T> {
  void args;
  switch (cmd) {
    case 'clipboard_file_paths':
      // 浏览器预览拿不到 OS 剪贴板的文件 flavor；返回 [] 让调用方按纯文本
      // 粘贴处理，而不是把 {} 当数组遍历。
      return [] as unknown as T;

    case 'git_status':
      return [
        { path: 'src/sample.txt', status: 'M', isStaged: false, isUnstaged: true, additions: 0, deletions: 1 },
        { path: 'service/tests/test_ref_pick.py', status: 'M', isStaged: true, isUnstaged: false, additions: 18, deletions: 6 },
        { path: 'service/models/entities.py', status: 'M', isStaged: false, isUnstaged: true, additions: 4, deletions: 1 },
        { path: 'service/worker/jobs/units.py', status: 'M', isStaged: false, isUnstaged: true, additions: 12, deletions: 0 },
        { path: 'docs/specs/spec.md', status: 'M', isStaged: false, isUnstaged: true, additions: 2, deletions: 0 },
        { path: '.env', status: 'M', isStaged: false, isUnstaged: true, additions: 1, deletions: 0 }
      ] as unknown as T;

    case 'git_submodule_list':
    case 'git_worktree_list':
      return [] as unknown as T;

    case 'git_lfs_status':
      return { installed: false, version: '', files: [], error: 'git-lfs not installed' } as unknown as T;

    case 'git_credential_status':
      return { helper: '', osxKeychainAvailable: false } as unknown as T;

    case 'git_list_tags':
      return [
        { name: 'v1.0.0', shortHash: 'e7b21a8', date: '2026-01-01', message: 'First release' }
      ] as unknown as T;

    case 'git_branches':
      return [
        { name: 'feat-designer-mode', isCurrent: true, isRemote: false, tracking: 'origin/feat-designer-mode', ahead: 2, behind: 0 },
        { name: 'main', isCurrent: false, isRemote: false, isFavorite: true },
        { name: 'dev', isCurrent: false, isRemote: false },
        { name: 'dev-contributor', isCurrent: false, isRemote: false },
        { name: 'origin/feat-designer-mode', isCurrent: false, isRemote: true },
        { name: 'origin/main', isCurrent: false, isRemote: true }
      ] as unknown as T;

    case 'git_log':
      // 与 Rust assign_graph_lanes 同构的示意轨道数据（含一次分叉合并）
      return [
        {
          hash: 'e7b21a8f9c0',
          shortHash: 'e7b21a8',
          message: t().preview.sampleCommit(10),
          author: 'contributor',
          authorEmail: 'dev@example.com',
          date: String(Math.floor(Date.now() / 1000) - 16 * 60),
          branchTags: [
            { name: 'feat-designer-mode', type: 'local' },
            { name: 'origin/feat-designer-mode', type: 'remote' }
          ],
          isMerge: true,
          graphTrack: 0,
          graphIns: [],
          graphStraight: [],
          graphOuts: [{ from: 0, to: 0 }, { from: 0, to: 1 }],
          parents: ['c41e98d2a1b', '98d120a4b3c']
        },
        {
          hash: '98d120a4b3c',
          shortHash: '98d120a',
          message: t().preview.sampleCommit(8),
          author: 'contributor',
          authorEmail: 'dev@example.com',
          date: String(Math.floor(Date.now() / 1000) - 20 * 3600),
          graphTrack: 1,
          graphIns: [],
          graphStraight: [0],
          graphOuts: [{ from: 1, to: 1 }]
        },
        {
          hash: 'c41e98d2a1b',
          shortHash: 'c41e98d',
          message: t().preview.sampleCommit(9),
          author: 'contributor',
          authorEmail: 'dev@example.com',
          date: String(Math.floor(Date.now() / 1000) - 3600),
          graphTrack: 0,
          graphIns: [],
          graphStraight: [1],
          graphOuts: [{ from: 0, to: 0 }]
        },
        {
          hash: '1127fd03e91',
          shortHash: '1127fd03',
          message: 'chore: add CI and LICENSE',
          author: 'contributor',
          authorEmail: 'dev@example.com',
          date: String(Math.floor(Date.now() / 1000) - 10 * 60),
          branchTags: [{ name: 'origin/dev-multi-node', type: 'remote' }],
          graphTrack: 1,
          graphIns: [0],
          graphStraight: [],
          graphOuts: [{ from: 1, to: 1 }]
        },
        {
          hash: '38ab91cf8e4',
          shortHash: '38ab91c',
          message: t().preview.sampleCommit(3),
          author: 'contributor',
          authorEmail: 'dev@example.com',
          date: String(Math.floor(Date.now() / 1000) - 23 * 86400),
          branchTags: [
            { name: 'main', type: 'main' },
            { name: 'v0.1.0', type: 'tag' }
          ],
          graphTrack: 1
        }
      ] as unknown as T;

    case 'git_repo_info':
      return { isRepo: true, root: '.', branch: 'main' } as unknown as T;

    case 'git_init':
      return t().preview.notExecutedInit as unknown as T;

    case 'git_pending_commits':
      return [] as unknown as T;

    case 'git_blame':
    case 'git_file_history':
    case 'git_stash_list':
      return [] as unknown as T;

    case 'git_stash_files':
      return [] as unknown as T;

    case 'git_stash_push':
    case 'git_stash_apply':
    case 'git_stash_drop':
      return t().preview.notExecutedStash as unknown as T;

    case 'git_diff_ref_worktree':
      // 注意：返回的是对象而非数组，必须给出完整形状，否则调用方访问 .lines 会抛错
      return {
        path: '',
        oldPath: null,
        isNew: false,
        isDeleted: false,
        isBinary: false,
        additions: 0,
        deletions: 0,
        lines: []
      } as unknown as T;

    case 'git_remote_list':
      return [{ name: 'origin', url: 'https://example.com/demo.git' }] as unknown as T;

    case 'git_push':
      return t().preview.notExecutedPush as unknown as T;

    case 'git_pull':
      return t().preview.notExecutedPull as unknown as T;

    case 'git_fetch':
      return t().preview.notExecutedFetch as unknown as T;

    // 浏览器预览：无法访问真实文件系统，返回示意结构，仅供 UI 联调。
    // 桌面端由 Rust 侧真实读取。
    case 'read_directory':
      return {
        name: 'lazecode',
        path: '.',
        isDirectory: true,
        gitStatus: undefined,
        children: [
          {
            name: 'src',
            path: 'src',
            isDirectory: true,
            gitStatus: undefined,
            children: [
              {
                name: 'components',
                path: 'src/components',
                isDirectory: true,
                gitStatus: undefined,
                children: [
                  { name: 'GhosttyTerminal.tsx', path: 'src/components/GhosttyTerminal.tsx', isDirectory: false, gitStatus: 'M', children: null },
                  { name: 'SettingsDialog.tsx', path: 'src/components/SettingsDialog.tsx', isDirectory: false, gitStatus: 'A', children: null },
                  { name: 'EditorView.tsx', path: 'src/components/EditorView.tsx', isDirectory: false, gitStatus: undefined, children: null }
                ]
              },
              { name: 'App.tsx', path: 'src/App.tsx', isDirectory: false, gitStatus: 'M', children: null },
                  { name: 'sample.txt', path: 'src/sample.txt', isDirectory: false, gitStatus: 'M', children: null },
              { name: 'main.tsx', path: 'src/main.tsx', isDirectory: false, gitStatus: undefined, children: null }
            ]
          },
          {
            name: 'src-tauri',
            path: 'src-tauri',
            isDirectory: true,
            gitStatus: undefined,
            children: [
              { name: 'Cargo.toml', path: 'src-tauri/Cargo.toml', isDirectory: false, gitStatus: undefined, children: null },
              { name: 'tauri.conf.json', path: 'src-tauri/tauri.conf.json', isDirectory: false, gitStatus: undefined, children: null }
            ]
          },
          { name: 'package.json', path: 'package.json', isDirectory: false, gitStatus: undefined, children: null }
        ]
      } as unknown as T;

    case 'pick_path':
      return null as unknown as T;

    case 'read_file_content':
      return t().preview.sampleFile as unknown as T;

    case 'read_file_text':
      return { text: t().preview.sampleFile, encoding: 'UTF-8' } as unknown as T;

    case 'read_image_bytes':
      // 浏览器预览无文件系统：抛错让调用方走失败占位，而不是返回畸形 data URL
      throw new Error(t().preview.imageUnavailable);

    case 'git_commit':
      return t().preview.notExecutedCommit as unknown as T;

    // 浏览器预览：以下命令无法访问真实仓库，返回空结果而非伪造数据。
    // 桌面端由 Rust 侧真实实现。
    case 'global_search':
      return [] as unknown as T;

    case 'git_commit_files':
      return (SIM_COMMIT_FILES[String(args?.commit ?? '')] ?? []) as unknown as T;

    case 'git_commits_files': {
      const commits = Array.isArray(args?.commits) ? (args!.commits as string[]) : [];
      const merged = new Map<string, SimCommitFile>();
      for (const commit of commits) {
        for (const file of SIM_COMMIT_FILES[commit] ?? []) {
          const prev = merged.get(file.path);
          if (!prev) {
            merged.set(file.path, { ...file });
          } else {
            prev.additions += file.additions;
            prev.deletions += file.deletions;
          }
        }
      }
      // 预览环境的真实日志哈希不在 fixture 里：给多选一个稳定的示意并集，
      // 让目录树/菜单可联调；原生环境永远走真实命令。
      if (merged.size === 0 && commits.length > 1) {
        return [
          { path: 'src/components/CommitDrawer.tsx', status: 'M', additions: 12, deletions: 3 },
          { path: 'src/components/FileIcons.tsx', status: 'M', additions: 4, deletions: 1 },
          { path: 'src/services/i18n/en.ts', status: 'M', additions: 8, deletions: 2 },
          { path: 'src/services/i18n/types.ts', status: 'A', additions: 16, deletions: 0 },
          { path: 'src/services/commandCatalog.ts', status: 'M', additions: 2, deletions: 1 },
          { path: 'src/services/panelLayout.ts', status: 'A', additions: 30, deletions: 0 },
          { path: 'src/types/index.ts', status: 'M', additions: 3, deletions: 1 }
        ] as unknown as T;
      }
      return [...merged.values()] as unknown as T;
    }

    case 'git_commits_diff':
      return {
        path: String(args?.path ?? `${Array.isArray(args?.commits) ? (args!.commits as string[]).length : 0} commits`),
        oldPath: null,
        isNew: false,
        isDeleted: false,
        isBinary: false,
        additions: 2,
        deletions: 0,
        lines: [
          { kind: 'hunk', oldLine: null, newLine: null, text: '@@ -1 +1,2 @@' },
          { kind: 'context', oldLine: 1, newLine: 1, text: 'one' },
          { kind: 'add', oldLine: null, newLine: 2, text: 'two' }
        ]
      } as unknown as T;

    case 'replace_in_files':
      return { files: 0, occurrences: 0, errors: [], changedPaths: [] } as unknown as T;

    // 注意：这两个返回的是对象而非数组，必须给出完整形状，
    // 否则调用方访问 .lines 会抛错。
    case 'git_diff':
    case 'git_show':
    case 'git_diff_commits':
      // 浏览器预览：复刻「仅行尾换行符变化」的真实 diff 形态
      // （content 相同的 -/+ 对 + EOF 无换行标记），供 UI 联调合并渲染
      return {
        path: 'sample.txt',
        oldPath: null,
        isNew: false,
        isDeleted: false,
        isBinary: false,
        additions: 0,
        deletions: 1,
        lines: [
          { kind: 'hunk', oldLine: null, newLine: null, text: '@@ -46,4 +46,4 @@' },
          { kind: 'context', oldLine: 46, newLine: 46, text: '      "debugApplicationIdSuffix": ".debug"' },
          { kind: 'context', oldLine: 47, newLine: 47, text: '    }' },
          { kind: 'context', oldLine: 48, newLine: 48, text: '  }' },
          { kind: 'context', oldLine: 49, newLine: 49, text: '}' },
          { kind: 'del', oldLine: 50, newLine: null, text: '', eofNoNewline: true }
        ]
      } as unknown as T;

    // Diff 页取两侧原文的命令：浏览器预览给空串（与「该侧不存在」同义）
    case 'git_file_at_rev':
      return '' as unknown as T;

    case 'detect_ghostty':
      // 浏览器预览环境无法访问原生二进制，标记为不可用
      return {
        installed: false,
        version: '',
        binary_path: '',
        resources_dir: '',
        config_path: '',
        theme_count: 0,
        platform_supported: false
      } as unknown as T;

    case 'ghostty_list_themes':
      return [
        'Dracula', 'Adwaita Dark', 'Abernathy', 'Aardvark Blue', '3024 Night',
        'Adventure', 'Afterglow', 'Alien Blood', 'Argonaut', 'Ayu Mirage'
      ] as unknown as T;

    case 'ghostty_load_theme':
    case 'ghostty_show_config':
      return {
        background: '#282c34',
        foreground: '#ffffff',
        cursor_color: '#ffffff',
        cursor_text: '#282c34',
        selection_background: '#44475a',
        selection_foreground: '#ffffff',
        font_family: '',
        font_size: 13,
        theme_name: 'Ghostty Default (One Dark)',
        palette: [
          '#1d1f21', '#cc6666', '#b5bd68', '#f0c674', '#81a2be', '#b294bb', '#8abeb7', '#c5c8c6',
          '#666666', '#d54e53', '#b9ca4a', '#e7c547', '#7aa6da', '#c397d8', '#70c0b1', '#eaeaea'
        ],
        source: 'bundled'
      } as unknown as T;

    case 'launch_ghostty':
      throw new Error(t().preview.ghosttyUnavailable);

    case 'pty_spawn':
    case 'pty_write':
    case 'pty_resize':
    case 'pty_close':
    case 'pty_close_by_cwd':
    case 'vt_snapshot':
    case 'vt_encode_key':
    case 'vt_set_selection':
    case 'vt_clear_selection':
    case 'vt_selected_text':
      // 不能吞成 {}：spawnPty 会拿到 undefined sessionId 继续跑，
      // 最后以「Cannot read properties of undefined」这种无意义报错收场
      throw new Error(t().preview.terminalUnavailable);

    // 命令目录覆盖门禁（scripts/verify-command-catalog.mjs）：以下分支此前走
    // `default: {} as T` 静默返回。形状按 Rust 签名给保守值 —— void 用 undefined、
    // 列表用 []、字符串用 ''、结构已知给最小可用形状、结构未知用 {}（与旧 default
    // 一致，调用到再补精确形状）。预览里调到它们，行为与今天完全一致，只是"显式"了：
    // Rust 侧新增命令时，这里必须同步加分支，否则门禁红。
    case 'changelists_load':
    case 'changelist_assign':
    case 'changelist_create':
    case 'changelist_delete':
      // ChangelistsDto：与 App 关闭项目时的重置形状一致
      return { lists: ['Changes'], files: {} } as unknown as T;
    case 'git_shelf_diff_preview':
      return { oldPath: '', newPath: '', title: '' } as unknown as T;


    case 'search_files':
    case 'git_list_shelves':
    case 'git_list_deleted_shelves':
    case 'git_file_change_hunks':
    case 'list_other_windows':
    case 'lsp_server_status':
    case 'list_extensions':
      return [] as unknown as T;

    case 'git_add':
    case 'git_apply_patch':
    case 'git_clone':
    case 'git_credential_set_helper':
    case 'git_lfs_pull':
    case 'git_lfs_track':
    case 'git_submodule_add':
    case 'git_submodule_sync':
    case 'git_submodule_update':
    case 'git_worktree_add':
    case 'git_worktree_remove':
    case 'git_checkout':
    case 'git_create_branch':
    case 'git_rename_branch':
    case 'git_delete_branch':
    case 'git_remote_add':
    case 'git_remote_remove':
    case 'git_remote_rename':
    case 'git_remote_set_url':
    case 'git_create_tag':
    case 'git_delete_tag':
    case 'git_cherry_pick':
    case 'git_cherry_pick_abort':
    case 'git_cherry_pick_continue':
    case 'git_reset':
    case 'git_merge':
    case 'git_merge_abort':
    case 'git_merge_continue':
    case 'git_rebase':
    case 'git_rebase_abort':
    case 'git_rebase_continue':
    case 'git_rebase_skip':
    case 'git_rebase_interactive':
    case 'git_amend_message':
    case 'git_drop_commit':
    case 'git_drop_commits':
    case 'git_squash_commits':
    case 'git_commit_patch':
    case 'git_diff_patch':
    case 'git_diff_patch_paths':
    case 'git_checkout_file_revision':
    case 'git_revert_commit':
    case 'git_discard_file_changes':
    case 'git_shelve_save':
    case 'git_delete_shelf':
    case 'git_delete_shelf_file':
    case 'git_delete_shelf_files':
    case 'git_restore_shelf':
    case 'git_rename_shelf':
    case 'git_read_shelf_patch':
    case 'git_import_shelf_patch':
    case 'git_apply_shelf':
    case 'git_apply_shelf_partial':
    case 'git_stage_file':
    case 'git_use_merge_side':
    case 'fs_create_directory':
    case 'fs_create_file':
    case 'fs_paste':
    case 'fs_rename':
    case 'set_app_menu_language':
    case 'lsp_server_send':
    case 'lsp_server_stop':
      return '' as unknown as T;

    case 'fs_path_exists':
    case 'is_conflict_window_open':
    case 'vt_mouse':
    case 'vt_mouse_tracking':
    case 'vt_wheel':
      return false as unknown as T;

    case 'vt_select_word':
    case 'vt_select_line':
      return null as unknown as T;

    case 'pty_session_busy':
      return { busy: false, commands: [] } as unknown as T;

    case 'git_sequencer_state':
      return { merging: false, cherryPicking: false, rebasing: false, mergeHead: null, cherryPickHead: null, cherryPickAuthor: null, cherryPickSubject: null } as unknown as T;

    case 'vt_images':
      return { placements: [], generation: 0 } as unknown as T;

    case 'fs_diff_files':
      // FileDiffDto：与 git_diff 示意形状同口径（调用方读 .lines）
      return {
        path: '',
        oldPath: null,
        isNew: false,
        isDeleted: false,
        isBinary: false,
        additions: 0,
        deletions: 0,
        lines: []
      } as unknown as T;

    case 'close_window':
    case 'restart_app':
    case 'create_new_window':
    case 'finish_app_exit':
    case 'cancel_app_exit':
    case 'open_conflict_window':
    case 'open_merge_tool_window':
    case 'focus_window':
    case 'fs_delete':
    case 'fs_reveal':
    case 'open_external_url':
    case 'register_workspace':
    case 'unregister_workspace':
    case 'set_window_title':
    case 'fs_watch_dirs':
    case 'fs_watch_files':
    case 'write_file_content':
    case 'write_file_text':
    case 'vt_set_colors':
    case 'vt_scroll':
      return undefined as unknown as T;

    case 'get_conflict_repo_path':
    case 'get_merge_tool_params':
      return null as unknown as T;

    case 'git_compare_branches': {
      const nowSec = Math.floor(Date.now() / 1000);
      return {
        mergeBase: '31f0c2a8b7e4',
        leftTruncated: false,
        rightTruncated: false,
        leftCommits: [
          {
            hash: 'a1f0c2a8b7e4',
            shortHash: 'a1f0c2a',
            message: 'Refine pipeline',
            author: 'contributor',
            authorEmail: 'contributor@example.com',
            date: String(nowSec - 18 * 60),
            branchTags: [{ name: 'feat-designer-mode', type: 'local' }],
            graphTrack: 0,
            parents: ['a2f0c2a8b7e4']
          },
          {
            hash: 'a2f0c2a8b7e4',
            shortHash: 'a2f0c2a',
            message: 'Improve renderer',
            author: 'contributor',
            authorEmail: 'contributor@example.com',
            date: String(nowSec - 20 * 3600),
            branchTags: [],
            graphTrack: 0,
            parents: ['a3f0c2a8b7e4']
          },
          {
            hash: 'a3f0c2a8b7e4',
            shortHash: 'a3f0c2a',
            message: 'Add storyboard draft',
            author: 'dev',
            authorEmail: 'dev@example.com',
            date: String(nowSec - 2 * 86400),
            branchTags: [{ name: 'origin/feat-designer-mode', type: 'remote' }],
            graphTrack: 0,
            parents: []
          }
        ],
        rightCommits: [
          {
            hash: 'b1f0c2a8b7e4',
            shortHash: 'b1f0c2a',
            message: 'Optimize keyframe exports',
            author: 'dev',
            authorEmail: 'dev@example.com',
            date: String(nowSec - 3 * 3600),
            branchTags: [{ name: 'main', type: 'main' }],
            graphTrack: 0,
            parents: ['b2f0c2a8b7e4']
          },
          {
            hash: 'b2f0c2a8b7e4',
            shortHash: 'b2f0c2a',
            message: 'Improve workflow',
            author: 'contributor',
            authorEmail: 'contributor@example.com',
            date: String(nowSec - 7 * 3600),
            branchTags: [],
            graphTrack: 0,
            parents: ['b3f0c2a8b7e4']
          },
          {
            hash: 'b3f0c2a8b7e4',
            shortHash: 'b3f0c2a',
            message: 'fix: enforce dialogue budget before timing callback',
            author: 'dev',
            authorEmail: 'dev@example.com',
            date: String(nowSec - 9 * 86400),
            branchTags: [{ name: 'main', type: 'main' }],
            graphTrack: 0,
            parents: []
          }
        ],
        leftFiles: [
          { path: 'service/api/v1/catalog.py', status: 'M', additions: 12, deletions: 3 },
          { path: 'service/services/catalog/catalog_service.py', status: 'A', additions: 40, deletions: 0 },
          { path: 'service/services/catalog/planner.py', status: 'M', additions: 8, deletions: 2 },
          { path: 'service/services/catalog/adapter.py', status: 'M', additions: 5, deletions: 1 },
          { path: 'service/services/catalog/pipeline.py', status: 'M', additions: 9, deletions: 1 },
          { path: 'service/services/catalog/catalog_pipeline.py', status: 'A', additions: 30, deletions: 0 },
          { path: 'service/services/catalog/catalog_renderer.py', status: 'M', additions: 6, deletions: 4 },
          { path: 'service/tests/test_catalog.py', status: 'M', additions: 18, deletions: 6 },
          { path: 'web/src/App.tsx', status: 'M', additions: 4, deletions: 1 },
          { path: 'docs/spec.md', status: 'A', additions: 20, deletions: 0 }
        ],
        rightFiles: [
          { path: 'service/api/v1/admin.py', status: 'M', additions: 2, deletions: 2 },
          { path: 'service/queue/dispatcher.py', status: 'M', additions: 5, deletions: 0 },
          { path: 'service/queue/jobs/jobs.py', status: 'M', additions: 11, deletions: 4 },
          { path: 'service/queue/worker_pipeline.py', status: 'A', additions: 25, deletions: 0 },
          { path: 'service/services/renderer.py', status: 'M', additions: 7, deletions: 3 },
          { path: 'README.md', status: 'M', additions: 1, deletions: 0 }
        ]
      } as unknown as T;
    }

    case 'git_compare_commits':
      return [
        { path: 'service/api/v1/catalog.py', status: 'M', additions: 12, deletions: 3 },
        { path: 'service/services/catalog/catalog_pipeline.py', status: 'A', additions: 30, deletions: 0 },
        { path: 'service/queue/worker_pipeline.py', status: 'A', additions: 25, deletions: 0 },
        { path: 'service/services/renderer.py', status: 'M', additions: 7, deletions: 3 },
        { path: 'web/src/App.tsx', status: 'M', additions: 4, deletions: 1 },
        { path: 'docs/spec.md', status: 'A', additions: 20, deletions: 0 }
      ] as unknown as T;

    case 'git_compare_refs':
    case 'git_conflict_kinds':
    case 'git_merge_file_content':
    case 'vt_search':
    case 'vt_engine_info':
    case 'vt_image_data':
    case 'vt_snapshot_diff':
      // 结构未知：{}（与旧 default 一致），调用到再补精确形状
      return {} as unknown as T;

    case 'git_merge_file_marked':
      return { path: '', merged: '' } as unknown as T;

    case 'read_file_range':
      // FileRangeDto 最小形状（camelCase，见 file_range.rs）
      return { path: '', startLine: 1, lines: [], totalLines: 0, hasMore: false } as unknown as T;

    case 'file_probe':
      // FileProbeDto 最小形状（camelCase，见 commands.rs file_probe）：
      // 预览环境当"不存在"处理，调用方走已删除分支（安全方向）
      return { exists: false, isFile: false, len: 0, mtimeMs: null } as unknown as T;

    case 'lsp_server_start':
      // LspServerDto 最小形状（camelCase，见 lsp.rs）
      return {
        language: '',
        command: '',
        pid: 0,
        running: false,
        startedMs: 0,
        exitCode: null,
        rssKb: null,
      } as unknown as T;

    default:
      return {} as T;
  }
}
