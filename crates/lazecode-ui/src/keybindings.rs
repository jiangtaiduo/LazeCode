//! 可自定义的应用级快捷键：动作目录、默认键、`settings.toml` 覆盖解析、
//! 组合键串（`Cmd+1`）与显示串（`⌘1`）互转，以及顶栏工具按钮的统一触发
//! 逻辑（快捷键与点击共用同一份实现）。

use egui::{Key, Modifiers};
use lazecode_core::{AppModel, Command, PanelTab, SidebarView, TerminalPlacement};

use crate::runtime::UiRuntime;

/// 一个组合键：修饰符 + 按键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub modifiers: Modifiers,
    pub key: Key,
}

/// 所有可在设置页改键的动作。顺序即设置页的展示顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    // --- 顶栏工具（六个按钮）---
    Files,
    Changes,
    GitLog,
    Terminal,
    Consoles,
    Compare,
    // --- 其余应用级快捷键（shortcuts.rs 分发）---
    OpenPalette,
    OpenSettings,
    OpenCommit,
    NewTerminal,
    OpenProject,
    Save,
    Undo,
    Redo,
    Search,
    Find,
    GotoLine,
    NewFile,
    CloseTab,
    TogglePanel,
    FormatJson,
    MinifyJson,
    UnshelveSilent,
    UnshelveDialog,
    ToggleTreeView,
}

/// 设置页展示顺序（工具键在前，其余按功能聚簇）。
pub const ALL_ACTIONS: [Action; 25] = [
    Action::Files,
    Action::Changes,
    Action::GitLog,
    Action::Terminal,
    Action::Consoles,
    Action::Compare,
    Action::OpenPalette,
    Action::OpenSettings,
    Action::OpenCommit,
    Action::NewTerminal,
    Action::OpenProject,
    Action::Save,
    Action::Undo,
    Action::Redo,
    Action::Search,
    Action::Find,
    Action::GotoLine,
    Action::NewFile,
    Action::CloseTab,
    Action::TogglePanel,
    Action::FormatJson,
    Action::MinifyJson,
    Action::UnshelveSilent,
    Action::UnshelveDialog,
    Action::ToggleTreeView,
];

#[allow(clippy::len_without_is_empty)]
impl Action {
    /// 稳定的设置持久化 id，不得改动（旧 `settings.toml` 依赖它）。
    pub fn id(self) -> &'static str {
        match self {
            Action::Files => "files",
            Action::Changes => "changes",
            Action::GitLog => "git_log",
            Action::Terminal => "terminal",
            Action::Consoles => "consoles",
            Action::Compare => "compare",
            Action::OpenPalette => "palette",
            Action::OpenSettings => "settings",
            Action::OpenCommit => "commit",
            Action::NewTerminal => "new_terminal",
            Action::OpenProject => "open_project",
            Action::Save => "save",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::Search => "search",
            Action::Find => "find",
            Action::GotoLine => "goto_line",
            Action::NewFile => "new_file",
            Action::CloseTab => "close_tab",
            Action::TogglePanel => "panel",
            Action::FormatJson => "format_json",
            Action::MinifyJson => "minify_json",
            Action::UnshelveSilent => "unshelve",
            Action::UnshelveDialog => "unshelve_dialog",
            Action::ToggleTreeView => "group_by_dir",
        }
    }
}

/// 从持久化 id 反查动作；未知 id（未来版本删除的动作）被忽略。
pub fn action_by_id(text: &str) -> Option<Action> {
    ALL_ACTIONS.into_iter().find(|action| action.id() == text)
}

