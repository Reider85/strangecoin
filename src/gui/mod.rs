//! GUI-клиент (egui/eframe) за feature-флагом `gui` (скелет P02; перенесён из
//! `lib.rs` при исправлении BUG-S0-020). До Stage 4 живёт здесь; в Stage 4
//! переедет в отдельный крейт `strangecoin-gui` (ARCHITECT3 §10.6).

use eframe::egui;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::SystemTime;
use tracing::{debug, error, info, warn};

use crate::error::StrangecoinError;
use crate::{wallet, MiningStatus, MiningTask, Node, Transaction};

pub struct WalletApp {
    pub node: Node,
    pub wallet_address: String,
    pub password: String,
    pub is_authenticated: bool,
    pub receiver_address: String,
    pub amount: String,
    pub ip: String,
    pub port: String,
    pub status: String,
    pub mining_status: Arc<Mutex<MiningStatus>>,
    pub mining_progress: Arc<Mutex<Option<String>>>,
    pub progress_rx: Option<mpsc::Receiver<String>>,
    pub status_rx: Option<mpsc::Receiver<String>>,
    pub mining_tx: tokio::sync::mpsc::UnboundedSender<MiningTask>,
    pub last_repaint: f64,
    pub new_wallet_password: String,
    pub data_dir: PathBuf,
}

