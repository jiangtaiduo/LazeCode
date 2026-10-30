// 原生应用菜单事件订阅。
//
// Rust 侧 `menu.rs` 构造应用菜单（File / View / Navigate / Git / Help），
// 点击后 emit `app-menu://<id>`；这里把每个 id 映射到前端已有的动作。
// 业务逻辑全部复用 App.tsx 里既有的 callback，不在此重复实现。

import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';
import { isTauriEnvironment } from './tauriBridge';

/** 与 `src-tauri/src/menu.rs` 的 `ids` 模块严格对应。 */
export const MENU = {
  NEW_WINDOW: 'app-menu://new_window',
  OPEN_PROJECT: 'app-menu://open_project',
  CLOSE_PROJECT: 'app-menu://close_project',
  SAVE: 'app-menu://save',
  SAVE_ALL: 'app-menu://save_all',
  CLOSE_TAB: 'app-menu://close_tab',
  CLOSE_ALL_TABS: 'app-menu://close_all_tabs',
  TOGGLE_PROJECT_DRAWER: 'app-menu://toggle_project_drawer',
  TOGGLE_COMMIT_DRAWER: 'app-menu://toggle_commit_drawer',
  TOGGLE_GIT_PANEL: 'app-menu://toggle_git_panel',
  TOGGLE_TERMINAL: 'app-menu://toggle_terminal',
  TOGGLE_SETTINGS: 'app-menu://toggle_settings',
  SEARCH_EVERYWHERE: 'app-menu://search_everywhere',
  GO_TO_LINE: 'app-menu://go_to_line',
  COMMIT: 'app-menu://commit',
  PUSH: 'app-menu://push',
  UPDATE_PROJECT: 'app-menu://update_project',
  FETCH: 'app-menu://fetch',
  MANAGE_REMOTES: 'app-menu://manage_remotes',
  SHOW_HISTORY: 'app-menu://show_history',
  STASH: 'app-menu://stash',
  UNSTASH: 'app-menu://unstash',
  SHELVE: 'app-menu://shelve',
  RESOLVE_CONFLICTS: 'app-menu://resolve_conflicts',
  REFRESH: 'app-menu://refresh',
  CHECK_FOR_UPDATES: 'app-menu://check_for_updates',
} as const;

/** 菜单事件名 → 处理函数。 */
export interface MenuHandlers {
  [event: string]: () => void;
}

/**
 * 订阅原生应用菜单事件。
 *
 * 浏览器预览（npm run dev）没有 Tauri 运行时，直接跳过 —— 否则
 * `@tauri-apps/api` 的 listen 会抛错。
 */
export function useAppMenu(handlers: MenuHandlers): void {
  // 每次渲染同步最新 handler：监听器只注册一次（键集合在挂载时固定），
  // 但事件到达时转发到"当前"回调。否则 effect 捕获的是首帧闭包，
  // 而首帧的 workspacePath/activeFilePath 等 state 还是空值 → 菜单项永久无响应。
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;

  useEffect(() => {
    if (!isTauriEnvironment()) return;
    let unlisteners: Array<() => void> = [];
    let disposed = false;
    const events = Object.keys(handlersRef.current);

    void (async () => {
      for (const event of events) {
        const unlisten = await listen(event, () => handlersRef.current[event]?.());
        if (disposed) {
          unlisten();
          continue;
        }
        unlisteners.push(unlisten);
      }
    })();

    return () => {
      disposed = true;
      unlisteners.forEach(fn => fn());
      unlisteners = [];
    };
  }, []);
}
