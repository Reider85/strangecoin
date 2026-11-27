//! Internationalization (i18n) system for Strangecoin UI
//!
//! Uses the fluent crate for L20n-style localization with support for
//! en/ru/zh locales and fallback to English.

use fluent::concurrent::FluentBundle;
use fluent::{FluentArgs as FluentNativeArgs, FluentResource};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use unic_langid::{langid, LanguageIdentifier};

/// Supported locales
const SUPPORTED_LOCALES: &[LanguageIdentifier] = &[langid!("en"), langid!("ru"), langid!("zh")];

/// Default locale
const DEFAULT_LOCALE: LanguageIdentifier = langid!("en");

macro_rules! ui_p05_accessors {
    ($($method:ident => $key:literal),* $(,)?) => {
        $(
            pub fn $method(&self) -> String {
                self.t($key)
            }
        )*
    };
}

/// Localization manager
pub struct Localization {
    current_locale: LanguageIdentifier,
    /// Pre-rendered BCP-47 tag for the current locale (`current_locale()` accessor).
    locale_tag: String,
    bundle: FluentBundle<FluentResource>,
    fallback_bundle: FluentBundle<FluentResource>,
    resource_root: PathBuf,
}

impl Localization {
    pub fn new() -> Self {
        Self::with_root(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    }

    pub fn with_root(resource_root: PathBuf) -> Self {
        let current_locale = Self::detect_system_locale().unwrap_or(DEFAULT_LOCALE);
        Self::with_locale_and_root(&current_locale.to_string(), resource_root.clone())
            .unwrap_or_else(|_| Self::fallback_with_root(resource_root))
    }

    fn fallback_with_root(resource_root: PathBuf) -> Self {
        let current_locale = DEFAULT_LOCALE;
        let locale_tag = current_locale.to_string();
        let (bundle, fallback_bundle) = Self::load_bundles(&current_locale, &resource_root);
        Self {
            current_locale,
            locale_tag,
            bundle,
            fallback_bundle,
            resource_root,
        }
    }

    pub fn with_locale_and_root(locale: &str, resource_root: PathBuf) -> Result<Self, String> {
        let current_locale: LanguageIdentifier = locale.parse().map_err(|e| format!("{e}"))?;
        if !SUPPORTED_LOCALES.contains(&current_locale) {
            return Err(format!("Unsupported locale: {locale}"));
        }
        let (bundle, fallback_bundle) = Self::load_bundles(&current_locale, &resource_root);
        Ok(Self {
            locale_tag: current_locale.to_string(),
            current_locale,
            bundle,
            fallback_bundle,
            resource_root,
        })
    }

    pub fn set_locale(&mut self, locale: &str) -> Result<(), String> {
        let lang_id: LanguageIdentifier = locale
            .parse()
            .map_err(|e| format!("Invalid locale: {}", e))?;

        if !SUPPORTED_LOCALES.contains(&lang_id) {
            return Err(format!("Unsupported locale: {}", locale));
        }

        let (bundle, fallback_bundle) = Self::load_bundles(&lang_id, &self.resource_root);

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
        SUPPORTED_LOCALES
            .iter()
            .map(|lang| lang.to_string())
            .collect()
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
        root: &Path,
    ) -> (FluentBundle<FluentResource>, FluentBundle<FluentResource>) {
        let current_bundle = Self::load_bundle(locale, root);
        let fallback_bundle = Self::load_bundle(&DEFAULT_LOCALE, root);

        (current_bundle, fallback_bundle)
    }

    /// Load a single fluent bundle
    fn load_bundle(locale: &LanguageIdentifier, root: &Path) -> FluentBundle<FluentResource> {
        let mut bundle = FluentBundle::new_concurrent(vec![locale.clone()]);

        let resource_path = Self::get_resource_path(locale, root);
        if let Ok(resource) = Self::load_resource(&resource_path) {
            if let Err(e) = bundle.add_resource(resource) {
                tracing::error!("Failed to add resource: {:?}", e);
            }
        } else {
            tracing::warn!("Failed to load resource file: {}", resource_path.display());
        }

        bundle
    }

    /// Get resource file path for locale
    fn get_resource_path(locale: &LanguageIdentifier, root: &Path) -> PathBuf {
        root.join("locales")
            .join(locale.to_string())
            .join("main.ftl")
    }

    /// Load fluent resource from file
    fn load_resource(path: &Path) -> Result<FluentResource, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file {}: {e}", path.display()))?;

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
    pub fn wallet_balance(&self) -> String {
        self.t("wallet-balance")
    }
    pub fn wallet_send_form_title(&self) -> String {
        self.t("wallet-send-form-title")
    }
    pub fn wallet_send_batch(&self) -> String {
        self.t("wallet-send-batch")
    }
    pub fn wallet_send_batch_unavailable(&self) -> String {
        self.t("wallet-send-batch-unavailable")
    }
    pub fn wallet_recent_transactions(&self) -> String {
        self.t("wallet-recent-transactions")
    }
    pub fn wallet_deployed_contracts(&self) -> String {
        self.t("wallet-deployed-contracts")
    }
    pub fn wallet_view_abi(&self) -> String {
        self.t("wallet-view-abi")
    }
    pub fn wallet_interact(&self) -> String {
        self.t("wallet-interact")
    }
    pub fn wallet_empty_history(&self) -> String {
        self.t("wallet-empty-history")
    }
    pub fn wallet_empty_contracts(&self) -> String {
        self.t("wallet-empty-contracts")
    }
    pub fn wallet_history_filter_all(&self) -> String {
        self.t("wallet-history-filter-all")
    }
    pub fn wallet_history_filter_sent(&self) -> String {
        self.t("wallet-history-filter-sent")
    }
    pub fn wallet_history_filter_received(&self) -> String {
        self.t("wallet-history-filter-received")
    }
    pub fn wallet_history_filter_failed(&self) -> String {
        self.t("wallet-history-filter-failed")
    }
    pub fn wallet_history_col_txid(&self) -> String {
        self.t("wallet-history-col-txid")
    }
    pub fn wallet_history_col_type(&self) -> String {
        self.t("wallet-history-col-type")
    }
    pub fn wallet_history_col_from(&self) -> String {
        self.t("wallet-history-col-from")
    }
    pub fn wallet_history_col_to(&self) -> String {
        self.t("wallet-history-col-to")
    }
    pub fn wallet_history_col_amount(&self) -> String {
        self.t("wallet-history-col-amount")
    }
    pub fn wallet_history_col_fee(&self) -> String {
        self.t("wallet-history-col-fee")
    }
    pub fn wallet_history_col_status(&self) -> String {
        self.t("wallet-history-col-status")
    }
    pub fn wallet_history_col_height(&self) -> String {
        self.t("wallet-history-col-height")
    }
    pub fn wallet_history_prev(&self) -> String {
        self.t("wallet-history-prev")
    }
    pub fn wallet_history_next(&self) -> String {
        self.t("wallet-history-next")
    }
    pub fn wallet_contracts_read(&self) -> String {
        self.t("wallet-contracts-read")
    }
    pub fn wallet_contracts_write(&self) -> String {
        self.t("wallet-contracts-write")
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
    pub fn dashboard_tip_height(&self) -> String {
        self.t("dashboard-tip-height")
    }
    pub fn dashboard_sync_percent(&self, percent: u64) -> String {
        let mut args = HashMap::new();
        args.insert("percent", percent.to_string());
        self.tr("dashboard-sync-percent", args)
    }
    pub fn dashboard_connected_peers(&self, count: usize) -> String {
        let mut args = HashMap::new();
        args.insert("count", count.to_string());
        self.tr("dashboard-connected-peers", args)
    }
    pub fn dashboard_uptime(&self) -> String {
        self.t("dashboard-uptime")
    }
    pub fn dashboard_version(&self) -> String {
        self.t("dashboard-version")
    }
    pub fn dashboard_current_height(&self) -> String {
        self.t("dashboard-current-height")
    }
    pub fn dashboard_sync_state(&self) -> String {
        self.t("dashboard-sync-state")
    }
    pub fn dashboard_headers_queue(&self) -> String {
        self.t("dashboard-headers-queue")
    }
    pub fn dashboard_blocks_queue(&self) -> String {
        self.t("dashboard-blocks-queue")
    }
    pub fn dashboard_score_healthy(&self) -> String {
        self.t("dashboard-score-healthy")
    }
    pub fn dashboard_score_warning(&self) -> String {
        self.t("dashboard-score-warning")
    }
    pub fn dashboard_score_banned(&self) -> String {
        self.t("dashboard-score-banned")
    }
    pub fn dashboard_no_peers(&self) -> String {
        self.t("dashboard-no-peers")
    }
    pub fn dashboard_peer_id(&self) -> String {
        self.t("dashboard-peer-id")
    }
    pub fn dashboard_peer_addr(&self) -> String {
        self.t("dashboard-peer-addr")
    }
    pub fn dashboard_peer_latency(&self) -> String {
        self.t("dashboard-peer-latency")
    }
    pub fn dashboard_peer_score(&self) -> String {
        self.t("dashboard-peer-score")
    }
    pub fn dashboard_peer_direction(&self) -> String {
        self.t("dashboard-peer-direction")
    }
    pub fn dashboard_peer_capabilities(&self) -> String {
        self.t("dashboard-peer-capabilities")
    }
    pub fn dashboard_total_fee(&self) -> String {
        self.t("dashboard-total-fee")
    }
    pub fn dashboard_no_mempool(&self) -> String {
        self.t("dashboard-no-mempool")
    }
    pub fn dashboard_mempool_txid(&self) -> String {
        self.t("dashboard-mempool-txid")
    }
    pub fn dashboard_mempool_from(&self) -> String {
        self.t("dashboard-mempool-from")
    }
    pub fn dashboard_mempool_to(&self) -> String {
        self.t("dashboard-mempool-to")
    }
    pub fn dashboard_mempool_amount(&self) -> String {
        self.t("dashboard-mempool-amount")
    }
    pub fn dashboard_mempool_fee(&self) -> String {
        self.t("dashboard-mempool-fee")
    }
    pub fn dashboard_mempool_gas(&self) -> String {
        self.t("dashboard-mempool-gas")
    }
    pub fn dashboard_mempool_rbf(&self) -> String {
        self.t("dashboard-mempool-rbf")
    }
    pub fn dashboard_no_blocks(&self) -> String {
        self.t("dashboard-no-blocks")
    }
    pub fn dashboard_block_height(&self) -> String {
        self.t("dashboard-block-height")
    }
    pub fn dashboard_block_hash(&self) -> String {
        self.t("dashboard-block-hash")
    }
    pub fn dashboard_block_txs(&self) -> String {
        self.t("dashboard-block-txs")
    }
    pub fn dashboard_block_gas(&self) -> String {
        self.t("dashboard-block-gas")
    }
    pub fn dashboard_block_miner(&self) -> String {
        self.t("dashboard-block-miner")
    }
    pub fn dashboard_block_timestamp(&self) -> String {
        self.t("dashboard-block-timestamp")
    }
    pub fn dashboard_block_tx_list(&self) -> String {
        self.t("dashboard-block-tx-list")
    }
    pub fn dashboard_block_detail(&self) -> String {
        self.t("dashboard-block-detail")
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
    pub fn tx_detail_overview(&self) -> String {
        self.t("tx-detail-overview")
    }
    pub fn tx_detail_status(&self) -> String {
        self.t("tx-detail-status")
    }
    pub fn tx_detail_block(&self) -> String {
        self.t("tx-detail-block")
    }
    pub fn tx_detail_from(&self) -> String {
        self.t("tx-detail-from")
    }
    pub fn tx_detail_to(&self) -> String {
        self.t("tx-detail-to")
    }
    pub fn tx_detail_amount(&self) -> String {
        self.t("tx-detail-amount")
    }
    pub fn tx_detail_fee(&self) -> String {
        self.t("tx-detail-fee")
    }
    pub fn tx_detail_gas_used(&self) -> String {
        self.t("tx-detail-gas-used")
    }
    pub fn tx_detail_timestamp(&self) -> String {
        self.t("tx-detail-timestamp")
    }
    pub fn tx_detail_input_data(&self) -> String {
        self.t("tx-detail-input-data")
    }
    pub fn tx_detail_verify(&self) -> String {
        self.t("tx-detail-verify")
    }
    pub fn tx_detail_load(&self) -> String {
        self.t("tx-detail-load")
    }
    pub fn tx_detail_not_found(&self) -> String {
        self.t("tx-detail-not-found")
    }
    pub fn tx_detail_txid(&self) -> String {
        self.t("tx-detail-txid")
    }
    pub fn tx_detail_kind(&self) -> String {
        self.t("tx-detail-kind")
    }
    pub fn tx_detail_status_pending(&self) -> String {
        self.t("tx-detail-status-pending")
    }
    pub fn tx_detail_copy(&self) -> String {
        self.t("tx-detail-copy")
    }
    pub fn tx_detail_gas_limit(&self) -> String {
        self.t("tx-detail-gas-limit")
    }
    pub fn tx_detail_revert(&self) -> String {
        self.t("tx-detail-revert")
    }
    pub fn tx_detail_events_empty(&self) -> String {
        self.t("tx-detail-events-empty")
    }
    pub fn tx_detail_receipt_unavailable(&self) -> String {
        self.t("tx-detail-receipt-unavailable")
    }
    pub fn tx_detail_verified(&self) -> String {
        self.t("tx-detail-verified")
    }
    pub fn tx_detail_verify_failed(&self) -> String {
        self.t("tx-detail-verify-failed")
    }
    pub fn tx_detail_witness_unavailable(&self) -> String {
        self.t("tx-detail-witness-unavailable")
    }
    pub fn tx_detail_witness_account(&self) -> String {
        self.t("tx-detail-witness-account")
    }
    pub fn tx_detail_witness_storage(&self) -> String {
        self.t("tx-detail-witness-storage")
    }
    pub fn tx_detail_witness_raw(&self) -> String {
        self.t("tx-detail-witness-raw")
    }
    pub fn tx_detail_event_topics(&self) -> String {
        self.t("tx-detail-event-topics")
    }
    pub fn tx_detail_event_data(&self) -> String {
        self.t("tx-detail-event-data")
    }
    pub fn tx_detail_no_selection(&self) -> String {
        self.t("tx-detail-no-selection")
    }
    pub fn tx_detail_loading(&self) -> String {
        self.t("tx-detail-loading")
    }
    pub fn status_confirmed(&self) -> String {
        self.t("status-confirmed")
    }
    pub fn status_failed(&self) -> String {
        self.t("status-failed")
    }
    pub fn status_pending(&self) -> String {
        self.t("status-pending")
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

    ui_p05_accessors! {
        agent_activity_feed_title => "agent-activity-feed-title",
        agent_activity_filter_title => "agent-activity-filter-title",
        agent_activity_filter_agent => "agent-activity-filter-agent",
        agent_activity_filter_types => "agent-activity-filter-types",
        agent_activity_filter_from => "agent-activity-filter-from",
        agent_activity_filter_to => "agent-activity-filter-to",
        agent_activity_filter_clear => "agent-activity-filter-clear",
        agent_activity_col_time => "agent-activity-col-time",
        agent_activity_col_agent => "agent-activity-col-agent",
        agent_activity_col_type => "agent-activity-col-type",
        agent_activity_col_details => "agent-activity-col-details",
        agent_activity_stub => "agent-activity-stub",
        agent_activity_open_tx => "agent-activity-open-tx",
        agent_activity_open_acp => "agent-activity-open-acp",
        agent_activity_acp_detail => "agent-activity-acp-detail",
        agent_activity_no_match => "agent-activity-no-match",
        agent_event_acp_negotiate => "agent-event-acp-negotiate",
        agent_event_acp_task => "agent-event-acp-task",
        agent_event_acp_settle => "agent-event-acp-settle",
        agent_event_acp_revoke => "agent-event-acp-revoke",
        agent_event_batched_tx => "agent-event-batched-tx",
        agent_event_sponsor_tx => "agent-event-sponsor-tx",
        agent_event_transfer => "agent-event-transfer",
        settings_network_listen => "settings-network-listen",
        settings_network_seeds => "settings-network-seeds",
        settings_network_max_peers => "settings-network-max-peers",
        settings_network_id => "settings-network-id",
        settings_read_only => "settings-read-only",
        settings_peers_ban => "settings-peers-ban",
        settings_peers_unban => "settings-peers-unban",
        settings_peers_add_seed => "settings-peers-add-seed",
        settings_peers_banned => "settings-peers-banned",
        settings_keystore_operator => "settings-keystore-operator",
        settings_keystore_agent => "settings-keystore-agent",
        settings_keystore_import => "settings-keystore-import",
        settings_keystore_export => "settings-keystore-export",
        settings_keystore_change_password => "settings-keystore-change-password",
        settings_keystore_derive => "settings-keystore-derive",
        settings_keystore_unavailable => "settings-keystore-unavailable",
        settings_config_raw => "settings-config-raw",
        settings_config_validate => "settings-config-validate",
        settings_config_save_restart => "settings-config-save-restart",
        settings_config_invalid => "settings-config-invalid",
        settings_mcp_enable => "settings-mcp-enable",
        settings_mcp_transport => "settings-mcp-transport",
        settings_mcp_listen => "settings-mcp-listen",
        settings_mcp_clients => "settings-mcp-clients",
        settings_acp_enable => "settings-acp-enable",
        settings_acp_sessions => "settings-acp-sessions",
        settings_acp_ban_agent => "settings-acp-ban-agent",
        settings_status_saved => "settings-status-saved",
        settings_status_error => "settings-status-error",
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

    #[test]
    fn manifest_root_loads_real_translations_for_all_locales() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut key_counts = Vec::new();
        for locale in ["en", "ru", "zh"] {
            let localization = Localization::with_locale_and_root(locale, root.clone())
                .expect("supported locale must load");
            let title = localization.wallet_title();
            assert!(!title.is_empty());
            assert!(!title.starts_with("{{"), "locale {locale} fell back to key");
            assert_eq!(localization.current_locale(), locale);

            let path = root.join("locales").join(locale).join("main.ftl");
            let content = fs::read_to_string(path).expect("locale file");
            let count = content
                .lines()
                .filter(|line| line.contains('=') && !line.trim_start().starts_with('#'))
                .count();
            key_counts.push(count);
        }
        assert_eq!(key_counts[0], key_counts[1]);
        assert_eq!(key_counts[1], key_counts[2]);
        assert!(key_counts[0] >= 100);
    }
}
