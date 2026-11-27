//! Strangler migration from src/gui/ to strangecoin-ui
//!
//! UI-P05 closeout: the legacy renderer, phase switch and legacy-only handlers
//! are gone. This module is now the single professional UI launcher; every
//! screen pulls its data from an in-process adapter (`src/api/`), and the
//! launcher only owns auth state, navigation and the event/mining bridges.

use eframe::egui;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::SystemTime;
use tracing::{debug, error, info, warn};

use crate::api::{
    InProcessAgentActivityBackend, InProcessDashboardBackend, InProcessSettingsBackend,
    InProcessTxDetailBackend, InProcessWalletBackend,
};
use crate::error::StrangecoinError;
use crate::events::NodeEvent;
use crate::{wallet, MiningStatus, MiningTask, Node};

use strangecoin_ui::theme::Theme;
use strangecoin_ui::{
    AgentActivityBackend, AgentActivityScreen, DashboardBackend, DashboardScreen, Localization,
    SettingsBackend, SettingsScreen, ThemeManager, TxDetailBackend, TxDetailScreen, WalletBackend,
    WalletScreen,
};

/// Strangler migration launcher for UI
pub struct StranglerLauncher {
    pub node: Node,
    pub wallet_address: String,
    pub password: String,
    pub is_authenticated: bool,
    pub data_dir: PathBuf,
    pub theme_manager: ThemeManager,
    pub localization: Localization,
    pub current_screen: ScreenType,
    pub wallet_screen: WalletScreen,
    pub dashboard_screen: DashboardScreen,
    pub tx_detail_screen: TxDetailScreen,
    pub agent_activity_screen: AgentActivityScreen,
    pub settings_screen: SettingsScreen,
    pub mining_status: Arc<Mutex<MiningStatus>>,
    pub mining_progress: Arc<Mutex<Option<String>>>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub status_rx: Option<mpsc::Receiver<String>>,
    pub mining_tx: tokio::sync::mpsc::UnboundedSender<MiningTask>,
    pub last_repaint: f64,
    pub new_wallet_password: String,
    /// UI-P03: in-process data source behind `WalletBackend` (src/api/).
    pub wallet_backend: Arc<InProcessWalletBackend>,
    /// UI-P04: in-process data sources for the dashboard / tx audit screens.
    pub dashboard_backend: Arc<InProcessDashboardBackend>,
    pub tx_detail_backend: Arc<InProcessTxDetailBackend>,
    /// UI-P05: in-process data sources for the agent activity / settings screens.
    pub agent_activity_backend: Arc<InProcessAgentActivityBackend>,
    pub settings_backend: Arc<InProcessSettingsBackend>,
    /// UI-P04: `NodeEvent` subscription owned by the launcher; drained once
    /// per frame into the dashboard digest (never consumed in the UI crate).
    pub event_rx: Option<crossbeam_channel::Receiver<NodeEvent>>,
}

/// Screen types for the new UI
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScreenType {
    Wallet,
    Dashboard,
    TxDetail,
    AgentActivity,
    Settings,
}

impl StranglerLauncher {
    /// Create a new strangler launcher (standalone entry point).
    pub fn new(node: Node, data_dir: PathBuf) -> Self {
        let (mining_tx, _unused_rx) = tokio::sync::mpsc::unbounded_channel();
        Self::with_ui_config(
            node,
            data_dir,
            &crate::config::UiConfig::default(),
            mining_tx,
        )
    }

