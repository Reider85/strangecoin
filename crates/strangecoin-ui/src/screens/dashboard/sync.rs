//! Sync panel (UI-P04) — current/tip height, sync percentage, queue depths
//! and the sync engine state. All values are injected by the backend; this
//! module contains no wall clock and no RNG.

use crate::accessibility::WidgetProbe;
use crate::backend::SyncStatus;
use crate::components::{Card, Stat};
use crate::focus::WidgetMeta;
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub struct SyncPanel {
    status: SyncStatus,
}

impl SyncPanel {
    pub fn new() -> Self {
        Self {
            status: SyncStatus {
                current_height: 0,
                tip_height: 0,
                sync_percent: 100,
                headers_queue: 0,
                blocks_queue: 0,
                state: "idle".to_string(),
            },
        }
    }

    pub fn set_status(&mut self, status: SyncStatus) {
        self.status = status;
    }

    pub fn status(&self) -> &SyncStatus {
        &self.status
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let tokens = theme.tokens();
        let status = &self.status;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = tokens.spacing.lg;

            Card::new()
                .title(loc.dashboard_current_height())
                .child(Stat::new(
                    "sync-current-height",
                    "",
                    &status.current_height.to_string(),
                ))
                .show(ui, theme);

            Card::new()
                .title(loc.dashboard_tip_height())
                .child(Stat::new(
                    "sync-tip-height",
                    "",
                    &status.tip_height.to_string(),
                ))
                .show(ui, theme);

            Card::new()
                .title(loc.dashboard_sync())
                .child(Stat::new(
                    "sync-percent",
                    "",
                    &loc.dashboard_sync_percent(status.sync_percent),
                ))
                .show(ui, theme);

            Card::new()
                .title(loc.dashboard_sync_state())
                .child(Stat::new("sync-state", "", &status.state))
                .show(ui, theme);
        });

        ui.add_space(tokens.spacing.base);
        ui.label(format!(
            "{}: {} — {}: {}",
            loc.dashboard_headers_queue(),
            status.headers_queue,
            loc.dashboard_blocks_queue(),
            status.blocks_queue
        ));
        ui.add_space(tokens.spacing.sm);
        let progress = (status.sync_percent as f32 / 100.0).clamp(0.0, 1.0);
        ui.add(
            egui::ProgressBar::new(progress)
                .desired_width(f32::INFINITY)
                .text(loc.dashboard_sync_percent(status.sync_percent)),
        );
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        [
            Stat::new("sync-current-height", &loc.dashboard_current_height(), "0"),
            Stat::new("sync-tip-height", &loc.dashboard_tip_height(), "0"),
            Stat::new("sync-percent", &loc.dashboard_sync(), "100%"),
            Stat::new("sync-state", &loc.dashboard_sync_state(), "idle"),
            Stat::new("sync-headers-queue", &loc.dashboard_headers_queue(), "0"),
            Stat::new("sync-blocks-queue", &loc.dashboard_blocks_queue(), "0"),
        ]
        .iter()
        .map(|stat| stat.a11y_probe(theme))
        .chain(std::iter::once(WidgetProbe::non_interactive(
            WidgetMeta::new("sync-percent", "status", loc.dashboard_sync()),
            colors.text,
            colors.surface,
        )))
        .collect()
    }
}

impl Default for SyncPanel {
    fn default() -> Self {
        Self::new()
    }
}
