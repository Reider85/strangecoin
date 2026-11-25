//! Button component with variants and accessibility support

use crate::accessibility::WidgetProbe;
use crate::focus::{self, WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::theme::{ThemeColors, ThemeManager};
use crate::tokens::{Color, DesignTokens, Shade};
use eframe::egui;

/// Button component with variants and accessibility support
pub struct Button {
    label: String,
    variant: ButtonVariant,
    size: ButtonSize,
    loading: bool,
    disabled: bool,
    onclick: Option<Box<dyn Fn() + Send + Sync>>,
    id: String,
}

/// Button variants
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// Button sizes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonSize {
    Sm,
    Md,
    Lg,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            id: format!("btn_{}", label),
            label,
            variant: ButtonVariant::Primary,
            size: ButtonSize::Md,
            loading: false,
            disabled: false,
            onclick: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn onclick<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.onclick = Some(Box::new(callback));
        self
    }

    pub fn id(mut self, id: &str) -> Self {
        self.id = id.to_string();
        self
    }

    fn variant_colors(&self, colors: &ThemeColors, tokens: &DesignTokens) -> (Color, Color, Color) {
        let ink = tokens.colors.neutral(Shade::_950);
        match (self.variant, self.disabled, self.loading) {
            (ButtonVariant::Primary, true, _) => {
                (colors.surface, colors.text_secondary, colors.border)
            }
            (ButtonVariant::Primary, false, _) => (colors.primary, ink, colors.primary),
            (ButtonVariant::Secondary, true, _) => {
                (colors.surface, colors.text_secondary, colors.border)
            }
            (ButtonVariant::Secondary, false, _) => (colors.surface, colors.text, colors.border),
            (ButtonVariant::Ghost, true, _) => {
                (colors.surface, colors.text_secondary, colors.border)
            }
            (ButtonVariant::Ghost, false, _) => (colors.background, colors.text, colors.border),
            (ButtonVariant::Danger, true, _) => {
                (colors.surface, colors.text_secondary, colors.border)
            }
            (ButtonVariant::Danger, false, _) => (colors.error, ink, colors.error),
        }
    }

    fn a11y_meta(&self) -> WidgetMeta {
        WidgetMeta::new("button", "button", self.label.clone())
    }

    pub fn a11y_probe(&self, theme: &ThemeManager) -> WidgetProbe {
        let colors = theme.colors();
        let (background, foreground, _) = self.variant_colors(&colors, theme.tokens());
        WidgetProbe::interactive(
            self.a11y_meta(),
            foreground,
            background,
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        )
        .with_focus(true, FOCUS_OUTLINE_WIDTH)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Calculate size
        let (padding, font_size) = match self.size {
            ButtonSize::Sm => (tokens.spacing.sm, tokens.typography.sm.size),
            ButtonSize::Md => (tokens.spacing.base, tokens.typography.base.size),
            ButtonSize::Lg => (tokens.spacing.lg, tokens.typography.lg.size),
        };

        // Calculate colors based on variant and state
        let (bg_color, fg_color, stroke_color) = self.variant_colors(&colors, tokens);
        let focus_color = colors.text;
        let meta = self.a11y_meta();

        // Style
        let mut style = egui::Style::default();
        style.spacing.button_padding = egui::vec2(padding, padding);
        style.spacing.icon_width = font_size * 0.75;
        style.spacing.icon_spacing = padding * 0.5;
        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(4.0);

        // Button frame
        egui::Frame::default()
            .fill(bg_color.as_egui())
            .stroke(egui::Stroke::new(1.0_f32, stroke_color.as_egui()))
            .rounding(egui::Rounding::same(4.0))
            .show(ui, |ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Button);
                ui.style_mut().override_font_id =
                    Some(egui::FontId::new(font_size, egui::FontFamily::Proportional));

                let response = ui
                    .push_id(focus::widget_id(&meta), |ui| {
                        ui.add_enabled(
                            !self.disabled,
                            egui::Button::new(
                                egui::RichText::new(&self.label).color(fg_color.as_egui()),
                            )
                            .min_size(focus::min_touch_size(egui::Vec2::ZERO)),
                        )
                    })
                    .inner;
                focus::apply_focus_ring(ui, &response, focus_color.as_egui());

                // Handle click
                if !self.disabled && !self.loading && response.clicked() {
                    if let Some(ref callback) = self.onclick {
                        callback();
                    }
                }

                // Show loading spinner
                if self.loading {
                    ui.spinner();
                }

                // Add keyboard shortcut support
                if ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && response.has_focus()
                    && !self.disabled
                    && !self.loading
                {
                    if let Some(ref callback) = self.onclick {
                        callback();
                    }
                }
            });
    }

    pub fn show_simple(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        // Simplified version for quick usage
        self.show(ui, theme_manager);
    }
}

