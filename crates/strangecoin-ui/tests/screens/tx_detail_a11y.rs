//! UI-P04: transaction detail WCAG AA gate (invariant #27).
//!
//! Pins the four audit panels — Overview, Receipt, Witness and Events — and
//! the explicit Verify control (lazy witness reconstruction).

use std::path::PathBuf;
use strangecoin_ui::accessibility::{AccessibilityCIGate, AccessibilityTestSuite, WidgetProbe};
use strangecoin_ui::screens::TxDetailScreen;
use strangecoin_ui::theme::{Theme, ThemeManager};

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(PathBuf::from("target/tx-detail-a11y-config.toml"), kind)
}

fn probes(manager: &ThemeManager) -> Vec<WidgetProbe> {
    TxDetailScreen::new().a11y_probes(manager)
}

#[test]
fn tx_detail_panels_pass_wcag_aa_in_light_and_dark() {
    for (variant, kind) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let manager = theme(kind);
        let widgets = probes(&manager);
        assert!(
            widgets.len() >= 15,
            "{variant}: tx detail must expose probes for all four panels"
        );

        let mut suite = AccessibilityTestSuite::new();
        for (index, probe) in widgets.into_iter().enumerate() {
            suite.register_component_test(format!("tx_detail/{variant}/{index}"), probe);
        }
        let results = suite.run_all();
        let report = AccessibilityCIGate::new().generate_ci_report(&results);
        assert!(
            AccessibilityCIGate::new().check_compliance(&results),
            "tx detail ({variant}) must pass the WCAG AA CI gate:\n{report}"
        );
    }
}

#[test]
fn tx_detail_audits_all_four_panels_and_verify_control() {
    let manager = theme(Theme::Light);
    let probes = probes(&manager);
    let names: Vec<&str> = probes
        .iter()
        .map(|probe| probe.meta.name.as_str())
        .collect();
    for required in [
        "tx-detail-txid",
        "tx-detail-kind",
        "tx-detail-receipt",
        "tx-detail-verify",
        "tx-detail-witness-account",
        "tx-detail-witness-storage",
        "tx-detail-witness-raw",
        "tx-detail-events",
    ] {
        assert!(
            names.contains(&required),
            "tx detail a11y probe set is missing `{required}`"
        );
    }
}
