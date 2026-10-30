//! 扩展清单与能力门（Zed `extension_host` 的第一步：清单 + 授权）。
//!
//! Zed 的扩展是 `wasmtime` 沙箱 + `capability_granter.rs` 按能力放行 +
//! `extension.toml` 声明。本模块先落"清单与门"：扩展目录
//! `<workspace>/.lazecode/extensions/*/extension.toml` 的解析、校验与能力判定。
//! wasmtime 执行运行时是后续项（引入之前，扩展只能被"看见"，不能被"运行" ——
//! 这是有意为之：先有清单与门，再有执行器，顺序不能反）。
//!
//! 能力词汇是**封闭集合**（见 CAPABILITY_DOCS）：清单里出现未知能力，
//! 整个清单判无效（fail-closed）。新能力只能随发版加入词汇表，
//! 旧版本遇到新能力扩展会明确报错，而不是静默放行。
//!
//! 当前调用方：测试 + 命令注册。前端扩展面板是后续项。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::State;

use crate::commands::{authorize_workspace_path, WorkspaceRegistry};

/// 扩展根（工作区下）。点开头：与 .vscode 同类，不污染项目树展示。
const EXTENSIONS_DIR: &str = ".lazecode/extensions";
const MANIFEST_FILE: &str = "extension.toml";

/// 能力词汇：id → 人类可读说明（报错与未来 UI 直接用）。
const CAPABILITY_DOCS: &[(&str, &str)] = &[
    ("fs.read", "读取工作区文件"),
    ("fs.write", "写入工作区文件"),
    ("terminal.spawn", "启动终端/子进程"),
    ("network.access", "访问网络"),
    ("settings.read", "读取应用设置"),
    ("settings.write", "修改应用设置"),
];

fn is_known_capability(cap: &str) -> bool {
    CAPABILITY_DOCS.iter().any(|(id, _)| *id == cap)
}

/// 清单原文（extension.toml 顶层拍平，Zed 同名文件同思路但更扁平一层）。
#[derive(Debug, Deserialize)]
struct ManifestToml {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionDto {
    /// 扩展目录名（清单无效时定位用）
    pub dir: String,
    pub id: String,
    pub name: String,
    pub version: String,
    /// 已确认的能力（未知能力进 warnings，不进这里）
    pub capabilities: Vec<String>,
    pub valid: bool,
    pub warnings: Vec<String>,
}

fn valid_id(id: &str) -> bool {
    // kebab-case：小写字母数字中横线，开头须为字母数字
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_version(version: &str) -> bool {
    // X.Y.Z 纯数字（预发布/构建元数据是后续项，现在拒绝比半支持诚实）
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 8 && p.chars().all(|c| c.is_ascii_digit()))
}

/// 解析并校验一份清单文本。返回 DTO（valid=false 时 id/name/version 尽量填，
/// 调用方按 warnings 展示原因）。
pub fn parse_manifest(dir_name: &str, text: &str) -> ExtensionDto {
    let mut warnings = Vec::new();
    let parsed: ManifestToml = match toml::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            return ExtensionDto {
                dir: dir_name.to_string(),
                id: String::new(),
                name: String::new(),
                version: String::new(),
                capabilities: Vec::new(),
                valid: false,
                warnings: vec![format!("TOML 解析失败: {e}")],
            };
        }
    };
    if !valid_id(&parsed.id) {
        warnings.push(format!("非法 id（应为 kebab-case）: '{}'", parsed.id));
    }
    if parsed.name.trim().is_empty() || parsed.name.len() > 100 {
        warnings.push("name 为空或超过 100 字符".to_string());
    }
    if !valid_version(&parsed.version) {
        warnings.push(format!("非法 version（应为 X.Y.Z）: '{}'", parsed.version));
    }
    let mut capabilities = Vec::new();
    for cap in &parsed.capabilities {
        if is_known_capability(cap) {
            if !capabilities.contains(cap) {
                capabilities.push(cap.clone());
            }
        } else {
            warnings.push(format!("未知能力（已拒绝）: '{cap}'"));
        }
    }
    // fail-closed：任何警告（未知能力/非法 id/非法版本）都判清单无效。
    // 默许"部分有效"等于让扩展带着没过审的能力跑 —— 门就白设了。
    // 唯一例外：name 为空这种展示问题不影响安全，但同样判无效
    //（逼作者修好，而不是带病上岗）。
    let valid = warnings.is_empty();
    ExtensionDto {
        dir: dir_name.to_string(),
        id: parsed.id,
        name: parsed.name,
        version: parsed.version,
        capabilities,
        valid,
        warnings,
    }
}

/// 判定某扩展是否被授予某能力（执行器接进来后调这里，不直接读清单）。
pub fn is_capability_granted(ext: &ExtensionDto, capability: &str) -> bool {
    ext.valid && ext.capabilities.iter().any(|c| c == capability)
}

