//! Git clean/smudge pipeline: worktree bytes must match what Git would
//! materialise, without invoking the `git` binary.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context};
use gix::attrs::StateRef;

use crate::{discover, workdir, Result};

const LFS_POINTER_VERSION: &str = "https://git-lfs.github.com/spec/v1";

#[derive(Default)]
struct Attributes {
    text: Option<bool>,
    text_auto: bool,
    eol: Option<String>,
    filter: Option<String>,
    ident: Option<bool>,
}

fn attributes(repo: &gix::Repository, path: &str) -> Result<Attributes> {
    let index = repo.index_or_empty()?;
    let mut stack = repo.attributes_only(
        &index,
        gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
    )?;
    let mut outcome = stack.selected_attribute_matches(["text", "eol", "filter", "ident"]);
    {
        let platform = stack.at_entry(path, None)?;
        platform.matching_attributes(&mut outcome);
    }
    let mut attributes = Attributes::default();
    for matched in outcome.iter_selected() {
        let name = matched.assignment.name.as_str();
        match (name, &matched.assignment.state) {
            ("text", StateRef::Set) => attributes.text = Some(true),
            ("text", StateRef::Unset | StateRef::Unspecified) => attributes.text = Some(false),
            ("text", StateRef::Value(value)) if value.as_bstr() == "auto" => {
                attributes.text_auto = true;
            }
            ("eol", StateRef::Value(value)) => attributes.eol = Some(value.as_bstr().to_string()),
            ("filter", StateRef::Value(value)) => {
                attributes.filter = Some(value.as_bstr().to_string())
            }
            ("ident", StateRef::Set) => attributes.ident = Some(true),
            ("ident", StateRef::Unset | StateRef::Unspecified) => attributes.ident = Some(false),
            _ => {}
        }
    }
    Ok(attributes)
}

fn config_string(repo: &gix::Repository, key: &str) -> Option<String> {
    repo.config_snapshot()
        .string(key)
        .map(|value| value.to_string())
}

fn config_bool(repo: &gix::Repository, key: &str) -> Option<bool> {
    repo.config_snapshot().boolean(key)
}

fn is_text(repo: &gix::Repository, attributes: &Attributes, data: &[u8]) -> bool {
    if let Some(text) = attributes.text {
        return text;
    }
    let binary = data.iter().take(8000).any(|byte| *byte == 0);
    if attributes.text_auto {
        return !binary;
    }
    if binary {
        return false;
    }
    if attributes.eol.is_some() {
        return true;
    }
    matches!(
        config_string(repo, "core.autocrlf").as_deref(),
        Some("true") | Some("input")
    )
}

/// `cwd` must be the repository workdir: git runs filter drivers from the
/// worktree top level, so `%f`-relative resolution and driver-relative
/// reads expect that directory, not wherever the UI opened the repo.
fn run_filter(cwd: &Path, command: &str, path: &str, data: &[u8]) -> Result<Vec<u8>> {
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    let command = command.replace("%f", &quoted);
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot start filter command: {command}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(data)
            .with_context(|| format!("cannot write to filter command: {command}"))?;
    }
    let output = child
        .wait_with_output()
        .with_context(|| format!("cannot wait for filter command: {command}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "filter command failed ({command}): {}",
            stderr.trim()
        ));
    }
    Ok(output.stdout)
}

fn run_driver_filter(
    repo: &gix::Repository,
    cwd: &Path,
    driver: &str,
    direction: &str,
    path: &str,
    data: &[u8],
) -> Result<Option<Vec<u8>>> {
    let key = format!("filter.{driver}.{direction}");
    let Some(command) = config_string(repo, &key) else {
        return Ok(None);
    };
    let required = config_bool(repo, &format!("filter.{driver}.required")).unwrap_or(false);
    match run_filter(cwd, &command, path, data) {
        Ok(output) => Ok(Some(output)),
        Err(error) if required => Err(error),
        Err(error) => {
            log::warn!("optional {key} failed, using unfiltered bytes: {error}");
            Ok(None)
        }
    }
}

fn normalize_lf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut index = 0;
    while index < data.len() {
        if data[index] == b'\r' && data.get(index + 1) == Some(&b'\n') {
            out.push(b'\n');
            index += 2;
        } else {
            out.push(data[index]);
            index += 1;
        }
    }
    out
}

