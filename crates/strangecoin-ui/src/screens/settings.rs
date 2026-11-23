//! Settings screen implementation

use crate::screens::Screen;
use crate::theme::ThemeManager;
use crate::i18n::Localization;
use crate::components::{Button, Input, Card, CardText, Tabs};
use eframe::egui;

/// Settings screen with configuration options
pub struct SettingsScreen {
    pub active_tab: SettingsTab,
    pub general_tab: GeneralTab,
    pub network_tab: NetworkTab,
    pub security_tab: SecurityTab,
    pub appearance_tab: AppearanceTab,
    pub about_tab: AboutTab,
}

/// Settings tabs
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTab {
    General,
    Network,
    Security,
    Appearance,
    About,
}

/// General settings
pub struct GeneralTab {
    pub language: String,
    pub currency: String,
    pub notifications: bool,
    pub auto_update: bool,
    pub data_directory: String,
}

/// Network settings
pub struct NetworkTab {
    pub node_address: String,
    pub port: u16,
    pub use_tls: bool,
    pub sync_mode: SyncMode,
    pub max_peers: u32,
}

/// Sync modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncMode {
    Full,
    Fast,
    Light,
}

/// Security settings
pub struct SecurityTab {
    pub wallet_encryption: bool,
    pub biometric_auth: bool,
    pub two_factor_auth: bool,
    pub auto_lock: bool,
    pub lock_timeout: u32,
}

/// Appearance settings
pub struct AppearanceTab {
    pub theme: ThemeMode,
    pub font_size: FontSize,
    pub compact_mode: bool,
    pub animations: bool,
}

/// Theme modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// Font sizes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    Small,
    Medium,
    Large,
}

/// About settings
pub struct AboutTab {
    pub version: String,
    pub build_date: String,
    pub commit_hash: String,
    pub license: String,
    pub contributors: Vec<String>,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::General,
            general_tab: GeneralTab {
                language: "en".to_string(),
                currency: "USD".to_string(),
                notifications: true,
                auto_update: true,
                data_directory: "./data".to_string(),
            },
            network_tab: NetworkTab {
                node_address: "localhost".to_string(),
                port: 8333,
                use_tls: false,
                sync_mode: SyncMode::Fast,
                max_peers: 50,
            },
            security_tab: SecurityTab {
                wallet_encryption: true,
                biometric_auth: false,
                two_factor_auth: false,
                auto_lock: true,
                lock_timeout: 300,
            },
            appearance_tab: AppearanceTab {
                theme: ThemeMode::System,
                font_size: FontSize::Medium,
                compact_mode: false,
                animations: true,
            },
            about_tab: AboutTab {
                version: "3.0.0".to_string(),
                build_date: "2026-11-22".to_string(),
                commit_hash: "abc123".to_string(),
                license: "MIT".to_string(),
                contributors: vec![
                    "Developer 1".to_string(),
                    "Developer 2".to_string(),
                    "Developer 3".to_string(),
                ],
            },
        }
    }

    pub fn set_active_tab(&mut self, tab: SettingsTab) {
        self.active_tab = tab;
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

    pub fn general_tab_mut(&mut self) -> &mut GeneralTab {
        &mut self.general_tab
    }

    pub fn network_tab_mut(&mut self) -> &mut NetworkTab {
        &mut self.network_tab
    }

    pub fn security_tab_mut(&mut self) -> &mut SecurityTab {
        &mut self.security_tab
    }

    pub fn appearance_tab_mut(&mut self) -> &mut AppearanceTab {
        &mut self.appearance_tab
    }

    pub fn about_tab_mut(&mut self) -> &mut AboutTab {
        &mut self.about_tab
    }
}

impl Screen for SettingsScreen {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, localization: &Localization) {
        let tokens = theme_manager.tokens();

        // Header
        ui.label(
            egui::RichText::new(localization.settings_title())
                .size(tokens.typography.xl.size)
                .strong()
        );

        ui.add_space(tokens.spacing.lg);