/// 列出工作区内的扩展（只读命令：读已授权根内的清单，无执行）。
#[tauri::command]
pub async fn list_extensions(
    workspace_path: String,
    registry: State<'_, WorkspaceRegistry>,
) -> Result<Vec<ExtensionDto>, String> {
    let root = authorize_workspace_path(&registry, &workspace_path, false)?;
    let ext_root = root.join(EXTENSIONS_DIR);
    tauri::async_runtime::spawn_blocking(move || Ok(list_extensions_impl(&ext_root)))
        .await
        .map_err(|e| format!("list_extensions 任务失败: {e}"))?
}

fn list_extensions_impl(ext_root: &Path) -> Vec<ExtensionDto> {
    let mut out = Vec::new();
    // 目录不存在 = 没有扩展（大多数项目），不是错误
    let entries = match std::fs::read_dir(ext_root) {
        Ok(e) => e,
        Err(_) => return out,
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let manifest = dir.join(MANIFEST_FILE);
        let dto = match std::fs::read_to_string(&manifest) {
            Ok(text) => parse_manifest(&dir_name, &text),
            Err(_) => ExtensionDto {
                dir: dir_name,
                id: String::new(),
                name: String::new(),
                version: String::new(),
                capabilities: Vec::new(),
                valid: false,
                warnings: vec![format!("缺少 {MANIFEST_FILE}")],
            },
        };
        out.push(dto);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::register_workspace_dir;

    fn registry_for(dir: &Path) -> WorkspaceRegistry {
        let reg = WorkspaceRegistry::default();
        register_workspace_dir(&reg, dir.to_str().unwrap(), false).unwrap();
        reg
    }

    fn write_ext(root: &Path, dir: &str, manifest: Option<&str>) {
        let d = root.join(".lazecode/extensions").join(dir);
        std::fs::create_dir_all(&d).unwrap();
        if let Some(text) = manifest {
            std::fs::write(d.join("extension.toml"), text).unwrap();
        }
    }

    #[test]
    fn valid_manifest_parses_with_capabilities() {
        let dto = parse_manifest(
            "demo",
            "id = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\ncapabilities = [\"fs.read\", \"fs.read\"]\n",
        );
        assert!(dto.valid, "应有效: {:?}", dto.warnings);
        assert_eq!(dto.capabilities, vec!["fs.read"], "能力去重");
    }

    #[test]
    fn unknown_capability_fails_closed() {
        let dto = parse_manifest(
            "evil",
            "id = \"evil\"\nname = \"Evil\"\nversion = \"1.0.0\"\ncapabilities = [\"fs.read\", \"process.spawn-all\"]\n",
        );
        assert!(!dto.valid, "未知能力必须判无效");
        assert_eq!(dto.capabilities, vec!["fs.read"], "已知能力保留，未知不进");
        assert!(dto.warnings.iter().any(|w| w.contains("process.spawn-all")));
    }

    #[test]
    fn bad_id_version_name_rejected() {
        for (id, version, name) in [
            ("Bad_Name", "1.0.0", "x"),
            ("ok", "1.0", "x"),
            ("ok", "1.0.0-beta", "x"),
            ("ok", "1.0.0", ""),
        ] {
            let text = format!("id = \"{id}\"\nname = \"{name}\"\nversion = \"{version}\"\n");
            let dto = parse_manifest("t", &text);
            assert!(!dto.valid, "{id}/{version} 应无效: {:?}", dto.warnings);
        }
    }

    #[test]
    fn broken_toml_reports_not_panics() {
        let dto = parse_manifest("broken", "id = [unclosed\n");
        assert!(!dto.valid);
        assert!(dto.warnings.iter().any(|w| w.contains("TOML")));
    }

    #[test]
    fn capability_gate_needs_valid_manifest() {
        let good = parse_manifest(
            "g",
            "id = \"g\"\nname = \"G\"\nversion = \"1.0.0\"\ncapabilities = [\"fs.read\"]\n",
        );
        assert!(is_capability_granted(&good, "fs.read"));
        assert!(!is_capability_granted(&good, "fs.write"));
        let bad = parse_manifest("b", "id = \"b\"\nname = \"B\"\nversion = \"1.0.0\"\ncapabilities = [\"fs.read\", \"nope\"]\n");
        assert!(
            !is_capability_granted(&bad, "fs.read"),
            "无效清单一律无能力"
        );
    }

    #[test]
    fn list_scans_workspace_dir() {
        let dir = std::env::temp_dir().join(format!("ext_list_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reg = registry_for(&dir);

        // 空扩展目录 = 空列表，不是错误
        let root =
            crate::commands::authorize_workspace_path(&reg, dir.to_str().unwrap(), false).unwrap();
        assert!(list_extensions_impl(&root.join(EXTENSIONS_DIR)).is_empty());

        write_ext(&dir, "good", Some("id = \"good\"\nname = \"Good\"\nversion = \"2.3.4\"\ncapabilities = [\"settings.read\"]\n"));
        write_ext(&dir, "nocfg", None);
        let list = list_extensions_impl(&root.join(EXTENSIONS_DIR));
        assert_eq!(list.len(), 2, "两个目录都应列出: {list:?}");
        assert_eq!(list[0].dir, "good");
        assert!(list[0].valid);
        assert_eq!(list[1].dir, "nocfg");
        assert!(!list[1].valid, "缺清单的目录应标无效而不是跳过");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