    pub fn with_ui_config(
        node: Node,
        data_dir: PathBuf,
        ui: &crate::config::UiConfig,
        mining_tx: tokio::sync::mpsc::UnboundedSender<MiningTask>,
    ) -> Self {
        let config_path = std::env::current_dir()
            .unwrap_or_default()
            .join("config.toml");
        let theme = Theme::from_config_str(&ui.theme).unwrap_or_default();
        let theme_manager = ThemeManager::with_theme(config_path.clone(), theme);
        let localization = Localization::with_locale_and_root(
            &ui.locale,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        )
        .unwrap_or_else(|_| Localization::new());

        let mining_status = Arc::new(Mutex::new(MiningStatus::Idle));
        let wallet_backend = InProcessWalletBackend::new(
            Arc::clone(&node.blockchain),
            node.network_id,
            data_dir.clone(),
        );

        // UI-P03: wire the real mining sender. The mining worker tolerates
        // dropped progress/status receivers (all sends are best-effort).
        let hook_blockchain = Arc::clone(&node.blockchain);
        let hook_rate_limiter = Arc::clone(&node.rate_limiter);
        let hook_shutdown = Arc::clone(&node.shutdown);
        let hook_event_bus = Arc::clone(&node.event_bus);
        let hook_inbox = node.inbox.clone();
        let hook_mining_status = Arc::clone(&mining_status);
        let hook_tx = mining_tx.clone();
        let wallet_backend = wallet_backend.with_submit_hook(Arc::new(move |transaction| {
            let (progress_tx, _progress_rx) = mpsc::channel();
            let (status_tx, _status_rx) = mpsc::channel();
            let _ = hook_tx.send(MiningTask {
                blockchain: Arc::clone(&hook_blockchain),
                transaction: transaction.clone(),
                mining_status: Arc::clone(&hook_mining_status),
                progress_tx,
                status_tx,
                rate_limiter: Arc::clone(&hook_rate_limiter),
                shutdown: Arc::clone(&hook_shutdown),
                event_bus: Arc::clone(&hook_event_bus),
                inbox: hook_inbox.clone(),
            });
            if let Ok(mut guard) = hook_mining_status.lock() {
                *guard = MiningStatus::Mining;
            }
        }));

        let wallet_backend = Arc::new(wallet_backend);
        let wallet_screen =
            WalletScreen::with_backend(wallet_backend.clone() as Arc<dyn WalletBackend>);

        // UI-P04: the screen layer stays headless — both new screens receive
        // in-process adapters, and the launcher owns the EventBus receiver.
        let dashboard_backend = Arc::new(InProcessDashboardBackend::new(
            Arc::clone(&node.blockchain),
            Arc::clone(&node.peer_manager),
        ));
        let tx_detail_backend =
            Arc::new(InProcessTxDetailBackend::new(Arc::clone(&node.blockchain)));
        let dashboard_screen =
            DashboardScreen::with_backend(dashboard_backend.clone() as Arc<dyn DashboardBackend>);
        let tx_detail_screen =
            TxDetailScreen::with_backend(tx_detail_backend.clone() as Arc<dyn TxDetailBackend>);
        let event_rx = Some(node.event_bus.subscribe());

        // UI-P05: agent activity and settings are backed by real node state
        // (chain/mempool and loaded config/peer manager respectively). Config
        // load is best-effort: a missing or invalid config.toml leaves the
        // adapter in its fail-closed "unavailable" state.
        let loaded_config = crate::config::Config::load(&config_path).ok();
        let agent_activity_backend = Arc::new(InProcessAgentActivityBackend::new(Arc::clone(
            &node.blockchain,
        )));
        let agent_activity_screen = AgentActivityScreen::with_backend(
            agent_activity_backend.clone() as Arc<dyn AgentActivityBackend>,
        );
        let settings_backend = Arc::new(InProcessSettingsBackend::new(
            loaded_config,
            config_path.clone(),
            Arc::clone(&node.peer_manager),
            data_dir.clone(),
            node.network_id,
        ));
        let settings_screen =
            SettingsScreen::with_backend(settings_backend.clone() as Arc<dyn SettingsBackend>);

        Self {
            node,
            wallet_address: String::new(),
            password: String::new(),
            is_authenticated: false,
            data_dir,
            theme_manager,
            localization,
            current_screen: ScreenType::Wallet,
            wallet_screen,
            dashboard_screen,
            tx_detail_screen,
            agent_activity_screen,
            settings_screen,
            mining_status,
            mining_progress: Arc::new(Mutex::new(None)),
            progress_rx: None,
            status_rx: None,
            mining_tx,
            last_repaint: 0.0,
            new_wallet_password: String::new(),
            wallet_backend,
            dashboard_backend,
            tx_detail_backend,
            agent_activity_backend,
            settings_backend,
            event_rx,
        }
    }

