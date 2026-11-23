//! Transaction detail screen implementation

use crate::components::{Button, Card, CardText, Input, Table};
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;

/// Transaction detail screen with detailed transaction information
pub struct TxDetailScreen {
    pub transaction_id: Option<String>,
    pub transaction: Option<TransactionDetail>,
    pub loading: bool,
    pub error: Option<String>,
}

/// Detailed transaction information
pub struct TransactionDetail {
    pub id: String,
    pub hash: String,
    pub block_hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub from: String,
    pub to: String,
    pub value: u64,
    pub fee: u64,
    pub gas_used: u64,
    pub gas_limit: u64,
    pub status: TransactionStatus,
    pub input: String,
    pub confirmations: u64,
}

/// Transaction status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransactionStatus {
    Pending,
    Success,
    Failed,
}

impl TxDetailScreen {
    pub fn new() -> Self {
        Self {
            transaction_id: None,
            transaction: None,
            loading: false,
            error: None,
        }
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

    pub fn set_transaction_id(&mut self, id: &str) {
        self.transaction_id = Some(id.to_string());
        self.loading = true;
        self.error = None;

        // In a real implementation, this would fetch the transaction
        // For now, we'll create a mock transaction
        self.transaction = Some(TransactionDetail {
            id: id.to_string(),
            hash: format!("0x{}", hex::encode(id.as_bytes())),
            block_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .to_string(),
            block_number: 12345,
            timestamp: 1642697400,
            from: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q6".to_string(),
            to: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q7".to_string(),
            value: 1000,
            fee: 10,
            gas_used: 21000,
            gas_limit: 21000,
            status: TransactionStatus::Success,
            input: "0x".to_string(),
            confirmations: 12,
        });

        self.loading = false;
    }

    pub fn transaction_id_mut(&mut self) -> &mut Option<String> {
        &mut self.transaction_id
    }

    pub fn transaction_mut(&mut self) -> &mut Option<TransactionDetail> {
        &mut self.transaction
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
        let colors = theme_manager.colors();

        // Header
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(localization.tx_detail_title())
                    .size(tokens.typography.xl.size)
                    .strong(),
            );

            // Transaction ID input
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Load Transaction").clicked() {
                    let load_id = self.transaction_id.clone();
                    if let Some(id) = load_id {
                        self.set_transaction_id(&id);
                    }
                }