fn to_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 16);
    for byte in normalize_lf(data) {
        if byte == b'\n' {
            out.push(b'\r');
        }
        out.push(byte);
    }
    out
}

/// Collapse `$Id: hash $` to `$Id$` (clean direction).
fn collapse_ident(data: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(data) else {
        return data.to_vec();
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("$Id:") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        if let Some(end) = after.find('$') {
            out.push_str("$Id$");
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.into_bytes()
}

/// Expand `$Id$` with the blob hash (smudge direction).
fn expand_ident(data: &[u8], oid: &str) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(data) else {
        return data.to_vec();
    };
    text.replace("$Id$", &format!("$Id: {oid} $")).into_bytes()
}

fn should_crlf(repo: &gix::Repository, attributes: &Attributes) -> bool {
    if let Some(eol) = attributes.eol.as_deref() {
        return match eol {
            "crlf" => true,
            "lf" => false,
            _ => false,
        };
    }
    match config_string(repo, "core.autocrlf").as_deref() {
        Some("true") => true,
        Some("input") => false,
        _ => config_string(repo, "core.eol").as_deref() == Some("crlf"),
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, data);
    let mut out = String::with_capacity(64);
    for byte in digest.as_ref() {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

const LFS_OID_LEN: usize = 64;

fn valid_lfs_oid(oid: &str) -> bool {
    oid.len() == LFS_OID_LEN && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn lfs_pointer(data: &[u8]) -> Option<(String, u64)> {
    let text = std::str::from_utf8(data).ok()?;
    let mut version = false;
    let mut oid = None;
    let mut size = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version ") {
            version = value.trim() == LFS_POINTER_VERSION;
        } else if let Some(value) = line.strip_prefix("oid sha256:") {
            oid = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("size ") {
            size = value.trim().parse::<u64>().ok();
        }
    }
    let oid = oid?;
    if !valid_lfs_oid(&oid) {
        log::warn!("ignoring LFS pointer with a malformed object id");
        return None;
    }
    (version && size.is_some()).then(|| (oid, size.unwrap_or(0)))
}

/// Resolve the deepest existing ancestor of `path`, then append the missing
/// components. Canonicalising only the existing part keeps symlinked parents
/// from redirecting a not-yet-created LFS object outside the object store.
fn canonical_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut current = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&current) {
            Ok(resolved) => {
                let mut resolved = resolved;
                for component in missing.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = current
                    .file_name()
                    .ok_or_else(|| anyhow!("cannot resolve LFS object path"))?
                    .to_os_string();
                missing.push(name);
                current = current
                    .parent()
                    .ok_or_else(|| anyhow!("cannot resolve LFS object path"))?
                    .to_path_buf();
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Resolve an object id strictly inside `<common_dir>/lfs/objects`. Both the
/// object store itself and each existing path component are canonicalised so
/// absolute ids, `..`, and symlink escapes are rejected instead of read.
fn lfs_object_path(repo: &gix::Repository, oid: &str) -> Result<PathBuf> {
    if !valid_lfs_oid(oid) {
        return Err(anyhow!("invalid LFS object id '{oid}'"));
    }
    let common = std::fs::canonicalize(repo.common_dir())?;
    let objects = common.join("lfs").join("objects");
    let root = std::fs::canonicalize(&objects)?;
    if root != objects {
        return Err(anyhow!(
            "LFS object directory resolves outside of {}",
            objects.display()
        ));
    }
    let candidate = root.join(&oid[..2]).join(&oid[2..4]).join(oid);
    let parent = candidate
        .parent()
        .ok_or_else(|| anyhow!("LFS object has no parent directory"))?;
    let canonical_parent = canonical_existing_prefix(parent)?;
    if !canonical_parent.starts_with(&root) {
        return Err(anyhow!("LFS object path escapes the object directory"));
    }
    let resolved = match std::fs::canonicalize(&candidate) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            canonical_parent.join(candidate.file_name().expect("LFS object has a file name"))
        }
        Err(error) => return Err(error.into()),
    };
    if !resolved.starts_with(&root) {
        return Err(anyhow!("LFS object path escapes the object directory"));
    }
    Ok(resolved)
}

fn lfs_clean(repo: &gix::Repository, data: &[u8]) -> Result<Vec<u8>> {
    if lfs_pointer(data).is_some() {
        return Ok(data.to_vec());
    }
    let oid = sha256_hex(data);
    let object_dir = repo.common_dir().join("lfs/objects");
    std::fs::create_dir_all(&object_dir)?;
    let path = lfs_object_path(repo, &oid)?;
    if !path.exists() {
        lazecode_fs::file::write_atomic(&path, data)?;
    }
    Ok(format!(
        "version {LFS_POINTER_VERSION}\noid sha256:{oid}\nsize {}\n",
        data.len()
    )
    .into_bytes())
}

fn lfs_clean_file(repo: &gix::Repository, source: &Path) -> Result<Vec<u8>> {
    let length = std::fs::metadata(source)?.len();
    if length <= 1024 {
        let small = std::fs::read(source)?;
        if lfs_pointer(&small).is_some() {
            return Ok(small);
        }
    }
    let object_dir = repo.common_dir().join("lfs/objects");
    let temp_dir = object_dir.join("tmp");
    std::fs::create_dir_all(&temp_dir)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp = temp_dir.join(format!("{}-{stamp}", std::process::id()));
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::File::create(&temp)?;
    let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read])?;
    }
    output.sync_all()?;
    drop(output);
    let digest = hasher.finish();
    let mut oid = String::with_capacity(64);
    for byte in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(oid, "{byte:02x}");
    }
    let target = lfs_object_path(repo, &oid)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if target.exists() {
        let _ = std::fs::remove_file(&temp);
    } else {
        std::fs::rename(&temp, &target)?;
    }
    Ok(format!("version {LFS_POINTER_VERSION}\noid sha256:{oid}\nsize {length}\n").into_bytes())
}

