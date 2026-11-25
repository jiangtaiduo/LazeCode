//! Context menus: the dispatcher plus one module per menu family.

mod branch;
mod log;
mod prelude;
mod project;
mod tabs;

use branch::*;
use log::*;
use prelude::*;
use project::*;
use tabs::*;

#[allow(clippy::too_many_arguments)]
fn draw_changelist_header_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    // payload: 组名 \u{1} 组内全部文件路径（稳定版组头菜单 = New/Delete
    // Changelist + 整组文件菜单，条目与文件行右键完全一致）。
    let mut segments = payload.split('\u{1}');
    let Some(name) = segments.next() else {
        return;
    };
    let paths: Vec<String> = segments
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let Some(project) = model.projects.get(project_idx) else {
        return;
    };
    let is_default = name.eq_ignore_ascii_case("default") || name.eq_ignore_ascii_case("changes");
    let unversioned = crate::tr!(model, "Unversioned Files", "未版本控制的文件");
    let ignored = crate::tr!(model, "Ignored Files", "被忽略的文件");
    let is_pseudo = name == unversioned || name == ignored;
    let zh = model.settings.language.is_zh();
    let mut items: Vec<ChangeMenuItem> = Vec::new();
    items.push(change_menu_item(
        if zh {
            "新建变更列表…".to_string()
        } else {
            "New Changelist…".to_string()
        },
        100,
        false,
        true,
    ));
    if !is_default && !is_pseudo {
        items.push(change_menu_item(
            if zh {
                "删除变更列表…".to_string()
            } else {
                "Delete Changelist…".to_string()
            },
            101,
            true,
            true,
        ));
    }
    let mut menu_ctx_holder: Option<ChangeMenuCtx> = None;
    if !paths.is_empty() {
        change_menu_sep(&mut items);
        let (file_items, menu_ctx) = change_menu_items(model, project, &paths);
        items.extend(file_items);
        menu_ctx_holder = Some(menu_ctx);
    } else {
        change_menu_sep(&mut items);
        items.push(change_menu_item(
            if zh {
                "刷新 VCS 更改".to_string()
            } else {
                "Refresh VCS Changes".to_string()
            },
            7,
            false,
            true,
        ));
    }

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行，直接列菜单项。
    let mut close = false;
    if let Some(code) =
        render_change_menu_items(ui, theme, rect, rect.top() + 8.0, "clist_h_ctx", &items)
    {
        match code {
            100 => {
                rt.dialog = DialogState::NewChangelist {
                    value: String::new(),
                    error: None,
                };
                close = true;
            }
            101 => {
                // 删除前确认（稳定版语义）：移入默认列表 or 回滚丢弃。
                if let Some(project) = model.projects.get(project_idx) {
                    let lists = lazecode_git::Changelists::open(project.active_root());
                    let count = lists.files_of(name).len();
                    rt.dialog = DialogState::DeleteChangelist {
                        name: name.to_string(),
                        count,
                        rollback: false,
                    };
                }
                close = true;
            }
            7 => {
                model.queue(Command::ReloadGit);
                close = true;
            }
            other => {
                if let Some(ctx) = &menu_ctx_holder {
                    close = apply_change_menu_action(ui, model, rt, ctx, other);
                }
            }
        }
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
/// Change-file menu codes: 0..10 generic, 50..200 move-to lists, 20/21 untracked,
/// 22..24 new actions, 30..37 extended actions, SEP divider, 100/101
/// changelist headers.
struct ChangeMenuItem {
    label: String,
    code: u8,
    danger: bool,
    enabled: bool,
}

const CHANGE_MENU_SEP: u8 = u8::MAX;

fn change_menu_sep(items: &mut Vec<ChangeMenuItem>) {
    items.push(ChangeMenuItem {
        label: String::new(),
        code: CHANGE_MENU_SEP,
        danger: false,
        enabled: false,
    });
}

fn change_menu_item(label: String, code: u8, danger: bool, enabled: bool) -> ChangeMenuItem {
    ChangeMenuItem {
        label,
        code,
        danger,
        enabled,
    }
}

struct ChangeMenuCtx {
    project_root: std::path::PathBuf,
    conflicts: Vec<lazecode_git::ConflictEntry>,
    paths: Vec<String>,
    untracked: std::collections::HashSet<String>,
    single: bool,
    first: String,
    display_name: String,
    all_lists: Vec<String>,
}

/// 构建变更文件菜单条目（文件行右键与变更列表组头右键共用，稳定版
/// CommitDrawer 菜单一比一：词条连 zh-CN 都保持原文，不走近路 tr!）。
fn change_menu_items(
    model: &AppModel,
    project: &lazecode_core::ProjectState,
    paths: &[String],
) -> (Vec<ChangeMenuItem>, ChangeMenuCtx) {
    let single = paths.len() == 1;
    let first = paths.first().cloned().unwrap_or_default();
    let lists = lazecode_git::Changelists::open(project.active_root());
    let all_lists: Vec<String> = lists.lists().to_vec();
    let untracked: std::collections::HashSet<String> = project
        .active_git()
        .map(|git| {
            git.changes
                .iter()
                .filter(|change| change.untracked)
                .map(|change| change.path.clone())
                .collect()
        })
        .unwrap_or_default();
    let ctx = ChangeMenuCtx {
        project_root: project.active_root().to_path_buf(),
        conflicts: project.conflicts.clone(),
        paths: paths.to_vec(),
        untracked: untracked.clone(),
        single,
        first: first.clone(),
        display_name: first.rsplit('/').next().unwrap_or(&first).to_string(),
        all_lists: all_lists.clone(),
    };
    let conflicted = project
        .conflicts
        .iter()
        .any(|entry| paths.iter().any(|path| path == &entry.path));
    // 稳定版 allUntracked 规则：所选文件全部是未跟踪才走未跟踪菜单。
    let all_untracked = paths.iter().all(|path| untracked.contains(path));

    let mut items: Vec<ChangeMenuItem> = Vec::new();
    let zh = model.settings.language.is_zh();
    if all_untracked {
        items.push(change_menu_item(
            if zh {
                if single {
                    "添加到 VCS".to_string()
                } else {
                    format!("添加 {} 个文件到 VCS", paths.len())
                }
            } else {
                if single {
                    "Add to VCS".to_string()
                } else {
                    format!("Add {} Files to VCS", paths.len())
                }
            },
            20,
            false,
            true,
        ));
        if single {
            items.push(change_menu_item(
                if zh {
                    "跳转到源代码".to_string()
                } else {
                    "Jump to Source".to_string()
                },
                1,
                false,
                true,
            ));
        }
        change_menu_sep(&mut items);
        items.push(change_menu_item(
            if zh {
                if single {
                    "删除…".to_string()
                } else {
                    format!("删除 {} 个文件…", paths.len())
                }
            } else {
                if single {
                    "Delete…".to_string()
                } else {
                    format!("Delete {} Files…", paths.len())
                }
            },
            21,
            true,
            true,
        ));
        return (items, ctx);
    }

    items.push(change_menu_item(
        if zh {
            if single {
                "提交文件…".to_string()
            } else {
                "提交所选文件…".to_string()
            }
        } else {
            if single {
                "Commit File...".to_string()
            } else {
                "Commit Selected Files…".to_string()
            }
        },
        3,
        false,
        true,
    ));
    if single {
        items.push(change_menu_item(
            if zh {
                "查看差异".to_string()
            } else {
                "Show Diff".to_string()
            },
            0,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "在新标签页查看差异".to_string()
            } else {
                "Show Diff in a New Tab".to_string()
            },
            30,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "与本地比较".to_string()
            } else {
                "Compare with Local".to_string()
            },
            31,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "与本地的前一版本比较".to_string()
            } else {
                "Compare Before with Local".to_string()
            },
            22,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "跳转到源代码".to_string()
            } else {
                "Jump to Source".to_string()
            },
            1,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "打开仓库版本".to_string()
            } else {
                "Open Repository Version".to_string()
            },
            32,
            false,
            true,
        ));
        if conflicted {
            items.push(change_menu_item(
                if zh {
                    "解决…".to_string()
                } else {
                    "Resolve…".to_string()
                },
                2,
                false,
                true,
            ));
        }
    }
    change_menu_sep(&mut items);
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "还原选中更改".to_string()
        } else {
            "Revert Selected Changes".to_string()
        },
        4,
        true,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "拣选选中更改".to_string()
        } else {
            "Cherry-Pick Selected Changes".to_string()
        },
        33,
        false,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "提取选中更改到独立提交…".to_string()
        } else {
            "Extract Selected Changes to Separate Commit…".to_string()
        },
        34,
        false,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "丢弃选中更改".to_string()
        } else {
            "Drop Selected Changes".to_string()
        },
        35,
        true,
        true,
    ));
    if single {
        items.push(change_menu_item(
            if zh {
                "从本地更改创建补丁…".to_string()
            } else {
                "Create Patch from Local Changes...".to_string()
            },
            23,
            false,
            true,
        ));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "从版本获取…".to_string()
            } else {
                "Get from Revision…".to_string()
            },
            36,
            false,
            true,
        ));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "到此为止的历史".to_string()
            } else {
                "History Up to Here".to_string()
            },
            24,
            false,
            true,
        ));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "显示与父提交的差异".to_string()
            } else {
                "Show Changes to Parents".to_string()
            },
            37,
            false,
            true,
        ));
    }
    change_menu_sep(&mut items);
    for (idx, list_name) in all_lists.iter().enumerate() {
        // 稳定版规则：所选文件已全部在该列表中 → 禁用（多选不再跳过）。
        let all_in_list = paths.iter().all(|path| lists.list_of(path) == *list_name);
        items.push(change_menu_item(
            if zh {
                format!("移动到 {list_name}")
            } else {
                format!("Move to {list_name}")
            },
            50 + idx as u8,
            false,
            !all_in_list,
        ));
    }
    if single {
        items.push(change_menu_item(
            if zh {
                "复制补丁到剪贴板".to_string()
            } else {
                "Copy as Patch to Clipboard".to_string()
            },
            5,
            false,
            true,
        ));
    }
    items.push(change_menu_item(
        if zh {
            "贮藏更改…".to_string()
        } else {
            "Shelve Changes...".to_string()
        },
        6,
        false,
        true,
    ));
    items.push(change_menu_item(
        if zh {
            "刷新 VCS 更改".to_string()
        } else {
            "Refresh VCS Changes".to_string()
        },
        7,
        false,
        true,
    ));
    items.push(change_menu_item(
        if zh {
            "新建变更列表…".to_string()
        } else {
            "New Changelist…".to_string()
        },
        8,
        false,
        true,
    ));
    (items, ctx)
}