impl Default for Button {
    fn default() -> Self {
        Self::new("Button")
    }
}

/// Button builder for convenience
pub struct ButtonBuilder {
    button: Button,
}

impl ButtonBuilder {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            button: Button::new(label),
        }
    }

    pub fn primary(mut self) -> Self {
        self.button.variant = ButtonVariant::Primary;
        self
    }

    pub fn secondary(mut self) -> Self {
        self.button.variant = ButtonVariant::Secondary;
        self
    }

    pub fn ghost(mut self) -> Self {
        self.button.variant = ButtonVariant::Ghost;
        self
    }

    pub fn danger(mut self) -> Self {
        self.button.variant = ButtonVariant::Danger;
        self
    }

    pub fn small(mut self) -> Self {
        self.button.size = ButtonSize::Sm;
        self
    }

    pub fn medium(mut self) -> Self {
        self.button.size = ButtonSize::Md;
        self
    }

    pub fn large(mut self) -> Self {
        self.button.size = ButtonSize::Lg;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.button.loading = loading;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.button.disabled = disabled;
        self
    }

    pub fn onclick<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.button.onclick = Some(Box::new(callback));
        self
    }

    pub fn id(mut self, id: &str) -> Self {
        self.button.id = id.to_string();
        self
    }

    pub fn build(self) -> Button {
        self.button
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_creation() {
        let button = Button::new("Test");
        assert_eq!(button.label, "Test");
        assert_eq!(button.variant, ButtonVariant::Primary);
        assert_eq!(button.size, ButtonSize::Md);
        assert!(!button.loading);
        assert!(!button.disabled);
        assert!(button.onclick.is_none());
    }

    #[test]
    fn test_button_builder() {
        let button = ButtonBuilder::new("Submit")
            .primary()
            .medium()
            .onclick(|| {
                println!("Clicked!");
            })
            .build();

        assert_eq!(button.label, "Submit");
        assert_eq!(button.variant, ButtonVariant::Primary);
        assert_eq!(button.size, ButtonSize::Md);
        assert!(button.onclick.is_some());
    }

    #[test]
    fn test_button_variants() {
        let button = Button::new("Test");
        assert_eq!(button.variant, ButtonVariant::Primary);

        let button = button.variant(ButtonVariant::Secondary);
        assert_eq!(button.variant, ButtonVariant::Secondary);

        let button = button.variant(ButtonVariant::Ghost);
        assert_eq!(button.variant, ButtonVariant::Ghost);

        let button = button.variant(ButtonVariant::Danger);
        assert_eq!(button.variant, ButtonVariant::Danger);
    }

    #[test]
    fn test_button_sizes() {
        let button = Button::new("Test");
        assert_eq!(button.size, ButtonSize::Md);

        let button = button.size(ButtonSize::Sm);
        assert_eq!(button.size, ButtonSize::Sm);

        let button = button.size(ButtonSize::Md);
        assert_eq!(button.size, ButtonSize::Md);

        let button = button.size(ButtonSize::Lg);
        assert_eq!(button.size, ButtonSize::Lg);
    }
}
