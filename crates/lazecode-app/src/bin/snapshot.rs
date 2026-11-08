use std::path::{Path, PathBuf};

use lazecode_core::{AppModel, PanelTab, ProjectState, Settings, SidebarView, UiState};
use lazecode_ui::UiRuntime;
use std::collections::HashSet;

const W: f32 = 1440.0;
const H: f32 = 900.0;
const SCALE: f32 = 2.0;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "/tmp/laze-snapshot.png".into()),
    );
    let project_root = PathBuf::from(
        args.next()
            .unwrap_or_else(|| std::env::current_dir().unwrap().display().to_string()),
    );
    let mode = args.next().unwrap_or_else(|| "editor".to_string());
    let language = args.next().unwrap_or_else(|| "en".to_string());

    let mut settings = Settings::default();
    if language == "zh" {
        settings.language = lazecode_core::Language::Chinese;
    }
    let mut model = AppModel {
        settings: settings.clone(),
        projects: Vec::new(),
        active_project: None,
        ui: UiState::default(),
        toasts: Vec::new(),
        recent: Vec::new(),
        commands: Vec::new(),
        notifier: None,
        should_quit: false,
        version: env!("CARGO_PKG_VERSION"),
        favorite_branches: HashSet::new(),
    };
    if mode != "welcome" {
        let mut project = ProjectState::open(project_root.clone(), &settings, None)
            .map_err(|err| anyhow::anyhow!(err))?;
        let readme = project_root.join("README.md");
        if readme.exists() {
            let _ = project.open_file(readme, &settings);
        }
        let source = project_root.join("crates/lazecode-ui/src/editor.rs");
        if source.exists() {
            let _ = project.open_file(source, &settings);
        }
        let docs = project_root.join("docs/design/architecture.md");
        if docs.exists() {
            let _ = project.open_file(docs, &settings);
        }
        if mode == "diff" {
            project.open_diff_source(lazecode_core::DiffSource::Commit {
                commit: "e338822".to_string(),
                path: "images.py".to_string(),
            });
        }
        if mode == "compare" {
            // 分支比较页：开当前分支与 main 的比较（feat-rust 仓库内有效）。
            // snapshot 单帧渲染拿不到后台 job 回填，这里同步预取注入。
            let title = "Compare: feat-rust and main".to_string();
            project.open_compare_branches("feat-rust".to_string(), "main".to_string(), title);
            if let Ok(data) =
                lazecode_git::compare_branches(&project_root, "feat-rust", "main", None, None, 1000)
            {
                model.ui.compare_branches.root = project_root.clone();
                model.ui.compare_branches.left_ref = "feat-rust".to_string();
                model.ui.compare_branches.right_ref = "main".to_string();
                model.ui.compare_branches.loading = false;
                // 复现 bug 场景：上侧选中首个提交（详情同步注入），下侧
                // 保持未选中——下侧详情必须为空，不得串用上侧数据。
                if let Some(first) = data.left.commits.first().cloned() {
                    model.ui.compare_branches.left_view.selected = vec![first.id.clone()];
                    if let Ok(files) = lazecode_git::commit_files(&project_root, &first.id) {
                        model.ui.compare_branches.left_view.detail =
                            Some(lazecode_core::CompareDetail {
                                key: format!("c:{}", first.id),
                                files,
                                ..Default::default()
                            });
                    }
                }
                model.ui.compare_branches.sides = Some(data);
            }
            model.ui.panel_visible = false;
        }
        if let Some(commit) = project.log.first() {
            let id = commit.id.clone();
            project.selected_commit = Some(id.clone());
            project.commit_details = lazecode_git::commit_details(&project_root, &id).ok();
        }
        match mode.as_str() {
            "git" => {
                model.ui.panel_tab = PanelTab::Git;
                model.ui.panel_visible = true;
                model.ui.sidebar_view = SidebarView::Git;
            }
            "terminal" => {
                model.ui.terminal_visible = true;
                model.ui.panel_visible = false;
                model.ui.panel_visible = true;
                model.ui.sidebar_view = SidebarView::Consoles;
            }
            _ => {
                model.ui.panel_visible = true;
                model.ui.terminal_visible = true;
                model.ui.panel_visible = false;
                model.ui.sidebar_view = SidebarView::Files;
            }
        }
        model.add_loaded_project(project);
        // Give the PTY a moment to print its prompt.
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let mut runtime = UiRuntime::default();
    match mode.as_str() {
        "settings" => runtime.settings_open = true,
        "palette" => model.ui.command_palette_open = true,
        "search" => {
            runtime.search.open = true;
            runtime.search.query = "terminal".to_string();
            runtime.search.results = lazecode_fs::search_in_files(
                &project_root,
                "terminal",
                &lazecode_fs::SearchOptions {
                    max_results: 40,
                    ..Default::default()
                },
                None,
            )
            .unwrap_or_default();
            runtime.search.searched = true;
        }
        "branch_menu" => {
            runtime.open_menu(
                lazecode_ui::MenuKind::Branch,
                egui::Rect::from_min_size(egui::pos2(270.0, 10.0), egui::vec2(100.0, 24.0)),
                None,
            );
        }
        "worktree_menu" => {
            runtime.open_menu(
                lazecode_ui::MenuKind::Worktree,
                egui::Rect::from_min_size(egui::pos2(190.0, 10.0), egui::vec2(80.0, 24.0)),
                None,
            );
        }
        "project_menu" => {
            runtime.open_menu(
                lazecode_ui::MenuKind::Project,
                egui::Rect::from_min_size(egui::pos2(70.0, 10.0), egui::vec2(120.0, 24.0)),
                None,
            );
        }
        _ => {}
    }
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W, H));
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(SCALE),
            inner_rect: Some(rect),
            outer_rect: Some(rect),
            focused: Some(true),
            ..Default::default()
        },
    );
    let raw = egui::RawInput {
        screen_rect: Some(rect),
        viewports,
        time: Some(1.0),
        ..Default::default()
    };
    let output = ctx.run_ui(raw, |ui| {
        lazecode_ui::draw(ui, &mut model, &mut runtime);
    });
    let width = (W * SCALE) as u32;
    let height = (H * SCALE) as u32;
    let pixels = render(&ctx, output, width, height)?;

    save_png(&output_path, width, height, &pixels)?;
    println!("wrote {}", output_path.display());
    Ok(())
}

