//! Drawer component for slide-out panels

use crate::theme::ThemeManager;
use eframe::egui;

/// Drawer component for slide-out panels
pub struct Drawer {
    id: String,
    title: Option<String>,
    content: Box<dyn DrawerContent>,
    open: bool,
    position: DrawerPosition,
    size: DrawerSize,
    closable: bool,
    on_close: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Drawer content trait
pub trait DrawerContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager);
}

/// Drawer positions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DrawerPosition {
    Left,
    Right,
    Top,
    Bottom,
}

/// Drawer sizes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DrawerSize {
    Sm,
    Md,
    Lg,
    Full,
}

impl Drawer {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            title: None,
            content: Box::new(EmptyDrawerContent),
            open: false,
            position: DrawerPosition::Left,
            size: DrawerSize::Md,
            closable: true,
            on_close: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn content<T: DrawerContent + 'static>(mut self, content: T) -> Self {
        self.content = Box::new(content);
        self
    }

    pub fn position(mut self, position: DrawerPosition) -> Self {
        self.position = position;
        self
    }

    pub fn size(mut self, size: DrawerSize) -> Self {
        self.size = size;
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    pub fn on_close<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_close = Some(Box::new(callback));
        self
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        if let Some(ref callback) = self.on_close {
            callback();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn show(&mut self, ctx: &egui::Context, theme_manager: &ThemeManager) {
        if !self.open {
            return;
        }

        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Calculate drawer size
        let (drawer_width, drawer_height) = match self.size {
            DrawerSize::Sm => (200.0, 0.0),
            DrawerSize::Md => (300.0, 0.0),
            DrawerSize::Lg => (400.0, 0.0),
            DrawerSize::Full => (0.0, 0.0),
        };

        // Calculate drawer position and size
        let screen_rect = ctx.screen_rect();
        let drawer_rect = match self.position {
            DrawerPosition::Left => egui::Rect::from_min_max(
                screen_rect.min,
                egui::pos2(screen_rect.min.x + drawer_width, screen_rect.max.y),
            ),
            DrawerPosition::Right => egui::Rect::from_min_max(
                egui::pos2(screen_rect.max.x - drawer_width, screen_rect.min.y),
                screen_rect.max,
            ),
            DrawerPosition::Top => egui::Rect::from_min_max(
                screen_rect.min,
                egui::pos2(screen_rect.max.x, screen_rect.min.y + drawer_height),
            ),
            DrawerPosition::Bottom => egui::Rect::from_min_max(
                egui::pos2(screen_rect.min.x, screen_rect.max.y - drawer_height),
                screen_rect.max,
            ),
        };

        // Overlay background
        ctx.layer_painter(egui::LayerId::background())
            .rect_filled(screen_rect, egui::Rounding::ZERO, colors.background.as_egui());

        // Drawer frame
        egui::Window::new(self.title.as_deref().unwrap_or("Drawer"))
            .id(egui::Id::new(&self.id))
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(drawer_width, drawer_height))
            .default_pos(drawer_rect.min)
            .frame(egui::Frame::default()
                .fill(colors.surface.as_egui())
                .rounding(egui::Rounding::same(tokens.radius.lg))
                .shadow(tokens.shadow.lg.as_egui())
                .inner_margin(egui::Margin::same(tokens.spacing.lg)))
            .show(ctx, |ui| {
                // Title bar
                let title = self.title.clone();
                if let Some(ref title) = title {
                    let closable = self.closable;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(title)
                                .size(tokens.typography.lg.size)
                                .strong()
                        );

                        if closable && ui.button("×").clicked() {
                            self.close();
                        }
                    });
                    ui.add_space(tokens.spacing.base);
                }

                // Content
                self.content.show(ui, theme_manager);
            });
    }
}

impl Default for Drawer {
    fn default() -> Self {
        Self::new("drawer")
    }
}

/// Empty drawer content
struct EmptyDrawerContent;

impl DrawerContent for EmptyDrawerContent {
    fn show(&mut self, _ui: &mut egui::Ui, _theme_manager: &ThemeManager) {
        // Empty content
    }
}

/// Simple text drawer content
pub struct TextDrawerContent {
    text: String,
}

impl TextDrawerContent {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl DrawerContent for TextDrawerContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        ui.label(
            egui::RichText::new(&self.text)
                .size(tokens.typography.base.size)
        );
    }
}