fn lfs_smudge(repo: &gix::Repository, data: &[u8]) -> Result<Vec<u8>> {
    let Some((oid, size)) = lfs_pointer(data) else {
        return Ok(data.to_vec());
    };
    let path = match lfs_object_path(repo, &oid) {
        Ok(path) => path,
        Err(error) => {
            log::warn!("LFS object path is unsafe ({error}); leaving pointer text");
            return Ok(data.to_vec());
        }
    };
    match std::fs::read(&path) {
        Ok(bytes) if bytes.len() as u64 == size => Ok(bytes),
        Ok(bytes) => {
            log::warn!("LFS object {oid} has size {}, expected {size}", bytes.len());
            Ok(data.to_vec())
        }
        Err(error) => {
            log::warn!("LFS object {oid} is missing ({error}); leaving pointer text");
            Ok(data.to_vec())
        }
    }
}

/// Materialise an LFS pointer straight into `target` without buffering the
/// object. Returns false when the path is not LFS or the object is missing so
/// the caller can fall back to the normal smudge path.
pub fn smudge_lfs_to_file(root: &Path, path: &str, data: &[u8], target: &Path) -> Result<bool> {
    let repo = discover(root)?;
    smudge_lfs_to_file_in(&repo, path, data, target)
}

pub fn smudge_lfs_to_file_in(
    repo: &gix::Repository,
    path: &str,
    data: &[u8],
    target: &Path,
) -> Result<bool> {
    let attributes = attributes(repo, path)?;
    smudge_lfs_to_file_with(repo, path, &attributes, data, target)
}

fn smudge_lfs_to_file_with(
    repo: &gix::Repository,
    path: &str,
    attributes: &Attributes,
    data: &[u8],
    target: &Path,
) -> Result<bool> {
    let _ = path;
    if attributes.filter.as_deref() != Some("lfs") {
        return Ok(false);
    }
    let Some((oid, expected)) = lfs_pointer(data) else {
        return Ok(false);
    };
    let object = match lfs_object_path(repo, &oid) {
        Ok(object) => object,
        Err(error) => {
            log::warn!("LFS object path is unsafe ({error}); leaving pointer text");
            return Ok(false);
        }
    };
    let Ok(mut input) = std::fs::File::open(&object) else {
        log::warn!("LFS object {oid} is missing; leaving pointer text");
        return Ok(false);
    };
    let Some(parent) = target.parent() else {
        return Ok(false);
    };
    std::fs::create_dir_all(parent)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let temp = parent.join(format!(".lazecode-lfs-{}-{stamp}", std::process::id()));
    let mut output = std::fs::File::create(&temp)?;
    let copied = match std::io::copy(&mut input, &mut output) {
        Ok(copied) => copied,
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
    };
    output.sync_all()?;
    drop(output);
    if copied != expected {
        let _ = std::fs::remove_file(&temp);
        log::warn!("LFS object {oid} has size {copied}, expected {expected}; leaving pointer text");
        return Ok(false);
    }
    std::fs::rename(&temp, target)?;
    Ok(true)
}

