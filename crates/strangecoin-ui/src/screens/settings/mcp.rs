//! MCP settings panel (UI-P05, ADR-0029) — enable toggle plus transport /
//! listen address and connected clients. When the backend reports
//! `available == false` (no live client registry) the note is shown and no
//! client data is invented. Every toggle goes through `SettingsBackend`.

use super::{action_button, interactive_probe, status_probe, StatusText};
use crate::accessibility::WidgetProbe;
use crate::backend::{McpSettings, MockSettingsBackend, SettingsBackend};
use crate::components::Card;
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

const MAX_MCP_CLIENTS: usize = 32;

pub struct McpPanel {
    backend: Arc<dyn SettingsBackend>,
    data: McpSettings,
    status: Option<StatusText>,
}

impl McpPanel {
    pub fn new() -> Self {
        Self {
            backend: Arc::new(MockSettingsBackend::new()),
            data: MockSettingsBackend::new().snapshot().mcp,
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

    pub fn set_mcp(&mut self, mut data: McpSettings) {
        data.clients.sort();
        data.clients.dedup();
        data.clients.truncate(MAX_MCP_CLIENTS);
        self.data = data;
    }

    pub fn mcp(&self) -> &McpSettings {
        &self.data
    }

    pub fn status(&self) -> Option<&StatusText> {
        self.status.as_ref()
    }

    pub fn toggle(&mut self, loc: &Localization) {
        let enabled = !self.data.enabled;
        self.status = Some(match self.backend.set_mcp_enabled(enabled) {
            Ok(()) => {
                self.data.enabled = enabled;
                StatusText::saved(loc)
            }
            Err(err) => StatusText::error(loc, &err),
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let colors = theme.colors();
        let error_color = colors.error.as_egui();
        let saved_color = colors.success.as_egui();
        let enabled_text = if self.data.enabled {
            loc.success()
        } else {
            loc.error()
        };

        Card::new()
            .title(loc.settings_mcp())
            .show_with(ui, theme, |ui| {
                if action_button(&loc.settings_mcp_enable(), "settings-mcp-toggle", ui, theme) {
                    self.toggle(loc);
                }
                ui.label(&enabled_text);

                if !self.data.available {
                    if !self.data.note.is_empty() {
                        ui.label(&self.data.note);
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(loc.settings_mcp_transport()).strong());
                        ui.label(&self.data.transport);
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(loc.settings_mcp_listen()).strong());
                        ui.label(&self.data.http_listen);
                    });
                    ui.label(loc.settings_mcp_clients());
                    for client in &self.data.clients {
                        ui.monospace(client);
                    }
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
                "settings-mcp-toggle",
                "button",
                loc.settings_mcp_enable(),
                theme,
            ),
            interactive_probe(
                "settings-mcp-transport",
                "columnheader",
                loc.settings_mcp_transport(),
                theme,
            ),
            interactive_probe(
                "settings-mcp-listen",
                "columnheader",
                loc.settings_mcp_listen(),
                theme,
            ),
            status_probe("settings-mcp-clients", loc.settings_mcp_clients(), theme),
            status_probe("settings-mcp-note", loc.settings_mcp_enable(), theme),
        ]
    }
}

impl Default for McpPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_are_sorted_deduplicated_and_capped() {
        let mut panel = McpPanel::new();
        let mut data = panel.mcp().clone();
        data.clients = vec!["z".to_string(), "a".to_string(), "a".to_string()];
        panel.set_mcp(data);
        assert_eq!(panel.mcp().clients, ["a", "z"]);
    }

    #[test]
    fn unavailable_registry_does_not_invent_clients() {
        let panel = McpPanel::new();
        assert!(!panel.mcp().available);
        assert!(panel.mcp().clients.is_empty());
        assert!(!panel.mcp().note.is_empty());
    }

    #[test]
    fn toggle_failure_is_surfaced_without_state_change() {
        let loc = Localization::new();
        let mut panel = McpPanel::new();
        let before = panel.mcp().enabled;
        panel.toggle(&loc);
        assert_eq!(panel.mcp().enabled, before);
        let status = panel.status().expect("toggle status");
        assert!(status.is_error());
        assert!(status.text().contains("MCP toggle is not available"));
    }
}
