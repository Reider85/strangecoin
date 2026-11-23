//! Wallet screen implementation

use crate::components::{Button, Card, CardSpacer, CardText, Input};
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;

/// Wallet screen with tabs for Send, History, and Contracts
pub struct WalletScreen {
    pub active_tab: WalletTab,
    pub send_tab: SendTab,
    pub history_tab: HistoryTab,
    pub contracts_tab: ContractsTab,
    /// Status line surfaced by the strangler launcher (login/send feedback).
    status: String,
    /// Legacy transfer form buffers kept for the strangler launcher.
    receiver_address: String,
    amount: String,
    ip: String,
    port: String,
}

/// Wallet tabs
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WalletTab {
    Send,
    History,
    Contracts,
}

impl WalletScreen {
    pub fn new() -> Self {
        Self {
            active_tab: WalletTab::Send,
            send_tab: SendTab::new(),
            history_tab: HistoryTab::new(),
            contracts_tab: ContractsTab::new(),
            status: String::new(),
            receiver_address: String::new(),
            amount: String::new(),
            ip: String::new(),
            port: String::new(),
        }
    }

    // ------------------------------------------------------------------
    // Strangler-launcher bridge (src/gui/strangler_migration.rs)
    // ------------------------------------------------------------------

    pub fn set_status(&mut self, status: String) {
        self.status = status;
    }

    pub fn status(&self) -> String {
        self.status.clone()
    }

    pub fn receiver_address(&self) -> &str {
        &self.receiver_address
    }

    pub fn receiver_address_mut(&mut self) -> &mut String {
        &mut self.receiver_address
    }

    pub fn amount(&self) -> &str {
        &self.amount
    }

    pub fn amount_mut(&mut self) -> &mut String {
        &mut self.amount
    }

    pub fn ip(&self) -> &str {
        &self.ip
    }

    pub fn ip_mut(&mut self) -> &mut String {
        &mut self.ip
    }

    pub fn port(&self) -> &str {
        &self.port
    }

    pub fn port_mut(&mut self) -> &mut String {
        &mut self.port
    }

    pub fn set_balance(&mut self, balance: u64) {
        self.send_tab.balance = balance;
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

    pub fn set_active_tab(&mut self, tab: WalletTab) {
        self.active_tab = tab;
    }

    pub fn send_tab_mut(&mut self) -> &mut SendTab {
        &mut self.send_tab
    }

    pub fn history_tab_mut(&mut self) -> &mut HistoryTab {
        &mut self.history_tab
    }

    pub fn contracts_tab_mut(&mut self) -> &mut ContractsTab {
        &mut self.contracts_tab
    }
}

impl Screen for WalletScreen {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        ui.horizontal(|ui| {
            // Tab navigation
            let tabs = [
                (WalletTab::Send, localization.wallet_send_tab()),
                (WalletTab::History, localization.wallet_history_tab()),
                (WalletTab::Contracts, localization.wallet_contracts_tab()),
            ];

            for (tab, label) in tabs {
                if ui.selectable_label(self.active_tab == tab, label).clicked() {
                    self.set_active_tab(tab);
                }
            }
        });

        ui.separator();

        // Show active tab content
        match self.active_tab {
            WalletTab::Send => self.send_tab.show(ui, theme_manager, localization),
            WalletTab::History => self.history_tab.show(ui, theme_manager, localization),
            WalletTab::Contracts => self.contracts_tab.show(ui, theme_manager, localization),
        }
    }

    fn title(&self, localization: &Localization) -> String {
        localization.wallet_title()
    }

