//! In-app updater UI: checks are background-only, installs happen on explicit
//! click, and the worker never blocks the window.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::{Align2, FontId, Key, Pos2, Rect, Sense, Ui, Vec2};
use lazecode_core::updater::{self, CheckOutcome};
use lazecode_core::{AppModel, Settings, ToastKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{DialogState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{checkbox, primary_button, secondary_button_enabled};

/// Delay after launch before the automatic check. Each launch checks once;
/// a manifest fetch is tiny and this keeps a newly published release visible
/// immediately after the user restarts.
pub const AUTO_CHECK_DELAY: Duration = Duration::from_secs(5);

/// Phase of the background update task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatePhase {
    Checking,
    Downloading,
    Installing,
}

impl UpdatePhase {
    fn label(self, zh: bool) -> &'static str {
        match (self, zh) {
            (UpdatePhase::Checking, true) => "正在检查更新…",
            (UpdatePhase::Checking, false) => "Checking for updates…",
            (UpdatePhase::Downloading, true) => "正在下载并校验…",
            (UpdatePhase::Downloading, false) => "Downloading and verifying…",
            (UpdatePhase::Installing, true) => "正在安装…",
            (UpdatePhase::Installing, false) => "Installing…",
        }
    }
}

/// Snapshot of the background updater task, polled by the UI thread.
#[derive(Clone, Debug, Default)]
pub struct UpdateProgress {
    pub phase: Option<UpdatePhase>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub done: bool,
    pub error: Option<String>,
    pub outcome: Option<CheckOutcome>,
    pub staged: Option<PathBuf>,
    pub installed: Option<String>,
    /// Where the new build was installed (bundle or executable), so the
    /// dialog can relaunch exactly what was replaced.
    pub installed_target: Option<PathBuf>,
}

/// Shared handle a worker thread writes and the UI polls once per frame.
#[derive(Default)]
pub struct UpdateJob {
    progress: Mutex<UpdateProgress>,
}

impl UpdateJob {
    fn snapshot(&self) -> UpdateProgress {
        self.progress
            .lock()
            .map(|progress| progress.clone())
            .unwrap_or_default()
    }

    fn with(&self, update: impl FnOnce(&mut UpdateProgress)) {
        if let Ok(mut progress) = self.progress.lock() {
            update(&mut progress);
        }
    }
}

/// The exact surface the update window should show. Derived from
/// [`UpdateState`] once per frame so the drawing code has no priority soup.
#[derive(Clone, Debug)]
pub enum UpdateView {
    Unconfigured,
    Idle,
    Checking,
    UpToDate {
        checked_at: Option<i64>,
    },
    Available {
        outcome: Box<CheckOutcome>,
        ignored: bool,
        error: Option<String>,
    },
    Downloading {
        downloaded: u64,
        total: Option<u64>,
    },
    Installing,
    Ready {
        version: String,
        error: Option<String>,
    },
    Failed {
        message: String,
    },
}

/// Top-bar update pill state.
#[derive(Clone, Debug)]
pub struct UpdateBadge {
    pub text: String,
    pub tooltip: String,
}

/// Everything the update window needs across frames. Lives in `UiRuntime`
/// so progress and auto-check results survive closing the window.
#[derive(Default)]
pub struct UpdateState {
    /// Last successful check, including "already up to date".
    pub available: Option<CheckOutcome>,
    /// Verified artifact waiting to be installed.
    pub staged: Option<PathBuf>,
    /// Running background task.
    pub job: Option<Arc<UpdateJob>>,
    /// Error from the most recent task.
    pub error: Option<String>,
    /// Version installed in this session (restart required).
    pub installed: Option<String>,
    /// Install location of that version (bundle or executable).
    pub installed_target: Option<PathBuf>,
    /// Automatic check already scheduled this session.
    auto_checked: bool,
    /// When the delayed automatic check should run.
    auto_check_at: Option<Instant>,
    /// The next completed check is an automatic check and may raise a toast.
    auto_notify: bool,
    /// Version already announced in a toast, so it is not repeated.
    notified: Option<String>,
    /// Last check attempt, for the "up to date" page and settings.
    pub last_checked_at: Option<i64>,
    /// Local checkbox state for "ignore this version".
    pub ignore_checked: bool,
    /// Version the local ignore checkbox belongs to.
    ignore_version: Option<String>,
    /// Markdown cache for release notes; reset when the version changes.
    pub notes_cache: egui_commonmark::CommonMarkCache,
}

impl UpdateState {
    pub fn busy(&self) -> bool {
        self.job
            .as_ref()
            .map(|job| job.snapshot())
            .is_some_and(|progress| progress.phase.is_some() && !progress.done)
    }