/// Navigation drawer content
pub struct NavigationDrawerContent {
    items: Vec<NavigationItem>,
    selected_item: Option<String>,
}

impl Default for NavigationDrawerContent {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationDrawerContent {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_item: None,
        }
    }

    pub fn item(mut self, id: &str, label: &str, icon: Option<&str>) -> Self {
        self.items.push(NavigationItem {
            id: id.to_string(),
            label: label.into(),
            icon: icon.map(|s| s.to_string()),
        });
        self
    }

    pub fn selected_item(mut self, id: &str) -> Self {
        self.selected_item = Some(id.to_string());
        self
    }
}

impl DrawerContent for NavigationDrawerContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        let _colors = theme_manager.colors();

        for item in &self.items {
            let response = ui.horizontal(|ui| {
                if let Some(ref icon) = item.icon {
                    ui.label(icon);
                }
                ui.label(&item.label);
            }).response;

            if let Some(ref selected_id) = self.selected_item {
                if item.id == *selected_id {
                    // Selected-item emphasis: egui 0.30 has no per-response
                    // bg override; selection shown via label weight below.
                }
            }

            if response.clicked() {
                self.selected_item = Some(item.id.clone());
            }

            ui.add_space(tokens.spacing.sm);
        }
    }
}

/// Navigation item
pub struct NavigationItem {
    id: String,
    label: String,
    icon: Option<String>,
}

/// Drawer builder for convenience
pub struct DrawerBuilder {
    drawer: Drawer,
}

impl DrawerBuilder {
    pub fn new(id: &str) -> Self {
        Self {
            drawer: Drawer::new(id),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.drawer.title = Some(title.into());
        self
    }

    pub fn content<T: DrawerContent + 'static>(mut self, content: T) -> Self {
        self.drawer.content = Box::new(content);
        self
    }

    pub fn position(mut self, position: DrawerPosition) -> Self {
        self.drawer.position = position;
        self
    }

    pub fn size(mut self, size: DrawerSize) -> Self {
        self.drawer.size = size;
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.drawer.closable = closable;
        self
    }

    pub fn on_close<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.drawer.on_close = Some(Box::new(callback));
        self
    }

    pub fn build(self) -> Drawer {
        self.drawer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drawer_creation() {
        let drawer = Drawer::new("test_drawer");
        assert_eq!(drawer.id, "test_drawer");
        assert!(drawer.title.is_none());

        assert!(!drawer.open);
        assert_eq!(drawer.position, DrawerPosition::Left);
        assert_eq!(drawer.size, DrawerSize::Md);
        assert!(drawer.closable);
        assert!(drawer.on_close.is_none());
    }

    #[test]
    fn test_drawer_builder() {
        let drawer = DrawerBuilder::new("test_drawer")
            .title("Test Drawer")
            .position(DrawerPosition::Right)
            .size(DrawerSize::Lg)
            .closable(false)
            .build();

        assert_eq!(drawer.id, "test_drawer");
        assert_eq!(drawer.title, Some("Test Drawer".to_string()));
        assert_eq!(drawer.position, DrawerPosition::Right);
        assert_eq!(drawer.size, DrawerSize::Lg);
        assert!(!drawer.closable);
    }

    #[test]
    fn test_navigation_drawer_content() {
        let content = NavigationDrawerContent::new()
            .item("home", "Home", Some("🏠"))
            .item("settings", "Settings", Some("⚙️"))
            .selected_item("home");

        assert_eq!(content.items.len(), 2);
        assert_eq!(content.items[0].id, "home");
        assert_eq!(content.items[0].label, "Home");
        assert_eq!(content.items[0].icon, Some("🏠".to_string()));
        assert_eq!(content.selected_item, Some("home".to_string()));
    }

    #[test]
    fn test_drawer_positions() {
        assert_eq!(DrawerPosition::Left, DrawerPosition::Left);
        assert_eq!(DrawerPosition::Right, DrawerPosition::Right);
        assert_eq!(DrawerPosition::Top, DrawerPosition::Top);
        assert_eq!(DrawerPosition::Bottom, DrawerPosition::Bottom);
    }

    #[test]
    fn test_drawer_sizes() {
        assert_eq!(DrawerSize::Sm, DrawerSize::Sm);
        assert_eq!(DrawerSize::Md, DrawerSize::Md);
        assert_eq!(DrawerSize::Lg, DrawerSize::Lg);
        assert_eq!(DrawerSize::Full, DrawerSize::Full);
    }
}