fn discardable_paths(ctx: &ChangeMenuCtx) -> Vec<String> {
    // The untracked branch has its own Delete-to-trash confirmation. A mixed
    // selection must not silently delete an untracked file through Rollback.
    ctx.paths
        .iter()
        .filter(|path| !ctx.untracked.contains(*path))
        .cloned()
        .collect()
}

/// 变更菜单动作分发（文件行与组头菜单共用；返回是否应关闭菜单）。
fn apply_change_menu_action(
    ui: &Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    ctx: &ChangeMenuCtx,
    code: u8,
) -> bool {
    let project_idx = model.active_project.unwrap_or(0);
    match code {
        0 => model.queue(Command::OpenDiff(ctx.first.clone())),
        1 => {
            // Jump to Source：在编辑器打开（仓库相对 → 绝对路径）。
            let abs = ctx.project_root.join(&ctx.first);
            if let Err(err) = model.open_file_in(project_idx, abs) {
                model.toast(err, ToastKind::Error);
            }
        }
        2 => {
            if ctx.conflicts.iter().any(|entry| entry.path == ctx.first) {
                if let Ok(content) = lazecode_git::merge_file_content(&ctx.project_root, &ctx.first)
                {
                    rt.dialog = DialogState::ConflictResolve {
                        path: ctx.first.clone(),
                        ours: content.ours,
                        theirs: content.theirs,
                        worktree: content.worktree,
                    };
                }
            }
        }
        3 => {
            rt.dialog = DialogState::CommitFiles {
                paths: ctx.paths.clone(),
                value: String::new(),
                error: None,
            };
        }
        4 => {
            // 统一确认弹窗（对齐稳定版 rollbackFileConfirm）。Mixed
            // selections must not delete untracked files via Rollback.
            let paths = discardable_paths(ctx);
            if paths.is_empty() {
                model.toast(
                    crate::tr!(
                        model,
                        "Use Delete for untracked files",
                        "未跟踪文件请使用「删除」"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            } else {
                rt.dialog = DialogState::ConfirmDiscard { paths };
            }
        }
        5 => {
            // Copy as Patch：HEAD→工作区补丁（含未跟踪文件）。
            match lazecode_git::diff_patch_paths(&ctx.project_root, &ctx.paths) {
                Ok(patch) => {
                    ui.ctx().copy_text(patch);
                    model.toast(
                        crate::tr!(model, "Patch copied to clipboard", "已将补丁复制到剪贴板")
                            .to_string(),
                        ToastKind::Info,
                    );
                }
                Err(err) => model.toast(err.to_string(), ToastKind::Error),
            }
        }
        6 => {
            // Shelve Changes...：架藏所选文件，默认名对齐稳定版。
            let default_name = if ctx.single {
                ctx.display_name.clone()
            } else {
                "Default".to_string()
            };
            rt.dialog = DialogState::NewShelf {
                value: default_name,
                error: None,
                paths: ctx.paths.clone(),
            };
        }
        7 => model.queue(Command::ReloadGit),
        8 => {
            rt.dialog = DialogState::NewChangelist {
                value: String::new(),
                error: None,
            };
        }
        20 => {
            // Add to VCS：git add 走后台任务，回填后刷新快照。
            model.queue(Command::AddToVcs(ctx.paths.clone()));
        }
        21 => {
            // 未跟踪文件 Delete…：全部移入废纸篓（稳定版 fs_delete 语义）。
            if ctx.single {
                rt.dialog = DialogState::DeletePath {
                    path: ctx.project_root.join(&ctx.first),
                    is_dir: false,
                };
            } else {
                rt.dialog = DialogState::ConfirmDeleteUntracked {
                    paths: ctx.paths.iter().map(|p| ctx.project_root.join(p)).collect(),
                };
            }
        }
        22 => {
            // Compare Before with Local：HEAD ↔ 工作区。
            model.queue(Command::OpenCompareWithLocalDiff {
                commit: "HEAD".to_string(),
                path: ctx.first.clone(),
            });
        }
        23 => {
            // Create Patch from Local Changes：先弹保存对话框再后台写文件。
            model.queue(Command::SavePatch(ctx.paths.clone()));
        }
        24 => {
            rt.dialog = DialogState::FileHistory {
                path: ctx.first.clone(),
                entries: Vec::new(),
                selected: 0,
                loaded: false,
            };
        }
        32 => model.queue(Command::OpenRepositoryVersion {
            path: ctx.first.clone(),
        }),
        30 => model.queue(Command::OpenDiffNewTab(ctx.first.clone())),
        31 => model.queue(Command::OpenIndexDiff(ctx.first.clone())),
        33 => {
            rt.dialog = DialogState::CherryPickChanges {
                paths: ctx.paths.clone(),
                target: String::new(),
                error: None,
            };
        }
        34 => {
            rt.dialog = DialogState::ExtractChanges {
                paths: ctx.paths.clone(),
                value: String::new(),
                error: None,
            };
        }
        35 => {
            let paths = discardable_paths(ctx);
            if paths.is_empty() {
                model.toast(
                    crate::tr!(
                        model,
                        "Use Delete for untracked files",
                        "未跟踪文件请使用「删除」"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            } else {
                rt.dialog = DialogState::ConfirmDiscard { paths };
            }
        }
        36 => {
            rt.dialog = DialogState::GetFileFromRevision {
                path: ctx.first.clone(),
                revision: "HEAD".to_string(),
                error: None,
            };
        }
        37 => {
            let project = &model.projects[project_idx];
            let head_id = project
                .git
                .as_ref()
                .and_then(|git| git.head.as_ref())
                .map(|head| head.id.as_str());
            let head_row = head_id.and_then(|id| project.log.iter().find(|commit| commit.id == id));
            let parent = match head_row {
                Some(row) => row.parents.first().cloned(),
                // Log not loaded/filtered: let git resolve HEAD^ itself.
                None => Some("HEAD^".to_string()),
            };
            if let Some(parent) = parent {
                model.queue(Command::OpenCompareWithLocalDiff {
                    commit: parent,
                    path: ctx.first.clone(),
                });
            } else {
                model.toast(
                    crate::tr!(
                        model,
                        "The current branch has no parent commit",
                        "当前分支没有父提交"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            }
        }
        idx if (50..=200).contains(&idx) => {
            let target_idx = (idx - 50) as usize;
            if let Some(target_list) = ctx.all_lists.get(target_idx) {
                model.queue(Command::ChangelistAssign {
                    paths: ctx.paths.clone(),
                    list: target_list.clone(),
                });
            }
        }
        _ => {}
    }
    true
}

fn render_change_menu_items(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    row_start: f32,
    id_salt: &str,
    items: &[ChangeMenuItem],
) -> Option<u8> {
    let p = theme.palette;
    let mut action: Option<u8> = None;
    for (i, item) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if item.code == CHANGE_MENU_SEP {
            ui.painter().line_segment(
                [
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Pos2::new(row.right() - 8.0, row.center().y),
                ],
                egui::Stroke::new(1.0, p.menu_border),
            );
            continue;
        }
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with((id_salt, i)),
            &item.label,
            item.danger,
            item.enabled,
        ) {
            action = Some(item.code);
        }
    }
    action
}

#[allow(clippy::too_many_arguments)]
/// 提交抽屉 ⋮ 扩展入口：稳定版抽屉没有这些按钮，保留功能入口
/// （Stash Changes… / Stash Manager…）。
fn draw_commit_extras_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let items = [
        (crate::tr!(model, "Stash Changes…", "贮藏更改…"), 0u8, false),
        (crate::tr!(model, "Stash Manager…", "贮藏管理器…"), 1, false),
        (crate::tr!(model, "Shelf Manager…", "架藏管理器…"), 2, false),
        (
            crate::tr!(model, "Import into Shelf…", "导入架藏…"),
            3,
            false,
        ),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("cextras", i)),
            label,
            *danger,
            true,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        Some(0) => {
            let has_untracked = model
                .active_project()
                .is_some_and(|project| project.changes().iter().any(|change| change.untracked));
            rt.dialog = crate::runtime::DialogState::StashCreate {
                message: String::new(),
                include_untracked: has_untracked,
                error: None,
            };
            close = true;
        }
        Some(1) => {
            rt.dialog = crate::runtime::DialogState::StashManager {
                stashes: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
            close = true;
        }
        Some(2) => {
            rt.dialog = crate::runtime::DialogState::ShelfManager {
                shelves: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
            close = true;
        }
        Some(3) => {
            #[cfg(target_os = "macos")]
            {
                if let Ok(output) = std::process::Command::new("osascript")
                    .arg("-e")
                    .arg("POSIX path of (choose file with prompt \"Select patch file to import into Shelf\")")
                    .output()
                {
                    if output.status.success() {
                        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !path_str.is_empty() {
                            let root = model
                                .active_project()
                                .map(|p| p.active_root().to_path_buf())
                                .unwrap_or_default();
                            match lazecode_git::shelf_import_patch(&root, std::path::Path::new(&path_str), None) {
                                Ok(name) => {
                                    model.toast(
                                        if model.settings.language.is_zh() {
                                            format!("已导入架藏: {name}")
                                        } else {
                                            format!("Imported shelf: {name}")
                                        },
                                        lazecode_core::ToastKind::Success,
                                    );
                                }
                                Err(err) => model.toast(err.to_string(), lazecode_core::ToastKind::Error),
                            }
                        }
                    }
                }
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}
/// 贮藏条目右键菜单：弹出 / 应用 / Unstash… / 丢弃 / 清空 / 显示差异。
#[allow(clippy::too_many_arguments)]
fn draw_stash_entry_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: Option<&str>,
) {
    let p = theme.palette;
    let stash = payload
        .map(str::to_string)
        .or_else(|| rt.stash_selected.clone());
    let message = stash
        .as_ref()
        .and_then(|stash| {
            rt.stash_entries
                .iter()
                .find(|entry| &entry.name == stash)
                .map(|entry| entry.message())
        })
        .unwrap_or_else(|| stash.clone().unwrap_or_default());
    // 显示差异需要一个文件：优先用该贮藏下当前选中的文件，否则第一条。
    let diff_target = stash.as_ref().and_then(|stash| {
        let paths = model.projects.get_mut(project_idx)?.stash_paths_for(stash);
        rt.stash_file_sel
            .as_ref()
            .filter(|path| paths.iter().any(|known| known == *path))
            .cloned()
            .or_else(|| paths.first().cloned())
    });
    let branch = model
        .projects
        .get(project_idx)
        .map(|project| project.branch())
        .unwrap_or_default();

    // (label, code, danger, enabled)
    let items: [(&str, u8, bool, bool); 7] = [
        (crate::tr!(model, "Pop", "弹出"), 0, false, stash.is_some()),
        (
            crate::tr!(model, "Apply", "应用"),
            1,
            false,
            stash.is_some(),
        ),
        (
            crate::tr!(model, "Unstash…", "Unstash…"),
            2,
            false,
            stash.is_some(),
        ),
        (crate::tr!(model, "Drop", "丢弃"), 3, true, stash.is_some()),
        (crate::tr!(model, "Clear", "清空"), 4, true, true),
        (
            crate::tr!(model, "Show Diff", "显示差异"),
            5,
            false,
            diff_target.is_some(),
        ),
        (
            crate::tr!(model, "Show Diff in a New Tab", "在新标签页显示差异"),
            6,
            false,
            diff_target.is_some(),
        ),
    ];
    let sep_after = 4usize;
    let height = items.len() as f32 * MENU_ROW_H + 12.0;
    let rect = popover_rect(rt, screen, menu.anchor, 230.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    let mut y = rect.top() + 4.0;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        if i == sep_after {
            ui.painter().hline(
                (rect.left() + 8.0)..=(rect.right() - 8.0),
                y + 3.0,
                Stroke::new(1.0, p.border_soft),
            );
            y += 6.0;
        }
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, y),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("stash-entry", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
        y += MENU_ROW_H;
    }
    let mut close = false;
    match picked {
        Some(0) => {
            if let Some(stash) = stash {
                model.queue(Command::StashPop(stash));
            }
            close = true;
        }
        Some(1) => {
            if let Some(stash) = stash {
                model.queue(Command::StashApply(stash));
            }
            close = true;
        }
        Some(2) => {
            if let Some(stash) = stash {
                rt.dialog = crate::runtime::DialogState::Unstash {
                    stash,
                    message,
                    branch,
                    as_new_branch: String::new(),
                    pop: false,
                    reinstate_index: false,
                    error: None,
                };
            }
            close = true;
        }
        Some(3) => {
            if let Some(stash) = stash {
                model.queue(Command::StashDrop(stash));
            }
            close = true;
        }
        Some(4) => {
            model.queue(Command::StashClear);
            close = true;
        }
        Some(5) => {
            if let (Some(stash), Some(path)) = (stash, diff_target) {
                model.queue(Command::OpenStashDiff { stash, path });
            }
            close = true;
        }
        Some(6) => {
            if let (Some(stash), Some(path)) = (stash, diff_target) {
                model.queue(Command::OpenStashDiffNewTab { stash, path });
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 贮藏文件 / 目录右键菜单（对齐参考实现）：
/// Show Diff · Show Diff in a New Tab · Compare Before with Local ·
/// Edit Source · Open Repository Version · Cherry-Pick Selected Changes。
#[allow(clippy::too_many_arguments)]
fn draw_stash_file_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let mut segments = payload.split('\u{1}');
    let Some(stash) = segments.next().filter(|stash| !stash.is_empty()) else {
        return;
    };
    let kind = segments.next().unwrap_or("f");
    let path = segments.next().unwrap_or("").to_string();
    let stash = stash.to_string();

    // 目录 / 树根展开为该范围内的文件清单；文件就是它自己。
    let mut files: Vec<String> = Vec::new();
    if let Some(details) = model
        .projects
        .get_mut(project_idx)
        .and_then(|project| project.stash_details_for(&stash))
    {
        match kind {
            "t" => files.extend(details.tracked.iter().cloned()),
            "u" => files.extend(details.untracked.iter().cloned()),
            "d" => {
                let prefix = if path.is_empty() {
                    String::new()
                } else {
                    format!("{path}/")
                };
                files.extend(
                    details
                        .tracked
                        .iter()
                        .chain(details.untracked.iter())
                        .filter(|file| !prefix.is_empty() && file.starts_with(&prefix))
                        .cloned(),
                );
            }
            _ => {}
        }
    }
    if kind == "f" && !path.is_empty() {
        files = vec![path.clone()];
    }
    let target = files.first().cloned();
    let worktree_target = target.as_ref().and_then(|target| {
        model
            .active_project()
            .map(|project| project.active_root().join(target))
    });
    let target_exists = worktree_target.as_ref().is_some_and(|path| path.is_file());

    // (label, code, danger, enabled)
    let items: [(&str, u8, bool, bool); 6] = [
        (
            crate::tr!(model, "Show Diff", "显示差异"),
            0,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Show Diff in a New Tab", "在新标签页显示差异"),
            1,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Compare Before with Local", "与本地的前一版本比较"),
            2,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Edit Source", "编辑源文件"),
            3,
            false,
            target_exists,
        ),
        (
            crate::tr!(model, "Open Repository Version", "打开仓库版本"),
            4,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Cherry-Pick Selected Changes", "拣选选中更改"),
            5,
            false,
            !files.is_empty(),
        ),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 250.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("stash-file", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        Some(0) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashDiff { stash, path });
            }
            close = true;
        }
        Some(1) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashDiffNewTab { stash, path });
            }
            close = true;
        }
        Some(2) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashBeforeLocalDiff { stash, path });
            }
            close = true;
        }
        Some(3) => {
            if let Some(path) = worktree_target {
                model.queue(Command::OpenFileAt {
                    path,
                    line: 0,
                    character: 0,
                });
            }
            close = true;
        }
        Some(4) => {
            if let Some(path) = target {
                model.queue(Command::OpenRepositoryVersion { path });
            }
            close = true;
        }
        Some(5) => {
            if !files.is_empty() {
                model.queue(Command::CherryPickStashPaths {
                    stash,
                    paths: files,
                });
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn draw_commit_view_options_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let menu_w = 210.0;
    let menu_h = 100.0;
    let rect = popover_rect(rt, screen, menu.anchor, menu_w, menu_h);
    menu_frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Group By", "分组依据"),
        FontId::proportional(10.5),
        p.text3,
    );

    let item_tree = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 22.0),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    let mut close = false;
    if menu_row(
        ui,
        theme,
        item_tree,
        ui.scope_id().with("commit-view-tree"),
        &*crate::tr!(model, "Directory", "目录"),
        false,
        true,
    ) {
        rt.commit_tree_view = !rt.commit_tree_view;
        close = true;
    }
    if rt.commit_tree_view {
        ui.painter().text(
            Pos2::new(item_tree.right() - 34.0, item_tree.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }
    ui.painter().text(
        Pos2::new(item_tree.right() - 10.0, item_tree.center().y),
        Align2::RIGHT_CENTER,
        "^P",
        FontId::proportional(10.5),
        p.text3,
    );

    ui.painter().hline(
        (rect.left() + 6.0)..=(rect.right() - 6.0),
        rect.top() + 52.0,
        Stroke::new(1.0, p.border_soft),
    );

    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.top() + 62.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Show", "显示"),
        FontId::proportional(10.5),
        p.text3,
    );

    let item_ignored = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 70.0),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    if menu_row(
        ui,
        theme,
        item_ignored,
        ui.scope_id().with("commit-view-ignored"),
        &*crate::tr!(model, "Show Ignored Files", "显示被忽略的文件"),
        false,
        true,
    ) {
        rt.commit_show_ignored = !rt.commit_show_ignored;
        close = true;
    }
    if rt.commit_show_ignored {
        ui.painter().text(
            Pos2::new(item_ignored.right() - 12.0, item_ignored.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}
fn draw_compare_view_options_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let menu_w = 180.0;
    let menu_h = 56.0;
    let rect = popover_rect(rt, screen, menu.anchor, menu_w, menu_h);
    menu_frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.top() + 11.0),
        Align2::LEFT_CENTER,
        if zh { "分组方式" } else { "Group by" },
        FontId::proportional(10.5),
        p.text3,
    );

    let item = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 20.0),
        Vec2::new(menu_w - 8.0, 24.0),
    );
    let mut close = false;
    let is_grouped = model
        .projects
        .get(project_idx)
        .and_then(|p| p.compare_session.as_ref())
        .map(|s| s.group_by_dir)
        .unwrap_or(false);
    if menu_row(
        ui,
        theme,
        item,
        ui.scope_id().with("cmp-view-dir"),
        if zh { "目录" } else { "Directory" },
        false,
        true,
    ) {
        if let Some(session) = model
            .projects
            .get_mut(project_idx)
            .and_then(|p| p.compare_session.as_mut())
        {
            session.group_by_dir = !session.group_by_dir;
        }
        close = true;
    }
    if is_grouped {
        ui.painter().text(
            Pos2::new(item.left() + 10.0, item.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }
    ui.painter().text(
        Pos2::new(item.right() - 8.0, item.center().y),
        Align2::RIGHT_CENTER,
        "^P",
        FontId::proportional(10.5),
        p.text3,
    );

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 架藏右键菜单（稳定版 Shelf 页签）：恢复 / 补丁 / 重命名 / 删除。
#[allow(clippy::too_many_arguments)]
fn draw_shelf_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let mut segments = payload.split('\u{1}');
    let Some(name) = segments.next().filter(|name| !name.is_empty()) else {
        return;
    };
    let name = name.to_string();
    let deleted = segments.next() == Some("1");
    let paths: Vec<String> = segments
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let _ = project_idx;

    // (label, code, danger, enabled)
    let mut items: Vec<(String, u8, bool, bool)> = Vec::new();
    if deleted {
        items.push((
            crate::tr!(model, "Restore", "恢复").to_string(),
            0,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Delete…", "删除…").to_string(),
            3,
            true,
            true,
        ));
    } else {
        items.push((
            crate::tr!(model, "Unshelve…", "恢复贮藏…").to_string(),
            1,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Copy as Patch to Clipboard", "以补丁复制到剪贴板").to_string(),
            2,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Rename…", "重命名…").to_string(),
            4,
            false,
            paths.is_empty(),
        ));
        items.push((
            crate::tr!(model, "Delete…", "删除…").to_string(),
            3,
            true,
            true,
        ));
    }
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("shelf-ctx", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        // Restore（最近删除 → 恢复）
        Some(0) => {
            model.queue(Command::ShelfRestoreDeleted(name.clone()));
            rt.shelf_loaded = None;
            close = true;
        }
        // Unshelve…
        Some(1) => {
            {
                let files: Vec<(String, bool)> =
                    paths.iter().map(|path| (path.clone(), true)).collect();
                rt.dialog = crate::runtime::DialogState::Unshelve {
                    name: name.clone(),
                    files,
                    target: None,
                    new_name: name.clone(),
                    error: None,
                };
            }
            close = true;
        }
        // Copy as Patch（补丁读取是本地小文件，同步可接受）
        Some(2) => {
            if let Some(project) = model.projects.get(model.active_project.unwrap_or(0)) {
                match lazecode_git::shelf_patch(project.active_root(), &name) {
                    Ok(patch) => {
                        ui.ctx().copy_text(patch);
                        let zh = model.settings.language.is_zh();
                        model.toast(
                            if zh {
                                "补丁已复制到剪贴板"
                            } else {
                                "Patch copied to clipboard"
                            },
                            ToastKind::Success,
                        );
                    }
                    Err(err) => model.toast(err.to_string(), ToastKind::Error),
                }
            }
            close = true;
        }
        // Delete…（确认）
        Some(3) => {
            rt.dialog = crate::runtime::DialogState::ConfirmDeleteShelf {
                name: name.clone(),
                purge: deleted,
            };
            close = true;
        }
        // Rename…
        Some(4) => {
            rt.dialog = crate::runtime::DialogState::RenameShelf {
                from: name.clone(),
                value: name.clone(),
                error: None,
            };
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_change_file_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let Some(project) = model.projects.get(project_idx) else {
        return;
    };
    // payload: 多选时以 \u{1} 连接的仓库相对路径。
    let paths: Vec<String> = payload
        .split('\u{1}')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    if paths.is_empty() {
        return;
    }
    let (items, ctx) = change_menu_items(model, project, &paths);

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行，直接列菜单项。
    let mut close = false;
    if let Some(code) =
        render_change_menu_items(ui, theme, rect, rect.top() + 8.0, "cfile_ctx", &items)
    {
        close = apply_change_menu_action(ui, model, rt, &ctx, code);
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn draw_agent_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    anchor: Rect,
) {
    let p = theme.palette;
    let options = model.settings.agents.options.clone();
    let visible_rows = options.len().min(9);
    let height = 52.0 + visible_rows as f32 * 46.0 + 24.0;
    let rect = popover_rect(rt, screen, anchor, 320.0, height);
    menu_frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Start agent", "启动 agent"),
        FontId::proportional(11.0),
        p.text3,
    );
    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 34.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 10.0),
    );
    let mut launch: Option<(String, String)> = None;
    let mut close = false;
    crate::app_ui::region(ui, rows, "agent-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("agent-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for option in &options {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(5), p.hover);
                    }
                    let icon_rect = Rect::from_center_size(
                        Pos2::new(row.left() + 20.0, row.center().y),
                        Vec2::splat(24.0),
                    );
                    ui.painter()
                        .rect_filled(icon_rect, egui::CornerRadius::same(6), p.accent_soft);
                    paint_icon(ui.painter(), icon_rect.shrink(5.0), Icon::Spark, p.accent);
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.top() + 5.0),
                            Pos2::new(row.right() - 12.0, row.center().y + 3.0),
                        ),
                        &option.name,
                        FontId::proportional(11.5),
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    let description = if option.description.is_empty() {
                        if option.command.is_empty() {
                            crate::tr!(model, "Start a plain login shell", "启动登录 Shell")
                                .to_string()
                        } else {
                            format!("Run {}", option.command)
                        }
                    } else {
                        option.description.clone()
                    };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.center().y + 3.0),
                            Pos2::new(row.right() - 12.0, row.bottom() - 3.0),
                        ),
                        &description,
                        FontId::proportional(9.5),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    if !option.command.is_empty() {
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row.right() - 120.0, row.top()),
                                Pos2::new(row.right() - 12.0, row.center().y - 2.0),
                            ),
                            &option.command,
                            FontId::monospace(8.5),
                            p.text3,
                            Align2::RIGHT_CENTER,
                        );
                    }
                    if response.clicked() {
                        launch = Some((option.name.clone(), option.command.clone()));
                        close = true;
                    }
                }
            });
    });
    if let Some((title, command)) = launch {
        if command.is_empty() {
            model.queue(Command::NewTerminal);
        } else {
            model.queue(Command::NewTerminalCommand { title, command });
        }
    }
    if should_dismiss(ui, rect, rt.agent_menu_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.agent_menu = Some(anchor);
    }
}

