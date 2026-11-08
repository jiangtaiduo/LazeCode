use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Files,
    Git,
    Changes,
    Terminal,
    Search,
    Plus,
    X,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    Sun,
    Moon,
    Sidebar,
    Panel,
    Check,
    Stop,
    Play,
    Refresh,
    Folder,
    FolderOpen,
    File,
    Copy,
    More,
    Spark,
    GitLog,
    Commit,
    Clock,
    ArrowUp,
    ArrowDown,
    ArrowRight,
    Tag,
    Layers,
    Book,
    Info,
    Alert,
    Error,
    Maximize,
    Minimize,
    Worktree,
    External,
    Settings,
    Trash,
    Filter,
    Dot,
    Branch,
    History,
    Save,
    Undo,
    Redo,
    Star,
    Diff,
    ExpandAll,
    CollapseAll,
    DoubleChevronLeft,
    DoubleChevronRight,
    Merge,
    Compare,
}

/// Map an icon to its stable-app SVG source ("icons_svg::SVGS").
/// `bool` = multicolor (brand palette baked in, draw without tint).
/// Icons without a stable counterpart fall back to the hand-drawn vectors.
fn svg_of(icon: Icon) -> Option<(&'static str, bool)> {
    Some(match icon {
        Icon::Files => ("project", false),
        Icon::Git | Icon::Branch => ("branch", false),
        Icon::Changes | Icon::Commit => ("commit", false),
        Icon::Terminal => ("terminal", false),
        Icon::Search => ("search", false),
        Icon::Plus => ("plus", false),
        Icon::X => ("close_small", false),
        Icon::ChevronDown => ("chevron_down", false),
        Icon::ChevronRight => ("chevron_right", false),
        Icon::ChevronUp => ("chevron_up", false),
        Icon::Panel => ("dock_bottom", false),
        Icon::Play => ("play", false),
        Icon::Refresh => ("refresh", false),
        Icon::Folder => ("folder", true),
        Icon::FolderOpen => ("folder_open", true),
        Icon::File => ("text_file", false),
        Icon::Copy => ("copy", false),
        Icon::More => ("more_vert", false),
        Icon::Spark => ("magic_wand", false),
        Icon::GitLog => ("gitlog", false),
        Icon::Clock | Icon::History => ("history", false),
        Icon::ArrowUp => ("arrow_up", false),
        Icon::ArrowDown => ("arrow_down", false),
        Icon::ArrowRight => ("arrow_right", false),
        Icon::Tag => ("tag", false),
        Icon::Alert => ("warning", false),
        Icon::Maximize => ("window_max", false),
        Icon::Minimize => ("window_min", false),
        Icon::External => ("new_window", false),
        Icon::Settings => ("gear", false),
        Icon::Trash => ("trash", false),
        Icon::Undo => ("rollback", false),
        Icon::Star => ("star", false),
        Icon::Diff => ("diff", false),
        Icon::ExpandAll => ("expand_all", false),
        Icon::CollapseAll => ("collapse_all", false),
        Icon::DoubleChevronLeft => ("double_chevron_left", false),
        Icon::DoubleChevronRight => ("double_chevron_right", false),
        Icon::Merge => ("merge", false),
        Icon::Compare => ("compare", false),
        _ => return None,
    })
}

/// Draw an icon scaled into `rect`. Stable-extracted SVGs take priority;
/// the remaining few use the hand-painted 16×16 vectors below.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    if let Some((name, _)) = svg_of(icon) {
        crate::icons_svg::paint_svg(painter, rect, name, color);
        return;
    }
    paint_vector(painter, rect, icon, color);
}

