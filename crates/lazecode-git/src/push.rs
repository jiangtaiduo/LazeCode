//! Native `git push` transport.
//!
//! gix 0.90 has no send-pack driver, so this module speaks receive-pack
//! directly over the gix blocking transport: handshake, update commands,
//! a self-contained zlib pack, and `report-status` parsing.

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use anyhow::anyhow;
use gix::hash::ObjectId;
use gix::protocol::transport::client::blocking_io::{ReadlineBufRead, Transport as _};
use gix::protocol::transport::client::{MessageKind, WriteMode};
use gix::protocol::transport::packetline::PacketLineRef;
use gix::protocol::transport::Service;
use gix::refs::transaction::{PreviousValue, RefEdit};
use gix::refs::Target;
use gix::remote::Direction;

use crate::{discover, Result};

fn collect_reachable(repo: &gix::Repository, tip: ObjectId) -> Result<Vec<ObjectId>> {
    let mut seen = HashSet::new();
    let mut queue = vec![tip];
    let mut objects = Vec::new();
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        let object = repo.find_object(id)?;
        match object.kind {
            gix::objs::Kind::Commit => {
                let commit = repo.find_commit(id)?;
                queue.push(commit.tree_id()?.detach());
                queue.extend(commit.parent_ids().map(|parent| parent.detach()));
            }
            gix::objs::Kind::Tree => {
                let tree = repo.find_tree(id)?;
                for entry in tree.iter() {
                    queue.push(entry?.object_id());
                }
            }
            gix::objs::Kind::Tag => {
                let tag = repo.find_tag(id)?;
                queue.push(tag.target_id()?.detach());
            }
            gix::objs::Kind::Blob => {}
        }
        objects.push(id);
    }
    Ok(objects)
}

fn write_object_header(out: &mut Vec<u8>, kind: u8, mut size: usize) {
    let mut byte = (kind << 4) | ((size & 0x0f) as u8);
    size >>= 4;
    while size > 0 {
        out.push(byte | 0x80);
        byte = (size & 0x7f) as u8;
        size >>= 7;
    }
    out.push(byte);
}

fn build_pack(repo: &gix::Repository, objects: &[ObjectId]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(b"PACK");
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(objects.len() as u32).to_be_bytes());
    for id in objects {
        let object = repo.find_object(*id)?;
        let kind = match object.kind {
            gix::objs::Kind::Commit => 1u8,
            gix::objs::Kind::Tree => 2,
            gix::objs::Kind::Blob => 3,
            gix::objs::Kind::Tag => 4,
        };
        write_object_header(&mut out, kind, object.data.len());
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&object.data)?;
        out.extend_from_slice(&encoder.finish()?);
    }
    let mut hasher = gix::hash::hasher(repo.object_hash());
    hasher.update(&out);
    let digest = hasher
        .try_finalize()
        .map_err(|err| anyhow!("cannot hash pack: {err}"))?;
    out.extend_from_slice(digest.as_slice());
    Ok(out)
}

fn parse_status(status: &[u8], branch: &str) -> Result<()> {
    let text = String::from_utf8_lossy(status);
    let expected_ref = format!("refs/heads/{branch}");
    let mut saw_ref = false;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(error) = line.strip_prefix("unpack ") {
            if error != "ok" {
                anyhow::bail!("remote failed to unpack pack: {error}");
            }
        } else if let Some(error) = line.strip_prefix("ng ") {
            anyhow::bail!("push rejected: {error}");
        } else if let Some(name) = line.strip_prefix("ok ") {
            if name == expected_ref {
                saw_ref = true;
            }
        }
    }
    if !saw_ref {
        anyhow::bail!("push did not report success for {expected_ref}");
    }
    Ok(())
}

pub(crate) fn write_upstream(repo: &gix::Repository, branch: &str, remote: &str) -> Result<()> {
    let path = repo.common_dir().join("config");
    let mut config =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)?;
    config.set_raw_value_by(
        "branch",
        Some(gix::bstr::BStr::new(branch)),
        "remote",
        remote,
    )?;
    config.set_raw_value_by(
        "branch",
        Some(gix::bstr::BStr::new(branch)),
        "merge",
        format!("refs/heads/{branch}"),
    )?;
    let mut bytes = Vec::new();
    config.write_to(&mut bytes)?;
    lazecode_fs::file::write_atomic(&path, &bytes)?;
    Ok(())
}

pub fn push(
    root: &Path,
    remote_name: &str,
    branch: &str,
    set_upstream: bool,
    force: bool,
) -> Result<String> {
    let repo = discover(root)?;
    let new: ObjectId = repo
        .find_reference(format!("refs/heads/{branch}").as_str())
        .map_err(|_| anyhow!("branch '{branch}' does not exist"))?
        .target()
        .try_id()
        .map(|id| id.to_owned())
        .ok_or_else(|| anyhow!("branch '{branch}' has no target"))?;
    let tracking = format!("refs/remotes/{remote_name}/{branch}");
    let old = repo
        .find_reference(tracking.as_str())
        .ok()
        .and_then(|reference| reference.target().try_id().map(|id| id.to_owned()))
        .unwrap_or_else(|| ObjectId::null(repo.object_hash()));
    if !force
        && old != ObjectId::null(repo.object_hash())
        && repo.merge_base(old, new)?.map(|id| id.detach()) != Some(old)
    {
        anyhow::bail!("push rejected: non-fast-forward (fetch or force first)");
    }

    let old_objects: HashSet<ObjectId> = if old == ObjectId::null(repo.object_hash()) {
        HashSet::new()
    } else if repo.find_object(old).is_ok() {
        collect_reachable(&repo, old)?.into_iter().collect()
    } else {
        HashSet::new()
    };
    let objects: Vec<ObjectId> = collect_reachable(&repo, new)?
        .into_iter()
        .filter(|id| !old_objects.contains(id))
        .collect();
    let pack = build_pack(&repo, &objects)?;

    {
        let remote = repo.find_remote(remote_name)?;
        let mut connection = remote.connect(Direction::Push)?;
        let transport = connection.transport_mut();
        {
            let response = transport.handshake(Service::ReceivePack, &[])?;
            if let Some(mut refs) = response.refs {
                while refs.readline().is_some() {}
            }
        }
        let mut command = format!("{old} {new} refs/heads/{branch}\0report-status\n");
        let mut writer = transport.request(
            WriteMode::OneLfTerminatedLinePerWriteCall,
            MessageKind::Flush,
            false,
        )?;
        writer.write_all(command.as_bytes())?;
        writer.flush()?;
        writer.write_message(MessageKind::Flush)?;
        command.clear();
        let (mut raw, mut reader) = writer.into_parts();
        raw.write_all(&pack)?;
        raw.flush()?;
        drop(raw);

        let mut status = Vec::new();
        while let Some(line) = reader.readline() {
            let line = line.map_err(|err| anyhow!("read push response: {err}"))?;
            let line = line.map_err(|err| anyhow!("read push response: {err}"))?;
            match line {
                PacketLineRef::Data(data) => status.extend_from_slice(data),
                PacketLineRef::Flush => break,
                PacketLineRef::Delimiter | PacketLineRef::ResponseEnd => {}
            }
        }
        parse_status(&status, branch)?;
    }

    let message = format!("push: {remote_name}/{branch}");
    let edit = RefEdit::update(
        tracking.as_str().try_into()?,
        Target::Object(new),
        PreviousValue::Any,
        message,
    );
    repo.edit_reference(edit)?;

    if set_upstream {
        write_upstream(&repo, branch, remote_name)?;
    }
    Ok(format!("pushed {branch} to {remote_name}"))
}
