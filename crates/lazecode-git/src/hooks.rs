//! Git hook discovery and execution: a failing hook aborts the operation and
//! hooks are user processes, not the `git` binary.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context};

use crate::{discover, workdir, Result};

/// Run `git_dir/hooks/<name>` if it exists and is executable.
///
/// `args` are passed verbatim. `stdin` is written to the hook before waiting;
/// stdout/stderr are captured so a failure can be shown in the UI.
pub fn run(root: &Path, name: &str, args: &[&str], stdin: Option<&str>) -> Result<()> {
    let repo = discover(root)?;
    let git_dir = repo.git_dir().to_path_buf();
    let hook = git_dir.join("hooks").join(name);
    let metadata = match std::fs::symlink_metadata(&hook) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(anyhow!(
                "cannot inspect {} hook {}: {error}",
                name,
                hook.display()
            ))
        }
    };
    if !metadata.is_file() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Ok(());
        }
    }
    let workdir = workdir(&repo)?;
    let mut command = Command::new(&hook);
    command
        .current_dir(&workdir)
        .env("GIT_DIR", &git_dir)
        .env("GIT_WORK_TREE", &workdir)
        .env("GIT_INDEX_FILE", repo.index_path())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = command
        .spawn()
        .with_context(|| format!("cannot run {name} hook {}", hook.display()))?;
    if let Some(input) = stdin {
        use std::io::Write;
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(input.as_bytes())
                .with_context(|| format!("cannot write to {name} hook"))?;
        }
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("cannot wait for {name} hook"))?;
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if detail.is_empty() {
        Err(anyhow!("{name} hook failed ({})", output.status))
    } else {
        Err(anyhow!("{name} hook failed: {detail}"))
    }
}
