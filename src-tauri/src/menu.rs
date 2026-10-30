//! 原生应用菜单（macOS 应用栏 / Windows 应用菜单）。
//!
//! 设计原则：**菜单只做派发，不含业务**。所有动作在前端已经实现
//! （PushDialog / PullDialog / StashDialog / 各抽屉开关 / 快捷键），
//! 因此每个菜单项点击后只是向当前窗口 emit 一个事件，由前端的
//! `useEffect(() => listen('app-menu://<id>', ...))` 调用既有 callback。
//! 在 Rust 里复制一份业务逻辑会立刻产生两份真相。
//!
//! 与自定义标题栏的关系：本项目 `titleBarStyle: Overlay` + 隐藏标题，
//! macOS 上应用菜单仍然显示在屏幕顶部（系统级），不与 Overlay 冲突。
//! Windows 上 `titleBarStyle` 未设，菜单挂在窗口内。
//!
//! # 多语言
//!
//! 菜单文案只有一份真相，在前端 `src/services/i18n/` 的字典里。Rust 不维护
//! 第二份翻译表 —— 那会立刻漂移。这里的做法是：**Rust 只保存结构**
//! （id、快捷键、层级），文案按启动/切换时的语言从一份常量表里取。
//!
//! 取表而非 IPC 拉取：菜单在 setup 里构造，前端还没起来；而切换语言时
//! 前端把当前语言传给 `set_app_menu_language`，Rust 整条重建。
//! 代价是新增语言要在 Rust 侧补一张表，收益是构造路径是同步的、
//! 不需要给菜单挂一个 AppHandle 去 await 前端字典。
//!
//! 两份文案的一致性由 `scripts/verify-menu-parity.mjs` 在 CI 里守着：
//! 只改一侧会直接让该脚本失败，不会带到运行时。

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Runtime};

/// 菜单项 id 与前端的约定。
///
/// 每个 id 对应一个前端事件名 `app-menu://<id>`。新增项时两边同步。
/// 用 `&str` 常量而不是 enum：前端要按字符串 listen，enum 只会多一层转换。
pub mod ids {
    // ---- File ----
    pub const NEW_WINDOW: &str = "new_window";
    pub const OPEN_PROJECT: &str = "open_project";
    pub const CLOSE_PROJECT: &str = "close_project";
    pub const SAVE: &str = "save";
    pub const SAVE_ALL: &str = "save_all";
    pub const CLOSE_TAB: &str = "close_tab";
    pub const CLOSE_ALL_TABS: &str = "close_all_tabs";
    // ---- View / Tool Windows ----
    pub const TOGGLE_PROJECT_DRAWER: &str = "toggle_project_drawer";
    pub const TOGGLE_COMMIT_DRAWER: &str = "toggle_commit_drawer";
    pub const TOGGLE_GIT_PANEL: &str = "toggle_git_panel";
    pub const TOGGLE_TERMINAL: &str = "toggle_terminal";
    pub const TOGGLE_SETTINGS: &str = "toggle_settings";
    // ---- Navigate ----
    pub const SEARCH_EVERYWHERE: &str = "search_everywhere";
    pub const GO_TO_LINE: &str = "go_to_line";
    // ---- Git ----
    pub const COMMIT: &str = "commit";
    pub const PUSH: &str = "push";
    pub const UPDATE_PROJECT: &str = "update_project";
    pub const FETCH: &str = "fetch";
    pub const MANAGE_REMOTES: &str = "manage_remotes";
    pub const SHOW_HISTORY: &str = "show_history";
    pub const STASH: &str = "stash";
    pub const UNSTASH: &str = "unstash";
    pub const SHELVE: &str = "shelve";
    pub const RESOLVE_CONFLICTS: &str = "resolve_conflicts";
    pub const REFRESH: &str = "refresh";
    pub const CHECK_FOR_UPDATES: &str = "check_for_updates";
}

/// 菜单事件通道前缀：完整事件名是 `app-menu://<id>`。
pub const EVENT_PREFIX: &str = "app-menu://";

/// 应用名。作为 macOS 首个 submenu（应用菜单）的标题。
///
/// 不进 i18n 字典：产品名不是可翻译文案（对比 `about` 才是）。
/// 前端字典里同样没有 `menu.app` 字段，见 types.ts 的说明。
pub const APP_NAME: &str = "LazeCode";