/// Hand-drawn fallbacks for icons the stable frontend has no SVG for
/// (theme toggles, worktrees, tree dots…).
fn paint_vector(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width().min(rect.height()) / 16.0;
    let origin = rect.center() - Vec2::splat(8.0 * scale);
    let p = |x: f32, y: f32| Pos2::new(origin.x + x * scale, origin.y + y * scale);
    let stroke = Stroke::new(1.45 * scale.max(0.75), color);
    let line = |points: &[(f32, f32)]| {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| p(*x, *y)).collect();
        painter.add(egui::Shape::line(points, stroke));
    };
    let seg = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
    };
    let circle = |c: (f32, f32), r: f32| {
        painter.circle_stroke(p(c.0, c.1), r * scale, stroke);
    };
    let dot = |c: (f32, f32), r: f32| {
        painter.circle_filled(p(c.0, c.1), r * scale, color);
    };
    let rect_outline = |min: (f32, f32), max: (f32, f32), radius: f32| {
        painter.rect_stroke(
            Rect::from_min_max(p(min.0, min.1), p(max.0, max.1)),
            egui::CornerRadius::same((radius * scale) as u8),
            stroke,
            StrokeKind::Middle,
        );
    };

    match icon {
        Icon::Sun => {
            circle((8.0, 8.0), 3.0);
            for (dx, dy) in [(0.0, -6.4), (0.0, 6.4), (-6.4, 0.0), (6.4, 0.0)] {
                seg((8.0 + dx * 0.75, 8.0 + dy * 0.75), (8.0 + dx, 8.0 + dy));
            }
            seg((3.5, 3.5), (4.5, 4.5));
            seg((11.5, 11.5), (12.5, 12.5));
            seg((12.5, 3.5), (11.5, 4.5));
            seg((4.5, 11.5), (3.5, 12.5));
        }
        Icon::Moon => {
            let mut points = Vec::new();
            for step in 0..24 {
                let angle = (110.0 + (step as f32) * (230.0 / 24.0)).to_radians();
                points.push(p(8.0 + 5.2 * angle.cos(), 8.0 + 5.2 * angle.sin()));
            }
            painter.add(egui::Shape::line(points, stroke));
        }
        Icon::Sidebar => {
            rect_outline((1.8, 2.4), (14.2, 13.6), 2.0);
            line(&[(6.1, 2.4), (6.1, 13.6)]);
        }
        Icon::Check => line(&[(3.4, 8.4), (6.4, 11.4), (12.6, 4.6)]),
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_min_max(p(4.4, 4.4), p(11.6, 11.6)),
                egui::CornerRadius::same((1.2 * scale) as u8),
                color,
            );
        }
        Icon::Layers => {
            line(&[(8.0, 1.9), (14.0, 5.0), (8.0, 8.1), (2.0, 5.0), (8.0, 1.9)]);
            line(&[(2.0, 8.0), (8.0, 11.1), (14.0, 8.0)]);
            line(&[(2.0, 11.0), (8.0, 14.1), (14.0, 11.0)]);
        }
        Icon::Book => {
            line(&[
                (2.2, 3.1),
                (6.3, 3.1),
                (7.6, 4.2),
                (7.6, 13.4),
                (6.2, 12.4),
                (2.2, 12.4),
                (2.2, 3.1),
            ]);
            line(&[
                (13.8, 3.1),
                (9.7, 3.1),
                (8.4, 4.2),
                (8.4, 13.4),
                (9.8, 12.4),
                (13.8, 12.4),
                (13.8, 3.1),
            ]);
        }
        Icon::Info => {
            circle((8.0, 8.0), 6.0);
            seg((8.0, 7.3), (8.0, 11.3));
            dot((8.0, 4.9), 0.55);
        }
        Icon::Error => {
            circle((8.0, 8.0), 6.0);
            seg((5.8, 5.8), (10.2, 10.2));
            seg((10.2, 5.8), (5.8, 10.2));
        }
        Icon::Worktree => {
            rect_outline((1.8, 1.8), (7.0, 7.0), 1.3);
            rect_outline((9.0, 9.0), (14.2, 14.2), 1.3);
            line(&[(4.4, 7.0), (4.4, 9.1)]);
            line(&[(4.4, 9.1), (6.7, 9.1), (9.0, 9.1)]);
        }
        Icon::Filter => {
            line(&[
                (2.0, 3.0),
                (14.0, 3.0),
                (9.4, 8.4),
                (9.4, 13.4),
                (6.6, 12.0),
                (6.6, 8.4),
                (2.0, 3.0),
            ]);
        }
        Icon::Dot => dot((8.0, 8.0), 3.0),
        Icon::Save => {
            line(&[
                (2.4, 2.4),
                (11.0, 2.4),
                (13.6, 5.0),
                (13.6, 13.6),
                (2.4, 13.6),
                (2.4, 2.4),
            ]);
            rect_outline((5.2, 2.4), (10.4, 6.2), 0.6);
            rect_outline((4.4, 9.0), (11.6, 13.6), 0.6);
        }
        Icon::Redo => {
            line(&[
                (13.0, 6.0),
                (8.6, 6.0),
                (5.6, 6.8),
                (4.0, 8.6),
                (4.4, 11.0),
                (6.2, 12.8),
                (9.6, 13.0),
            ]);
            line(&[(13.0, 6.0), (9.8, 2.8)]);
            line(&[(13.0, 6.0), (9.8, 9.2)]);
        }
        _ => {}
    }
}

/// Paint an icon into a square of `size` at `center`.
pub fn paint_centered(painter: &Painter, center: Pos2, size: f32, icon: Icon, color: Color32) {
    paint(
        painter,
        Rect::from_center_size(center, Vec2::splat(size)),
        icon,
        color,
    );
}
