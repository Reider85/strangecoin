//! Input component with validation and accessibility support

use crate::theme::ThemeManager;
use eframe::egui;

/// Input component with validation and accessibility support
pub struct Input {
    label: String,
    value: String,
    placeholder: String,
    input_type: InputType,
    error: Option<String>,
    required: bool,
    disabled: bool,
    readonly: bool,
    on_change: Option<Box<dyn Fn(String) + Send + Sync>>,
    id: String,
}

/// Input types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputType {
    Text,
    Number,
    Email,
    Password,
    Bech32, // Strangecoin address
}

impl Input {
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            id: format!("input_{}", label),
            label,
            value: String::new(),
            placeholder: String::new(),
            input_type: InputType::Text,
            error: None,
            required: false,
            disabled: false,
            readonly: false,
            on_change: None,
        }
    }

    pub fn placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = placeholder.to_string();
        self
    }

    pub fn input_type(mut self, input_type: InputType) -> Self {
        self.input_type = input_type;
        self
    }

    pub fn value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self
    }

    pub fn error(mut self, error: Option<&str>) -> Self {
        self.error = error.map(|s| s.to_string());
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn on_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.on_change = Some(Box::new(callback));
        self
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = id.to_string();
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();
        
        // Container with error styling
        let frame_style = if self.error.is_some() {
            egui::Frame::default()
                .stroke(egui::Stroke::new(1.0_f32, colors.error.as_egui()))
                .rounding(egui::Rounding::same(4.0))
        } else {
            egui::Frame::default()
                .stroke(egui::Stroke::new(1.0_f32, colors.border.as_egui()))
                .rounding(egui::Rounding::same(4.0))
        };

        frame_style.show(ui, |ui| {
            // Label
            if !self.label.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(&self.label);
                    if self.required {
                        ui.label(egui::RichText::new("*").color(colors.error.as_egui()));
                    }
                });
            }

            // Input field
            let response = ui.add_enabled(
                !self.disabled,
                egui::TextEdit::singleline(&mut self.value)
                    .hint_text(&self.placeholder)
                    .frame(false)
                    .desired_width(f32::INFINITY),
            );

            // Disabled/readonly tint: egui 0.30 has no per-response style
            // override; the enabled flag above handles disabled state.
            if self.readonly {
                ui.visuals_mut().widgets.noninteractive.fg_stroke.color =
                    colors.text.as_egui();
            }

            // Handle input change
            if response.changed() {
                if let Some(ref callback) = self.on_change {
                    callback(self.value.clone());
                }
            }

            // Keyboard navigation
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && response.has_focus() {
                // Trigger change on Enter key
                if let Some(ref callback) = self.on_change {
                    callback(self.value.clone());
                }
            }

            // Error message
            if let Some(ref error) = self.error {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = tokens.spacing.sm;
                    ui.label(
                        egui::RichText::new(error)
                            .color(colors.error.as_egui())
                            .size(tokens.typography.sm.size),
                    );
                });
            }
        });
    }

    pub fn show_multiline(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();
        
        // Container with error styling
        let frame_style = if self.error.is_some() {
            egui::Frame::default()
                .stroke(egui::Stroke::new(1.0_f32, colors.error.as_egui()))
                .rounding(egui::Rounding::same(4.0))
        } else {
            egui::Frame::default()
                .stroke(egui::Stroke::new(1.0_f32, colors.border.as_egui()))
                .rounding(egui::Rounding::same(4.0))
        };

        frame_style.show(ui, |ui| {
            // Label
            if !self.label.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(&self.label);
                    if self.required {
                        ui.label(egui::RichText::new("*").color(colors.error.as_egui()));
                    }
                });
            }

            // Input field
            let mut response = ui.add_enabled(
                !self.disabled,
                egui::TextEdit::multiline(&mut self.value)
                    .hint_text(&self.placeholder)
                    .frame(false)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8),
            );

            // Disabled/readonly tint: egui 0.30 has no per-response style override.
            if self.readonly {
                ui.visuals_mut().widgets.noninteractive.fg_stroke.color =
                    colors.text.as_egui();
            }

            // Handle input change
            if response.changed() {
                if let Some(ref callback) = self.on_change {
                    callback(self.value.clone());
                }
            }

            // Error message
            if let Some(ref error) = self.error {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = tokens.spacing.sm;
                    ui.label(
                        egui::RichText::new(error)
                            .color(colors.error.as_egui())
                            .size(tokens.typography.sm.size),
                    );
                });
            }

            let _ = &mut response;
        });
    }

    /// Validate input based on type
    pub fn validate(&self) -> Option<String> {
        if self.value.is_empty() {
            if self.required {
                return Some("This field is required".to_string());
            }
            return None;
        }

        match self.input_type {
            InputType::Text => {
                // Basic text validation
                if self.value.len() < 2 {
                    return Some("Value must be at least 2 characters".to_string());
                }
            }
            InputType::Number => {
                if self.value.parse::<f64>().is_err() {
                    return Some("Please enter a valid number".to_string());
                }
            }
            InputType::Email => {
                let email_regex = regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap();
                if !email_regex.is_match(&self.value) {
                    return Some("Please enter a valid email address".to_string());
                }
            }
            InputType::Password => {
                if self.value.len() < 8 {
                    return Some("Password must be at least 8 characters".to_string());
                }
            }
            InputType::Bech32 => {
                // Basic bech32 validation for Strangecoin addresses
                // Simplified Strangecoin address shape check (full bech32
                // checksum validation lives in strangecoin-core::address)
                let bech32_regex = regex::Regex::new(r"^sc1[0-9a-z]{39,59}$").unwrap();
                if !bech32_regex.is_match(&self.value) {
                    return Some("Please enter a valid Strangecoin address".to_string());
                }
            }
        }

        None
    }

    /// Clear error state
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Set error based on validation
    pub fn validate_and_set_error(&mut self) {
        self.error = self.validate();
    }

    /// Get current value
    pub fn get_value(&self) -> &str {
        &self.value
    }

    /// Set value
    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_string();
        self.error = None; // Clear error when value changes
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new("Input")
    }
}

