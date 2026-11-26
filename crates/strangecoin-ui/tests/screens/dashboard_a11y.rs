//! UI-P04: node dashboard WCAG AA gate (invariant #27).
//!
//! The color-coded peer score is audited both for contrast/focus and for the
//! accompanying textual status (WCAG 1.4.1 — never color alone).

use std::path::PathBuf;
use strangecoin_ui::accessibility::{AccessibilityCIGate, AccessibilityTestSuite, WidgetProbe};
use strangecoin_ui::screens::DashboardScreen;
use strangecoin_ui::theme::{Theme, ThemeManager};

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(PathBuf::from("target/dashboard-a11y-config.toml"), kind)
}

fn probes(manager: &ThemeManager) -> Vec<WidgetProbe> {
    DashboardScreen::new().a11y_probes(manager)
}

#[test]
fn dashboard_panels_pass_wcag_aa_in_light_and_dark() {
    for (variant, kind) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let manager = theme(kind);
        let widgets = probes(&manager);
        assert!(
            widgets.len() >= 20,
            "{variant}: dashboard must expose probes for all four panels"
        );

        let mut suite = AccessibilityTestSuite::new();
        for (index, probe) in widgets.into_iter().enumerate() {
            suite.register_component_test(format!("dashboard/{variant}/{index}"), probe);
        }
        let results = suite.run_all();
        let report = AccessibilityCIGate::new().generate_ci_report(&results);
        assert!(
            AccessibilityCIGate::new().check_compliance(&results),
            "dashboard ({variant}) must pass the WCAG AA CI gate:\n{report}"
        );
    }
}

#[test]
fn dashboard_audits_all_four_panels_and_score_labels() {
    let manager = theme(Theme::Light);
    let probes = probes(&manager);
    let names: Vec<&str> = probes
        .iter()
        .map(|probe| probe.meta.name.as_str())
        .collect();
    for required in [
        "dashboard-peer-id",
        "dashboard-peer-score",
        "dashboard-score-healthy",
        "dashboard-score-warning",
        "dashboard-score-banned",
        "sync-percent",
        "dashboard-mempool-rbf",
        "dashboard-block-detail",
    ] {
        assert!(
            names.contains(&required),
            "dashboard a11y probe set is missing `{required}`"
        );
    }
    for probe in probes
        .iter()
        .filter(|probe| probe.meta.name.as_str().starts_with("dashboard-score-"))
    {
        assert!(
            probe.textual_status,
            "peer score must carry a text label (WCAG 1.4.1)"
        );
    }
}
