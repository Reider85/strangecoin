//! Badge component for status indicators

use crate::theme::ThemeManager;
use eframe::egui;

/// Badge component for status indicators
pub struct Badge {
    id: String,
    text: String,
    variant: BadgeVariant,
    size: BadgeSize,
    rounded: bool,
}

/// Badge variants
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BadgeVariant {
    Default,
    Primary,
    Secondary,
    Success,
    Warning,
    Error,
    Info,
}

/// Badge sizes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BadgeSize {
    Sm,
    Md,
    Lg,
}

impl Badge {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn new(id: &str, text: &str) -> Self {
        Self {
            id: id.to_string(),
            text: text.to_string(),
            variant: BadgeVariant::Default,
            size: BadgeSize::Md,
            rounded: false,
        }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.size = size;
        self
    }

    pub fn rounded(mut self, rounded: bool) -> Self {
        self.rounded = rounded;
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Calculate size
        let (padding, font_size) = match self.size {
            BadgeSize::Sm => (tokens.spacing.xs, tokens.typography.xs.size),
            BadgeSize::Md => (tokens.spacing.sm, tokens.typography.sm.size),
            BadgeSize::Lg => (tokens.spacing.base, tokens.typography.base.size),
        };

        // Calculate colors
        let (bg_color, fg_color) = match self.variant {
            BadgeVariant::Default => (colors.surface, colors.text),
            BadgeVariant::Primary => (colors.primary, colors.background),
            BadgeVariant::Secondary => (colors.secondary, colors.text),
            BadgeVariant::Success => (colors.success, colors.background),
            BadgeVariant::Warning => (colors.warning, colors.background),
            BadgeVariant::Error => (colors.error, colors.background),
            BadgeVariant::Info => (colors.info, colors.background),
        };

        // Style
        let mut style = egui::Style::default();
        style.spacing.button_padding = egui::vec2(padding, padding);
        style.visuals.widgets.noninteractive.rounding = if self.rounded {
            egui::Rounding::same(999.0)
        } else {
            egui::Rounding::same(2.0)
        };
        style.visuals.widgets.hovered.rounding = if self.rounded {
            egui::Rounding::same(999.0)
        } else {
            egui::Rounding::same(2.0)
        };
        style.visuals.widgets.active.rounding = if self.rounded {
            egui::Rounding::same(999.0)
        } else {
            egui::Rounding::same(2.0)
        };

        // Badge frame
        egui::Frame::default()
            .fill(bg_color.as_egui())
            .rounding(if self.rounded {
                egui::Rounding::same(999.0)
            } else {
                egui::Rounding::same(2.0)
            })
            .show(ui, |ui| {
                ui.style_mut().override_text_style = Some(egui::TextStyle::Button);
                ui.style_mut().override_font_id =
                    Some(egui::FontId::new(font_size, egui::FontFamily::Proportional));
                *ui.style_mut() = style;

                ui.label(
                    egui::RichText::new(&self.text)
                        .color(fg_color.as_egui())
                        .strong(),
                );
            });
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
    }
}

impl Default for Badge {
    fn default() -> Self {
        Self::new("badge", "Badge")
    }
}

/// Badge builder for convenience
pub struct BadgeBuilder {
    badge: Badge,
}

impl BadgeBuilder {
    pub fn new(id: &str, text: &str) -> Self {
        Self {
            badge: Badge::new(id, text),
        }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.badge.variant = variant;
        self
    }

    pub fn size(mut self, size: BadgeSize) -> Self {
        self.badge.size = size;
        self
    }

    pub fn rounded(mut self, rounded: bool) -> Self {
        self.badge.rounded = rounded;
        self
    }

    pub fn build(self) -> Badge {
        self.badge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_badge_creation() {
        let badge = Badge::new("test", "Hello");
        assert_eq!(badge.id, "test");
        assert_eq!(badge.text, "Hello");
        assert_eq!(badge.variant, BadgeVariant::Default);
        assert_eq!(badge.size, BadgeSize::Md);
        assert!(!badge.rounded);
    }

    #[test]
    fn test_badge_builder() {
        let badge = BadgeBuilder::new("test", "Hello")
            .variant(BadgeVariant::Success)
            .size(BadgeSize::Lg)
            .rounded(true)
            .build();

        assert_eq!(badge.id, "test");
        assert_eq!(badge.text, "Hello");
        assert_eq!(badge.variant, BadgeVariant::Success);
        assert_eq!(badge.size, BadgeSize::Lg);
        assert!(badge.rounded);
    }

    #[test]
    fn test_badge_variants() {
        assert_eq!(BadgeVariant::Default, BadgeVariant::Default);
        assert_eq!(BadgeVariant::Primary, BadgeVariant::Primary);
        assert_eq!(BadgeVariant::Secondary, BadgeVariant::Secondary);
        assert_eq!(BadgeVariant::Success, BadgeVariant::Success);
        assert_eq!(BadgeVariant::Warning, BadgeVariant::Warning);
        assert_eq!(BadgeVariant::Error, BadgeVariant::Error);
        assert_eq!(BadgeVariant::Info, BadgeVariant::Info);
    }

    #[test]
    fn test_badge_sizes() {
        assert_eq!(BadgeSize::Sm, BadgeSize::Sm);
        assert_eq!(BadgeSize::Md, BadgeSize::Md);
        assert_eq!(BadgeSize::Lg, BadgeSize::Lg);
    }
}