                // Value round-trip: Input owns its buffer for the frame,
                // the result is copied back into screen state below.
                let mut input = Input::new("Transaction ID")
                    .placeholder("Enter transaction ID")
                    .value(self.transaction_id.as_deref().unwrap_or(""));
                input.show(ui, theme_manager);
                let typed = input.get_value().to_string();
                if self.transaction_id.as_deref() != Some(typed.as_str()) {
                    self.transaction_id = if typed.is_empty() { None } else { Some(typed) };
                }
            });
        });

        ui.add_space(tokens.spacing.lg);

        // Loading state
        if self.loading {
            ui.label("Loading transaction details...");
            return;
        }

        // Error state
        if let Some(ref error) = self.error {
            Card::new()
                .title("Error")
                .child(CardText::new(error).color(colors.error.as_egui()))
                .show(ui, theme_manager);
            return;
        }

        // Transaction details
        if let Some(ref tx) = self.transaction {
            // Basic info card
            Card::new()
                .title("Transaction Details")
                .child(
                    Card::new()
                        .child(CardText::new(&format!("ID: {}", tx.id)))
                        .child(CardText::new(&format!("Hash: {}", tx.hash)))
                        .child(CardText::new(&format!("Block: {}", tx.block_number)))
                        .child(CardText::new(&format!("Timestamp: {}", tx.timestamp)))
                        .child(CardText::new(&format!("Status: {:?}", tx.status)).color(
                            match tx.status {
                                TransactionStatus::Pending => colors.warning.as_egui(),
                                TransactionStatus::Success => colors.success.as_egui(),
                                TransactionStatus::Failed => colors.error.as_egui(),
                            },
                        )),
                )
                .show(ui, theme_manager);

            ui.add_space(tokens.spacing.lg);

            // Transfer info card
            Card::new()
                .title("Transfer Information")
                .child(
                    Card::new()
                        .child(CardText::new(&format!("From: {}", tx.from)))
                        .child(CardText::new(&format!("To: {}", tx.to)))
                        .child(CardText::new(&format!("Value: {} SC", tx.value)))
                        .child(CardText::new(&format!("Fee: {} SC", tx.fee)))
                        .child(CardText::new(&format!(
                            "Confirmations: {}",
                            tx.confirmations
                        ))),
                )
                .show(ui, theme_manager);

            ui.add_space(tokens.spacing.lg);

            // Gas info card
            Card::new()
                .title("Gas Information")
                .child(
                    Card::new()
                        .child(CardText::new(&format!("Gas Used: {}", tx.gas_used)))
                        .child(CardText::new(&format!("Gas Limit: {}", tx.gas_limit)))
                        .child(CardText::new(&format!(
                            "Gas Price: {} SC",
                            tx.fee / tx.gas_used
                        ))),
                )
                .show(ui, theme_manager);

            ui.add_space(tokens.spacing.lg);

            // Input data card
            Card::new()
                .title("Input Data")
                .child(
                    crate::components::CodeBlock::new("input", &tx.input)
                        .language("hex")
                        .theme(crate::components::CodeTheme::Dark),
                )
                .show(ui, theme_manager);

            ui.add_space(tokens.spacing.lg);

            // Raw data table
            Card::new()
                .title("Raw Transaction Data")
                .child(
                    Table::new()
                        .column("Property", "Property")
                        .column("Value", "Value")
                        .width(150.0)
                        .data(vec![
                            ("ID", tx.id.clone()),
                            ("Hash", tx.hash.clone()),
                            ("Block Hash", tx.block_hash.clone()),
                            ("Block Number", tx.block_number.to_string()),
                            ("Timestamp", tx.timestamp.to_string()),
                            ("From", tx.from.clone()),
                            ("To", tx.to.clone()),
                            ("Value", tx.value.to_string()),
                            ("Fee", tx.fee.to_string()),
                            ("Gas Used", tx.gas_used.to_string()),
                            ("Gas Limit", tx.gas_limit.to_string()),
                            ("Status", format!("{:?}", tx.status)),
                            ("Input", tx.input.clone()),
                            ("Confirmations", tx.confirmations.to_string()),
                        ]),
                )
                .show(ui, theme_manager);

            ui.add_space(tokens.spacing.lg);

            // Action buttons
            ui.horizontal(|ui| {
                Button::new("Copy Transaction Hash")
                    .onclick(|| {
                        // Copy to clipboard
                        println!("Copy transaction hash");
                    })
                    .show(ui, theme_manager);

                Button::new("View Block Explorer")
                    .onclick(|| {
                        // Open block explorer
                        println!("View block explorer");
                    })
                    .show(ui, theme_manager);

                Button::new("Export Transaction")
                    .variant(crate::components::ButtonVariant::Secondary)
                    .onclick(|| {
                        // Export transaction
                        println!("Export transaction");
                    })
                    .show(ui, theme_manager);
            });
        } else {
            Card::new()
                .title("No Transaction Selected")
                .child(CardText::new(
                    "Please enter a transaction ID to view details.",
                ))
                .show(ui, theme_manager);
        }
    }

    fn title(&self, localization: &Localization) -> String {
        localization.tx_detail_title()
    }

    fn on_back(&mut self) {
        // No back navigation for transaction detail screen
    }

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

    #[test]
    fn test_tx_detail_screen_creation() {
        let screen = TxDetailScreen::new();
        assert!(screen.transaction_id.is_none());
        assert!(screen.transaction.is_none());
        assert!(!screen.loading);
        assert!(screen.error.is_none());
    }

    #[test]
    fn test_tx_detail_screen_set_transaction_id() {
        let mut screen = TxDetailScreen::new();
        screen.set_transaction_id("tx_001");

        assert_eq!(screen.transaction_id, Some("tx_001".to_string()));
        assert!(screen.transaction.is_some());
        assert!(!screen.loading);
        assert!(screen.error.is_none());
    }

    #[test]
    fn test_tx_detail_screen_loading() {
        let mut screen = TxDetailScreen::new();
        assert!(!*screen.loading_mut());

        *screen.loading_mut() = true;
        assert!(*screen.loading_mut());
    }

    #[test]
    fn test_tx_detail_screen_error() {
        let mut screen = TxDetailScreen::new();
        assert!(screen.error.is_none());

        *screen.error_mut() = Some("Transaction not found".to_string());
        assert_eq!(screen.error, Some("Transaction not found".to_string()));
    }

    #[test]
    fn test_transaction_detail() {
        let tx = TransactionDetail {
            id: "tx_001".to_string(),
            hash: "0x1234567890abcdef".to_string(),
            block_hash: "0x1234567890abcdef".to_string(),
            block_number: 12345,
            timestamp: 1642697400,
            from: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q6".to_string(),
            to: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q7".to_string(),
            value: 1000,
            fee: 10,
            gas_used: 21000,
            gas_limit: 21000,
            status: TransactionStatus::Success,
            input: "0x".to_string(),
            confirmations: 12,
        };

        assert_eq!(tx.id, "tx_001");
        assert_eq!(tx.hash, "0x1234567890abcdef");
        assert_eq!(tx.block_number, 12345);
        assert_eq!(tx.status, TransactionStatus::Success);
        assert_eq!(tx.confirmations, 12);
    }

    #[test]
    fn test_transaction_statuses() {
        assert_eq!(TransactionStatus::Pending, TransactionStatus::Pending);
        assert_eq!(TransactionStatus::Success, TransactionStatus::Success);
        assert_eq!(TransactionStatus::Failed, TransactionStatus::Failed);
    }
}