/// Canonical (repository) bytes for `data` staged from the worktree.
pub fn clean(root: &Path, path: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    let repo = discover(root)?;
    clean_in(&repo, path, data)
}

pub fn clean_in(repo: &gix::Repository, path: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    let _ = workdir(repo)?;
    let attributes = attributes(repo, path)?;
    clean_with(repo, path, &attributes, data)
}

/// Reusable attribute/filter session. Building the attributes stack once
/// avoids re-reading the index and global attributes for every changed file.
pub struct FilterContext<'repo> {
    stack: gix::AttributeStack<'repo>,
    /// The repository workdir. Filter-driver subprocesses always run from
    /// here, regardless of which path the caller used to open the repo.
    workdir: std::path::PathBuf,
}

impl<'repo> FilterContext<'repo> {
    pub fn new(repo: &'repo gix::Repository) -> Result<Self> {
        let index = repo.index_or_empty()?;
        let stack = repo.attributes_only(
            &index,
            gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
        )?;
        let workdir = workdir(repo)?.to_path_buf();
        Ok(Self { stack, workdir })
    }

    pub fn repo(&self) -> &gix::Repository {
        self.stack.repo
    }

    fn attributes(&mut self, path: &str) -> Result<Attributes> {
        let mut outcome = self
            .stack
            .selected_attribute_matches(["text", "eol", "filter", "ident"]);
        {
            let platform = self.stack.at_entry(path, None)?;
            platform.matching_attributes(&mut outcome);
        }
        let mut attributes = Attributes::default();
        for matched in outcome.iter_selected() {
            let name = matched.assignment.name.as_str();
            match (name, &matched.assignment.state) {
                ("text", StateRef::Set) => attributes.text = Some(true),
                ("text", StateRef::Unset | StateRef::Unspecified) => attributes.text = Some(false),
                ("text", StateRef::Value(value)) if value.as_bstr() == "auto" => {
                    attributes.text_auto = true;
                }
                ("eol", StateRef::Value(value)) => {
                    attributes.eol = Some(value.as_bstr().to_string())
                }
                ("filter", StateRef::Value(value)) => {
                    attributes.filter = Some(value.as_bstr().to_string())
                }
                ("ident", StateRef::Set) => attributes.ident = Some(true),
                ("ident", StateRef::Unset | StateRef::Unspecified) => {
                    attributes.ident = Some(false)
                }
                _ => {}
            }
        }
        Ok(attributes)
    }

    pub fn filter_driver(&mut self, path: &str) -> Result<Option<String>> {
        Ok(self.attributes(path)?.filter)
    }

    pub fn clean(&mut self, path: &str, data: Vec<u8>) -> Result<Vec<u8>> {
        let attributes = self.attributes(path)?;
        clean_with(self.stack.repo, path, &attributes, data)
    }

    pub fn clean_file(&mut self, path: &str, source: &Path) -> Result<Vec<u8>> {
        let attributes = self.attributes(path)?;
        if attributes.filter.as_deref() == Some("lfs") {
            return lfs_clean_file(self.stack.repo, source);
        }
        let data = std::fs::read(source)?;
        clean_with(self.stack.repo, path, &attributes, data)
    }

