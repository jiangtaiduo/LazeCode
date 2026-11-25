//! Diff tab rendering: file diff, modes and the empty/hint states.

use super::prelude::*;

pub(super) fn draw_diff(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
        return;
    };
    let TabKind::Diff(diff) = &tab.kind else {
        return;
    };
    let path = diff.path.clone();
    let diff_source = diff.source.clone();
    let view_mode = diff.view_mode;
    let current_group = diff.current_hunk;
    let edge_pending = diff.edge_pending;
    let revealed = diff.revealed;
    let status = diff.diff.as_ref().map(|d| d.kind);
    let additions = diff.diff.as_ref().map(|d| d.diff.additions).unwrap_or(0);
    let deletions = diff.diff.as_ref().map(|d| d.diff.deletions).unwrap_or(0);
    let error = diff.error.clone();
    let hunks = diff.diff.as_ref().map(|d| d.diff.hunks.clone());
    let binary = diff.diff.as_ref().map(|d| d.binary).unwrap_or(false);
    let loading = diff.loading;

    // Monaco 语义：变更组（连续增删行）驱动导航/蓝框，git hunk 数驱动统计。
    let (rows, groups) = hunks
        .as_deref()
        .map_or_else(|| (Vec::new(), 0usize), crate::diff_view::build_rows);

    // ---- 跨文件导航清单（Compare 抽屉 / 提交详情文件树） ----
    let nav_files: Vec<String> = match &diff_source {
        DiffSource::CompareCommits { from, to, .. } => model.projects[index]
            .compare_session
            .as_ref()
            .filter(|s| &s.from == from && &s.to == to)
            .map(|s| s.nav_order.clone())
            .unwrap_or_default(),
        DiffSource::CompareWithLocal { commit, .. } => model.projects[index]
            .compare_session
            .as_ref()
            .filter(|s| s.local && &s.from == commit)
            .map(|s| s.nav_order.clone())
            .unwrap_or_default(),
        DiffSource::Stash { stash, .. } => model.projects[index]
            .stash_details
            .get(stash)
            .map(|details| {
                details
                    .tracked
                    .iter()
                    .chain(details.untracked.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        DiffSource::StashBeforeLocal { stash, .. } => model.projects[index]
            .stash_details
            .get(stash)
            .map(|details| {
                details
                    .tracked
                    .iter()
                    .chain(details.untracked.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        DiffSource::Commit { commit, .. } => model.projects[index]
            .commit_details
            .as_ref()
            .filter(|d| &d.info.id == commit)
            .map(|d| d.files.iter().map(|f| f.path.clone()).collect())
            .unwrap_or_default(),
        DiffSource::Worktree { .. } | DiffSource::Index { .. } => {
            let include_untracked = matches!(diff_source, DiffSource::Worktree { .. });
            let project = &model.projects[index];
            let lists = lazecode_git::Changelists::open(project.active_root());
            let changes = project.changes();
            let mut ordered = Vec::new();
            for list_name in lists.lists() {
                for c in changes {
                    if !c.untracked && lists.list_of(&c.path) == *list_name {
                        ordered.push(c.path.clone());
                    }
                }
            }
            for c in changes {
                if !c.untracked && !ordered.contains(&c.path) {
                    ordered.push(c.path.clone());
                }
            }
            if include_untracked {
                for c in changes {
                    if c.untracked && !ordered.contains(&c.path) {
                        ordered.push(c.path.clone());
                    }
                }
            }
            ordered
        }
        _ => Vec::new(),
    };
    let has_nav = !nav_files.is_empty();
    let nav_index = nav_files.iter().position(|f| *f == path);
    let can_prev_file = has_nav && nav_index.is_some_and(|i| i > 0);
    let can_next_file = has_nav && nav_index.is_some_and(|i| i + 1 < nav_files.len());

    // F7 / Shift+F7（稳定版快捷键）。
    let (f7, f7_shift) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::F7),
            input.modifiers.shift && input.key_pressed(egui::Key::F7),
        )
    });
    let mut nav_request: i32 = 0;
    if f7_shift {
        nav_request = -1;
    } else if f7 {
        nav_request = 1;
    }

    let zh = model.settings.language.is_zh();
    let worktree_mode = matches!(diff_source, DiffSource::Worktree { .. });

    // ----- 稳定版 DiffViewer：28px 工具栏 + 28px 等宽 pane 标题栏 -----
    let toolbar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 28.0));
    ui.painter().rect_filled(toolbar, 0.0, p.sidebar);
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let titlebar = Rect::from_min_size(
        Pos2::new(rect.left(), toolbar.bottom()),
        Vec2::new(rect.width(), 28.0),
    );
    ui.painter().rect_filled(titlebar, 0.0, p.sidebar);
    ui.painter().hline(
        titlebar.x_range(),
        titlebar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    let mut nav_target: Option<String> = None;
    let mut lx = toolbar.left() + 8.0;
    let icon_btn = |ui: &mut Ui, x: &mut f32, icon: Icon, enabled: bool| -> bool {
        let rect =
            Rect::from_center_size(Pos2::new(*x + 10.0, toolbar.center().y), Vec2::splat(20.0));
        *x += 22.0;
        if !enabled {
            paint_icon(
                ui.painter(),
                rect.shrink(3.5),
                icon,
                p.text3.gamma_multiply(0.5),
            );
            return false;
        }
        let response = ui.interact(
            rect,
            ui.scope_id().with(("diff-tb", *x as u32, icon as u8)),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(6), p.hover);
        }
        paint_icon(ui.painter(), rect.shrink(3.5), icon, p.text2);
        response.clicked()
    };
    // 文件间导航 ←/→（清单来自 Compare 抽屉或提交详情文件树）。
    let _ = icon_btn(ui, &mut lx, Icon::ArrowLeft, can_prev_file);
    let _ = icon_btn(ui, &mut lx, Icon::ArrowRight, can_next_file);
    if has_nav && nav_index.is_some() {
        // 命中检测在下方单独处理（icon_btn 已渲染）；用位置重新交互一次。
    }
    let prev_file_rect = Rect::from_center_size(
        Pos2::new(toolbar.left() + 8.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    let next_file_rect = Rect::from_center_size(
        Pos2::new(toolbar.left() + 8.0 + 22.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    if can_prev_file
        && ui
            .interact(
                prev_file_rect,
                ui.scope_id().with("diff-prev-file"),
                Sense::click(),
            )
            .clicked()
    {
        if let Some(i) = nav_index {
            nav_target = Some(nav_files[i - 1].clone());
        }
    }
    if can_next_file
        && ui
            .interact(
                next_file_rect,
                ui.scope_id().with("diff-next-file"),
                Sense::click(),
            )
            .clicked()
    {
        if let Some(i) = nav_index {
            nav_target = Some(nav_files[i + 1].clone());
        }
    }
    lx += 3.0;
    ui.painter().vline(
        lx,
        (toolbar.center().y - 7.0)..=(toolbar.center().y + 7.0),
        Stroke::new(1.0, p.border_soft),
    );
    lx += 5.0;
    let can_hunk_nav = groups > 0;
    let _ = icon_btn(ui, &mut lx, Icon::ChevronUp, can_hunk_nav);
    let _ = icon_btn(ui, &mut lx, Icon::ChevronDown, can_hunk_nav);
    let up_rect = Rect::from_center_size(
        Pos2::new(lx - 44.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    let down_rect = Rect::from_center_size(
        Pos2::new(lx - 22.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    if can_hunk_nav {
        if ui
            .interact(
                up_rect,
                ui.scope_id().with("diff-prev-hunk"),
                Sense::click(),
            )
            .clicked()
        {
            nav_request -= 1;
        }
        if ui
            .interact(
                down_rect,
                ui.scope_id().with("diff-next-hunk"),
                Sense::click(),
            )
            .clicked()
        {
            nav_request += 1;
        }
    }
    if groups > 0 {
        // hunk 计数药丸："n / m"（Monaco 变更组数）。
        let counter = Rect::from_center_size(
            Pos2::new(lx + 24.0, toolbar.center().y),
            Vec2::new(44.0, 20.0),
        );
        lx += 50.0;
        ui.painter()
            .rect_filled(counter, egui::CornerRadius::same(4), p.panel2);
        ui.painter().rect_stroke(
            counter,
            egui::CornerRadius::same(4),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
        let hunk_text = format!(
            "{} / {}",
            current_group.min(groups.saturating_sub(1)) + 1,
            groups
        );
        ui.painter().text(
            counter.center(),
            Align2::CENTER_CENTER,
            hunk_text,
            FontId::proportional(11.0),
            p.text,
        );
        // 工作区模式：当前组所在 hunk 的 Stage / Rollback（稳定版工具栏按钮）。
        if worktree_mode {
            let mut hunk_btn = |ui: &mut Ui,
                                x: &mut f32,
                                label: &str,
                                patch: Option<String>,
                                cached: bool,
                                reverse: bool| {
                let w = label.chars().count() as f32 * 6.2 + 14.0;
                let rect = Rect::from_center_size(
                    Pos2::new(*x + w / 2.0, toolbar.center().y),
                    Vec2::new(w, 18.0),
                );
                *x += w + 4.0;
                let response = ui.interact(
                    rect,
                    ui.scope_id().with(("diff-hunk-btn", label)),
                    Sense::click(),
                );
                let bg = if response.hovered() {
                    p.hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(4), bg);
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(4),
                    Stroke::new(1.0, p.border_soft),
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(10.0),
                    if response.hovered() {
                        p.text_bright
                    } else {
                        p.text2
                    },
                );
                if response.clicked() {
                    if let Some(patch) = patch {
                        model.queue(Command::ApplyPatch {
                            patch,
                            cached,
                            reverse,
                            root: model.projects[index].active_root().to_path_buf(),
                        });
                    }
                }
            };
            let patch = hunks.as_ref().and_then(|h| {
                let hi = crate::diff_view::hunk_of_group(h, current_group)?;
                Some(lazecode_text::unified_patch(
                    &path,
                    &lazecode_text::TextDiff {
                        hunks: vec![h[hi].clone()],
                        ..Default::default()
                    },
                ))
            });
            let label_stage = if zh { "暂存此块" } else { "Stage hunk" };
            let label_rollback = if zh { "回滚此块" } else { "Rollback hunk" };
            let patch2 = patch.clone();
            hunk_btn(ui, &mut lx, label_stage, patch, true, false);
            hunk_btn(ui, &mut lx, label_rollback, patch2, false, true);
        }
    }
    // 视图选择（select 样式 20px，点击弹 Unified/Side by Side 两项）。
    let mode_label = match view_mode {
        DiffViewMode::Unified => crate::tr!(model, "Unified view", "统一视图"),
        DiffViewMode::SideBySide => crate::tr!(model, "Side by Side", "并排对比"),
    };
    let select_rect = Rect::from_center_size(
        Pos2::new(lx + 58.0, toolbar.center().y),
        Vec2::new(112.0, 20.0),
    );
    lx += 122.0;
    let select_open = rt.diff_mode_open == Some(tab_id);
    ui.painter()
        .rect_filled(select_rect, egui::CornerRadius::same(4), p.panel2);
    ui.painter().rect_stroke(
        select_rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, if select_open { p.accent } else { p.border_soft }),
        StrokeKind::Inside,
    );
    text_clipped(
        ui.painter(),
        select_rect.shrink2(Vec2::new(8.0, 0.0)),
        mode_label,
        FontId::proportional(11.0),
        p.text,
        Align2::LEFT_CENTER,
    );
    // 原生 select 的上下双箭头。
    let chev_x = select_rect.right() - 11.0;
    let chev_cy = select_rect.center().y;
    for (dy, up) in [(-3.0, true), (3.0, false)] {
        let c = Pos2::new(chev_x, chev_cy + dy);
        let (a, b, d) = if up {
            (
                Pos2::new(c.x - 3.0, c.y + 1.5),
                Pos2::new(c.x, c.y - 1.0),
                Pos2::new(c.x + 3.0, c.y + 1.5),
            )
        } else {
            (
                Pos2::new(c.x - 3.0, c.y - 1.5),
                Pos2::new(c.x, c.y + 1.0),
                Pos2::new(c.x + 3.0, c.y - 1.5),
            )
        };
        ui.painter().line_segment([a, b], Stroke::new(1.0, p.text3));
        ui.painter().line_segment([b, d], Stroke::new(1.0, p.text3));
    }
    if ui
        .interact(select_rect, ui.scope_id().with("diff-mode"), Sense::click())
        .clicked()
    {
        rt.diff_mode_open = if select_open { None } else { Some(tab_id) };
    }
    // 统计：git hunk 数（对齐稳定版 t().diff.stats(a, d, hunkOffsets.length)）。
    let total_hunks = hunks.as_ref().map_or(0, |h| h.len());
    let stats = if loading {
        crate::tr!(model, "Loading…", "加载中…").to_string()
    } else if zh {
        format!("{additions} 增 / {deletions} 删 · {total_hunks} 处差异")
    } else {
        format!("{additions} additions / {deletions} deletions · {total_hunks} differences")
    };
    ui.painter().text(
        Pos2::new(lx + 8.0, toolbar.center().y),
        Align2::LEFT_CENTER,
        stats,
        FontId::proportional(11.0),
        p.text3,
    );

    // 右组：比较标签（muted 11px）。
    let comparison = match &diff_source {
        DiffSource::Commit { commit, .. } => {
            let hash: String = commit.chars().take(8).collect();
            if zh {
                format!("提交 {hash}")
            } else {
                format!("Commit {hash}")
            }
        }
        DiffSource::CompareWithLocal { commit, .. } => {
            let hash: String = commit.chars().take(8).collect();
            if zh {
                format!("{hash} → 工作区")
            } else {
                format!("{hash} → working tree")
            }
        }
        DiffSource::CompareCommits { from, to, .. } => {
            let f: String = from.chars().take(8).collect();
            let to8: String = to.chars().take(8).collect();
            if zh {
                format!("提交 {f} ↔ {to8}")
            } else {
                format!("{f} ↔ {to8}")
            }
        }
        DiffSource::Files { .. } => crate::tr!(model, "File vs File", "两文件对比").to_string(),
        DiffSource::Stash { stash, .. } => {
            if zh {
                format!("{stash} 中的更改")
            } else {
                format!("Changes in {stash}")
            }
        }
        DiffSource::StashBeforeLocal { .. } => {
            if zh {
                "更改前 → 工作区".to_string()
            } else {
                "Before → working tree".to_string()
            }
        }
        DiffSource::Worktree { .. } => {
            crate::tr!(model, "Working tree vs HEAD", "工作区 vs HEAD").to_string()
        }
        DiffSource::Index { .. } => {
            crate::tr!(model, "Working tree vs index", "工作区 vs 索引").to_string()
        }
    };
    ui.painter().text(
        Pos2::new(toolbar.right() - 10.0, toolbar.center().y),
        Align2::RIGHT_CENTER,
        comparison,
        FontId::proportional(11.0),
        p.text3,
    );

    // 边缘气球（PyCharm Balloon：工具栏下方悬浮，1.8s 自动消失）。
    // 采用 Order::Tooltip 顶层绘制，绝对不透明背景 + 实心箭头 + 阴影，避免被底层代码穿透。
    if let Some((text, at)) = &rt.diff_balloon {
        if at.elapsed().as_secs_f32() < 1.8 {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                ui.scope_id().with("diff-balloon"),
            ));
            let w = text.chars().count() as f32 * 7.2 + 32.0;
            let b = Rect::from_min_size(
                Pos2::new(rect.left() + 10.0, toolbar.bottom() + 6.0),
                Vec2::new(w, 28.0),
            );
            let solid_bg = if theme.dark {
                Color32::from_rgb(0x2b, 0x2d, 0x30)
            } else {
                Color32::from_rgb(0xff, 0xff, 0xff)
            };
            let text_color = if theme.dark {
                Color32::from_rgb(0xdf, 0xe1, 0xe5)
            } else {
                Color32::from_rgb(0x1e, 0x1f, 0x22)
            };
            // 阴影
            let shape = egui::epaint::RectShape {
                rect: b.translate(egui::vec2(0.0, 3.0)).expand(4.0),
                corner_radius: egui::CornerRadius::same(6),
                fill: Color32::from_black_alpha(150),
                stroke: Stroke::NONE,
                stroke_kind: egui::StrokeKind::Inside,
                round_to_pixels: None,
                blur_width: 6.0,
                brush: None,
                angle: 0.0,
            };
            painter.add(egui::Shape::Rect(shape));
            painter.rect_filled(b, egui::CornerRadius::same(5), solid_bg);
            painter.rect_stroke(
                b,
                egui::CornerRadius::same(5),
                Stroke::new(1.0, p.jb_accent),
                StrokeKind::Inside,
            );
            painter.text(
                b.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(12.5),
                text_color,
            );
            // 顶部实心小三角形
            let tip = Pos2::new(b.left() + 20.0, b.top() - 4.0);
            let arrow_poly = vec![
                tip,
                Pos2::new(tip.x - 4.5, b.top() + 0.5),
                Pos2::new(tip.x + 4.5, b.top() + 0.5),
            ];
            painter.add(egui::Shape::convex_polygon(
                arrow_poly,
                solid_bg,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(tip.x - 4.5, b.top()), tip],
                Stroke::new(1.0, p.jb_accent),
            );
            painter.line_segment(
                [tip, Pos2::new(tip.x + 4.5, b.top())],
                Stroke::new(1.0, p.jb_accent),
            );
        }
    }

    // ----- pane 标题栏：hash（或 "diff"）+ 路径 + 状态 tag -----
    let hash_text = match &diff_source {
        DiffSource::Commit { commit, .. } => commit.chars().take(8).collect::<String>(),
        _ => "diff".to_string(),
    };
    let hash_w = hash_text.chars().count() as f32 * 6.6 + 8.0;
    ui.painter().text(
        Pos2::new(titlebar.left() + 8.0, titlebar.center().y),
        Align2::LEFT_CENTER,
        &hash_text,
        FontId::monospace(12.0),
        p.text3,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(titlebar.left() + 8.0 + hash_w, titlebar.top()),
            Pos2::new(titlebar.right() - 60.0, titlebar.bottom()),
        ),
        &path,
        FontId::monospace(12.5),
        p.text_bright,
        Align2::LEFT_CENTER,
    );
    let tag_kind = status.filter(|kind| {
        matches!(
            kind,
            lazecode_git::ChangeKind::Added | lazecode_git::ChangeKind::Deleted
        )
    });
    if let Some(kind) = tag_kind {
        // 稳定版只渲染 Added/Deleted 标签（git-tag git-staged / git-del）。
        let label = kind.label();
        let tag_w = label.chars().count() as f32 * 10.0 + 12.0;
        let tag = Rect::from_center_size(
            Pos2::new(titlebar.right() - 10.0 - tag_w * 0.5, titlebar.center().y),
            Vec2::new(tag_w, 15.0),
        );
        let (tag_color, tag_bg, tag_border) = match kind {
            lazecode_git::ChangeKind::Added | lazecode_git::ChangeKind::Renamed => {
                (p.green, p.diff_add, p.diff_add_strong)
            }
            lazecode_git::ChangeKind::Deleted => (p.rose, p.diff_del, p.diff_del_strong),
            _ => (p.text3, p.hover, p.border_soft),
        };
        ui.painter()
            .rect_filled(tag, egui::CornerRadius::same(3), tag_bg);
        ui.painter().rect_stroke(
            tag,
            egui::CornerRadius::same(3),
            Stroke::new(1.0, tag_border),
            StrokeKind::Inside,
        );
        ui.painter().text(
            tag.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(10.0),
            tag_color,
        );
    }

    // ----- 导航（点击 + F7）：组游标、边缘气球、二次点击跨文件 -----
    if nav_request != 0 && groups > 0 {
        let next = current_group as isize + nav_request as isize;
        if (0..groups as isize).contains(&next) {
            let next = next as usize;
            diff_mut(model, index, tab_id, |d| {
                d.current_hunk = next;
                d.edge_pending = 0;
            });
            rt.diff_scroll = Some((tab_id, next));
        } else if edge_pending == 0 {
            let msg = if has_nav {
                if nav_request > 0 {
                    crate::tr!(
                        model,
                        "Already at the last difference — click again to open the next file",
                        "已是最后一处差异，再次点击查看下一个文件的差异"
                    )
                } else {
                    crate::tr!(
                        model,
                        "Already at the first difference — click again to open the previous file",
                        "已是第一处差异，再次点击查看上一个文件的差异"
                    )
                }
            } else if nav_request > 0 {
                crate::tr!(model, "Already at the last difference", "已是最后一处差异")
            } else {
                crate::tr!(model, "Already at the first difference", "已是第一处差异")
            };
            rt.diff_balloon = Some((msg.to_string(), std::time::Instant::now()));
            let sign = nav_request.signum() as i8;
            diff_mut(model, index, tab_id, |d| d.edge_pending = sign);
        } else if has_nav && nav_index.is_some() {
            let target_i = if nav_request > 0 {
                nav_index.unwrap_or(0) + 1
            } else {
                nav_index.unwrap_or(0).wrapping_sub(1)
            };
            if let Some(target) = nav_files.get(target_i) {
                nav_target = Some(target.clone());
            }
        }
    }

    if let Some(target) = nav_target.clone() {
        if let Some(project) = model.projects.get_mut(index) {
            if let Some(session) = project.compare_session.as_mut() {
                session.selected = Some(target.clone());
                let parts: Vec<&str> = target.split('/').collect();
                for i in 1..parts.len() {
                    let parent = parts[..i].join("/");
                    session.collapsed.remove(&parent);
                }
                session.root_expanded = true;
            }
        }
        let cmd = match &diff_source {
            DiffSource::CompareCommits { from, to, .. } => Some(Command::OpenCompareCommitsDiff {
                from: from.clone(),
                to: to.clone(),
                path: target,
            }),
            DiffSource::CompareWithLocal { commit, .. } => {
                Some(Command::OpenCompareWithLocalDiff {
                    commit: commit.clone(),
                    path: target,
                })
            }
            DiffSource::Commit { commit, .. } => Some(Command::OpenCommitDiff {
                commit: commit.clone(),
                path: target,
            }),
            DiffSource::Worktree { .. } => Some(Command::OpenDiff(target)),
            DiffSource::Index { .. } => Some(Command::OpenIndexDiff(target)),
            _ => None,
        };
        if let Some(cmd) = cmd {
            model.queue(cmd);
            diff_mut(model, index, tab_id, |d| {
                d.edge_pending = 0;
                d.revealed = true;
            });
            rt.diff_balloon = None;
        }
    }

    // 首次显示：定位第一处差异（stable revealedRef 语义）。
    if !revealed && !rows.is_empty() {
        rt.diff_scroll = Some((tab_id, 0));
        diff_mut(model, index, tab_id, |d| d.revealed = true);
    }

    let body = Rect::from_min_max(Pos2::new(rect.left(), titlebar.bottom()), rect.max);
    if loading {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            crate::tr!(model, "Loading diff…", "正在读取差异…"),
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }
    if let Some(error) = error {
        ui.painter().text(
            Pos2::new(body.left() + 20.0, body.top() + 30.0),
            Align2::LEFT_CENTER,
            error,
            FontId::proportional(11.0),
            p.rose,
        );
        return;
    }
    if binary {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            crate::tr!(
                model,
                "Cannot show diff for an unknown file type",
                "无法显示未知文件类型的差异"
            ),
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }
    if hunks.is_none() || rows.is_empty() {
        let empty = match &diff_source {
            DiffSource::Worktree { .. } => {
                crate::tr!(
                    model,
                    "This file has no uncommitted changes",
                    "该文件没有未提交的改动"
                )
            }
            DiffSource::Index { .. } => {
                crate::tr!(
                    model,
                    "The index and working tree are identical for this file",
                    "该文件的索引与工作区内容相同"
                )
            }
            _ => crate::tr!(
                model,
                "This commit has no text difference",
                "该提交没有文本差异"
            ),
        };
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            empty,
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }

    // 视图模式下拉（两行小菜单）。
    if select_open {
        let menu_w = 150.0;
        let menu_rect = Rect::from_min_size(
            Pos2::new(select_rect.left(), toolbar.bottom() + 2.0),
            Vec2::new(menu_w, 48.0),
        );
        // Register for the shell's wheel interception (next frame).
        rt.menu_hit_rects.push(menu_rect);
        // Register the outside-click catcher BEFORE the menu rows: egui gives
        // the later interaction precedence, and the expanded rect overlaps
        // the two rows. With the old order every row click only dismissed
        // the menu, making Side by Side unreachable.
        if ui
            .interact(
                rect.expand(40.0),
                ui.scope_id().with("diff-mode-dismiss"),
                Sense::click(),
            )
            .clicked()
        {
            rt.diff_mode_open = None;
        }
        ui.painter()
            .rect_filled(menu_rect, egui::CornerRadius::same(6), p.menu_bg);
        ui.painter().rect_stroke(
            menu_rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, p.menu_border),
            StrokeKind::Inside,
        );
        for (row, mode, label) in [
            (
                0usize,
                DiffViewMode::Unified,
                crate::tr!(model, "Unified view", "统一视图"),
            ),
            (
                1,
                DiffViewMode::SideBySide,
                crate::tr!(model, "Side by Side", "并排对比"),
            ),
        ] {
            let row_rect = Rect::from_min_size(
                Pos2::new(
                    menu_rect.left() + 4.0,
                    menu_rect.top() + 4.0 + row as f32 * 20.0,
                ),
                Vec2::new(menu_w - 8.0, 20.0),
            );
            let hovered =
                row_rect.contains(ui.input(|i| i.pointer.latest_pos().unwrap_or_default()));
            if hovered {
                ui.painter()
                    .rect_filled(row_rect, egui::CornerRadius::same(4), p.menu_item_hover);
            }
            ui.painter().text(
                Pos2::new(row_rect.left() + 8.0, row_rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                if view_mode == mode {
                    p.text_bright
                } else {
                    p.text2
                },
            );
            if view_mode == mode {
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(row_rect.right() - 12.0, row_rect.center().y),
                        Vec2::splat(12.0),
                    ),
                    Icon::Check,
                    p.text_bright,
                );
            }
            if ui
                .interact(
                    row_rect,
                    ui.scope_id().with(("diff-mode-item", row)),
                    Sense::click(),
                )
                .clicked()
            {
                if view_mode != mode {
                    if let Some(tab) = model.projects[index].tab_mut_by_id(tab_id) {
                        if let TabKind::Diff(d) = &mut tab.kind {
                            d.view_mode = mode;
                        }
                    }
                }
                rt.diff_mode_open = None;
            }
        }
    }

    let ctx = crate::diff_view::DiffBodyCtx {
        tab_id,
        rows: &rows,
        groups,
        current_group: current_group.min(groups.saturating_sub(1)),
        scroll_to_group: rt.diff_scroll,
        can_rollback: worktree_mode,
    };
    let rollback_group = match view_mode {
        DiffViewMode::Unified => crate::diff_view::draw_unified(ui, theme, body, ctx),
        DiffViewMode::SideBySide => crate::diff_view::draw_split(ui, theme, body, ctx),
    };
    if let Some(gi) = rollback_group {
        if let Some(hunks) = &hunks {
            if let Some(hi) = crate::diff_view::hunk_of_group(hunks, gi) {
                let patch = lazecode_text::unified_patch(
                    &path,
                    &lazecode_text::TextDiff {
                        hunks: vec![hunks[hi].clone()],
                        ..Default::default()
                    },
                );
                model.queue(Command::ApplyPatch {
                    patch,
                    cached: false,
                    reverse: true,
                    root: model.projects[index].active_root().to_path_buf(),
                });
            }
        }
    }
    // 滚动请求为单帧语义。
    rt.diff_scroll = None;
}

pub(super) fn diff_mut(
    model: &mut AppModel,
    index: usize,
    tab_id: u64,
    f: impl FnOnce(&mut DiffTab),
) {
    if let Some(tab) = model.projects[index].tab_mut_by_id(tab_id) {
        if let TabKind::Diff(diff) = &mut tab.kind {
            f(diff);
        }
    }
}