    /// UI-P04: fold the EventBus stream into the dashboard digest once per
    /// frame. Bounded to avoid starving the render loop on a burst; leftover
    /// events are picked up on the next repaint.
    fn drain_node_events(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.event_rx else {
            return;
        };
        let mut seen = 0usize;
        while seen < 64 {
            match receiver.try_recv() {
                Ok(event) => {
                    self.dashboard_backend.record_event(&event);
                    seen += 1;
                }
                Err(crossbeam_channel::TryRecvError::Empty)
                | Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }
        if seen > 0 {
            debug!(events = seen, "dashboard consumed node events");
            ctx.request_repaint();
        }
    }

    /// Open a selected txid in Tx Detail. Shared by the wallet History and
    /// Agent Activity navigation seams.
    fn open_tx_detail(&mut self, txid: &str) {
        self.tx_detail_screen.set_transaction_id(txid);
        self.current_screen = ScreenType::TxDetail;
    }

    /// Show professional UI (new implementation)
    fn show_professional_ui(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_node_events(ctx);

        // Wallet unlock stays in the launcher until a dedicated Settings/auth
        // surface owns keystore access (ADR-0028 key isolation).
        if !self.is_authenticated {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Аутентификация");
                ui.label(self.wallet_screen.status());
                ui.label("Адрес кошелька (публичный ключ):");
                ui.text_edit_singleline(&mut self.wallet_address);
                ui.label("Пароль:");
                ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
                ui.horizontal(|ui| {
                    if ui.button("Войти").clicked() {
                        self.handle_login();
                    }
                    if ui.button("Регистрация").clicked() {
                        self.handle_registration();
                    }
                });
                ui.label("Пароль для нового кошелька:");
                ui.add(egui::TextEdit::singleline(&mut self.new_wallet_password).password(true));
            });
            return;
        }

        // Update the current screen with blockchain data
        self.update_screens_with_blockchain_data();

        // Show navigation header
        self.show_navigation_header(ctx);

        // Show current screen
        match self.current_screen {
            ScreenType::Wallet => {
                self.wallet_screen
                    .render(ctx, &self.theme_manager, &self.localization);
            }
            ScreenType::Dashboard => {
                self.dashboard_screen
                    .render(ctx, &self.theme_manager, &self.localization);
            }
            ScreenType::TxDetail => {
                self.tx_detail_screen
                    .render(ctx, &self.theme_manager, &self.localization);
            }
            ScreenType::AgentActivity => {
                self.agent_activity_screen
                    .render(ctx, &self.theme_manager, &self.localization);
            }
            ScreenType::Settings => {
                self.settings_screen
                    .render(ctx, &self.theme_manager, &self.localization);
            }
        }

        // UI-P03 → UI-P04 wire: a History row click selects a txid; the
        // launcher opens Tx Detail (implemented by UI-P04).
        if let Some(txid) = self.wallet_screen.take_selected_transaction() {
            self.open_tx_detail(&txid);
            info!(txid = %txid, "wallet history row opened transaction detail");
        }

