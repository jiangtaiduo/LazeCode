//! PyCharm-style log reference labels.
//!
//! The commit table never appends one badge per reference. Each row shows a
//! single compact label — the name of the first reference group (a local
//! branch and its tracking remote merge into `origin & main`) — preceded by a
//! fanned stack of label icons that only hints at the other references.
//! Hovering the label lists every reference on its own line: HEAD, local
//! branches, remotes, then tags.
//!
//! Mirrors IntelliJ's `GitRefManager.groupForTable` compact branch and
//! `LabelPainter`/`LabelIcon` rendering: the label width is capped at a third
//! of the available text area, and tag names stay behind the icons until the
//! tooltip opens.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Ui, Vec2};
use lazecode_core::Language;
use lazecode_git::{RefKind, RefLabel};

use crate::icons::{paint as paint_icon, Icon};
use crate::theme::Palette;

pub(crate) const REF_FONT_SIZE: f32 = 11.5;
const ICON_SIZE: f32 = 11.0;
const ICON_OFFSET: f32 = 3.0;
/// The fan is only a hint; the tooltip carries the complete list.
const MAX_STACK: usize = 6;
const ICON_TEXT_GAP: f32 = 5.0;
const TOOLTIP_LIMIT: usize = 10;

/// A commit's references, reduced to what the table row draws.
pub(crate) struct RefCluster {
    /// First group's name, shortened to the label budget (empty for tags only).
    pub text: String,
    /// Every reference in display order; the fan paints these reversed so the
    /// first one (HEAD) lands on top, like IntelliJ's stacked label icons.
    pub kinds: Vec<RefKind>,
    /// Complete reference list for the hover tooltip.
    pub rows: Vec<RefLabel>,
    pub icon_w: f32,
    pub width: f32,
}

/// Sort references the way the log renders them: HEAD, current branch, other
/// local branches, remotes, tags. The git layer already puts the checked-out
/// branch first among local branches (right after HEAD).
fn ordered_rows(refs: &[RefLabel]) -> Vec<RefLabel> {
    let mut rows: Vec<RefLabel> = Vec::with_capacity(refs.len());
    for kind in [
        RefKind::Head,
        RefKind::LocalBranch,
        RefKind::RemoteBranch,
        RefKind::Tag,
    ] {
        rows.extend(refs.iter().filter(|r| r.kind == kind).cloned());
    }
    rows
}

fn is_mainish(name: &str) -> bool {
    name == "main" || name == "master"
}

/// Local branches in the order the compact label considers them: the current
/// branch first when HEAD is on the commit (the git layer guarantees it),
/// otherwise `main`/`master` first.
fn local_order<'a>(locals: &[&'a RefLabel], head: bool) -> Vec<&'a RefLabel> {
    let mut ordered = locals.to_vec();
    if !head {
        ordered.sort_by_key(|label| !is_mainish(&label.label));
    }
    ordered
}

/// `remote/<branch>` paired with a local `<branch>` becomes PyCharm's
/// `remote & branch` label. The prefix must be a bare remote name, matching
/// IntelliJ's fallback of `remote.name + "/" + local.name`.
fn remote_prefix<'a>(remote: &'a str, local: &str) -> Option<&'a str> {
    let prefix = remote.strip_suffix(&format!("/{local}"))?;
    (!prefix.is_empty() && !prefix.contains('/')).then_some(prefix)
}

fn paired_remote<'a>(local: &RefLabel, remotes: &[&'a RefLabel]) -> Option<&'a RefLabel> {
    remotes
        .iter()
        .copied()
        .find(|remote| remote_prefix(&remote.label, &local.label).is_some())
}

/// The inline name: the first reference group, exactly like PyCharm's compact
/// view (which keeps only `refGroups.first().name`). Tag-only commits show no
/// name — their names live in the tooltip.
fn primary_name(refs: &[RefLabel]) -> String {
    let head = refs.iter().any(|r| r.kind == RefKind::Head);
    let locals: Vec<&RefLabel> = refs
        .iter()
        .filter(|r| r.kind == RefKind::LocalBranch)
        .collect();
    let remotes: Vec<&RefLabel> = refs
        .iter()
        .filter(|r| r.kind == RefKind::RemoteBranch)
        .collect();
    let ordered = local_order(&locals, head);

    if head {
        if let Some(local) = ordered.first() {
            return match paired_remote(local, &remotes) {
                Some(remote) => format!(
                    "{} & {}",
                    remote_prefix(&remote.label, &local.label).unwrap_or_default(),
                    local.label
                ),
                None => local.label.clone(),
            };
        }
        return "HEAD".to_string();
    }

    // Without HEAD, the first *tracked pair* outranks a lone branch.
    if let Some((local, remote)) = ordered
        .iter()
        .find_map(|local| paired_remote(local, &remotes).map(|remote| (*local, remote)))
    {
        if let Some(prefix) = remote_prefix(&remote.label, &local.label) {
            return format!("{prefix} & {}", local.label);
        }
    }
    if let Some(local) = ordered.first() {
        return local.label.clone();
    }
    if let Some(remote) = remotes.first() {
        return remote.label.clone();
    }
    String::new()
}

