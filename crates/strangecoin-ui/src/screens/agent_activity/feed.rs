//! Activity feed panel (UI-P05) — chronological typed table over
//! [`ActivityRecord`] rows supplied by the screen. Stub rows (unavailable live
//! source) carry an explicit localized marker instead of silently showing
//! placeholder data. Row clicks feed the one-shot transaction / ACP seams.

use crate::accessibility::WidgetProbe;
use crate::backend::ActivityRecord;
use crate::components::Table;
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;
use std::collections::HashMap;

use super::filter::{event_type_label, EVENT_TYPES};
use super::{interactive_probe, status_probe};

pub struct FeedPanel {
    rows: Vec<ActivityRecord>,
    selected_transaction: Option<String>,
    selected_acp_message: Option<String>,
}

impl FeedPanel {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            selected_transaction: None,
            selected_acp_message: None,
        }
    }

    /// Rows arrive already sorted by the screen refresh (timestamp
    /// descending, id as a deterministic tie-break).
    pub fn set_rows(&mut self, rows: Vec<ActivityRecord>) {
        self.rows = rows;
    }

    pub fn rows(&self) -> &[ActivityRecord] {
        &self.rows
    }

    /// One-shot navigation seam consumed by `StranglerLauncher`.
    pub fn take_selected_transaction(&mut self) -> Option<String> {
        self.selected_transaction.take()
    }

    /// One-shot navigation seam consumed by the ACP detail modal wiring.
    pub fn take_selected_acp_message(&mut self) -> Option<String> {
        self.selected_acp_message.take()
    }

    fn select_row(&mut self, index: usize) {
        let Some(record) = self.rows.get(index) else {
            return;
        };
        if let Some(message) = record.acp_message.clone() {
            self.selected_acp_message = Some(message);
        } else if let Some(txid) = record.txid.clone() {
            self.selected_transaction = Some(txid);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        if self.rows.is_empty() {
            ui.label(loc.agent_activity_no_match());
            return;
        }

        let stub = loc.agent_activity_stub();
        let type_labels: HashMap<_, _> = EVENT_TYPES
            .iter()
            .map(|kind| (*kind, event_type_label(*kind, loc)))
            .collect();
        let rows = self.rows.clone();

        let mut table = Table::new()
            .column("time", &loc.agent_activity_col_time())
            .width(110.0)
            .column("agent", &loc.agent_activity_col_agent())
            .width(150.0)
            .column("type", &loc.agent_activity_col_type())
            .width(150.0)
            .column("details", &loc.agent_activity_col_details())
            .width(320.0)
            .cell_renderer(
                move |row: &ActivityRecord, column: &str, ui: &mut egui::Ui| match column {
                    "time" => {
                        ui.label(row.timestamp.to_string());
                    }
                    "agent" => {
                        ui.monospace(&row.agent_address);
                    }
                    "type" => {
                        if let Some(label) = type_labels.get(&row.event_type) {
                            ui.label(label.as_str());
                        }
                    }
                    "details" => {
                        if row.available {
                            ui.label(&row.details);
                        } else {
                            ui.label(format!("{} ({})", row.details, stub));
                        }
                    }
                    _ => {}
                },
            )
            .data(rows);
        table.show(ui, theme);

        if let Some(index) = table.selected_row() {
            self.select_row(index);
        }
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let mut probes = vec![interactive_probe(
            theme,
            "agent-activity-feed",
            "button",
            loc.agent_activity_feed_title(),
        )];
        for (id, label) in [
            ("agent-activity-col-time", loc.agent_activity_col_time()),
            ("agent-activity-col-agent", loc.agent_activity_col_agent()),
            ("agent-activity-col-type", loc.agent_activity_col_type()),
            (
                "agent-activity-col-details",
                loc.agent_activity_col_details(),
            ),
        ] {
            probes.push(interactive_probe(theme, id, "columnheader", label));
        }

        // Textual status alongside every color-coded row state (WCAG 1.4.1).
        probes.push(status_probe(
            theme,
            "agent-activity-status-available",
            loc.success(),
        ));
        probes.push(status_probe(
            theme,
            "agent-activity-status-stub",
            loc.agent_activity_stub(),
        ));
        probes.push(status_probe(
            theme,
            "agent-activity-no-match",
            loc.agent_activity_no_match(),
        ));
        probes.push(status_probe(
            theme,
            "agent-activity-acp-message",
            loc.agent_activity_acp_detail(),
        ));
        for kind in EVENT_TYPES {
            probes.push(status_probe(
                theme,
                format!(
                    "agent-activity-event-{}",
                    kind.as_str().to_ascii_lowercase().replace('_', "-")
                ),
                event_type_label(kind, loc),
            ));
        }
        probes
    }
}

impl Default for FeedPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::ActivityEventType;

    fn record(txid: Option<&str>, acp_message: Option<&str>) -> ActivityRecord {
        ActivityRecord {
            id: "activity-01".to_string(),
            timestamp: 1_700_000_000,
            agent_address: "sc1agent".to_string(),
            event_type: ActivityEventType::AcpTask,
            details: "details".to_string(),
            txid: txid.map(str::to_string),
            acp_message: acp_message.map(str::to_string),
            available: true,
        }
    }

    #[test]
    fn row_click_prefers_acp_message_over_transaction() {
        let mut panel = FeedPanel::new();
        panel.set_rows(vec![record(Some("0x01"), Some("ACP-NEGOTIATE"))]);
        panel.select_row(0);
        assert_eq!(
            panel.take_selected_acp_message().as_deref(),
            Some("ACP-NEGOTIATE")
        );
        assert_eq!(panel.take_selected_transaction(), None);
    }

    #[test]
    fn row_click_sets_transaction_when_no_acp_message() {
        let mut panel = FeedPanel::new();
        panel.set_rows(vec![record(Some("0x02"), None)]);
        panel.select_row(0);
        assert_eq!(panel.take_selected_transaction().as_deref(), Some("0x02"));
        assert_eq!(panel.take_selected_acp_message(), None);
    }

    #[test]
    fn selection_seams_are_one_shot() {
        let mut panel = FeedPanel::new();
        panel.set_rows(vec![record(Some("0x03"), None)]);
        panel.select_row(0);
        assert!(panel.take_selected_transaction().is_some());
        assert!(panel.take_selected_transaction().is_none());
    }
}