/// 设置页/提示里展示的动作名。
pub fn label(model: &AppModel, action: Action) -> String {
    let (en, zh) = match action {
        Action::Files => ("Files", "文件"),
        Action::Changes => ("Changes", "变更清单"),
        Action::GitLog => ("Git Log", "Git 日志"),
        Action::Terminal => ("Terminal", "终端"),
        Action::Consoles => ("Consoles", "控制台"),
        Action::Compare => ("Compare", "比较"),
        Action::OpenPalette => ("Command Palette", "命令面板"),
        Action::OpenSettings => ("Settings", "设置"),
        Action::OpenCommit => ("Commit Drawer", "提交抽屉"),
        Action::NewTerminal => ("New Terminal", "新建终端"),
        Action::OpenProject => ("Open Project", "打开项目"),
        Action::Save => ("Save", "保存"),
        Action::Undo => ("Undo", "撤销"),
        Action::Redo => ("Redo", "重做"),
        Action::Search => ("Search Everywhere", "全局搜索"),
        Action::Find => ("Find in File", "文件内查找"),
        Action::GotoLine => ("Go to Line", "跳转到行"),
        Action::NewFile => ("New File", "新建文件"),
        Action::CloseTab => ("Close Tab / Overlay", "关闭标签页/浮层"),
        Action::TogglePanel => ("Toggle Bottom Panel", "切换底部面板"),
        Action::FormatJson => ("Format JSON", "格式化 JSON"),
        Action::MinifyJson => ("Minify JSON", "压缩 JSON"),
        Action::UnshelveSilent => ("Unshelve (silent)", "应用贮藏（静默）"),
        Action::UnshelveDialog => ("Unshelve (dialog)", "应用贮藏（对话框）"),
        Action::ToggleTreeView => ("Group by Directory", "按目录分组"),
    };
    crate::tr!(model, en, zh).to_string()
}

fn command() -> Modifiers {
    Modifiers {
        command: true,
        ..Default::default()
    }
}

fn command_shift() -> Modifiers {
    Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    }
}

fn alt_shift() -> Modifiers {
    Modifiers {
        alt: true,
        shift: true,
        ..Default::default()
    }
}

fn command_alt() -> Modifiers {
    Modifiers {
        command: true,
        alt: true,
        ..Default::default()
    }
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Default::default()
    }
}

/// 编译期默认键；`None` = 默认无快捷键（Compare、面板显隐）。
pub fn default_binding(action: Action) -> Option<Binding> {
    let bind = |modifiers, key| Some(Binding { modifiers, key });
    match action {
        Action::Files => bind(command(), Key::Backtick),
        Action::Changes => bind(command(), Key::Num1),
        Action::GitLog => bind(command(), Key::Num2),
        Action::Terminal => bind(command(), Key::Num3),
        Action::Consoles => bind(command(), Key::Num4),
        Action::Compare => None,
        Action::OpenPalette => bind(command_shift(), Key::A),
        Action::OpenSettings => bind(command(), Key::Comma),
        Action::OpenCommit => bind(command(), Key::K),
        Action::NewTerminal => bind(command_shift(), Key::T),
        Action::OpenProject => bind(command(), Key::O),
        Action::Save => bind(command(), Key::S),
        Action::Undo => bind(command(), Key::Z),
        Action::Redo => bind(command_shift(), Key::Z),
        Action::Search => bind(command_shift(), Key::F),
        Action::Find => bind(command(), Key::F),
        Action::GotoLine => bind(command(), Key::L),
        Action::NewFile => bind(command(), Key::N),
        Action::CloseTab => bind(command(), Key::W),
        Action::TogglePanel => None,
        Action::FormatJson => bind(alt_shift(), Key::F),
        Action::MinifyJson => bind(alt_shift(), Key::M),
        Action::UnshelveSilent => bind(command_alt(), Key::U),
        Action::UnshelveDialog => bind(command_shift(), Key::U),
        Action::ToggleTreeView => bind(ctrl(), Key::P),
    }
}

fn key_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::ArrowDown => "ArrowDown",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowRight => "ArrowRight",
        Key::ArrowUp => "ArrowUp",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Enter => "Enter",
        Key::Space => "Space",
        Key::Insert => "Insert",
        Key::Delete => "Delete",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Colon => "Colon",
        Key::Comma => "Comma",
        Key::Backslash => "Backslash",
        Key::Slash => "Slash",
        Key::Pipe => "Pipe",
        Key::Questionmark => "Questionmark",
        Key::Exclamationmark => "Exclamationmark",
        Key::OpenBracket => "OpenBracket",
        Key::CloseBracket => "CloseBracket",
        Key::OpenCurlyBracket => "OpenCurlyBracket",
        Key::CloseCurlyBracket => "CloseCurlyBracket",
        Key::Backtick => "Backtick",
        Key::Minus => "Minus",
        Key::Period => "Period",
        Key::Plus => "Plus",
        Key::Equals => "Equals",
        Key::Semicolon => "Semicolon",
        Key::Quote => "Quote",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        _ => return None,
    })
}

