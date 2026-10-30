//! 后端错误码协议。
//!
//! # 为什么需要
//!
//! 后端（Rust）与前端（React）各自持有界面语言，而错误串原先在后端硬编码
//! 中文：`Err(format!("路径不存在: {}", path))`。前端拿到后套进自己语言的模板
//! （`showStatus(t().status.readDirFailed(msg))`），结果是
//! 「Failed to read directory: 路径不存在: /x」—— 两种语言混在一句里。
//!
//! # 协议
//!
//! 后端返回的错误串可以是两种形态：
//!
//! 1. **结构化**：`ERR:<code>|<detail>`，例如 `ERR:path_not_found|/x/y`
//!    - `<code>` 是稳定 ASCII 标识，前端据此查表得到当前语言的模板
//!    - `<detail>` 是**未经翻译的**上下文（路径、名称、系统错误原文），
//!      前端原样插入模板。它可能是中文（来自 git 原生输出），那是数据不是文案
//! 2. **纯文本**：不带 `ERR:` 前缀的串。前端原样显示（兜底，不猜内容）
//!
//! 判定只认前缀 `ERR:`，因此普通错误文本不可能被误判 —— 只有当文本以
//! `ERR:` 开头且含 `|` 时才结构化。若 detail 里含 `|`，取第一个 `|` 之后
//! 的全部内容，不 split。
//!
//! # 边界
//!
//! - 本模块只管「把后端错误串变成前端可用的 (code, detail)」。
//!   code -> 当前语言文案的映射在 `errors.ts`（前端字典）。
//! - 不改后端任何既有行为：只是把错误串的**格式**从「拼好的句子」换成
//!   「码 + 上下文」。前端在拿到非结构化串时行为与今天完全一致。

/// 结构化错误的前缀。
pub const ERROR_PREFIX: &str = "ERR:";

/// 解析后的后端错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// 结构化错误：稳定码 + 未翻译上下文。
    Coded { code: String, detail: String },
    /// 纯文本错误：原样展示。
    Raw(String),
}

impl BackendError {
    /// 供前端查表用的码；纯文本错误返回 `None`（前端走兜底显示）。
    pub fn code(&self) -> Option<&str> {
        match self {
            BackendError::Coded { code, .. } => Some(code),
            BackendError::Raw(_) => None,
        }
    }

    /// 未翻译上下文（路径 / 名称 / 系统错误原文）。纯文本错误返回其自身。
    pub fn detail(&self) -> &str {
        match self {
            BackendError::Coded { detail, .. } => detail,
            BackendError::Raw(s) => s,
        }
    }

    /// 原始串（用于日志 / 调试：日志不应依赖协议格式）。
    pub fn raw(&self) -> String {
        match self {
            BackendError::Coded { code, detail } => format!("{ERROR_PREFIX}{code}|{detail}"),
            BackendError::Raw(s) => s.clone(),
        }
    }
}

/// 解析后端错误串。
///
/// 只在**恰好**以 `ERR:` 开头且含 `|` 时结构化；其余一律 `Raw`。
/// 这意味着即使某个后端错误文本真的以 "ERR:" 开头（例如用户提供的路径），
/// 只要不含 `|` 就仍是 Raw；含 `|` 时前段不是已知码，前端查表会 miss
/// 并回落 Raw —— 见 errors.ts 的兜底顺序。
pub fn parse_error(text: &str) -> BackendError {
    if let Some(rest) = text.strip_prefix(ERROR_PREFIX) {
        if let Some(idx) = rest.find('|') {
            let code = &rest[..idx];
            let detail = &rest[idx + 1..];
            // 只认 ASCII 码：非 ASCII 说明这不是协议产生的串
            if !code.is_empty() && code.is_ascii() && !code.contains(char::is_whitespace) {
                // 码表在前端字典，这里只做格式判定；未知码由前端回落。
                return BackendError::Coded {
                    code: code.to_string(),
                    detail: detail.to_string(),
                };
            }
        }
    }
    BackendError::Raw(text.to_string())
}

/// 构造结构化错误串（后端调用点用）。
///
/// `detail` 用 `AsRef<str>`：调用点既有 `&str` 也有 `String`（例如
/// `dir: String` 的命令参数），强制 `&` 会在几十个调用点重复造噪音。
///
/// ```ignore
/// return Err(coded("path_not_found", path));
/// ```
pub fn coded(code: &str, detail: impl AsRef<str>) -> String {
    format!("{ERROR_PREFIX}{code}|{}", detail.as_ref())
}

/// 构造结构化错误串，detail 来自 `Display`（系统错误原文）。
pub fn coded_display(code: &str, detail: impl std::fmt::Display) -> String {
    coded(code, detail.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_error() {
        let e = parse_error("ERR:path_not_found|/x/y");
        assert_eq!(e.code(), Some("path_not_found"));
        assert_eq!(e.detail(), "/x/y");
        assert_eq!(e.raw(), "ERR:path_not_found|/x/y");
    }

    /// detail 含 `|` 时不能截断：路径或 git 输出里都可能有。
    #[test]
    fn keeps_pipes_in_detail() {
        let e = parse_error("ERR:git_failed|error: a | b | c");
        assert_eq!(e.code(), Some("git_failed"));
        assert_eq!(e.detail(), "error: a | b | c");
    }

    /// 普通中文错误串必须原样通过，不能被误判成协议。
    #[test]
    fn plain_text_is_raw() {
        for s in [
            "路径不存在: /x",
            "读取失败",
            "ERR:",
            "ERR:no_pipe",
            "ERR: 有空格 | detail",
            "ERR:中文码|detail",
        ] {
            let e = parse_error(s);
            assert_eq!(e.code(), None, "{s} 不应被结构化");
            assert_eq!(e.detail(), s, "{s} detail 应为自身");
        }
    }

    /// 空 detail 合法：某些错误不需要上下文。
    #[test]
    fn empty_detail_is_allowed() {
        let e = parse_error("ERR:commit_message_empty|");
        assert_eq!(e.code(), Some("commit_message_empty"));
        assert_eq!(e.detail(), "");
    }

    /// coded/coded_display 与 parse 互为逆运算。
    #[test]
    fn round_trips() {
        let s = coded("not_a_dir", "/tmp");
        assert_eq!(parse_error(&s).detail(), "/tmp");
        let s2 = coded_display("io_error", std::io::Error::other("boom"));
        assert_eq!(parse_error(&s2).detail(), "boom");
    }
}
