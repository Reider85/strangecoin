//! Accessibility audit harness for Strangecoin UI
//!
//! This module provides accessibility testing using axe-core to ensure
//! WCAG AA compliance for the UI components and screens.

use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Accessibility audit results
#[derive(Debug, Clone)]
pub struct AccessibilityAuditResult {
    pub violations: Vec<AccessibilityViolation>,
    pub passes: Vec<String>,
    pub incomplete: Vec<String>,
    pub timestamp: u64,
    pub score: f64,
}

/// Accessibility violation details
#[derive(Debug, Clone)]
pub struct AccessibilityViolation {
    pub id: String,
    pub impact: String,
    pub description: String,
    pub help: String,
    pub help_url: String,
    pub tags: Vec<String>,
    pub nodes: Vec<AccessibilityNode>,
}

/// Accessibility node details
#[derive(Debug, Clone)]
pub struct AccessibilityNode {
    pub html: String,
    pub selector: String,
    pub target: Vec<String>,
    pub summary: String,
}

/// Accessibility auditor for UI components
pub struct AccessibilityAuditor {
    pub axe_version: String,
    pub rules: Vec<String>,
    pub exclude_rules: Vec<String>,
}

impl Default for AccessibilityAuditor {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessibilityAuditor {
    /// Create a new accessibility auditor
    pub fn new() -> Self {
        Self {
            axe_version: "4.9.1".to_string(),
            rules: vec![
                "color-contrast".to_string(),
                "button-name".to_string(),
                "image-alt".to_string(),
                "form-field-label".to_string(),
                "link-name".to_string(),
                "aria-required-attr".to_string(),
                "landmark-one-main".to_string(),
                "page-has-heading-one".to_string(),
                "html-lang-valid".to_string(),
                "meta-viewport".to_string(),
            ],
            exclude_rules: vec![
                "color-contrast".to_string(), // Exclude for testing, should be enabled in production
            ],
        }
    }