    /// Start a manual or automatic check, recording it in the persisted
    /// settings. Returns `false` when updates are disabled or a task is busy.
    pub fn begin_check(
        &mut self,
        settings: &mut Settings,
        current_version: &str,
        automatic: bool,
    ) -> bool {
        if self.busy() || !settings.updates_configured() {
            return false;
        }
        let now = unix_now();
        self.last_checked_at = Some(now);
        settings.last_update_check_time = now;
        self.error = None;
        self.auto_notify = automatic;
        self.auto_check_at = None;
        self.check(
            &settings.update_endpoint,
            current_version,
            &settings.update_public_key,
        );
        true
    }

    /// Fetch and verify the manifest; runs off the UI thread.
    fn check(&mut self, endpoint: &str, current_version: &str, public_key: &str) {
        if self.busy() {
            return;
        }
        let endpoint = endpoint.to_string();
        let current_version = current_version.to_string();
        let public_key = public_key.to_string();
        self.start(UpdatePhase::Checking, move |job| {
            match updater::check(&endpoint, &current_version, &public_key) {
                Ok(outcome) => job.with(|progress| progress.outcome = Some(outcome)),
                Err(err) => job.with(|progress| progress.error = Some(err.to_string())),
            }
        });
    }

    /// Download, verify and install the announced artifact in one task.
    /// The caller has already shown the release notes and obtained consent.
    pub fn download_and_install(
        &mut self,
        manifest: updater::Manifest,
        public_key: &str,
        dest: PathBuf,
    ) {
        if self.busy() {
            return;
        }
        let public_key = public_key.to_string();
        let version = manifest.version.clone();
        let kind = manifest.artifact_kind();
        self.error = None;
        self.start(UpdatePhase::Downloading, move |job| {
            let result = updater::download_and_verify(
                &manifest,
                &public_key,
                &dest,
                &mut |downloaded, total| {
                    job.with(|progress| {
                        progress.downloaded = downloaded;
                        progress.total = total;
                    });
                },
            );
            if let Err(err) = result {
                job.with(|progress| progress.error = Some(err.to_string()));
                return;
            }
            job.with(|progress| {
                progress.phase = Some(UpdatePhase::Installing);
                progress.staged = Some(dest.clone());
            });
            let target = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("LazeCode"));
            match updater::install_artifact(&dest, &target, kind) {
                Ok(report) => {
                    if let Some(backup) = &report.backup {
                        log::info!("previous build kept at {}", backup.display());
                    }
                    job.with(|progress| {
                        progress.installed = Some(version.clone());
                        progress.installed_target = Some(report.target.clone());
                        progress.staged = None;
                    });
                }
                Err(err) => job.with(|progress| progress.error = Some(err.to_string())),
            }
        });
    }

    /// Install a previously verified staged artifact (retry path).
    fn install_staged(&mut self) {
        if self.busy() {
            return;
        }
        let Some(staged) = self.staged.clone() else {
            return;
        };
        let Some(outcome) = self.available.as_ref() else {
            return;
        };
        let kind = outcome.manifest.artifact_kind();
        let version = outcome.manifest.version.clone();
        self.error = None;
        self.start(UpdatePhase::Installing, move |job| {
            let target = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("LazeCode"));
            match updater::install_artifact(&staged, &target, kind) {
                Ok(report) => {
                    if let Some(backup) = &report.backup {
                        log::info!("previous build kept at {}", backup.display());
                    }
                    job.with(|progress| {
                        progress.installed = Some(version.clone());
                        progress.installed_target = Some(report.target.clone());
                        progress.staged = None;
                    });
                }
                Err(err) => job.with(|progress| progress.error = Some(err.to_string())),
            }
        });
    }

    /// Retry whatever failed: install if a verified artifact is still
    /// staged, download again if a manifest is known, otherwise re-check.
    pub fn retry(&mut self, settings: &mut Settings, current_version: &str) {
        if self.busy() {
            return;
        }
        if self.staged.is_some() {
            self.install_staged();
        } else if let Some(outcome) = self
            .available
            .as_ref()
            .filter(|outcome| outcome.update_available)
        {
            let manifest = outcome.manifest.clone();
            let dest = updater::staged_path(outcome.available_version());
            self.download_and_install(manifest, &settings.update_public_key, dest);
        } else {
            self.begin_check(settings, current_version, false);
        }
    }

    /// Start the freshly installed build and let the caller quit so it takes
    /// over.
    pub fn relaunch_installed(&mut self) -> Result<(), String> {
        if self.busy() {
            return Err("an update task is still running".into());
        }
        let target = self
            .installed_target
            .clone()
            .ok_or_else(|| "nothing was installed yet".to_string())?;
        updater::relaunch(&target).map_err(|err| err.to_string())
    }

    /// Poll the worker, schedule the post-launch check and raise the one-time
    /// "new version" toast for automatic checks.
    pub fn tick(&mut self, model: &mut AppModel) -> bool {
        let had_job = self.job.is_some();
        let mut changed = false;
        if self.last_checked_at.is_none() && model.settings.last_update_check_time > 0 {
            self.last_checked_at = Some(model.settings.last_update_check_time);
        }

        if self.job.is_some() {
            self.poll();
            changed |= had_job && self.job.is_none();
        }

        if !self.auto_checked {
            self.auto_checked = true;
            if model.settings.auto_check_updates && model.settings.updates_configured() {
                self.auto_check_at = Some(Instant::now() + AUTO_CHECK_DELAY);
            }
        }

        if let Some(at) = self.auto_check_at {
            if !model.settings.auto_check_updates || !model.settings.updates_configured() {
                self.auto_check_at = None;
            } else if Instant::now() >= at {
                self.auto_check_at = None;
                if self.available.is_none()
                    && !self.busy()
                    && self.begin_check(&mut model.settings, model.version, true)
                {
                    model.save_settings();
                    changed = true;
                }
            }
        }

        if self.auto_notify && !self.busy() {
            if let Some(outcome) = self.available.as_ref() {
                if outcome.update_available
                    && self.error.is_none()
                    && !version_is_ignored(
                        outcome.available_version(),
                        &model.settings.ignored_update_version,
                    )
                {
                    let version = outcome.available_version().to_string();
                    if self.notified.as_deref() != Some(version.as_str()) {
                        self.notified = Some(version.clone());
                        let zh = model.settings.language.is_zh();
                        model.toast(
                            if zh {
                                format!("发现新版本 {version}，点击顶栏提示查看更新。")
                            } else {
                                format!(
                                    "Version {version} is available; use the title-bar badge to review it."
                                )
                            },
                            ToastKind::Info,
                        );
                    }
                }
            }
            self.auto_notify = false;
        }
        changed
    }

    /// Delay until the delayed automatic check should run, if one is
    /// scheduled. The shell uses it to wake an otherwise idle event loop.
    pub fn next_wakeup(&self) -> Option<Duration> {
        self.auto_check_at
            .map(|at| at.saturating_duration_since(Instant::now()))
    }

    fn poll(&mut self) {
        let Some(job) = self.job.clone() else {
            return;
        };
        let progress = job.snapshot();
        if progress.phase.is_none() || !progress.done {
            return;
        }
        self.job = None;
        self.error = progress.error;
        if let Some(outcome) = progress.outcome {
            let version = outcome.available_version().to_string();
            if self
                .available
                .as_ref()
                .map(|current| current.available_version())
                != Some(version.as_str())
            {
                self.staged = None;
                self.notes_cache = egui_commonmark::CommonMarkCache::default();
            }
            self.available = Some(outcome);
        }
        if let Some(staged) = progress.staged {
            self.staged = Some(staged);
        }
        if let Some(version) = progress.installed {
            self.installed = Some(version);
            self.staged = None;
        }
        if let Some(target) = progress.installed_target {
            self.installed_target = Some(target);
        }
    }

    fn start(&mut self, phase: UpdatePhase, task: impl FnOnce(&UpdateJob) + Send + 'static) {
        let job = Arc::new(UpdateJob::default());
        job.with(|progress| progress.phase = Some(phase));
        self.error = None;
        self.job = Some(job.clone());
        if let Err(err) = std::thread::Builder::new()
            .name("lazecode-update".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(&job)));
                if result.is_err() {
                    job.with(|progress| {
                        progress.error = Some("internal updater task failed".into());
                    });
                }
                job.with(|progress| progress.done = true);
            })
        {
            self.error = Some(err.to_string());
            self.job = None;
        }
    }

    /// Keep the local ignore checkbox in sync with the version currently
    /// shown and the persisted setting.
    pub fn sync_ignore(&mut self, ignored: &str) {
        let current = self
            .available
            .as_ref()
            .filter(|outcome| outcome.update_available)
            .map(|outcome| outcome.available_version().to_string());
        match current {
            Some(version) if self.ignore_version.as_deref() != Some(version.as_str()) => {
                self.ignore_version = Some(version.clone());
                self.ignore_checked = version_is_ignored(&version, ignored);
            }
            None => {
                self.ignore_version = None;
                self.ignore_checked = false;
            }
            _ => {}
        }
    }

    pub fn view(&self, settings: &Settings) -> UpdateView {
        if !settings.updates_configured() {
            return UpdateView::Unconfigured;
        }
        if let Some(job) = &self.job {
            let progress = job.snapshot();
            if !progress.done {
                match progress.phase {
                    Some(UpdatePhase::Checking) => return UpdateView::Checking,
                    Some(UpdatePhase::Downloading) => {
                        return UpdateView::Downloading {
                            downloaded: progress.downloaded,
                            total: progress.total,
                        }
                    }
                    Some(UpdatePhase::Installing) => return UpdateView::Installing,
                    None => {}
                }
            }
        }
        if let Some(version) = &self.installed {
            return UpdateView::Ready {
                version: version.trim_start_matches('v').to_string(),
                error: self.error.clone(),
            };
        }
        if let Some(outcome) = &self.available {
            if outcome.update_available {
                return UpdateView::Available {
                    outcome: Box::new(outcome.clone()),
                    ignored: version_is_ignored(
                        outcome.available_version(),
                        &settings.ignored_update_version,
                    ),
                    error: self.error.clone(),
                };
            }
            return UpdateView::UpToDate {
                checked_at: self.last_checked_at,
            };
        }
        if let Some(error) = &self.error {
            return UpdateView::Failed {
                message: error.clone(),
            };
        }
        UpdateView::Idle
    }

    pub fn badge(&self, settings: &Settings) -> Option<UpdateBadge> {
        if let Some(version) = &self.installed {
            let version = version.trim_start_matches('v');
            let zh = settings.language.is_zh();
            return Some(UpdateBadge {
                text: if zh {
                    format!("v{version} 待重启")
                } else {
                    format!("Restart for v{version}")
                },
                tooltip: if zh {
                    format!("LazeCode {version} 已安装，点击查看并重启。")
                } else {
                    format!("LazeCode {version} is installed; click to review and restart.")
                },
            });
        }
        if let Some(job) = &self.job {
            let progress = job.snapshot();
            if !progress.done {
                match progress.phase {
                    Some(UpdatePhase::Downloading) => {
                        let ratio = progress
                            .total
                            .filter(|total| *total > 0)
                            .map(|total| {
                                (progress.downloaded as f32 / total as f32).clamp(0.0, 1.0)
                            })
                            .unwrap_or(0.0);
                        return Some(UpdateBadge {
                            text: if settings.language.is_zh() {
                                format!("更新中 {:.0}%", ratio * 100.0)
                            } else {
                                format!("Updating {:.0}%", ratio * 100.0)
                            },
                            tooltip: if settings.language.is_zh() {
                                "正在下载并校验更新，点击查看进度。".into()
                            } else {
                                "Downloading and verifying the update; click to see progress."
                                    .into()
                            },
                        });
                    }
                    Some(UpdatePhase::Installing) => {
                        return Some(UpdateBadge {
                            text: if settings.language.is_zh() {
                                "正在安装…".into()
                            } else {
                                "Installing…".into()
                            },
                            tooltip: if settings.language.is_zh() {
                                "正在安装更新，点击查看进度。".into()
                            } else {
                                "Installing the update; click to see progress.".into()
                            },
                        });
                    }
                    _ => return None,
                }
            }
        }
        let outcome = self.available.as_ref()?;
        if !outcome.update_available
            || version_is_ignored(
                outcome.available_version(),
                &settings.ignored_update_version,
            )
        {
            return None;
        }
        let version = outcome.available_version().trim_start_matches('v');
        let zh = settings.language.is_zh();
        Some(UpdateBadge {
            text: if zh {
                format!("v{version} 可更新")
            } else {
                format!("v{version} available")
            },
            tooltip: if zh {
                format!("LazeCode {version} 可用，点击查看更新。")
            } else {
                format!("LazeCode {version} is available. Click to view the update.")
            },
        })
    }
}