fn key_by_name(name: &str) -> Option<Key> {
    let digit = |prefix: &str, index: usize| {
        name.strip_prefix(prefix)
            .and_then(|rest| rest.parse::<usize>().ok())
            .filter(|&value| value == index)
            .is_some()
    };
    Some(match name {
        "ArrowDown" => Key::ArrowDown,
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "ArrowUp" => Key::ArrowUp,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Enter" => Key::Enter,
        "Space" => Key::Space,
        "Insert" => Key::Insert,
        "Delete" => Key::Delete,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Colon" => Key::Colon,
        "Comma" => Key::Comma,
        "Backslash" => Key::Backslash,
        "Slash" => Key::Slash,
        "Pipe" => Key::Pipe,
        "Questionmark" => Key::Questionmark,
        "Exclamationmark" => Key::Exclamationmark,
        "OpenBracket" => Key::OpenBracket,
        "CloseBracket" => Key::CloseBracket,
        "OpenCurlyBracket" => Key::OpenCurlyBracket,
        "CloseCurlyBracket" => Key::CloseCurlyBracket,
        "Backtick" => Key::Backtick,
        "Minus" => Key::Minus,
        "Period" => Key::Period,
        "Plus" => Key::Plus,
        "Equals" => Key::Equals,
        "Semicolon" => Key::Semicolon,
        "Quote" => Key::Quote,
        "0" => Key::Num0,
        "1" => Key::Num1,
        "2" => Key::Num2,
        "3" => Key::Num3,
        "4" => Key::Num4,
        "5" => Key::Num5,
        "6" => Key::Num6,
        "7" => Key::Num7,
        "8" => Key::Num8,
        "9" => Key::Num9,
        "A" => Key::A,
        "B" => Key::B,
        "C" => Key::C,
        "D" => Key::D,
        "E" => Key::E,
        "F" => Key::F,
        "G" => Key::G,
        "H" => Key::H,
        "I" => Key::I,
        "J" => Key::J,
        "K" => Key::K,
        "L" => Key::L,
        "M" => Key::M,
        "N" => Key::N,
        "O" => Key::O,
        "P" => Key::P,
        "Q" => Key::Q,
        "R" => Key::R,
        "S" => Key::S,
        "T" => Key::T,
        "U" => Key::U,
        "V" => Key::V,
        "W" => Key::W,
        "X" => Key::X,
        "Y" => Key::Y,
        "Z" => Key::Z,
        _ if digit("F", 1) => Key::F1,
        _ if digit("F", 2) => Key::F2,
        _ if digit("F", 3) => Key::F3,
        _ if digit("F", 4) => Key::F4,
        _ if digit("F", 5) => Key::F5,
        _ if digit("F", 6) => Key::F6,
        _ if digit("F", 7) => Key::F7,
        _ if digit("F", 8) => Key::F8,
        _ if digit("F", 9) => Key::F9,
        _ if digit("F", 10) => Key::F10,
        _ if digit("F", 11) => Key::F11,
        _ if digit("F", 12) => Key::F12,
        _ => return None,
    })
}

/// 组合键的持久化串（`Cmd+1`/`Ctrl+P`），修饰符顺序固定。
pub fn binding_string(binding: Binding) -> String {
    let mut tokens = Vec::new();
    if binding.modifiers.command {
        tokens.push("Cmd");
    }
    if binding.modifiers.ctrl {
        tokens.push("Ctrl");
    }
    if binding.modifiers.alt {
        tokens.push("Alt");
    }
    if binding.modifiers.shift {
        tokens.push("Shift");
    }
    let key = key_name(binding.key).unwrap_or("Unknown");
    tokens.push(key);
    tokens.join("+")
}

/// 解析持久化串；未知修饰符/按键返回 `None`（回退默认键）。
pub fn parse_binding(text: &str) -> Option<Binding> {
    let mut modifiers = Modifiers::default();
    let mut key = None;
    for token in text.split('+').filter(|token| !token.is_empty()) {
        match token {
            "Cmd" => modifiers.command = true,
            "Ctrl" => modifiers.ctrl = true,
            "Alt" => modifiers.alt = true,
            "Shift" => modifiers.shift = true,
            other => {
                key = Some(key_by_name(other)?);
            }
        }
    }
    Some(Binding {
        modifiers,
        key: key?,
    })
}

