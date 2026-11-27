//! Operator settings screen (UI-P05, ADR-0031 Amendment E).
//!
//! The КГ enumerates six spec sub-screens (Network, Peers, Keystore, Config,
//! MCP, ACP) while calling them "7 sub-screen"; ADR-0031 Amendment E resolves
//! the count by keeping the pre-existing General and About panels, so this
//! screen exposes all eight [`SettingsTab`] variants. Each of the six spec
//! panels owns a module; General/About stay here because they only show
//! theme, locale and `env!("CARGO_PKG_VERSION")` — no fabricated build data.
//!
//! Every panel pulls data from [`SettingsBackend`] snapshots. Mutations go
//! through the backend and surface errors as text; read-only panels never
//! claim success. No RNG, wall clock or `println!` is used.

mod acp;
mod config;
mod keystore;
mod mcp;
mod network;
mod peers;

pub use acp::AcpPanel;
pub use config::ConfigPanel;
pub use keystore::{KeystorePanel, KeystoreTab};
pub use mcp::McpPanel;
pub use network::NetworkPanel;
pub use peers::{PeersPanel, MAX_SETTINGS_PEERS};

use crate::accessibility::WidgetProbe;
use crate::backend::{MockSettingsBackend, SettingsBackend, SettingsSnapshot};
use crate::components::{Card, Tabs, TextTabContent};
use crate::focus::{self, WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

/// Localized status line shared by all panels. Backend errors are prefixed
/// with `settings-status-error`; success uses `settings-status-saved`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusText {
    error: bool,
    text: String,
}

impl StatusText {
    pub fn saved(loc: &Localization) -> Self {
        Self {
            error: false,
            text: loc.settings_status_saved(),
        }
    }

    pub fn error_only(loc: &Localization) -> Self {
        Self {
            error: true,
            text: loc.settings_status_error(),
        }
    }

    pub fn error(loc: &Localization, detail: &str) -> Self {
        if detail.trim().is_empty() {
            return Self::error_only(loc);
        }
        Self {
            error: true,
            text: format!("{}: {}", loc.settings_status_error(), detail),
        }
    }

    pub fn is_error(&self) -> bool {
        self.error
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

pub(crate) fn interactive_probe(
    id: &str,
    role: &'static str,
    label: String,
    theme: &ThemeManager,
) -> WidgetProbe {
    let colors = theme.colors();
    WidgetProbe::interactive(
        WidgetMeta::new(id, role, label),
        colors.text,
        colors.surface,
        MIN_TOUCH_TARGET,
        MIN_TOUCH_TARGET,
    )
    .with_focus(true, FOCUS_OUTLINE_WIDTH)
    .with_textual_status(true)
}

pub(crate) fn status_probe(id: &str, label: String, theme: &ThemeManager) -> WidgetProbe {
    let colors = theme.colors();
    WidgetProbe::non_interactive(
        WidgetMeta::new(id, "status", label),
        colors.text,
        colors.surface,
    )
    .with_textual_status(true)
}

/// A deterministic button with a stable widget id and a focus ring.
pub(crate) fn action_button(
    label: &str,
    id: &str,
    ui: &mut egui::Ui,
    theme: &ThemeManager,
) -> bool {
    let colors = theme.colors();
    let meta = WidgetMeta::new(id, "button", label);
    let response = ui
        .push_id(focus::widget_id(&meta), |ui| {
            ui.add(egui::Button::new(label).min_size(focus::min_touch_size(egui::Vec2::ZERO)))
        })
        .inner;
    focus::apply_focus_ring(ui, &response, colors.text.as_egui());
    response.clicked()
}

/// A deterministic selectable tab with a stable widget id and a focus ring.
pub(crate) fn tab_button(
    label: &str,
    id: &str,
    selected: bool,
    ui: &mut egui::Ui,
    theme: &ThemeManager,
) -> bool {
    let colors = theme.colors();
    let meta = WidgetMeta::new(id, "tab", label);
    let response = ui
        .push_id(focus::widget_id(&meta), |ui| {
            ui.set_min_size(focus::min_touch_size(egui::Vec2::ZERO));
            ui.selectable_label(selected, label)
        })
        .inner;
    focus::apply_focus_ring(ui, &response, colors.text.as_egui());
    response.clicked()
}

/// All eight panels: six spec sub-screens plus General/About (Amendment E).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Network,
    Peers,
    Keystore,
    Config,
    Mcp,
    Acp,
    About,
}

impl SettingsTab {
    pub const ALL: [Self; 8] = [
        Self::General,
        Self::Network,
        Self::Peers,
        Self::Keystore,
        Self::Config,
        Self::Mcp,
        Self::Acp,
        Self::About,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Network => "network",
            Self::Peers => "peers",
            Self::Keystore => "keystore",
            Self::Config => "config",
            Self::Mcp => "mcp",
            Self::Acp => "acp",
            Self::About => "about",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::General => 0,
            Self::Network => 1,
            Self::Peers => 2,
            Self::Keystore => 3,
            Self::Config => 4,
            Self::Mcp => 5,
            Self::Acp => 6,
            Self::About => 7,
        }
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::General)
    }

    fn label(self, loc: &Localization) -> String {
        match self {
            Self::General => loc.settings_general(),
            Self::Network => loc.settings_network(),
            Self::Peers => loc.settings_peers(),
            Self::Keystore => loc.settings_keystore(),
            Self::Config => loc.settings_config(),
            Self::Mcp => loc.settings_mcp(),
            Self::Acp => loc.settings_acp(),
            Self::About => loc.settings_about(),
        }
    }
}