/// Compare versions while tolerating a leading `v` on either side.
pub fn version_is_ignored(version: &str, ignored: &str) -> bool {
    let version = version.trim().trim_start_matches('v');
    let ignored = ignored.trim().trim_start_matches('v');
    !ignored.is_empty() && version == ignored
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index + 1 < UNITS.len() {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

pub fn checked_label(checked_at: Option<i64>, zh: bool) -> String {
    let Some(checked_at) = checked_at.filter(|timestamp| *timestamp > 0) else {
        return if zh {
            "尚未成功检查".into()
        } else {
            "Not checked yet".into()
        };
    };
    let delta = unix_now().saturating_sub(checked_at).max(0);
    if zh {
        match delta {
            0..=59 => "刚刚".into(),
            60..=3_599 => format!("{} 分钟前", delta / 60),
            3_600..=86_399 => format!("{} 小时前", delta / 3_600),
            _ => format!("{} 天前", delta / 86_400),
        }
    } else {
        match delta {
            0..=59 => "just now".into(),
            60..=3_599 => format!("{} min ago", delta / 60),
            3_600..=86_399 => format!("{} h ago", delta / 3_600),
            _ => format!("{} d ago", delta / 86_400),
        }
    }
}

/// Draw the modal update window. Called by `overlays::draw`.
pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, screen: Rect) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let ignored = model.settings.ignored_update_version.clone();
    rt.update.sync_ignore(&ignored);
    let view = rt.update.view(&model.settings);
    let can_close = !matches!(
        view,
        UpdateView::Downloading { .. } | UpdateView::Installing
    );

    if can_close && ui.input(|input| input.key_pressed(Key::Escape)) {
        rt.update_open = false;
        return;
    }

    ui.painter()
        .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(150));
    let height = match &view {
        UpdateView::Available { .. } => 540.0,
        UpdateView::Failed { .. } => 420.0,
        UpdateView::Downloading { .. } => 380.0,
        UpdateView::Installing | UpdateView::Ready { .. } => 360.0,
        UpdateView::Unconfigured
        | UpdateView::Idle
        | UpdateView::Checking
        | UpdateView::UpToDate { .. } => 340.0,
    };
    let rect = Rect::from_center_size(screen.center(), Vec2::new(600.0, height));
    crate::overlays::frame(ui, rect, theme);

    let close = Rect::from_center_size(
        Pos2::new(rect.right() - 20.0, rect.top() + 20.0),
        Vec2::splat(22.0),
    );
    if can_close {
        let close_response = ui.interact(close, ui.scope_id().with("update-close"), Sense::click());
        paint_icon(
            ui.painter(),
            close.shrink(4.0),
            Icon::X,
            if close_response.hovered() {
                p.text
            } else {
                p.text3
            },
        );
        if close_response.clicked() {
            rt.update_open = false;
            return;
        }
    }

    let title = match &view {
        UpdateView::Unconfigured | UpdateView::Idle => {
            crate::tr!(model, "Software Update", "软件更新")
        }
        UpdateView::Checking => crate::tr!(model, "Checking for updates", "正在检查更新"),
        UpdateView::UpToDate { .. } => crate::tr!(model, "You're up to date", "已是最新版本"),
        UpdateView::Available { .. } => crate::tr!(model, "Update available", "发现新版本"),
        UpdateView::Downloading { .. } => crate::tr!(model, "Downloading update", "正在下载更新"),
        UpdateView::Installing => crate::tr!(model, "Installing update", "正在安装更新"),
        UpdateView::Ready { .. } => crate::tr!(model, "Update ready", "更新已就绪"),
        UpdateView::Failed { .. } => crate::tr!(model, "Update failed", "更新失败"),
    };
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 28.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(16.0),
        p.text,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 50.0),
        Align2::LEFT_CENTER,
        format!(
            "{} {}",
            crate::tr!(model, "Current version", "当前版本"),
            model.version
        ),
        FontId::proportional(11.0),
        p.text3,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 66.5,
        egui::Stroke::new(1.0, p.border_soft),
    );

    let body_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 22.0, rect.top() + 80.0),
        Pos2::new(rect.right() - 22.0, rect.bottom() - 58.0),
    );
    let mut body = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body_rect)
            .id_salt("update-body")
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    body.set_clip_rect(body_rect.intersect(ui.clip_rect()));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut body, |ui| {
            ui.set_width(body_rect.width());
            draw_body(ui, model, rt, theme, &view, zh);
        });

    draw_footer(ui, model, rt, theme, rect, &view, zh);
}

