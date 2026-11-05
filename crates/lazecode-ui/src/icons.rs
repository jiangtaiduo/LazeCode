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
}

/// Draw a 16×16 stroke icon scaled into `rect`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width().min(rect.height()) / 16.0;
    let origin = rect.center() - Vec2::splat(8.0 * scale);
    let p = |x: f32, y: f32| Pos2::new(origin.x + x * scale, origin.y + y * scale);
    let stroke = Stroke::new(1.45 * scale.max(0.75), color);
    let thin = Stroke::new(1.2 * scale.max(0.75), color);
    let line = |points: &[(f32, f32)]| {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| p(*x, *y)).collect();
        painter.add(egui::Shape::line(points, stroke));
    };
    let seg = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], stroke);
    };
    let seg_thin = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], thin);
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
        Icon::Files => {
            rect_outline((5.2, 1.8), (13.2, 12.0), 1.6);
            line(&[(3.2, 4.4), (3.2, 12.5)]);
            line(&[(3.2, 12.5), (4.9, 14.2), (10.8, 14.2)]);
        }
        Icon::Git | Icon::Branch => {
            circle((4.6, 3.6), 1.6);
            circle((4.6, 12.4), 1.6);
            circle((11.4, 5.6), 1.6);
            line(&[(4.6, 5.2), (4.6, 10.8)]);
            line(&[(11.4, 7.2), (11.4, 8.6)]);
            line(&[(11.4, 8.6), (8.1, 8.6)]);
            line(&[(8.1, 8.6), (8.1, 11.9), (6.2, 11.9)]);
        }
        Icon::Changes => {
            line(&[(2.3, 3.2), (8.4, 3.2)]);
            line(&[(2.3, 8.0), (8.4, 8.0)]);
            line(&[(2.3, 12.8), (8.4, 12.8)]);
            line(&[(12.4, 2.0), (12.4, 4.6)]);
            line(&[(11.1, 3.3), (13.7, 3.3)]);
            dot((12.4, 8.0), 0.85);
            line(&[(11.1, 12.5), (13.7, 12.5)]);
        }
        Icon::Terminal => {
            rect_outline((1.8, 2.6), (14.2, 13.4), 2.0);
            line(&[(4.6, 6.2), (6.9, 8.3), (4.6, 10.4)]);
            line(&[(8.5, 10.6), (11.5, 10.6)]);
        }
        Icon::Search => {
            circle((6.8, 6.8), 4.1);
            seg((9.9, 9.9), (13.5, 13.5));
        }
        Icon::Plus => {
            seg((8.0, 2.8), (8.0, 13.2));
            seg((2.8, 8.0), (13.2, 8.0));
        }
        Icon::X => {
            seg((4.3, 4.3), (11.7, 11.7));
            seg((11.7, 4.3), (4.3, 11.7));
        }
        Icon::ChevronDown => line(&[(4.0, 6.0), (8.0, 10.0), (12.0, 6.0)]),
        Icon::ChevronRight => line(&[(6.0, 3.6), (10.4, 8.0), (6.0, 12.4)]),
        Icon::ChevronUp => line(&[(4.0, 10.0), (8.0, 6.0), (12.0, 10.0)]),
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
        Icon::Panel => {
            rect_outline((1.8, 2.4), (14.2, 13.6), 2.0);
            line(&[(1.8, 9.6), (14.2, 9.6)]);
        }
        Icon::Check => line(&[(3.4, 8.4), (6.4, 11.4), (12.6, 4.6)]),
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_min_max(p(4.4, 4.4), p(11.6, 11.6)),
                egui::CornerRadius::same((1.2 * scale) as u8),
                color,
            );
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![p(5.4, 3.6), p(12.0, 8.0), p(5.4, 12.4)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Refresh => {
            let mut points = Vec::new();
            for step in 0..20 {
                let angle = (-50.0 + step as f32 * 13.0).to_radians();
                points.push(p(8.0 + 5.2 * angle.cos(), 8.0 + 5.2 * angle.sin()));
            }
            painter.add(egui::Shape::line(points, stroke));
            line(&[(11.6, 2.6), (11.6, 5.6), (8.6, 5.6)]);
        }
        Icon::Folder => {
            line(&[
                (1.9, 5.1),
                (2.1, 4.2),
                (3.4, 3.6),
                (5.8, 3.6),
                (7.2, 5.3),
                (12.6, 5.3),
                (13.4, 6.1),
                (13.4, 11.2),
                (12.6, 12.0),
                (2.9, 12.0),
                (1.9, 11.2),
                (1.9, 5.1),
            ]);
        }
        Icon::FolderOpen => {
            line(&[
                (1.9, 11.9),
                (1.9, 5.1),
                (2.1, 4.2),
                (3.4, 3.6),
                (5.8, 3.6),
                (7.2, 5.3),
                (12.1, 5.3),
                (13.0, 6.0),
            ]);
            line(&[
                (2.1, 12.9),
                (4.0, 7.4),
                (15.0, 7.4),
                (13.0, 12.9),
                (2.1, 12.9),
            ]);
        }
        Icon::File => {
            line(&[
                (3.4, 2.2),
                (9.3, 2.2),
                (12.6, 5.5),
                (12.6, 13.8),
                (3.4, 13.8),
                (3.4, 2.2),
            ]);
            line(&[(9.1, 2.2), (9.1, 5.7), (12.6, 5.7)]);
        }
        Icon::Copy => {
            rect_outline((5.3, 5.3), (13.5, 13.5), 1.5);
            line(&[(10.7, 5.3), (10.7, 3.7)]);
            line(&[(10.7, 3.7), (9.3, 2.3)]);
            line(&[(9.3, 2.3), (3.7, 2.3)]);
            line(&[(3.7, 2.3), (2.3, 3.7)]);
            line(&[(2.3, 3.7), (2.3, 9.3)]);
            line(&[(2.3, 9.3), (3.7, 10.7)]);
            line(&[(3.7, 10.7), (5.3, 10.7)]);
        }
        Icon::More => {
            dot((3.4, 8.0), 1.15);
            dot((8.0, 8.0), 1.15);
            dot((12.6, 8.0), 1.15);
        }
        Icon::Spark => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    p(8.0, 1.6),
                    p(9.5, 6.5),
                    p(14.4, 8.0),
                    p(9.5, 9.5),
                    p(8.0, 14.4),
                    p(6.5, 9.5),
                    p(1.6, 8.0),
                    p(6.5, 6.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::GitLog => {
            line(&[(4.5, 2.2), (4.5, 13.8)]);
            circle((4.5, 4.1), 1.5);
            circle((4.5, 11.9), 1.5);
            circle((10.6, 8.0), 1.5);
            line(&[(6.0, 4.1), (7.6, 4.1)]);
            line(&[(7.6, 4.1), (10.6, 7.0)]);
            line(&[(6.0, 11.9), (7.6, 11.9)]);
            line(&[(7.6, 11.9), (10.6, 10.0)]);
        }
        Icon::Commit | Icon::History => {
            circle((8.0, 8.0), 2.5);
            seg((1.8, 8.0), (5.5, 8.0));
            seg((10.5, 8.0), (14.2, 8.0));
        }
        Icon::Clock => {
            circle((8.0, 8.0), 6.0);
            line(&[(8.0, 4.6), (8.0, 8.0), (10.4, 9.5)]);
        }
        Icon::ArrowUp => {
            seg((8.0, 13.0), (8.0, 3.0));
            line(&[(4.0, 6.8), (8.0, 2.8), (12.0, 6.8)]);
        }
        Icon::ArrowDown => {
            seg((8.0, 3.0), (8.0, 13.0));
            line(&[(4.0, 9.2), (8.0, 13.2), (12.0, 9.2)]);
        }
        Icon::ArrowRight => {
            seg((3.0, 8.0), (13.0, 8.0));
            line(&[(9.0, 4.0), (13.0, 8.0), (9.0, 12.0)]);
        }
        Icon::Tag => {
            line(&[
                (2.2, 7.3),
                (2.2, 2.7),
                (6.8, 2.7),
                (13.8, 9.7),
                (9.6, 13.9),
                (2.2, 7.3),
            ]);
            circle((5.1, 5.1), 1.0);
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
        Icon::Alert => {
            line(&[
                (7.0, 2.8),
                (1.8, 12.0),
                (2.9, 14.0),
                (13.1, 14.0),
                (14.2, 12.0),
                (9.0, 2.8),
                (7.0, 2.8),
            ]);
            seg((8.0, 6.3), (8.0, 9.7));
            dot((8.0, 12.0), 0.5);
        }
        Icon::Error => {
            circle((8.0, 8.0), 6.0);
            seg((5.8, 5.8), (10.2, 10.2));
            seg((10.2, 5.8), (5.8, 10.2));
        }
        Icon::Maximize => {
            line(&[(6.2, 2.0), (2.0, 2.0), (2.0, 6.2)]);
            line(&[(9.8, 2.0), (14.0, 2.0), (14.0, 6.2)]);
            line(&[(14.0, 9.8), (14.0, 14.0), (9.8, 14.0)]);
            line(&[(6.2, 14.0), (2.0, 14.0), (2.0, 9.8)]);
        }
        Icon::Minimize => {
            line(&[(6.0, 2.0), (6.0, 6.0), (2.0, 6.0)]);
            line(&[(10.0, 2.0), (10.0, 6.0), (14.0, 6.0)]);
            line(&[(14.0, 10.0), (10.0, 10.0), (10.0, 14.0)]);
            line(&[(6.0, 14.0), (6.0, 10.0), (2.0, 10.0)]);
        }
        Icon::Worktree => {
            rect_outline((1.8, 1.8), (7.0, 7.0), 1.3);
            rect_outline((9.0, 9.0), (14.2, 14.2), 1.3);
            line(&[(4.4, 7.0), (4.4, 9.1)]);
            line(&[(4.4, 9.1), (6.7, 9.1), (9.0, 9.1)]);
        }
        Icon::External => {
            line(&[(9.2, 1.8), (14.2, 1.8), (14.2, 6.8)]);
            line(&[(14.2, 1.8), (7.6, 8.4)]);
            line(&[
                (12.2, 9.1),
                (12.2, 13.3),
                (10.8, 14.7),
                (2.9, 14.7),
                (1.5, 13.3),
                (1.5, 5.4),
                (2.9, 4.0),
                (7.2, 4.0),
            ]);
        }
        Icon::Settings => {
            line(&[(2.5, 4.5), (13.5, 4.5)]);
            line(&[(2.5, 8.0), (13.5, 8.0)]);
            line(&[(2.5, 11.5), (13.5, 11.5)]);
            painter.circle_filled(p(6.0, 4.5), 1.4 * scale, color);
            painter.circle_filled(p(10.5, 8.0), 1.4 * scale, color);
            painter.circle_filled(p(4.75, 11.5), 1.4 * scale, color);
        }
        Icon::Trash => {
            line(&[(2.6, 4.0), (13.4, 4.0)]);
            line(&[(6.0, 4.0), (6.4, 2.4), (9.6, 2.4), (10.0, 4.0)]);
            line(&[(4.0, 4.0), (4.8, 13.4), (11.2, 13.4), (12.0, 4.0)]);
            seg_thin((7.0, 6.2), (7.0, 11.4));
            seg_thin((9.0, 6.2), (9.0, 11.4));
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
        Icon::Undo => {
            line(&[
                (3.0, 6.0),
                (7.4, 6.0),
                (10.4, 6.8),
                (12.0, 8.6),
                (11.6, 11.0),
                (9.8, 12.8),
                (6.4, 13.0),
            ]);
            line(&[(3.0, 6.0), (6.2, 2.8)]);
            line(&[(3.0, 6.0), (6.2, 9.2)]);
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