/// 菜单文案表。
///
/// **这不是翻译真相**，真相在前端 `src/services/i18n/{en,zh-CN}.ts` 的
/// `menu` 子树。这里是构造原生菜单时取得的那一份快照。
///
/// 字段名是 snake_case，与前端 camelCase 一一对应（转换由
/// `scripts/verify-menu-parity.mjs` 校验，勿手工对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuLabels {
    pub file: &'static str,
    pub edit: &'static str,
    pub view: &'static str,
    pub navigate: &'static str,
    pub git: &'static str,
    pub help: &'static str,
    pub new_window: &'static str,
    pub open_project: &'static str,
    pub close_project: &'static str,
    pub save: &'static str,
    pub save_all: &'static str,
    pub close_tab: &'static str,
    pub close_all_tabs: &'static str,
    pub undo: &'static str,
    pub redo: &'static str,
    pub cut: &'static str,
    pub copy: &'static str,
    pub paste: &'static str,
    pub select_all: &'static str,
    pub tool_windows: &'static str,
    pub project: &'static str,
    pub commit: &'static str,
    pub git_log: &'static str,
    pub terminal: &'static str,
    pub preferences: &'static str,
    pub search_everywhere: &'static str,
    pub go_to_line: &'static str,
    pub commit_ellipsis: &'static str,
    pub push: &'static str,
    pub update_project: &'static str,
    pub fetch: &'static str,
    pub manage_remotes: &'static str,
    pub show_history: &'static str,
    pub stash: &'static str,
    pub unstash: &'static str,
    pub shelve: &'static str,
    pub resolve_conflicts: &'static str,
    pub refresh: &'static str,
    pub quit: &'static str,
    pub about: &'static str,
    pub check_for_updates: &'static str,
}

impl MenuLabels {
    /// 英文。
    pub const EN: MenuLabels = MenuLabels {
        file: "File",
        edit: "Edit",
        view: "View",
        navigate: "Navigate",
        git: "Git",
        help: "Help",
        new_window: "New Window",
        open_project: "Open Project…",
        close_project: "Close",
        save: "Save",
        save_all: "Save All",
        close_tab: "Close Tab",
        close_all_tabs: "Close All Tabs",
        undo: "Undo",
        redo: "Redo",
        cut: "Cut",
        copy: "Copy",
        paste: "Paste",
        select_all: "Select All",
        tool_windows: "Tool Windows",
        project: "Project",
        commit: "Commit",
        git_log: "Git Log",
        terminal: "Terminal",
        preferences: "Settings…",
        search_everywhere: "Search Everywhere…",
        go_to_line: "Go to Line…",
        commit_ellipsis: "Commit…",
        push: "Push…",
        update_project: "Update Project…",
        fetch: "Fetch",
        manage_remotes: "Manage Remotes…",
        show_history: "Show History…",
        stash: "Stash Changes…",
        unstash: "Unstash…",
        shelve: "Shelve…",
        resolve_conflicts: "Resolve Conflicts…",
        refresh: "Refresh",
        quit: "Quit",
        about: "About LazeCode",
        check_for_updates: "Check for Updates…",
    };

    /// 简体中文。
    pub const ZH_CN: MenuLabels = MenuLabels {
        file: "文件",
        edit: "编辑",
        view: "视图",
        navigate: "导航",
        git: "Git",
        help: "帮助",
        new_window: "新建窗口",
        open_project: "打开项目…",
        close_project: "关闭",
        save: "保存",
        save_all: "保存全部",
        close_tab: "关闭标签",
        close_all_tabs: "关闭全部标签",
        undo: "撤销",
        redo: "重做",
        cut: "剪切",
        copy: "复制",
        paste: "粘贴",
        select_all: "全选",
        tool_windows: "工具窗口",
        project: "项目",
        commit: "提交",
        git_log: "Git 日志",
        terminal: "终端",
        preferences: "偏好设置…",
        search_everywhere: "Search Everywhere…",
        go_to_line: "跳转到行…",
        commit_ellipsis: "提交…",
        push: "推送…",
        update_project: "更新项目…",
        fetch: "获取远程更新",
        manage_remotes: "管理远程仓库…",
        show_history: "显示历史…",
        stash: "贮藏更改…",
        unstash: "恢复贮藏…",
        shelve: "Shelve…",
        resolve_conflicts: "解决冲突…",
        refresh: "刷新",
        quit: "退出",
        about: "关于 LazeCode",
        check_for_updates: "检查更新…",
    };
}

