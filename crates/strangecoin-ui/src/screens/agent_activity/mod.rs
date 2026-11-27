//! Agent activity screen (UI-P05, ADR-0031).
//!
//! Pulls `ActivityRecord`s from an [`AgentActivityBackend`] (the mock keeps
//! `new()` usable in the gallery / a11y gate; the launcher injects the real
//! `agent_getActivity` adapter), applies a pure filter and renders a
//! chronological typed table. An ACP row click opens an owned modal with the
//! raw message; a transaction row click exposes a one-shot navigation seam.
//! No wall clock, RNG or fabricated data is used.

mod feed;
mod filter;

pub use feed::FeedPanel;
pub use filter::{apply_filter, event_type_label, matches, FilterPanel, EVENT_TYPES};

use crate::accessibility::WidgetProbe;
use crate::backend::{ActivityRecord, AgentActivityBackend, MockAgentActivityBackend};
use crate::components::modal::{Modal, ModalContent, ModalSize};
use crate::components::Card;
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

/// Shared probe builder: every interactive widget is named, focusable and
/// meets the 44x44 touch target (pure-Rust WCAG AA gate).
pub(crate) fn interactive_probe(
    theme: &ThemeManager,
    id: impl Into<String>,
    role: &'static str,
    label: impl Into<String>,
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
}

/// Status probes keep a textual status so color is never the only signal.
pub(crate) fn status_probe(
    theme: &ThemeManager,
    id: impl Into<String>,
    label: impl Into<String>,
) -> WidgetProbe {
    let colors = theme.colors();
    WidgetProbe::non_interactive(
        WidgetMeta::new(id, "status", label),
        colors.text,
        colors.surface,
    )
    .with_textual_status(true)
}

struct AcpMessageContent {
    message: String,
    detail_label: String,
    event_label: String,
}

impl ModalContent for AcpMessageContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        ui.label(egui::RichText::new(&self.detail_label).strong());
        ui.label(
            egui::RichText::new(&self.event_label)
                .size(tokens.typography.sm.size)
                .weak(),
        );
        ui.add_space(tokens.spacing.sm);
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .show(ui, |ui| {
                ui.monospace(&self.message);
            });
    }
}

pub struct AgentActivityScreen {
    backend: Arc<dyn AgentActivityBackend>,
    activities: Vec<ActivityRecord>,
    error: Option<String>,
    feed_panel: FeedPanel,
    filter_panel: FilterPanel,
    selected_transaction: Option<String>,
    selected_acp_message: Option<String>,
    acp_modal: Modal,
}

