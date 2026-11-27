//! ACP settings panel (UI-P05, ADR-0030 / ADR-0031 Amendment E) — enable
//! toggle and active sessions. `available == false` means there is no live
//! session registry: the note is shown and no session is invented.
//!
//! Deviation (documented): `SettingsBackend` has no dedicated agent-ban
//! method, so the Ban-agent action reuses `set_peer_ban` for the selected
//! agent id. It still goes through the backend and fails closed — no ban is
//! ever reported as applied unless the backend returns `Ok`.

use super::{action_button, interactive_probe, status_probe, StatusText};
use crate::accessibility::WidgetProbe;
use crate::backend::{AcpSessionRow, AcpSettings, MockSettingsBackend, SettingsBackend};
use crate::components::{Card, Table};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

const MAX_ACP_SESSIONS: usize = 32;

pub struct AcpPanel {
    backend: Arc<dyn SettingsBackend>,
    data: AcpSettings,
    selected_agent: Option<String>,
    status: Option<StatusText>,
}

impl AcpPanel {
    pub fn new() -> Self {
        Self {
            backend: Arc::new(MockSettingsBackend::new()),
            data: MockSettingsBackend::new().snapshot().acp,
            selected_agent: None,
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

    pub fn set_acp(&mut self, mut data: AcpSettings) {
        data.sessions.sort_by(|left, right| {
            left.agent_a
                .cmp(&right.agent_a)
                .then_with(|| left.agent_b.cmp(&right.agent_b))
                .then_with(|| left.escrow_txid.cmp(&right.escrow_txid))
        });
        data.sessions.truncate(MAX_ACP_SESSIONS);
        self.data = data;
        self.selected_agent = None;
    }

    pub fn acp(&self) -> &AcpSettings {
        &self.data
    }

    pub fn status(&self) -> Option<&StatusText> {
        self.status.as_ref()
    }

    pub fn select_agent(&mut self, agent: &str) {
        self.selected_agent = Some(agent.to_string());
    }

    pub fn toggle(&mut self, loc: &Localization) {
        let enabled = !self.data.enabled;
        self.status = Some(match self.backend.set_acp_enabled(enabled) {
            Ok(()) => {
                self.data.enabled = enabled;
                StatusText::saved(loc)
            }
            Err(err) => StatusText::error(loc, &err),
        });
    }

    pub fn ban_agent(&mut self, loc: &Localization) {
        let Some(agent) = self.selected_agent.clone() else {
            self.status = Some(StatusText::error_only(loc));
            return;
        };
        if !self.data.available {
            self.status = Some(StatusText::error(loc, &self.data.note));
            return;
        }
        self.status = Some(match self.backend.set_peer_ban(&agent, true) {
            Ok(()) => StatusText::saved(loc),
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
            .title(loc.settings_acp())
            .show_with(ui, theme, |ui| {
                if action_button(&loc.settings_acp_enable(), "settings-acp-toggle", ui, theme) {
                    self.toggle(loc);
                }
                ui.label(&enabled_text);

                if !self.data.available {
                    if !self.data.note.is_empty() {
                        ui.label(&self.data.note);
                    }
                } else {
                    let sessions = self.data.sessions.clone();
                    let mut table = Table::new()
                        .column("session", &loc.settings_acp_sessions())
                        .cell_renderer(
                            move |row: &AcpSessionRow, column: &str, ui: &mut egui::Ui| {
                                if column == "session" {
                                    ui.label(format!(
                                        "{} → {} · {} · {}",
                                        row.agent_a, row.agent_b, row.escrow_txid, row.status
                                    ));
                                }
                            },
                        )
                        .data(sessions.clone());
                    table.show(ui, theme);
                    if let Some(index) = table.selected_row() {
                        if let Some(session) = sessions.get(index) {
                            self.selected_agent = Some(session.agent_b.clone());
                        }
                    }
                }

                if action_button(
                    &loc.settings_acp_ban_agent(),
                    "settings-acp-ban-agent",
                    ui,
                    theme,
                ) {
                    self.ban_agent(loc);
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
                "settings-acp-toggle",
                "button",
                loc.settings_acp_enable(),
                theme,
            ),
            interactive_probe(
                "settings-acp-sessions",
                "columnheader",
                loc.settings_acp_sessions(),
                theme,
            ),
            interactive_probe(
                "settings-acp-ban-agent",
                "button",
                loc.settings_acp_ban_agent(),
                theme,
            ),
            status_probe(
                "settings-acp-session-status",
                loc.settings_acp_sessions(),
                theme,
            ),
            status_probe("settings-acp-note", loc.settings_acp_enable(), theme),
        ]
    }
}

impl Default for AcpPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(agent_a: &str) -> AcpSessionRow {
        AcpSessionRow {
            agent_a: agent_a.to_string(),
            agent_b: format!("{agent_a}-peer"),
            escrow_txid: format!("0x{agent_a}"),
            status: "active".to_string(),
        }
    }

    #[test]
    fn sessions_are_sorted_and_capped() {
        let mut panel = AcpPanel::new();
        let mut data = panel.acp().clone();
        data.sessions = (0..(MAX_ACP_SESSIONS + 2))
            .rev()
            .map(|index| session(&format!("agent-{index:03}")))
            .collect();
        panel.set_acp(data);
        assert_eq!(panel.acp().sessions.len(), MAX_ACP_SESSIONS);
        assert_eq!(panel.acp().sessions[0].agent_a, "agent-000");
    }

    #[test]
    fn unavailable_registry_does_not_invent_sessions() {
        let panel = AcpPanel::new();
        assert!(!panel.acp().available);
        assert!(panel.acp().sessions.is_empty());
        assert!(!panel.acp().note.is_empty());
    }

    #[test]
    fn ban_without_selection_fails_closed() {
        let loc = Localization::new();
        let mut panel = AcpPanel::new();
        panel.ban_agent(&loc);
        assert!(panel.status().is_some_and(StatusText::is_error));
    }

    #[test]
    fn toggle_failure_is_surfaced_without_state_change() {
        let loc = Localization::new();
        let mut panel = AcpPanel::new();
        let before = panel.acp().enabled;
        panel.toggle(&loc);
        assert_eq!(panel.acp().enabled, before);
        let status = panel.status().expect("toggle status");
        assert!(status.is_error());
        assert!(status.text().contains("ACP toggle is not available"));
    }
}
