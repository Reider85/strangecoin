//! Chart component for data visualization

use crate::accessibility::WidgetProbe;
use crate::focus::WidgetMeta;
use crate::theme::ThemeManager;
use eframe::egui;

/// Chart component for data visualization
pub struct Chart {
    id: String,
    chart_type: ChartType,
    data: Vec<ChartDataPoint>,
    title: Option<String>,
    x_axis: Option<String>,
    y_axis: Option<String>,
    width: Option<f32>,
    height: Option<f32>,
}

/// Chart types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChartType {
    Line,
    Bar,
    Area,
    Pie,
}

/// Chart data point
#[derive(Debug, Clone)]
pub struct ChartDataPoint {
    pub x: f32,
    pub y: f32,
    pub label: Option<String>,
    pub color: Option<egui::Color32>,
}

impl Chart {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }

    fn a11y_meta(&self) -> WidgetMeta {
        let label = self.title.clone().unwrap_or_else(|| self.id.clone());
        WidgetMeta::new("chart", "img", label)
    }

    pub fn a11y_probe(&self, theme: &ThemeManager) -> WidgetProbe {
        let colors = theme.colors();
        WidgetProbe::non_interactive(self.a11y_meta(), colors.text, colors.surface)
    }

    pub fn new(id: &str, chart_type: ChartType) -> Self {
        Self {
            id: id.to_string(),
            chart_type,
            data: Vec::new(),
            title: None,
            x_axis: None,
            y_axis: None,
            width: None,
            height: None,
        }
    }

    pub fn data(mut self, data: Vec<ChartDataPoint>) -> Self {
        self.data = data;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn x_axis(mut self, label: &str) -> Self {
        self.x_axis = Some(label.to_string());
        self
    }

    pub fn y_axis(mut self, label: &str) -> Self {
        self.y_axis = Some(label.to_string());
        self
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Set default size
        let width = self.width.unwrap_or(400.0);
        let height = self.height.unwrap_or(300.0);

        // Chart container
        egui::Frame::default()
            .fill(colors.surface.as_egui())
            .rounding(egui::Rounding::same(tokens.radius.md))
            .inner_margin(egui::Margin::same(tokens.spacing.base))
            .show(ui, |ui| {
                // Title
                if let Some(ref title) = self.title {
                    ui.label(
                        egui::RichText::new(title)
                            .size(tokens.typography.lg.size)
                            .strong(),
                    );
                }

                // Chart area
                egui::Frame::default()
                    .fill(colors.background.as_egui())
                    .rounding(egui::Rounding::same(4.0))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(width, height));

                        match self.chart_type {
                            ChartType::Line => self.show_line_chart(ui, theme_manager),
                            ChartType::Bar => self.show_bar_chart(ui, theme_manager),
                            ChartType::Area => self.show_area_chart(ui, theme_manager),
                            ChartType::Pie => self.show_pie_chart(ui, theme_manager),
                        }
                    });

                // Axis labels
                if let Some(ref x_label) = self.x_axis {
                    ui.label(
                        egui::RichText::new(x_label)
                            .size(tokens.typography.sm.size)
                            .color(colors.text_secondary.as_egui()),
                    );
                }

                if let Some(ref y_label) = self.y_axis {
                    ui.vertical(|ui| {
                        ui.add_space(height - 20.0);
                        ui.label(
                            egui::RichText::new(y_label)
                                .size(tokens.typography.sm.size)
                                .color(colors.text_secondary.as_egui()),
                        );
                    });
                }
            });
    }

    fn show_line_chart(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();

        if self.data.is_empty() {
            ui.label("No data available");
            return;
        }

        // Calculate bounds
        let x_min = self.data.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let x_max = self
            .data
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let y_min = self.data.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let y_max = self
            .data
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        let x_range = (x_max - x_min).max(f32::EPSILON);
        let y_range = (y_max - y_min).max(f32::EPSILON);

        let padding = 40.0f32;
        let chart_width = (ui.available_width() - padding * 2.0).max(1.0);
        let chart_height = (ui.available_height() - padding * 2.0).max(1.0);

        // Draw grid
        for i in 0..=5 {
            let y = padding + (chart_height / 5.0) * i as f32;
            ui.painter().line_segment(
                [egui::pos2(padding, y), egui::pos2(padding + chart_width, y)],
                egui::Stroke::new(1.0_f32, colors.border.as_egui()),
            );
        }

        // Draw data points and lines
        for i in 0..self.data.len() {
            let point = &self.data[i];
            let x = padding + ((point.x - x_min) / x_range) * chart_width;
            let y = padding + chart_height - ((point.y - y_min) / y_range) * chart_height;

            // Draw line to previous point
            if i > 0 {
                let prev_point = &self.data[i - 1];
                let prev_x = padding + ((prev_point.x - x_min) / x_range) * chart_width;
                let prev_y =
                    padding + chart_height - ((prev_point.y - y_min) / y_range) * chart_height;

                ui.painter().line_segment(
                    [egui::pos2(prev_x, prev_y), egui::pos2(x, y)],
                    egui::Stroke::new(2.0_f32, colors.primary.as_egui()),
                );
            }

            // Draw point
            ui.painter()
                .circle_filled(egui::pos2(x, y), 4.0, colors.primary.as_egui());
        }
    }

    fn show_bar_chart(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();

        if self.data.is_empty() {
            ui.label("No data available");
            return;
        }

        // Calculate bounds
        let y_max = self
            .data
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .max(f32::EPSILON);
        let padding = 40.0f32;
        let chart_width = (ui.available_width() - padding * 2.0).max(1.0);
        let chart_height = (ui.available_height() - padding * 2.0).max(1.0);
        let bar_width = chart_width / self.data.len() as f32 * 0.8;
        let bar_spacing = chart_width / self.data.len() as f32 * 0.2;

        // Draw bars
        for (i, point) in self.data.iter().enumerate() {
            let bar_height = (point.y / y_max) * chart_height;
            let x = padding + (i as f32 * (bar_width + bar_spacing));
            let y = padding + chart_height - bar_height;

            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(bar_width, bar_height)),
                egui::Rounding::same(2.0),
                colors.primary.as_egui(),
            );
        }
    }

    fn show_area_chart(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();

        if self.data.is_empty() {
            ui.label("No data available");
            return;
        }

        // Calculate bounds
        let x_min = self.data.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let x_max = self
            .data
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let y_min = self.data.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let y_max = self
            .data
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        let x_range = (x_max - x_min).max(f32::EPSILON);
        let y_range = (y_max - y_min).max(f32::EPSILON);

        let padding = 40.0f32;
        let chart_width = (ui.available_width() - padding * 2.0).max(1.0);
        let chart_height = (ui.available_height() - padding * 2.0).max(1.0);

        // Create points for area
        let mut points = Vec::new();
        for point in &self.data {
            let x = padding + ((point.x - x_min) / x_range) * chart_width;
            let y = padding + chart_height - ((point.y - y_min) / y_range) * chart_height;
            points.push(egui::pos2(x, y));
        }

        // Close the area
        if !points.is_empty() {
            points.push(egui::pos2(points.last().unwrap().x, padding + chart_height));
            points.push(egui::pos2(
                points.first().unwrap().x,
                padding + chart_height,
            ));

            // Draw filled area (30% alpha via unmultiplied construction)
            let fill = colors.primary.as_egui();
            let fill = egui::Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 77);
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                fill,
                egui::Stroke::new(2.0_f32, colors.primary.as_egui()),
            ));
        }
    }

    fn show_pie_chart(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();

        if self.data.is_empty() {
            ui.label("No data available");
            return;
        }

        let center = egui::pos2(ui.available_width() / 2.0, ui.available_height() / 2.0);
        let radius = (ui.available_width().min(ui.available_height()) / 2.0) - 20.0;

        // Calculate total
        let total: f32 = self.data.iter().map(|p| p.y).sum::<f32>().max(f32::EPSILON);

        // Draw pie slices
        let mut current_angle = 0.0f64;
        for point in &self.data {
            let slice_angle = (point.y as f64 / total as f64) * 2.0 * std::f64::consts::PI;
            let end_angle = current_angle + slice_angle;

            // Create pie slice points
            let mut points = vec![egui::pos2(center.x, center.y)];

            let mut angle = current_angle;
            while angle < end_angle {
                let x = center.x + radius * angle.cos() as f32;
                let y = center.y + radius * angle.sin() as f32;
                points.push(egui::pos2(x, y));
                angle += 5.0f64.to_radians();
            }

            points.push(egui::pos2(
                center.x + radius * end_angle.cos() as f32,
                center.y + radius * end_angle.sin() as f32,
            ));

            // Draw slice
            let color = point.color.unwrap_or_else(|| {
                // Generate colors from palette
                let hue = (current_angle / (2.0 * std::f64::consts::PI)) * 360.0;
                hsl_to_color32(hue as f32, 0.7, 0.5)
            });

            ui.painter().add(egui::Shape::convex_polygon(
                points,
                color,
                egui::Stroke::new(1.0_f32, colors.border.as_egui()),
            ));

            current_angle = end_angle;
        }
    }

    /// Mutable access to the underlying data (builder alternative).
    pub fn data_mut(&mut self) -> &mut Vec<ChartDataPoint> {
        &mut self.data
    }
}