    fn on_back(&mut self) {
        // No back navigation for wallet screen
    }

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl Default for WalletScreen {
    fn default() -> Self {
        Self::new()
    }
}

/// Send tab implementation
pub struct SendTab {
    pub recipient: Input,
    pub amount: Input,
    pub fee: Input,
    pub balance: u64,
    pub loading: bool,
    pub error: Option<String>,
}

impl SendTab {
    pub fn new() -> Self {
        Self {
            recipient: Input::new("Recipient")
                .input_type(crate::components::InputType::Bech32)
                .required(true),
            amount: Input::new("Amount")
                .input_type(crate::components::InputType::Number)
                .required(true),
            fee: Input::new("Fee").input_type(crate::components::InputType::Number),
            balance: 1000, // Mock balance
            loading: false,
            error: None,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();
        let colors = theme_manager.colors();

        // Balance card
        Card::new()
            .title("Balance")
            .child(CardText::new(&format!("{} SC", self.balance)))
            .show(ui, theme_manager);

        ui.add_space(tokens.spacing.lg);

        // Send form: inputs rendered inline inside the card frame
        // (they borrow `self`, so they cannot be stored as `'static` children)
        let mut form = Card::new().title("Send Transaction");
        let error_color = colors.error.as_egui();
        let loading = self.loading;
        form.show_with(ui, theme_manager, |ui| {
            self.recipient.show(ui, theme_manager);
            ui.add_space(tokens.spacing.sm);
            self.amount.show(ui, theme_manager);
            ui.add_space(tokens.spacing.sm);
            self.fee.show(ui, theme_manager);
            ui.add_space(tokens.spacing.base);

            if let Some(ref error) = self.error {
                ui.label(egui::RichText::new(error).color(error_color));
                ui.add_space(tokens.spacing.base);
            }

            let mut submit = Button::new(localization.wallet_send_submit())
                .loading(loading)
                .disabled(loading)
                .onclick(|| {
                    // Handle send logic
                    println!("Send transaction clicked");
                });
            submit.show(ui, theme_manager);
        });
    }

    pub fn set_balance(&mut self, balance: u64) {
        self.balance = balance;
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    pub fn recipient(&mut self) -> &mut Input {
        &mut self.recipient
    }

    pub fn amount(&mut self) -> &mut Input {
        &mut self.amount
    }

    pub fn fee(&mut self) -> &mut Input {
        &mut self.fee
    }
}

impl Default for SendTab {
    fn default() -> Self {
        Self::new()
    }
}

/// History tab implementation
pub struct HistoryTab {
    pub transactions: Vec<Transaction>,
    pub selected_transaction: Option<String>,
}

impl HistoryTab {
    pub fn new() -> Self {
        Self {
            transactions: vec![
                Transaction {
                    id: "tx_001".to_string(),
                    from: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q6"
                        .to_string(),
                    to: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q7"
                        .to_string(),
                    amount: 100,
                    fee: 1,
                    status: TransactionStatus::Success,
                    timestamp: 1642697400,
                },
                Transaction {
                    id: "tx_002".to_string(),
                    from: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q7"
                        .to_string(),
                    to: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q6"
                        .to_string(),
                    amount: 50,
                    fee: 1,
                    status: TransactionStatus::Pending,
                    timestamp: 1642697400,
                },
            ],
            selected_transaction: None,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        _localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();

        // Transaction list (scroll content rendered inside the card frame)
        let mut card = Card::new().title("Recent Transactions");
        card.show_with(ui, theme_manager, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for tx in &self.transactions {
                    Card::new()
                        .child(CardText::new(&format!("ID: {}", tx.id)))
                        .child(CardText::new(&format!("From: {}", tx.from)))
                        .child(CardText::new(&format!("To: {}", tx.to)))
                        .child(CardText::new(&format!("Amount: {} SC", tx.amount)))
                        .child(CardText::new(&format!("Fee: {} SC", tx.fee)))
                        .child(CardText::new(&format!("Status: {:?}", tx.status)).color(
                            match tx.status {
                                TransactionStatus::Success => {
                                    theme_manager.colors().success.as_egui()
                                }
                                TransactionStatus::Pending => {
                                    theme_manager.colors().warning.as_egui()
                                }
                                TransactionStatus::Failed => theme_manager.colors().error.as_egui(),
                            },
                        ))
                        .child(CardText::new(&format!("Time: {}", tx.timestamp)))
                        .child(CardSpacer::new(tokens.spacing.sm))
                        .child(Button::new("View Details").onclick({
                            let tx_id = tx.id.clone();
                            move || println!("View transaction details: {}", tx_id)
                        }))
                        .show(ui, theme_manager);

                    ui.add_space(tokens.spacing.base);
                }
            });
        });
    }

