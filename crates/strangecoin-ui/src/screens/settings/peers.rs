//! Peers settings panel (UI-P05) — sorted peer table plus Ban / Unban /
//! Add-seed actions. Rows are sorted by `node_id` (then address) and capped so
//! the view is deterministic for the same input set (V-21 extension). Ban and
//! seed changes always go through `SettingsBackend`; a failure is shown as
//! text and is never reported as a success.

use super::{action_button, interactive_probe, status_probe, StatusText};
use crate::accessibility::WidgetProbe;
use crate::backend::{MockSettingsBackend, SettingsBackend, SettingsPeerRow};
use crate::components::{Card, Input, Table};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

pub const MAX_SETTINGS_PEERS: usize = 50;

pub struct PeersPanel {
    backend: Arc<dyn SettingsBackend>,
    rows: Vec<SettingsPeerRow>,
    selected: Option<String>,
    seed_input: String,
    status: Option<StatusText>,
}

impl PeersPanel {
    pub fn new() -> Self {
        Self {
            backend: Arc::new(MockSettingsBackend::new()),
            rows: Vec::new(),
            selected: None,
            seed_input: String::new(),
            status: None,
        }
    }

    pub fn with_backend(backend: Arc<dyn SettingsBackend>) -> Self {
        let mut panel = Self::new();
        panel.set_backend(backend);
        panel
    }

    pub fn set_backend(&mut self, backend: Arc<dyn SettingsBackend>) {
        self.backend = backend;
        self.status = None;
    }

    pub fn set_rows(&mut self, mut rows: Vec<SettingsPeerRow>) {
        rows.sort_by(|left, right| {
            left.node_id
                .cmp(&right.node_id)
                .then_with(|| left.addr.cmp(&right.addr))
        });
        rows.truncate(MAX_SETTINGS_PEERS);
        self.rows = rows;
        self.selected = None;
    }

    pub fn rows(&self) -> &[SettingsPeerRow] {
        &self.rows
    }

    pub fn status(&self) -> Option<&StatusText> {
        self.status.as_ref()
    }

    pub fn select_node(&mut self, node_id: &str) {
        self.selected = Some(node_id.to_string());
    }

    pub fn ban_selected(&mut self, loc: &Localization) {
        self.set_ban(loc, true);
    }

    pub fn unban_selected(&mut self, loc: &Localization) {
        self.set_ban(loc, false);
    }

    fn set_ban(&mut self, loc: &Localization, banned: bool) {
        let Some(node_id) = self.selected.clone() else {
            self.status = Some(StatusText::error_only(loc));
            return;
        };
        self.status = Some(match self.backend.set_peer_ban(&node_id, banned) {
            Ok(()) => StatusText::saved(loc),
            Err(err) => StatusText::error(loc, &err),
        });
    }

