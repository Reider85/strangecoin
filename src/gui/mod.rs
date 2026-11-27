//! GUI-клиент (egui/eframe) за feature-флагом `gui` (скелет P02; перенесён из
//! `lib.rs` при исправлении BUG-S0-020).
//!
//! UI-P05 strangler closeout: legacy `WalletApp` удалён. Точка входа `GuiApp`
//! всегда поднимает профессиональный UI через `StranglerLauncher`.
//!
//! [UI-P02-B] eframe 0.22 → 0.30 API audit: the ONLY breaking change on this
//! file's surface was `AppCreator` returning `Result` (fixed in [UI-P02-A]).
//! `NativeOptions::default()`, `eframe::App::update(&mut self, ctx, frame)`,
//! `ctx.request_repaint()`, `ctx.input(|i| i.time)` and the widget calls used
//! here (CentralPanel/TextEdit/spinner) are unchanged in 0.30 — no
//! ViewportBuilder or font-API migration was required.

use std::path::PathBuf;

use crate::{MiningTask, Node};

pub mod strangler_migration;

/// Минимальная точка входа GUI: рендеринг целиком живёт в `StranglerLauncher`
/// (профессиональный UI), legacy-ветки больше нет.
pub struct GuiApp {
    pub node: Node,
    pub data_dir: PathBuf,
    pub ui: crate::config::UiConfig,
    pub mining_tx: tokio::sync::mpsc::UnboundedSender<MiningTask>,
}

/// Strangler migration launcher (professional UI only).
pub use strangler_migration::{run_strangler_launcher, ScreenType, StranglerLauncher};

/// Запускает egui event loop на выделенном blocking-потоке (ADR-0007):
/// `eframe::run_native` блокирующий и владеет windowing event loop, поэтому
/// не должен занимать worker runtime.
///
/// [UI-P02-B] Known PRE-EXISTING issue (not introduced by the 0.22 → 0.30
/// bump — the identical panic exists in winit 0.28.7 `event_loop.rs`): winit
/// panics when the EventLoop is created off the main thread on Windows, и
/// `spawn_blocking` is not the main thread. The Windows GUI has failed this
/// way since ADR-0007; fixing it (either `EventLoopBuilderExtWindows::
/// any_thread` through `NativeOptions::event_loop_builder`, or moving the
/// GUI back to the main thread) is a separate follow-up, outside CI4 Prompt 6.
pub async fn run(app: GuiApp) {
    let strangler_app =
        StranglerLauncher::with_ui_config(app.node, app.data_dir, &app.ui, app.mining_tx);
    run_strangler_launcher(strangler_app).await;
}

/// Run the strangler migration launcher directly
pub async fn run_strangler_gui(node: Node, data_dir: PathBuf) {
    let strangler_app = StranglerLauncher::new(node, data_dir);
    run_strangler_launcher(strangler_app).await;
}