impl AgentActivityScreen {
    /// Deterministic mock-backed screen (tests, a11y gate, gallery).
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockAgentActivityBackend::new()))
    }

    pub fn with_backend(backend: Arc<dyn AgentActivityBackend>) -> Self {
        let mut screen = Self {
            backend,
            activities: Vec::new(),
            error: None,
            feed_panel: FeedPanel::new(),
            filter_panel: FilterPanel::new(),
            selected_transaction: None,
            selected_acp_message: None,
            acp_modal: Modal::new("agent-activity-acp-detail")
                .size(ModalSize::Md)
                .closable(true),
        };
        screen.refresh();
        screen
    }

    /// Swap the data source without rebuilding launcher navigation state.
    pub fn set_backend(&mut self, backend: Arc<dyn AgentActivityBackend>) {
        self.backend = backend;
        self.refresh();
    }

    pub fn backend(&self) -> Arc<dyn AgentActivityBackend> {
        Arc::clone(&self.backend)
    }

    pub fn activities(&self) -> &[ActivityRecord] {
        &self.activities
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Reload from the backend. Newest first, with the record id as a stable
    /// tie-break; a backend failure is recorded as text and clears the feed
    /// rather than inventing rows.
    pub fn refresh(&mut self) {
        match self.backend.activities() {
            Ok(mut records) => {
                records.sort_by(|left, right| {
                    right
                        .timestamp
                        .cmp(&left.timestamp)
                        .then_with(|| left.id.cmp(&right.id))
                });
                self.activities = records;
                self.error = None;
            }
            Err(err) => {
                self.activities.clear();
                self.error = Some(err.to_string());
            }
        }
        self.feed_panel.set_rows(self.activities.clone());
    }

    pub fn a11y_probes(&self, theme: &ThemeManager) -> Vec<WidgetProbe> {
        let localization = Localization::new();
        let mut probes = self.filter_panel.a11y_probes(theme, &localization);
        probes.extend(self.feed_panel.a11y_probes(theme, &localization));
        probes.push(interactive_probe(
            theme,
            "agent-activity-open-tx",
            "button",
            localization.agent_activity_open_tx(),
        ));
        probes.push(interactive_probe(
            theme,
            "agent-activity-open-acp",
            "button",
            localization.agent_activity_open_acp(),
        ));
        probes.push(interactive_probe(
            theme,
            "agent-activity-acp-modal",
            "button",
            localization.agent_activity_acp_detail(),
        ));
        probes
    }

    /// Render into an [`egui::Context`] (strangler launcher entry point).
    /// The modal is rendered after the central panel so it overlays the feed.
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            Screen::show(self, ui, theme_manager, localization);
        });
        self.acp_modal.show(ctx, theme_manager);
    }

    /// Tx chosen in the feed; the launcher navigates to Tx Detail (UI-P04).
    pub fn take_selected_transaction(&mut self) -> Option<String> {
        self.selected_transaction.take()
    }

    /// Raw ACP message chosen in the feed; consumed by the detail modal.
    pub fn take_selected_acp_message(&mut self) -> Option<String> {
        self.selected_acp_message.take()
    }

    pub fn filter_panel_mut(&mut self) -> &mut FilterPanel {
        &mut self.filter_panel
    }

    pub fn feed_panel_mut(&mut self) -> &mut FeedPanel {
        &mut self.feed_panel
    }
}

impl Screen for AgentActivityScreen {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        self.refresh();
        let tokens = theme_manager.tokens();

        ui.label(
            egui::RichText::new(localization.agent_activity_title())
                .size(tokens.typography.xl.size)
                .strong(),
        );
        ui.add_space(tokens.spacing.base);

        if let Some(ref error) = self.error {
            ui.label(egui::RichText::new(error).color(theme_manager.colors().error.as_egui()));
            ui.add_space(tokens.spacing.sm);
        }

        let spec = self.filter_panel.spec();
        self.feed_panel
            .set_rows(apply_filter(&self.activities, &spec));

        ui.horizontal(|ui| {
            Card::new()
                .title(localization.agent_activity_filter_title())
                .show_with(ui, theme_manager, |ui| {
                    self.filter_panel.show(ui, theme_manager, localization);
                });
            ui.add_space(tokens.spacing.base);
            Card::new()
                .title(localization.agent_activity_feed_title())
                .show_with(ui, theme_manager, |ui| {
                    self.feed_panel.show(ui, theme_manager, localization);
                });
        });