    pub fn add_seed(&mut self, loc: &Localization) {
        let addr = self.seed_input.clone();
        if addr.trim().is_empty() {
            self.status = Some(StatusText::error_only(loc));
            return;
        }
        self.status = Some(match self.backend.add_seed(&addr) {
            Ok(()) => StatusText::saved(loc),
            Err(err) => StatusText::error(loc, &err),
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let colors = theme.colors();
        let error_color = colors.error.as_egui();
        let saved_color = colors.success.as_egui();
        let banned_text = loc.settings_peers_banned();

        Card::new()
            .title(loc.settings_peers())
            .show_with(ui, theme, |ui| {
                if self.rows.is_empty() {
                    ui.label(loc.settings_peers());
                } else {
                    let rows = self.rows.clone();
                    let mut table = Table::new()
                        .column("node_id", &loc.dashboard_peer_id())
                        .width(110.0)
                        .column("addr", &loc.dashboard_peer_addr())
                        .width(140.0)
                        .column("score", &loc.dashboard_peer_score())
                        .width(110.0)
                        .cell_renderer(
                            move |row: &SettingsPeerRow, column: &str, ui: &mut egui::Ui| {
                                match column {
                                    "node_id" => {
                                        ui.monospace(&row.node_id);
                                    }
                                    "addr" => {
                                        ui.label(&row.addr);
                                    }
                                    "score" => {
                                        if row.banned {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{:.2} ({})",
                                                    row.score, banned_text
                                                ))
                                                .color(error_color),
                                            );
                                        } else {
                                            ui.label(format!("{:.2}", row.score));
                                        }
                                    }
                                    _ => {}
                                }
                            },
                        )
                        .data(rows.clone());
                    table.show(ui, theme);
                    if let Some(index) = table.selected_row() {
                        if let Some(row) = rows.get(index) {
                            self.selected = Some(row.node_id.clone());
                        }
                    }
                }

                let mut seed_input = Input::new(loc.settings_peers_add_seed())
                    .id("settings-peers-seed-input")
                    .value(&self.seed_input);
                seed_input.show(ui, theme);
                self.seed_input = seed_input.get_value().to_string();

                let ban = action_button(&loc.settings_peers_ban(), "settings-peers-ban", ui, theme);
                let unban = action_button(
                    &loc.settings_peers_unban(),
                    "settings-peers-unban",
                    ui,
                    theme,
                );
                let add_seed = action_button(
                    &loc.settings_peers_add_seed(),
                    "settings-peers-add-seed",
                    ui,
                    theme,
                );

                if ban {
                    self.ban_selected(loc);
                }
                if unban {
                    self.unban_selected(loc);
                }
                if add_seed {
                    self.add_seed(loc);
                }

                if let Some(status) = &self.status {
                    ui.label(
                        egui::RichText::new(status.text()).color(if status.is_error() {
                            error_color
                        } else {
                            saved_color
                        }),
                    );
                }
            });
    }

    pub fn a11y_probes(&self, theme: &ThemeManager) -> Vec<WidgetProbe> {
        let loc = Localization::new();
        vec![
            interactive_probe(
                "settings-peers-col-node",
                "columnheader",
                loc.dashboard_peer_id(),
                theme,
            ),
            interactive_probe(
                "settings-peers-col-addr",
                "columnheader",
                loc.dashboard_peer_addr(),
                theme,
            ),
            interactive_probe(
                "settings-peers-col-score",
                "columnheader",
                loc.dashboard_peer_score(),
                theme,
            ),
            interactive_probe(
                "settings-peers-seed-input",
                "textbox",
                loc.settings_peers_add_seed(),
                theme,
            ),
            interactive_probe(
                "settings-peers-ban",
                "button",
                loc.settings_peers_ban(),
                theme,
            ),
            interactive_probe(
                "settings-peers-unban",
                "button",
                loc.settings_peers_unban(),
                theme,
            ),
            interactive_probe(
                "settings-peers-add-seed",
                "button",
                loc.settings_peers_add_seed(),
                theme,
            ),
            status_probe("settings-peers-banned", loc.settings_peers_banned(), theme),
        ]
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
    use crate::backend::MockSettingsBackend;

    fn row(node_id: &str, banned: bool) -> SettingsPeerRow {
        SettingsPeerRow {
            node_id: node_id.to_string(),
            addr: format!("127.0.0.1:{node_id}"),
            score: 0.5,
            banned,
        }
    }

    #[test]
    fn rows_are_sorted_by_node_id_and_capped() {
        let mut panel = PeersPanel::new();
        let rows: Vec<_> = (0..(MAX_SETTINGS_PEERS + 5))
            .rev()
            .map(|index| row(&format!("peer-{index:03}"), false))
            .collect();
        panel.set_rows(rows);
        assert_eq!(panel.rows().len(), MAX_SETTINGS_PEERS);
        assert_eq!(panel.rows()[0].node_id, "peer-000");
        assert!(panel
            .rows()
            .windows(2)
            .all(|pair| pair[0].node_id < pair[1].node_id));
    }

    #[test]
    fn mock_backend_failure_is_surfaced_without_fake_success() {
        let loc = Localization::new();
        let mut panel = PeersPanel::with_backend(Arc::new(MockSettingsBackend::new()));
        panel.select_node("peer-a");
        panel.ban_selected(&loc);
        let status = panel.status().expect("ban status");
        assert!(status.is_error());
        assert!(status.text().contains("peer ban control is not available"));
    }

    #[test]
    fn empty_seed_input_does_not_call_backend() {
        let loc = Localization::new();
        let mut panel = PeersPanel::new();
        panel.add_seed(&loc);
        assert!(panel.status().is_some_and(StatusText::is_error));
    }
}
