//! UI-P05: Agent Activity screen WCAG AA gate (invariant #27).

use std::path::PathBuf;
use strangecoin_ui::accessibility::{AccessibilityCIGate, AccessibilityTestSuite, WidgetProbe};
use strangecoin_ui::screens::AgentActivityScreen;
use strangecoin_ui::theme::{Theme, ThemeManager};

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(
        PathBuf::from("target/agent-activity-a11y-config.toml"),
        kind,
    )
}

fn probes(manager: &ThemeManager) -> Vec<WidgetProbe> {
    AgentActivityScreen::new().a11y_probes(manager)
}

#[test]
fn agent_activity_screen_pass_wcag_aa_in_light_and_dark() {
    for (variant, kind) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let manager = theme(kind);
        let widgets = probes(&manager);
        assert!(
            widgets.len() >= 18,
            "{variant}: agent activity screen must expose feed, filter and ACP controls"
        );

        let mut suite = AccessibilityTestSuite::new();
        for (index, probe) in widgets.into_iter().enumerate() {
            suite.register_component_test(format!("agent_activity/{variant}/{index}"), probe);
        }
        let results = suite.run_all();
        let report = AccessibilityCIGate::new().generate_ci_report(&results);
        assert!(
            AccessibilityCIGate::new().check_compliance(&results),
            "agent activity ({variant}) must pass the WCAG AA CI gate:\n{report}"
        );
    }
}

#[test]
fn agent_activity_screen_audits_all_control_groups() {
    let manager = theme(Theme::Light);
    let probes = probes(&manager);
    let names: Vec<&str> = probes
        .iter()
        .map(|probe| probe.meta.name.as_str())
        .collect();
    for required in [
        "agent-activity-feed",
        "agent-activity-filter",
        "agent-activity-filter-agent",
        "agent-activity-filter-types",
        "agent-activity-filter-from",
        "agent-activity-filter-to",
        "agent-activity-filter-clear",
        "agent-activity-open-tx",
        "agent-activity-open-acp",
        "agent-activity-acp-modal",
    ] {
        assert!(
            names.contains(&required),
            "agent activity a11y probe set is missing `{required}`"
        );
    }
}
