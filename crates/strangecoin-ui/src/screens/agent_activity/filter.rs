//! Activity filter sidebar (UI-P05, ADR-0031).
//!
//! The panel owns only raw input buffers and a deterministic boolean
//! selection over [`EVENT_TYPES`]. [`spec`] converts them into an
//! [`ActivityFilterSpec`]; [`matches`] / [`apply_filter`] are pure helpers so
//! the feed stays a pure function of backend data plus the current spec.
//! No wall clock or RNG is used here.

use crate::accessibility::WidgetProbe;
use crate::backend::{ActivityEventType, ActivityFilterSpec, ActivityRecord};
use crate::components::Input;
use crate::focus::{self, WidgetMeta};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

use super::interactive_probe;

/// Stable order for the multi-select event-type control (V-21: no iteration
/// over a `HashMap` when rendering or building a spec).
pub const EVENT_TYPES: [ActivityEventType; 7] = [
    ActivityEventType::AcpNegotiate,
    ActivityEventType::AcpTask,
    ActivityEventType::AcpSettle,
    ActivityEventType::AcpRevoke,
    ActivityEventType::BatchedTx,
    ActivityEventType::SponsorTx,
    ActivityEventType::Transfer,
];

/// Localized label for an event type; every variant has a typed accessor.
pub fn event_type_label(kind: ActivityEventType, loc: &Localization) -> String {
    match kind {
        ActivityEventType::AcpNegotiate => loc.agent_event_acp_negotiate(),
        ActivityEventType::AcpTask => loc.agent_event_acp_task(),
        ActivityEventType::AcpSettle => loc.agent_event_acp_settle(),
        ActivityEventType::AcpRevoke => loc.agent_event_acp_revoke(),
        ActivityEventType::BatchedTx => loc.agent_event_batched_tx(),
        ActivityEventType::SponsorTx => loc.agent_event_sponsor_tx(),
        ActivityEventType::Transfer => loc.agent_event_transfer(),
    }
}

/// Pure predicate: an empty `event_types` means "no type constraint" (the
/// `ActivityFilterSpec::default()` case), otherwise the record type must be
/// one of the selected variants. Absent address / bounds impose no constraint.
pub fn matches(record: &ActivityRecord, spec: &ActivityFilterSpec) -> bool {
    let address_matches = match spec.agent_address.as_deref() {
        Some(address) => record.agent_address == address,
        None => true,
    };
    let type_matches = spec.event_types.is_empty() || spec.event_types.contains(&record.event_type);
    let from_matches = match spec.from {
        Some(from) => record.timestamp >= from,
        None => true,
    };
    let to_matches = match spec.to {
        Some(to) => record.timestamp <= to,
        None => true,
    };
    address_matches && type_matches && from_matches && to_matches
}

/// Filter without reordering: the caller (screen refresh) owns sort order.
pub fn apply_filter(records: &[ActivityRecord], spec: &ActivityFilterSpec) -> Vec<ActivityRecord> {
    records
        .iter()
        .filter(|record| matches(record, spec))
        .cloned()
        .collect()
}

fn parse_timestamp(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.parse::<u64>().ok()
    }
}

/// Filter sidebar with three text buffers and a seven-way multi-select.
pub struct FilterPanel {
    agent_address: String,
    from: String,
    to: String,
    selected_types: [bool; EVENT_TYPES.len()],
}

impl FilterPanel {
    pub fn new() -> Self {
        Self {
            agent_address: String::new(),
            from: String::new(),
            to: String::new(),
            selected_types: [true; EVENT_TYPES.len()],
        }
    }

    /// Current spec. Invalid timestamps are treated as "no bound" so a
    /// half-typed input never blanks the feed.
    pub fn spec(&self) -> ActivityFilterSpec {
        let address = self.agent_address.trim();
        ActivityFilterSpec {
            agent_address: if address.is_empty() {
                None
            } else {
                Some(address.to_string())
            },
            event_types: EVENT_TYPES
                .iter()
                .enumerate()
                .filter_map(|(index, kind)| self.selected_types[index].then_some(*kind))
                .collect(),
            from: parse_timestamp(&self.from),
            to: parse_timestamp(&self.to),
        }
    }

