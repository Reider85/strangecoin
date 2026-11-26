//! Transaction Detail screen (UI-P04, ADR-0031 Amendment D).
//!
//! Four panels — Overview, Receipt, Witness, Events — driven by a
//! `TxDetailBackend`. Navigated from the Wallet History row click through
//! `StranglerLauncher` (`WalletScreen::take_selected_transaction`).
//! Receipts are tip-only (ADR-0024) and witnesses are block-level and built
//! lazily (S3-P11); both limitations are surfaced honestly, never mocked in
//! the production adapter.

mod events;
mod overview;
mod receipt;
mod witness;

pub use events::EventsPanel;
pub use overview::OverviewPanel;
pub use receipt::ReceiptPanel;
pub use witness::WitnessPanel;

use crate::accessibility::WidgetProbe;
use crate::backend::{MockTxDetailBackend, TxDetailBackend, TxDetailData, TxVerifyResult};
use crate::components::{Card, Input};
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

pub struct TxDetailScreen {
    backend: Arc<dyn TxDetailBackend>,
    pub transaction_id: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    data: Option<TxDetailData>,
    pub overview_panel: OverviewPanel,
    pub receipt_panel: ReceiptPanel,
    pub witness_panel: WitnessPanel,
    pub events_panel: EventsPanel,
}

impl TxDetailScreen {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockTxDetailBackend::new()))
    }

    pub fn with_backend(backend: Arc<dyn TxDetailBackend>) -> Self {
        Self {
            backend,
            transaction_id: None,
            loading: false,
            error: None,
            data: None,
            overview_panel: OverviewPanel::new(),
            receipt_panel: ReceiptPanel::new(),
            witness_panel: WitnessPanel::new(),
            events_panel: EventsPanel::new(),
        }
    }

    pub fn set_backend(&mut self, backend: Arc<dyn TxDetailBackend>) {
        self.backend = backend;
        if let Some(txid) = self.transaction_id.clone() {
            self.set_transaction_id(&txid);
        }
    }

    pub fn backend(&self) -> Arc<dyn TxDetailBackend> {
        Arc::clone(&self.backend)
    }

    pub fn transaction_id(&self) -> &Option<String> {
        &self.transaction_id
    }

    pub fn data(&self) -> Option<&TxDetailData> {
        self.data.as_ref()
    }

    /// Load a transaction from the backend. History navigation and the
    /// explicit "Load Transaction" button both go through this single path.
    pub fn set_transaction_id(&mut self, id: &str) {
        self.transaction_id = Some(id.to_string());
        self.loading = true;
        self.error = None;
        self.data = None;
        self.overview_panel.set_overview(None);
        self.receipt_panel.set_receipt(None);
        self.witness_panel.reset();
        self.events_panel.set_events(Vec::new());

        match self.backend.load(id) {
            Ok(data) => {
                self.overview_panel.set_overview(data.overview.clone());
                self.receipt_panel.set_receipt(data.receipt.clone());
                self.events_panel.set_events(data.events.clone());
                self.data = Some(data);
            }
            Err(err) => {
                self.error = Some(err.to_string());
            }
        }
        self.loading = false;
    }

    fn verify(&mut self) {
        let Some(txid) = self.transaction_id.clone() else {
            return;
        };
        match self.backend.verify(&txid) {
            Ok(result) => self.witness_panel.set_result(result),
            Err(err) => {
                self.witness_panel.set_result(TxVerifyResult {
                    verified: false,
                    message: err.to_string(),
                    witness: None,
                });
            }
        }
    }

    pub fn a11y_probes(&self, theme: &ThemeManager) -> Vec<WidgetProbe> {
        let localization = Localization::new();
        let mut probes = vec![
            Input::new(localization.tx_detail_load()).a11y_probe(theme),
            Card::new()
                .title(localization.tx_detail_title())
                .a11y_probe(theme),
        ];
        probes.extend(self.overview_panel.a11y_probes(theme, &localization));
        probes.extend(self.receipt_panel.a11y_probes(theme, &localization));
        probes.extend(self.witness_panel.a11y_probes(theme, &localization));
        probes.extend(self.events_panel.a11y_probes(theme, &localization));
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

    pub fn transaction_id_mut(&mut self) -> &mut Option<String> {
        &mut self.transaction_id
    }

    pub fn loading_mut(&mut self) -> &mut bool {
        &mut self.loading
    }

    pub fn error_mut(&mut self) -> &mut Option<String> {
        &mut self.error
    }
}