fn render(
    ctx: &egui::Context,
    output: egui::FullOutput,
    width: u32,
    height: u32,
) -> anyhow::Result<Vec<u8>> {
    use egui_wgpu::wgpu;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|err| anyhow::anyhow!("no adapter: {err}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("lazecode-snapshot"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    }))?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer =
        egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
    let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
    eprintln!("primitives={}", primitives.len());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point: output.pixels_per_point,
    };
    let mut textures = output.textures_delta;
    for (id, deltas) in &textures.set {
        for delta in deltas {
            renderer.update_texture(&device, &queue, *id, delta);
        }
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("snapshot"),
    });
    renderer.update_buffers(&device, &queue, &mut encoder, &primitives, &screen);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.043,
                            g: 0.055,
                            b: 0.067,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, &primitives, &screen);
        drop(pass);
    }
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snapshot-readback"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    for id in textures.free.drain() {
        renderer.free_texture(&id);
    }
    textures.clear();
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv()??;
    let data = slice
        .get_mapped_range()
        .map_err(|err| anyhow::anyhow!("map readback: {err}"))?;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * bytes_per_row as usize;
        pixels.extend_from_slice(&data[start..start + width as usize * 4]);
    }
    drop(data);
    buffer.unmap();
    Ok(pixels)
}

fn save_png(path: &Path, width: u32, height: u32, pixels: &[u8]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    Ok(())
}