pub struct SettingsScreen {
    backend: Arc<dyn SettingsBackend>,
    snapshot: SettingsSnapshot,
    pub active_tab: SettingsTab,
    pub network_panel: NetworkPanel,
    pub peers_panel: PeersPanel,
    pub keystore_panel: KeystorePanel,
    pub config_panel: ConfigPanel,
    pub mcp_panel: McpPanel,
    pub acp_panel: AcpPanel,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockSettingsBackend::new()))
    }

    pub fn with_backend(backend: Arc<dyn SettingsBackend>) -> Self {
        let snapshot = backend.snapshot();
        let mut screen = Self {
            backend,
            snapshot,
            active_tab: SettingsTab::General,
            network_panel: NetworkPanel::new(),
            peers_panel: PeersPanel::new(),
            keystore_panel: KeystorePanel::new(),
            config_panel: ConfigPanel::new(),
            mcp_panel: McpPanel::new(),
            acp_panel: AcpPanel::new(),
        };
        screen.peers_panel.set_backend(Arc::clone(&screen.backend));
        screen.config_panel.set_backend(Arc::clone(&screen.backend));
        screen.mcp_panel.set_backend(Arc::clone(&screen.backend));
        screen.acp_panel.set_backend(Arc::clone(&screen.backend));
        screen.apply_snapshot();
        screen
    }

    pub fn set_backend(&mut self, backend: Arc<dyn SettingsBackend>) {
        self.backend = backend;
        self.peers_panel.set_backend(Arc::clone(&self.backend));
        self.config_panel.set_backend(Arc::clone(&self.backend));
        self.mcp_panel.set_backend(Arc::clone(&self.backend));
        self.acp_panel.set_backend(Arc::clone(&self.backend));
        self.refresh();
    }

    pub fn backend(&self) -> Arc<dyn SettingsBackend> {
        Arc::clone(&self.backend)
    }

    pub fn snapshot(&self) -> &SettingsSnapshot {
        &self.snapshot
    }

    /// Pull a fresh snapshot. Explicit by design: an automatic refresh on
    /// every frame would discard the operator's in-progress config edits.
    pub fn refresh(&mut self) {
        self.snapshot = self.backend.snapshot();
        self.apply_snapshot();
    }

    fn apply_snapshot(&mut self) {
        self.network_panel
            .set_network(self.snapshot.network.clone());
        self.peers_panel.set_rows(self.snapshot.peers.clone());
        self.keystore_panel
            .set_keystore(self.snapshot.keystore.clone());
        self.config_panel.set_config(self.snapshot.config.clone());
        self.mcp_panel.set_mcp(self.snapshot.mcp.clone());
        self.acp_panel.set_acp(self.snapshot.acp.clone());
    }

    pub fn set_active_tab(&mut self, tab: SettingsTab) {
        self.active_tab = tab;
    }

    pub fn active_tab(&self) -> SettingsTab {
        self.active_tab
    }

    pub fn a11y_probes(&self, theme: &ThemeManager) -> Vec<WidgetProbe> {
        let loc = Localization::new();
        let mut probes = Vec::new();
        for tab in SettingsTab::ALL {
            probes.push(interactive_probe(
                &format!("settings-tab-{}", tab.id()),
                "tab",
                tab.label(&loc),
                theme,
            ));
        }
        probes.push(Card::new().title(loc.settings_title()).a11y_probe(theme));
        probes.push(status_probe(
            "settings-general-locale",
            loc.current_locale().to_string(),
            theme,
        ));
        probes.push(status_probe(
            "settings-about-version",
            env!("CARGO_PKG_VERSION").to_string(),
            theme,
        ));

        probes.extend(self.network_panel.a11y_probes(theme));
        probes.extend(self.peers_panel.a11y_probes(theme));
        probes.extend(self.keystore_panel.a11y_probes(theme));
        probes.extend(self.config_panel.a11y_probes(theme));
        probes.extend(self.mcp_panel.a11y_probes(theme));
        probes.extend(self.acp_panel.a11y_probes(theme));
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

    fn show_general(&self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let tokens = theme.tokens();
        Card::new()
            .title(loc.settings_general())
            .show_with(ui, theme, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(loc.settings_appearance()).strong());
                    ui.label(theme.get_theme().to_string());
                });
                ui.label(loc.current_locale());
                ui.add_space(tokens.spacing.sm);
                ui.label(loc.settings_read_only());
            });
    }

    fn show_about(&self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        Card::new()
            .title(loc.settings_about())
            .show_with(ui, theme, |ui| {
                ui.label(format!("Strangecoin {}", env!("CARGO_PKG_VERSION")));
            });
    }
}

