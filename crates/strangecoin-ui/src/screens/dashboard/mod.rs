//! Node operator dashboard (UI-P04, ADR-0031 Amendment D).
//!
//! Four panels — Peers, Sync, Mempool, Blocks — render a snapshot pulled from
//! a `DashboardBackend`. The monolith injects an in-process adapter over
//! `BlockchainFacade` / `PeerManager`; tests and the gallery use a
//! deterministic mock. No wall clock or RNG is used in this module.

mod blocks;
mod mempool;
mod peers;
mod sync;

pub use blocks::BlocksPanel;
pub use mempool::{MempoolPanel, MAX_MEMPOOL_ROWS};
pub use peers::PeersPanel;
pub use sync::SyncPanel;

use crate::accessibility::WidgetProbe;
use crate::backend::{DashboardBackend, DashboardSnapshot, MockDashboardBackend};
use crate::components::Card;
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

pub struct DashboardScreen {
    backend: Arc<dyn DashboardBackend>,
    snapshot: DashboardSnapshot,
    /// Compatibility setters used by `StranglerLauncher` and its tests.
    pub block_count: usize,
    pub mempool_count: usize,
    pub peers_panel: PeersPanel,
    pub sync_panel: SyncPanel,
    pub mempool_panel: MempoolPanel,
    pub blocks_panel: BlocksPanel,
}

impl DashboardScreen {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockDashboardBackend::new()))
    }

    pub fn with_backend(backend: Arc<dyn DashboardBackend>) -> Self {
        let snapshot = backend.snapshot();
        let mut screen = Self {
            backend,
            snapshot,
            block_count: 0,
            mempool_count: 0,
            peers_panel: PeersPanel::new(),
            sync_panel: SyncPanel::new(),
            mempool_panel: MempoolPanel::new(),
            blocks_panel: BlocksPanel::new(),
        };
        screen.apply_snapshot();
        screen
    }

    /// Swap the mock for the real in-process adapter without rebuilding the
    /// screen (or the launcher's navigation state).
    pub fn set_backend(&mut self, backend: Arc<dyn DashboardBackend>) {
        self.backend = backend;
        self.refresh();
    }

    pub fn backend(&self) -> Arc<dyn DashboardBackend> {
        Arc::clone(&self.backend)
    }

    pub fn snapshot(&self) -> &DashboardSnapshot {
        &self.snapshot
    }

    pub fn refresh(&mut self) {
        self.snapshot = self.backend.snapshot();
        self.apply_snapshot();
    }

    fn apply_snapshot(&mut self) {
        if self.block_count > 0 {
            self.snapshot.sync.current_height = self.block_count as u64;
            self.snapshot.sync.tip_height =
                self.snapshot.sync.tip_height.max(self.block_count as u64);
        }
        if self.mempool_count > 0 {
            self.snapshot.mempool_count = self.mempool_count;
        }
        self.peers_panel.set_rows(self.snapshot.peers.clone());
        self.sync_panel.set_status(self.snapshot.sync.clone());
        self.mempool_panel.set_data(
            self.snapshot.mempool_count,
            self.snapshot.total_fee,
            self.snapshot.mempool.clone(),
        );
        self.blocks_panel.set_rows(self.snapshot.blocks.clone());
    }

    pub fn set_block_count(&mut self, count: usize) {
        self.block_count = count;
        self.apply_snapshot();
    }

    pub fn set_mempool_count(&mut self, count: usize) {
        self.mempool_count = count;
        self.apply_snapshot();
    }

    pub fn a11y_probes(&self, theme: &ThemeManager) -> Vec<WidgetProbe> {
        let localization = Localization::new();
        let mut probes = self.peers_panel.a11y_probes(theme, &localization);
        probes.extend(self.sync_panel.a11y_probes(theme, &localization));
        probes.extend(self.mempool_panel.a11y_probes(theme, &localization));
        probes.extend(self.blocks_panel.a11y_probes(theme, &localization));
        probes
    }

    /// Render into an [`egui::Context`] (strangler launcher entry point).
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            Screen::show(self, ui, theme_manager, localization);
        });
    }
}

impl Screen for DashboardScreen {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        self.refresh();
        let tokens = theme_manager.tokens();
        ui.label(
            egui::RichText::new(localization.dashboard_title())
                .size(tokens.typography.xl.size)
                .strong(),
        );
        ui.add_space(tokens.spacing.base);

        Card::new()
            .title(localization.dashboard_sync())
            .show_with(ui, theme_manager, |ui| {
                self.sync_panel.show(ui, theme_manager, localization);
            });
        ui.add_space(tokens.spacing.base);

        Card::new()
            .title(localization.dashboard_peers())
            .show_with(ui, theme_manager, |ui| {
                self.peers_panel.show(ui, theme_manager, localization);
            });
        ui.add_space(tokens.spacing.base);

        Card::new()
            .title(localization.dashboard_mempool())
            .show_with(ui, theme_manager, |ui| {
                self.mempool_panel.show(ui, theme_manager, localization);
            });
        ui.add_space(tokens.spacing.base);

        Card::new()
            .title(localization.dashboard_blocks())
            .show_with(ui, theme_manager, |ui| {
                self.blocks_panel.show(ui, theme_manager, localization);
            });
    }

    fn title(&self, localization: &Localization) -> String {
        localization.dashboard_title()
    }

    fn on_back(&mut self) {}

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl Default for DashboardScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_panels_come_from_backend_and_are_deterministic() {
        let screen = DashboardScreen::new();
        let snapshot = screen.snapshot();
        assert_eq!(snapshot.peers.len(), 3);
        assert_eq!(snapshot.blocks.len(), 10);
        assert_eq!(snapshot.mempool_count, 2);
        assert_eq!(screen.peers_panel.rows().len(), 3);
        assert_eq!(screen.mempool_panel.rows().len(), 2);
        assert_eq!(screen.blocks_panel.rows().len(), 10);
        assert_eq!(screen.sync_panel.status().current_height, 42);
    }

    #[test]
    fn launcher_compat_setters_apply_to_panels() {
        let mut screen = DashboardScreen::new();
        screen.set_block_count(100);
        screen.set_mempool_count(7);
        assert_eq!(screen.sync_panel.status().current_height, 100);
        assert_eq!(screen.mempool_panel.rows().len(), 2);
        screen.refresh();
        assert_eq!(screen.sync_panel.status().current_height, 100);
    }
}