fn draw_body(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    view: &UpdateView,
    zh: bool,
) {
    let p = theme.palette;
    match view {
        UpdateView::Unconfigured => {
            ui.label(
                egui::RichText::new(crate::tr!(
                    model,
                    "Updates are disabled. Set the update endpoint and the pinned public key in Settings → Updates.",
                    "更新功能未启用。请在 设置 → 更新 中填写更新地址与固定的公钥。"
                ))
                .font(FontId::proportional(12.5))
                .color(p.text2),
            );
        }
        UpdateView::Idle => {
            ui.label(
                egui::RichText::new(if zh {
                    format!(
                        "更新地址：{}\n\n点击“检查更新”获取发布清单。签名与 SHA-256 校验通过后才会下载。",
                        model.settings.update_endpoint
                    )
                } else {
                    format!(
                        "Endpoint: {}\n\nChoose “Check for updates”. The manifest signature and the artifact SHA-256 are verified before anything is installed.",
                        model.settings.update_endpoint
                    )
                })
                .font(FontId::proportional(12.0))
                .color(p.text2),
            );
        }
        UpdateView::Checking => {
            ui.add(egui::Spinner::new().size(18.0).color(p.accent));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(UpdatePhase::Checking.label(zh))
                    .font(FontId::proportional(13.0))
                    .color(p.text),
            );
            ui.label(
                egui::RichText::new(crate::tr!(
                    model,
                    "The manifest is fetched and signature-checked. Nothing is downloaded yet.",
                    "正在获取并校验发布清单，此时不会下载安装包。"
                ))
                .font(FontId::proportional(11.0))
                .color(p.text3),
            );
        }
        UpdateView::UpToDate { checked_at } => {
            let row = ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                paint_icon(ui.painter(), icon_rect.shrink(2.0), Icon::Check, p.green);
                ui.label(
                    egui::RichText::new(if zh {
                        format!("已是最新版本（{}）。", model.version)
                    } else {
                        format!("LazeCode is up to date ({}).", model.version)
                    })
                    .font(FontId::proportional(13.0))
                    .color(p.text),
                );
            });
            let _ = row;
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(if zh {
                    format!("上次检查：{}", checked_label(*checked_at, true))
                } else {
                    format!("Last checked: {}", checked_label(*checked_at, false))
                })
                .font(FontId::proportional(11.0))
                .color(p.text3),
            );
        }
        UpdateView::Available {
            outcome,
            ignored,
            error,
        } => {
            ui.label(
                egui::RichText::new(if zh {
                    format!(
                        "v{} 已发布（当前 v{}）",
                        outcome.available_version().trim_start_matches('v'),
                        outcome.current_version
                    )
                } else {
                    format!(
                        "v{} is available (current v{}).",
                        outcome.available_version().trim_start_matches('v'),
                        outcome.current_version
                    )
                })
                .font(FontId::proportional(14.0))
                .strong()
                .color(p.text),
            );
            if let Some(date) = outcome.manifest.release_date() {
                ui.label(
                    egui::RichText::new(if zh {
                        format!("发布日期：{date}")
                    } else {
                        format!("Released: {date}")
                    })
                    .font(FontId::proportional(11.0))
                    .color(p.text3),
                );
            }
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(p.panel2)
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("update-notes")
                        .max_height(220.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if let Some(notes) = outcome.manifest.release_notes() {
                                egui_commonmark::CommonMarkViewer::new().show(
                                    ui,
                                    &mut rt.update.notes_cache,
                                    notes,
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(crate::tr!(
                                        model,
                                        "No release notes were published.",
                                        "该版本没有发布说明。"
                                    ))
                                    .font(FontId::proportional(11.5))
                                    .color(p.text3),
                                );
                            }
                        });
                });
            if let Some(error) = error {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(error)
                        .font(FontId::proportional(10.5))
                        .color(p.rose),
                );
            }
            ui.add_space(8.0);
            let mut ignore = rt.update.ignore_checked;
            ui.horizontal(|ui| {
                let (checkbox_rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                if checkbox(ui, checkbox_rect, ignore, theme).clicked() {
                    ignore = !ignore;
                }
                ui.label(
                    egui::RichText::new(crate::tr!(model, "Ignore this version", "忽略此版本"))
                        .font(FontId::proportional(11.0))
                        .color(p.text2),
                );
            });
            rt.update.ignore_checked = ignore;
            if *ignored {
                ui.label(
                    egui::RichText::new(crate::tr!(
                        model,
                        "This version is currently ignored in Settings → Updates.",
                        "该版本已在 设置 → 更新 中被忽略。"
                    ))
                    .font(FontId::proportional(10.5))
                    .color(p.text3),
                );
            }
        }
        UpdateView::Downloading { downloaded, total } => {
            let ratio = total
                .filter(|total| *total > 0)
                .map(|total| (*downloaded as f32 / total as f32).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            ui.label(
                egui::RichText::new(UpdatePhase::Downloading.label(zh))
                    .font(FontId::proportional(13.0))
                    .color(p.text),
            );
            ui.add_space(10.0);
            let (bar, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
            ui.painter()
                .rect_filled(bar, egui::CornerRadius::same(4), p.panel2);
            ui.painter().rect_filled(
                Rect::from_min_size(bar.min, Vec2::new(bar.width() * ratio, bar.height())),
                egui::CornerRadius::same(4),
                p.accent,
            );
            ui.add_space(6.0);
            let bytes = match total {
                Some(total) if *total > 0 => {
                    format!("{} / {}", human_bytes(*downloaded), human_bytes(*total))
                }
                _ => human_bytes(*downloaded),
            };
            ui.horizontal(|ui| {
                if total.is_some_and(|total| total > 0) {
                    ui.label(
                        egui::RichText::new(format!("{:.0}%", ratio * 100.0))
                            .font(FontId::monospace(11.0))
                            .color(p.text2),
                    );
                }
                ui.label(
                    egui::RichText::new(bytes)
                        .font(FontId::monospace(11.0))
                        .color(p.text3),
                );
            });
            ui.label(
                egui::RichText::new(crate::tr!(
                    model,
                    "The download is checked against the manifest SHA-256 and Ed25519 signature before installation.",
                    "下载完成后会按清单校验 SHA-256 与 Ed25519 签名，再执行安装。"
                ))
                .font(FontId::proportional(10.5))
                .color(p.text3),
            );
        }
        UpdateView::Installing => {
            ui.add(egui::Spinner::new().size(18.0).color(p.accent));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(UpdatePhase::Installing.label(zh))
                    .font(FontId::proportional(13.0))
                    .color(p.text),
            );
            ui.label(
                egui::RichText::new(crate::tr!(
                    model,
                    "Replacing the application bundle. Keep LazeCode open until this finishes.",
                    "正在替换应用包，完成前请保持 LazeCode 打开。"
                ))
                .font(FontId::proportional(11.0))
                .color(p.text3),
            );
        }
        UpdateView::Ready { version, error } => {
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                paint_icon(ui.painter(), icon_rect.shrink(2.0), Icon::Check, p.green);
                ui.label(
                    egui::RichText::new(if zh {
                        format!("{version} 已安装，重启后生效。")
                    } else {
                        format!("{version} is installed and takes effect after a restart.")
                    })
                    .font(FontId::proportional(13.0))
                    .color(p.text),
                );
            });
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(crate::tr!(
                    model,
                    "Restarting closes all windows and running console commands. Save any open files first.",
                    "重启会关闭所有窗口和运行中的控制台命令，请先保存打开的文件。"
                ))
                .font(FontId::proportional(11.0))
                .color(p.text3),
            );
            if let Some(error) = error {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(error)
                        .font(FontId::proportional(10.5))
                        .color(p.rose),
                );
            }
        }
        UpdateView::Failed { message } => {
            ui.label(
                egui::RichText::new(crate::tr!(
                    model,
                    "The update could not be checked or installed.",
                    "更新检查或安装失败。"
                ))
                .font(FontId::proportional(13.0))
                .color(p.rose),
            );
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(p.panel2)
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(message)
                            .font(FontId::monospace(10.5))
                            .color(p.text2),
                    );
                });
        }
    }
}