        // UI-P05 wire: an Agent Activity row click selects a txid; the ACP
        // detail modal is owned by the screen itself and never navigates here.
        if let Some(txid) = self.agent_activity_screen.take_selected_transaction() {
            self.open_tx_detail(&txid);
            info!(txid = %txid, "agent activity row opened transaction detail");
        }
    }

    /// Show navigation header for professional UI
    fn show_navigation_header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("navigation").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 20.0;

                // Navigation buttons
                if ui.button("👛 Wallet").clicked() {
                    self.current_screen = ScreenType::Wallet;
                }
                if ui.button("📊 Dashboard").clicked() {
                    self.current_screen = ScreenType::Dashboard;
                }
                if ui.button("📋 Transactions").clicked() {
                    self.current_screen = ScreenType::TxDetail;
                }
                if ui.button("🤖 Agents").clicked() {
                    self.current_screen = ScreenType::AgentActivity;
                }
                if ui.button("⚙️ Settings").clicked() {
                    self.current_screen = ScreenType::Settings;
                }

                // Theme switcher
                ui.separator();
                if ui.button("🌙 Dark").clicked() {
                    self.theme_manager.set_dark_mode(true);
                }
                if ui.button("☀️ Light").clicked() {
                    self.theme_manager.set_dark_mode(false);
                }

                // Language switcher
                ui.separator();
                if ui.button("🇺🇸 EN").clicked() {
                    let _ = self.localization.set_language("en");
                }
                if ui.button("🇷🇺 RU").clicked() {
                    let _ = self.localization.set_language("ru");
                }
                if ui.button("🇨🇳 ZH").clicked() {
                    let _ = self.localization.set_language("zh");
                }
            });
        });
    }

    /// Update all screens with current blockchain data
    fn update_screens_with_blockchain_data(&mut self) {
        // Update wallet screen with current balance and transactions
        let balance = self.node.blockchain.get_balance(&self.wallet_address);
        self.wallet_screen.set_balance(balance);

        // UI-P04: the dashboard pulls its own snapshot from the in-process
        // adapter (real tip height, peers, mempool and blocks). The legacy
        // `block_count`/`mempool_count` setters remain only as compat bridges
        // for older call sites and tests; they must not override the adapter.

        // Update other screens as needed
    }

    /// Handle login functionality (professional auth gate)
    fn handle_login(&mut self) {
        let start_time = SystemTime::now();
        info!(wallet_address = %self.wallet_address, "Кнопка 'Войти' нажата");

        match wallet::Wallet::list_keystores(&self.data_dir) {
            Ok(keystores) => {
                // Keystore filenames come from the base64 public key, not the
                // bech32 address (src/wallet.rs) — resolve by loaded identity.
                let mut matched = false;
                let mut load_error: Option<String> = None;
                for path in &keystores {
                    match wallet::Wallet::load(&self.password, path) {
                        Ok(loaded) => {
                            let expected = crate::address::encode_address(
                                &loaded.public_key,
                                self.node.network_id,
                            )
                            .unwrap_or_default();
                            if expected == self.wallet_address {
                                matched = true;
                                break;
                            }
                        }
                        Err(err) => load_error = Some(err.to_string()),
                    }
                }
                if matched {
                    self.is_authenticated = true;
                    if let Ok(mut session) = self.wallet_backend.session().lock() {
                        session.address = self.wallet_address.clone();
                        session.password = self.password.clone();
                    }
                    self.wallet_screen
                        .set_status("Успешная аутентификация".to_string());
                    self.node.discover_peers();
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    info!(duration_secs = duration, "Аутентификация успешна");
                } else {
                    let status = load_error
                        .map(|err| format!("Ошибка аутентификации: {err}"))
                        .unwrap_or_else(|| "Кошелёк не найден".to_string());
                    self.wallet_screen.set_status(status);
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    warn!(duration_secs = duration, "Аутентификация не удалась");
                }
            }
            Err(e) => {
                self.wallet_screen
                    .set_status(format!("Ошибка поиска кошельков: {}", e));
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                error!(duration_secs = duration, error = %e, "Ошибка поиска кошельков");
            }
        }
    }

    /// Handle registration functionality (professional auth gate)
    fn handle_registration(&mut self) {
        let start_time = SystemTime::now();
        info!("Кнопка 'Регистрация' нажата");

        let password = if self.new_wallet_password.is_empty() {
            wallet::Wallet::get_password_from_env()
        } else {
            Some(self.new_wallet_password.clone())
        };
        let password = match password {
            Some(p) => p,
            None => {
                self.wallet_screen.set_status("Пароль для регистрации не может быть пустым (введите в поле или задайте STRANGECOIN_WALLET_PASSWORD)".to_string());
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                warn!(
                    duration_secs = duration,
                    "Ошибка регистрации: пустой пароль"
                );
                return;
            }
        };

        match wallet::Wallet::new(&password, &self.data_dir) {
            Ok(wallet) => {
                self.wallet_address =
                    crate::address::encode_address(&wallet.public_key, self.node.network_id)
                        .unwrap_or_else(|_| "invalid_address".to_string());
                if let Ok(mut session) = self.wallet_backend.session().lock() {
                    session.address = self.wallet_address.clone();
                    session.password = password.clone();
                }
                self.password = password;
                self.is_authenticated = true;
                self.wallet_screen
                    .set_status(format!("Кошелёк успешно создан: {}", self.wallet_address));
                self.node.discover_peers();
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                info!(duration_secs = duration, wallet_address = %self.wallet_address, "Регистрация успешна");

                if !self.node.blockchain.has_account(&self.wallet_address) {
                    match self
                        .node
                        .blockchain
                        .grant_initial_balance_to_first_wallet(&self.wallet_address)
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            self.node.blockchain.ensure_account(&self.wallet_address);
                        }
                        Err(StrangecoinError::GrantBlocksDisabled) => {
                            warn!("Grant blocks disabled, creating zero-balance entry");
                            self.node.blockchain.ensure_account(&self.wallet_address);
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to grant initial balance");
                            self.node.blockchain.ensure_account(&self.wallet_address);
                        }
                    }
                    self.node.blockchain.save_state();
                }
            }
            Err(e) => {
                self.wallet_screen
                    .set_status(format!("Ошибка регистрации: {}", e));
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                error!(duration_secs = duration, error = %e, "Ошибка регистрации");
            }
        }
    }
}

impl eframe::App for StranglerLauncher {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if now - self.last_repaint > 0.01 {
            ctx.request_repaint();
            self.last_repaint = now;
        }

