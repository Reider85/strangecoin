use eframe::egui;

pub const MIN_TOUCH_TARGET: f32 = 44.0;
pub const FOCUS_OUTLINE_WIDTH: f32 = 2.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetMeta {
    pub name: String,
    pub role: &'static str,
    pub label: String,
}

impl WidgetMeta {
    pub fn new(name: impl Into<String>, role: &'static str, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            role,
            label: label.into(),
        }
    }

    pub fn is_interactive(&self) -> bool {
        matches!(
            self.role,
            "button" | "textbox" | "checkbox" | "tab" | "link" | "combobox"
        )
    }
}

pub fn widget_id(meta: &WidgetMeta) -> egui::Id {
    egui::Id::new((meta.role, meta.name.as_str()))
}

pub fn min_touch_size(size: egui::Vec2) -> egui::Vec2 {
    size.max(egui::vec2(MIN_TOUCH_TARGET, MIN_TOUCH_TARGET))
}

pub fn focus_stroke(color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new(FOCUS_OUTLINE_WIDTH, color)
}

pub fn apply_focus_ring(ui: &egui::Ui, response: &egui::Response, color: egui::Color32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(FOCUS_OUTLINE_WIDTH),
            egui::Rounding::same(4.0),
            focus_stroke(color),
        );
    }
}