impl Screen for SettingsScreen {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();

        ui.label(
            egui::RichText::new(localization.settings_title())
                .size(tokens.typography.xl.size)
                .strong(),
        );
        ui.add_space(tokens.spacing.lg);

        // `Tabs` renders the labels and tracks the active index; content is
        // rendered below from the synced `SettingsTab` (old settings.rs rule).
        let mut tabs = Tabs::new("settings-tabs");
        for tab in SettingsTab::ALL {
            let label = tab.label(localization);
            tabs = tabs.tab(tab.id(), &label, Box::new(TextTabContent::new("")));
        }
        tabs = tabs.active_tab(self.active_tab.index());
        tabs.show(ui, theme_manager);
        self.active_tab = SettingsTab::from_index(tabs.active_tab_index());

        ui.add_space(tokens.spacing.lg);

        match self.active_tab {
            SettingsTab::General => self.show_general(ui, theme_manager, localization),
            SettingsTab::Network => {
                self.network_panel.show(ui, theme_manager, localization);
            }
            SettingsTab::Peers => {
                self.peers_panel.show(ui, theme_manager, localization);
            }
            SettingsTab::Keystore => {
                self.keystore_panel.show(ui, theme_manager, localization);
            }
            SettingsTab::Config => {
                self.config_panel.show(ui, theme_manager, localization);
            }
            SettingsTab::Mcp => {
                self.mcp_panel.show(ui, theme_manager, localization);
            }
            SettingsTab::Acp => {
                self.acp_panel.show(ui, theme_manager, localization);
            }
            SettingsTab::About => self.show_about(ui, theme_manager, localization),
        }
    }

    fn title(&self, localization: &Localization) -> String {
        localization.settings_title()
    }

    fn on_back(&mut self) {}

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessibility::AccessibilityAuditor;
    use crate::theme::{Theme, ThemeManager};
    use std::path::PathBuf;
    use std::sync::Mutex;

    struct FailingBackend;

    impl SettingsBackend for FailingBackend {
        fn snapshot(&self) -> SettingsSnapshot {
            MockSettingsBackend::new().snapshot()
        }
    }

    struct OkBackend {
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl OkBackend {
        fn record(&self, call: &str) {
            self.calls.lock().expect("test lock").push(call.to_string());
        }
    }

    impl SettingsBackend for OkBackend {
        fn snapshot(&self) -> SettingsSnapshot {
            MockSettingsBackend::new().snapshot()
        }

        fn validate_config(&self, _raw: &str) -> Result<(), String> {
            self.record("validate");
            Ok(())
        }

        fn save_config(&self, raw: &str) -> Result<(), String> {
            self.validate_config(raw)?;
            self.record("save");
            Ok(())
        }

        fn set_peer_ban(&self, node_id: &str, banned: bool) -> Result<(), String> {
            self.record(&format!("ban:{node_id}:{banned}"));
            Ok(())
        }

        fn add_seed(&self, addr: &str) -> Result<(), String> {
            self.record(&format!("seed:{addr}"));
            Ok(())
        }

        fn set_mcp_enabled(&self, enabled: bool) -> Result<(), String> {
            self.record(&format!("mcp:{enabled}"));
            Ok(())
        }

        fn set_acp_enabled(&self, enabled: bool) -> Result<(), String> {
            self.record(&format!("acp:{enabled}"));
            Ok(())
        }
    }

    #[test]
    fn two_snapshots_are_equal_and_deterministic() {
        let screen = SettingsScreen::new();
        assert_eq!(screen.snapshot(), screen.snapshot());
        assert_eq!(screen.snapshot(), &MockSettingsBackend::new().snapshot());
        let mut other = SettingsScreen::new();
        other.refresh();
        assert_eq!(screen.snapshot(), other.snapshot());
    }

    #[test]
    fn tab_switch_round_trips_through_index() {
        let mut screen = SettingsScreen::new();
        assert_eq!(screen.active_tab(), SettingsTab::General);
        for tab in SettingsTab::ALL {
            screen.set_active_tab(tab);
            assert_eq!(screen.active_tab(), tab);
            assert_eq!(SettingsTab::from_index(tab.index()), tab);
        }
        screen.set_active_tab(SettingsTab::from_index(99));
        assert_eq!(screen.active_tab(), SettingsTab::General);
    }

    #[test]
    fn a11y_probes_pass_pure_rust_gate_with_stable_names() {
        let theme = ThemeManager::with_theme(
            PathBuf::from("target/settings-a11y-config.toml"),
            Theme::Dark,
        );
        let probes = SettingsScreen::new().a11y_probes(&theme);
        assert!(probes.len() >= 20, "got {} probes", probes.len());

        let names: Vec<&str> = probes
            .iter()
            .map(|probe| probe.meta.name.as_str())
            .collect();
        for required in [
            "settings-tab-network",
            "settings-tab-peers",
            "settings-tab-keystore",
            "settings-tab-config",
            "settings-tab-mcp",
            "settings-tab-acp",
            "settings-keystore-operator",
            "settings-keystore-agent",
            "settings-peers-ban",
            "settings-peers-add-seed",
            "settings-config-save",
            "settings-mcp-toggle",
            "settings-acp-toggle",
        ] {
            assert!(names.contains(&required), "missing probe {required}");
        }

        let auditor = AccessibilityAuditor::new();
        for probe in &probes {
            let result = auditor.audit_widget(probe);
            assert!(
                result.violations.is_empty(),
                "{}: {:?}",
                probe.meta.name,
                result.violations
            );
        }
    }

    #[test]
    fn failing_backend_mutations_surface_error_text() {
        let loc = Localization::new();
        let mut screen = SettingsScreen::with_backend(Arc::new(FailingBackend));

        screen.peers_panel.select_node("peer-a");
        screen.peers_panel.ban_selected(&loc);
        let status = screen.peers_panel.status().expect("ban status");
        assert!(status.is_error());
        assert!(status.text().contains("peer ban control is not available"));

        screen.config_panel.save(&loc);
        let status = screen.config_panel.status().expect("save status");
        assert!(status.is_error());
        assert!(status.text().contains("config validation is not available"));

        screen.mcp_panel.toggle(&loc);
        let status = screen.mcp_panel.status().expect("mcp status");
        assert!(status.is_error());
        assert!(status.text().contains("MCP toggle is not available"));

        screen.acp_panel.toggle(&loc);
        let status = screen.acp_panel.status().expect("acp status");
        assert!(status.is_error());
        assert!(status.text().contains("ACP toggle is not available"));
    }

    #[test]
    fn successful_backend_mutations_are_recorded_and_report_saved() {
        let loc = Localization::new();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut screen = SettingsScreen::with_backend(Arc::new(OkBackend {
            calls: Arc::clone(&calls),
        }));

        screen.peers_panel.select_node("peer-a");
        screen.peers_panel.ban_selected(&loc);
        assert_eq!(
            screen.peers_panel.status().map(StatusText::text),
            Some(loc.settings_status_saved().as_str())
        );

        screen.config_panel.save(&loc);
        screen.mcp_panel.toggle(&loc);
        screen.acp_panel.toggle(&loc);

        let calls = calls.lock().expect("test lock");
        assert_eq!(
            calls.as_slice(),
            [
                "ban:peer-a:true",
                "validate",
                "save",
                "mcp:true",
                "acp:true"
            ]
        );
    }
}
