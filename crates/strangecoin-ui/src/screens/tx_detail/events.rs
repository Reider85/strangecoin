//! Events panel (UI-P04) — decoded event logs (raw hex topics/data until the
//! Stage-4 indexer supplies an ABI, S4-core-P07).

use crate::accessibility::WidgetProbe;
use crate::backend::TxEventLog;
use crate::components::CodeBlock;
use crate::focus::WidgetMeta;
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub struct EventsPanel {
    events: Vec<TxEventLog>,
}

impl EventsPanel {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn set_events(&mut self, events: Vec<TxEventLog>) {
        self.events = events;
    }

    pub fn events(&self) -> &[TxEventLog] {
        &self.events
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        if self.events.is_empty() {
            ui.label(loc.tx_detail_events_empty());
            return;
        }
        for (index, event) in self.events.iter().enumerate() {
            ui.label(egui::RichText::new(format!("{} — {}", index + 1, event.address)).strong());
            ui.label(loc.tx_detail_event_topics());
            for topic in &event.topics {
                ui.monospace(topic);
            }
            ui.label(loc.tx_detail_event_data());
            CodeBlock::new(&format!("tx-detail-event-data-{index}"), &event.data)
                .language("hex")
                .show(ui, theme);
        }
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        let mut probes = vec![WidgetProbe::non_interactive(
            WidgetMeta::new("tx-detail-events", "status", loc.tx_detail_events()),
            colors.text,
            colors.surface,
        )];
        if let Some(event) = self.events.first() {
            probes.push(CodeBlock::new("tx-detail-event-data-0", &event.data).a11y_probe(theme));
        }
        probes
    }
}

impl Default for EventsPanel {
    fn default() -> Self {
        Self::new()
    }
}