#[allow(clippy::too_many_arguments)]
/// User 过滤菜单：本侧出现过的作者（首见序）。
fn draw_compare_user_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut authors = crate::compare::unique_authors(&commits);
    authors.insert(0, crate::tr!(model, "All authors", "全部作者").to_string());
    let height = (authors.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, author) in authors.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbuser", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            author,
            FontId::proportional(11.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 { None } else { Some(author.clone()) });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.author = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Date 过滤菜单：All / Today / Last 7 days / Last 30 days。
fn draw_compare_date_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let items: Vec<(String, Option<lazecode_core::CompareDateFilter>)> = vec![
        (crate::tr!(model, "All", "全部").to_string(), None),
        (
            crate::tr!(model, "Today", "今天").to_string(),
            Some(lazecode_core::CompareDateFilter::Today),
        ),
        (
            crate::tr!(model, "Last 7 days", "最近 7 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last7Days),
        ),
        (
            crate::tr!(model, "Last 30 days", "最近 30 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last30Days),
        ),
    ];
    let height = items.len() as f32 * 24.0 + 20.0;
    let rect = popover_rect(rt, screen, menu.anchor, 160.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<lazecode_core::CompareDateFilter>> = None;
    for (i, (label, value)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbdate", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            p.text,
        );
        if response.clicked() {
            picked = Some(*value);
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.date = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Branch(ref) 过滤菜单：本侧提交所属的引用名（首见序）。
fn draw_compare_ref_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut refs = crate::compare::unique_refs(&commits);
    refs.insert(0, crate::tr!(model, "All Branches", "全部分支").to_string());
    let height = (refs.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, name) in refs.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbref", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            name,
            FontId::monospace(10.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 { None } else { Some(name.clone()) });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.selected_ref_filter = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Draw the open context menu, if any. Called by `overlays::draw`.
pub(super) fn draw_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
) {
    if let Some(menu) = rt.menu.take() {
        match menu.kind {
            MenuKind::Project => draw_project_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Branch => draw_branch_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Worktree => draw_worktree_menu(ui, model, rt, theme, screen, menu),
            MenuKind::LogBranchFilter { project } => {
                draw_log_branch_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogDateFilter { project } => {
                draw_log_date_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogCommitMenu { project } => {
                let payload = menu.payload.clone();
                draw_commit_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::BranchContext { project } => {
                let payload = menu.payload.clone();
                draw_branch_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::TagContext { project } => {
                let payload = menu.payload.clone();
                draw_tag_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::TreeContext { project, is_dir } => {
                let payload = menu.payload.clone();
                draw_tree_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                    is_dir,
                )
            }
            MenuKind::EditorTabContext { project, tab_id } => {
                let payload = menu.payload.clone();
                draw_editor_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    tab_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::TerminalTabContext {
                project,
                tab_id,
                terminal_id,
            } => {
                let payload = menu.payload.clone();
                draw_terminal_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    tab_id,
                    terminal_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangelistHeaderContext { project } => {
                let payload = menu.payload.clone();
                draw_changelist_header_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangeFileContext { project } => {
                let payload = menu.payload.clone();
                draw_change_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitHistory { project } => {
                draw_commit_history_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::CommitExtras => {
                draw_commit_extras_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::StashEntryMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_entry_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                );
            }
            MenuKind::StashFileMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitViewOptions => {
                draw_commit_view_options_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::CompareViewOptions { project } => {
                draw_compare_view_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::ShelfContext { project } => {
                let payload = menu.payload.clone();
                draw_shelf_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CompareUserFilter { side } => {
                draw_compare_user_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareDateFilter { side } => {
                draw_compare_date_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareRefFilter { side } => {
                draw_compare_ref_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::LogSortOptions { project } => {
                draw_log_sort_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::LogPathFilter { project } => {
                let payload = menu.payload.clone();
                draw_log_path_filter_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitDetailsFileContext { project } => {
                let payload = menu.payload.clone();
                draw_commit_details_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::EditorContext { project } => {
                let payload = menu.payload.clone();
                draw_editor_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
        }
    }
    if let Some(anchor) = rt.agent_menu.take() {
        draw_agent_menu(ui, model, rt, theme, screen, anchor);
    }
}