        // Tab navigation (Tabs renders labels + tracks active index;
        // content is rendered separately below per SettingsTab)
        let mut tabs = Tabs::new("settings_tabs")
            .tab(
                "general",
                &localization.settings_general(),
                Box::new(crate::components::tabs::TextTabContent::new("")),
            )
            .tab(
                "network",
                &localization.settings_network(),
                Box::new(crate::components::tabs::TextTabContent::new("")),
            )
            .tab(
                "security",
                &localization.settings_security(),
                Box::new(crate::components::tabs::TextTabContent::new("")),
            )
            .tab(
                "appearance",
                &localization.settings_appearance(),
                Box::new(crate::components::tabs::TextTabContent::new("")),
            )
            .tab(
                "about",
                &localization.settings_about(),
                Box::new(crate::components::tabs::TextTabContent::new("")),
            )
            .active_tab(match self.active_tab {
                SettingsTab::General => 0,
                SettingsTab::Network => 1,
                SettingsTab::Security => 2,
                SettingsTab::Appearance => 3,
                SettingsTab::About => 4,
            });
        tabs.show(ui, theme_manager);

        // Sync tab selection back to screen state
        let new_tab = match tabs.active_tab_index() {
            0 => SettingsTab::General,
            1 => SettingsTab::Network,
            2 => SettingsTab::Security,
            3 => SettingsTab::Appearance,
            4 => SettingsTab::About,
            _ => self.active_tab,
        };
        if new_tab != self.active_tab {
            self.set_active_tab(new_tab);
        }

        ui.add_space(tokens.spacing.lg);

        // Tab content
        match self.active_tab {
            SettingsTab::General => self.show_general_tab(ui, theme_manager, localization),
            SettingsTab::Network => self.show_network_tab(ui, theme_manager, localization),
            SettingsTab::Security => self.show_security_tab(ui, theme_manager, localization),
            SettingsTab::Appearance => self.show_appearance_tab(ui, theme_manager, localization),
            SettingsTab::About => self.show_about_tab(ui, theme_manager, localization),
        }

        ui.add_space(tokens.spacing.lg);

