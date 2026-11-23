//! Dashboard screen implementation

use crate::components::{Button, Card, CardSpacer, CardText, Chart, Stat};
use crate::i18n::Localization;
use crate::screens::Screen;
use crate::theme::ThemeManager;
use eframe::egui;

/// Dashboard screen with overview statistics and charts
pub struct DashboardScreen {
    pub stats: DashboardStats,
    pub transactions: Vec<Transaction>,
    pub agents: Vec<Agent>,
    pub selected_chart: ChartType,
    /// Live chain stats fed by the strangler launcher.
    pub block_count: usize,
    pub mempool_count: usize,
}

/// Dashboard statistics
pub struct DashboardStats {
    pub total_balance: u64,
    pub pending_transactions: u64,
    pub active_agents: u64,
    pub network_hashrate: f64,
}

/// Transaction data
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

/// Agent data
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub status: AgentStatus,
    pub uptime: f64,
    pub last_seen: u64,
}

/// Agent status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Online,
    Offline,
    Error,
}

/// Chart types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChartType {
    Balance,
    Transactions,
    Network,
}

impl DashboardScreen {
    pub fn new() -> Self {
        Self {
            stats: DashboardStats {
                total_balance: 1000,
                pending_transactions: 5,
                active_agents: 3,
                network_hashrate: 1.5e6,
            },
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
            agents: vec![
                Agent {
                    id: "agent_001".to_string(),
                    name: "Validator 1".to_string(),
                    status: AgentStatus::Online,
                    uptime: 99.9,
                    last_seen: 1642697400,
                },
                Agent {
                    id: "agent_002".to_string(),
                    name: "Validator 2".to_string(),
                    status: AgentStatus::Online,
                    uptime: 99.5,
                    last_seen: 1642697400,
                },
                Agent {
                    id: "agent_003".to_string(),
                    name: "Validator 3".to_string(),
                    status: AgentStatus::Error,
                    uptime: 95.0,
                    last_seen: 1642697400,
                },
            ],
            selected_chart: ChartType::Balance,
            block_count: 0,
            mempool_count: 0,
        }
    }

    // ------------------------------------------------------------------
    // Strangler-launcher bridge (src/gui/strangler_migration.rs)
    // ------------------------------------------------------------------

    pub fn set_block_count(&mut self, count: usize) {
        self.block_count = count;
    }

    pub fn set_mempool_count(&mut self, count: usize) {
        self.mempool_count = count;
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

    pub fn stats_mut(&mut self) -> &mut DashboardStats {
        &mut self.stats
    }

    pub fn transactions_mut(&mut self) -> &mut Vec<Transaction> {
        &mut self.transactions
    }

    pub fn agents_mut(&mut self) -> &mut Vec<Agent> {
        &mut self.agents
    }

    pub fn selected_chart_mut(&mut self) -> &mut ChartType {
        &mut self.selected_chart
    }
}

impl Screen for DashboardScreen {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        let tokens = theme_manager.tokens();

        // Header
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(localization.dashboard_title())
                    .size(tokens.typography.xl.size)
                    .strong(),
            );

