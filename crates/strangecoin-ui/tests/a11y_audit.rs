use std::path::PathBuf;
use strangecoin_ui::accessibility::{AccessibilityCIGate, AccessibilityTestSuite, WidgetProbe};
use strangecoin_ui::components::{
    Avatar, Badge, Breadcrumb, Button, Card, Chart, ChartType, CodeBlock, Drawer, Input, Modal,
    Stat, Table, Tabs, TextTabContent, Toast, ToastLevel,
};
use strangecoin_ui::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use strangecoin_ui::theme::{Theme, ThemeManager};
use strangecoin_ui::tokens::Color;

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(PathBuf::from("target/a11y-audit-config.toml"), kind)
}

fn component_probes(manager: &ThemeManager) -> Vec<(&'static str, WidgetProbe)> {
    vec![
        (
            "avatar",
            Avatar::new("agent-1", "Alice").a11y_probe(manager),
        ),
        ("badge", Badge::new("status", "Ready").a11y_probe(manager)),
        (
            "breadcrumb",
            Breadcrumb::new("crumbs")
                .item("home", "Home")
                .a11y_probe(manager),
        ),
        ("button", Button::new("OK").a11y_probe(manager)),
        ("card", Card::new().title("Overview").a11y_probe(manager)),
        (
            "chart",
            Chart::new("chart", ChartType::Line)
                .title("Throughput")
                .a11y_probe(manager),
        ),
        (
            "codeblock",
            CodeBlock::new("code", "fn main() {}")
                .language("rust")
                .a11y_probe(manager),
        ),
        (
            "drawer",
            Drawer::new("drawer").title("Details").a11y_probe(manager),
        ),
        ("input", Input::new("Recipient").a11y_probe(manager)),
        (
            "modal",
            Modal::new("modal").title("Confirm").a11y_probe(manager),
        ),
        (
            "stat",
            Stat::new("balance", "Balance", "1000 SC").a11y_probe(manager),
        ),
        (
            "table",
            Table::<String>::new()
                .column("name", "Name")
                .a11y_probe(manager),
        ),
        (
            "tabs",
            Tabs::new("tabs")
                .tab(
                    "overview",
                    "Overview",
                    Box::new(TextTabContent::new("Overview")),
                )
                .a11y_probe(manager),
        ),
        (
            "toast",
            Toast::new("toast", "Saved", ToastLevel::Info).a11y_probe(manager),
        ),
    ]
}

fn suite_for(manager: &ThemeManager) -> AccessibilityTestSuite {
    let mut suite = AccessibilityTestSuite::new();
    for (name, probe) in component_probes(manager) {
        suite.register_component_test(name, probe);
    }
    suite
}

fn assert_compliant(manager: &ThemeManager) {
    let suite = suite_for(manager);
    assert_eq!(suite.component_probes.len(), 14);
    let results = suite.run_all();
    assert_eq!(results.len(), 14);
    let report = AccessibilityCIGate::new().generate_ci_report(&results);
    assert!(
        AccessibilityCIGate::new().check_compliance(&results),
        "CI gate must accept all 14 real component probes:\n{report}"
    );
}

#[test]
fn all_components_pass_ci_gate_in_light_theme() {
    assert_compliant(&theme(Theme::Light));
}

#[test]
fn all_components_pass_ci_gate_in_dark_theme() {
    assert_compliant(&theme(Theme::Dark));
}

#[test]
fn all_components_pass_ci_gate_with_default_theme_manager() {
    assert_compliant(&ThemeManager::new());
}

fn valid_interactive_probe(name: &'static str) -> WidgetProbe {
    WidgetProbe::interactive(
        WidgetMeta::new(name, "button", "OK"),
        Color::new(9, 9, 11),
        Color::new(250, 250, 250),
        MIN_TOUCH_TARGET,
        MIN_TOUCH_TARGET,
    )
    .with_focus(true, FOCUS_OUTLINE_WIDTH)
}

fn run_single(
    probe: WidgetProbe,
) -> std::collections::HashMap<String, strangecoin_ui::accessibility::AccessibilityAuditResult> {
    let mut suite = AccessibilityTestSuite::new();
    suite.register_component_test("bad", probe);
    suite.run_all()
}

#[test]
fn probe_without_focus_ring_fails_gate() {
    let probe = WidgetProbe::interactive(
        WidgetMeta::new("no-focus", "button", "OK"),
        Color::new(9, 9, 11),
        Color::new(250, 250, 250),
        MIN_TOUCH_TARGET,
        MIN_TOUCH_TARGET,
    );
    let results = run_single(probe);
    assert!(results["bad"]
        .violations
        .iter()
        .any(|v| v.id == "focus-visible"));
    assert!(!AccessibilityCIGate::new().check_compliance(&results));
}

#[test]
fn probe_with_tiny_target_fails_gate() {
    let probe = WidgetProbe::interactive(
        WidgetMeta::new("tiny", "button", "OK"),
        Color::new(9, 9, 11),
        Color::new(250, 250, 250),
        20.0,
        20.0,
    )
    .with_focus(true, FOCUS_OUTLINE_WIDTH);
    let results = run_single(probe);
    assert!(results["bad"]
        .violations
        .iter()
        .any(|v| v.id == "target-size"));
    assert!(!AccessibilityCIGate::new().check_compliance(&results));
}

#[test]
fn probe_with_low_contrast_fails_gate() {
    let probe = WidgetProbe::non_interactive(
        WidgetMeta::new("faded", "text", "Low contrast"),
        Color::new(200, 200, 200),
        Color::new(255, 255, 255),
    );
    let results = run_single(probe);
    assert!(results["bad"]
        .violations
        .iter()
        .any(|v| v.id == "color-contrast"));
    assert!(!AccessibilityCIGate::new().check_compliance(&results));
}

#[test]
fn probe_with_empty_name_fails_gate() {
    let mut probe = valid_interactive_probe("unnamed");
    probe.meta.label.clear();
    let results = run_single(probe);
    assert!(results["bad"]
        .violations
        .iter()
        .any(|v| v.id == "button-name" && v.impact == "critical"));
    assert!(!AccessibilityCIGate::new().check_compliance(&results));
}

#[test]
fn empty_suite_fails_closed() {
    let results = AccessibilityTestSuite::new().run_all();
    assert!(results.is_empty());
    assert!(!AccessibilityCIGate::new().check_compliance(&results));
}
