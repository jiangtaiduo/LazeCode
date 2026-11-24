//! Three-way conflict editor (ours | editable result | theirs) shown in the
//! dedicated conflict dialog.

use egui::{Align2, FontId, Pos2, Rect, Stroke, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, ToastKind};

use crate::theme::Theme;
use crate::widgets::{primary_button, secondary_button};

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_conflict_tool(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    path: &mut String,
    ours: &mut String,
    theirs: &mut String,
    worktree: &mut String,
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let font = FontId::monospace(10.5);

    // Top action bar
    let header_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header_rect, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header_rect.x_range(),
        header_rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    ui.painter().text(
        Pos2::new(header_rect.left() + 16.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        format!(
            "⚡ {} — {}",
            if zh {
                "三方冲突解决"
            } else {
                "3-Way Merge Tool"
            },
            path
        ),
        FontId::proportional(12.0),
        p.text,
    );

    let mut btn_x = header_rect.right() - 16.0;

    // Close / Cancel
    btn_x -= 64.0;
    let cancel_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(60.0, 22.0),
    );
    if secondary_button(
        ui,
        cancel_rect,
        if zh { "取消" } else { "Cancel" },
        None,
        theme,
    )
    .clicked()
    {
        *close = true;
    }
    btn_x -= 8.0;

    // Mark as resolved button
    let count_conflicts = worktree.matches("<<<<<<<").count();
    btn_x -= 144.0;
    let save_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(140.0, 22.0),
    );
    let save_label = if count_conflicts == 0 {
        if zh {
            "✓ 保存并标记解决".to_string()
        } else {
            "✓ Save & Mark Resolved".to_string()
        }
    } else if zh {
        format!("标记解决 (剩余 {count_conflicts})")
    } else {
        format!("Resolve ({count_conflicts} left)")
    };
    if primary_button(
        ui,
        save_rect,
        &save_label,
        None,
        count_conflicts == 0,
        theme,
    )
    .clicked()
        && count_conflicts == 0
    {
        if let Some(active) = model.active_project {
            let root = model.projects[active].active_root().to_path_buf();
            match lazecode_git::mark_conflict_resolved(&root, path, Some(worktree)) {
                Ok(()) => {
                    model.queue(Command::ReloadGit);
                    model.queue(Command::RefreshLog);
                    model.toast(
                        if zh {
                            format!("已解决 {path}")
                        } else {
                            format!("Resolved {path}")
                        },
                        ToastKind::Info,
                    );
                    *close = true;
                }
                Err(error) => {
                    model.toast(
                        if zh {
                            format!("无法标记解决: {error}")
                        } else {
                            format!("Could not mark resolved: {error}")
                        },
                        ToastKind::Error,
                    );
                }
            }
        }
    }
    btn_x -= 8.0;

    btn_x -= 114.0;
    let theirs_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(110.0, 22.0),
    );
    if secondary_button(
        ui,
        theirs_rect,
        if zh {
            "全部采用对方 ▶"
        } else {
            "Accept Theirs ▶"
        },
        None,
        theme,
    )
    .clicked()
    {
        *worktree = theirs.clone();
    }
    btn_x -= 8.0;

    btn_x -= 114.0;
    let ours_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(110.0, 22.0),
    );
    if secondary_button(
        ui,
        ours_rect,
        if zh {
            "◀ 全部采用我方"
        } else {
            "◀ Accept Ours"
        },
        None,
        theme,
    )
    .clicked()
    {
        *worktree = ours.clone();
    }

    // Body: 3 panels
    let body_rect = Rect::from_min_max(Pos2::new(rect.left(), header_rect.bottom()), rect.max);
    let col_w = ((body_rect.width() - 4.0) / 3.0).floor();

    let left_rect = Rect::from_min_size(body_rect.min, Vec2::new(col_w, body_rect.height()));
    let mid_rect = Rect::from_min_size(
        Pos2::new(body_rect.min.x + col_w + 2.0, body_rect.min.y),
        Vec2::new(col_w, body_rect.height()),
    );
    let right_rect = Rect::from_min_size(
        Pos2::new(body_rect.min.x + (col_w * 2.0) + 4.0, body_rect.min.y),
        Vec2::new(col_w, body_rect.height()),
    );

    // Vertical dividers
    ui.painter().vline(
        left_rect.right() + 1.0,
        body_rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().vline(
        mid_rect.right() + 1.0,
        body_rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );

    // Left: Ours
    let l_head = Rect::from_min_size(left_rect.min, Vec2::new(left_rect.width(), 26.0));
    ui.painter().rect_filled(l_head, 0.0, p.panel2);
    ui.painter().text(
        Pos2::new(l_head.left() + 12.0, l_head.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "我方 (Ours / 当前)"
        } else {
            "Ours (Current)"
        },
        FontId::proportional(10.5),
        p.accent,
    );
    let l_body = Rect::from_min_max(Pos2::new(left_rect.left(), l_head.bottom()), left_rect.max);
    crate::app_ui::region(ui, l_body, "ours-scroll", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("ours-scroll-area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&*ours)
                        .font(font.clone())
                        .color(p.text2),
                );
            });
    });

    // Center: Result
    let m_head = Rect::from_min_size(mid_rect.min, Vec2::new(mid_rect.width(), 26.0));
    ui.painter().rect_filled(m_head, 0.0, p.panel2);
    if count_conflicts == 0 {
        ui.painter().text(
            Pos2::new(m_head.left() + 12.0, m_head.center().y),
            Align2::LEFT_CENTER,
            if zh {
                "✓ 合并结果 (冲突已全解)"
            } else {
                "✓ Result (No conflicts remaining)"
            },
            FontId::proportional(10.5),
            p.green,
        );
    } else {
        ui.painter().text(
            Pos2::new(m_head.left() + 12.0, m_head.center().y),
            Align2::LEFT_CENTER,
            if zh {
                format!("合并结果 (剩余 {count_conflicts} 处冲突)")
            } else {
                format!("Result ({count_conflicts} remaining)")
            },
            FontId::proportional(10.5),
            p.amber,
        );
    }
    let m_body = Rect::from_min_max(Pos2::new(mid_rect.left(), m_head.bottom()), mid_rect.max);
    crate::app_ui::region(ui, m_body, "result-scroll", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("result-scroll-area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(worktree)
                        .font(font.clone())
                        .frame(egui::Frame::NONE)
                        .desired_width(mid_rect.width() - 16.0),
                );
            });
    });

    // Right: Theirs
    let r_head = Rect::from_min_size(right_rect.min, Vec2::new(right_rect.width(), 26.0));
    ui.painter().rect_filled(r_head, 0.0, p.panel2);
    ui.painter().text(
        Pos2::new(r_head.left() + 12.0, r_head.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "对方 (Theirs / 传入)"
        } else {
            "Theirs (Incoming)"
        },
        FontId::proportional(10.5),
        p.rose,
    );
    let r_body = Rect::from_min_max(
        Pos2::new(right_rect.left(), r_head.bottom()),
        right_rect.max,
    );
    crate::app_ui::region(ui, r_body, "theirs-scroll", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("theirs-scroll-area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&*theirs)
                        .font(font.clone())
                        .color(p.text2),
                );
            });
    });
}
