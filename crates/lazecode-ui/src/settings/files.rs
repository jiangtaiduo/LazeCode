//! Files/search settings section.

use super::prelude::*;
use super::widgets::*;

pub(super) fn files(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Files & Excludes", "文件与忽略规则"),
        crate::tr!(
            model,
            "Patterns excluded from file exploration and indexing.",
            "在资源管理器与索引中排除的忽略模式。"
        ),
    );
    let mut y = rect.top() + 64.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Add ignore pattern", "添加忽略模式"),
        FontId::proportional(11.5),
        p.text2,
    );

    let input_field = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(200.0, 26.0),
    );
    ui.painter()
        .rect_filled(input_field, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input_field,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let text_resp = ui.put(
        input_field.shrink2(Vec2::new(7.0, 3.0)),
        TextEdit::singleline(&mut rt.new_ignore_pattern)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text("e.g. .venv, build, *.log")
            .font(FontId::monospace(10.5)),
    );
    let enter_pressed = text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let add_btn = Rect::from_min_size(
        Pos2::new(input_field.right() + 8.0, y - 13.0),
        Vec2::new(60.0, 26.0),
    );
    let mut add_clicked =
        secondary_button(ui, add_btn, crate::tr!(model, "Add", "添加"), None, theme).clicked();
    if enter_pressed {
        add_clicked = true;
    }
    if add_clicked {
        let pattern = rt.new_ignore_pattern.trim();
        if !pattern.is_empty() && !model.settings.ignored_patterns.iter().any(|s| s == pattern) {
            model.settings.ignored_patterns.push(pattern.to_string());
            model.save_settings();
            model.queue(lazecode_core::Command::ReloadTree);
            rt.new_ignore_pattern.clear();
        }
    }

    let reset_btn = Rect::from_min_size(
        Pos2::new(add_btn.right() + 8.0, y - 13.0),
        Vec2::new(100.0, 26.0),
    );
    if secondary_button(
        ui,
        reset_btn,
        crate::tr!(model, "Reset defaults", "重置默认"),
        None,
        theme,
    )
    .clicked()
    {
        model.settings.ignored_patterns = vec![
            ".git".into(),
            ".idea".into(),
            ".DS_Store".into(),
            "node_modules".into(),
            ".venv".into(),
            "__pycache__".into(),
            "target".into(),
        ];
        model.save_settings();
        model.queue(lazecode_core::Command::ReloadTree);
    }

    y += 44.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Ignored patterns", "已忽略的模式"),
        FontId::proportional(11.5),
        p.text2,
    );

    let list_area = Rect::from_min_max(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Pos2::new(rect.right() - 20.0, rect.bottom() - 20.0),
    );
    let mut remove_index: Option<usize> = None;
    crate::app_ui::region(ui, list_area, "settings-ignore-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("ignored-patterns-scroll")
            .show(ui, |ui| {
                for (idx, pat) in model.settings.ignored_patterns.iter().enumerate() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.panel2);
                    ui.painter().text(
                        Pos2::new(row.left() + 10.0, row.center().y),
                        Align2::LEFT_CENTER,
                        pat,
                        FontId::monospace(11.0),
                        p.text,
                    );
                    let del_btn = Rect::from_center_size(
                        Pos2::new(row.right() - 14.0, row.center().y),
                        Vec2::splat(16.0),
                    );
                    let del_resp = ui.interact(
                        del_btn,
                        ui.scope_id().with(("del-pat", idx)),
                        Sense::click(),
                    );
                    if del_resp.hovered() {
                        ui.painter().circle_filled(del_btn.center(), 8.0, p.hover);
                    }
                    ui.painter().text(
                        del_btn.center(),
                        Align2::CENTER_CENTER,
                        "×",
                        FontId::proportional(11.0),
                        if del_resp.hovered() {
                            p.text_bright
                        } else {
                            p.text3
                        },
                    );
                    if del_resp.clicked() {
                        remove_index = Some(idx);
                    }
                }
            });
    });
    if let Some(idx) = remove_index {
        model.settings.ignored_patterns.remove(idx);
        model.save_settings();
        model.queue(lazecode_core::Command::ReloadTree);
    }
}
