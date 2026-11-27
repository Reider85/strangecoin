//! Config settings panel (UI-P05) — the raw `config.toml` is editable here;
//! Validate and "Save and Restart" always call `SettingsBackend`. Both success
//! and failure are reported as localized status text, never assumed.

use super::{action_button, interactive_probe, status_probe, StatusText};
use crate::accessibility::WidgetProbe;
use crate::backend::{ConfigSettings, MockSettingsBackend, SettingsBackend};
use crate::components::{Card, Input};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

pub struct ConfigPanel {
    backend: Arc<dyn SettingsBackend>,
    raw: String,
    validation: String,
    status: Option<StatusText>,
}

impl ConfigPanel {
    pub fn new() -> Self {
        Self {
            backend: Arc::new(MockSettingsBackend::new()),
            raw: String::new(),
            validation: String::new(),
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

    pub fn set_config(&mut self, data: ConfigSettings) {
        self.raw = data.raw;
        self.validation = data.validation;
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn status(&self) -> Option<&StatusText> {
        self.status.as_ref()
    }

    pub fn validate(&mut self, loc: &Localization) {
        let detail = match self.backend.validate_config(&self.raw) {
            Ok(()) => return self.set_saved(loc),
            Err(err) => err,
        };
        let detail = if detail.trim().is_empty() {
            loc.settings_config_invalid()
        } else {
            detail
        };
        self.status = Some(StatusText::error(loc, &detail));
    }

    pub fn save(&mut self, loc: &Localization) {
        let detail = match self.backend.save_config(&self.raw) {
            Ok(()) => return self.set_saved(loc),
            Err(err) => err,
        };
        let detail = if detail.trim().is_empty() {
            loc.settings_config_invalid()
        } else {
            detail
        };
        self.status = Some(StatusText::error(loc, &detail));
    }

    fn set_saved(&mut self, loc: &Localization) {
        self.status = Some(StatusText::saved(loc));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let colors = theme.colors();
        let error_color = colors.error.as_egui();
        let saved_color = colors.success.as_egui();

        Card::new()
            .title(loc.settings_config())
            .show_with(ui, theme, |ui| {
                let mut editor = Input::new(loc.settings_config_raw())
                    .id("settings-config-raw")
                    .value(&self.raw);
                editor.show_multiline(ui, theme);
                self.raw = editor.get_value().to_string();

                let validate = action_button(
                    &loc.settings_config_validate(),
                    "settings-config-validate",
                    ui,
                    theme,
                );
                let save = action_button(
                    &loc.settings_config_save_restart(),
                    "settings-config-save",
                    ui,
                    theme,
                );
                if validate {
                    self.validate(loc);
                }
                if save {
                    self.save(loc);
                }

                if !self.validation.is_empty() {
                    ui.label(&self.validation);
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
                "settings-config-raw",
                "textbox",
                loc.settings_config_raw(),
                theme,
            ),
            interactive_probe(
                "settings-config-validate",
                "button",
                loc.settings_config_validate(),
                theme,
            ),
            interactive_probe(
                "settings-config-save",
                "button",
                loc.settings_config_save_restart(),
                theme,
            ),
            status_probe(
                "settings-config-invalid",
                loc.settings_config_invalid(),
                theme,
            ),
        ]
    }
}

impl Default for ConfigPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_text_is_loaded_from_snapshot() {
        let mut panel = ConfigPanel::new();
        panel.set_config(ConfigSettings {
            raw: "[network]\nnetwork_id = \"main\"\n".to_string(),
            validation: String::new(),
        });
        assert!(panel.raw().contains("network_id"));
    }

    #[test]
    fn backend_validation_failure_is_surfaced_as_text() {
        let loc = Localization::new();
        let mut panel = ConfigPanel::new();
        panel.validate(&loc);
        let status = panel.status().expect("validate status");
        assert!(status.is_error());
        assert!(status.text().contains("config validation is not available"));
    }
}
