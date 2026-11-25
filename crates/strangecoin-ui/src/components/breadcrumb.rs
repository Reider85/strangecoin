//! Breadcrumb component for navigation trails

use crate::accessibility::WidgetProbe;
use crate::focus::{self, WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::theme::ThemeManager;
use eframe::egui;

/// Breadcrumb component for navigation trails
pub struct Breadcrumb {
    id: String,
    items: Vec<BreadcrumbItem>,
    separator: String,
    on_click: Option<Box<dyn Fn(String) + Send + Sync>>,
}

/// Breadcrumb item
pub struct BreadcrumbItem {
    id: String,
    label: String,
    disabled: bool,
}

impl Breadcrumb {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }

    fn a11y_meta(&self) -> WidgetMeta {
        let label = self
            .items
            .first()
            .map(|item| item.label.clone())
            .unwrap_or_else(|| self.id.clone());
        WidgetMeta::new("breadcrumb", "link", label)
    }

    pub fn a11y_probe(&self, theme: &ThemeManager) -> WidgetProbe {
        let colors = theme.colors();
        WidgetProbe::interactive(
            self.a11y_meta(),
            colors.text,
            colors.background,
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        )
        .with_focus(true, FOCUS_OUTLINE_WIDTH)
    }

    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            items: Vec::new(),
            separator: "/".to_string(),
            on_click: None,
        }
    }

    pub fn item(mut self, id: &str, label: &str) -> Self {
        self.items.push(BreadcrumbItem {
            id: id.to_string(),
            label: label.into(),
            disabled: false,
        });
        self
    }

    pub fn disabled_item(mut self, id: &str, label: &str) -> Self {
        self.items.push(BreadcrumbItem {
            id: id.to_string(),
            label: label.into(),
            disabled: true,
        });
        self
    }

    pub fn separator(mut self, separator: &str) -> Self {
        self.separator = separator.to_string();
        self
    }

    pub fn on_click<F>(mut self, callback: F) -> Self
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.on_click = Some(Box::new(callback));
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let focus_color = colors.text;
        let _tokens = theme_manager.tokens();

        ui.horizontal(|ui| {
            for (i, item) in self.items.iter().enumerate() {
                if i > 0 {
                    ui.label(&self.separator);
                }

                if item.disabled {
                    ui.label(
                        egui::RichText::new(&item.label).color(colors.text_secondary.as_egui()),
                    );
                } else {
                    let meta =
                        WidgetMeta::new(format!("breadcrumb-{}", i), "link", item.label.clone());
                    let response = ui
                        .push_id(focus::widget_id(&meta), |ui| {
                            ui.set_min_size(focus::min_touch_size(egui::Vec2::ZERO));
                            ui.add(egui::Label::new(&item.label).sense(egui::Sense::click()))
                        })
                        .inner;
                    focus::apply_focus_ring(ui, &response, focus_color.as_egui());

                    if response.clicked() {
                        if let Some(ref callback) = self.on_click {
                            callback(item.id.clone());
                        }
                    }
                }
            }
        });
    }

    pub fn items(&self) -> &[BreadcrumbItem] {
        &self.items
    }

    pub fn items_mut(&mut self) -> &mut Vec<BreadcrumbItem> {
        &mut self.items
    }
}

impl Default for Breadcrumb {
    fn default() -> Self {
        Self::new("breadcrumb")
    }
}

/// Breadcrumb builder for convenience
pub struct BreadcrumbBuilder {
    breadcrumb: Breadcrumb,
}

impl BreadcrumbBuilder {
    pub fn new(id: &str) -> Self {
        Self {
            breadcrumb: Breadcrumb::new(id),
        }
    }

    pub fn item(mut self, id: &str, label: &str) -> Self {
        self.breadcrumb.items.push(BreadcrumbItem {
            id: id.to_string(),
            label: label.into(),
            disabled: false,
        });
        self
    }

    pub fn disabled_item(mut self, id: &str, label: &str) -> Self {
        self.breadcrumb.items.push(BreadcrumbItem {
            id: id.to_string(),
            label: label.into(),
            disabled: true,
        });
        self
    }

    pub fn separator(mut self, separator: &str) -> Self {
        self.breadcrumb.separator = separator.to_string();
        self
    }

    pub fn on_click<F>(mut self, callback: F) -> Self
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.breadcrumb.on_click = Some(Box::new(callback));
        self
    }

    pub fn build(self) -> Breadcrumb {
        self.breadcrumb
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_breadcrumb_creation() {
        let breadcrumb = Breadcrumb::new("test");
        assert_eq!(breadcrumb.id, "test");
        assert!(breadcrumb.items.is_empty());
        assert_eq!(breadcrumb.separator, "/");
        assert!(breadcrumb.on_click.is_none());
    }

    #[test]
    fn test_breadcrumb_builder() {
        let breadcrumb = BreadcrumbBuilder::new("test")
            .item("home", "Home")
            .item("dashboard", "Dashboard")
            .separator("›")
            .build();

        assert_eq!(breadcrumb.id, "test");
        assert_eq!(breadcrumb.items.len(), 2);
        assert_eq!(breadcrumb.items[0].id, "home");
        assert_eq!(breadcrumb.items[0].label, "Home");
        assert!(!breadcrumb.items[0].disabled);
        assert_eq!(breadcrumb.items[1].id, "dashboard");
        assert_eq!(breadcrumb.items[1].label, "Dashboard");
        assert!(!breadcrumb.items[1].disabled);
        assert_eq!(breadcrumb.separator, "›");
    }
}
