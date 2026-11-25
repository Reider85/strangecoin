use std::path::PathBuf;
use strangecoin_ui::accessibility::{AccessibilityCIGate, AccessibilityTestSuite, WidgetProbe};
use strangecoin_ui::screens::{
    AgentActivityScreen, DashboardScreen, SettingsScreen, TxDetailScreen, WalletScreen,
};
use strangecoin_ui::theme::{Theme, ThemeManager};

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(PathBuf::from("target/a11y-screens-config.toml"), kind)
}

fn assert_screen_probes(name: &str, probes_for: impl Fn(&ThemeManager) -> Vec<WidgetProbe>) {
    for (variant, kind) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let manager = theme(kind);
        let probes = probes_for(&manager);
        assert!(!probes.is_empty(), "{name} must expose at least one probe");

        let mut suite = AccessibilityTestSuite::new();
        for (index, probe) in probes.into_iter().enumerate() {
            suite.register_component_test(format!("{name}/{variant}/{index}"), probe);
        }
        let results = suite.run_all();
        let report = AccessibilityCIGate::new().generate_ci_report(&results);
        assert!(
            AccessibilityCIGate::new().check_compliance(&results),
            "{name} ({variant}) screen probes must pass the CI gate:\n{report}"
        );
    }
}

#[test]
fn wallet_screen_a11y_probes_pass_gate() {
    assert_screen_probes("wallet", |manager| WalletScreen::new().a11y_probes(manager));
}

#[test]
fn dashboard_screen_a11y_probes_pass_gate() {
    assert_screen_probes("dashboard", |manager| {
        DashboardScreen::new().a11y_probes(manager)
    });
}

#[test]
fn tx_detail_screen_a11y_probes_pass_gate() {
    assert_screen_probes("tx_detail", |manager| {
        TxDetailScreen::new().a11y_probes(manager)
    });
}

#[test]
fn agent_activity_screen_a11y_probes_pass_gate() {
    assert_screen_probes("agent_activity", |manager| {
        AgentActivityScreen::new().a11y_probes(manager)
    });
}

#[test]
fn settings_screen_a11y_probes_pass_gate() {
    assert_screen_probes("settings", |manager| {
        SettingsScreen::new().a11y_probes(manager)
    });
}