        // Action buttons
        ui.horizontal(|ui| {
            Button::new("Save Settings")
                .onclick(|| {
                    println!("Save settings");
                })
                .show(ui, theme_manager);

            Button::new("Reset to Defaults")
                .variant(crate::components::ButtonVariant::Secondary)
                .onclick(|| {
                    println!("Reset to defaults");
                })
                .show(ui, theme_manager);
        });
    }

    fn title(&self, localization: &Localization) -> String {
        localization.settings_title()
    }

    fn on_back(&mut self) {
        // No back navigation for settings screen
    }

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl SettingsScreen {
    fn show_general_tab(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, localization: &Localization) {
        let _tokens = theme_manager.tokens();

        Card::new()
            .title(localization.settings_general())
            .child(
                Card::new()
                    .child(
                        Card::new()
                            .child(CardText::new("Language"))
                            .child(Input::new("language")
                                .placeholder("Select language")
                                .value(&self.general_tab.language)
                                .on_change(|value| {
                                    println!("Language changed to: {}", value);
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Currency"))
                            .child(Input::new("currency")
                                .placeholder("Select currency")
                                .value(&self.general_tab.currency)
                                .on_change(|value| {
                                    println!("Currency changed to: {}", value);
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Notifications"))
                            .child(
                                crate::components::Button::new(
                                    if self.general_tab.notifications {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle notifications");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Auto Update"))
                            .child(
                                crate::components::Button::new(
                                    if self.general_tab.auto_update {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle auto update");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Data Directory"))
                            .child(Input::new("data_directory")
                                .placeholder("Enter data directory")
                                .value(&self.general_tab.data_directory)
                                .on_change(|value| {
                                    println!("Data directory changed to: {}", value);
                                })
                            )
                    )
            )
            .show(ui, theme_manager);
    }

    fn show_network_tab(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, localization: &Localization) {
        let _tokens = theme_manager.tokens();

        Card::new()
            .title(localization.settings_network())
            .child(
                Card::new()
                    .child(
                        Card::new()
                            .child(CardText::new("Node Address"))
                            .child(Input::new("node_address")
                                .placeholder("Enter node address")
                                .value(&self.network_tab.node_address)
                                .on_change(|value| {
                                    println!("Node address changed to: {}", value);
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Port"))
                            .child(Input::new("port")
                                .input_type(crate::components::InputType::Number)
                                .value(&self.network_tab.port.to_string())
                                .on_change(|value| {
                                    if let Ok(port) = value.parse::<u16>() {
                                        println!("Port changed to: {}", port);
                                    }
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Use TLS"))
                            .child(
                                crate::components::Button::new(
                                    if self.network_tab.use_tls {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle TLS");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Sync Mode"))
                            .child(
                                crate::components::Button::new(
                                    match self.network_tab.sync_mode {
                                        SyncMode::Full => "Full",
                                        SyncMode::Fast => "Fast",
                                        SyncMode::Light => "Light",
                                    }
                                )
                                .onclick(|| {
                                    println!("Change sync mode");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Max Peers"))
                            .child(Input::new("max_peers")
                                .input_type(crate::components::InputType::Number)
                                .value(&self.network_tab.max_peers.to_string())
                                .on_change(|value| {
                                    if let Ok(peers) = value.parse::<u32>() {
                                        println!("Max peers changed to: {}", peers);
                                    }
                                })
                            )
                    )
            )
            .show(ui, theme_manager);
    }

    fn show_security_tab(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, localization: &Localization) {
        let _tokens = theme_manager.tokens();

        Card::new()
            .title(localization.settings_security())
            .child(
                Card::new()
                    .child(
                        Card::new()
                            .child(CardText::new("Wallet Encryption"))
                            .child(
                                crate::components::Button::new(
                                    if self.security_tab.wallet_encryption {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle wallet encryption");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Biometric Auth"))
                            .child(
                                crate::components::Button::new(
                                    if self.security_tab.biometric_auth {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle biometric auth");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Two-Factor Auth"))
                            .child(
                                crate::components::Button::new(
                                    if self.security_tab.two_factor_auth {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle two-factor auth");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Auto Lock"))
                            .child(
                                crate::components::Button::new(
                                    if self.security_tab.auto_lock {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle auto lock");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Lock Timeout"))
                            .child(Input::new("lock_timeout")
                                .input_type(crate::components::InputType::Number)
                                .value(&self.security_tab.lock_timeout.to_string())
                                .on_change(|value| {
                                    if let Ok(timeout) = value.parse::<u32>() {
                                        println!("Lock timeout changed to: {}", timeout);
                                    }
                                })
                            )
                    )
            )
            .show(ui, theme_manager);
    }

    fn show_appearance_tab(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, localization: &Localization) {
        let _tokens = theme_manager.tokens();

        Card::new()
            .title(localization.settings_appearance())
            .child(
                Card::new()
                    .child(
                        Card::new()
                            .child(CardText::new("Theme"))
                            .child(
                                crate::components::Button::new(
                                    match self.appearance_tab.theme {
                                        ThemeMode::Light => "Light",
                                        ThemeMode::Dark => "Dark",
                                        ThemeMode::System => "System",
                                    }
                                )
                                .onclick(|| {
                                    println!("Change theme");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Font Size"))
                            .child(
                                crate::components::Button::new(
                                    match self.appearance_tab.font_size {
                                        FontSize::Small => "Small",
                                        FontSize::Medium => "Medium",
                                        FontSize::Large => "Large",
                                    }
                                )
                                .onclick(|| {
                                    println!("Change font size");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Compact Mode"))
                            .child(
                                crate::components::Button::new(
                                    if self.appearance_tab.compact_mode {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle compact mode");
                                })
                            )
                    )
                    .child(
                        Card::new()
                            .child(CardText::new("Animations"))
                            .child(
                                crate::components::Button::new(
                                    if self.appearance_tab.animations {
                                        "Enabled"
                                    } else {
                                        "Disabled"
                                    }
                                )
                                .onclick(|| {
                                    println!("Toggle animations");
                                })
                            )
                    )
            )
            .show(ui, theme_manager);
    }

    fn show_about_tab(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, localization: &Localization) {
        let _tokens = theme_manager.tokens();

        Card::new()
            .title(localization.settings_about())
            .child(
                Card::new()
                    .child(CardText::new(&format!("Version: {}", self.about_tab.version)))
                    .child(CardText::new(&format!("Build Date: {}", self.about_tab.build_date)))
                    .child(CardText::new(&format!("Commit Hash: {}", self.about_tab.commit_hash)))
                    .child(CardText::new(&format!("License: {}", self.about_tab.license)))
                    .child(
                        Card::new()
                            .child(CardText::new("Contributors"))
                            .child(
                                crate::components::CodeBlock::new("contributors", &self.about_tab.contributors.join("\n"))
                                    .language("text")
                                    .theme(crate::components::CodeTheme::Dark)
                            )
                    )
            )
            .show(ui, theme_manager);
    }
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_screen_creation() {
        let screen = SettingsScreen::new();
        assert_eq!(screen.active_tab, SettingsTab::General);
        assert_eq!(screen.general_tab.language, "en");
        assert_eq!(screen.general_tab.currency, "USD");
        assert!(screen.general_tab.notifications);
        assert!(screen.general_tab.auto_update);
        assert_eq!(screen.general_tab.data_directory, "./data");
        assert_eq!(screen.network_tab.node_address, "localhost");
        assert_eq!(screen.network_tab.port, 8333);
        assert!(!screen.network_tab.use_tls);
        assert_eq!(screen.network_tab.sync_mode, SyncMode::Fast);
        assert_eq!(screen.network_tab.max_peers, 50);
        assert!(screen.security_tab.wallet_encryption);
        assert!(!screen.security_tab.biometric_auth);
        assert!(!screen.security_tab.two_factor_auth);
        assert!(screen.security_tab.auto_lock);
        assert_eq!(screen.security_tab.lock_timeout, 300);
        assert_eq!(screen.appearance_tab.theme, ThemeMode::System);
        assert_eq!(screen.appearance_tab.font_size, FontSize::Medium);
        assert!(!screen.appearance_tab.compact_mode);
        assert!(screen.appearance_tab.animations);
        assert_eq!(screen.about_tab.version, "3.0.0");
        assert_eq!(screen.about_tab.build_date, "2026-11-22");
        assert_eq!(screen.about_tab.commit_hash, "abc123");
        assert_eq!(screen.about_tab.license, "MIT");
        assert_eq!(screen.about_tab.contributors.len(), 3);
    }

    #[test]
    fn test_settings_screen_tab_switching() {
        let mut screen = SettingsScreen::new();
        assert_eq!(screen.active_tab, SettingsTab::General);
        
        screen.set_active_tab(SettingsTab::Network);
        assert_eq!(screen.active_tab, SettingsTab::Network);
        
        screen.set_active_tab(SettingsTab::Security);
        assert_eq!(screen.active_tab, SettingsTab::Security);
        
        screen.set_active_tab(SettingsTab::Appearance);
        assert_eq!(screen.active_tab, SettingsTab::Appearance);
        
        screen.set_active_tab(SettingsTab::About);
        assert_eq!(screen.active_tab, SettingsTab::About);
    }

    #[test]
    fn test_settings_screen_tab_accessors() {
        let mut screen = SettingsScreen::new();
        
        let general = screen.general_tab_mut();
        general.language = "fr".to_string();
        assert_eq!(general.language, "fr");
        
        let network = screen.network_tab_mut();
        network.port = 8443;
        assert_eq!(network.port, 8443);
        
        let security = screen.security_tab_mut();
        security.two_factor_auth = true;
        assert!(security.two_factor_auth);
        
        let appearance = screen.appearance_tab_mut();
        appearance.compact_mode = true;
        assert!(appearance.compact_mode);
        
        let about = screen.about_tab_mut();
        about.version = "3.1.0".to_string();
        assert_eq!(about.version, "3.1.0");
    }

    #[test]
    fn test_sync_modes() {
        assert_eq!(SyncMode::Full, SyncMode::Full);
        assert_eq!(SyncMode::Fast, SyncMode::Fast);
        assert_eq!(SyncMode::Light, SyncMode::Light);
    }

    #[test]
    fn test_theme_modes() {
        assert_eq!(ThemeMode::Light, ThemeMode::Light);
        assert_eq!(ThemeMode::Dark, ThemeMode::Dark);
        assert_eq!(ThemeMode::System, ThemeMode::System);
    }

    #[test]
    fn test_font_sizes() {
        assert_eq!(FontSize::Small, FontSize::Small);
        assert_eq!(FontSize::Medium, FontSize::Medium);
        assert_eq!(FontSize::Large, FontSize::Large);
    }
}