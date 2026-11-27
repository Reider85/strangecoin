//! Network settings panel (UI-P05) — listen address, seeds, peer cap and
//! network id are read-only after start (`settings-read-only`). All values
//! come from a `SettingsBackend` snapshot; nothing here mutates state.

use super::{interactive_probe, status_probe};
use crate::accessibility::WidgetProbe;
use crate::backend::NetworkSettings;
use crate::components::Card;
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub struct NetworkPanel {
    data: NetworkSettings,
}

impl NetworkPanel {
    pub fn new() -> Self {
        Self {
            data: NetworkSettings {
                listen_addr: String::new(),
                seeds: Vec::new(),
                max_peers: 0,
                network_id: String::new(),
            },
        }
    }

    /// Seeds are sorted and deduplicated so two snapshots with the same set
    /// render byte-identically (V-21 determinism).
    pub fn set_network(&mut self, mut data: NetworkSettings) {
        data.seeds.sort();
        data.seeds.dedup();
        self.data = data;
    }

    pub fn network(&self) -> &NetworkSettings {
        &self.data
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let tokens = theme.tokens();
        Card::new()
            .title(loc.settings_network())
            .show_with(ui, theme, |ui| {
                for (label, value) in [
                    (loc.settings_network_listen(), self.data.listen_addr.clone()),
                    (loc.settings_network_seeds(), self.data.seeds.join(", ")),
                    (
                        loc.settings_network_max_peers(),
                        self.data.max_peers.to_string(),
                    ),
                    (loc.settings_network_id(), self.data.network_id.clone()),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(label).strong());
                        ui.label(value);
                    });
                }
                ui.add_space(tokens.spacing.sm);
                ui.label(loc.settings_read_only());
            });
    }

    pub fn a11y_probes(&self, theme: &ThemeManager) -> Vec<WidgetProbe> {
        let loc = Localization::new();
        vec![
            interactive_probe(
                "settings-network-listen",
                "columnheader",
                loc.settings_network_listen(),
                theme,
            ),
            interactive_probe(
                "settings-network-seeds",
                "columnheader",
                loc.settings_network_seeds(),
                theme,
            ),
            interactive_probe(
                "settings-network-max-peers",
                "columnheader",
                loc.settings_network_max_peers(),
                theme,
            ),
            interactive_probe(
                "settings-network-id",
                "columnheader",
                loc.settings_network_id(),
                theme,
            ),
            status_probe(
                "settings-network-read-only",
                loc.settings_read_only(),
                theme,
            ),
        ]
    }
}

impl Default for NetworkPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_are_sorted_and_deduplicated() {
        let mut panel = NetworkPanel::new();
        panel.set_network(NetworkSettings {
            listen_addr: "127.0.0.1:8333".to_string(),
            seeds: vec![
                "b:8333".to_string(),
                "a:8333".to_string(),
                "a:8333".to_string(),
            ],
            max_peers: 32,
            network_id: "main".to_string(),
        });
        assert_eq!(panel.network().seeds, ["a:8333", "b:8333"]);
    }

    #[test]
    fn a11y_probes_cover_all_read_only_fields() {
        let theme = ThemeManager::new();
        let probes = NetworkPanel::new().a11y_probes(&theme);
        assert_eq!(probes.len(), 5);
        assert!(probes.iter().all(|probe| !probe.meta.label.is_empty()));
        assert!(probes
            .iter()
            .all(|probe| probe.meta.name.starts_with("settings-network")));
    }
}
