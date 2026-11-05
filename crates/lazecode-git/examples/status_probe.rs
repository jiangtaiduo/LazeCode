use std::collections::BTreeMap;
fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let snap = lazecode_git::snapshot(std::path::Path::new(&root)).unwrap();
    println!("changes={}", snap.changes.len());
    let mut groups: BTreeMap<&str, usize> = BTreeMap::new();
    for change in &snap.changes {
        let first = change.path.split('/').next().unwrap_or("");
        *groups.entry(first).or_default() += 1;
    }
    for (group, count) in groups {
        println!("{count}\t{group}");
    }
}
