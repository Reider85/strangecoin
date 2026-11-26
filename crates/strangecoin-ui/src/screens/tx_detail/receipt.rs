//! Receipt panel (UI-P04) — status, gas breakdown, revert reason and logs.
//! Receipts are derived and only retained for the tip block (ADR-0024), so a
//! missing receipt is reported honestly instead of being fabricated.

use crate::accessibility::WidgetProbe;
use crate::backend::{TxReceipt, TxStatus};
use crate::components::CodeBlock;
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub struct ReceiptPanel {
    receipt: Option<TxReceipt>,
}

impl ReceiptPanel {
    pub fn new() -> Self {
        Self { receipt: None }
    }

    pub fn set_receipt(&mut self, receipt: Option<TxReceipt>) {
        self.receipt = receipt;
    }

    pub fn receipt(&self) -> Option<&TxReceipt> {
        self.receipt.as_ref()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let Some(receipt) = &self.receipt else {
            ui.label(loc.tx_detail_receipt_unavailable());
            return;
        };
        if !receipt.available {
            ui.label(loc.tx_detail_receipt_unavailable());
            return;
        }
        let colors = theme.colors();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(loc.tx_detail_status()).strong());
            let (text, color) = match receipt.status {
                TxStatus::Success => (loc.status_confirmed(), colors.success.as_egui()),
                TxStatus::Pending => (loc.tx_detail_status_pending(), colors.warning.as_egui()),
                TxStatus::Failed => (loc.status_failed(), colors.error.as_egui()),
            };
            ui.label(egui::RichText::new(text).color(color));
        });
        for (label, value) in [
            (loc.tx_detail_gas_used(), receipt.gas_used.to_string()),
            (loc.tx_detail_gas_limit(), receipt.gas_limit.to_string()),
        ] {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).strong());
                ui.label(value);
            });
        }
        if let Some(reason) = &receipt.revert_reason {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(loc.tx_detail_revert()).strong());
                ui.label(egui::RichText::new(reason).color(colors.error.as_egui()));
            });
        }
        if receipt.logs.is_empty() {
            ui.label(loc.tx_detail_events_empty());
        }
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        vec![
            WidgetProbe::non_interactive(
                WidgetMeta::new("tx-detail-receipt", "status", loc.tx_detail_receipt()),
                colors.text,
                colors.surface,
            ),
            WidgetProbe::interactive(
                WidgetMeta::new("tx-detail-gas-limit", "button", loc.tx_detail_gas_limit()),
                colors.text,
                colors.surface,
                MIN_TOUCH_TARGET,
                MIN_TOUCH_TARGET,
            )
            .with_focus(true, FOCUS_OUTLINE_WIDTH),
            CodeBlock::new("tx-detail-receipt-status", "0x").a11y_probe(theme),
        ]
    }
}

impl Default for ReceiptPanel {
    fn default() -> Self {
        Self::new()
    }
}
