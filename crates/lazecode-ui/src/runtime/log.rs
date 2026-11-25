//! Derived Git-log cache (filtered list, lane layout, author chips).

/// Derived Git-log view: the filtered commit list, its lane layout and the
/// author-chip list. Recomputing all three per frame dominated the log view's
/// frame time (a regex filter recompiled the pattern per commit and the lane
/// layout cloned lane strings per row), so they are cached until the log, the
/// filter or the graph toggle changes.
pub struct GitLogCache {
    /// `project index | log generation | filter fingerprint | show graph`.
    pub key: String,
    pub filtered: Vec<lazecode_git::CommitInfo>,
    pub dag: lazecode_git::DagLayout,
    pub authors: Vec<String>,
}
