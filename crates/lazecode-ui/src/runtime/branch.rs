//! Branch submenu state and the commit drawer tab enum.

#[derive(Clone, Debug, PartialEq)]
pub struct BranchSubmenuState {
    pub branch_name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub anchor: egui::Rect,
}

/// CommitDrawer 顶部页签（稳定版 tabCommit / tabShelf + 贮藏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommitTab {
    Commit,
    Shelf,
    /// 仅在仓库存在 stash 记录时显示。
    Stash,
}