    pub fn clear(&mut self) {
        self.agent_address.clear();
        self.from.clear();
        self.to.clear();
        self.selected_types = [true; EVENT_TYPES.len()];
    }

    /// Test/bridge helper for setting the address buffer without an egui UI.
    pub fn set_agent_address(&mut self, address: &str) {
        self.agent_address = address.to_string();
    }

    /// Test/bridge helper for setting the timestamp buffers.
    pub fn set_range(&mut self, from: Option<u64>, to: Option<u64>) {
        self.from = match from {
            Some(value) => value.to_string(),
            None => String::new(),
        };
        self.to = match to {
            Some(value) => value.to_string(),
            None => String::new(),
        };
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let colors = theme.colors();
        let tokens = theme.tokens();

        ui.label(loc.agent_activity_filter_agent());
        let mut agent_input = Input::new(loc.agent_activity_filter_agent())
            .id("agent-activity-filter-agent")
            .value(&self.agent_address);
        agent_input.show(ui, theme);
        self.agent_address = agent_input.get_value().to_string();

        ui.add_space(tokens.spacing.sm);
        ui.label(loc.agent_activity_filter_types());
        ui.horizontal_wrapped(|ui| {
            for (index, kind) in EVENT_TYPES.iter().enumerate() {
                let label = event_type_label(*kind, loc);
                let selected = self.selected_types[index];
                let meta = WidgetMeta::new(
                    format!("agent-activity-filter-type-{index}"),
                    "button",
                    label.clone(),
                );
                let response = ui
                    .push_id(focus::widget_id(&meta), |ui| {
                        ui.add(
                            egui::Button::new(label.as_str())
                                .selected(selected)
                                .min_size(focus::min_touch_size(egui::Vec2::ZERO)),
                        )
                    })
                    .inner;
                focus::apply_focus_ring(ui, &response, colors.text.as_egui());
                if response.clicked() {
                    self.selected_types[index] = !selected;
                }
            }
        });

        ui.add_space(tokens.spacing.sm);
        ui.label(loc.agent_activity_filter_from());
        let mut from_input = Input::new(loc.agent_activity_filter_from())
            .id("agent-activity-filter-from")
            .value(&self.from);
        from_input.show(ui, theme);
        self.from = from_input.get_value().to_string();

        ui.add_space(tokens.spacing.sm);
        ui.label(loc.agent_activity_filter_to());
        let mut to_input = Input::new(loc.agent_activity_filter_to())
            .id("agent-activity-filter-to")
            .value(&self.to);
        to_input.show(ui, theme);
        self.to = to_input.get_value().to_string();

        ui.add_space(tokens.spacing.sm);
        let clear_label = loc.agent_activity_filter_clear();
        let meta = WidgetMeta::new("agent-activity-filter-clear", "button", clear_label.clone());
        let response = ui
            .push_id(focus::widget_id(&meta), |ui| {
                ui.add(
                    egui::Button::new(clear_label.as_str())
                        .min_size(focus::min_touch_size(egui::Vec2::ZERO)),
                )
            })
            .inner;
        focus::apply_focus_ring(ui, &response, colors.text.as_egui());
        if response.clicked() {
            self.clear();
        }
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let mut probes = vec![interactive_probe(
            theme,
            "agent-activity-filter",
            "button",
            loc.agent_activity_filter_title(),
        )];
        for (id, label) in [
            (
                "agent-activity-filter-agent",
                loc.agent_activity_filter_agent(),
            ),
            (
                "agent-activity-filter-types",
                loc.agent_activity_filter_types(),
            ),
            (
                "agent-activity-filter-from",
                loc.agent_activity_filter_from(),
            ),
            ("agent-activity-filter-to", loc.agent_activity_filter_to()),
            (
                "agent-activity-filter-clear",
                loc.agent_activity_filter_clear(),
            ),
        ] {
            probes.push(interactive_probe(theme, id, "button", label));
        }
        for (index, kind) in EVENT_TYPES.iter().enumerate() {
            probes.push(interactive_probe(
                theme,
                format!("agent-activity-filter-type-{index}"),
                "tab",
                event_type_label(*kind, loc),
            ));
        }
        probes
    }
}

