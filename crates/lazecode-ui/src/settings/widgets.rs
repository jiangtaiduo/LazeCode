//! Shared settings widgets: section heading, labelled inputs, option
//! buttons and the font picker.

use super::prelude::*;
use crate::widgets::secondary_button;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FontTarget {
    Ui,
    Editor,
    Terminal,
}

pub(super) struct FontPicker<'a> {
    pub(super) rect: Rect,
    pub(super) y: f32,
    pub(super) label: &'a str,
    pub(super) target: FontTarget,
    pub(super) monospaced_only: bool,
}

/// Family picker used by Appearance / Editor / Terminal. The choice applies
/// on the next frame through `UiRuntime::fonts_dirty`.
pub(super) fn font_picker(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    picker: FontPicker<'_>,
) {
    let FontPicker {
        rect,
        y,
        label,
        target,
        monospaced_only,
    } = picker;
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        p.text2,
    );
    let current = match target {
        FontTarget::Ui => model.settings.ui_font.clone(),
        FontTarget::Editor => model.settings.editor.font_family.clone(),
        FontTarget::Terminal => model.settings.terminal.font_family.clone(),
    };
    let default_label = match target {
        FontTarget::Ui => crate::tr!(model, "System default", "系统默认"),
        FontTarget::Editor => crate::tr!(model, "System monospace", "系统等宽"),
        FontTarget::Terminal => crate::tr!(model, "Match editor", "跟随编辑器"),
    };
    let selected_text = if current.is_empty() {
        if target == FontTarget::Terminal && !model.settings.editor.font_family.is_empty() {
            format!("{} ({})", default_label, model.settings.editor.font_family)
        } else {
            default_label.to_string()
        }
    } else {
        current.clone()
    };

    let field = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(260.0, 26.0),
    );
    let id = match target {
        FontTarget::Ui => "settings-ui-font",
        FontTarget::Editor => "settings-editor-font",
        FontTarget::Terminal => "settings-terminal-font",
    };
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .id_salt(id)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let filter_id = ui.make_persistent_id((id, "filter"));
    let mut filter = ui.data(|data| data.get_temp::<String>(filter_id).unwrap_or_default());
    let mut chosen: Option<String> = None;

    egui::ComboBox::from_id_salt(id)
        .width(field.width())
        .selected_text(egui::RichText::new(selected_text).color(p.text))
        .show_ui(&mut child, |ui| {
            ui.set_min_width(260.0);
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text(crate::tr!(model, "Search fonts", "搜索字体"))
                    .desired_width(f32::INFINITY),
            );
            let needle = filter.trim().to_lowercase();
            if ui
                .selectable_label(current.is_empty(), default_label)
                .clicked()
            {
                chosen = Some(String::new());
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt((id, "list"))
                .max_height(260.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut any = false;
                    for family in crate::fonts::families(monospaced_only) {
                        if !needle.is_empty() && !family.to_lowercase().contains(&needle) {
                            continue;
                        }
                        any = true;
                        if ui.selectable_label(current == family, family).clicked() {
                            chosen = Some(family.to_string());
                        }
                    }
                    if !any {
                        ui.weak(crate::tr!(model, "No matching fonts", "没有匹配的字体"));
                    }
                });
        });
    ui.data_mut(|data| data.insert_temp(filter_id, filter));

    if let Some(font) = chosen {
        match target {
            FontTarget::Ui => model.settings.ui_font = font,
            FontTarget::Editor => model.settings.editor.font_family = font,
            FontTarget::Terminal => model.settings.terminal.font_family = font,
        }
        model.save_settings();
        rt.fonts_dirty = true;
    }
}

pub(super) fn option_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    selected: bool,
    icon: Option<Icon>,
    theme: &Theme,
) -> egui::Response {
    if selected {
        primary_button(ui, rect, label, icon, true, theme)
    } else {
        secondary_button(ui, rect, label, icon, theme)
    }
}

pub(super) fn heading(ui: &mut Ui, theme: &Theme, rect: Rect, title: &str, subtitle: &str) {
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left(), rect.top() + 8.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(15.0),
        p.text,
    );
    ui.painter().text(
        Pos2::new(rect.left(), rect.top() + 28.0),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(10.5),
        p.text3,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 44.5,
        Stroke::new(1.0, p.border_soft),
    );
}

/// Single-line labelled settings input. Returns `true` when the user
/// committed a change (focus lost or Enter).
pub(super) fn labelled_input(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    y: f32,
    label: &str,
    hint: &str,
    value: &mut String,
) -> bool {
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        p.text2,
    );
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new((rect.width() - 170.0).max(160.0), 26.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let response = ui.put(
        input.shrink2(Vec2::new(7.0, 3.0)),
        TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(hint)
            .font(FontId::monospace(10.5)),
    );
    let entered = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    response.lost_focus() || entered
}
