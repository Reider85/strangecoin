//! Codeblock component for displaying code

use crate::accessibility::WidgetProbe;
use crate::focus::{self, WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::theme::ThemeManager;
use eframe::egui;

/// Codeblock component for displaying code
pub struct CodeBlock {
    id: String,
    code: String,
    language: Option<String>,
    theme: CodeTheme,
    wrap: bool,
    copy_button: bool,
}

/// Code themes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CodeTheme {
    Light,
    Dark,
}

impl CodeBlock {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }

    fn a11y_meta(&self) -> WidgetMeta {
        let label = self.language.clone().unwrap_or_else(|| "code".to_string());
        WidgetMeta::new("codeblock", "textbox", label)
    }

    pub fn a11y_probe(&self, theme: &ThemeManager) -> WidgetProbe {
        let colors = theme.colors();
        let background = if self.theme == CodeTheme::Dark {
            colors.surface
        } else {
            colors.background
        };
        WidgetProbe::interactive(
            self.a11y_meta(),
            colors.text,
            background,
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        )
        .with_focus(true, FOCUS_OUTLINE_WIDTH)
    }

    pub fn new(id: &str, code: &str) -> Self {
        Self {
            id: id.to_string(),
            code: code.to_string(),
            language: None,
            theme: CodeTheme::Dark,
            wrap: false,
            copy_button: true,
        }
    }

    pub fn language(mut self, language: &str) -> Self {
        self.language = Some(language.to_string());
        self
    }

    pub fn theme(mut self, theme: CodeTheme) -> Self {
        self.theme = theme;
        self
    }

    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    pub fn copy_button(mut self, copy_button: bool) -> Self {
        self.copy_button = copy_button;
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let focus_color = colors.text;
        let tokens = theme_manager.tokens();

        // Codeblock container
        egui::Frame::default()
            .fill(if self.theme == CodeTheme::Dark {
                colors.surface.as_egui()
            } else {
                colors.background.as_egui()
            })
            .rounding(egui::Rounding::same(tokens.radius.md))
            .inner_margin(egui::Margin::same(tokens.spacing.base))
            .show(ui, |ui| {
                // Header
                ui.horizontal(|ui| {
                    if let Some(ref language) = self.language {
                        ui.label(
                            egui::RichText::new(language)
                                .size(tokens.typography.sm.size)
                                .color(colors.text_secondary.as_egui()),
                        );
                    }

                    if self.copy_button {
                        let meta = WidgetMeta::new("codeblock-copy", "button", "Copy");
                        let response = ui
                            .push_id(focus::widget_id(&meta), |ui| {
                                ui.add(
                                    egui::Button::new("Copy")
                                        .min_size(focus::min_touch_size(egui::Vec2::ZERO)),
                                )
                            })
                            .inner;
                        focus::apply_focus_ring(ui, &response, focus_color.as_egui());
                        if response.clicked() {
                            // Copy to clipboard
                            ui.output_mut(|o| o.copied_text = self.code.clone());
                        }
                    }
                });

                ui.add_space(tokens.spacing.sm);

                // Code content
                egui::Frame::default()
                    .fill(if self.theme == CodeTheme::Dark {
                        colors.surface.as_egui()
                    } else {
                        colors.background.as_egui()
                    })
                    .rounding(egui::Rounding::same(4.0))
                    .inner_margin(egui::Margin::same(tokens.spacing.md))
                    .show(ui, |ui| {
                        let meta = self.a11y_meta();
                        if self.wrap {
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut self.code)
                                    .id(focus::widget_id(&meta))
                                    .frame(false)
                                    .desired_width(f32::INFINITY)
                                    .min_size(focus::min_touch_size(egui::Vec2::ZERO))
                                    .lock_focus(true),
                            );
                            focus::apply_focus_ring(ui, &response, focus_color.as_egui());
                        } else {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                let response = ui.add(
                                    egui::TextEdit::multiline(&mut self.code)
                                        .id(focus::widget_id(&meta))
                                        .frame(false)
                                        .desired_width(f32::INFINITY)
                                        .min_size(focus::min_touch_size(egui::Vec2::ZERO))
                                        .lock_focus(true),
                                );
                                focus::apply_focus_ring(ui, &response, focus_color.as_egui());
                            });
                        }
                    });
            });
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn set_code(&mut self, code: &str) {
        self.code = code.to_string();
    }

    pub fn current_language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub fn set_language(&mut self, language: &str) {
        self.language = Some(language.to_string());
    }
}

impl Default for CodeBlock {
    fn default() -> Self {
        Self::new("codeblock", "// Your code here")
    }
}

/// Codeblock builder for convenience
pub struct CodeBlockBuilder {
    codeblock: CodeBlock,
}

impl CodeBlockBuilder {
    pub fn new(id: &str, code: &str) -> Self {
        Self {
            codeblock: CodeBlock::new(id, code),
        }
    }

    pub fn language(mut self, language: &str) -> Self {
        self.codeblock.language = Some(language.to_string());
        self
    }