        // Handle sync and status updates
        while let Ok(received_snapshot) = self.node.sync_rx.try_recv() {
            if received_snapshot.chain.len() <= 1 {
                debug!("Получена пустая или минимальная цепочка через sync_rx, игнорируем");
                continue;
            }
            let wallet = self.wallet_address.clone();
            let old_balance = self.node.blockchain.get_balance(&wallet);
            let adopted = self
                .node
                .blockchain
                .adopt_wire(received_snapshot)
                .unwrap_or(false);
            if adopted {
                info!("UI: Блокчейн обновлён через канал синхронизации");
                let new_balance = self.node.blockchain.get_balance(&wallet);
                if old_balance != new_balance {
                    info!(wallet = %wallet, old_balance = old_balance, new_balance = new_balance, "Баланс кошелька изменился, запрашивается перерисовка");
                    ctx.request_repaint();
                } else {
                    debug!(wallet = %wallet, balance = old_balance, "Баланс кошелька не изменился, перерисовка не требуется");
                }
            } else {
                debug!("Полученный блокчейн через канал синхронизации не длиннее или не прошёл валидацию");
            }
        }

        if let Some(ref status_rx) = self.status_rx {
            while let Ok(status) = status_rx.try_recv() {
                self.wallet_screen.set_status(status.clone());
                self.wallet_screen.refresh();
                if status.starts_with("Транзакция отправлена") {
                    if let Ok(mut mining_status) = self.mining_status.lock() {
                        *mining_status = MiningStatus::Idle;
                        debug!("Статус майнинга сброшен на Idle");
                    }
                    self.progress_rx = None;
                }
                debug!(status = %status, "Получено обновление статуса");
                ctx.request_repaint();
            }
        }

        // UI-P05: professional UI only — no legacy phase switch remains.
        self.show_professional_ui(ctx, frame);
    }
}

