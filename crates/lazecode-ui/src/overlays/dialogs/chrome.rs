//! Static chrome for every dialog variant: title, subtitle and frame size.
//!
//! Kept apart from the layout code so adding a dialog means editing one
//! `tr!` table row here plus its body, instead of threading strings through
//! the 3k-line dispatcher.

use lazecode_core::AppModel;

use crate::runtime::DialogState;

/// Title, subtitle and frame size for `dialog`, or `None` when the variant
/// is drawn by its own dedicated window (managers, conflict tool) or there is
/// no dialog at all.
pub(super) fn dialog_chrome(
    model: &AppModel,
    dialog: &DialogState,
) -> Option<(String, String, f32, f32)> {
    let unstash_text = if let DialogState::Unstash { message, .. } = dialog {
        let name = if message.trim().is_empty() {
            crate::tr!(model, "stash", "贮藏").to_string()
        } else {
            message.clone()
        };
        Some((
            crate::tr!(
                model,
                "Unstash Changes in {name}…",
                "Unstash Changes in {name}…"
            )
            .replace("{name}", &name),
            crate::tr!(
                model,
                "Apply the stashed changes to the current branch or a new one.",
                "将贮藏的更改应用到当前分支或新分支。"
            )
            .to_string(),
        ))
    } else {
        None
    };
    let (title, subtitle, width, height) = match dialog {
        DialogState::NewBranch { .. } => (
            crate::tr!(model, "New branch", "新建分支"),
            crate::tr!(model, "Create a branch at the selected start point. Checkout is optional.", "从选定起点创建分支，可选创建后检出。"),
            460.0,
            290.0,
        ),
        DialogState::NewWorktree { .. } => (
            crate::tr!(model, "New worktree", "新建工作树"),
            crate::tr!(
                model,
                "Creates a linked worktree. One branch can be checked out in only one worktree; create a new branch or pick an unused one.",
                "创建关联的工作树。同一分支只能在一个工作树中检出；可新建分支或选择未使用的分支。"
            ),
            520.0,
            370.0,
        ),
        DialogState::CheckoutRevision { .. } => (
            crate::tr!(model, "Checkout tag or revision", "检出标签或修订版本"),
            crate::tr!(
                model,
                "Enter a branch, tag, or commit hash to check out.",
                "输入要检出的分支、标签或提交哈希。"
            ),
            460.0,
            230.0,
        ),
        DialogState::RemoveWorktree { dirty: false, .. } => (
            crate::tr!(model, "Remove worktree", "删除工作树"),
            crate::tr!(
                model,
                "The directories of the worktree and its consoles are removed.",
                "将删除工作树目录及其控制台。"
            ),
            470.0,
            210.0,
        ),
        DialogState::RemoveWorktree { dirty: true, .. } => (
            crate::tr!(model, "Remove worktree with changes", "删除有更改的工作树"),
            crate::tr!(
                model,
                "This worktree has uncommitted changes. They will be permanently discarded.",
                "此工作树有未提交的更改，删除后将永久丢失。"
            ),
            470.0,
            230.0,
        ),
        DialogState::DeleteBranch { remote, force, .. } => {
            if *remote {
                (
                    crate::tr!(model, "Delete remote branch", "删除远程分支"),
                    crate::tr!(model, "The remote branch is deleted on the remote. The local branch is not touched.", "将在远程删除该分支，不会影响本地分支。"),
                    460.0,
                    220.0,
                )
            } else if *force {
                (
                    crate::tr!(model, "Force delete branch", "强制删除分支"),
                    crate::tr!(model, "This branch is not fully merged. Confirm explicitly to delete its reference.", "该分支尚未合并。确认后将强制删除其引用。"),
                    480.0,
                    230.0,
                )
            } else {
                (
                    crate::tr!(model, "Delete branch", "删除分支"),
                    crate::tr!(model, "The branch and its reflog entry are removed. Commits stay reachable until garbage collection.", "将删除该分支及其 reflog 记录；提交在垃圾回收前仍可访问。"),
                    440.0,
                    210.0,
                )
            }
        }
        DialogState::RenameBranch { .. } => (
            crate::tr!(model, "Rename branch", "重命名分支"),
            crate::tr!(model, "The ref name of a local branch is rewritten in place.", "直接重命名本地分支的引用名。"),
            460.0,
            220.0,
        ),
        DialogState::Push { .. } => (
            crate::tr!(model, "Push", "推送"),
            crate::tr!(model, "Publish the branch and review commits before pushing. Force uses force-with-lease semantics.", "推送分支并在推送前检查提交；Force 使用 force-with-lease 语义。"),
            570.0,
            500.0,
        ),
        DialogState::Pull { .. } => (
            crate::tr!(model, "Pull", "拉取"),
            crate::tr!(model, "Fetch and integrate upstream changes (merge/rebase/ff-only).", "拉取并集成上游变更（merge/rebase/仅快进）。"),
            470.0,
            280.0,
        ),
        DialogState::Remotes { .. } => (
            crate::tr!(model, "Manage remotes", "管理远程"),
            crate::tr!(model, "Add, rename, re-point or delete remote repositories.", "添加、重命名、修改或删除远程仓库。"),
            520.0,
            380.0,
        ),
        DialogState::Reset { .. } => (
            crate::tr!(model, "Reset current branch", "重置当前分支"),
            crate::tr!(model, "Soft keeps changes, mixed unstages, hard discards both index and working tree.", "soft 保留更改，mixed 取消暂存，hard 连暂存和工作区一起丢弃。"),
            470.0,
            300.0,
        ),
        DialogState::NewTag { .. } => (
            crate::tr!(model, "New tag", "新建标签"),
            crate::tr!(model, "Tags mark a revision for later comparison or releasing.", "标签用于标记一个提交，以供后续比较或发布。"),
            460.0,
            240.0,
        ),
        DialogState::CloseDirtyTab { .. } => (
            crate::tr!(model, "Discard changes?", "放弃更改？"),
            crate::tr!(model, "This tab has unsaved changes. Closing it will discard them.", "此标签页有未保存的更改，关闭将丢弃它们。"),
            440.0,
            210.0,
        ),
        DialogState::ConfirmTerminalClose { action, .. } => (
            if matches!(action, crate::runtime::TerminalCloseAction::Project(_)) {
                crate::tr!(model, "Close project", "关闭项目")
            } else {
                crate::tr!(model, "Close terminal", "关闭控制台")
            },
            crate::tr!(
                model,
                "Commands are still running in this console. Closing it will terminate them.",
                "控制台里还有命令在运行，关闭会终止这些进程。"
            ),
            520.0,
            230.0,
        ),
        DialogState::Quit {
            restart_update, ..
        } => (
            if *restart_update {
                crate::tr!(model, "Restart to finish updating?", "重启以完成更新？")
            } else {
                crate::tr!(model, "Quit LazeCode?", "退出 LazeCode？")
            },
            if *restart_update {
                crate::tr!(model, "The new version is installed. Restarting closes all windows; save any open files first.", "新版本已安装。重启会关闭所有窗口，请先保存打开的文件。")
            } else {
                crate::tr!(model, "Tabs with unsaved changes will lose their edits unless you cancel and save them first.", "未保存的标签页将丢失更改，请取消并先保存。")
            },
            460.0,
            240.0,
        ),
        DialogState::NewFile { .. } => (
            crate::tr!(model, "New file", "新建文件"),
            crate::tr!(model, "Create a file in the selected directory.", "在选定目录下创建文件。"),
            440.0,
            220.0,
        ),
        DialogState::NewDirectory { .. } => (
            crate::tr!(model, "New directory", "新建文件夹"),
            crate::tr!(model, "Create a directory in the selected location.", "在选定位置创建文件夹。"),
            440.0,
            220.0,
        ),
        DialogState::RenamePath { .. } => (
            crate::tr!(model, "Rename", "重命名"),
            crate::tr!(model, "Enter a new name for the file or directory.", "输入文件或目录的新名称。"),
            440.0,
            220.0,
        ),
        DialogState::DeletePath { is_dir, .. } => (
            crate::tr!(model, "Confirm delete", "确认删除"),
            if *is_dir {
                crate::tr!(
                    model,
                    "Move this directory and all its contents to the trash?",
                    "将此目录及其所有内容移入系统废纸篓？"
                )
            } else {
                crate::tr!(
                    model,
                    "Move this file to the trash?",
                    "将此文件移入系统废纸篓？"
                )
            },
            440.0,
            210.0,
        ),
        DialogState::ConfirmDeleteUntracked { .. } => (
            crate::tr!(model, "Delete files", "删除文件"),
            // 问句保持静态文案；具体文件名与数量由下方路径预览呈现
            // （与 ConfirmDiscard 同一模式）。文案对齐稳定版
            // trashUnversionedConfirm 语义。
            crate::tr!(
                model,
                "Move the selected untracked files to the trash?",
                "将所选未跟踪文件移入系统废纸篓？"
            ),
            460.0,
            250.0,
        ),
        DialogState::ConfirmDeleteShelf { .. } => (
            crate::tr!(model, "Delete shelf", "删除架藏"),
            crate::tr!(
                model,
                "Delete this shelf? The shelved patch will be removed permanently.",
                "删除此架藏？架藏补丁将被永久移除。"
            ),
            440.0,
            210.0,
        ),
        DialogState::NewShelf { .. } => (
            crate::tr!(model, "Shelve changes", "架藏更改"),
            crate::tr!(
                model,
                "Save uncommitted changes into a shelf patch file.",
                "将未提交的更改保存到补丁文件中。"
            ),
            440.0,
            230.0,
        ),
        DialogState::GoToLine { .. } => (
            crate::tr!(model, "Go to line", "跳转到行"),
            crate::tr!(
                model,
                "Enter a line number to jump to in the active document.",
                "输入要跳转到的行号。"
            ),
            380.0,
            210.0,
        ),
        DialogState::RenameSymbol { .. } => (
            crate::tr!(model, "Rename symbol", "重命名符号"),
            crate::tr!(
                model,
                "Rename this symbol and every reference through the language server.",
                "通过语言服务器重命名该符号及其全部引用。"
            ),
            440.0,
            210.0,
        ),
        DialogState::RenameTerminal { .. } => (
            crate::tr!(model, "Rename console tab", "重命名控制台标签"),
            crate::tr!(
                model,
                "Enter a new title for this console session.",
                "输入此控制台会话的新标题。"
            ),
            420.0,
            210.0,
        ),
        DialogState::NewChangelist { .. } => (
            crate::tr!(model, "New changelist", "新建变更列表"),
            crate::tr!(
                model,
                "Create a separate changelist to organize changes.",
                "创建一个独立的变更列表以归类更改。"
            ),
            420.0,
            210.0,
        ),
        DialogState::StashCreate { .. } => (
            crate::tr!(model, "Stash Changes", "贮藏更改"),
            crate::tr!(
                model,
                "Stash the working tree changes with an optional description.",
                "将工作区更改贮藏入栈，可附说明。"
            ),
            460.0,
            280.0,
        ),
        DialogState::ConfirmDiscard { .. } => (
            crate::tr!(model, "Rollback changes", "回滚更改"),
            crate::tr!(
                model,
                "Discard the uncommitted changes of the selected files? This cannot be undone.",
                "放弃所选文件的未提交更改？此操作不可撤销。"
            ),
            460.0,
            250.0,
        ),
        DialogState::DeleteChangelist { .. } => (
            crate::tr!(model, "Delete Changelist", "删除变更列表"),
            crate::tr!(
                model,
                "Choose what happens to the files of the deleted changelist.",
                "选择被删除变更列表中文件的去向。"
            ),
            480.0,
            250.0,
        ),
        DialogState::Unshelve { .. } => (
            crate::tr!(model, "Unshelve", "恢复架藏"),
            crate::tr!(
                model,
                "Restore the selected files to the working tree and put them into a changelist.",
                "恢复所选文件到工作区，并放入变更列表。"
            ),
            500.0,
            400.0,
        ),
        DialogState::CommitFiles { .. } => (
            crate::tr!(model, "Commit selected files", "提交所选文件"),
            crate::tr!(
                model,
                "Commit only the chosen files with their own message.",
                "仅提交所选文件，并使用独立的提交信息。"
            ),
            480.0,
            260.0,
        ),
        DialogState::GetFileFromRevision { .. } => (
            crate::tr!(model, "Get from revision", "从版本获取"),
            crate::tr!(
                model,
                "Restore the path from a branch, tag, or commit hash.",
                "从分支、标签或提交哈希恢复该路径。"
            ),
            500.0,
            230.0,
        ),
        DialogState::ExtractChanges { .. } => (
            crate::tr!(
                model,
                "Extract selected changes to a separate commit",
                "提取选中更改到独立提交"
            ),
            crate::tr!(
                model,
                "Commit only these changes; everything else stays uncommitted.",
                "仅提交这些更改，其他更改保持未提交。"
            ),
            500.0,
            290.0,
        ),
        DialogState::CherryPickChanges { .. } => (
            crate::tr!(model, "Cherry-pick selected changes", "拣选选中更改"),
            crate::tr!(
                model,
                "Move the selected working-tree changes onto the target branch.",
                "将选中的工作区更改移动到目标分支。"
            ),
            500.0,
            260.0,
        ),
        DialogState::Unstash { .. } => ("", "", 500.0, 360.0),
        _ => return None,
    };
    let (title, subtitle) = match unstash_text {
        Some((title, subtitle)) => (title, subtitle),
        None => (title.to_string(), subtitle.to_string()),
    };
    let subtitle = match dialog {
        DialogState::ConfirmTerminalClose {
            action, commands, ..
        } => {
            let names = commands.join(if model.settings.language.is_zh() {
                "、"
            } else {
                ", "
            });
            let project = matches!(action, crate::runtime::TerminalCloseAction::Project(_));
            if model.settings.language.is_zh() {
                if project {
                    format!(
                        "项目控制台里还有命令在运行（{names}），关闭项目会终止这些进程。确定关闭？"
                    )
                } else {
                    format!("控制台里还有命令在运行（{names}），关闭会终止这些进程。确定关闭？")
                }
            } else if project {
                format!(
                    "Commands are still running in this project's consoles ({names}). Closing the project will terminate them. Close anyway?"
                )
            } else {
                format!(
                    "Commands are still running in the terminal ({names}). Closing it will terminate them. Close anyway?"
                )
            }
        }
        DialogState::Quit {
            commands,
            restart_update,
        } => {
            let mut lines = Vec::new();
            if *restart_update {
                lines.push(if model.settings.language.is_zh() {
                    "更新将在重启后生效。".to_string()
                } else {
                    "The installed update takes effect after the restart.".to_string()
                });
            }
            if model
                .projects
                .iter()
                .any(|project| project.tabs.iter().any(|tab| tab.is_dirty()))
            {
                lines.push(if model.settings.language.is_zh() {
                    "未保存的标签页将丢失更改。".to_string()
                } else {
                    "Tabs with unsaved changes will lose their edits.".to_string()
                });
            }
            if !commands.is_empty() {
                let names = commands.join(if model.settings.language.is_zh() {
                    "、"
                } else {
                    ", "
                });
                lines.push(if model.settings.language.is_zh() {
                    format!("控制台里仍有命令在运行（{names}），退出会终止它们。")
                } else {
                    format!(
                        "Console commands are still running ({names}); quitting will terminate them."
                    )
                });
            }
            lines.join("\n")
        }
        _ => subtitle,
    };
    Some((title, subtitle, width, height))
}