/// Shorten like IntelliJ: first replace the part before the first `/` with
/// `..` (so long `remote/feature/name` refs keep their meaningful tail), then
/// trim to the budget with an ellipsis.
pub(crate) fn shorten_label(name: &str, max_w: f32, measure: impl Fn(&str) -> f32) -> String {
    if name.is_empty() || max_w <= 0.0 {
        return String::new();
    }
    if measure(name) <= max_w {
        return name.to_string();
    }
    // A tracked pair is `remote & branch`; when the pair does not fit, keep
    // the branch name and drop the remote prefix (the tooltip still shows both).
    let mut candidate = name;
    if let Some((_, tail)) = name.rsplit_once(" & ") {
        if !tail.is_empty() {
            if measure(tail) <= max_w {
                return tail.to_string();
            }
            candidate = tail;
        }
    }
    let collapsed = match candidate.split_once('/') {
        Some((prefix, rest)) if prefix.len() > 2 => format!("..{}{rest}", '/'),
        _ => candidate.to_string(),
    };
    if measure(&collapsed) <= max_w {
        return collapsed;
    }
    let ellipsis = "…";
    if measure(ellipsis) > max_w {
        return String::new();
    }
    let mut fitted = String::new();
    for ch in collapsed.chars() {
        let mut probe = fitted.clone();
        probe.push(ch);
        probe.push_str(ellipsis);
        if measure(&probe) > max_w {
            break;
        }
        fitted.push(ch);
    }
    fitted.push_str(ellipsis);
    fitted
}

/// Build the compact label for `refs`, capped at `max_w` pixels.
pub(crate) fn prepare(painter: &Painter, refs: &[RefLabel], max_w: f32) -> Option<RefCluster> {
    if refs.is_empty() || max_w <= 0.0 {
        return None;
    }
    let rows = ordered_rows(refs);
    let kinds: Vec<RefKind> = rows.iter().map(|r| r.kind).collect();
    let stack = kinds.len().min(MAX_STACK);
    let icon_w = if stack == 0 {
        0.0
    } else {
        ICON_SIZE + ICON_OFFSET * (stack - 1) as f32
    };
    if max_w < icon_w {
        return None;
    }
    let font = FontId::proportional(REF_FONT_SIZE);
    let color = Color32::WHITE; // only metrics matter before painting
    let measure = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), color)
            .size()
            .x
    };
    let full = primary_name(refs);
    let text = shorten_label(&full, max_w - icon_w - ICON_TEXT_GAP, measure);
    let text_w = if text.is_empty() { 0.0 } else { measure(&text) };
    let width = if text_w > 0.0 {
        icon_w + ICON_TEXT_GAP + text_w
    } else {
        icon_w
    };
    Some(RefCluster {
        text,
        kinds,
        rows,
        icon_w,
        width,
    })
}

pub(crate) fn icon_color(kind: RefKind, p: &Palette) -> Color32 {
    match kind {
        // IntelliJ: yellow marks the current branch head, green local
        // branches, violet remotes, gray tags.
        RefKind::Head => p.amber,
        RefKind::LocalBranch => p.green,
        RefKind::RemoteBranch => p.violet,
        RefKind::Tag => p.text2,
    }
}

