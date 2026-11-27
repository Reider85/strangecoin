//! Keystore settings panel (UI-P05, ADR-0028 / ADR-0031 Amendment E) —
//! operator and agent keys are shown as two separate tabs to preserve key
//! isolation. Import / Export / Change-password / Derive are mutation
//! requests: the `SettingsBackend` seam exposes no keystore write method yet,
//! so every request fails closed (explicit `settings-keystore-unavailable`
//! when `available == false`, localized error status otherwise). No success
//! is ever fabricated.

use super::{action_button, interactive_probe, status_probe, tab_button, StatusText};
use crate::accessibility::WidgetProbe;
use crate::backend::{KeystoreSettings, MockSettingsBackend, SettingsBackend};
use crate::components::Card;
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

const MAX_KEYSTORE_ENTRIES: usize = 32;

/// ADR-0028 key isolation: operator and agent keyrings are never mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeystoreTab {
    Operator,
    Agent,
}

pub struct KeystorePanel {
    data: KeystoreSettings,
    active: KeystoreTab,
    status: Option<StatusText>,
}

impl KeystorePanel {
    pub fn new() -> Self {
        Self {
            data: MockSettingsBackend::new().snapshot().keystore,
            active: KeystoreTab::Operator,
            status: None,
        }
    }

    pub fn set_keystore(&mut self, mut data: KeystoreSettings) {
        for entries in [&mut data.operator, &mut data.agents] {
            entries.sort_by(|left, right| {
                left.label
                    .cmp(&right.label)
                    .then_with(|| left.address.cmp(&right.address))
            });
            entries.truncate(MAX_KEYSTORE_ENTRIES);
        }
        self.data = data;
    }

    pub fn keystore(&self) -> &KeystoreSettings {
        &self.data
    }

    pub fn active_tab(&self) -> KeystoreTab {
        self.active
    }

    pub fn set_active_tab(&mut self, tab: KeystoreTab) {
        self.active = tab;
    }

    pub fn status(&self) -> Option<&StatusText> {
        self.status.as_ref()
    }

    fn request(&mut self, loc: &Localization) {
        self.status = Some(if self.data.available {
            // No keystore mutation seam exists on `SettingsBackend`: fail
            // closed instead of pretending the key was changed.
            StatusText::error_only(loc)
        } else {
            StatusText::error(loc, &loc.settings_keystore_unavailable())
        });
    }

    pub fn import_key(&mut self, loc: &Localization) {
        self.request(loc);
    }

    pub fn export_key(&mut self, loc: &Localization) {
        self.request(loc);
    }

    pub fn change_password(&mut self, loc: &Localization) {
        self.request(loc);
    }

    pub fn derive_key(&mut self, loc: &Localization) {
        self.request(loc);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let colors = theme.colors();
        let error_color = colors.error.as_egui();
        let saved_color = colors.success.as_egui();

        Card::new()
            .title(loc.settings_keystore())
            .show_with(ui, theme, |ui| {
                if tab_button(
                    &loc.settings_keystore_operator(),
                    "settings-keystore-operator",
                    self.active == KeystoreTab::Operator,
                    ui,
                    theme,
                ) {
                    self.active = KeystoreTab::Operator;
                }
                if tab_button(
                    &loc.settings_keystore_agent(),
                    "settings-keystore-agent",
                    self.active == KeystoreTab::Agent,
                    ui,
                    theme,
                ) {
                    self.active = KeystoreTab::Agent;
                }

                if !self.data.available {
                    if !self.data.note.is_empty() {
                        ui.label(&self.data.note);
                    }
                    ui.label(loc.settings_keystore_unavailable());
                } else {
                    let entries = match self.active {
                        KeystoreTab::Operator => &self.data.operator,
                        KeystoreTab::Agent => &self.data.agents,
                    };
                    for entry in entries {
                        ui.horizontal(|ui| {
                            ui.label(&entry.label);
                            ui.monospace(&entry.address);
                            ui.label(&entry.kind);
                        });
                    }
                }

                let import = action_button(
                    &loc.settings_keystore_import(),
                    "settings-keystore-import",
                    ui,
                    theme,
                );
                let export = action_button(
                    &loc.settings_keystore_export(),
                    "settings-keystore-export",
                    ui,
                    theme,
                );
                let password = action_button(
                    &loc.settings_keystore_change_password(),
                    "settings-keystore-change-password",
                    ui,
                    theme,
                );
                let derive = action_button(
                    &loc.settings_keystore_derive(),
                    "settings-keystore-derive",
                    ui,
                    theme,
                );

                if import {
                    self.import_key(loc);
                }
                if export {
                    self.export_key(loc);
                }
                if password {
                    self.change_password(loc);
                }
                if derive {
                    self.derive_key(loc);
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
                "settings-keystore-operator",
                "tab",
                loc.settings_keystore_operator(),
                theme,
            ),
            interactive_probe(
                "settings-keystore-agent",
                "tab",
                loc.settings_keystore_agent(),
                theme,
            ),
            interactive_probe(
                "settings-keystore-import",
                "button",
                loc.settings_keystore_import(),
                theme,
            ),
            interactive_probe(
                "settings-keystore-export",
                "button",
                loc.settings_keystore_export(),
                theme,
            ),
            interactive_probe(
                "settings-keystore-change-password",
                "button",
                loc.settings_keystore_change_password(),
                theme,
            ),
            interactive_probe(
                "settings-keystore-derive",
                "button",
                loc.settings_keystore_derive(),
                theme,
            ),
            status_probe(
                "settings-keystore-unavailable",
                loc.settings_keystore_unavailable(),
                theme,
            ),
        ]
    }
}

impl Default for KeystorePanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str) -> crate::backend::KeystoreEntry {
        crate::backend::KeystoreEntry {
            label: label.to_string(),
            address: format!("sc1{label}"),
            kind: "session".to_string(),
        }
    }

    #[test]
    fn operator_and_agent_entries_are_sorted_and_capped() {
        let mut panel = KeystorePanel::new();
        let mut data = panel.keystore().clone();
        data.operator = (0..(MAX_KEYSTORE_ENTRIES + 3))
            .rev()
            .map(|index| entry(&format!("key-{index:03}")))
            .collect();
        data.agents = vec![entry("z-agent"), entry("a-agent")];
        panel.set_keystore(data);

        let operator = &panel.keystore().operator;
        assert_eq!(operator.len(), MAX_KEYSTORE_ENTRIES);
        assert_eq!(operator[0].label, "key-000");
        assert_eq!(panel.keystore().agents[0].label, "a-agent");
    }

    #[test]
    fn unavailable_keystore_reports_explicit_note() {
        let loc = Localization::new();
        let mut panel = KeystorePanel::new();
        let mut data = panel.keystore().clone();
        data.available = false;
        panel.set_keystore(data);

        panel.import_key(&loc);
        let status = panel.status().expect("import status");
        assert!(status.is_error());
        assert!(status
            .text()
            .contains(loc.settings_keystore_unavailable().as_str()));
    }

    #[test]
    fn tabs_switch_without_mixing_keyrings() {
        let mut panel = KeystorePanel::new();
        assert_eq!(panel.active_tab(), KeystoreTab::Operator);
        panel.set_active_tab(KeystoreTab::Agent);
        assert_eq!(panel.active_tab(), KeystoreTab::Agent);
    }
}