/// Run the strangler launcher
pub async fn run_strangler_launcher(app: StranglerLauncher) {
    tokio::task::spawn_blocking(move || {
        eframe::run_native(
            "Strangecoin Wallet",
            eframe::NativeOptions::default(),
            Box::new(|_cc| Ok(Box::new(app))),
        )
        .expect("Ошибка запуска приложения");
    })
    .await
    .expect("GUI task panicked");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{create_node_for_test, create_test_blockchain, random_port, TestDir};

    /// Build a throwaway node on a temp RocksDB dir.
    ///
    /// The `TestDir` must outlive the node (it owns the DB files), so both
    /// are returned; drop order inside the test keeps the DB closed first.
    fn make_node(tag: &str) -> (TestDir, Node) {
        let dir = TestDir::new(tag);
        let bc = Arc::new(create_test_blockchain(dir.path()));
        let (node, _peers) = create_node_for_test(&bc, random_port());
        // TestDir drops last (reverse declaration order), after the node
        // closes the DB handles.
        (dir, node)
    }

    #[test]
    fn test_strangler_launcher_creation() {
        let (_dir, node) = make_node("strangler_create");
        let launcher = StranglerLauncher::new(node, PathBuf::from("./test_data"));

        assert_eq!(launcher.current_screen, ScreenType::Wallet);
        assert!(!launcher.is_authenticated);
        assert_eq!(launcher.wallet_address, "");
        assert_eq!(launcher.password, "");
    }

    #[test]
    fn test_ui_p05_backends_are_wired_into_screens() {
        let (_dir, node) = make_node("strangler_ui_p05");
        let launcher = StranglerLauncher::new(node, PathBuf::from("./test_data"));

        // Both new screens expose exactly the data their in-process adapters
        // produce (no mock fallback survives the closeout).
        assert_eq!(
            launcher.agent_activity_screen.activities(),
            launcher
                .agent_activity_backend
                .activities()
                .unwrap()
                .as_slice()
        );
        assert!(launcher
            .agent_activity_screen
            .activities()
            .iter()
            .any(|row| !row.available && row.txid.is_none()));
        assert_eq!(
            launcher.settings_screen.snapshot(),
            &launcher.settings_backend.snapshot()
        );
    }

    #[test]
    fn test_screen_type_navigation() {
        let (_dir, node) = make_node("strangler_nav");
        let mut launcher = StranglerLauncher::new(node, PathBuf::from("./test_data"));

        assert_eq!(launcher.current_screen, ScreenType::Wallet);

        launcher.current_screen = ScreenType::Dashboard;
        assert_eq!(launcher.current_screen, ScreenType::Dashboard);

        launcher.current_screen = ScreenType::TxDetail;
        assert_eq!(launcher.current_screen, ScreenType::TxDetail);

        launcher.current_screen = ScreenType::AgentActivity;
        assert_eq!(launcher.current_screen, ScreenType::AgentActivity);

        launcher.current_screen = ScreenType::Settings;
        assert_eq!(launcher.current_screen, ScreenType::Settings);
    }

    #[test]
    fn test_strangler_launcher_bridge_methods() {
        let (_dir, node) = make_node("strangler_bridge");
        let mut launcher = StranglerLauncher::new(node, PathBuf::from("./test_data"));
        let temp_config = std::env::temp_dir().join(format!(
            "strangecoin-ui-launcher-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_config);
        launcher.theme_manager = ThemeManager::with_config_path(temp_config.clone());

        // Theme bridge
        launcher.theme_manager.set_dark_mode(true);
        assert_eq!(
            launcher.theme_manager.get_theme(),
            strangecoin_ui::theme::Theme::Dark
        );
        launcher.theme_manager.set_dark_mode(false);
        assert_eq!(
            launcher.theme_manager.get_theme(),
            strangecoin_ui::theme::Theme::Light
        );

        // Localization bridge
        assert_eq!(launcher.localization.current_locale(), "en");
        launcher.localization.set_language("ru").unwrap();
        assert_eq!(launcher.localization.current_locale(), "ru");

        // Wallet screen bridge
        launcher.wallet_screen.set_balance(1000);
        assert_eq!(launcher.wallet_screen.send_tab.balance, 1000);
        launcher.wallet_screen.set_status("Test status".to_string());
        assert_eq!(launcher.wallet_screen.status(), "Test status");

        // Dashboard screen bridge
        launcher.dashboard_screen.set_block_count(100);
        assert_eq!(launcher.dashboard_screen.block_count, 100);
        launcher.dashboard_screen.set_mempool_count(50);
        assert_eq!(launcher.dashboard_screen.mempool_count, 50);

        // Screen navigation
        launcher.current_screen = ScreenType::Dashboard;
        assert_eq!(launcher.current_screen, ScreenType::Dashboard);
        launcher.current_screen = ScreenType::Wallet;
        assert_eq!(launcher.current_screen, ScreenType::Wallet);
        let _ = std::fs::remove_file(&temp_config);
    }

    #[test]
    fn test_update_screens_with_blockchain_data() {
        let (_dir, node) = make_node("strangler_update");
        let mut launcher = StranglerLauncher::new(node, PathBuf::from("./test_data"));

        launcher.update_screens_with_blockchain_data();

        // Fresh regtest chain: zero balance, empty mempool, genesis-only chain.
        // UI-P04: the dashboard reads real state through its adapter, so the
        // assertion targets the snapshot rather than legacy compat counters.
        assert_eq!(launcher.wallet_screen.send_tab.balance, 0);
        let snapshot = launcher.dashboard_screen.snapshot();
        assert_eq!(snapshot.mempool_count, 0);
        assert!(!snapshot.blocks.is_empty());
        assert_eq!(snapshot.sync.current_height, snapshot.blocks[0].height);
    }

    #[test]
    fn test_screen_type_equality() {
        assert_eq!(ScreenType::Wallet, ScreenType::Wallet);
        assert_eq!(ScreenType::Dashboard, ScreenType::Dashboard);
        assert_eq!(ScreenType::TxDetail, ScreenType::TxDetail);
        assert_eq!(ScreenType::AgentActivity, ScreenType::AgentActivity);
        assert_eq!(ScreenType::Settings, ScreenType::Settings);

        assert_ne!(ScreenType::Wallet, ScreenType::Dashboard);
        assert_ne!(ScreenType::Dashboard, ScreenType::TxDetail);
        assert_ne!(ScreenType::TxDetail, ScreenType::AgentActivity);
        assert_ne!(ScreenType::AgentActivity, ScreenType::Settings);
        assert_ne!(ScreenType::Settings, ScreenType::Wallet);
    }

    #[test]
    fn test_agent_activity_nav_seam() {
        let (_dir, node) = make_node("strangler_activity_nav");
        let mut launcher = StranglerLauncher::new(node, PathBuf::from("./test_data"));

        assert_eq!(launcher.current_screen, ScreenType::Wallet);
        assert_eq!(
            launcher.agent_activity_screen.take_selected_transaction(),
            None
        );

        // Shared launcher seam: both History and Agent Activity selections
        // open Tx Detail with the chosen txid.
        launcher.open_tx_detail("0xabc");
        assert_eq!(launcher.current_screen, ScreenType::TxDetail);
    }
}
