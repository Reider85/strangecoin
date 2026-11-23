//! Stat component for displaying statistics

use crate::theme::ThemeManager;
use eframe::egui;

/// Stat component for displaying statistics
pub struct Stat {
    id: String,
    title: String,
    value: String,
    change: Option<StatChange>,
    trend: Option<StatTrend>,
    icon: Option<String>,
    size: StatSize,
}

/// Stat change
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatChange {
    value: f64,
    percentage: bool,
}

/// Stat trend
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatTrend {
    Up,
    Down,
    Neutral,
}

/// Stat sizes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatSize {
    Sm,
    Md,
    Lg,
}

impl Stat {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn new(id: &str, title: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            value: value.to_string(),
            change: None,
            trend: None,
            icon: None,
            size: StatSize::Md,
        }
    }

    pub fn change(mut self, value: f64, percentage: bool) -> Self {
        self.change = Some(StatChange { value, percentage });
        self
    }

    pub fn trend(mut self, trend: StatTrend) -> Self {
        self.trend = Some(trend);
        self
    }

    pub fn icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.to_string());
        self
    }

    pub fn size(mut self, size: StatSize) -> Self {
        self.size = size;
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Calculate size
        let (padding, title_size, value_size) = match self.size {
            StatSize::Sm => (
                tokens.spacing.xs,
                tokens.typography.xs.size,
                tokens.typography.sm.size,
            ),
            StatSize::Md => (
                tokens.spacing.sm,
                tokens.typography.sm.size,
                tokens.typography.base.size,
            ),
            StatSize::Lg => (
                tokens.spacing.base,
                tokens.typography.base.size,
                tokens.typography.lg.size,
            ),
        };

        // Stat container
        egui::Frame::default()
            .fill(colors.surface.as_egui())
            .rounding(egui::Rounding::same(tokens.radius.md))
            .inner_margin(egui::Margin::same(padding))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // Icon
                    if let Some(ref icon) = self.icon {
                        ui.horizontal(|ui| {
                            ui.label(icon);
                            ui.add_space(tokens.spacing.sm);
                        });
                    }

                    // Title
                    ui.label(
                        egui::RichText::new(&self.title)
                            .size(title_size)
                            .color(colors.text_secondary.as_egui()),
                    );

                    // Value
                    ui.label(
                        egui::RichText::new(&self.value)
                            .size(value_size)
                            .strong()
                            .color(colors.text.as_egui()),
                    );

                    // Change and trend
                    if let Some(ref change) = self.change {
                        let change_text = if change.percentage {
                            format!("{:.1}%", change.value)
                        } else {
                            format!("{:.1}", change.value)
                        };

                        let change_color = if change.value >= 0.0 {
                            colors.success.as_egui()
                        } else {
                            colors.error.as_egui()
                        };

                        let trend_icon = if let Some(trend) = self.trend {
                            match trend {
                                StatTrend::Up => "↑",
                                StatTrend::Down => "↓",
                                StatTrend::Neutral => "→",
                            }
                        } else {
                            ""
                        };

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&change_text)
                                    .color(change_color)
                                    .strong(),
                            );
                            if !trend_icon.is_empty() {
                                ui.label(
                                    egui::RichText::new(trend_icon)
                                        .color(change_color)
                                        .size(title_size),
                                );
                            }
                        });
                    }
                });
            });
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_string();
    }

    pub fn set_change(&mut self, value: f64, percentage: bool) {
        self.change = Some(StatChange { value, percentage });
    }

    pub fn set_trend(&mut self, trend: StatTrend) {
        self.trend = Some(trend);
    }
}

impl Default for Stat {
    fn default() -> Self {
        Self::new("stat", "Title", "Value")
    }
}

/// Stat builder for convenience
pub struct StatBuilder {
    stat: Stat,
}

impl StatBuilder {
    pub fn new(id: &str, title: &str, value: &str) -> Self {
        Self {
            stat: Stat::new(id, title, value),
        }
    }

    pub fn change(mut self, value: f64, percentage: bool) -> Self {
        self.stat.change = Some(StatChange { value, percentage });
        self
    }

    pub fn trend(mut self, trend: StatTrend) -> Self {
        self.stat.trend = Some(trend);
        self
    }

    pub fn icon(mut self, icon: &str) -> Self {
        self.stat.icon = Some(icon.to_string());
        self
    }

    pub fn size(mut self, size: StatSize) -> Self {
        self.stat.size = size;
        self
    }

    pub fn build(self) -> Stat {
        self.stat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_creation() {
        let stat = Stat::new("test", "Revenue", "$1,000");
        assert_eq!(stat.id, "test");
        assert_eq!(stat.title, "Revenue");
        assert_eq!(stat.value, "$1,000");
        assert!(stat.change.is_none());
        assert!(stat.trend.is_none());
        assert!(stat.icon.is_none());
        assert_eq!(stat.size, StatSize::Md);
    }

    #[test]
    fn test_stat_builder() {
        let stat = StatBuilder::new("test", "Revenue", "$1,000")
            .change(10.5, true)
            .trend(StatTrend::Up)
            .icon("💰")
            .size(StatSize::Lg)
            .build();

        assert_eq!(stat.id, "test");
        assert_eq!(stat.title, "Revenue");
        assert_eq!(stat.value, "$1,000");
        assert_eq!(
            stat.change,
            Some(StatChange {
                value: 10.5,
                percentage: true
            })
        );
        assert_eq!(stat.trend, Some(StatTrend::Up));
        assert_eq!(stat.icon, Some("💰".to_string()));
        assert_eq!(stat.size, StatSize::Lg);
    }

    #[test]
    fn test_stat_trends() {
        assert_eq!(StatTrend::Up, StatTrend::Up);
        assert_eq!(StatTrend::Down, StatTrend::Down);
        assert_eq!(StatTrend::Neutral, StatTrend::Neutral);
    }

    #[test]
    fn test_stat_sizes() {
        assert_eq!(StatSize::Sm, StatSize::Sm);
        assert_eq!(StatSize::Md, StatSize::Md);
        assert_eq!(StatSize::Lg, StatSize::Lg);
    }
}