/// Input builder for convenience
pub struct InputBuilder {
    input: Input,
}

impl InputBuilder {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            input: Input::new(label),
        }
    }

    pub fn placeholder(mut self, placeholder: &str) -> Self {
        self.input.placeholder = placeholder.to_string();
        self
    }

    pub fn input_type(mut self, input_type: InputType) -> Self {
        self.input.input_type = input_type;
        self
    }

    pub fn value(mut self, value: &str) -> Self {
        self.input.value = value.to_string();
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.input.required = required;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.input.disabled = disabled;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.input.readonly = readonly;
        self
    }

    pub fn on_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.input.on_change = Some(Box::new(callback));
        self
    }

    pub fn id(mut self, id: &str) -> Self {
        self.input.id = id.to_string();
        self
    }

    pub fn build(self) -> Input {
        self.input
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn test_input_creation() {
        let input = Input::new("Test");
        assert_eq!(input.label, "Test");
        assert_eq!(input.value, "");
        assert_eq!(input.input_type, InputType::Text);
        assert!(input.error.is_none());
        assert!(!input.required);
        assert!(!input.disabled);
    }

    #[test]
    fn test_input_builder() {
        let input = InputBuilder::new("Email")
            .placeholder("Enter your email")
            .input_type(InputType::Email)
            .required(true)
            .build();

        assert_eq!(input.label, "Email");
        assert_eq!(input.placeholder, "Enter your email");
        assert_eq!(input.input_type, InputType::Email);
        assert!(input.required);
    }

    #[test]
    fn test_input_validation() {
        let mut input = Input::new("Email").input_type(InputType::Email);
        
        // Valid email
        input.set_value("test@example.com");
        assert_eq!(input.validate(), None);
        
        // Invalid email
        input.set_value("invalid-email");
        assert!(input.validate().is_some());
        
        // Required field
        let mut input = Input::new("Required").required(true);
        assert_eq!(input.validate(), Some("This field is required".to_string()));
        
        input.set_value("value");
        assert_eq!(input.validate(), None);
    }

    #[test]
    fn test_bech32_validation() {
        let mut input = Input::new("Address").input_type(InputType::Bech32);
        
        // Valid bech32 address (simplified pattern)
        input.set_value("sc1ac1h0d4q3z5e7x9c2b4f6a8d0g2j4k6m9n1q3w5e7y0z2x4c6v8b0n2m4q6");
        assert_eq!(input.validate(), None);
        
        // Invalid bech32 address
        input.set_value("invalid-address");
        assert!(input.validate().is_some());
    }
}