/// mac 风格显示串（`⌘1`/`⇧⌘F`/`⌃P`）；`None` 显示为空。
pub fn binding_display(binding: Option<Binding>) -> String {
    let Some(binding) = binding else {
        return String::new();
    };
    let mut symbols = Vec::new();
    if binding.modifiers.ctrl {
        symbols.push("⌃");
    }
    if binding.modifiers.alt {
        symbols.push("⌥");
    }
    if binding.modifiers.shift {
        symbols.push("⇧");
    }
    if binding.modifiers.command {
        symbols.push("⌘");
    }
    let key = key_name(binding.key).unwrap_or("?");
    // 字母/数字直接显示，命名键加空格更易读（⌘ Backtick → ` 直接显示）。
    symbols.push(key);
    symbols.concat()
}

/// 用户覆盖优先；覆盖串无效时回退默认；显式清空（空串）表示无键。
pub fn resolved(model: &AppModel, action: Action) -> Option<Binding> {
    match model.settings.keybindings.get(action.id()) {
        Some(text) => {
            if text.is_empty() {
                None
            } else {
                parse_binding(text).or_else(|| default_binding(action))
            }
        }
        None => default_binding(action),
    }
}

/// 候选组合键是否已被其他动作占用；返回占用者的动作。
pub fn conflict(model: &AppModel, action: Action, candidate: Binding) -> Option<Action> {
    ALL_ACTIONS.into_iter().find(|&other| {
        other != action && resolved(model, other).is_some_and(|binding| binding == candidate)
    })
}

/// 反查当前拥有 `candidate` 组合键的动作（原生菜单加速键按此派发，
/// 用户在设置页改键后原生层依然正确）。
pub fn action_for_binding(model: &AppModel, candidate: Binding) -> Option<Action> {
    ALL_ACTIONS.into_iter().find(|&action| resolved(model, action) == Some(candidate))
}

/// 设置页录入态：下一次非修饰键按下即成为 `Some(action)` 的新键。
pub fn capture_tick(ctx: &egui::Context, model: &mut AppModel, rt: &mut UiRuntime) {
    let Some(action) = rt.keybind_capture else {
        return;
    };
    let mut captured: Option<Binding> = None;
    let mut cancel = false;
    ctx.input_mut(|input| {
        for event in &input.events {
            if let egui::Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } = event
            {
                if !pressed || *repeat {
                    continue;
                }
                if *key == Key::Escape {
                    cancel = true;
                } else if captured.is_none() {
                    captured = Some(Binding {
                        modifiers: *modifiers,
                        key: *key,
                    });
                }
            }
        }
        if cancel || captured.is_some() {
            // 吞掉这一帧的按键事件，避免录入完成后同帧再触发其他动作。
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Key { pressed: true, .. }));
        }
    });
    if cancel {
        rt.keybind_capture = None;
        return;
    }
    let Some(binding) = captured else {
        return;
    };
    rt.keybind_capture = None;
    if let Some(other) = conflict(model, action, binding) {
        model.toast(
            format!(
                "「{}」已被「{}」占用，未保存",
                binding_display(Some(binding)),
                label(model, other)
            ),
            lazecode_core::ToastKind::Warning,
        );
        return;
    }
    model
        .settings
        .keybindings
        .insert(action.id().to_string(), binding_string(binding));
    model.save_settings();
}

