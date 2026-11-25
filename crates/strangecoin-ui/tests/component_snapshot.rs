use std::path::PathBuf;
use strangecoin_ui::accessibility::WidgetProbe;
use strangecoin_ui::components::{
    Avatar, Badge, Breadcrumb, Button, Card, Chart, ChartType, CodeBlock, Drawer, Input, Modal,
    Stat, Table, Tabs, TextTabContent, Toast, ToastLevel,
};
use strangecoin_ui::theme::{Theme, ThemeManager};

fn theme(kind: Theme) -> ThemeManager {
    ThemeManager::with_theme(PathBuf::from("target/a11y-snapshot-config.toml"), kind)
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

fn component_style(probe: &WidgetProbe) -> String {
    format!(
        "role={}\nlabel={}\nforeground={}\nbackground={}\nwidth={:.1}\nheight={:.1}\nfocus={}\n",
        probe.meta.role,
        probe.meta.label,
        probe.foreground.as_css_hex(),
        probe.background.as_css_hex(),
        probe.width,
        probe.height,
        probe.focus_ring
    )
}

#[test]
fn component_styles_are_stable_in_light_and_dark_themes() {
    for (variant, kind) in [("light", Theme::Light), ("dark", Theme::Dark)] {
        let manager = theme(kind);
        let probes = component_probes(&manager);
        assert_eq!(probes.len(), 14);

        for (name, probe) in probes {
            let snapshot_name = format!("{variant}_{name}");
            insta::assert_snapshot!(snapshot_name, component_style(&probe));
        }
    }
}
