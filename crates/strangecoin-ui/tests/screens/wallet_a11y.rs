//! UI-P03: wallet screen WCAG AA gate (invariant #27).
//!
//! Same deterministic pure-Rust gate as `tests/screens_a11y.rs`, but scoped to
//! the wallet screen's three tabs. Extra assertions pin the controls added by
//! UI-P03 (batch send, history filter, ABI viewer) so an accidental removal
//! cannot silently shrink the audited widget set.

use std::path::PathBuf;
use strangecoin_ui::accessibility::{AccessibilityCIGate, AccessibilityTestSuite, WidgetProbe};
use strangecoin_ui::screens::WalletScreen;
use strangecoin_ui::theme::{Theme, ThemeManager};

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(PathBuf::from("target/wallet-a11y-config.toml"), kind)
}

fn probes(manager: &ThemeManager) -> Vec<WidgetProbe> {
    WalletScreen::new().a11y_probes(manager)
}

#[test]
fn wallet_screen_tabs_pass_wcag_aa_in_light_and_dark() {
    for (variant, kind) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let manager = theme(kind);
        let widgets = probes(&manager);
        assert!(
            widgets.len() >= 13,
            "{variant}: wallet screen must expose all 3 tabs plus send/history/contracts controls"
        );

        let mut suite = AccessibilityTestSuite::new();
        for (index, probe) in widgets.into_iter().enumerate() {
            suite.register_component_test(format!("wallet/{variant}/{index}"), probe);
        }
        let results = suite.run_all();
        let report = AccessibilityCIGate::new().generate_ci_report(&results);
        assert!(
            AccessibilityCIGate::new().check_compliance(&results),
            "wallet ({variant}) must pass the WCAG AA CI gate:\n{report}"
        );
    }
}

#[test]
fn wallet_screen_audits_all_three_tab_control_groups() {
    let manager = theme(Theme::Light);
    let probes = probes(&manager);
    let names: Vec<&str> = probes
        .iter()
        .map(|probe| probe.meta.name.as_str())
        .collect();
    for required in [
        "send",
        "history",
        "contracts",
        "wallet-send-batch",
        "wallet-history-filter",
        "wallet-history-page",
        "wallet-contract-select",
        "wallet-contract-read",
        "wallet-contract-write",
        "wallet-contract-abi",
    ] {
        assert!(
            names.contains(&required),
            "wallet a11y probe set is missing `{required}`"
        );
    }
}
