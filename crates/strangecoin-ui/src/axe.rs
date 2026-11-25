use std::fs;
use std::process::Command;

pub const AXE_VERSION: &str = "4.9.1";
pub const AXE_SHA256: &str = "182a40dc5d8207e626c09861ad65027d45e85e3a56d01045d068e2e88ee432ea";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxeReport {
    pub violations: Vec<String>,
    pub passes: Vec<String>,
}

pub fn is_enabled() -> bool {
    std::env::var("STRANGECOIN_AXE_AUDIT").as_deref() == Ok("1")
}

pub fn audit_html(html: &str) -> Result<AxeReport, String> {
    if !is_enabled() {
        return Err("axe audit is opt-in: set STRANGECOIN_AXE_AUDIT=1".to_string());
    }

    let axe_path = concat!(env!("CARGO_MANIFEST_DIR"), "/vendor/axe.min.js");
    let html_path =
        std::env::temp_dir().join(format!("strangecoin-a11y-{}.html", std::process::id()));
    fs::write(&html_path, html).map_err(|e| format!("cannot write audit html: {e}"))?;

    let script = r#"
const fs = require('fs');
const { JSDOM } = require('jsdom');
const html = fs.readFileSync(process.argv[2], 'utf8');
const dom = new JSDOM(html, { runScripts: 'outside-only' });
dom.window.eval(fs.readFileSync(process.argv[3], 'utf8'));
dom.window.axe.run(dom.window.document, {
  runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa'] }
}).then(r => {
  console.log(JSON.stringify({
    violations: r.violations.map(v => v.id),
    passes: r.passes.map(p => p.id)
  }));
}).catch(e => { console.error(String(e)); process.exit(2); });
"#;

    let output = Command::new("node")
        .args(["-e", script])
        .arg(&html_path)
        .arg(axe_path)
        .output()
        .map_err(|e| format!("node + jsdom required for opt-in axe audit: {e}"))?;
    let _ = fs::remove_file(&html_path);

    if !output.status.success() {
        return Err(format!(
            "axe audit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("invalid axe report: {e}"))?;
    let strings = |key: &str| -> Vec<String> {
        json[key]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    Ok(AxeReport {
        violations: strings("violations"),
        passes: strings("passes"),
    })
}