/// 支持的语言标识。与前端 `Language` 的字面值一致（持久化值不能漂移）。
pub const LANG_EN: &str = "en";
pub const LANG_ZH_CN: &str = "zh-CN";

/// 归一化语言标识，取不到受支持值时回落到英文。
///
/// 与前端 `normalizeLanguage` **逐条等价**（含怪癖）：前端是
/// `v === 'zh' || v.toLowerCase().startsWith('zh')`，即**不要求连字符** ——
/// `zhCN` 也算中文。这里必须照抄，否则手改文件写出 `zhCN` 时会出现
/// 「界面中文、菜单英文」的割裂状态。
///
/// 未知值（fr / 空串 / 乱码）回落 `en`：与前端 `DEFAULT_LANGUAGE` 一致。
pub fn normalize_menu_language(value: &str) -> &'static str {
    let v = value.trim().replace('_', "-").to_ascii_lowercase();
    if v == "zh" || v.starts_with("zh") {
        LANG_ZH_CN
    } else {
        LANG_EN
    }
}

/// 按语言取文案表。
pub fn labels_for(language: &str) -> MenuLabels {
    match normalize_menu_language(language) {
        LANG_ZH_CN => MenuLabels::ZH_CN,
        _ => MenuLabels::EN,
    }
}

/// 构造应用菜单（固定英文）。
///
/// 保留给「明确要英文菜单」的场景；启动链路一律用 `build_menu_with`
/// 带上持久化的语言。
pub fn build_menu<R: Runtime>(handle: &tauri::AppHandle<R>) -> tauri::Result<Menu<R>> {
    build_menu_with(handle, MenuLabels::EN)
}