impl Screen for TxDetailScreen {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(localization.tx_detail_title())
                    .size(tokens.typography.xl.size)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut load = false;
                if ui.button(localization.tx_detail_load()).clicked() {
                    load = true;
                }
                let mut input = Input::new(localization.tx_detail_load())
                    .placeholder(&localization.tx_detail_txid())
                    .value(self.transaction_id.as_deref().unwrap_or(""));
                input.show(ui, theme_manager);
                let typed = input.get_value().to_string();
                if self.transaction_id.as_deref() != Some(typed.as_str()) {
                    self.transaction_id = if typed.is_empty() { None } else { Some(typed) };
                }
                if load {
                    if let Some(txid) = self.transaction_id.clone() {
                        self.set_transaction_id(&txid);
                    }
                }
            });
        });
        ui.add_space(tokens.spacing.base);

        if self.witness_panel.take_verify_request() {
            self.verify();
        }

        if self.loading {
            ui.label(localization.tx_detail_loading());
            return;
        }
        if let Some(error) = &self.error {
            ui.label(egui::RichText::new(error).color(theme_manager.colors().error.as_egui()));
            return;
        }

        Card::new()
            .title(localization.tx_detail_overview())
            .show_with(ui, theme_manager, |ui| {
                self.overview_panel.show(ui, theme_manager, localization);
            });
        ui.add_space(tokens.spacing.base);

        Card::new()
            .title(localization.tx_detail_receipt())
            .show_with(ui, theme_manager, |ui| {
                self.receipt_panel.show(ui, theme_manager, localization);
            });
        ui.add_space(tokens.spacing.base);

        Card::new()
            .title(localization.tx_detail_witness())
            .show_with(ui, theme_manager, |ui| {
                self.witness_panel.show(ui, theme_manager, localization);
            });
        ui.add_space(tokens.spacing.base);

        Card::new()
            .title(localization.tx_detail_events())
            .show_with(ui, theme_manager, |ui| {
                self.events_panel.show(ui, theme_manager, localization);
            });
    }

    fn title(&self, localization: &Localization) -> String {
        localization.tx_detail_title()
    }

    fn on_back(&mut self) {}

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl Default for TxDetailScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::WalletError;

    #[test]
    fn mock_tx_is_loaded_into_all_panels() {
        let mut screen = TxDetailScreen::new();
        let txid = "0x0000000000000000000000000000000000000000000000000000000000000001";
        screen.set_transaction_id(txid);
        assert!(screen.error.is_none());
        assert!(!screen.loading);
        assert!(screen.overview_panel.overview().is_some());
        assert!(screen.receipt_panel.receipt().is_some());
        assert_eq!(screen.events_panel.events().len(), 1);
    }

    #[test]
    fn unknown_tx_fails_closed_without_fabricated_data() {
        let mut screen = TxDetailScreen::new();
        screen.set_transaction_id("unknown");
        assert!(screen.error.is_some());
        assert!(screen.data.is_none());
        assert!(screen.overview_panel.overview().is_none());
    }

    #[test]
    fn verify_only_builds_witness_on_request() {
        let mut screen = TxDetailScreen::new();
        let txid = "0x0000000000000000000000000000000000000000000000000000000000000001";
        screen.set_transaction_id(txid);
        assert!(!screen.witness_panel.take_verify_request());
        screen.verify();
        assert!(screen.witness_panel.result().is_some_and(|r| r.verified));
    }

    #[test]
    fn wallet_error_display_stays_stable() {
        assert_eq!(
            WalletError::NotFound("tx".to_string()).to_string(),
            "not found: tx"
        );
    }
}
