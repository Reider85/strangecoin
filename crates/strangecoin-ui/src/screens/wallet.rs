//! Wallet screen implementation (UI-P03).
//!
//! Three tabs — Send / History / Contracts — driven by [`WalletBackend`].
//! The mock backend keeps `WalletScreen::new()` usable by the a11y gate and
//! the gallery; the node passes a real in-process adapter via `with_backend`.

use std::sync::{Arc, Mutex};

use crate::accessibility::WidgetProbe;
use crate::backend::{
    ContractRecord, MockWalletBackend, TxHash, TxRecord, TxStatus, WalletBackend, WalletError,
};
use crate::components::toast::show_toast;
use crate::components::{Button, Card, CardText, CodeBlock, Input, InputType, Table, ToastLevel};
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;

const HISTORY_PAGE_SIZE: usize = 50;

type ContractOutcome = (ContractAction, Result<Vec<u8>, WalletError>);

/// Wallet screen with tabs for Send, History, and Contracts
pub struct WalletScreen {
    pub active_tab: WalletTab,
    pub send_tab: SendTab,
    pub history_tab: HistoryTab,
    pub contracts_tab: ContractsTab,
    /// Last tab rendered; drives one-shot data refresh (no wall clock).
    last_active_tab: Option<WalletTab>,
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
    /// Deterministic mock-backed screen (tests, a11y gate, gallery).
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockWalletBackend::new("sc1mock")))
    }

    pub fn with_backend(backend: Arc<dyn WalletBackend>) -> Self {
        Self {
            active_tab: WalletTab::Send,
            send_tab: SendTab::with_backend(Arc::clone(&backend)),
            history_tab: HistoryTab::with_backend(Arc::clone(&backend)),
            contracts_tab: ContractsTab::with_backend(backend),
            last_active_tab: None,
            status: String::new(),
            receiver_address: String::new(),
            amount: String::new(),
            ip: String::new(),
            port: String::new(),
        }
    }

    pub fn a11y_probes(&self, theme: &ThemeManager) -> Vec<WidgetProbe> {
        let mut probes = Vec::new();

        for (id, label) in [
            ("send", "Send"),
            ("history", "History"),
            ("contracts", "Contracts"),
        ] {
            probes.push(Self::interactive_probe(theme, id, "tab", label));
        }

        probes.push(self.send_tab.recipient.a11y_probe(theme));
        probes.push(self.send_tab.amount.a11y_probe(theme));
        probes.push(self.send_tab.fee.a11y_probe(theme));
        probes.push(Button::new("Send").a11y_probe(theme));
        probes.push(Self::interactive_probe(
            theme,
            "wallet-send-batch",
            "button",
            "Batch send",
        ));
        probes.push(Card::new().title("Balance").a11y_probe(theme));
        probes.push(Self::interactive_probe(
            theme,
            "wallet-history-filter",
            "combobox",
            "Transaction filter",
        ));
        probes.push(Self::interactive_probe(
            theme,
            "wallet-history-page",
            "button",
            "Next page",
        ));
        probes.push(Table::<TxRecord>::new().a11y_probe(theme));
        probes.push(Self::interactive_probe(
            theme,
            "wallet-contract-select",
            "button",
            "Select contract",
        ));
        probes.push(Self::interactive_probe(
            theme,
            "wallet-contract-read",
            "button",
            "Read contract",
        ));
        probes.push(Self::interactive_probe(
            theme,
            "wallet-contract-write",
            "button",
            "Write contract",
        ));
        probes.push(Self::interactive_probe(
            theme,
            "wallet-contract-abi",
            "button",
            "View ABI",
        ));
        probes
    }

    fn interactive_probe(
        theme: &ThemeManager,
        id: &str,
        role: &'static str,
        label: &str,
    ) -> WidgetProbe {
        let colors = theme.colors();
        WidgetProbe::interactive(
            WidgetMeta::new(id, role, label),
            colors.text,
            colors.background,
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        )
        .with_focus(true, FOCUS_OUTLINE_WIDTH)
    }

    /// Pull fresh data for the active tab once per tab activation.
    pub fn refresh(&mut self) {
        self.last_active_tab = None;
    }

    fn refresh_active_tab(&mut self) {
        match self.active_tab {
            WalletTab::Send => self.send_tab.refresh_balance(),
            WalletTab::History => self.history_tab.refresh(),
            WalletTab::Contracts => self.contracts_tab.refresh(),
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

    /// Tx chosen in History; the launcher navigates to Tx Detail (UI-P04).
    pub fn take_selected_transaction(&mut self) -> Option<String> {
        self.history_tab.selected_transaction.take()
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
        crate::components::toast::init_toast_manager();
        ui.horizontal(|ui| {
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

        if self.last_active_tab != Some(self.active_tab) {
            self.refresh_active_tab();
            self.last_active_tab = Some(self.active_tab);
        }

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
    pub last_txid: Option<TxHash>,
    backend: Arc<dyn WalletBackend>,
    submit_result: Arc<Mutex<Option<Result<TxHash, WalletError>>>>,
}

impl SendTab {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockWalletBackend::new("sc1mock")))
    }

    pub fn with_backend(backend: Arc<dyn WalletBackend>) -> Self {
        Self {
            recipient: Input::new("Recipient")
                .input_type(InputType::Bech32)
                .required(true),
            amount: Input::new("Amount")
                .input_type(InputType::Number)
                .required(true),
            fee: Input::new("Fee").input_type(InputType::Number),
            balance: 0,
            loading: false,
            error: None,
            last_txid: None,
            backend,
            submit_result: Arc::new(Mutex::new(None)),
        }
    }

    pub fn refresh_balance(&mut self) {
        match self.backend.get_balance(&self.backend.wallet_address()) {
            Ok(balance) => {
                self.balance = balance;
                self.error = None;
            }
            Err(err) => self.error = Some(err.to_string()),
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

        Card::new()
            .title(localization.wallet_balance())
            .child(CardText::new(&format!("{} SC", self.balance)))
            .show(ui, theme_manager);

        ui.add_space(tokens.spacing.lg);

        let mut form = Card::new().title(localization.wallet_send_form_title());
        let error_color = colors.error.as_egui();
        let loading = self.loading;
        let fee_default = self.backend.estimate_fee();
        if self.fee.get_value().is_empty() {
            self.fee.set_value(&fee_default.to_string());
        }

        let backend = Arc::clone(&self.backend);
        let result_slot = Arc::clone(&self.submit_result);
        let recipient = self.recipient.get_value().to_string();
        let amount = self.amount.get_value().to_string();
        let fee = self.fee.get_value().to_string();
        let success_prefix = localization.wallet_send_success("");
        let invalid_msg = localization.wallet_send_error_invalid_address();
        let balance_msg = localization.wallet_send_error_insufficient_balance();

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
                .onclick(move || {
                    let outcome = (|| -> Result<TxHash, WalletError> {
                        if !backend.validate_address(recipient.trim()) {
                            return Err(WalletError::InvalidAddress(invalid_msg.clone()));
                        }
                        let units = amount
                            .trim()
                            .parse::<u64>()
                            .map_err(|_| WalletError::InvalidAmount(amount.clone()))?;
                        if units == 0 {
                            return Err(WalletError::InvalidAmount(amount.clone()));
                        }
                        let fee_units = fee.trim().parse::<u64>().unwrap_or(0);
                        match backend.submit_transaction(recipient.trim(), units, fee_units) {
                            Ok(txid) => Ok(txid),
                            Err(WalletError::Rejected(_)) => {
                                Err(WalletError::Rejected(balance_msg.clone()))
                            }
                            Err(err) => Err(err),
                        }
                    })();
                    match &outcome {
                        Ok(txid) => show_toast(
                            "wallet-send",
                            &format!("{success_prefix}{txid}"),
                            ToastLevel::Success,
                        ),
                        Err(err) => show_toast("wallet-send", &err.to_string(), ToastLevel::Error),
                    }
                    if let Ok(mut slot) = result_slot.lock() {
                        *slot = Some(outcome);
                    }
                });
            submit.show(ui, theme_manager);

            ui.add_space(tokens.spacing.sm);
            let batch_backend = Arc::clone(&self.backend);
            let batch_slot = Arc::clone(&self.submit_result);
            let batch_msg = localization.wallet_send_batch_unavailable();
            Button::new(localization.wallet_send_batch())
                .onclick(move || {
                    let err = batch_backend
                        .submit_batch(&[])
                        .err()
                        .unwrap_or(WalletError::NotImplemented("batch unavailable"));
                    show_toast("wallet-batch", &batch_msg, ToastLevel::Warning);
                    if let Ok(mut slot) = batch_slot.lock() {
                        *slot = Some(Err(err));
                    }
                })
                .show(ui, theme_manager);
        });

        if let Ok(mut slot) = self.submit_result.lock() {
            if let Some(result) = slot.take() {
                match result {
                    Ok(txid) => {
                        self.last_txid = Some(txid);
                        self.error = None;
                    }
                    Err(err) => self.error = Some(err.to_string()),
                }
                self.loading = false;
            }
        }

        if let Some(ref txid) = self.last_txid {
            ui.add_space(tokens.spacing.sm);
            ui.label(egui::RichText::new(txid).weak().monospace());
        }
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistoryFilter {
    All,
    Sent,
    Received,
    Failed,
}

pub struct HistoryTab {
    pub transactions: Vec<TxRecord>,
    pub selected_transaction: Option<String>,
    filter: HistoryFilter,
    page: usize,
    error: Option<String>,
    backend: Arc<dyn WalletBackend>,
}

impl HistoryTab {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockWalletBackend::new("sc1mock")))
    }

    pub fn with_backend(backend: Arc<dyn WalletBackend>) -> Self {
        Self {
            transactions: Vec::new(),
            selected_transaction: None,
            filter: HistoryFilter::All,
            page: 0,
            error: None,
            backend,
        }
    }

    pub fn refresh(&mut self) {
        match self
            .backend
            .recent_transactions(&self.backend.wallet_address(), 200)
        {
            Ok(txs) => {
                self.transactions = txs;
                self.error = None;
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    fn filtered(&self) -> Vec<TxRecord> {
        let address = self.backend.wallet_address();
        self.transactions
            .iter()
            .filter(|tx| match self.filter {
                HistoryFilter::All => true,
                HistoryFilter::Sent => tx.from == address,
                HistoryFilter::Received => tx.to.as_deref() == Some(address.as_str()),
                HistoryFilter::Failed => tx.status == TxStatus::Failed,
            })
            .cloned()
            .collect()
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();
        let colors = theme_manager.colors();
        let error_color = colors.error.as_egui();

        ui.horizontal(|ui| {
            for (filter, label) in [
                (HistoryFilter::All, localization.wallet_history_filter_all()),
                (
                    HistoryFilter::Sent,
                    localization.wallet_history_filter_sent(),
                ),
                (
                    HistoryFilter::Received,
                    localization.wallet_history_filter_received(),
                ),
                (
                    HistoryFilter::Failed,
                    localization.wallet_history_filter_failed(),
                ),
            ] {
                if ui.selectable_label(self.filter == filter, label).clicked() {
                    self.filter = filter;
                    self.page = 0;
                }
            }
        });

        if let Some(ref error) = self.error {
            ui.label(egui::RichText::new(error).color(error_color));
        }

        let filtered = self.filtered();
        let pages = filtered.len().div_ceil(HISTORY_PAGE_SIZE).max(1);
        self.page = self.page.min(pages - 1);
        let start = self.page * HISTORY_PAGE_SIZE;
        let page_rows: Vec<TxRecord> = filtered
            .iter()
            .skip(start)
            .take(HISTORY_PAGE_SIZE)
            .cloned()
            .collect();

        if page_rows.is_empty() {
            ui.label(localization.wallet_empty_history());
        } else {
            let mut table = Table::new()
                .column("txid", &localization.wallet_history_col_txid())
                .column("type", &localization.wallet_history_col_type())
                .column("from", &localization.wallet_history_col_from())
                .column("to", &localization.wallet_history_col_to())
                .column("amount", &localization.wallet_history_col_amount())
                .column("fee", &localization.wallet_history_col_fee())
                .column("status", &localization.wallet_history_col_status())
                .column("height", &localization.wallet_history_col_height())
                .data(page_rows.clone())
                .cell_renderer(|tx: &TxRecord, column: &str, ui: &mut egui::Ui| {
                    let text = match column {
                        "txid" => tx.txid.clone(),
                        "type" => tx.kind.clone(),
                        "from" => tx.from.clone(),
                        "to" => tx.to.clone().unwrap_or_else(|| "—".to_string()),
                        "amount" => format!("{} SC", tx.amount),
                        "fee" => format!("{} SC", tx.fee),
                        "status" => format!("{:?}", tx.status),
                        "height" => tx
                            .height
                            .map(|h| h.to_string())
                            .unwrap_or_else(|| "—".to_string()),
                        _ => String::new(),
                    };
                    ui.label(text);
                });
            table.show(ui, theme_manager);

            if let Some(row) = table.selected_row() {
                if let Some(tx) = page_rows.get(row) {
                    self.selected_transaction = Some(tx.txid.clone());
                }
            }
        }

        ui.add_space(tokens.spacing.sm);
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(localization.wallet_history_prev())
                        .min_size(egui::vec2(MIN_TOUCH_TARGET, MIN_TOUCH_TARGET)),
                )
                .clicked()
                && self.page > 0
            {
                self.page -= 1;
            }
            ui.label(format!("{} / {}", self.page + 1, pages));
            if ui
                .add(
                    egui::Button::new(localization.wallet_history_next())
                        .min_size(egui::vec2(MIN_TOUCH_TARGET, MIN_TOUCH_TARGET)),
                )
                .clicked()
                && self.page + 1 < pages
            {
                self.page += 1;
            }
        });
    }

    pub fn set_filter(&mut self, filter: HistoryFilter) {
        self.filter = filter;
        self.page = 0;
    }

    pub fn transactions(&mut self) -> &mut Vec<TxRecord> {
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
    pub contracts: Vec<ContractRecord>,
    pub selected_contract: Option<String>,
    method_index: usize,
    args: Vec<String>,
    abi_open: bool,
    result: Option<Result<Vec<u8>, WalletError>>,
    error: Option<String>,
    backend: Arc<dyn WalletBackend>,
    contract_result: Arc<Mutex<Option<ContractOutcome>>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ContractAction {
    Read,
    Write,
}

impl ContractsTab {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(MockWalletBackend::new("sc1mock")))
    }

    pub fn with_backend(backend: Arc<dyn WalletBackend>) -> Self {
        Self {
            contracts: Vec::new(),
            selected_contract: None,
            method_index: 0,
            args: Vec::new(),
            abi_open: false,
            result: None,
            error: None,
            backend,
            contract_result: Arc::new(Mutex::new(None)),
        }
    }

    pub fn refresh(&mut self) {
        match self.backend.deployed_contracts() {
            Ok(contracts) => {
                self.contracts = contracts;
                self.error = None;
            }
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    fn selected(&self) -> Option<&ContractRecord> {
        self.contracts
            .iter()
            .find(|c| Some(c.address.as_str()) == self.selected_contract.as_deref())
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();
        let error_color = theme_manager.colors().error.as_egui();

        let mut card = Card::new().title(localization.wallet_deployed_contracts());
        card.show_with(ui, theme_manager, |ui| {
            if self.contracts.is_empty() {
                ui.label(localization.wallet_empty_contracts());
            }
            for contract in &self.contracts {
                let label = if contract.name.is_empty() {
                    contract.address.clone()
                } else {
                    format!("{} — {}", contract.name, contract.address)
                };
                if ui
                    .add(
                        egui::Button::new(label)
                            .min_size(egui::vec2(MIN_TOUCH_TARGET, MIN_TOUCH_TARGET)),
                    )
                    .clicked()
                {
                    self.selected_contract = Some(contract.address.clone());
                    self.method_index = 0;
                    self.args.clear();
                    self.result = None;
                    self.abi_open = false;
                }
            }
        });

        if let Some(ref error) = self.error {
            ui.label(egui::RichText::new(error).color(error_color));
        }

        ui.add_space(tokens.spacing.lg);

        let selected_address = self.selected().map(|c| c.address.clone());
        let methods = self
            .selected()
            .map(|c| c.methods.clone())
            .unwrap_or_default();
        if let (Some(address), true) = (selected_address, !methods.is_empty()) {
            let mut detail = Card::new().title(localization.wallet_interact());
            detail.show_with(ui, theme_manager, |ui| {
                let method_names: Vec<String> = methods
                    .iter()
                    .map(|m| {
                        if m.read_only {
                            format!("{} (read)", m.name)
                        } else {
                            format!("{} (write)", m.name)
                        }
                    })
                    .collect();
                egui::ComboBox::from_id_salt("wallet-contract-method")
                    .selected_text(
                        method_names
                            .get(self.method_index)
                            .cloned()
                            .unwrap_or_default(),
                    )
                    .show_ui(ui, |ui| {
                        for (index, name) in method_names.iter().enumerate() {
                            ui.selectable_value(&mut self.method_index, index, name);
                        }
                    });

                if let Some(method) = methods.get(self.method_index) {
                    if self.args.len() != method.inputs.len() {
                        self.args = vec![String::new(); method.inputs.len()];
                    }
                    for (index, input) in method.inputs.iter().enumerate() {
                        if let Some(arg) = self.args.get_mut(index) {
                            ui.horizontal(|ui| {
                                ui.label(input);
                                ui.text_edit_singleline(arg);
                            });
                        }
                    }

                    let backend = Arc::clone(&self.backend);
                    let slot = Arc::clone(&self.contract_result);
                    let method_name = method.name.clone();
                    let args = self.args.clone();
                    let addr = address.clone();
                    let read_only = method.read_only;
                    let action = if read_only {
                        ContractAction::Read
                    } else {
                        ContractAction::Write
                    };
                    let label = if read_only {
                        localization.wallet_contracts_read()
                    } else {
                        localization.wallet_contracts_write()
                    };
                    Button::new(label)
                        .onclick(move || {
                            let outcome = if read_only {
                                backend.call_contract(&addr, &method_name, &args)
                            } else {
                                backend
                                    .submit_call(&addr, &method_name, &args, 0)
                                    .map(|txid| txid.into_bytes())
                            };
                            if let Ok(mut guard) = slot.lock() {
                                *guard = Some((action, outcome));
                            }
                        })
                        .show(ui, theme_manager);
                }

                if ui
                    .add(
                        egui::Button::new(localization.wallet_view_abi())
                            .min_size(egui::vec2(MIN_TOUCH_TARGET, MIN_TOUCH_TARGET)),
                    )
                    .clicked()
                {
                    self.abi_open = !self.abi_open;
                }
                if self.abi_open {
                    let abi = self.selected().map(|c| c.abi.clone()).unwrap_or_default();
                    CodeBlock::new("wallet-contract-abi", &abi)
                        .language("json")
                        .show(ui, theme_manager);
                }
            });
        }

        if let Ok(mut guard) = self.contract_result.lock() {
            if let Some((action, outcome)) = guard.take() {
                match action {
                    ContractAction::Read => show_toast(
                        "wallet-contract-read",
                        &format!("{:?}", outcome),
                        ToastLevel::Info,
                    ),
                    ContractAction::Write => show_toast(
                        "wallet-contract-write",
                        &format!("{:?}", outcome),
                        ToastLevel::Success,
                    ),
                }
                self.result = Some(outcome);
            }
        }

        if let Some(ref result) = self.result {
            match result {
                Ok(bytes) => {
                    ui.label(format!("0x{}", hex::encode(bytes)));
                }
                Err(err) => {
                    ui.label(egui::RichText::new(err.to_string()).color(error_color));
                }
            }
        }
    }

    pub fn contracts(&mut self) -> &mut Vec<ContractRecord> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AbiMethod;

    fn tx(txid: &str, from: &str, status: TxStatus) -> TxRecord {
        TxRecord {
            txid: txid.to_string(),
            kind: "transfer".to_string(),
            from: from.to_string(),
            to: Some("sc1other".to_string()),
            amount: 100,
            fee: 1,
            status,
            height: Some(7),
            timestamp: 0,
        }
    }

    #[test]
    fn wallet_screen_starts_on_send_tab() {
        let screen = WalletScreen::new();
        assert_eq!(screen.active_tab, WalletTab::Send);
    }

    #[test]
    fn wallet_tab_switching() {
        let mut screen = WalletScreen::new();
        screen.set_active_tab(WalletTab::History);
        assert_eq!(screen.active_tab, WalletTab::History);
        screen.set_active_tab(WalletTab::Contracts);
        assert_eq!(screen.active_tab, WalletTab::Contracts);
    }

    #[test]
    fn send_tab_refreshes_balance_from_backend() {
        let backend = Arc::new(MockWalletBackend::new("sc1mock").with_balance(4242));
        let mut tab = SendTab::with_backend(backend);
        tab.refresh_balance();
        assert_eq!(tab.balance, 4242);
    }

    #[test]
    fn history_filter_and_pagination() {
        let backend = Arc::new(MockWalletBackend::new("sc1mock").with_transactions(vec![
            tx("0x01", "sc1mock", TxStatus::Success),
            tx("0x02", "sc1other", TxStatus::Failed),
        ]));
        let mut tab = HistoryTab::with_backend(backend);
        tab.refresh();
        assert_eq!(tab.filtered().len(), 2);
        tab.set_filter(HistoryFilter::Sent);
        assert_eq!(tab.filtered().len(), 1);
        tab.set_filter(HistoryFilter::Failed);
        assert_eq!(tab.filtered()[0].txid, "0x02");
    }

    #[test]
    fn contracts_tab_reads_backend_contracts() {
        let backend =
            Arc::new(
                MockWalletBackend::new("sc1mock").with_contracts(vec![ContractRecord {
                    address: "sc1contract".to_string(),
                    name: "Vault".to_string(),
                    abi: "[]".to_string(),
                    methods: vec![AbiMethod {
                        name: "balance".to_string(),
                        inputs: Vec::new(),
                        read_only: true,
                    }],
                }]),
            );
        let mut tab = ContractsTab::with_backend(backend);
        tab.refresh();
        assert_eq!(tab.contracts.len(), 1);
        assert_eq!(tab.contracts[0].methods.len(), 1);
    }

    #[test]
    fn batch_send_fails_closed() {
        let backend = Arc::new(MockWalletBackend::new("sc1mock"));
        assert!(backend
            .submit_batch(&[])
            .unwrap_err()
            .to_string()
            .contains("AI-P04"));
    }
}
