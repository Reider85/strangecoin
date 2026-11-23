//! Strangecoin Professional UI
//!
//! A professional UI crate for Strangecoin, featuring:
//! - Design system with tokens, themes, and components
//! - 5 screens: Wallet, Dashboard, Transaction Detail, Agent Activity, Settings
//! - WCAG AA accessibility compliance
//! - i18n support (en/ru/zh)
//! - Light/dark themes
//!
//! ## Usage
//!
//! ```rust,ignore
//! use strangecoin_ui::{App, UiBackend, EframeEguiBackend};
//! use std::sync::Arc;
//!
//! let node = Arc::new(BlockchainFacade::open_local(...));
//! let app = App::new(node);
//! let backend = EframeEguiBackend::new(app);
//! backend.run();
//! ```

pub mod accessibility;
pub mod components;
pub mod i18n;
pub mod screens;
pub mod theme;
pub mod tokens;

pub use crate::i18n::Localization;
pub use crate::theme::ThemeManager;
use eframe::egui;
pub use screens::{
    AgentActivityScreen, DashboardScreen, SettingsScreen, TxDetailScreen, WalletScreen,
};
use std::sync::Arc;

/// Backend trait for UI rendering (render-backend-agnostic)
pub trait UiBackend {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame);
    fn theme(&self) -> &ThemeManager;
    fn localization(&self) -> &Localization;
}

/// Eframe/egui backend implementation
pub struct EframeEguiBackend {
    app: App,
}

impl EframeEguiBackend {
    pub fn new(app: App) -> Self {
        Self { app }
    }

    pub fn run(self) -> Result<(), eframe::Error> {
        eframe::run_native(
            "Strangecoin",
            eframe::NativeOptions::default(),
            Box::new(|_cc| Ok(Box::new(self))),
        )
    }
}

impl eframe::App for EframeEguiBackend {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
    }
}

impl UiBackend for EframeEguiBackend {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.update(ctx, frame);
    }

    fn theme(&self) -> &ThemeManager {
        self.app.theme()
    }

    fn localization(&self) -> &Localization {
        self.app.localization()
    }
}

/// Main application struct
pub struct App {
    pub theme_manager: ThemeManager,
    pub localization: Localization,
    pub current_screen: Screen,
    pub node: Arc<BlockchainFacade>,
}

impl App {
    pub fn new(node: Arc<BlockchainFacade>) -> Self {
        let theme_manager = ThemeManager::new();
        let localization = Localization::new();
        let current_screen = Screen::Wallet;

        Self {
            theme_manager,
            localization,
            current_screen,
            node,
        }
    }

    pub fn theme(&self) -> &ThemeManager {
        &self.theme_manager
    }

    pub fn localization(&self) -> &Localization {
        &self.localization
    }

    pub fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme
        self.theme_manager.apply_to_egui(ctx);

        // Top-level navigation
        egui::TopBottomPanel::top("navigation").show(ctx, |ui| {
            self.render_navigation(ui);
        });

        // Main content area
        let screen = self.current_screen.clone();
        egui::CentralPanel::default().show(ctx, |ui| match screen {
            Screen::Wallet => self.render_wallet_screen(ui),
            Screen::Dashboard => self.render_dashboard_screen(ui),
            Screen::TxDetail(txid) => self.render_tx_detail_screen(ui, txid),
            Screen::AgentActivity => self.render_agent_activity_screen(ui),
            Screen::Settings => self.render_settings_screen(ui),
        });

        // Footer
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            self.render_footer(ui);
        });
    }

    fn render_navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let buttons = [
                (Screen::Wallet, "wallet-title"),
                (Screen::Dashboard, "dashboard-title"),
                (Screen::AgentActivity, "agent-activity-title"),
                (Screen::Settings, "settings-title"),
            ];

            for (screen, key) in buttons {
                let label = self.localization.tr(key, std::collections::HashMap::new());
                if ui
                    .selectable_label(self.current_screen == screen, label)
                    .clicked()
                {
                    self.current_screen = screen;
                }
            }
        });
    }

    fn render_wallet_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            self.localization
                .tr("wallet-title", std::collections::HashMap::new()),
        );
        // Wallet screen content will be implemented in UI-P03
        ui.label("Wallet screen - to be implemented in UI-P03");
    }

    fn render_dashboard_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            self.localization
                .tr("dashboard-title", std::collections::HashMap::new()),
        );
        // Dashboard screen content will be implemented in UI-P04
        ui.label("Dashboard screen - to be implemented in UI-P04");
    }

    fn render_tx_detail_screen(&mut self, ui: &mut egui::Ui, txid: String) {
        ui.heading(
            self.localization
                .tr("tx-detail-title", std::collections::HashMap::new()),
        );
        ui.label(format!("Transaction ID: {}", txid));
        // Transaction detail content will be implemented in UI-P04
        ui.label("Transaction detail screen - to be implemented in UI-P04");
    }

    fn render_agent_activity_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            self.localization
                .tr("agent-activity-title", std::collections::HashMap::new()),
        );
        // Agent activity content will be implemented in UI-P05
        ui.label("Agent activity screen - to be implemented in UI-P05");
    }

    fn render_settings_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            self.localization
                .tr("settings-title", std::collections::HashMap::new()),
        );
        // Settings screen content will be implemented in UI-P05
        ui.label("Settings screen - to be implemented in UI-P05");
    }

    fn render_footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!("Theme: {}", self.theme_manager.current()));
            ui.label(format!("Locale: {}", self.localization.current_locale()));
        });
    }
}

/// Application screens
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Wallet,
    Dashboard,
    TxDetail(String),
    AgentActivity,
    Settings,
}

/// Re-export for convenience
pub use tokens::{ColorPalette, DesignTokens, Radius, Shadow, SpacingScale, TypographyScale};

// Mock types for compilation (will be replaced with real types from strangecoin-core)
#[derive(Clone)]
pub struct BlockchainFacade {
    // Mock implementation
}

impl BlockchainFacade {
    pub fn open_local(_path: &std::path::Path) -> Arc<Self> {
        Arc::new(Self {})
    }
}