fn draw_footer(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    view: &UpdateView,
    zh: bool,
) {
    let p = theme.palette;
    let y = rect.bottom() - 44.0;
    let height = 30.0;
    let gap = 8.0;
    let close = |rt: &mut UiRuntime| rt.update_open = false;
    let right = rect.right() - 22.0;

    match view {
        UpdateView::Unconfigured => {
            let primary =
                Rect::from_min_size(Pos2::new(right - 120.0, y), Vec2::new(120.0, height));
            if primary_button(
                ui,
                primary,
                crate::tr!(model, "Open Settings", "打开设置"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                rt.update_open = false;
                rt.settings_open = true;
                model.ui.settings_section = 6;
            }
            let secondary = Rect::from_min_size(
                Pos2::new(primary.left() - gap - 80.0, y),
                Vec2::new(80.0, height),
            );
            if secondary_button_enabled(
                ui,
                secondary,
                crate::tr!(model, "Close", "关闭"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                close(rt);
            }
        }
        UpdateView::Idle => {
            let primary =
                Rect::from_min_size(Pos2::new(right - 110.0, y), Vec2::new(110.0, height));
            if primary_button(
                ui,
                primary,
                crate::tr!(model, "Check for updates", "检查更新"),
                None,
                model.settings.updates_configured() && !rt.update.busy(),
                theme,
            )
            .clicked()
                && rt
                    .update
                    .begin_check(&mut model.settings, model.version, false)
            {
                model.save_settings();
            }
            let secondary = Rect::from_min_size(
                Pos2::new(primary.left() - gap - 80.0, y),
                Vec2::new(80.0, height),
            );
            if secondary_button_enabled(
                ui,
                secondary,
                crate::tr!(model, "Close", "关闭"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                close(rt);
            }
        }
        UpdateView::Checking => {
            let secondary =
                Rect::from_min_size(Pos2::new(right - 80.0, y), Vec2::new(80.0, height));
            if secondary_button_enabled(
                ui,
                secondary,
                crate::tr!(model, "Close", "关闭"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                // The check continues in the background; a found update will
                // surface as the title-bar badge.
                close(rt);
            }
        }
        UpdateView::UpToDate { .. } => {
            let primary = Rect::from_min_size(Pos2::new(right - 80.0, y), Vec2::new(80.0, height));
            if primary_button(ui, primary, "OK", None, true, theme).clicked() {
                close(rt);
            }
        }
        UpdateView::Available { outcome, error, .. } => {
            let retry = error.is_some();
            let primary_width = if retry { 132.0 } else { 150.0 };
            let primary = Rect::from_min_size(
                Pos2::new(right - primary_width, y),
                Vec2::new(primary_width, height),
            );
            let primary_label = if retry {
                crate::tr!(model, "Retry update", "重试更新")
            } else {
                crate::tr!(model, "Update and restart", "更新并重启")
            };
            if primary_button(ui, primary, primary_label, None, !rt.update.busy(), theme).clicked()
            {
                if retry {
                    rt.update.retry(&mut model.settings, model.version);
                    model.save_settings();
                } else {
                    let dest = updater::staged_path(outcome.available_version());
                    rt.update.download_and_install(
                        outcome.manifest.clone(),
                        &model.settings.update_public_key,
                        dest,
                    );
                }
            }
            let later = Rect::from_min_size(
                Pos2::new(primary.left() - gap - 110.0, y),
                Vec2::new(110.0, height),
            );
            if secondary_button_enabled(
                ui,
                later,
                crate::tr!(model, "Remind me later", "稍后提醒"),
                None,
                !rt.update.busy(),
                theme,
            )
            .clicked()
            {
                if rt.update.ignore_checked {
                    model.settings.ignored_update_version = outcome.available_version().to_string();
                    model.save_settings();
                }
                close(rt);
            }
        }
        UpdateView::Downloading { .. } | UpdateView::Installing => {
            let label = match view {
                UpdateView::Downloading { .. } => {
                    crate::tr!(model, "Downloading…", "正在下载…")
                }
                _ => crate::tr!(model, "Installing…", "正在安装…"),
            };
            let primary =
                Rect::from_min_size(Pos2::new(right - 150.0, y), Vec2::new(150.0, height));
            primary_button(ui, primary, label, None, false, theme);
            let _ = p;
        }
        UpdateView::Ready { version, .. } => {
            let primary =
                Rect::from_min_size(Pos2::new(right - 130.0, y), Vec2::new(130.0, height));
            if primary_button(
                ui,
                primary,
                crate::tr!(model, "Restart now", "立即重启"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                let dirty = model
                    .projects
                    .iter()
                    .any(|project| project.tabs.iter().any(|tab| tab.is_dirty()));
                if dirty {
                    rt.update_open = false;
                    rt.dialog = DialogState::Quit {
                        commands: Vec::new(),
                        restart_update: true,
                    };
                } else {
                    match rt.update.relaunch_installed() {
                        Ok(()) => model.should_quit = true,
                        Err(err) => rt.update.error = Some(err),
                    }
                }
            }
            let later = Rect::from_min_size(
                Pos2::new(primary.left() - gap - 110.0, y),
                Vec2::new(110.0, height),
            );
            if secondary_button_enabled(
                ui,
                later,
                crate::tr!(model, "Restart later", "稍后重启"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                close(rt);
            }
            let _ = version;
            let _ = zh;
        }
        UpdateView::Failed { .. } => {
            let primary = Rect::from_min_size(Pos2::new(right - 90.0, y), Vec2::new(90.0, height));
            if primary_button(
                ui,
                primary,
                crate::tr!(model, "Retry", "重试"),
                None,
                !rt.update.busy(),
                theme,
            )
            .clicked()
            {
                rt.update.retry(&mut model.settings, model.version);
                model.save_settings();
            }
            let secondary = Rect::from_min_size(
                Pos2::new(primary.left() - gap - 80.0, y),
                Vec2::new(80.0, height),
            );
            if secondary_button_enabled(
                ui,
                secondary,
                crate::tr!(model, "Close", "关闭"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                close(rt);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_versions_tolerate_a_leading_v() {
        assert!(version_is_ignored("v0.4.7", "0.4.7"));
        assert!(version_is_ignored("0.4.7", "v0.4.7"));
        assert!(!version_is_ignored("0.4.7", "0.4.8"));
        assert!(!version_is_ignored("0.4.7", ""));
    }

    fn outcome(version: &str, update_available: bool) -> CheckOutcome {
        CheckOutcome {
            current_version: "0.4.6".into(),
            manifest: updater::Manifest {
                version: version.into(),
                url: "https://updates.example.test/app.tar.gz".into(),
                sha256: "aa".repeat(32),
                signature: String::new(),
                notes: "Release notes".into(),
                date: None,
                min_os: None,
                arch: None,
                kind: "app-bundle".into(),
            },
            update_available,
        }
    }

    #[test]
    fn available_update_ignores_badge_but_keeps_the_dialog_state() {
        let mut settings = Settings::default();
        let state = UpdateState {
            available: Some(outcome("9.9.9", true)),
            ..Default::default()
        };
        assert!(matches!(
            state.view(&settings),
            UpdateView::Available { .. }
        ));
        assert!(state.badge(&settings).is_some());

        settings.ignored_update_version = "v9.9.9".into();
        assert!(matches!(
            state.view(&settings),
            UpdateView::Available { ignored: true, .. }
        ));
        assert!(state.badge(&settings).is_none());
    }

    #[test]
    fn installed_version_wins_over_a_previous_outcome() {
        let settings = Settings::default();
        let state = UpdateState {
            available: Some(outcome("9.9.9", true)),
            installed: Some("9.9.9".into()),
            ..Default::default()
        };
        assert!(matches!(
            state.view(&settings),
            UpdateView::Ready { version, .. } if version == "9.9.9"
        ));
        assert!(state
            .badge(&settings)
            .is_some_and(|badge| badge.text.contains("9.9.9")));
    }

    #[test]
    fn up_to_date_outcome_has_no_badge() {
        let settings = Settings::default();
        let state = UpdateState {
            available: Some(outcome("0.4.6", false)),
            ..Default::default()
        };
        assert!(matches!(state.view(&settings), UpdateView::UpToDate { .. }));
        assert!(state.badge(&settings).is_none());
    }

    #[test]
    fn checked_label_reports_recent_and_old_attempts() {
        let now = unix_now();
        assert!(checked_label(Some(now), false).contains("just now"));
        assert!(checked_label(Some(now - 120), true).contains("2 分钟前"));
        assert!(checked_label(None, false).contains("Not checked"));
    }
}