/// 用指定语言构造菜单。菜单唯一的构造入口。
pub fn build_menu_with<R: Runtime>(
    handle: &tauri::AppHandle<R>,
    l: MenuLabels,
) -> tauri::Result<Menu<R>> {
    // ---------- File ----------
    let new_window = MenuItem::with_id(
        handle,
        ids::NEW_WINDOW,
        l.new_window,
        true,
        Some("CmdOrCtrl+Shift+N"),
    )?;
    let open_project = MenuItem::with_id(
        handle,
        ids::OPEN_PROJECT,
        l.open_project,
        true,
        Some("CmdOrCtrl+O"),
    )?;
    let close_project = MenuItem::with_id(
        handle,
        ids::CLOSE_PROJECT,
        l.close_project,
        true,
        None::<&str>,
    )?;
    let save = MenuItem::with_id(handle, ids::SAVE, l.save, true, Some("CmdOrCtrl+S"))?;
    let save_all = MenuItem::with_id(handle, ids::SAVE_ALL, l.save_all, true, None::<&str>)?;
    let close_tab = MenuItem::with_id(
        handle,
        ids::CLOSE_TAB,
        l.close_tab,
        true,
        Some("CmdOrCtrl+W"),
    )?;
    let close_all_tabs = MenuItem::with_id(
        handle,
        ids::CLOSE_ALL_TABS,
        l.close_all_tabs,
        true,
        None::<&str>,
    )?;
    let quit = PredefinedMenuItem::quit(handle, Some(l.quit))?;

    let file_menu = Submenu::with_items(
        handle,
        l.file,
        true,
        &[
            &new_window,
            &open_project,
            &close_project,
            &PredefinedMenuItem::separator(handle)?,
            &save,
            &save_all,
            &PredefinedMenuItem::separator(handle)?,
            &close_tab,
            &close_all_tabs,
            &PredefinedMenuItem::separator(handle)?,
            &quit,
        ],
    )?;

    // ---------- Edit（编辑）----------
    // 这一栏**必须有**。macOS 上 Cmd+C / Cmd+V / Cmd+X / Cmd+A 不是网页自己
    // 处理的按键，而是菜单快捷键：由菜单项把 `copy:` / `paste:` / … 动作沿
    // 响应链送到 WKWebView，WebView 才派发 DOM 的 copy/paste 事件。
    //
    // 此前菜单是自定义的且没有这一栏，而 Tauri 默认菜单（含 Edit）被它整体
    // 替换掉了 —— 于是 Cmd+V 再没有任何入口：终端（隐藏 textarea 的 onPaste）
    // 粘不进去，编辑器、提交信息框、各类对话框同样粘不进去。
    // 全是 predefined 项：不需要前端事件转发，交给系统响应链即可。
    let edit_menu = Submenu::with_items(
        handle,
        l.edit,
        true,
        &[
            &PredefinedMenuItem::undo(handle, Some(l.undo))?,
            &PredefinedMenuItem::redo(handle, Some(l.redo))?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::cut(handle, Some(l.cut))?,
            &PredefinedMenuItem::copy(handle, Some(l.copy))?,
            &PredefinedMenuItem::paste(handle, Some(l.paste))?,
            &PredefinedMenuItem::select_all(handle, Some(l.select_all))?,
        ],
    )?;

    // ---------- View / Tool Windows ----------
    let project_drawer = MenuItem::with_id(
        handle,
        ids::TOGGLE_PROJECT_DRAWER,
        l.project,
        true,
        Some("Alt+1"),
    )?;
    let commit_drawer = MenuItem::with_id(
        handle,
        ids::TOGGLE_COMMIT_DRAWER,
        l.commit,
        true,
        Some("Alt+0"),
    )?;
    let git_panel = MenuItem::with_id(
        handle,
        ids::TOGGLE_GIT_PANEL,
        l.git_log,
        true,
        Some("Alt+9"),
    )?;
    let terminal = MenuItem::with_id(
        handle,
        ids::TOGGLE_TERMINAL,
        l.terminal,
        true,
        Some("Alt+F12"),
    )?;
    let settings = MenuItem::with_id(
        handle,
        ids::TOGGLE_SETTINGS,
        l.preferences,
        true,
        Some("CmdOrCtrl+,"),
    )?;

    let tool_windows = Submenu::with_items(
        handle,
        l.tool_windows,
        true,
        &[&project_drawer, &commit_drawer, &git_panel, &terminal],
    )?;

    let view_menu = Submenu::with_items(
        handle,
        l.view,
        true,
        &[
            &tool_windows,
            &PredefinedMenuItem::separator(handle)?,
            &settings,
        ],
    )?;

    // ---------- Navigate ----------
    let search_everywhere = MenuItem::with_id(
        handle,
        ids::SEARCH_EVERYWHERE,
        l.search_everywhere,
        true,
        None::<&str>,
    )?;
    let go_to_line = MenuItem::with_id(
        handle,
        ids::GO_TO_LINE,
        l.go_to_line,
        true,
        Some("CmdOrCtrl+L"),
    )?;

    let navigate_menu =
        Submenu::with_items(handle, l.navigate, true, &[&search_everywhere, &go_to_line])?;

    // ---------- Git ----------
    let commit = MenuItem::with_id(
        handle,
        ids::COMMIT,
        l.commit_ellipsis,
        true,
        Some("CmdOrCtrl+K"),
    )?;
    let push = MenuItem::with_id(handle, ids::PUSH, l.push, true, Some("CmdOrCtrl+Shift+K"))?;
    let update = MenuItem::with_id(
        handle,
        ids::UPDATE_PROJECT,
        l.update_project,
        true,
        Some("CmdOrCtrl+T"),
    )?;
    let fetch = MenuItem::with_id(handle, ids::FETCH, l.fetch, true, None::<&str>)?;
    let manage_remotes = MenuItem::with_id(
        handle,
        ids::MANAGE_REMOTES,
        l.manage_remotes,
        true,
        None::<&str>,
    )?;
    let refresh = MenuItem::with_id(handle, ids::REFRESH, l.refresh, true, Some("F5"))?;
    let history = MenuItem::with_id(
        handle,
        ids::SHOW_HISTORY,
        l.show_history,
        true,
        None::<&str>,
    )?;
    let stash = MenuItem::with_id(handle, ids::STASH, l.stash, true, None::<&str>)?;
    let unstash = MenuItem::with_id(handle, ids::UNSTASH, l.unstash, true, None::<&str>)?;
    let shelve = MenuItem::with_id(handle, ids::SHELVE, l.shelve, true, None::<&str>)?;
    let conflicts = MenuItem::with_id(
        handle,
        ids::RESOLVE_CONFLICTS,
        l.resolve_conflicts,
        true,
        None::<&str>,
    )?;

    let git_menu = Submenu::with_items(
        handle,
        l.git,
        true,
        &[
            &commit,
            &push,
            &update,
            &fetch,
            &manage_remotes,
            &PredefinedMenuItem::separator(handle)?,
            &stash,
            &unstash,
            &shelve,
            &PredefinedMenuItem::separator(handle)?,
            &conflicts,
            &history,
            &PredefinedMenuItem::separator(handle)?,
            &refresh,
        ],
    )?;

    // ---------- Help ----------
    // 只放「关于」：本项目的快捷键散落在各处 tooltip 与欢迎页，
    // 单独做帮助窗口超出当前范围。
    let about = PredefinedMenuItem::about(handle, Some(l.about), None)?;
    let check_updates_help = MenuItem::with_id(
        handle,
        ids::CHECK_FOR_UPDATES,
        l.check_for_updates,
        true,
        None::<&str>,
    )?;
    let help_menu = Submenu::with_items(handle, l.help, true, &[&about, &check_updates_help])?;

    // macOS：首个 Submenu 会变成应用菜单（关于/服务/隐藏/退出那一组），
    // 必须显式提供，否则 Tauri 用默认项占位。
    let app_menu = Submenu::with_items(
        handle,
        APP_NAME,
        true,
        &[
            &PredefinedMenuItem::about(handle, None, None)?,
            &MenuItem::with_id(
                handle,
                ids::CHECK_FOR_UPDATES,
                l.check_for_updates,
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::services(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::hide(handle, None)?,
            &PredefinedMenuItem::hide_others(handle, None)?,
            &PredefinedMenuItem::show_all(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &quit,
        ],
    )?;

    Menu::with_items(
        handle,
        &[
            &app_menu,
            &file_menu,
            &edit_menu,
            &view_menu,
            &navigate_menu,
            &git_menu,
            &help_menu,
        ],
    )
}

/// 把菜单点击事件转发给该窗口的前端。
///
/// 转发成 `app-menu://<id>` 而不是复用 `menu` 事件：前端只 listen 自己关心的
/// id，且字符串比 `tauri::menu::MenuEvent` 更易与既有回调对接。
pub fn emit_menu_event<R: Runtime, E: Emitter<R>>(emitter: &E, id: &str) {
    let _ = emitter.emit(&format!("{}{}", EVENT_PREFIX, id), ());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 语言归一化必须与前端 `normalizeLanguage` 同一套规则，
    /// 否则会出现「界面中文、菜单英文」的割裂状态。
    #[test]
    fn normalizes_language_like_frontend() {
        assert_eq!(normalize_menu_language("en"), LANG_EN);
        assert_eq!(normalize_menu_language("EN"), LANG_EN);
        assert_eq!(normalize_menu_language("en-US"), LANG_EN);
        assert_eq!(normalize_menu_language("zh-CN"), LANG_ZH_CN);
        assert_eq!(normalize_menu_language("zh_CN"), LANG_ZH_CN);
        assert_eq!(normalize_menu_language("zh"), LANG_ZH_CN);
        assert_eq!(normalize_menu_language("zh-Hans"), LANG_ZH_CN);
        assert_eq!(normalize_menu_language("ZH"), LANG_ZH_CN);
        assert_eq!(normalize_menu_language("  zh  "), LANG_ZH_CN);
        // 前端规则是 startsWith('zh') 而非 startsWith('zh-')：拼错的
        // "zhCN" / "zh-C" 也算中文。两边必须一致，否则界面与菜单语言割裂。
        assert_eq!(normalize_menu_language("zhCN"), LANG_ZH_CN);
        assert_eq!(normalize_menu_language("zh-C"), LANG_ZH_CN);
        // 未知值一律回落英文，绝不 panic
        assert_eq!(normalize_menu_language("fr"), LANG_EN);
        assert_eq!(normalize_menu_language("ja"), LANG_EN);
        assert_eq!(normalize_menu_language(""), LANG_EN);
    }

    /// 两张表的文案必须**都没有空串**。空串会让菜单项渲染成一条空白，
    /// 用户看到"少了一项"而无法点。
    #[test]
    fn no_empty_labels() {
        let l = MenuLabels::EN;
        for (name, v) in [
            ("file", l.file),
            ("edit", l.edit),
            ("view", l.view),
            ("navigate", l.navigate),
            ("git", l.git),
            ("help", l.help),
            ("new_window", l.new_window),
            ("open_project", l.open_project),
            ("close_project", l.close_project),
            ("save", l.save),
            ("save_all", l.save_all),
            ("close_tab", l.close_tab),
            ("close_all_tabs", l.close_all_tabs),
            ("undo", l.undo),
            ("redo", l.redo),
            ("cut", l.cut),
            ("copy", l.copy),
            ("paste", l.paste),
            ("select_all", l.select_all),
            ("tool_windows", l.tool_windows),
            ("project", l.project),
            ("commit", l.commit),
            ("git_log", l.git_log),
            ("terminal", l.terminal),
            ("preferences", l.preferences),
            ("search_everywhere", l.search_everywhere),
            ("go_to_line", l.go_to_line),
            ("commit_ellipsis", l.commit_ellipsis),
            ("push", l.push),
            ("update_project", l.update_project),
            ("fetch", l.fetch),
            ("show_history", l.show_history),
            ("stash", l.stash),
            ("unstash", l.unstash),
            ("shelve", l.shelve),
            ("resolve_conflicts", l.resolve_conflicts),
            ("refresh", l.refresh),
            ("quit", l.quit),
            ("about", l.about),
        ] {
            assert!(!v.is_empty(), "en.{name} 为空");
        }
        let z = MenuLabels::ZH_CN;
        assert!(!z.file.is_empty() && !z.git.is_empty() && !z.quit.is_empty());
    }

    /// `labels_for` 必须真的返回对应语言的表，而不是永远英文。
    /// 这是「菜单切换是否生效」的唯一半路检查点。
    #[test]
    fn labels_for_returns_requested_language() {
        assert_eq!(labels_for("en").file, "File");
        assert_eq!(labels_for("zh-CN").file, "文件");
        // 中文表在若干关键项上确实不同于英文表
        assert_ne!(labels_for("zh-CN").save, labels_for("en").save);
        assert_ne!(labels_for("zh-CN").quit, labels_for("en").quit);
    }

    /// 菜单项 id 是前后端契约：任何一端改了名字，另一边就收不到事件。
    /// 这条断言守的是「id 常量可序列化为非空ASCII串」。
    #[test]
    fn menu_ids_are_ascii_and_non_empty() {
        for id in [
            ids::NEW_WINDOW,
            ids::OPEN_PROJECT,
            ids::CLOSE_PROJECT,
            ids::SAVE,
            ids::SAVE_ALL,
            ids::CLOSE_TAB,
            ids::CLOSE_ALL_TABS,
            ids::TOGGLE_PROJECT_DRAWER,
            ids::TOGGLE_COMMIT_DRAWER,
            ids::TOGGLE_GIT_PANEL,
            ids::TOGGLE_TERMINAL,
            ids::TOGGLE_SETTINGS,
            ids::SEARCH_EVERYWHERE,
            ids::GO_TO_LINE,
            ids::COMMIT,
            ids::PUSH,
            ids::UPDATE_PROJECT,
            ids::FETCH,
            ids::SHOW_HISTORY,
            ids::STASH,
            ids::UNSTASH,
            ids::SHELVE,
            ids::RESOLVE_CONFLICTS,
            ids::REFRESH,
            ids::CHECK_FOR_UPDATES,
        ] {
            assert!(!id.is_empty());
            assert!(id.is_ascii(), "菜单 id 含非 ASCII 字符: {id}");
            // 前端按字符串 listen，含空白会静默失配
            assert_eq!(id.trim(), id, "菜单 id 含首尾空白: {id:?}");
        }
    }
}