    pub fn transactions(&mut self) -> &mut Vec<Transaction> {
        &mut self.transactions
    }

    pub fn selected_transaction(&mut self) -> &mut Option<String> {
        &mut self.selected_transaction
    }
}

impl Default for HistoryTab {
    fn default() -> Self {
        Self::new()
    }
}

/// Contracts tab implementation
pub struct ContractsTab {
    pub contracts: Vec<Contract>,
    pub selected_contract: Option<String>,
}

impl ContractsTab {
    pub fn new() -> Self {
        Self {
            contracts: vec![
                Contract {
                    address: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q6"
                        .to_string(),
                    name: "Token Contract".to_string(),
                    abi: "{}".to_string(),
                },
                Contract {
                    address: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q7"
                        .to_string(),
                    name: "NFT Contract".to_string(),
                    abi: "{}".to_string(),
                },
            ],
            selected_contract: None,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        _localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();

        // Contract list (scroll content rendered inside the card frame)
        let mut card = Card::new().title("Deployed Contracts");
        card.show_with(ui, theme_manager, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for contract in &self.contracts {
                    Card::new()
                        .child(CardText::new(&format!("Name: {}", contract.name)))
                        .child(CardText::new(&format!("Address: {}", contract.address)))
                        .child(Button::new("View ABI").onclick({
                            let addr = contract.address.clone();
                            move || println!("View ABI for contract: {}", addr)
                        }))
                        .child(Button::new("Interact").onclick({
                            let addr = contract.address.clone();
                            move || println!("Interact with contract: {}", addr)
                        }))
                        .show(ui, theme_manager);

                    ui.add_space(tokens.spacing.base);
                }
            });
        });
    }

    pub fn contracts(&mut self) -> &mut Vec<Contract> {
        &mut self.contracts
    }

    pub fn selected_contract(&mut self) -> &mut Option<String> {
        &mut self.selected_contract
    }
}

impl Default for ContractsTab {
    fn default() -> Self {
        Self::new()
    }
}

/// Transaction data structure
#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: String,
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub status: TransactionStatus,
    pub timestamp: u64,
}

/// Transaction status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransactionStatus {
    Success,
    Pending,
    Failed,
}

/// Contract data structure
#[derive(Debug, Clone)]
pub struct Contract {
    pub address: String,
    pub name: String,
    pub abi: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_screen_creation() {
        let screen = WalletScreen::new();
        assert_eq!(screen.active_tab, WalletTab::Send);
    }

    #[test]
    fn test_wallet_tab_switching() {
        let mut screen = WalletScreen::new();
        assert_eq!(screen.active_tab, WalletTab::Send);

        screen.set_active_tab(WalletTab::History);
        assert_eq!(screen.active_tab, WalletTab::History);

        screen.set_active_tab(WalletTab::Contracts);
        assert_eq!(screen.active_tab, WalletTab::Contracts);
    }

    #[test]
    fn test_send_tab_creation() {
        let tab = SendTab::new();
        assert_eq!(tab.balance, 1000);
        assert!(!tab.loading);
        assert!(tab.error.is_none());
    }

    #[test]
    fn test_history_tab_creation() {
        let tab = HistoryTab::new();
        assert_eq!(tab.transactions.len(), 2);
        assert!(tab.selected_transaction.is_none());
    }

    #[test]
    fn test_contracts_tab_creation() {
        let tab = ContractsTab::new();
        assert_eq!(tab.contracts.len(), 2);
        assert!(tab.selected_contract.is_none());
    }
}
