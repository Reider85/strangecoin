//! Transaction Overview panel (UI-P04) — identity, type, parties, amounts,
//! status and block placement. All values come from a `TxDetailBackend`.

use crate::accessibility::WidgetProbe;
use crate::backend::TxOverview;
use crate::components::CodeBlock;
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub struct OverviewPanel {
    overview: Option<TxOverview>,
}

impl OverviewPanel {
    pub fn new() -> Self {
        Self { overview: None }
    }

    pub fn set_overview(&mut self, overview: Option<TxOverview>) {
        self.overview = overview;
    }

    pub fn overview(&self) -> Option<&TxOverview> {
        self.overview.as_ref()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let Some(overview) = &self.overview else {
            ui.label(loc.tx_detail_no_selection());
            return;
        };
        let colors = theme.colors();

        ui.label(egui::RichText::new(loc.tx_detail_txid()).strong());
        CodeBlock::new("tx-detail-txid", &overview.txid)
            .language("hex")
            .copy_button(true)
            .show(ui, theme);
        ui.add_space(theme.tokens().spacing.sm);

        for (label, value) in [
            (loc.tx_detail_kind(), overview.kind.clone()),
            (loc.tx_detail_from(), overview.from.clone()),
            (loc.tx_detail_to(), overview.to.clone()),
            (loc.tx_detail_amount(), overview.amount.to_string()),
            (loc.tx_detail_fee(), overview.fee.to_string()),
            (loc.tx_detail_gas_used(), overview.gas_used.to_string()),
            (
                loc.tx_detail_block(),
                overview
                    .block_height
                    .map(|height| height.to_string())
                    .unwrap_or_else(|| loc.tx_detail_status_pending()),
            ),
            (loc.tx_detail_timestamp(), overview.timestamp.to_string()),
        ] {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).strong());
                ui.label(value);
            });
        }

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(loc.tx_detail_status()).strong());
            let (text, color) = match overview.status {
                crate::backend::TxStatus::Success => {
                    (loc.status_confirmed(), colors.success.as_egui())
                }
                crate::backend::TxStatus::Pending => {
                    (loc.tx_detail_status_pending(), colors.warning.as_egui())
                }
                crate::backend::TxStatus::Failed => (loc.status_failed(), colors.error.as_egui()),
            };
            // Text label accompanies the color (WCAG 1.4.1).
            ui.label(egui::RichText::new(text).color(color));
        });

        ui.add_space(theme.tokens().spacing.sm);
        ui.label(egui::RichText::new(loc.tx_detail_input_data()).strong());
        CodeBlock::new("tx-detail-input", &overview.input)
            .language("hex")
            .show(ui, theme);
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        let mut probes = vec![
            CodeBlock::new("tx-detail-txid", "0x").a11y_probe(theme),
            WidgetProbe::non_interactive(
                WidgetMeta::new("tx-detail-txid", "status", loc.tx_detail_txid()),
                colors.text,
                colors.surface,
            ),
            WidgetProbe::non_interactive(
                WidgetMeta::new("tx-detail-overview", "status", loc.tx_detail_overview()),
                colors.text,
                colors.surface,
            ),
        ];
        for (id, label) in [
            ("tx-detail-kind", loc.tx_detail_kind()),
            ("tx-detail-from", loc.tx_detail_from()),
            ("tx-detail-to", loc.tx_detail_to()),
            ("tx-detail-amount", loc.tx_detail_amount()),
            ("tx-detail-fee", loc.tx_detail_fee()),
            ("tx-detail-gas-used", loc.tx_detail_gas_used()),
            ("tx-detail-block", loc.tx_detail_block()),
            ("tx-detail-timestamp", loc.tx_detail_timestamp()),
        ] {
            probes.push(WidgetProbe::non_interactive(
                WidgetMeta::new(id, "status", label),
                colors.text,
                colors.surface,
            ));
        }
        probes.push(
            WidgetProbe::interactive(
                WidgetMeta::new("tx-detail-copy", "button", loc.tx_detail_copy()),
                colors.text,
                colors.surface,
                MIN_TOUCH_TARGET,
                MIN_TOUCH_TARGET,
            )
            .with_focus(true, FOCUS_OUTLINE_WIDTH),
        );
        probes
    }
}

impl Default for OverviewPanel {
    fn default() -> Self {
        Self::new()
    }
}
