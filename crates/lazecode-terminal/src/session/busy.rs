//! Console busy detection: parse `ps` output into per-pane command lists.

use super::*;

pub(super) fn is_shell_name(name: &str, shell: &str) -> bool {
    let basename = |value: &str| {
        std::path::Path::new(value)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(value)
            .to_string()
    };
    let configured = basename(shell);
    let raw_base = basename(name);
    // macOS `ps` reports processes whose executable it cannot read with a
    // parenthesised name, e.g. `(bash)`; strip both forms before comparing.
    let base = raw_base
        .trim_start_matches('-')
        .trim_start_matches('(')
        .trim_end_matches(')');
    if raw_base == configured || base == configured {
        return true;
    }
    name.starts_with('-')
        && matches!(
            base,
            "zsh" | "bash" | "sh" | "dash" | "ksh" | "fish" | "tcsh" | "csh" | "nu" | "pwsh"
        )
}

/// One tty-attached process: the kernel's short command (login shells keep
/// their `-` prefix there, which is how an idle login shell is recognised)
/// and the display name (full executable path).
pub(super) struct TtyProcess {
    comm: String,
    name: String,
}

pub(super) fn busy_from_processes(
    entries: &[TtyProcess],
    _shell_pid: Option<u32>,
    shell: &str,
) -> BusyInfo {
    let mut commands = Vec::new();
    for process in entries {
        let name = process.name.trim();
        if name.is_empty() {
            continue;
        }
        // Match by name, not pid: macOS may re-exec or rename the shell, and
        // the recorded spawn pid no longer identifies it.
        if is_shell_name(process.comm.trim(), shell) || is_shell_name(name, shell) {
            continue;
        }
        let short = name.rsplit('/').next().unwrap_or(name).to_string();
        if !commands.iter().any(|command| command == &short) {
            commands.push(short);
        }
        if commands.len() >= 3 {
            break;
        }
    }
    BusyInfo {
        busy: !commands.is_empty(),
        commands,
    }
}

pub(super) fn busy_from_entries(
    entries: &[(u32, String)],
    shell_pid: Option<u32>,
    shell: &str,
) -> BusyInfo {
    let processes: Vec<TtyProcess> = entries
        .iter()
        .map(|(_, name)| TtyProcess {
            comm: name.clone(),
            name: name.clone(),
        })
        .collect();
    busy_from_processes(&processes, shell_pid, shell)
}

#[cfg(test)]
pub(super) fn parse_busy_output(stdout: &str, shell_pid: Option<u32>, shell: &str) -> BusyInfo {
    let entries: Vec<(u32, String)> = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let pid = parts.next()?.trim().parse::<u32>().ok()?;
            let name = parts.next()?.trim().to_string();
            (!name.is_empty()).then_some((pid, name))
        })
        .collect();
    busy_from_entries(&entries, shell_pid, shell)
}

/// One row of a single process-table snapshot.
pub(super) struct ProcessRow {
    /// Controlling tty device id, 0 when the process has no terminal.
    pub(super) tdev: u32,
    /// Kernel `p_comm`: short name, `-` prefixed for login shells.
    pub(super) comm: String,
    /// Full executable path (or the short command name as a fallback).
    pub(super) name: String,
}

/// Snapshot every process once, without spawning `ps`.
///
/// `ps -axo pid=,tty=,comm= -ww` costs 20-40ms of kernel time on a busy Mac,
/// and the activity dot polls every two seconds for the lifetime of the app.
/// `proc_listallpids` + `proc_pidinfo` + `proc_pidpath` answer the same three
/// questions (pid, controlling tty, executable) with per-process syscalls and
/// no fork/exec.
#[cfg(target_os = "macos")]
pub(super) fn process_table() -> Vec<ProcessRow> {
    use std::ffi::CStr;

    const PIDPATH_MAX: usize = 4096;
    let mut pids: Vec<libc::c_int> = vec![0; 4096];
    loop {
        let bytes = (pids.len() * std::mem::size_of::<libc::c_int>()) as libc::c_int;
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if count <= 0 {
            return Vec::new();
        }
        let count = count as usize;
        if count < pids.len() {
            pids.truncate(count);
            break;
        }
        pids.resize(pids.len() * 2, 0);
    }

    let mut rows = Vec::with_capacity(pids.len());
    let mut path = [0 as libc::c_char; PIDPATH_MAX];
    for pid in pids {
        if pid <= 0 {
            continue;
        }
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let got = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        if got != size {
            continue;
        }
        let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), PIDPATH_MAX as u32) };
        let comm: String = {
            let raw: Vec<u8> = info
                .pbi_comm
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8)
                .collect();
            String::from_utf8_lossy(&raw).into_owned()
        };
        let name = if len > 0 {
            unsafe { CStr::from_ptr(path.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        } else {
            // The kernel hides some paths (other users, protected binaries);
            // the 16-byte `comm` still identifies those.
            comm.clone()
        };
        rows.push(ProcessRow {
            tdev: info.e_tdev,
            comm,
            name,
        });
    }
    rows
}