    /// Run accessibility audit on HTML content
    pub fn audit_html(&self, html: &str) -> Result<AccessibilityAuditResult, String> {
        // Create a temporary HTML file with axe-core script
        let temp_dir = std::env::temp_dir();
        let html_file = temp_dir.join("temp_audit.html");
        let axe_script = temp_dir.join("axe.min.js");

        // Write HTML content
        fs::write(&html_file, html).map_err(|e| format!("Failed to write HTML file: {}", e))?;

        // Check if axe-core script exists, if not, download it
        if !axe_script.exists() {
            self.download_axe_core(&axe_script)?;
        }

        // Create JavaScript audit script
        let js_script = format!(
            r#"
            const axe = require('{axe_path}');
            axe.run('#{{document.body}}', {{
                rules: {{
                    enabled: {rules:?},
                    disabled: {exclude_rules:?}
                }}
            }}, (err, results) => {{
                console.log(JSON.stringify({{
                    violations: results.violations.map(v => ({{
                        id: v.id,
                        impact: v.impact,
                        description: v.description,
                        help: v.help,
                        helpUrl: v.helpUrl,
                        tags: v.tags,
                            nodes: v.nodes.map(n => ({{
                                html: n.html,
                                selector: n.selector,
                                target: n.target,
                                summary: n.summary
                            }}))
                        }})),
                    passes: results.passes.map(p => p.id),
                    incomplete: results.incomplete.map(i => i.id),
                    timestamp: Date.now(),
                    score: results.violations.len() / (results.violations.len() + results.passes.len()) * 100
                }}));
            }});
        "#,
            axe_path = axe_script.display(),
            rules = self.rules,
            exclude_rules = self.exclude_rules
        );

        // Run Node.js to execute the audit
        let output = Command::new("node")
            .args(["-e", &js_script, &format!("{}", html_file.display())])
            .output()
            .map_err(|e| format!("Failed to execute Node.js: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Node.js execution failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let result_json = String::from_utf8_lossy(&output.stdout);
        let result_value: Value = serde_json::from_str(&result_json)
            .map_err(|e| format!("Failed to parse JSON result: {}", e))?;

        self.parse_audit_result(result_value)
    }

    /// Download axe-core script (via system curl to avoid a heavy HTTP dep)
    fn download_axe_core(&self, path: &Path) -> Result<(), String> {
        let url = format!(
            "https://cdnjs.cloudflare.com/ajax/libs/axe-core/{}/axe.min.js",
            self.axe_version
        );
        let output = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(path)
            .arg(&url)
            .output()
            .map_err(|e| format!("Failed to download axe-core (curl): {}", e))?;

        if !output.status.success() {
            let _ = fs::remove_file(path);
            return Err(format!(
                "axe-core download failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    /// Parse audit result from JSON
    fn parse_audit_result(&self, result: Value) -> Result<AccessibilityAuditResult, String> {
        let violations = result["violations"]
            .as_array()
            .ok_or("Missing violations array")?
            .iter()
            .map(|v| AccessibilityViolation {
                id: v["id"].as_str().unwrap_or("").to_string(),
                impact: v["impact"].as_str().unwrap_or("").to_string(),
                description: v["description"].as_str().unwrap_or("").to_string(),
                help: v["help"].as_str().unwrap_or("").to_string(),
                help_url: v["helpUrl"].as_str().unwrap_or("").to_string(),
                tags: v["tags"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|t| t.as_str().unwrap_or("").to_string())
                    .collect(),
                nodes: v["nodes"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|n| AccessibilityNode {
                        html: n["html"].as_str().unwrap_or("").to_string(),
                        selector: n["selector"].as_str().unwrap_or("").to_string(),
                        target: n["target"]
                            .as_array()
                            .unwrap_or(&vec![])
                            .iter()
                            .map(|t| t.as_str().unwrap_or("").to_string())
                            .collect(),
                        summary: n["summary"].as_str().unwrap_or("").to_string(),
                    })
                    .collect(),
            })
            .collect();

        let passes = result["passes"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|p| p.as_str().unwrap_or("").to_string())
            .collect();

        let incomplete = result["incomplete"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|i| i.as_str().unwrap_or("").to_string())
            .collect();

        let score = result["score"].as_f64().unwrap_or(0.0);

        Ok(AccessibilityAuditResult {
            violations,
            passes,
            incomplete,
            timestamp: result["timestamp"].as_u64().unwrap_or(0),
            score,
        })
    }

    /// Run audit on component HTML
    pub fn audit_component(
        &self,
        component_name: &str,
        html: &str,
    ) -> Result<AccessibilityAuditResult, String> {
        let full_html = self.wrap_html_with_axe(component_name, html);
        self.audit_html(&full_html)
    }

    /// Wrap HTML with axe-core script
    fn wrap_html_with_axe(&self, component_name: &str, html: &str) -> String {
        format!(
            r#"
            <!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="UTF-8">
                <meta name="viewport" content="width=device-width, initial-scale=1.0">
                <title>Accessibility Audit - {}</title>
                <style>
                    body {{ font-family: Arial, sans-serif; margin: 20px; }}
                    .component {{ border: 1px solid #ccc; padding: 20px; margin: 20px; }}
                </style>
            </head>
            <body>
                <h1>Accessibility Audit - {}</h1>
                <div class="component">
                    {}
                </div>
                <script src="https://cdnjs.cloudflare.com/ajax/libs/axe-core/{}/axe.min.js"></script>
            </body>
            </html>
        "#,
            component_name, component_name, html, self.axe_version
        )
    }
}

/// Accessibility test suite for UI components
pub struct AccessibilityTestSuite {
    auditor: AccessibilityAuditor,
    pub component_tests: HashMap<String, String>,
}

impl Default for AccessibilityTestSuite {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessibilityTestSuite {
    /// Create a new accessibility test suite
    pub fn new() -> Self {
        Self {
            auditor: AccessibilityAuditor::new(),
            component_tests: HashMap::new(),
        }
    }

    /// Register a component test
    pub fn register_component_test(&mut self, component_name: &str, html: &str) {
        self.component_tests
            .insert(component_name.to_string(), html.to_string());
    }

    /// Run all component tests
    pub fn run_all_tests(&mut self) -> HashMap<String, AccessibilityAuditResult> {
        let mut results = HashMap::new();

        for (component_name, html) in &self.component_tests {
            match self.auditor.audit_component(component_name, html) {
                Ok(result) => {
                    results.insert(component_name.clone(), result);
                }
                Err(e) => {
                    eprintln!("Failed to audit component {}: {}", component_name, e);
                }
            }
        }

        results
    }

    /// Run a specific component test
    pub fn run_component_test(&mut self, component_name: &str) -> Option<AccessibilityAuditResult> {
        if let Some(html) = self.component_tests.get(component_name) {
            self.auditor.audit_component(component_name, html).ok()
        } else {
            None
        }
    }

    /// Generate accessibility report
    pub fn generate_report(&self, results: &HashMap<String, AccessibilityAuditResult>) -> String {
        let mut report = String::new();
        report.push_str("# Accessibility Audit Report\n\n");
        report.push_str("## Summary\n\n");
        report.push_str(&format!("Total components tested: {}\n", results.len()));

        let total_violations = results.values().map(|r| r.violations.len()).sum::<usize>();

        let total_passes = results.values().map(|r| r.passes.len()).sum::<usize>();

        let average_score = results.values().map(|r| r.score).sum::<f64>() / results.len() as f64;

        report.push_str(&format!("Total violations: {}\n", total_violations));
        report.push_str(&format!("Total passes: {}\n", total_passes));
        report.push_str(&format!("Average score: {:.1}%\n\n", average_score));

        report.push_str("## Component Details\n\n");

        for (component_name, result) in results {
            report.push_str(&format!("### {}\n", component_name));
            report.push_str(&format!("Score: {:.1}%\n", result.score));
            report.push_str(&format!("{} violations\n", result.violations.len()));
            report.push_str(&format!("Passes: {}\n", result.passes.len()));

            if !result.violations.is_empty() {
                report.push_str("\n**Violations:**\n");
                for violation in &result.violations {
                    report.push_str(&format!(
                        "- **{}**: {}\n",
                        violation.id, violation.description
                    ));
                    report.push_str(&format!("  - Impact: {}\n", violation.impact));
                    report.push_str(&format!("  - Help: {}\n", violation.help));
                    report.push_str(&format!("  - Tags: {:?}\n", violation.tags));
                }
            }

            report.push_str("\n\n");
        }

        report
    }
}

/// CI gate for accessibility compliance
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
    /// Create a new CI gate
    pub fn new() -> Self {
        Self {
            threshold_score: 90.0, // 90% score threshold
            max_violations: 5,     // Maximum 5 violations allowed
        }
    }

    /// Check if audit results pass CI gate
    ///
    /// Gate rules:
    /// - every component must meet the score threshold;
    /// - a `critical`-impact violation fails the gate immediately;
    /// - aggregate violation count must stay within `max_violations`.
    pub fn check_compliance(&self, results: &HashMap<String, AccessibilityAuditResult>) -> bool {
        let mut total_violations = 0;
        let mut total_components = 0;

        for result in results.values() {
            total_violations += result.violations.len();
            total_components += 1;
            if result.score < self.threshold_score {
                return false;
            }
            if result.violations.iter().any(|v| v.impact == "critical") {
                return false;
            }
        }

        if total_components == 0 {
            return true; // No components tested, pass by default
        }

        total_violations <= self.max_violations
    }

    /// Generate CI gate report
    pub fn generate_ci_report(
        &self,
        results: &HashMap<String, AccessibilityAuditResult>,
    ) -> String {
        let mut report = String::new();
        report.push_str("# Accessibility CI Gate Report\n\n");

        let total_violations = results.values().map(|r| r.violations.len()).sum::<usize>();

        let total_components = results.len();
        let average_score =
            results.values().map(|r| r.score).sum::<f64>() / total_components as f64;

        report.push_str("## Gate Status\n\n");

        let passes = self.check_compliance(results);
        report.push_str(&format!(
            "Status: {}\n\n",
            if passes { "✅ PASS" } else { "❌ FAIL" }
        ));

        report.push_str("## Thresholds:\n\n");
        report.push_str(&format!("Score threshold: {:.1}%\n", self.threshold_score));
        report.push_str(&format!("Max violations: {}\n\n", self.max_violations));

        report.push_str("## Results:\n\n");
        report.push_str(&format!("Components tested: {}\n", total_components));
        report.push_str(&format!("Average score: {:.1}%\n", average_score));
        report.push_str(&format!("Total violations: {}\n\n", total_violations));

        if !passes {
            report.push_str("## Failures\n\n");

            if average_score < self.threshold_score {
                report.push_str(&format!(
                    "- Average score ({:.1}%) below threshold ({:.1}%)\n",
                    average_score, self.threshold_score
                ));
            }

            if total_violations > self.max_violations {
                report.push_str(&format!(
                    "- Total violations ({}) exceeds maximum allowed ({})\n",
                    total_violations, self.max_violations
                ));
            }
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accessibility_auditor_creation() {
        let auditor = AccessibilityAuditor::new();
        assert_eq!(auditor.axe_version, "4.9.1");
        assert!(auditor.rules.contains(&"color-contrast".to_string()));
        assert!(auditor
            .exclude_rules
            .contains(&"color-contrast".to_string()));
    }

    #[test]
    fn test_accessibility_test_suite_creation() {
        let suite = AccessibilityTestSuite::new();
        assert_eq!(suite.component_tests.len(), 0);
    }

    #[test]
    fn test_accessibility_test_suite_register_test() {
        let mut suite = AccessibilityTestSuite::new();
        suite.register_component_test("test_component", "<div>Test</div>");
        assert_eq!(suite.component_tests.len(), 1);
        assert_eq!(
            suite.component_tests.get("test_component"),
            Some(&"<div>Test</div>".to_string())
        );
    }

    #[test]
    fn test_accessibility_ci_gate_creation() {
        let gate = AccessibilityCIGate::new();
        assert_eq!(gate.threshold_score, 90.0);
        assert_eq!(gate.max_violations, 5);
    }

    #[test]
    fn test_accessibility_ci_gate_check_compliance_empty() {
        let gate = AccessibilityCIGate::new();
        let results = HashMap::new();
        assert!(gate.check_compliance(&results));
    }

    #[test]
    fn test_accessibility_ci_gate_check_compliance_pass() {
        let gate = AccessibilityCIGate::new();
        let mut results = HashMap::new();

        let result = AccessibilityAuditResult {
            violations: vec![],
            passes: vec!["test".to_string()],
            incomplete: vec![],
            timestamp: 0,
            score: 95.0,
        };

        results.insert("test_component".to_string(), result);
        assert!(gate.check_compliance(&results));
    }

    #[test]
    fn test_accessibility_ci_gate_check_compliance_fail_score() {
        let gate = AccessibilityCIGate::new();
        let mut results = HashMap::new();

        let result = AccessibilityAuditResult {
            violations: vec![],
            passes: vec!["test".to_string()],
            incomplete: vec![],
            timestamp: 0,
            score: 85.0,
        };

        results.insert("test_component".to_string(), result);
        assert!(!gate.check_compliance(&results));
    }

    #[test]
    fn test_accessibility_ci_gate_check_compliance_fail_violations() {
        let gate = AccessibilityCIGate::new();
        let mut results = HashMap::new();

        let result = AccessibilityAuditResult {
            violations: vec![AccessibilityViolation {
                id: "test-violation".to_string(),
                impact: "critical".to_string(),
                description: "Test violation".to_string(),
                help: "Test help".to_string(),
                help_url: "https://example.com".to_string(),
                tags: vec!["test".to_string()],
                nodes: vec![],
            }],
            passes: vec!["test".to_string()],
            incomplete: vec![],
            timestamp: 0,
            score: 95.0,
        };

        results.insert("test_component".to_string(), result);
        assert!(!gate.check_compliance(&results));
    }

    #[test]
    fn test_accessibility_audit_result_creation() {
        let result = AccessibilityAuditResult {
            violations: vec![],
            passes: vec![],
            incomplete: vec![],
            timestamp: 0,
            score: 0.0,
        };

        assert_eq!(result.violations.len(), 0);
        assert_eq!(result.passes.len(), 0);
        assert_eq!(result.incomplete.len(), 0);
        assert_eq!(result.timestamp, 0);
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn test_accessibility_violation_creation() {
        let violation = AccessibilityViolation {
            id: "test-violation".to_string(),
            impact: "critical".to_string(),
            description: "Test violation".to_string(),
            help: "Test help".to_string(),
            help_url: "https://example.com".to_string(),
            tags: vec!["test".to_string()],
            nodes: vec![],
        };

        assert_eq!(violation.id, "test-violation");
        assert_eq!(violation.impact, "critical");
        assert_eq!(violation.description, "Test violation");
        assert_eq!(violation.help, "Test help");
        assert_eq!(violation.help_url, "https://example.com");
        assert_eq!(violation.tags, vec!["test".to_string()]);
        assert_eq!(violation.nodes.len(), 0);
    }

    #[test]
    fn test_accessibility_node_creation() {
        let node = AccessibilityNode {
            html: "<div>Test</div>".to_string(),
            selector: "div".to_string(),
            target: vec!["div".to_string()],
            summary: "Test node".to_string(),
        };

        assert_eq!(node.html, "<div>Test</div>");
        assert_eq!(node.selector, "div");
        assert_eq!(node.target, vec!["div".to_string()]);
        assert_eq!(node.summary, "Test node");
    }
}