/// Draw the label at `x` and attach the all-references tooltip to it.
pub(crate) fn paint(
    ui: &mut Ui,
    language: Language,
    p: &Palette,
    row: Rect,
    x: f32,
    cluster: &RefCluster,
    id: egui::Id,
) {
    let painter = ui.painter();
    let center_y = row.center().y;
    for (index, kind) in cluster.kinds.iter().take(MAX_STACK).rev().enumerate() {
        paint_icon(
            painter,
            Rect::from_center_size(
                Pos2::new(x + index as f32 * ICON_OFFSET + ICON_SIZE / 2.0, center_y),
                Vec2::splat(ICON_SIZE),
            ),
            Icon::Tag,
            icon_color(*kind, p),
        );
    }
    if !cluster.text.is_empty() {
        painter.text(
            Pos2::new(x + cluster.icon_w + ICON_TEXT_GAP, center_y),
            Align2::LEFT_CENTER,
            &cluster.text,
            FontId::proportional(REF_FONT_SIZE),
            p.text2,
        );
    }

    let hit = Rect::from_min_max(
        Pos2::new(x, row.top()),
        Pos2::new(x + cluster.width, row.bottom()),
    );
    let response = ui.interact(hit, id, Sense::hover());
    // The log usually sits in the bottom panel, where a tooltip anchored below
    // the row would be clipped by the window edge. Pick the side with room,
    // like PyCharm does when its log touches the bottom of the screen.
    let estimated_h = cluster.rows.len().min(TOOLTIP_LIMIT) as f32 * 21.0 + 20.0;
    let room_below = ui.ctx().content_rect().bottom() - response.rect.bottom();
    let align = if room_below >= estimated_h + 24.0 {
        egui::RectAlign::BOTTOM_START
    } else {
        egui::RectAlign::TOP_START
    };
    let mut tooltip = egui::Tooltip::for_enabled(&response);
    tooltip.popup = tooltip.popup.align(align);
    tooltip.show(|ui| {
        ui.set_max_width(360.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        for label in cluster.rows.iter().take(TOOLTIP_LIMIT) {
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                paint_icon(
                    ui.painter(),
                    icon_rect,
                    Icon::Tag,
                    icon_color(label.kind, p),
                );
                ui.label(egui::RichText::new(&label.label).size(12.0));
            });
        }
        let rest = cluster.rows.len().saturating_sub(TOOLTIP_LIMIT);
        if rest > 0 {
            let more = crate::i18n::t(language, "and {} more", "另有 {} 个")
                .replace("{}", &rest.to_string());
            ui.label(egui::RichText::new(more).size(12.0).color(p.text3));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str, kind: RefKind) -> RefLabel {
        RefLabel {
            label: name.to_string(),
            kind,
        }
    }

    fn head() -> RefLabel {
        label("HEAD", RefKind::Head)
    }

    #[test]
    fn head_branch_and_tracked_remote_form_one_group() {
        let refs = vec![
            head(),
            label("main", RefKind::LocalBranch),
            label("origin/main", RefKind::RemoteBranch),
        ];
        assert_eq!(primary_name(&refs), "origin & main");
        let rows = ordered_rows(&refs);
        let names: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(names, ["HEAD", "main", "origin/main"]);
    }

    #[test]
    fn head_branch_without_upstream_shows_branch_name_not_head() {
        // PyCharm names the label after the branch (yellow icon marks HEAD)
        // and lists HEAD separately in the tooltip.
        let refs = vec![head(), label("feature/a", RefKind::LocalBranch)];
        assert_eq!(primary_name(&refs), "feature/a");
    }

    #[test]
    fn current_branch_wins_over_main_when_head_is_on_the_commit() {
        // The git layer keeps the checked-out branch first among locals.
        let refs = vec![
            head(),
            label("feature/a", RefKind::LocalBranch),
            label("main", RefKind::LocalBranch),
        ];
        assert_eq!(primary_name(&refs), "feature/a");
    }

    #[test]
    fn without_head_the_first_tracked_pair_leads() {
        let refs = vec![
            label("dev", RefKind::LocalBranch),
            label("main", RefKind::LocalBranch),
            label("origin/main", RefKind::RemoteBranch),
        ];
        assert_eq!(primary_name(&refs), "origin & main");
    }

    #[test]
    fn without_head_a_lone_main_outranks_other_locals() {
        let refs = vec![
            label("dev", RefKind::LocalBranch),
            label("main", RefKind::LocalBranch),
        ];
        assert_eq!(primary_name(&refs), "main");
    }

    #[test]
    fn remote_only_commits_use_the_remote_name() {
        let refs = vec![label("origin/main", RefKind::RemoteBranch)];
        assert_eq!(primary_name(&refs), "origin/main");
    }

    #[test]
    fn tag_only_commits_hide_the_inline_name() {
        let refs = vec![label("v1.0", RefKind::Tag), label("v2.0", RefKind::Tag)];
        assert_eq!(primary_name(&refs), "");
        let rows = ordered_rows(&refs);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.kind == RefKind::Tag));
    }

    #[test]
    fn shallow_remote_prefix_never_pairs_nested_remote_refs() {
        // `origin/feature/a` must not pair with a local branch literally
        // named `a`; only bare remote names count.
        let refs = vec![
            label("a", RefKind::LocalBranch),
            label("origin/feature/a", RefKind::RemoteBranch),
        ];
        assert_eq!(primary_name(&refs), "a");
    }

    #[test]
    fn long_labels_collapse_the_remote_prefix_then_ellipsize() {
        let measure = |s: &str| s.chars().count() as f32 * 6.0;
        // Fits as-is.
        assert_eq!(
            shorten_label("origin/feature/a", 200.0, measure),
            "origin/feature/a"
        );
        // Too long: prefix before the first slash becomes `..`.
        let short = shorten_label("origin/feature/really-long-name", 120.0, measure);
        assert!(short.starts_with(".."), "{short}");
        assert!(measure(&short) <= 120.0, "{short}");
        assert!(short.ends_with('…'), "{short}");
        // Paired labels keep the branch name when the pair does not fit.
        assert_eq!(shorten_label("origin & main", 60.0, measure), "main");
        // No slash: plain ellipsis.
        let plain = shorten_label("averylongbranchname", 60.0, measure);
        assert_eq!(plain, "averylong…");
    }

    #[test]
    fn prepare_refuses_a_label_without_room() {
        let ctx = egui::Context::default();
        let refs = vec![head(), label("main", RefKind::LocalBranch)];
        let mut tiny = None;
        let mut cluster = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            tiny = prepare(ui.painter(), &refs, 4.0);
            cluster = prepare(ui.painter(), &refs, 120.0);
        });
        output.textures_delta.clear();
        assert!(tiny.is_none());
        let cluster = cluster.expect("label");
        assert_eq!(cluster.text, "main");
        assert!(cluster.width <= 120.0);
    }
}
