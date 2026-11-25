//! Strangler migration from src/gui/ to strangecoin-ui
//!
//! This module implements the strangler pattern migration from the old GUI
//! implementation in src/gui/ to the new professional UI in crates/strangecoin-ui.
//!
//! The migration strategy:
//! 1. Create a thin launcher that uses both old and new UI
//! 2. Gradually migrate functionality from old to new UI
//! 3. Eventually remove old UI completely
//!
//! This allows for a gradual migration without breaking existing functionality.

use eframe::egui;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::SystemTime;
use tracing::{debug, error, info, warn};

use crate::error::StrangecoinError;
use crate::{wallet, MiningStatus, MiningTask, Node, Transaction};

use strangecoin_ui::theme::Theme;
use strangecoin_ui::{
    AgentActivityScreen, DashboardScreen, Localization, SettingsScreen, ThemeManager,
    TxDetailScreen, WalletScreen,
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
    pub migration_phase: MigrationPhase,
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

/// Migration phases
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MigrationPhase {
    Phase1_LegacyOnly,
    Phase2_Hybrid,
    Phase3_ProfessionalOnly,
}

impl StranglerLauncher {
    /// Create a new strangler launcher
    pub fn new(node: Node, data_dir: PathBuf) -> Self {
        Self::with_ui_config(node, data_dir, &crate::config::UiConfig::default())
    }

    pub fn with_ui_config(node: Node, data_dir: PathBuf, ui: &crate::config::UiConfig) -> Self {
        let config_path = std::env::current_dir()
            .unwrap_or_default()
            .join("config.toml");
        let theme = Theme::from_config_str(&ui.theme).unwrap_or_default();
        let theme_manager = ThemeManager::with_theme(config_path, theme);
        let localization = Localization::with_locale_and_root(
            &ui.locale,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        )
        .unwrap_or_else(|_| Localization::new());

        Self {
            node,
            wallet_address: String::new(),
            password: String::new(),
            is_authenticated: false,
            data_dir,
            theme_manager,
            localization,
            current_screen: ScreenType::Wallet,
            wallet_screen: WalletScreen::new(),
            dashboard_screen: DashboardScreen::new(),
            tx_detail_screen: TxDetailScreen::new(),
            agent_activity_screen: AgentActivityScreen::new(),
            settings_screen: SettingsScreen::new(),
            mining_status: Arc::new(Mutex::new(MiningStatus::Idle)),
            mining_progress: Arc::new(Mutex::new(None)),
            progress_rx: None,
            status_rx: None,
            mining_tx: tokio::sync::mpsc::unbounded_channel().0,
            last_repaint: 0.0,
            new_wallet_password: String::new(),
            migration_phase: MigrationPhase::Phase1_LegacyOnly,
        }
    }

    /// Set the migration phase
    pub fn set_migration_phase(&mut self, phase: MigrationPhase) {
        self.migration_phase = phase;
        info!("Migration phase set to: {:?}", phase);
    }

    /// Show legacy UI (old implementation)
    fn show_legacy_ui(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // This is the old GUI implementation from src/gui/mod.rs
        // We keep it for backward compatibility during migration

        let now = ctx.input(|i| i.time);
        if now - self.last_repaint > 0.01 {
            ctx.request_repaint();
            self.last_repaint = now;
        }

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
                // Update status in new UI as well
                self.wallet_screen.set_status(status.clone());
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

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(&self.wallet_screen.status());
            ui.label(format!(
                "Количество транзакций в базе данных: {}",
                self.node.blockchain.mempool_len()
            ));
            if !self.is_authenticated {
                ui.heading("Аутентификация");
                ui.label("Адрес кошелька (публичный ключ):");
                ui.text_edit_singleline(&mut self.wallet_address);
                ui.label("Пароль:");
                ui.text_edit_singleline(&mut self.password);
                ui.horizontal(|ui| {
                    if ui.button("Войти").clicked() {
                        self.handle_login();
                        ctx.request_repaint();
                    }
                    if ui.button("Регистрация").clicked() {
                        self.handle_registration();
                        ctx.request_repaint();
                    }
                });
                ui.label("Пароль для нового кошелька:");
                ui.text_edit_singleline(&mut self.new_wallet_password);
            } else {
                ui.heading("Кошелёк");
                ui.label(format!("Адрес: {}", self.wallet_address));
                let balance = self.node.blockchain.get_balance(&self.wallet_address);
                ui.label(format!("Баланс: {}", balance));

                ui.heading("Перевод");
                ui.text_edit_singleline(self.wallet_screen.receiver_address_mut());
                ui.text_edit_singleline(self.wallet_screen.amount_mut());

                if let Some(ref progress_rx) = self.progress_rx {
                    while let Ok(progress) = progress_rx.try_recv() {
                        let mut mining_progress = self
                            .mining_progress
                            .lock()
                            .expect("Не удалось захватить Mutex для mining_progress");
                        *mining_progress = Some(progress);
                        debug!(?mining_progress, "Прогресс майнинга обновлён в UI");
                    }
                }

                let is_mining = matches!(*self.mining_status.lock().unwrap(), MiningStatus::Mining);

                if is_mining {
                    ui.label("Майнинг блока в процессе...");
                    let progress = self
                        .mining_progress
                        .lock()
                        .expect("Не удалось захватить Mutex для mining_progress");
                    if let Some(progress_msg) = &*progress {
                        ui.label(format!("Прогресс: {}", progress_msg));
                    }
                    ui.spinner();
                    ctx.request_repaint();
                } else if ui.button("Отправить").clicked() {
                    self.handle_send_transaction();
                    ctx.request_repaint();
                }

                ui.heading("Поиск кошелька по IP и порту");
                ui.horizontal(|ui| {
                    ui.label("IP: ");
                    ui.text_edit_singleline(self.wallet_screen.ip_mut());
                    ui.label("Порт: ");
                    ui.add(
                        egui::TextEdit::singleline(self.wallet_screen.port_mut())
                            .desired_width(50.0),
                    );
                });
                if ui.button("Найти кошелёк").clicked() {
                    self.handle_find_wallet();
                    ctx.request_repaint();
                }
            }
        });
    }

    /// Show professional UI (new implementation)
    fn show_professional_ui(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                    self.localization.set_language("en");
                }
                if ui.button("🇷🇺 RU").clicked() {
                    self.localization.set_language("ru");
                }
                if ui.button("🇨🇳 ZH").clicked() {
                    self.localization.set_language("zh");
                }
            });
        });
    }

    /// Update all screens with current blockchain data
    fn update_screens_with_blockchain_data(&mut self) {
        // Update wallet screen with current balance and transactions
        let balance = self.node.blockchain.get_balance(&self.wallet_address);
        self.wallet_screen.set_balance(balance);

        // Update dashboard with statistics
        let block_count = self.node.blockchain.chain_len();
        let mempool_count = self.node.blockchain.mempool_len();
        self.dashboard_screen.set_block_count(block_count);
        self.dashboard_screen.set_mempool_count(mempool_count);

        // Update other screens as needed
    }

    /// Handle login functionality
    fn handle_login(&mut self) {
        let start_time = SystemTime::now();
        info!(wallet_address = %self.wallet_address, "Кнопка 'Войти' нажата");

        match wallet::Wallet::list_keystores(&self.data_dir) {
            Ok(keystores) => {
                let sanitized_address = self
                    .wallet_address
                    .replace("/", "_")
                    .replace("+", "_")
                    .replace("=", "_");
                let keystore_path = keystores.iter().find(|p| {
                    p.file_name().and_then(|n| n.to_str())
                        == Some(&format!("wallet_{}.json", sanitized_address))
                });
                let keystore_path = match keystore_path {
                    Some(p) => p,
                    None => {
                        self.wallet_screen
                            .set_status("Кошелёк не найден".to_string());
                        return;
                    }
                };
                match wallet::Wallet::load(&self.password, keystore_path) {
                    Ok(wallet) => {
                        let expected_address = crate::address::encode_address(
                            &wallet.public_key,
                            self.node.network_id,
                        )
                        .unwrap_or_else(|_| "invalid_address".to_string());
                        if expected_address == self.wallet_address {
                            self.is_authenticated = true;
                            self.wallet_screen
                                .set_status("Успешная аутентификация".to_string());
                            self.node.discover_peers();
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            info!(duration_secs = duration, "Аутентификация успешна");
                        } else {
                            self.wallet_screen
                                .set_status("Неверный адрес кошелька".to_string());
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            warn!(
                                duration_secs = duration,
                                "Аутентификация не удалась: неверный адрес кошелька"
                            );
                        }
                    }
                    Err(e) => {
                        self.wallet_screen
                            .set_status(format!("Ошибка аутентификации: {}", e));
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        error!(duration_secs = duration, error = %e, "Аутентификация не удалась");
                    }
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

    /// Handle registration functionality
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

    /// Handle send transaction functionality
    fn handle_send_transaction(&mut self) {
        let start_time = SystemTime::now();
        info!(receiver = %self.wallet_screen.receiver_address(), amount = %self.wallet_screen.amount(), "Кнопка 'Отправить' нажата");

        if self.wallet_screen.receiver_address().trim().is_empty() {
            self.wallet_screen
                .set_status("Адрес получателя не может быть пустым".to_string());
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            warn!(duration_secs = duration, "Пустой адрес получателя");
            return;
        }

        if let Ok(amount) = self.wallet_screen.amount().trim().parse::<u64>() {
            if amount == 0 {
                self.wallet_screen
                    .set_status("Сумма должна быть больше нуля".to_string());
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                warn!(duration_secs = duration, "Сумма равна нулю");
                return;
            }

            let sender_nonce = self.node.blockchain.get_nonce(&self.wallet_address);
            let mut transaction = Transaction::transfer(
                &self.wallet_address,
                self.wallet_screen.receiver_address().trim(),
                amount,
                sender_nonce + 1,
                self.node.network_id,
            );

            let sanitized_address = self
                .wallet_address
                .replace("/", "_")
                .replace("+", "_")
                .replace("=", "_");
            let keystore_path = self
                .data_dir
                .join("keystore")
                .join(format!("wallet_{}.json", sanitized_address));
            let wallet = match wallet::Wallet::load(&self.password, &keystore_path) {
                Ok(w) => w,
                Err(e) => {
                    self.wallet_screen
                        .set_status(format!("Ошибка загрузки кошелька: {}", e));
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    error!(duration_secs = duration, error = %e, "Ошибка загрузки кошелька");
                    return;
                }
            };

            if let Err(e) = wallet.sign_transaction(&mut transaction) {
                self.wallet_screen
                    .set_status(format!("Ошибка подписи транзакции: {}", e));
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                error!(duration_secs = duration, error = %e, "Ошибка подписи транзакции");
                return;
            }

            let blockchain = Arc::clone(&self.node.blockchain);
            let mining_status = Arc::clone(&self.mining_status);

            {
                debug!(?transaction, "Транзакция для добавления");
                if blockchain.apply_tx(transaction.clone()).is_err() {
                    self.wallet_screen
                        .set_status("Недостаточно средств или неверный адрес".to_string());
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    warn!(
                        duration_secs = duration,
                        "Недостаточно средств или неверный адрес"
                    );
                    return;
                }
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                info!(duration_secs = duration, "Транзакция успешно добавлена");
            }

            self.wallet_screen
                .set_status("Запуск майнинга...".to_string());
            info!("Подготовка к отправке задачи майнинга");

            let (progress_tx, progress_rx) = mpsc::channel();
            let (status_tx, status_rx) = mpsc::channel();

            {
                let mut mining_progress = self
                    .mining_progress
                    .lock()
                    .expect("Не удалось захватить Mutex для mining_progress");
                *mining_progress = None;
                self.progress_rx = Some(progress_rx);
                self.status_rx = Some(status_rx);
                debug!("Каналы прогресса и статуса созданы");
            }

            if let Ok(mut mining_status) = self.mining_status.lock() {
                *mining_status = MiningStatus::Mining;
                info!("Статус майнинга установлен: Mining");
            } else {
                self.wallet_screen
                    .set_status("Ошибка: Не удалось установить статус майнинга".to_string());
                error!("Не удалось установить статус майнинга");
                self.progress_rx = None;
                self.status_rx = None;
                return;
            }

            info!("Попытка отправки задачи майнинга");
            if let Err(e) = self.mining_tx.send(MiningTask {
                blockchain,
                transaction,
                mining_status,
                progress_tx,
                status_tx,
                rate_limiter: self.node.rate_limiter.clone(),
                shutdown: self.node.shutdown.clone(),
                event_bus: Arc::clone(&self.node.event_bus),
                inbox: self.node.inbox.clone(),
            }) {
                self.wallet_screen
                    .set_status(format!("Ошибка отправки задачи майнинга: {}", e));
                error!(error = %e, "Ошибка отправки задачи майнинга");
                if let Ok(mut mining_status) = self.mining_status.lock() {
                    *mining_status = MiningStatus::Idle;
                    debug!("Статус майнинга сброшен на Idle");
                }
                self.progress_rx = None;
                self.status_rx = None;
                return;
            }

            info!("Задача майнинга успешно отправлена");
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            info!(duration_secs = duration, "Запуск майнинга завершён");
        } else {
            self.wallet_screen
                .set_status("Неверный формат суммы".to_string());
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            warn!(duration_secs = duration, port = %self.wallet_screen.port(), "Неверный формат суммы");
        }
    }

    /// Handle find wallet functionality
    fn handle_find_wallet(&mut self) {
        let start_time = SystemTime::now();
        info!(ip = %self.wallet_screen.ip(), port = %self.wallet_screen.port(), "Кнопка 'Найти кошелёк' нажата");

        let ip_buf = self.wallet_screen.ip().to_string();
        let port_buf = self.wallet_screen.port().to_string();
        let ip = ip_buf.trim();
        if ip.is_empty() {
            self.wallet_screen
                .set_status("IP-адрес не может быть пустым".to_string());
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            warn!(duration_secs = duration, "Пустой IP-адрес");
        } else if let Ok(port_num) = port_buf.trim().parse::<u16>() {
            let address = format!("{}:{}", ip, port_num);
            if let Some(wallet) = self.node.find_wallet_by_ip(ip, port_num) {
                if self.node.add_peer(address.clone()) {
                    self.wallet_screen.set_status(format!(
                        "Найден кошелёк: {} для {}:{} и добавлен в network.json",
                        wallet, ip, port_num
                    ));
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    info!(wallet = %wallet, ip = %ip, port = port_num, duration_secs = duration, "Кошелёк найден и добавлен в network.json");
                } else {
                    self.wallet_screen.set_status(format!(
                        "Найден кошелёк: {} для {}:{}, уже существует в network.json",
                        wallet, ip, port_num
                    ));
                    let duration = SystemTime::now()
                        .duration_since(start_time)
                        .unwrap()
                        .as_secs_f64();
                    info!(wallet = %wallet, ip = %ip, port = port_num, duration_secs = duration, "Кошелёк найден, уже существует в network.json");
                }
            } else {
                self.wallet_screen
                    .set_status(format!("Кошелёк не найден для {}:{}", ip, port_num));
                let duration = SystemTime::now()
                    .duration_since(start_time)
                    .unwrap()
                    .as_secs_f64();
                warn!(ip = %ip, port = port_num, duration_secs = duration, "Кошелёк не найден");
            }
        } else {
            self.wallet_screen
                .set_status("Неверный формат порта".to_string());
            let duration = SystemTime::now()
                .duration_since(start_time)
                .unwrap()
                .as_secs_f64();
            warn!(port = %port_buf, duration_secs = duration, "Неверный формат порта");
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

        // Handle sync and status updates (same for both UIs)
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
                // Update status in both UIs
                self.wallet_screen.set_status(status.clone());
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

        // Show appropriate UI based on migration phase
        match self.migration_phase {
            MigrationPhase::Phase1_LegacyOnly => {
                self.show_legacy_ui(ctx, frame);
            }
            MigrationPhase::Phase2_Hybrid => {
                // Show a toggle button to switch between UIs during development
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Show Legacy UI").clicked() {
                            self.show_legacy_ui(ctx, frame);
                        }
                        if ui.button("Show Professional UI").clicked() {
                            self.show_professional_ui(ctx, frame);
                        }
                    });
                });
            }
            MigrationPhase::Phase3_ProfessionalOnly => {
                self.show_professional_ui(ctx, frame);
            }
        }
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

        assert_eq!(launcher.migration_phase, MigrationPhase::Phase1_LegacyOnly);
        assert_eq!(launcher.current_screen, ScreenType::Wallet);
        assert!(!launcher.is_authenticated);
        assert_eq!(launcher.wallet_address, "");
        assert_eq!(launcher.password, "");
    }

    #[test]
    fn test_migration_phase_switching() {
        let (_dir, node) = make_node("strangler_phase");
        let mut launcher = StranglerLauncher::new(node, PathBuf::from("./test_data"));

        assert_eq!(launcher.migration_phase, MigrationPhase::Phase1_LegacyOnly);

        launcher.set_migration_phase(MigrationPhase::Phase2_Hybrid);
        assert_eq!(launcher.migration_phase, MigrationPhase::Phase2_Hybrid);

        launcher.set_migration_phase(MigrationPhase::Phase3_ProfessionalOnly);
        assert_eq!(
            launcher.migration_phase,
            MigrationPhase::Phase3_ProfessionalOnly
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
        assert_eq!(launcher.wallet_screen.send_tab.balance, 0);
        assert_eq!(launcher.dashboard_screen.mempool_count, 0);
        assert!(launcher.dashboard_screen.block_count >= 1);
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
    fn test_migration_phase_equality() {
        assert_eq!(
            MigrationPhase::Phase1_LegacyOnly,
            MigrationPhase::Phase1_LegacyOnly
        );
        assert_eq!(MigrationPhase::Phase2_Hybrid, MigrationPhase::Phase2_Hybrid);
        assert_eq!(
            MigrationPhase::Phase3_ProfessionalOnly,
            MigrationPhase::Phase3_ProfessionalOnly
        );

        assert_ne!(
            MigrationPhase::Phase1_LegacyOnly,
            MigrationPhase::Phase2_Hybrid
        );
        assert_ne!(
            MigrationPhase::Phase2_Hybrid,
            MigrationPhase::Phase3_ProfessionalOnly
        );
        assert_ne!(
            MigrationPhase::Phase3_ProfessionalOnly,
            MigrationPhase::Phase1_LegacyOnly
        );
    }
}
