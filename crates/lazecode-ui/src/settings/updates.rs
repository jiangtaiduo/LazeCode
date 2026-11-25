//! Updates section: channel, manifest and install actions.

use super::prelude::*;
use super::widgets::*;

pub(super) fn updates(
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
        crate::tr!(model, "Updates", "更新"),
        crate::tr!(
            model,
            "Signed release manifest: HTTPS endpoint plus the pinned Ed25519 public key.",
            "签名发布清单：更新地址与固定的 Ed25519 公钥。"
        ),
    );
    let endpoint_label = crate::tr!(model, "Endpoint", "更新地址");
    let key_label = crate::tr!(model, "Public key", "公钥");
    let auto_label = crate::tr!(model, "Check automatically", "自动检查更新");
    let mut changed = false;
    let mut y = rect.top() + 74.0;
    changed |= labelled_input(
        ui,
        theme,
        rect,
        y,
        endpoint_label,
        "https://updates.example.com/manifest.json",
        &mut model.settings.update_endpoint,
    );
    y += 42.0;
    changed |= labelled_input(
        ui,
        theme,
        rect,
        y,
        key_label,
        "base64 or hex Ed25519 public key",
        &mut model.settings.update_public_key,
    );
    y += 42.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        auto_label,
        FontId::proportional(11.5),
        p.text2,
    );
    let check_rect =
        Rect::from_min_size(Pos2::new(rect.left() + 160.0, y - 9.0), Vec2::splat(18.0));
    if checkbox(ui, check_rect, model.settings.auto_check_updates, theme).clicked() {
        model.settings.auto_check_updates = !model.settings.auto_check_updates;
        changed = true;
    }
    ui.painter().text(
        Pos2::new(rect.left() + 186.0, y),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "Five seconds after launch, once per launch (no download starts).",
            "启动 5 秒后检查，每次启动一次（不会自动下载）。"
        ),
        FontId::proportional(10.5),
        p.text3,
    );

    y += 30.0;
    let checked = if model.settings.last_update_check_time > 0 {
        crate::update_ui::checked_label(
            Some(model.settings.last_update_check_time),
            model.settings.language.is_zh(),
        )
    } else {
        crate::tr!(model, "never", "从未").to_string()
    };
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!("上次检查：{checked}")
        } else {
            format!("Last checked: {checked}")
        },
        FontId::proportional(10.5),
        p.text3,
    );

    if !model.settings.ignored_update_version.is_empty() {
        y += 24.0;
        let ignored = model.settings.ignored_update_version.clone();
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            if model.settings.language.is_zh() {
                format!("已忽略版本：v{ignored}")
            } else {
                format!("Ignored version: v{ignored}")
            },
            FontId::proportional(10.5),
            p.text3,
        );
        let clear_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 160.0, y - 11.0),
            Vec2::new(56.0, 22.0),
        );
        if secondary_button(
            ui,
            clear_rect,
            crate::tr!(model, "Clear", "清除"),
            None,
            theme,
        )
        .clicked()
        {
            model.settings.ignored_update_version.clear();
            rt.update.ignore_checked = false;
            model.save_settings();
        }
    }

    y += 36.0;
    let open_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(170.0, 26.0),
    );
    if secondary_button_enabled(
        ui,
        open_rect,
        crate::tr!(model, "Check for updates…", "检查更新…"),
        None,
        !rt.update.busy(),
        theme,
    )
    .clicked()
    {
        rt.update_open = true;
        let version = model.version;
        if rt.update.begin_check(&mut model.settings, version, false) {
            model.save_settings();
        }
    }

    y += 34.0;
    if model.settings.updates_configured() {
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            format!(
                "{} {}",
                crate::tr!(model, "Signing key:", "签名公钥："),
                match model.settings.update_public_key.len() {
                    0 => crate::tr!(model, "not set", "未设置").to_string(),
                    len => format!("{len} chars"),
                }
            ),
            FontId::proportional(10.5),
            p.text3,
        );
    } else {
        let note = crate::tr!(
            model,
            "Updates stay disabled until both fields are filled in. Only HTTPS/HTTP, file:// URLs and local paths are accepted; artifacts are SHA-256 checked and Ed25519 verified.",
            "两项都填写后才会启用更新。仅接受 HTTPS/HTTP、file:// 或本地路径；下载后会校验 SHA-256 与 Ed25519 签名。"
        );
        let galley = ui.painter().layout(
            note.to_string(),
            FontId::proportional(10.5),
            p.amber,
            rect.width(),
        );
        ui.painter()
            .with_clip_rect(rect)
            .galley(Pos2::new(rect.left(), y - 7.0), galley, p.amber);
    }

    if changed {
        model.save_settings();
    }
}
