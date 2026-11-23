//! UI screens for Strangecoin

pub mod agent_activity;
pub mod dashboard;
pub mod settings;
pub mod tx_detail;
pub mod wallet;

// Re-export screen types
pub use agent_activity::AgentActivityScreen;
pub use dashboard::DashboardScreen;
pub use settings::SettingsScreen;
pub use tx_detail::TxDetailScreen;
pub use wallet::WalletScreen;

use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::Arc;

/// Screen trait for all UI screens
pub trait Screen {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme_manager: &ThemeManager,
        localization: &Localization,
    );
    fn title(&self, localization: &Localization) -> String;
    fn on_back(&mut self);
    fn can_navigate_back(&self) -> bool;
}

/// Screen navigation context
pub struct ScreenContext {
    pub node: Arc<BlockchainFacade>,
    pub theme_manager: ThemeManager,
    pub localization: Localization,
}

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