    pub fn smudge(&mut self, path: &str, oid: &str, data: Vec<u8>) -> Result<Vec<u8>> {
        let attributes = self.attributes(path)?;
        let mut data = data;
        if let Some(driver) = attributes.filter.as_deref() {
            if driver == "lfs" {
                data = lfs_smudge(self.stack.repo, &data)?;
            } else if let Some(filtered) = run_driver_filter(
                self.stack.repo,
                &self.workdir.clone(),
                driver,
                "smudge",
                path,
                &data,
            )? {
                data = filtered;
            }
        }
        if attributes.ident == Some(true) {
            data = expand_ident(&data, oid);
        }
        if is_text(self.stack.repo, &attributes, &data) && should_crlf(self.stack.repo, &attributes)
        {
            data = to_crlf(&data);
        }
        Ok(data)
    }

    /// Return LFS pointer metadata when `path` is filtered through LFS.
    pub fn lfs_pointer_info(&mut self, path: &str, data: &[u8]) -> Result<Option<(String, u64)>> {
        if self.attributes(path)?.filter.as_deref() != Some("lfs") {
            return Ok(None);
        }
        Ok(lfs_pointer(data))
    }

    /// Resolve an object id inside the repository's LFS object store. A
    /// malformed or escaping id is an error so callers cannot read unrelated
    /// files (including through symlinks).
    pub fn lfs_object_path(&self, oid: &str) -> Result<PathBuf> {
        lfs_object_path(self.stack.repo, oid)
    }

    pub fn smudge_lfs_to_file(&mut self, path: &str, data: &[u8], target: &Path) -> Result<bool> {
        let attributes = self.attributes(path)?;
        smudge_lfs_to_file_with(self.stack.repo, path, &attributes, data, target)
    }
}

/// Return the configured filter driver for a path, without touching bytes.
pub fn filter_driver(root: &Path, path: &str) -> Result<Option<String>> {
    let repo = discover(root)?;
    filter_driver_in(&repo, path)
}

pub fn filter_driver_in(repo: &gix::Repository, path: &str) -> Result<Option<String>> {
    let _ = workdir(repo)?;
    Ok(attributes(repo, path)?.filter)
}

/// Canonical bytes for a worktree file, streaming LFS objects instead of
/// loading the whole file into memory.
pub fn clean_file(root: &Path, path: &str, source: &Path) -> Result<Vec<u8>> {
    let repo = discover(root)?;
    clean_file_in(&repo, path, source)
}

pub fn clean_file_in(repo: &gix::Repository, path: &str, source: &Path) -> Result<Vec<u8>> {
    let _ = workdir(repo)?;
    let attributes = attributes(repo, path)?;
    if attributes.filter.as_deref() == Some("lfs") {
        return lfs_clean_file(repo, source);
    }
    let data = std::fs::read(source)?;
    clean_with(repo, path, &attributes, data)
}

fn clean_with(
    repo: &gix::Repository,
    path: &str,
    attributes: &Attributes,
    data: Vec<u8>,
) -> Result<Vec<u8>> {
    let mut data = data;
    if let Some(driver) = attributes.filter.as_deref() {
        if driver == "lfs" {
            data = lfs_clean(repo, &data)?;
        } else if let Some(filtered) =
            run_driver_filter(repo, &workdir(repo)?, driver, "clean", path, &data)?
        {
            data = filtered;
        }
    }
    if attributes.ident == Some(true) {
        data = collapse_ident(&data);
    }
    if is_text(repo, attributes, &data) {
        data = normalize_lf(&data);
    }
    Ok(data)
}

/// Worktree bytes for `data` stored in the repository (checkout direction).
pub fn smudge(root: &Path, path: &str, oid: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    let repo = discover(root)?;
    smudge_in(&repo, path, oid, data)
}

pub fn smudge_in(repo: &gix::Repository, path: &str, oid: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    let _ = workdir(repo)?;
    let attributes = attributes(repo, path)?;
    let mut data = data;
    if let Some(driver) = attributes.filter.as_deref() {
        if driver == "lfs" {
            data = lfs_smudge(repo, &data)?;
        } else if let Some(filtered) =
            run_driver_filter(repo, &workdir(repo)?, driver, "smudge", path, &data)?
        {
            data = filtered;
        }
    }
    if attributes.ident == Some(true) {
        data = expand_ident(&data, oid);
    }
    if is_text(repo, &attributes, &data) && should_crlf(repo, &attributes) {
        data = to_crlf(&data);
    }
    Ok(data)
}