/// 顶栏工具的统一触发逻辑；快捷键（shortcuts.rs）与按钮点击（titlebar.rs）
/// 都走这里，保证两路行为一致。
pub fn run_tool(model: &mut AppModel, action: Action) {
    let has_project = model.active_project().is_some();
    let single_active = model.active_project().is_some_and(|project| project.is_single_file);
    let single_needs_terminal = single_active
        && model.active_project().is_some_and(|project| {
            project
                .worktrees
                .iter()
                .all(|worktree| worktree.terminals.is_empty())
        });
    match action {
        Action::Files => {
            let active = !single_active
                && model.ui.sidebar_visible
                && model.ui.sidebar_view == SidebarView::Files;
            if active {
                model.ui.sidebar_visible = false;
            } else {
                model.ui.sidebar_view = SidebarView::Files;
                model.ui.sidebar_visible = true;
            }
        }
        Action::Changes => {
            let active = !single_active
                && model.ui.sidebar_visible
                && model.ui.sidebar_view == SidebarView::Git;
            if active {
                model.ui.sidebar_visible = false;
            } else {
                model.ui.sidebar_view = SidebarView::Git;
                model.ui.sidebar_visible = true;
            }
        }
        Action::GitLog => {
            let active = model.ui.panel_visible && model.ui.panel_tab == PanelTab::Git;
            if active {
                model.ui.panel_visible = false;
            } else {
                model.ui.panel_visible = true;
                model.ui.panel_tab = PanelTab::Git;
                if model.ui.terminal_placement == TerminalPlacement::Bottom {
                    model.ui.terminal_visible = false;
                }
                model.queue(Command::RefreshLog);
            }
        }
        Action::Terminal => {
            let terminal_needs_host = single_needs_terminal || !has_project;
            let active = model.ui.terminal_visible;
            if model.ui.terminal_placement == TerminalPlacement::Bottom {
                if active && terminal_needs_host {
                    // dock 可见但没有 shell：第一次点应建一个宿主终端而不是
                    // 收起 dock。
                    model.queue(Command::NewTerminal);
                } else if active {
                    model.ui.terminal_visible = false;
                } else {
                    model.ui.terminal_visible = true;
                    model.ui.panel_visible = false;
                    if terminal_needs_host {
                        model.queue(Command::NewTerminal);
                    }
                }
            } else {
                model.ui.terminal_visible = !active;
                if model.ui.terminal_visible && terminal_needs_host {
                    model.queue(Command::NewTerminal);
                }
            }
        }
        Action::Consoles => {
            let active = model.ui.sidebar_visible
                && model.ui.sidebar_view == SidebarView::Consoles;
            if active {
                model.ui.sidebar_visible = false;
            } else {
                if !has_project || single_needs_terminal {
                    // 无项目，或单文件工作区还没有 shell：建一个家目录终端。
                    model.queue(Command::NewTerminal);
                }
                model.ui.sidebar_view = SidebarView::Consoles;
                model.ui.sidebar_visible = true;
            }
        }
        Action::Compare => {
            let active = !single_active
                && model.ui.sidebar_visible
                && model.ui.sidebar_view == SidebarView::Compare;
            if active {
                model.ui.sidebar_view = SidebarView::Git;
            } else {
                model.ui.sidebar_view = SidebarView::Compare;
                model.ui.sidebar_visible = true;
                if let Some(project) = model.active_project_mut() {
                    if let Some(session) = project.compare_session.as_mut() {
                        session.minimized = false;
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_round_trip_through_the_canonical_string() {
        for action in ALL_ACTIONS {
            let Some(binding) = default_binding(action) else {
                continue;
            };
            let text = binding_string(binding);
            assert_eq!(parse_binding(&text), Some(binding), "{text}");
            assert_eq!(parse_binding(&text), Some(parse_binding(&text).unwrap()));
        }
    }

    #[test]
    fn defaults_do_not_collide() {
        for (index, action) in ALL_ACTIONS.into_iter().enumerate() {
            let Some(binding) = default_binding(action) else {
                continue;
            };
            for other in ALL_ACTIONS.into_iter().skip(index + 1) {
                assert_ne!(
                    default_binding(other),
                    Some(binding),
                    "{} and {} share {}",
                    action.id(),
                    other.id(),
                    binding_string(binding)
                );
            }
        }
    }

    #[test]
    fn ids_are_unique_and_resolve_back() {
        for action in ALL_ACTIONS {
            assert_eq!(action_by_id(action.id()), Some(action), "{}", action.id());
        }
    }

    #[test]
    fn empty_override_unbinds_and_bad_override_falls_back() {
        // 纯数据测试不构造 AppModel：直接验证解析路径的两条规则。
        assert_eq!(parse_binding(""), None);
        assert_eq!(parse_binding("Cmd+NotAKey"), None);
        assert_eq!(
            parse_binding("Cmd+1").map(|binding| binding_string(binding)),
            Some("Cmd+1".to_string())
        );
    }
}
