//! Internationalization (i18n) system for Strangecoin UI
//!
//! Uses the fluent crate for L20n-style localization with support for
//! en/ru/zh locales and fallback to English.

use fluent::concurrent::FluentBundle;
use fluent::{FluentArgs as FluentNativeArgs, FluentResource};
use std::collections::HashMap;
use std::fs;
use unic_langid::{langid, LanguageIdentifier};

/// Supported locales
const SUPPORTED_LOCALES: &[LanguageIdentifier] = &[
    langid!("en"),
    langid!("ru"),
    langid!("zh"),
];

/// Default locale
const DEFAULT_LOCALE: LanguageIdentifier = langid!("en");

/// Localization manager
pub struct Localization {
    current_locale: LanguageIdentifier,
    /// Pre-rendered BCP-47 tag for the current locale (`current_locale()` accessor).
    locale_tag: String,
    bundle: FluentBundle<FluentResource>,
    fallback_bundle: FluentBundle<FluentResource>,
}

impl Localization {
    pub fn new() -> Self {
        let current_locale = Self::detect_system_locale().unwrap_or(DEFAULT_LOCALE);

        // Load bundles for current locale and fallback
        let (bundle, fallback_bundle) = Self::load_bundles(&current_locale);
        let locale_tag = current_locale.to_string();

        Self {
            current_locale,
            locale_tag,
            bundle,
            fallback_bundle,
        }
    }

    pub fn set_locale(&mut self, locale: &str) -> Result<(), String> {
        let lang_id: LanguageIdentifier = locale
            .parse()
            .map_err(|e| format!("Invalid locale: {}", e))?;

        if !SUPPORTED_LOCALES.contains(&lang_id) {
            return Err(format!("Unsupported locale: {}", locale));
        }

        let (bundle, fallback_bundle) = Self::load_bundles(&lang_id);

        self.locale_tag = lang_id.to_string();
        self.current_locale = lang_id;
        self.bundle = bundle;
        self.fallback_bundle = fallback_bundle;

        Ok(())
    }

    /// Alias used by the strangler launcher (`en`/`ru`/`zh`).
    pub fn set_language(&mut self, locale: &str) -> Result<(), String> {
        self.set_locale(locale)
    }

    pub fn current_locale(&self) -> &str {
        &self.locale_tag
    }

    pub fn available_locales() -> Vec<String> {
        SUPPORTED_LOCALES.iter().map(|lang| lang.to_string()).collect()
    }

    /// Translate a message key with optional arguments
    pub fn tr(&self, key: &str, args: HashMap<&str, String>) -> String {
        // Try current locale first
        if let Some(message) = self.try_translate(&self.bundle, key, &args) {
            return message;
        }

        // Fall back to default locale
        if let Some(message) = self.try_translate(&self.fallback_bundle, key, &args) {
            return message;
        }

        // If not found, return the key itself (fallback for missing translations)
        format!("{{{{{}}}}}", key)
    }

    /// Try to translate a message from a bundle
    fn try_translate(
        &self,
        bundle: &FluentBundle<FluentResource>,
        key: &str,
        args: &HashMap<&str, String>,
    ) -> Option<String> {
        let mut errors = Vec::new();

        if let Some(message) = bundle.get_message(key) {
            if let Some(pattern) = message.value() {
                let mut inter = FluentNativeArgs::new();
                for (arg_key, arg_value) in args {
                    inter.set(*arg_key, fluent::FluentValue::from(arg_value.as_str()));
                }

                return Some(
                    bundle
                        .format_pattern(pattern, Some(&inter), &mut errors)
                        .to_string(),
                );
            }
        }

        if !errors.is_empty() {
            tracing::warn!("Translation error for key '{}': {:?}", key, errors);
        }

        None
    }

    /// Detect system locale
    fn detect_system_locale() -> Option<LanguageIdentifier> {
        // Simplified: check environment variables (platform APIs land with UI-P05 polish)
        if let Ok(lang) = std::env::var("LANG") {
            if let Ok(lang_id) = lang.parse::<LanguageIdentifier>() {
                if SUPPORTED_LOCALES.contains(&lang_id) {
                    return Some(lang_id);
                }
            }
        }

        None
    }