    pub fn theme(mut self, theme: CodeTheme) -> Self {
        self.codeblock.theme = theme;
        self
    }

    pub fn wrap(mut self, wrap: bool) -> Self {
        self.codeblock.wrap = wrap;
        self
    }

    pub fn copy_button(mut self, copy_button: bool) -> Self {
        self.codeblock.copy_button = copy_button;
        self
    }

    pub fn build(self) -> CodeBlock {
        self.codeblock
    }
}

/// Syntax highlighting for common languages
pub struct SyntaxHighlighter;

impl SyntaxHighlighter {
    /// Apply syntax highlighting to code
    pub fn highlight<'a>(
        &self,
        code: &'a str,
        language: Option<&str>,
    ) -> Vec<(egui::Color32, &'a str)> {
        let mut tokens = Vec::new();

        if let Some(lang) = language {
            match lang {
                "rust" => self.highlight_rust(code, &mut tokens),
                "javascript" | "js" => self.highlight_javascript(code, &mut tokens),
                "python" | "py" => self.highlight_python(code, &mut tokens),
                "json" => self.highlight_json(code, &mut tokens),
                "toml" => self.highlight_toml(code, &mut tokens),
                _ => self.highlight_plain(code, &mut tokens),
            }
        } else {
            self.highlight_plain(code, &mut tokens);
        }

        tokens
    }

    fn highlight_rust<'a>(&self, code: &'a str, tokens: &mut Vec<(egui::Color32, &'a str)>) {
        // Simplified Rust syntax highlighting
        let mut current_pos = 0;

        for word in code.split_whitespace() {
            let start = code[current_pos..].find(word).unwrap() + current_pos;
            let end = start + word.len();

            // Rust keywords
            if [
                "fn", "let", "mut", "const", "static", "struct", "enum", "impl", "trait", "mod",
                "pub", "use", "crate", "self", "super", "async", "await", "move", "match", "if",
                "else", "for", "while", "loop", "return", "break", "continue", "true", "false",
                "Some", "None", "Ok", "Err", "Result", "Option",
            ]
            .contains(&word)
            {
                tokens.push((egui::Color32::from_rgb(0, 128, 0), word));
            } else if word.starts_with('"') && word.ends_with('"') {
                // String literals
                tokens.push((egui::Color32::from_rgb(255, 0, 0), word));
            } else if word.starts_with("//") {
                // Comments
                tokens.push((egui::Color32::from_rgb(128, 128, 128), word));
            } else if word.starts_with("/*") && word.ends_with("*/") {
                // Block comments
                tokens.push((egui::Color32::from_rgb(128, 128, 128), word));
            } else if word.starts_with("0x") || word.starts_with("0b") || word.starts_with("0o") {
                // Numeric literals
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else if word.chars().all(|c| c.is_ascii_digit()) {
                // Numbers
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else {
                // Default
                tokens.push((egui::Color32::from_rgb(255, 255, 255), word));
            }

            current_pos = end;
        }
    }

    fn highlight_javascript<'a>(&self, code: &'a str, tokens: &mut Vec<(egui::Color32, &'a str)>) {
        // Simplified JavaScript syntax highlighting
        let mut current_pos = 0;

        for word in code.split_whitespace() {
            let start = code[current_pos..].find(word).unwrap() + current_pos;
            let end = start + word.len();

            // JavaScript keywords
            if [
                "function",
                "const",
                "let",
                "var",
                "if",
                "else",
                "for",
                "while",
                "do",
                "return",
                "break",
                "continue",
                "true",
                "false",
                "null",
                "undefined",
                "class",
                "extends",
                "import",
                "export",
                "default",
                "async",
                "await",
                "try",
                "catch",
                "throw",
                "new",
                "this",
                "super",
                "static",
                "get",
                "set",
            ]
            .contains(&word)
            {
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else if (word.starts_with('"') && word.ends_with('"'))
                || (word.starts_with("'") && word.ends_with("'"))
            {
                // String literals
                tokens.push((egui::Color32::from_rgb(255, 0, 0), word));
            } else if word.starts_with("//") || (word.starts_with("/*") && word.ends_with("*/")) {
                // Comments
                tokens.push((egui::Color32::from_rgb(128, 128, 128), word));
            } else if word.starts_with("0x") || word.starts_with("0b") || word.starts_with("0o") {
                // Numeric literals
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else if word.chars().all(|c| c.is_ascii_digit()) {
                // Numbers
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else {
                // Default
                tokens.push((egui::Color32::from_rgb(255, 255, 255), word));
            }

            current_pos = end;
        }
    }

    fn highlight_python<'a>(&self, code: &'a str, tokens: &mut Vec<(egui::Color32, &'a str)>) {
        // Simplified Python syntax highlighting
        let mut current_pos = 0;

        for word in code.split_whitespace() {
            let start = code[current_pos..].find(word).unwrap() + current_pos;
            let end = start + word.len();

            // Python keywords
            if [
                "def", "class", "if", "elif", "else", "for", "while", "try", "except", "finally",
                "return", "break", "continue", "pass", "import", "from", "as", "lambda", "and",
                "or", "not", "in", "is", "None", "True", "False", "global", "nonlocal", "assert",
                "with", "yield", "async", "await",
            ]
            .contains(&word)
            {
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else if (word.starts_with('"') && word.ends_with('"'))
                || (word.starts_with("'") && word.ends_with("'"))
                || (word.starts_with("'''") && word.ends_with("'''"))
                || (word.starts_with("\"\"\"") && word.ends_with("\"\"\""))
            {
                // String literals (incl. triple-quoted)
                tokens.push((egui::Color32::from_rgb(255, 0, 0), word));
            } else if word.starts_with("#") {
                // Comments
                tokens.push((egui::Color32::from_rgb(128, 128, 128), word));
            } else if word.starts_with("0x") || word.starts_with("0b") || word.starts_with("0o") {
                // Numeric literals
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else if word.chars().all(|c| c.is_ascii_digit()) {
                // Numbers
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else {
                // Default
                tokens.push((egui::Color32::from_rgb(255, 255, 255), word));
            }

            current_pos = end;
        }
    }

    fn highlight_json<'a>(&self, code: &'a str, tokens: &mut Vec<(egui::Color32, &'a str)>) {
        // Simplified JSON syntax highlighting
        let mut current_pos = 0;

        for word in code.split_whitespace() {
            let start = code[current_pos..].find(word).unwrap() + current_pos;
            let end = start + word.len();

            if word.starts_with('"') && word.ends_with('"') {
                // String literals (keys and values)
                tokens.push((egui::Color32::from_rgb(255, 0, 0), word));
            } else if ["true", "false", "null"].contains(&word) {
                // JSON literals
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else if word.chars().all(|c| c.is_ascii_digit()) {
                // Numbers
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else {
                // Default
                tokens.push((egui::Color32::from_rgb(255, 255, 255), word));
            }

            current_pos = end;
        }
    }

    fn highlight_toml<'a>(&self, code: &'a str, tokens: &mut Vec<(egui::Color32, &'a str)>) {
        // Simplified TOML syntax highlighting
        let mut current_pos = 0;

        for word in code.split_whitespace() {
            let start = code[current_pos..].find(word).unwrap() + current_pos;
            let end = start + word.len();

            if word.starts_with('"') && word.ends_with('"') {
                // String literals
                tokens.push((egui::Color32::from_rgb(255, 0, 0), word));
            } else if word.starts_with("#") {
                // Comments
                tokens.push((egui::Color32::from_rgb(128, 128, 128), word));
            } else if word.chars().all(|c| c.is_ascii_digit()) {
                // Numbers
                tokens.push((egui::Color32::from_rgb(0, 0, 255), word));
            } else {
                // Default
                tokens.push((egui::Color32::from_rgb(255, 255, 255), word));
            }

            current_pos = end;
        }
    }

    fn highlight_plain<'a>(&self, code: &'a str, tokens: &mut Vec<(egui::Color32, &'a str)>) {
        // Plain text without highlighting
        tokens.push((egui::Color32::from_rgb(255, 255, 255), code));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codeblock_creation() {
        let codeblock = CodeBlock::new("test", "let x = 5;");
        assert_eq!(codeblock.id, "test");
        assert_eq!(codeblock.code, "let x = 5;");
        assert!(codeblock.language.is_none());
        assert_eq!(codeblock.theme, CodeTheme::Dark);
        assert!(!codeblock.wrap);
        assert!(codeblock.copy_button);
    }

    #[test]
    fn test_codeblock_builder() {
        let codeblock = CodeBlockBuilder::new("test", "let x = 5;")
            .language("rust")
            .theme(CodeTheme::Light)
            .wrap(true)
            .copy_button(false)
            .build();

        assert_eq!(codeblock.id, "test");
        assert_eq!(codeblock.code, "let x = 5;");
        assert_eq!(codeblock.language, Some("rust".to_string()));
        assert_eq!(codeblock.theme, CodeTheme::Light);
        assert!(codeblock.wrap);
        assert!(!codeblock.copy_button);
    }

    #[test]
    fn test_syntax_highlighter() {
        let highlighter = SyntaxHighlighter;

        let rust_code = "fn main() { let x = 5; }";
        let tokens = highlighter.highlight(rust_code, Some("rust"));
        assert!(!tokens.is_empty());

        let js_code = "function test() { let x = 5; }";
        let tokens = highlighter.highlight(js_code, Some("javascript"));
        assert!(!tokens.is_empty());

        let python_code = "def test(): x = 5";
        let tokens = highlighter.highlight(python_code, Some("python"));
        assert!(!tokens.is_empty());

        let json_code = r#"{"name": "test", "value": 5}"#;
        let tokens = highlighter.highlight(json_code, Some("json"));
        assert!(!tokens.is_empty());

        let plain_code = "Hello, World!";
        let tokens = highlighter.highlight(plain_code, None);
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_code_themes() {
        assert_eq!(CodeTheme::Light, CodeTheme::Light);
        assert_eq!(CodeTheme::Dark, CodeTheme::Dark);
    }
}