            // Chart selector
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for chart_type in [
                    ChartType::Balance,
                    ChartType::Transactions,
                    ChartType::Network,
                ] {
                    if ui
                        .selectable_label(
                            self.selected_chart == chart_type,
                            format!("{:?}", chart_type),
                        )
                        .clicked()
                    {
                        self.selected_chart = chart_type;
                    }
                }
            });
        });

        ui.add_space(tokens.spacing.lg);

        // Stats grid
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = tokens.spacing.lg;

            // Total Balance
            Card::new()
                .title(localization.dashboard_total_balance())
                .child(
                    Stat::new("balance", "", &format!("{} SC", self.stats.total_balance))
                        .icon("💰")
                        .size(crate::components::StatSize::Lg),
                )
                .show(ui, theme_manager);

            // Pending Transactions
            Card::new()
                .title(localization.dashboard_pending_transactions())
                .child(
                    Stat::new("pending", "", &self.stats.pending_transactions.to_string())
                        .icon("⏳")
                        .size(crate::components::StatSize::Lg),
                )
                .show(ui, theme_manager);

            // Active Agents
            Card::new()
                .title(localization.dashboard_active_agents())
                .child(
                    Stat::new("agents", "", &self.stats.active_agents.to_string())
                        .icon("🤖")
                        .size(crate::components::StatSize::Lg),
                )
                .show(ui, theme_manager);

            // Network Hashrate
            Card::new()
                .title(localization.dashboard_network_hashrate())
                .child(
                    Stat::new(
                        "hashrate",
                        "",
                        &format!("{:.2} TH/s", self.stats.network_hashrate / 1e12),
                    )
                    .icon("⚡")
                    .size(crate::components::StatSize::Lg),
                )
                .show(ui, theme_manager);
        });

        ui.add_space(tokens.spacing.lg);

        // Chart and transactions grid
        ui.horizontal(|ui| {
            // Chart (left side, 2/3 width)
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() * 0.66);

                Card::new()
                    .title(format!("{:?} Chart", self.selected_chart))
                    .child(
                        Chart::new("chart", crate::components::ChartType::Line)
                            .data(self.generate_chart_data())
                            .size(ui.available_width(), 300.0),
                    )
                    .show(ui, theme_manager);
            });

            // Recent transactions (right side, 1/3 width)
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() * 0.33);

                let mut card = Card::new().title(localization.dashboard_recent_transactions());
                card.show_with(ui, theme_manager, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for tx in &self.transactions {
                            Card::new()
                                .child(CardText::new(&format!("ID: {}", tx.id)))
                                .child(CardText::new(&format!("Amount: {} SC", tx.amount)))
                                .child(CardText::new(&format!("Status: {:?}", tx.status)).color(
                                    match tx.status {
                                        TransactionStatus::Success => {
                                            theme_manager.colors().success.as_egui()
                                        }
                                        TransactionStatus::Pending => {
                                            theme_manager.colors().warning.as_egui()
                                        }
                                        TransactionStatus::Failed => {
                                            theme_manager.colors().error.as_egui()
                                        }
                                    },
                                ))
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
            });
        });

        ui.add_space(tokens.spacing.lg);

        // Active agents
        let mut agents_card = Card::new().title(localization.dashboard_active_agents());
        agents_card.show_with(ui, theme_manager, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for agent in &self.agents {
                    Card::new()
                        .child(CardText::new(&format!("Name: {}", agent.name)))
                        .child(CardText::new(&format!("Status: {:?}", agent.status)).color(
                            match agent.status {
                                AgentStatus::Online => theme_manager.colors().success.as_egui(),
                                AgentStatus::Offline => {
                                    theme_manager.colors().text_secondary.as_egui()
                                }
                                AgentStatus::Error => theme_manager.colors().error.as_egui(),
                            },
                        ))
                        .child(CardText::new(&format!("Uptime: {:.1}%", agent.uptime)))
                        .child(CardText::new(&format!("Last Seen: {}", agent.last_seen)))
                        .child(CardSpacer::new(tokens.spacing.sm))
                        .child(Button::new("View Details").onclick({
                            let agent_id = agent.id.clone();
                            move || println!("View agent details: {}", agent_id)
                        }))
                        .show(ui, theme_manager);

                    ui.add_space(tokens.spacing.base);
                }
            });
        });
    }

    fn title(&self, localization: &Localization) -> String {
        localization.dashboard_title()
    }

    fn on_back(&mut self) {
        // No back navigation for dashboard screen
    }

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl Default for DashboardScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardScreen {
    fn generate_chart_data(&self) -> Vec<crate::components::ChartDataPoint> {
        match self.selected_chart {
            ChartType::Balance => vec![
                crate::components::ChartDataPoint {
                    x: 1.0,
                    y: 1000.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 2.0,
                    y: 1200.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 3.0,
                    y: 1100.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 4.0,
                    y: 1300.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 5.0,
                    y: 1250.0,
                    label: None,
                    color: None,
                },
            ],
            ChartType::Transactions => vec![
                crate::components::ChartDataPoint {
                    x: 1.0,
                    y: 10.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 2.0,
                    y: 15.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 3.0,
                    y: 12.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 4.0,
                    y: 18.0,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 5.0,
                    y: 20.0,
                    label: None,
                    color: None,
                },
            ],
            ChartType::Network => vec![
                crate::components::ChartDataPoint {
                    x: 1.0,
                    y: 1.2e6,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 2.0,
                    y: 1.3e6,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 3.0,
                    y: 1.4e6,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 4.0,
                    y: 1.5e6,
                    label: None,
                    color: None,
                },
                crate::components::ChartDataPoint {
                    x: 5.0,
                    y: 1.6e6,
                    label: None,
                    color: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_screen_creation() {
        let screen = DashboardScreen::new();
        assert_eq!(screen.stats.total_balance, 1000);
        assert_eq!(screen.stats.pending_transactions, 5);
        assert_eq!(screen.stats.active_agents, 3);
        assert_eq!(screen.stats.network_hashrate, 1.5e6);
        assert_eq!(screen.transactions.len(), 2);
        assert_eq!(screen.agents.len(), 3);
        assert_eq!(screen.selected_chart, ChartType::Balance);
    }

    #[test]
    fn test_dashboard_stats() {
        let mut screen = DashboardScreen::new();
        let stats = screen.stats_mut();

        stats.total_balance = 2000;
        stats.pending_transactions = 10;
        stats.active_agents = 5;
        stats.network_hashrate = 2.0e6;

        assert_eq!(stats.total_balance, 2000);
        assert_eq!(stats.pending_transactions, 10);
        assert_eq!(stats.active_agents, 5);
        assert_eq!(stats.network_hashrate, 2.0e6);
    }

    #[test]
    fn test_dashboard_transactions() {
        let mut screen = DashboardScreen::new();
        let transactions = screen.transactions_mut();

        transactions.push(Transaction {
            id: "tx_003".to_string(),
            from: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q8".to_string(),
            to: "sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q9".to_string(),
            amount: 75,
            fee: 1,
            status: TransactionStatus::Success,
            timestamp: 1642697400,
        });

        assert_eq!(transactions.len(), 3);
    }

    #[test]
    fn test_dashboard_agents() {
        let mut screen = DashboardScreen::new();
        let agents = screen.agents_mut();

        agents.push(Agent {
            id: "agent_004".to_string(),
            name: "Validator 4".to_string(),
            status: AgentStatus::Online,
            uptime: 99.8,
            last_seen: 1642697400,
        });

        assert_eq!(agents.len(), 4);
    }

    #[test]
    fn test_dashboard_chart_selection() {
        let mut screen = DashboardScreen::new();
        assert_eq!(screen.selected_chart, ChartType::Balance);

        screen.selected_chart = ChartType::Transactions;
        assert_eq!(screen.selected_chart, ChartType::Transactions);

        screen.selected_chart = ChartType::Network;
        assert_eq!(screen.selected_chart, ChartType::Network);
    }

    #[test]
    fn test_transaction_statuses() {
        assert_eq!(TransactionStatus::Success, TransactionStatus::Success);
        assert_eq!(TransactionStatus::Pending, TransactionStatus::Pending);
        assert_eq!(TransactionStatus::Failed, TransactionStatus::Failed);
    }

    #[test]
    fn test_agent_statuses() {
        assert_eq!(AgentStatus::Online, AgentStatus::Online);
        assert_eq!(AgentStatus::Offline, AgentStatus::Offline);
        assert_eq!(AgentStatus::Error, AgentStatus::Error);
    }

    #[test]
    fn test_chart_types() {
        assert_eq!(ChartType::Balance, ChartType::Balance);
        assert_eq!(ChartType::Transactions, ChartType::Transactions);
        assert_eq!(ChartType::Network, ChartType::Network);
    }
}
