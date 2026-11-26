//! Peers panel (UI-P04) — Stage 2 peer scoring bands with a text label
//! alongside the color (WCAG 1.4.1). Rows are sorted by `node_id` so the view
//! is deterministic for the same input set (V-21 extension).

use crate::accessibility::WidgetProbe;
use crate::backend::PeerRow;
use crate::components::Table;
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub struct PeersPanel {
    rows: Vec<PeerRow>,
}

impl PeersPanel {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn set_rows(&mut self, mut rows: Vec<PeerRow>) {
        rows.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        self.rows = rows;
    }

    pub fn rows(&self) -> &[PeerRow] {
        &self.rows
    }

    /// Green > 0.5, yellow 0.2..=0.5, red < 0.2 (ADR-0022).
    pub fn score_band(score: f32) -> &'static str {
        if score > 0.5 {
            "healthy"
        } else if score >= 0.2 {
            "warning"
        } else {
            "banned"
        }
    }

    pub fn score_label(score: f32, localization: &Localization) -> String {
        match Self::score_band(score) {
            "healthy" => localization.dashboard_score_healthy(),
            "warning" => localization.dashboard_score_warning(),
            _ => localization.dashboard_score_banned(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let colors = theme.colors();
        let healthy = colors.success.as_egui();
        let warning = colors.warning.as_egui();
        let banned = colors.error.as_egui();

        if self.rows.is_empty() {
            ui.label(loc.dashboard_no_peers());
            return;
        }

        let rows = self.rows.clone();
        Table::new()
            .column("peer_id", &loc.dashboard_peer_id())
            .width(110.0)
            .column("addr", &loc.dashboard_peer_addr())
            .width(140.0)
            .column("latency", &loc.dashboard_peer_latency())
            .width(80.0)
            .column("score", &loc.dashboard_peer_score())
            .width(100.0)
            .column("direction", &loc.dashboard_peer_direction())
            .width(90.0)
            .column("capabilities", &loc.dashboard_peer_capabilities())
            .cell_renderer(
                move |row: &PeerRow, column: &str, ui: &mut egui::Ui| match column {
                    "peer_id" => {
                        ui.label(&row.node_id);
                    }
                    "addr" => {
                        ui.label(&row.addr);
                    }
                    "latency" => {
                        ui.label(format!("{} ms", row.latency_ms));
                    }
                    "score" => {
                        let label = match Self::score_band(row.score) {
                            "healthy" => "healthy",
                            "warning" => "warning",
                            _ => "banned",
                        };
                        let color = match label {
                            "healthy" => healthy,
                            "warning" => warning,
                            _ => banned,
                        };
                        ui.label(
                            egui::RichText::new(format!("{:.2} ({label})", row.score)).color(color),
                        );
                    }
                    "direction" => {
                        ui.label(&row.direction);
                    }
                    "capabilities" => {
                        ui.label(row.capabilities.join(", "));
                    }
                    _ => {}
                },
            )
            .data(rows)
            .show(ui, theme);
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        let mut probes = Vec::new();
        for (id, label) in [
            ("dashboard-peer-id", loc.dashboard_peer_id()),
            ("dashboard-peer-addr", loc.dashboard_peer_addr()),
            ("dashboard-peer-latency", loc.dashboard_peer_latency()),
            ("dashboard-peer-score", loc.dashboard_peer_score()),
            ("dashboard-peer-direction", loc.dashboard_peer_direction()),
            (
                "dashboard-peer-capabilities",
                loc.dashboard_peer_capabilities(),
            ),
        ] {
            probes.push(
                WidgetProbe::interactive(
                    WidgetMeta::new(id, "columnheader", label),
                    colors.text,
                    colors.surface,
                    MIN_TOUCH_TARGET,
                    MIN_TOUCH_TARGET,
                )
                .with_focus(true, FOCUS_OUTLINE_WIDTH),
            );
        }
        // The color-coded score must also carry a textual status (WCAG 1.4.1).
        for band in ["healthy", "warning", "banned"] {
            let text = match band {
                "healthy" => loc.dashboard_score_healthy(),
                "warning" => loc.dashboard_score_warning(),
                _ => loc.dashboard_score_banned(),
            };
            probes.push(
                WidgetProbe::non_interactive(
                    WidgetMeta::new(format!("dashboard-score-{band}"), "status", text),
                    colors.text,
                    colors.surface,
                )
                .with_textual_status(true),
            );
        }
        probes
    }
}

impl Default for PeersPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_bands_match_stage2_thresholds() {
        assert_eq!(PeersPanel::score_band(0.51), "healthy");
        assert_eq!(PeersPanel::score_band(0.5), "warning");
        assert_eq!(PeersPanel::score_band(0.2), "warning");
        assert_eq!(PeersPanel::score_band(0.19), "banned");
    }
}
