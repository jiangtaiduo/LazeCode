//! Terminal panes and tabs: open/split/close/restart and the pane lookup
//! helpers, as an inherent `ProjectState` impl.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use lazecode_terminal::{Notifier, Session, SessionKind, SpawnOptions};

use crate::settings::Settings;

use super::{
    PaneSplit, ProjectState, TerminalPane, TerminalTab, AGENT_COLD_BYTES, AGENT_SCROLLBACK_LINES,
    MAX_PANES_PER_TAB, MAX_TERMINALS_PER_WORKTREE, NEXT_TERMINAL,
};
impl ProjectState {
    /// Close every terminal tab except the one that owns `pane_id`.
    pub fn close_other_terminals(&mut self, pane_id: u64) {
        let wt = self.active_worktree_mut();
        let Some(tab_id) = wt.tab_of_pane(pane_id).map(|tab| tab.id) else {
            return;
        };
        wt.terminals.retain(|tab| tab.id == tab_id);
        if let Some(tab) = wt.tab_by_id_mut(tab_id) {
            tab.active_pane = pane_id;
        }
        wt.active_terminal = Some(pane_id);
    }

    pub fn close_all_terminals(&mut self) {
        let wt = self.active_worktree_mut();
        wt.terminals.clear();
        wt.active_terminal = None;
    }

    /// Pane view (`TerminalPane`) by id across every worktree.
    pub fn term_by_id(&self, id: u64) -> Option<&TerminalPane> {
        self.worktrees.iter().find_map(|wt| wt.pane(id))
    }

    pub fn term_by_id_mut(&mut self, id: u64) -> Option<&mut TerminalPane> {
        self.worktrees
            .iter_mut()
            .find_map(|wt| wt.terminals.iter_mut().find_map(|tab| tab.pane_mut(id)))
    }

    /// (worktree, tab, pane) indices owning `pane_id`.
    pub fn pane_owner(&self, id: u64) -> Option<(usize, usize, usize)> {
        for (wt_index, wt) in self.worktrees.iter().enumerate() {
            for (tab_index, tab) in wt.terminals.iter().enumerate() {
                if let Some(pane_index) = tab.panes.iter().position(|pane| pane.id == id) {
                    return Some((wt_index, tab_index, pane_index));
                }
            }
        }
        None
    }