        if let Some(txid) = self.feed_panel.take_selected_transaction() {
            self.selected_transaction = Some(txid);
        }
        if let Some(message) = self.feed_panel.take_selected_acp_message() {
            // An ACP row opens the detail modal instead of navigating away.
            self.selected_transaction = None;
            let fallback_label = localization.agent_activity_acp_detail();
            let event_label = self
                .activities
                .iter()
                .find(|record| record.acp_message.as_deref() == Some(message.as_str()))
                .map(|record| event_type_label(record.event_type, localization))
                .unwrap_or(fallback_label);
            self.acp_modal
                .set_title(localization.agent_activity_acp_detail());
            self.acp_modal.set_content(AcpMessageContent {
                message: message.clone(),
                detail_label: localization.agent_activity_acp_detail(),
                event_label,
            });
            self.acp_modal.open();
            self.selected_acp_message = Some(message);
        }
    }

    fn title(&self, localization: &Localization) -> String {
        localization.agent_activity_title()
    }

    fn on_back(&mut self) {}

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl Default for AgentActivityScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ActivityEventType, WalletError};
    use crate::theme::Theme;
    use std::path::PathBuf;

    struct FailingBackend;

    impl AgentActivityBackend for FailingBackend {
        fn activities(&self) -> Result<Vec<ActivityRecord>, WalletError> {
            Err(WalletError::Backend("indexer offline".to_string()))
        }
    }

    fn record(id: &str, timestamp: u64) -> ActivityRecord {
        ActivityRecord {
            id: id.to_string(),
            timestamp,
            agent_address: "sc1agent".to_string(),
            event_type: ActivityEventType::AcpTask,
            details: "details".to_string(),
            txid: None,
            acp_message: None,
            available: true,
        }
    }

    #[test]
    fn refresh_is_deterministic_and_newest_first() {
        let backend = Arc::new(MockAgentActivityBackend::new().with_activities(vec![
            record("activity-b", 20),
            record("activity-a", 20),
            record("activity-c", 10),
        ]));
        let mut screen = AgentActivityScreen::with_backend(backend);
        let first = screen.activities().to_vec();
        screen.refresh();
        assert_eq!(screen.activities(), first.as_slice());
        assert_eq!(
            screen
                .activities()
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["activity-a", "activity-b", "activity-c"]
        );
    }

    #[test]
    fn backend_failure_is_recorded_without_fabricated_rows() {
        let mut screen = AgentActivityScreen::with_backend(Arc::new(FailingBackend));
        assert_eq!(screen.error(), Some("backend error: indexer offline"));
        assert!(screen.activities().is_empty());
        assert!(screen.feed_panel.rows().is_empty());
        screen.refresh();
        assert_eq!(screen.error(), Some("backend error: indexer offline"));
    }

    #[test]
    fn navigation_seams_are_one_shot() {
        let mut screen = AgentActivityScreen::new();
        screen.selected_transaction = Some("0xabc".to_string());
        screen.selected_acp_message = Some("acp-payload".to_string());
        assert_eq!(screen.take_selected_transaction().as_deref(), Some("0xabc"));
        assert_eq!(
            screen.take_selected_acp_message().as_deref(),
            Some("acp-payload")
        );
        assert_eq!(screen.take_selected_transaction(), None);
        assert_eq!(screen.take_selected_acp_message(), None);
    }

    #[test]
    fn default_backend_supplies_seven_stub_aware_rows() {
        let screen = AgentActivityScreen::new();
        assert_eq!(screen.activities().len(), 7);
        assert!(screen
            .activities()
            .iter()
            .any(|item| !item.available && item.txid.is_none()));
        assert!(screen
            .activities()
            .windows(2)
            .all(|pair| pair[0].timestamp >= pair[1].timestamp));
    }

    #[test]
    fn probes_expose_all_required_stable_names() {
        let theme = ThemeManager::with_theme(
            PathBuf::from("target/agent-activity-probes-config.toml"),
            Theme::Light,
        );
        let probes = AgentActivityScreen::new().a11y_probes(&theme);
        assert!(probes.len() >= 18);
        let names: Vec<&str> = probes
            .iter()
            .map(|probe| probe.meta.name.as_str())
            .collect();
        for required in [
            "agent-activity-feed",
            "agent-activity-filter",
            "agent-activity-filter-agent",
            "agent-activity-filter-types",
            "agent-activity-filter-from",
            "agent-activity-filter-to",
            "agent-activity-filter-clear",
            "agent-activity-open-tx",
            "agent-activity-open-acp",
            "agent-activity-acp-modal",
        ] {
            assert!(names.contains(&required), "missing probe `{required}`");
        }
        assert!(probes
            .iter()
            .filter(|probe| probe.meta.is_interactive())
            .all(|probe| probe.focus_ring && !probe.meta.label.trim().is_empty()));
    }
}
