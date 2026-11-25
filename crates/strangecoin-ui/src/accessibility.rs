use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::tokens::Color;
use std::collections::HashMap;

pub const WCAG_AA_CONTRAST: f32 = 4.5;
pub const WCAG_AA_LARGE_CONTRAST: f32 = 3.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityViolation {
    pub id: String,
    pub impact: String,
    pub description: String,
    pub help: String,
    pub help_url: String,
    pub tags: Vec<String>,
    pub nodes: Vec<AccessibilityNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityNode {
    pub html: String,
    pub selector: String,
    pub target: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityAuditResult {
    pub violations: Vec<AccessibilityViolation>,
    pub passes: Vec<String>,
    pub incomplete: Vec<String>,
    pub timestamp: u64,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetProbe {
    pub meta: WidgetMeta,
    pub foreground: Color,
    pub background: Color,
    pub large_text: bool,
    pub focus_ring: bool,
    pub focus_width: f32,
    pub width: f32,
    pub height: f32,
    pub textual_status: bool,
}

impl WidgetProbe {
    pub fn interactive(
        meta: WidgetMeta,
        foreground: Color,
        background: Color,
        width: f32,
        height: f32,
    ) -> Self {
        Self {
            meta,
            foreground,
            background,
            large_text: false,
            focus_ring: false,
            focus_width: 0.0,
            width,
            height,
            textual_status: true,
        }
    }

    pub fn non_interactive(meta: WidgetMeta, foreground: Color, background: Color) -> Self {
        Self {
            meta,
            foreground,
            background,
            large_text: false,
            focus_ring: false,
            focus_width: 0.0,
            width: MIN_TOUCH_TARGET,
            height: MIN_TOUCH_TARGET,
            textual_status: true,
        }
    }

    pub fn with_focus(mut self, ring: bool, width: f32) -> Self {
        self.focus_ring = ring;
        self.focus_width = width;
        self
    }

    pub fn with_large_text(mut self, large: bool) -> Self {
        self.large_text = large;
        self
    }

    pub fn with_textual_status(mut self, textual: bool) -> Self {
        self.textual_status = textual;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityAuditor {
    pub rules: Vec<String>,
    pub exclude_rules: Vec<String>,
    pub min_contrast: f32,
}

impl Default for AccessibilityAuditor {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessibilityAuditor {
    pub fn new() -> Self {
        Self {
            rules: vec![
                "color-contrast".to_string(),
                "button-name".to_string(),
                "focus-visible".to_string(),
                "target-size".to_string(),
                "use-of-color".to_string(),
            ],
            exclude_rules: Vec::new(),
            min_contrast: WCAG_AA_CONTRAST,
        }
    }

    fn violation(
        &self,
        id: &str,
        impact: &str,
        probe: &WidgetProbe,
        detail: &str,
    ) -> AccessibilityViolation {
        AccessibilityViolation {
            id: id.to_string(),
            impact: impact.to_string(),
            description: format!("{}: {}", probe.meta.name, detail),
            help: detail.to_string(),
            help_url: format!("https://www.w3.org/WAI/WCAG22/Understanding/{}", id),
            tags: vec!["wcag2aa".to_string()],
            nodes: vec![AccessibilityNode {
                html: format!("<{} label=\"{}\">", probe.meta.role, probe.meta.label),
                selector: format!("[data-ui-id=\"{}\"]", probe.meta.name),
                target: vec![probe.meta.name.clone()],
                summary: detail.to_string(),
            }],
        }
    }

    pub fn audit_widget(&self, probe: &WidgetProbe) -> AccessibilityAuditResult {
        let mut violations = Vec::new();
        let mut passes = Vec::new();

        let required_contrast = if probe.large_text {
            WCAG_AA_LARGE_CONTRAST
        } else {
            self.min_contrast
        };
        let ratio = probe.foreground.contrast_ratio(&probe.background);
        if ratio + f32::EPSILON < required_contrast {
            violations.push(self.violation(
                "color-contrast",
                "serious",
                probe,
                &format!("contrast {:.2}:1 below {:.2}:1", ratio, required_contrast),
            ));
        } else {
            passes.push("color-contrast".to_string());
        }

        let interactive = probe.meta.is_interactive();
        if interactive {
            if probe.meta.label.trim().is_empty() {
                violations.push(self.violation(
                    "button-name",
                    "critical",
                    probe,
                    "interactive widget has no accessible name",
                ));
            } else {
                passes.push("button-name".to_string());
            }

            if !probe.focus_ring || probe.focus_width + f32::EPSILON < FOCUS_OUTLINE_WIDTH {
                violations.push(self.violation(
                    "focus-visible",
                    "serious",
                    probe,
                    &format!(
                        "focus outline must be at least {:.0}px",
                        FOCUS_OUTLINE_WIDTH
                    ),
                ));
            } else {
                passes.push("focus-visible".to_string());
            }

            if probe.width + f32::EPSILON < MIN_TOUCH_TARGET
                || probe.height + f32::EPSILON < MIN_TOUCH_TARGET
            {
                violations.push(self.violation(
                    "target-size",
                    "serious",
                    probe,
                    &format!(
                        "touch target must be at least {:.0}x{:.0}px",
                        MIN_TOUCH_TARGET, MIN_TOUCH_TARGET
                    ),
                ));
            } else {
                passes.push("target-size".to_string());
            }
        }

        if probe.meta.role == "status" && !probe.textual_status {
            violations.push(self.violation(
                "use-of-color",
                "minor",
                probe,
                "status must be conveyed in text, not color alone",
            ));
        } else {
            passes.push("use-of-color".to_string());
        }

        let total = passes.len() + violations.len();
        let score = if total == 0 {
            0.0
        } else {
            passes.len() as f64 / total as f64 * 100.0
        };

        AccessibilityAuditResult {
            violations,
            passes,
            incomplete: Vec::new(),
            timestamp: 0,
            score,
        }
    }

    pub fn audit_component(&self, _name: &str, probe: &WidgetProbe) -> AccessibilityAuditResult {
        self.audit_widget(probe)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityTestSuite {
    auditor: AccessibilityAuditor,
    pub component_probes: HashMap<String, WidgetProbe>,
}

impl Default for AccessibilityTestSuite {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessibilityTestSuite {
    pub fn new() -> Self {
        Self {
            auditor: AccessibilityAuditor::new(),
            component_probes: HashMap::new(),
        }
    }

    pub fn register_component_test(&mut self, name: impl Into<String>, probe: WidgetProbe) {
        self.component_probes.insert(name.into(), probe);
    }

    pub fn run_all(&self) -> HashMap<String, AccessibilityAuditResult> {
        self.component_probes
            .iter()
            .map(|(name, probe)| (name.clone(), self.auditor.audit_widget(probe)))
            .collect()
    }

    pub fn generate_report(&self, results: &HashMap<String, AccessibilityAuditResult>) -> String {
        let mut report = String::from("# Accessibility Audit Report\n\n");
        let mut names: Vec<&String> = results.keys().collect();
        names.sort();
        for name in names {
            let result = &results[name];
            report.push_str(&format!(
                "## {} — {:.1}% ({} violations)\n",
                name,
                result.score,
                result.violations.len()
            ));
        }
        report
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityCIGate {
    pub threshold_score: f64,
    pub max_violations: usize,
}

impl Default for AccessibilityCIGate {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessibilityCIGate {
    pub fn new() -> Self {
        Self {
            threshold_score: 100.0,
            max_violations: 0,
        }
    }

    pub fn check_compliance(&self, results: &HashMap<String, AccessibilityAuditResult>) -> bool {
        if results.is_empty() {
            return false;
        }
        let mut total_violations = 0;
        for result in results.values() {
            if result.score + f64::EPSILON < self.threshold_score {
                return false;
            }
            if result.violations.iter().any(|v| v.impact == "critical") {
                return false;
            }
            total_violations += result.violations.len();
        }
        total_violations <= self.max_violations
    }

    pub fn generate_ci_report(
        &self,
        results: &HashMap<String, AccessibilityAuditResult>,
    ) -> String {
        let mut report = String::from("# Accessibility CI Gate Report\n\n");
        report.push_str(&format!(
            "Status: {}\n\n",
            if self.check_compliance(results) {
                "PASS"
            } else {
                "FAIL"
            }
        ));
        report.push_str("## Thresholds:\n\n");
        report.push_str(&format!("Score threshold: {:.1}%\n", self.threshold_score));
        report.push_str(&format!("Max violations: {}\n\n", self.max_violations));
        report.push_str("## Results:\n\n");
        let mut names: Vec<&String> = results.keys().collect();
        names.sort();
        for name in names {
            report.push_str(&format!(
                "- {}: {:.1}%, {} violations\n",
                name,
                results[name].score,
                results[name].violations.len()
            ));
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> WidgetProbe {
        WidgetProbe::interactive(
            WidgetMeta::new("test_button", "button", "OK"),
            Color::new(255, 255, 255),
            Color::new(0, 92, 170),
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        )
        .with_focus(true, FOCUS_OUTLINE_WIDTH)
    }

    #[test]
    fn accessible_widget_passes_all_rules() {
        let result = AccessibilityAuditor::new().audit_widget(&probe());
        assert!(result.violations.is_empty());
        assert!((result.score - 100.0).abs() < f64::EPSILON);
        assert_eq!(result.timestamp, 0);
    }

    #[test]
    fn missing_focus_and_small_target_are_violations() {
        let bad = WidgetProbe::interactive(
            WidgetMeta::new("tiny", "button", "go"),
            Color::new(255, 255, 255),
            Color::new(0, 92, 170),
            20.0,
            20.0,
        );
        let result = AccessibilityAuditor::new().audit_widget(&bad);
        let ids: Vec<&str> = result.violations.iter().map(|v| v.id.as_str()).collect();
        assert!(ids.contains(&"focus-visible"));
        assert!(ids.contains(&"target-size"));
    }

    #[test]
    fn low_contrast_is_a_violation() {
        let bad = WidgetProbe::non_interactive(
            WidgetMeta::new("text", "text", "low contrast"),
            Color::new(200, 200, 200),
            Color::new(255, 255, 255),
        );
        let result = AccessibilityAuditor::new().audit_widget(&bad);
        assert!(result.violations.iter().any(|v| v.id == "color-contrast"));
    }

    #[test]
    fn empty_name_is_critical() {
        let bad = WidgetProbe::interactive(
            WidgetMeta::new("unnamed", "button", "  "),
            Color::new(255, 255, 255),
            Color::new(0, 0, 0),
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        );
        let result = AccessibilityAuditor::new().audit_widget(&bad);
        assert!(result
            .violations
            .iter()
            .any(|v| v.id == "button-name" && v.impact == "critical"));
    }

    #[test]
    fn gate_fails_closed_on_empty_results() {
        assert!(!AccessibilityCIGate::new().check_compliance(&HashMap::new()));
    }

    #[test]
    fn score_is_not_inverted() {
        let bad = probe().with_focus(false, 0.0);
        let result = AccessibilityAuditor::new().audit_widget(&bad);
        assert!(result.score < 100.0);
        assert!(!AccessibilityCIGate::new()
            .check_compliance(&HashMap::from([("test_button".to_string(), result,)])));
    }
}