    /// Load fluent bundles for locale
    fn load_bundles(
        locale: &LanguageIdentifier,
    ) -> (FluentBundle<FluentResource>, FluentBundle<FluentResource>) {
        let current_bundle = Self::load_bundle(locale);
        let fallback_bundle = Self::load_bundle(&DEFAULT_LOCALE);

        (current_bundle, fallback_bundle)
    }

    /// Load a single fluent bundle
    fn load_bundle(locale: &LanguageIdentifier) -> FluentBundle<FluentResource> {
        let mut bundle = FluentBundle::new_concurrent(vec![locale.clone()]);

        let resource_path = Self::get_resource_path(locale);
        if let Ok(resource) = Self::load_resource(&resource_path) {
            if let Err(e) = bundle.add_resource(resource) {
                tracing::error!("Failed to add resource: {:?}", e);
            }
        } else {
            tracing::warn!("Failed to load resource file: {}", resource_path);
        }

        bundle
    }

    /// Get resource file path for locale
    fn get_resource_path(locale: &LanguageIdentifier) -> String {
        format!("locales/{}/main.ftl", locale)
    }

    /// Load fluent resource from file
    fn load_resource(path: &str) -> Result<FluentResource, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        FluentResource::try_new(content)
            .map_err(|e| format!("Failed to parse fluent resource: {:?}", e.1))
    }

    // ------------------------------------------------------------------
    // Convenience accessors (screens call these directly on Localization)
    // ------------------------------------------------------------------

    fn t(&self, key: &str) -> String {
        self.tr(key, HashMap::new())
    }

    // Wallet
    pub fn wallet_title(&self) -> String {
        self.t("wallet-title")
    }
    pub fn wallet_send_tab(&self) -> String {
        self.t("wallet-send-tab")
    }
    pub fn wallet_history_tab(&self) -> String {
        self.t("wallet-history-tab")
    }
    pub fn wallet_contracts_tab(&self) -> String {
        self.t("wallet-contracts-tab")
    }
    pub fn wallet_send_recipient(&self) -> String {
        self.t("wallet-send-recipient")
    }
    pub fn wallet_send_amount(&self) -> String {
        self.t("wallet-send-amount")
    }
    pub fn wallet_send_fee(&self) -> String {
        self.t("wallet-send-fee")
    }
    pub fn wallet_send_submit(&self) -> String {
        self.t("wallet-send-submit")
    }
    pub fn wallet_send_success(&self, txid: &str) -> String {
        let mut args = HashMap::new();
        args.insert("txid", txid.to_string());
        self.tr("wallet-send-success", args)
    }
    pub fn wallet_send_error_invalid_address(&self) -> String {
        self.t("wallet-send-error-invalid-address")
    }
    pub fn wallet_send_error_insufficient_balance(&self) -> String {
        self.t("wallet-send-error-insufficient-balance")
    }

    // Dashboard
    pub fn dashboard_title(&self) -> String {
        self.t("dashboard-title")
    }
    pub fn dashboard_peers(&self) -> String {
        self.t("dashboard-peers")
    }
    pub fn dashboard_sync(&self) -> String {
        self.t("dashboard-sync")
    }
    pub fn dashboard_mempool(&self) -> String {
        self.t("dashboard-mempool")
    }
    pub fn dashboard_blocks(&self) -> String {
        self.t("dashboard-blocks")
    }
    pub fn dashboard_total_balance(&self) -> String {
        self.t("dashboard-total-balance")
    }
    pub fn dashboard_pending_transactions(&self) -> String {
        self.t("dashboard-pending-transactions")
    }
    pub fn dashboard_network_hashrate(&self) -> String {
        self.t("dashboard-network-hashrate")
    }
    pub fn dashboard_active_agents(&self) -> String {
        self.t("dashboard-active-agents")
    }
    pub fn dashboard_recent_transactions(&self) -> String {
        self.t("dashboard-recent-transactions")
    }

    // Transaction detail
    pub fn tx_detail_title(&self) -> String {
        self.t("tx-detail-title")
    }
    pub fn tx_detail_receipt(&self) -> String {
        self.t("tx-detail-receipt")
    }
    pub fn tx_detail_witness(&self) -> String {
        self.t("tx-detail-witness")
    }
    pub fn tx_detail_events(&self) -> String {
        self.t("tx-detail-events")
    }

    // Agent activity
    pub fn agent_activity_title(&self) -> String {
        self.t("agent-activity-title")
    }
    pub fn agent_activity_filter(&self) -> String {
        self.t("agent-activity-filter")
    }
    pub fn agent_activity_agents(&self) -> String {
        self.t("agent-activity-agents")
    }
    pub fn agent_activity_activities(&self) -> String {
        self.t("agent-activity-activities")
    }
    pub fn agent_activity_total_agents(&self) -> String {
        self.t("agent-activity-total-agents")
    }
    pub fn agent_activity_online_agents(&self) -> String {
        self.t("agent-activity-online-agents")
    }
    pub fn agent_activity_offline_agents(&self) -> String {
        self.t("agent-activity-offline-agents")
    }
    pub fn agent_activity_error_agents(&self) -> String {
        self.t("agent-activity-error-agents")
    }

    // Settings
    pub fn settings_title(&self) -> String {
        self.t("settings-title")
    }
    pub fn settings_general(&self) -> String {
        self.t("settings-general")
    }
    pub fn settings_network(&self) -> String {
        self.t("settings-network")
    }
    pub fn settings_security(&self) -> String {
        self.t("settings-security")
    }
    pub fn settings_peers(&self) -> String {
        self.t("settings-peers")
    }
    pub fn settings_keystore(&self) -> String {
        self.t("settings-keystore")
    }
    pub fn settings_config(&self) -> String {
        self.t("settings-config")
    }
    pub fn settings_mcp(&self) -> String {
        self.t("settings-mcp")
    }
    pub fn settings_acp(&self) -> String {
        self.t("settings-acp")
    }
    pub fn settings_appearance(&self) -> String {
        self.t("settings-appearance")
    }
    pub fn settings_about(&self) -> String {
        self.t("settings-about")
    }

    // Theme
    pub fn theme_light(&self) -> String {
        self.t("theme-light")
    }
    pub fn theme_dark(&self) -> String {
        self.t("theme-dark")
    }
    pub fn theme_system(&self) -> String {
        self.t("theme-system")
    }

    // Buttons / generic
    pub fn button_ok(&self) -> String {
        self.t("button-ok")
    }
    pub fn button_cancel(&self) -> String {
        self.t("button-cancel")
    }
    pub fn button_save(&self) -> String {
        self.t("button-save")
    }
    pub fn button_delete(&self) -> String {
        self.t("button-delete")
    }
    pub fn button_add(&self) -> String {
        self.t("button-add")
    }
    pub fn button_edit(&self) -> String {
        self.t("button-edit")
    }
    pub fn loading(&self) -> String {
        self.t("loading")
    }
    pub fn error(&self) -> String {
        self.t("error")
    }
    pub fn success(&self) -> String {
        self.t("success")
    }
    pub fn warning(&self) -> String {
        self.t("warning")
    }
    pub fn info(&self) -> String {
        self.t("info")
    }
}

impl Default for Localization {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localization_creation() {
        let localization = Localization::new();
        assert!(SUPPORTED_LOCALES.contains(&localization.current_locale));
    }

    #[test]
    fn test_locale_switching() {
        let mut localization = Localization::new();

        // Should succeed for supported locale
        assert!(localization.set_locale("ru").is_ok());
        assert_eq!(localization.current_locale(), "ru");

        // set_language is the strangler-launcher alias
        assert!(localization.set_language("zh").is_ok());
        assert_eq!(localization.current_locale(), "zh");

        // Should fail for unsupported locale
        assert!(localization.set_locale("fr").is_err());
    }

    #[test]
    fn test_fallback_translation() {
        let localization = Localization::new();
        let message = localization.tr("non-existent-key", HashMap::new());
        assert_eq!(message, "{{non-existent-key}}");
    }

    #[test]
    fn test_convenience_accessors_not_empty() {
        let localization = Localization::new();
        assert!(!localization.wallet_title().is_empty());
        assert!(!localization.dashboard_title().is_empty());
        assert!(!localization.settings_title().is_empty());
        assert!(!localization.agent_activity_title().is_empty());
        assert!(!localization.tx_detail_title().is_empty());
    }
}