impl Default for FilterPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(index: u64, kind: ActivityEventType, address: &str) -> ActivityRecord {
        ActivityRecord {
            id: format!("activity-{index}"),
            timestamp: 1_700_000_000 + index,
            agent_address: address.to_string(),
            event_type: kind,
            details: format!("details-{index}"),
            txid: None,
            acp_message: None,
            available: true,
        }
    }

    #[test]
    fn default_spec_matches_everything() {
        let records = vec![
            record(1, ActivityEventType::AcpTask, "sc1a"),
            record(2, ActivityEventType::Transfer, "sc1b"),
        ];
        assert_eq!(
            apply_filter(&records, &ActivityFilterSpec::default()).len(),
            2
        );
    }

    #[test]
    fn matches_honours_address_types_and_bounds() {
        let record = record(10, ActivityEventType::AcpSettle, "sc1a");
        assert!(matches(
            &record,
            &ActivityFilterSpec {
                agent_address: Some("sc1a".to_string()),
                event_types: vec![ActivityEventType::AcpSettle],
                from: Some(1_700_000_010),
                to: Some(1_700_000_010),
            }
        ));
        assert!(!matches(
            &record,
            &ActivityFilterSpec {
                agent_address: Some("sc1b".to_string()),
                ..ActivityFilterSpec::default()
            }
        ));
        assert!(!matches(
            &record,
            &ActivityFilterSpec {
                event_types: vec![ActivityEventType::Transfer],
                ..ActivityFilterSpec::default()
            }
        ));
        assert!(!matches(
            &record,
            &ActivityFilterSpec {
                from: Some(1_700_000_011),
                ..ActivityFilterSpec::default()
            }
        ));
        assert!(!matches(
            &record,
            &ActivityFilterSpec {
                to: Some(1_700_000_009),
                ..ActivityFilterSpec::default()
            }
        ));
    }

    #[test]
    fn apply_filter_is_order_stable_and_deterministic() {
        let records = vec![
            record(3, ActivityEventType::Transfer, "sc1a"),
            record(1, ActivityEventType::AcpTask, "sc1a"),
            record(2, ActivityEventType::Transfer, "sc1b"),
        ];
        let spec = ActivityFilterSpec {
            agent_address: None,
            event_types: vec![ActivityEventType::Transfer],
            from: None,
            to: None,
        };
        let first = apply_filter(&records, &spec);
        let second = apply_filter(&records, &spec);
        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["activity-3", "activity-2"]
        );
    }

    #[test]
    fn panel_spec_and_clear_are_deterministic() {
        let mut panel = FilterPanel::new();
        let initial = panel.spec();
        assert_eq!(initial.agent_address, None);
        assert_eq!(initial.event_types, EVENT_TYPES.to_vec());
        assert_eq!(initial.from, None);
        assert_eq!(initial.to, None);
        panel.set_agent_address(" sc1agent ");
        panel.set_range(Some(100), Some(200));
        panel.selected_types = [false; EVENT_TYPES.len()];
        panel.selected_types[1] = true;
        assert_eq!(
            panel.spec(),
            ActivityFilterSpec {
                agent_address: Some("sc1agent".to_string()),
                event_types: vec![ActivityEventType::AcpTask],
                from: Some(100),
                to: Some(200),
            }
        );
        panel.clear();
        assert_eq!(panel.spec(), initial);
    }

    #[test]
    fn invalid_timestamps_are_ignored() {
        let mut panel = FilterPanel::new();
        panel.from = "not-a-number".to_string();
        panel.to = String::new();
        assert_eq!(panel.spec().from, None);
        assert_eq!(panel.spec().to, None);
    }

    #[test]
    fn every_event_type_has_a_distinct_localized_label() {
        let loc = Localization::new();
        let labels: Vec<String> = EVENT_TYPES
            .iter()
            .map(|kind| event_type_label(*kind, &loc))
            .collect();
        assert!(labels.iter().all(|label| !label.trim().is_empty()));
        let unique: std::collections::BTreeSet<&String> = labels.iter().collect();
        assert_eq!(unique.len(), EVENT_TYPES.len());
    }
}