impl eframe::App for WalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                self.status = status;
                if self.status.starts_with("Транзакция отправлена") {
                    if let Ok(mut mining_status) = self.mining_status.lock() {
                        *mining_status = MiningStatus::Idle;
                        debug!("Статус майнинга сброшен на Idle");
                    }
                    self.progress_rx = None;
                }
                debug!(status = %self.status, "Получено обновление статуса");
                ctx.request_repaint();
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(&self.status);
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
                        let start_time = SystemTime::now();
                        info!(wallet_address = %self.wallet_address, "Кнопка 'Войти' нажата");
                        let data_dir = self.data_dir.clone();
                        let password = if self.password.is_empty() {
                            wallet::Wallet::get_password_from_env()
                        } else {
                            Some(self.password.clone())
                        };
                        let password = match password {
                            Some(p) => p,
                            None => {
                                self.status = "Пароль не указан (введите в поле или задайте STRANGECOIN_WALLET_PASSWORD)".to_string();
                                ctx.request_repaint();
                                return;
                            }
                        };
                        match wallet::Wallet::list_keystores(&data_dir) {
                            Ok(keystores) => {
                                let sanitized_address = self.wallet_address
                                    .replace("/", "_")
                                    .replace("+", "_")
                                    .replace("=", "_");
                                let keystore_path = keystores.iter().find(|p| {
                                    p.file_name().and_then(|n| n.to_str()) == Some(&format!("wallet_{}.json", sanitized_address))
                                });
                                let keystore_path = match keystore_path {
                                    Some(p) => p,
                                    None => {
                                        self.status = "Кошелёк не найден".to_string();
                                        ctx.request_repaint();
                                        return;
                                    }
                                };
                                match wallet::Wallet::load(&password, keystore_path) {
                                    Ok(wallet) => {
                                        let expected_address = crate::address::encode_address(&wallet.public_key, self.node.network_id)
                        .unwrap_or_else(|_| "invalid_address".to_string());
                                        if expected_address == self.wallet_address {
                                            self.is_authenticated = true;
                                            self.status = "Успешная аутентификация".to_string();
                                            self.node.discover_peers();
                                            let duration = SystemTime::now()
                                                .duration_since(start_time)
                                                .unwrap()
                                                .as_secs_f64();
                                            info!(duration_secs = duration, "Аутентификация успешна");
                                        } else {
                                            self.status = "Неверный адрес кошелька".to_string();
                                            let duration = SystemTime::now()
                                                .duration_since(start_time)
                                                .unwrap()
                                                .as_secs_f64();
                                            warn!(duration_secs = duration, "Аутентификация не удалась: неверный адрес кошелька");
                                        }
                                    }
                                    Err(e) => {
                                        self.status = format!("Ошибка аутентификации: {}", e);
                                        let duration = SystemTime::now()
                                            .duration_since(start_time)
                                            .unwrap()
                                            .as_secs_f64();
                                        error!(duration_secs = duration, error = %e, "Аутентификация не удалась");
                                    }
                                }
                            }
                            Err(e) => {
                                self.status = format!("Ошибка поиска кошельков: {}", e);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                error!(duration_secs = duration, error = %e, "Ошибка поиска кошельков");
                            }
                        }
                        ctx.request_repaint();
                    }
                    if ui.button("Регистрация").clicked() {
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
                                self.status = "Пароль для регистрации не может быть пустым (введите в поле или задайте STRANGECOIN_WALLET_PASSWORD)".to_string();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                warn!(duration_secs = duration, "Ошибка регистрации: пустой пароль");
                                ctx.request_repaint();
                                return;
                            }
                        };
                        let data_dir = self.data_dir.clone();
                        match wallet::Wallet::new(&password, &data_dir) {
                            Ok(wallet) => {
                                self.wallet_address = crate::address::encode_address(&wallet.public_key, self.node.network_id)
                        .unwrap_or_else(|_| "invalid_address".to_string());
                                self.password = password;
                                self.is_authenticated = true;
                                self.status = format!("Кошелёк успешно создан: {}", self.wallet_address);
                                self.node.discover_peers();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                info!(duration_secs = duration, wallet_address = %self.wallet_address, "Регистрация успешна");
                                if !self.node.blockchain.has_account(&self.wallet_address) {
                                    match self.node.blockchain.grant_initial_balance_to_first_wallet(&self.wallet_address) {
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
                                self.status = format!("Ошибка регистрации: {}", e);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                error!(duration_secs = duration, error = %e, "Ошибка регистрации");
                            }
                        }
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
                ui.text_edit_singleline(&mut self.receiver_address);
                ui.text_edit_singleline(&mut self.amount);

if let Some(ref progress_rx) = self.progress_rx {
                    while let Ok(progress) = progress_rx.try_recv() {
                        let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                        *mining_progress = Some(progress);
                        debug!(?mining_progress, "Прогресс майнинга обновлён в UI");
                    }
                }

                let is_mining = matches!(*self.mining_status.lock().unwrap(), MiningStatus::Mining);

                if is_mining {
                    ui.label("Майнинг блока в процессе...");
                    let progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                    if let Some(progress_msg) = &*progress {
                        ui.label(format!("Прогресс: {}", progress_msg));
                    }
                    ui.spinner();
                    ctx.request_repaint();
                } else if ui.button("Отправить").clicked() {
                    let start_time = SystemTime::now();
                    info!(receiver = %self.receiver_address, amount = %self.amount, "Кнопка 'Отправить' нажата");
                    if self.receiver_address.trim().is_empty() {
                        self.status = "Адрес получателя не может быть пустым".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(duration_secs = duration, "Пустой адрес получателя");
                        ctx.request_repaint();
                        return;
                    }
                    if let Ok(amount) = self.amount.trim().parse::<u64>() {
                        if amount == 0 {
                            self.status = "Сумма должна быть больше нуля".to_string();
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            warn!(duration_secs = duration, "Сумма равна нулю");
                            ctx.request_repaint();
                            return;
                        }
                        let sender_nonce = self.node.blockchain.get_nonce(&self.wallet_address);
                            let mut transaction = Transaction {
                                sender: self.wallet_address.clone(),
                                receiver: self.receiver_address.trim().to_string(),
                                amount,
                                nonce: sender_nonce + 1,
                                chain_id: self.node.network_id,
                                signature: Vec::new(),
                                is_coinbase: false,
                            };
                            let data_dir = self.data_dir.clone();
                            let sanitized_address = self.wallet_address
                                .replace("/", "_")
                                .replace("+", "_")
                                .replace("=", "_");
                            let keystore_path = data_dir.join("keystore").join(format!("wallet_{}.json", sanitized_address));
                            let wallet = match wallet::Wallet::load(&self.password, &keystore_path) {
                                Ok(w) => w,
                                Err(e) => {
                                    self.status = format!("Ошибка загрузки кошелька: {}", e);
                                    let duration = SystemTime::now()
                                        .duration_since(start_time)
                                        .unwrap()
                                        .as_secs_f64();
                                    error!(duration_secs = duration, error = %e, "Ошибка загрузки кошелька");
                                    ctx.request_repaint();
                                    return;
                                }
                            };
                            if let Err(e) = wallet.sign_transaction(&mut transaction) {
                                self.status = format!("Ошибка подписи транзакции: {}", e);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                error!(duration_secs = duration, error = %e, "Ошибка подписи транзакции");
                                ctx.request_repaint();
                                return;
                            };
                        let blockchain = Arc::clone(&self.node.blockchain);
                        let mining_status = Arc::clone(&self.mining_status);

                        {
                            debug!(?transaction, "Транзакция для добавления");
                            if blockchain.apply_tx(transaction.clone()).is_err() {
                                self.status = "Недостаточно средств или неверный адрес".to_string();
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                warn!(duration_secs = duration, "Недостаточно средств или неверный адрес");
                                ctx.request_repaint();
                                return;
                            }
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            info!(duration_secs = duration, "Транзакция успешно добавлена");
                        }

                        self.status = "Запуск майнинга...".to_string();
                        info!("Подготовка к отправке задачи майнинга");
                        let (progress_tx, progress_rx) = mpsc::channel();
                        let (status_tx, status_rx) = mpsc::channel();
                        {
                            let mut mining_progress = self.mining_progress.lock().expect("Не удалось захватить Mutex для mining_progress");
                            *mining_progress = None;
                            self.progress_rx = Some(progress_rx);
                            self.status_rx = Some(status_rx);
                            debug!("Каналы прогресса и статуса созданы");
                        }
                        if let Ok(mut mining_status) = self.mining_status.lock() {
                            *mining_status = MiningStatus::Mining;
                            info!("Статус майнинга установлен: Mining");
                        } else {
                            self.status = "Ошибка: Не удалось установить статус майнинга".to_string();
                            error!("Не удалось установить статус майнинга");
                            self.progress_rx = None;
                            self.status_rx = None;
                            ctx.request_repaint();
                            return;
                        }
                        info!("Попытка отправки задачи майнинга");
                        // UnboundedSender::send is synchronous (ADR-0011).
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
                            self.status = format!("Ошибка отправки задачи майнинга: {}", e);
                            error!(error = %e, "Ошибка отправки задачи майнинга");
                            if let Ok(mut mining_status) = self.mining_status.lock() {
                                *mining_status = MiningStatus::Idle;
                                debug!("Статус майнинга сброшен на Idle");
                            }
                            self.progress_rx = None;
                            self.status_rx = None;
                            ctx.request_repaint();
                            return;
                        }
                        info!("Задача майнинга успешно отправлена");
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        info!(duration_secs = duration, "Запуск майнинга завершён");
                        ctx.request_repaint();
                    } else {
                        self.status = "Неверный формат суммы".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(duration_secs = duration, port = %self.port, "Неверный формат суммы");
                        ctx.request_repaint();
                    }
                }

                ui.heading("Поиск кошелька по IP и порту");
                ui.horizontal(|ui| {
                    ui.label("IP: ");
                    ui.text_edit_singleline(&mut self.ip);
                    ui.label("Порт: ");
                    ui.add(egui::TextEdit::singleline(&mut self.port).desired_width(50.0));
                });
                if ui.button("Найти кошелёк").clicked() {
                    let start_time = SystemTime::now();
                    info!(ip = %self.ip, port = %self.port, "Кнопка 'Найти кошелёк' нажата");
                    let ip = self.ip.trim();
                    if ip.is_empty() {
                        self.status = "IP-адрес не может быть пустым".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(duration_secs = duration, "Пустой IP-адрес");
                    } else if let Ok(port_num) = self.port.trim().parse::<u16>() {
                        let address = format!("{}:{}", ip, port_num);
                        if let Some(wallet) = self.node.find_wallet_by_ip(ip, port_num) {
                            if self.node.add_peer(address.clone()) {
                                self.status = format!("Найден кошелёк: {} для {}:{} и добавлен в network.json", wallet, ip, port_num);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                info!(wallet = %wallet, ip = %ip, port = port_num, duration_secs = duration, "Кошелёк найден и добавлен в network.json");
                            } else {
                                self.status = format!("Найден кошелёк: {} для {}:{}, уже существует в network.json", wallet, ip, port_num);
                                let duration = SystemTime::now()
                                    .duration_since(start_time)
                                    .unwrap()
                                    .as_secs_f64();
                                info!(wallet = %wallet, ip = %ip, port = port_num, duration_secs = duration, "Кошелёк найден, уже существует в network.json");
                            }
                        } else {
                            self.status = format!("Кошелёк не найден для {}:{}", ip, port_num);
                            let duration = SystemTime::now()
                                .duration_since(start_time)
                                .unwrap()
                                .as_secs_f64();
                            warn!(ip = %ip, port = port_num, duration_secs = duration, "Кошелёк не найден");
                        }
                    } else {
                        self.status = "Неверный формат порта".to_string();
                        let duration = SystemTime::now()
                            .duration_since(start_time)
                            .unwrap()
                            .as_secs_f64();
                        warn!(port = %self.port, duration_secs = duration, "Неверный формат порта");
                    }
                    ctx.request_repaint();
                }
            }
        });
    }
}

/// Запускает egui event loop на выделенном blocking-потоке (ADR-0007):
/// `eframe::run_native` блокирующий и владеет windowing event loop, поэтому
/// не должен занимать worker runtime.
pub async fn run(app: WalletApp) {
    tokio::task::spawn_blocking(move || {
        eframe::run_native(
            "Blockchain Wallet",
            eframe::NativeOptions::default(),
            Box::new(|_cc| Box::new(app)),
        )
        .expect("Ошибка запуска приложения");
    })
    .await
    .expect("GUI task panicked");
}
