//! UI-P05: Settings screen WCAG AA gate (invariant #27).

use std::path::PathBuf;
use strangecoin_ui::accessibility::{AccessibilityCIGate, AccessibilityTestSuite, WidgetProbe};
use strangecoin_ui::screens::SettingsScreen;
use strangecoin_ui::theme::{Theme, ThemeManager};

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(PathBuf::from("target/settings-a11y-config.toml"), kind)
}

fn probes(manager: &ThemeManager) -> Vec<WidgetProbe> {
    SettingsScreen::new().a11y_probes(manager)
}

#[test]
fn settings_screen_pass_wcag_aa_in_light_and_dark() {
    for (variant, kind) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let manager = theme(kind);
        let widgets = probes(&manager);
        assert!(
            widgets.len() >= 20,
            "{variant}: settings screen must expose all sub-screen controls"
        );

        let mut suite = AccessibilityTestSuite::new();
        for (index, probe) in widgets.into_iter().enumerate() {
            suite.register_component_test(format!("settings/{variant}/{index}"), probe);
        }
        let results = suite.run_all();
        let report = AccessibilityCIGate::new().generate_ci_report(&results);
        assert!(
            AccessibilityCIGate::new().check_compliance(&results),
            "settings ({variant}) must pass the WCAG AA CI gate:\n{report}"
        );
    }
}

#[test]
fn settings_screen_audits_all_sub_screen_control_groups() {
    let manager = theme(Theme::Light);
    let probes = probes(&manager);
    let names: Vec<&str> = probes
        .iter()
        .map(|probe| probe.meta.name.as_str())
        .collect();
    for required in [
        "settings-tab-network",
        "settings-tab-peers",
        "settings-tab-keystore",
        "settings-tab-config",
        "settings-tab-mcp",
        "settings-tab-acp",
        "settings-keystore-operator",
        "settings-keystore-agent",
        "settings-peers-ban",
        "settings-peers-add-seed",
        "settings-config-save",
        "settings-mcp-toggle",
        "settings-acp-toggle",
    ] {
        assert!(
            names.contains(&required),
            "settings a11y probe set is missing `{required}`"
        );
    }
}