    pub fn open_terminal(
        &mut self,
        command: Option<String>,
        title: Option<String>,
        kind: SessionKind,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<u64, String> {
        let cwd = if self.is_single_file {
            dirs::home_dir().unwrap_or_else(|| self.active_worktree().path.clone())
        } else {
            self.active_worktree().path.clone()
        };
        self.open_terminal_in(cwd, command, title, kind, settings, notifier)
    }

    /// used by every focus/close/restart call.
    pub fn open_terminal_in(
        &mut self,
        cwd: PathBuf,
        command: Option<String>,
        title: Option<String>,
        kind: SessionKind,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<u64, String> {
        if self.active_worktree().pane_count() >= MAX_TERMINALS_PER_WORKTREE {
            return Err(if settings.language.is_zh() {
                format!("此工作树最多同时打开 {MAX_TERMINALS_PER_WORKTREE} 个控制台")
            } else {
                format!("This worktree supports at most {MAX_TERMINALS_PER_WORKTREE} consoles")
            });
        }
        let session = spawn_pane(cwd, command.clone(), kind, settings, notifier)?;
        let tab_id = NEXT_TERMINAL.fetch_add(1, Ordering::Relaxed);
        let pane_id = NEXT_TERMINAL.fetch_add(1, Ordering::Relaxed);
        let title = title.unwrap_or_else(|| session.command_label.clone());
        let worktree = self.active_worktree_mut();
        worktree.terminals.push(TerminalTab {
            id: tab_id,
            panes: vec![TerminalPane {
                id: pane_id,
                session,
                title,
                kind,
                spawn_command: command,
                started: Instant::now(),
            }],
            active_pane: pane_id,
            split: PaneSplit::None,
        });
        worktree.active_terminal = Some(pane_id);
        Ok(pane_id)
    }

    /// Split the tab owning `pane_id`, adding a shell pane (max 4 per tab).
    pub fn split_terminal(
        &mut self,
        pane_id: u64,
        direction: PaneSplit,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<u64, String> {
        let Some((wt_index, tab_index, _)) = self.pane_owner(pane_id) else {
            return Err("terminal not found".into());
        };
        let wt = &self.worktrees[wt_index];
        if wt.terminals[tab_index].panes.len() >= MAX_PANES_PER_TAB {
            return Err(if settings.language.is_zh() {
                format!("每个标签最多 {MAX_PANES_PER_TAB} 个分屏")
            } else {
                format!("This tab supports at most {MAX_PANES_PER_TAB} panes")
            });
        }
        if wt.pane_count() >= MAX_TERMINALS_PER_WORKTREE {
            return Err(if settings.language.is_zh() {
                format!("此工作树最多同时打开 {MAX_TERMINALS_PER_WORKTREE} 个控制台")
            } else {
                format!("This worktree supports at most {MAX_TERMINALS_PER_WORKTREE} consoles")
            });
        }
        let cwd = wt.path.clone();
        let session = spawn_pane(cwd, None, SessionKind::Shell, settings, notifier)?;
        let new_id = NEXT_TERMINAL.fetch_add(1, Ordering::Relaxed);
        let title = session.command_label.clone();
        let wt = &mut self.worktrees[wt_index];
        let tab = &mut wt.terminals[tab_index];
        tab.panes.push(TerminalPane {
            id: new_id,
            session,
            title,
            kind: SessionKind::Shell,
            spawn_command: None,
            started: Instant::now(),
        });
        tab.split = direction;
        tab.active_pane = new_id;
        wt.active_terminal = Some(new_id);
        Ok(new_id)
    }

    /// Close one pane. Closing the last pane of a tab removes the tab.
    pub fn close_terminal(&mut self, pane_id: u64) {
        let Some((wt_index, tab_index, _)) = self.pane_owner(pane_id) else {
            return;
        };
        let wt = &mut self.worktrees[wt_index];
        let Some(pane_index) = wt.terminals[tab_index]
            .panes
            .iter()
            .position(|pane| pane.id == pane_id)
        else {
            return;
        };
        let tab = &mut wt.terminals[tab_index];
        tab.panes.remove(pane_index);
        if tab.active_pane == pane_id {
            let fallback = pane_index.saturating_sub(1);
            tab.active_pane = tab
                .panes
                .get(fallback)
                .or_else(|| tab.panes.first())
                .map(|pane| pane.id)
                .unwrap_or_default();
        }
        if tab.panes.is_empty() {
            wt.terminals.remove(tab_index);
            if wt.active_terminal == Some(pane_id) || wt.active_terminal.is_none() {
                wt.active_terminal = wt
                    .terminals
                    .get(tab_index.saturating_sub(1))
                    .or_else(|| wt.terminals.first())
                    .map(|tab| tab.active_pane);
            }
        } else {
            if tab.panes.len() == 1 {
                tab.split = PaneSplit::None;
            }
            if wt.active_terminal == Some(pane_id) || wt.active_terminal.is_none() {
                wt.active_terminal = Some(tab.active_pane);
            }
        }
    }

    /// Close a whole terminal tab (all of its panes).
    pub fn close_terminal_tab(&mut self, tab_id: u64) {
        let Some(wt_index) = self
            .worktrees
            .iter()
            .position(|wt| wt.terminals.iter().any(|tab| tab.id == tab_id))
        else {
            return;
        };
        let wt = &mut self.worktrees[wt_index];
        let Some(tab_index) = wt.terminals.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let owned_active = wt.active_terminal.is_none_or(|pane| {
            wt.terminals[tab_index]
                .panes
                .iter()
                .any(|pane_entry| pane_entry.id == pane)
        });
        wt.terminals.remove(tab_index);
        if owned_active {
            wt.active_terminal = wt
                .terminals
                .get(tab_index.saturating_sub(1))
                .or_else(|| wt.terminals.first())
                .map(|tab| tab.active_pane);
        }
    }

    pub fn restart_terminal(
        &mut self,
        id: u64,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<(), String> {
        let Some((wt_index, tab_index, pane_index)) = self.pane_owner(id) else {
            return Err("terminal not found".into());
        };
        let wt = &mut self.worktrees[wt_index];
        let old = &wt.terminals[tab_index].panes[pane_index];
        let command = old.spawn_command.clone();
        let title = old.title.clone();
        let kind = old.kind;
        let session = spawn_pane(wt.path.clone(), command, kind, settings, notifier)?;
        let tab = &mut wt.terminals[tab_index];
        let pane = &mut tab.panes[pane_index];
        pane.session = session;
        pane.title = title;
        pane.started = Instant::now();
        tab.active_pane = id;
        wt.active_terminal = Some(id);
        Ok(())
    }
}

/// Spawn one PTY for a terminal pane.
fn spawn_pane(
    cwd: PathBuf,
    command: Option<String>,
    kind: SessionKind,
    settings: &Settings,
    notifier: Option<Arc<dyn Notifier>>,
) -> Result<Session, String> {
    let is_agent = kind == SessionKind::Agent;
    let scrollback = if is_agent {
        settings.terminal.scrollback.min(AGENT_SCROLLBACK_LINES)
    } else {
        settings.terminal.scrollback
    };
    lazecode_terminal::spawn(SpawnOptions {
        shell: settings.terminal.shell.clone(),
        command,
        cwd,
        cols: 110,
        rows: 28,
        scrollback,
        // Cold history extends the hot budget onto disk; the image soft target
        // protects visible placements.
        cold_lines: scrollback.saturating_mul(8),
        cold_bytes: if is_agent {
            AGENT_COLD_BYTES
        } else {
            64 * 1024 * 1024
        },
        graphics_bytes: settings
            .terminal
            .graphics_cache_mb
            .saturating_mul(1024 * 1024),
        notifier,
        kind,
    })
    .map_err(|err| err.to_string())
}