/// Match one target's tty against the snapshot: anything attached to that
/// terminal besides the idle shell is a running command.
#[cfg(target_os = "macos")]
pub(super) fn busy_from_table(table: &[ProcessRow], target: &BusyTarget) -> BusyInfo {
    use std::os::unix::fs::MetadataExt;
    let Some(tty) = target.tty.as_deref() else {
        return BusyInfo::default();
    };
    // The device number of `/dev/ttysNNN` is exactly what `e_tdev` reports.
    let Ok(dev) = std::fs::metadata(tty).map(|meta| meta.rdev() as u32) else {
        return BusyInfo::default();
    };
    let entries: Vec<TtyProcess> = table
        .iter()
        .filter(|row| row.tdev == dev)
        .map(|row| TtyProcess {
            comm: row.comm.clone(),
            name: row.name.clone(),
        })
        .collect();
    busy_from_processes(&entries, target.shell_pid, &target.shell)
}

/// Probe all targets with one process-table snapshot: anything on the tty
/// besides the idle shell is a command (`zsh -lc omp` exec'd omp over the
/// shell pid).
pub fn probe_busy_many(targets: &[BusyTarget]) -> Vec<BusyInfo> {
    if targets.is_empty() {
        return Vec::new();
    }
    #[cfg(target_os = "macos")]
    {
        let table = process_table();
        if !table.is_empty() {
            return targets
                .iter()
                .map(|target| busy_from_table(&table, target))
                .collect();
        }
        // libproc unavailable (sandboxed build): fall back to `ps` below.
    }
    #[cfg(unix)]
    {
        let output = match std::process::Command::new("ps")
            .args(["-axo", "pid=,tty=,comm=", "-ww"])
            .output()
        {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                log::warn!(
                    "batch busy probe failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                return vec![BusyInfo::default(); targets.len()];
            }
            Err(err) => {
                log::warn!("batch busy probe failed: {err}");
                return vec![BusyInfo::default(); targets.len()];
            }
        };
        let mut by_tty: std::collections::HashMap<String, Vec<(u32, String)>> =
            std::collections::HashMap::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split_whitespace();
            let Some(pid) = parts.next().and_then(|pid| pid.parse::<u32>().ok()) else {
                continue;
            };
            let Some(tty) = parts.next() else {
                continue;
            };
            let name = parts.collect::<Vec<_>>().join(" ");
            if !name.is_empty() {
                by_tty
                    .entry(tty.trim_start_matches("/dev/").to_string())
                    .or_default()
                    .push((pid, name));
            }
        }
        targets
            .iter()
            .map(|target| {
                let Some(tty) = target.tty.as_deref() else {
                    return BusyInfo::default();
                };
                let tty = tty.to_string_lossy();
                let key = tty.strip_prefix("/dev/").unwrap_or(&tty);
                let entries = by_tty.get(key).map(Vec::as_slice).unwrap_or(&[]);
                busy_from_entries(entries, target.shell_pid, &target.shell)
            })
            .collect()
    }
    #[cfg(not(unix))]
    {
        vec![BusyInfo::default(); targets.len()]
    }
}

/// Probe one tty. Kept as the single-target convenience wrapper.
pub fn probe_busy(target: &BusyTarget) -> BusyInfo {
    probe_busy_many(std::slice::from_ref(target))
        .into_iter()
        .next()
        .unwrap_or_default()
}
