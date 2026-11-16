//! Installed-font discovery for the Settings pickers, via `fontdb` (covers
//! fonts inside `AssetsV2` and `.ttc` faces).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use egui::FontData;

/// One selectable family, pointing at its regular face.
#[derive(Clone, Debug)]
pub struct FontFace {
    pub family: String,
    pub path: PathBuf,
    pub index: u32,
    pub monospaced: bool,
}

/// A memory-mapped font file. egui shares the mapping; no font bytes are
/// copied into the process heap (PingFang.ttc alone is ~75 MiB).
struct Mapped(memmap2::Mmap);

impl AsRef<[u8]> for Mapped {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

static CATALOG: LazyLock<Vec<FontFace>> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    // Pick the regular, normal-width face when a family ships several
    // weights/styles. Lower score wins.
    fn score(face: &fontdb::FaceInfo) -> (u8, u16, u8) {
        let style = if matches!(face.style, fontdb::Style::Normal) {
            0
        } else {
            1
        };
        let weight = face.weight.0.abs_diff(400);
        let stretch = if matches!(face.stretch, fontdb::Stretch::Normal) {
            0
        } else {
            1
        };
        (style, weight, stretch)
    }

    let mut best: HashMap<String, (FontFace, (u8, u16, u8))> = HashMap::new();
    for face in db.faces() {
        let Some((raw_family, _)) = face.families.first() else {
            continue;
        };
        // CoreText hides these behind internal names; surface the names
        // users see in Font Book.
        let family = match raw_family.as_str() {
            ".SF NS Mono" => "SF Mono".to_string(),
            "System Font" => "SF Pro".to_string(),
            _ if raw_family.is_empty() || raw_family.starts_with('.') => continue,
            _ => raw_family.clone(),
        };
        let path = match &face.source {
            fontdb::Source::File(path) => path.clone(),
            fontdb::Source::SharedFile(path, _) => path.clone(),
            fontdb::Source::Binary(_) => continue,
        };
        let candidate = FontFace {
            family: family.clone(),
            path,
            index: face.index,
            monospaced: face.monospaced || family == "SF Mono",
        };
        let key = family.to_lowercase();
        let candidate_score = score(face);
        match best.get(&key) {
            Some((_, existing)) if *existing <= candidate_score => {}
            _ => {
                best.insert(key, (candidate, candidate_score));
            }
        }
    }

    let mut faces: Vec<FontFace> = best.into_values().map(|(face, _)| face).collect();
    faces.sort_by_cached_key(|face| face.family.to_lowercase());
    faces
});

fn catalog() -> &'static [FontFace] {
    &CATALOG
}

/// All installed families, sorted case-insensitively.
pub fn families(monospaced_only: bool) -> Vec<&'static str> {
    catalog()
        .iter()
        .filter(|face| !monospaced_only || face.monospaced)
        .map(|face| face.family.as_str())
        .collect()
}

/// Resolve a saved family name to its regular face.
pub fn face(family: &str) -> Option<&'static FontFace> {
    let family = family.trim();
    if family.is_empty() {
        return None;
    }
    catalog()
        .iter()
        .find(|face| face.family.eq_ignore_ascii_case(family))
}

/// Load a face without copying it into the heap.
///
/// # Safety
/// Read-only mapping of system font assets.
pub fn load(face: &FontFace) -> Option<FontData> {
    load_path(&face.path, face.index)
}

/// Load any font file via `FontData::from_blob`, keeping the mapping file-backed
/// (the 184 MiB emoji collection stays out of the heap).
pub fn load_path(path: &std::path::Path, index: u32) -> Option<FontData> {
    let file = std::fs::File::open(path).ok()?;
    let mmap = unsafe { memmap2::Mmap::map(&file).ok()? };
    Some(FontData::from_blob(Arc::new(Mapped(mmap)), index))
}