impl Default for Chart {
    fn default() -> Self {
        Self::new("chart", ChartType::Line)
    }
}

/// Chart builder for convenience
pub struct ChartBuilder {
    chart: Chart,
}

impl ChartBuilder {
    pub fn new(id: &str, chart_type: ChartType) -> Self {
        Self {
            chart: Chart::new(id, chart_type),
        }
    }

    pub fn data(mut self, data: Vec<ChartDataPoint>) -> Self {
        self.chart.data = data;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.chart.title = Some(title.into());
        self
    }

    pub fn x_axis(mut self, label: &str) -> Self {
        self.chart.x_axis = Some(label.to_string());
        self
    }

    pub fn y_axis(mut self, label: &str) -> Self {
        self.chart.y_axis = Some(label.to_string());
        self
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.chart.width = Some(width);
        self.chart.height = Some(height);
        self
    }

    pub fn build(self) -> Chart {
        self.chart
    }
}

/// Helper function to convert HSL to Color32
fn hsl_to_color32(h: f32, s: f32, l: f32) -> egui::Color32 {
    // Simplified HSL to RGB conversion
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = if (0.0..60.0).contains(&h) {
        (c, x, 0.0)
    } else if (60.0..120.0).contains(&h) {
        (x, c, 0.0)
    } else if (120.0..180.0).contains(&h) {
        (0.0, c, x)
    } else if (180.0..240.0).contains(&h) {
        (0.0, x, c)
    } else if (240.0..300.0).contains(&h) {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    egui::Color32::from_rgba_unmultiplied(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
        255,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chart_creation() {
        let chart = Chart::new("test", ChartType::Line);
        assert_eq!(chart.id, "test");
        assert_eq!(chart.chart_type, ChartType::Line);
        assert!(chart.data.is_empty());
        assert!(chart.title.is_none());
        assert!(chart.x_axis.is_none());
        assert!(chart.y_axis.is_none());
        assert!(chart.width.is_none());
        assert!(chart.height.is_none());
    }

    #[test]
    fn test_chart_builder() {
        let data = vec![
            ChartDataPoint {
                x: 1.0,
                y: 10.0,
                label: None,
                color: None,
            },
            ChartDataPoint {
                x: 2.0,
                y: 20.0,
                label: None,
                color: None,
            },
        ];

        let chart = ChartBuilder::new("test", ChartType::Bar)
            .data(data)
            .title("Sales Chart")
            .x_axis("Months")
            .y_axis("Revenue")
            .size(500.0, 400.0)
            .build();

        assert_eq!(chart.id, "test");
        assert_eq!(chart.chart_type, ChartType::Bar);
        assert_eq!(chart.data.len(), 2);
        assert_eq!(chart.title, Some("Sales Chart".to_string()));
        assert_eq!(chart.x_axis, Some("Months".to_string()));
        assert_eq!(chart.y_axis, Some("Revenue".to_string()));
        assert_eq!(chart.width, Some(500.0));
        assert_eq!(chart.height, Some(400.0));
    }

    #[test]
    fn test_chart_types() {
        assert_eq!(ChartType::Line, ChartType::Line);
        assert_eq!(ChartType::Bar, ChartType::Bar);
        assert_eq!(ChartType::Area, ChartType::Area);
        assert_eq!(ChartType::Pie, ChartType::Pie);
    }

    #[test]
    fn test_chart_data_point() {
        let point = ChartDataPoint {
            x: 1.0,
            y: 10.0,
            label: Some("Test".to_string()),
            color: Some(egui::Color32::RED),
        };

        assert_eq!(point.x, 1.0);
        assert_eq!(point.y, 10.0);
        assert_eq!(point.label, Some("Test".to_string()));
        assert_eq!(point.color, Some(egui::Color32::RED));
    }
}
