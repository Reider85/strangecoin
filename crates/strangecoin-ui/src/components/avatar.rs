//! Avatar component for user profile images

use crate::accessibility::WidgetProbe;
use crate::focus::WidgetMeta;
use crate::theme::ThemeManager;
use eframe::egui;

/// Avatar component for user profile images
pub struct Avatar {
    id: String,
    name: String,
    image: Option<String>,
    size: AvatarSize,
    shape: AvatarShape,
    online: bool,
    on_click: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Avatar sizes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AvatarSize {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
}

/// Avatar shapes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AvatarShape {
    Circle,
    Square,
}

impl Avatar {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }

    fn a11y_meta(&self) -> WidgetMeta {
        WidgetMeta::new("avatar", "status", self.name.clone())
    }

    pub fn a11y_probe(&self, theme: &ThemeManager) -> WidgetProbe {
        let colors = theme.colors();
        WidgetProbe::non_interactive(self.a11y_meta(), colors.text, colors.surface)
    }

    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            image: None,
            size: AvatarSize::Md,
            shape: AvatarShape::Circle,
            online: false,
            on_click: None,
        }
    }

    pub fn image(mut self, image: &str) -> Self {
        self.image = Some(image.to_string());
        self
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }

    pub fn shape(mut self, shape: AvatarShape) -> Self {
        self.shape = shape;
        self
    }

    pub fn online(mut self, online: bool) -> Self {
        self.online = online;
        self
    }

    pub fn on_click<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_click = Some(Box::new(callback));
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let _tokens = theme_manager.tokens();

        // Calculate size
        let size = match self.size {
            AvatarSize::Xs => 24.0,
            AvatarSize::Sm => 32.0,
            AvatarSize::Md => 48.0,
            AvatarSize::Lg => 64.0,
            AvatarSize::Xl => 80.0,
        };

        // Calculate colors
        let bg_color = colors.surface.as_egui();
        let text_color = colors.text.as_egui();
        let online_color = if self.online {
            colors.success.as_egui()
        } else {
            colors.text_secondary.as_egui()
        };

        // Avatar frame
        let frame = egui::Frame::default()
            .fill(bg_color)
            .rounding(if self.shape == AvatarShape::Circle {
                egui::Rounding::same(size / 2.0)
            } else {
                egui::Rounding::same(4.0)
            })
            .inner_margin(egui::Margin::same(2.0));

        frame.show(ui, |ui| {
            // Avatar content: placeholder with the first letter of the name
            // (image loading lands with UI-P05 polish).
            let first_letter = self.name.chars().next().unwrap_or('?');
            ui.label(
                egui::RichText::new(first_letter.to_string())
                    .size(size * 0.6)
                    .color(text_color),
            );
        });

        // Online indicator
        if self.online {
            let indicator_size = size * 0.25;
            let indicator_offset = size * 0.8;

            ui.painter().circle_filled(
                egui::pos2(
                    ui.min_rect().max.x - indicator_size / 2.0,
                    ui.min_rect().min.y + indicator_offset,
                ),
                indicator_size / 2.0,
                online_color,
            );
        }

        // Handle click
        if ui.rect_contains_pointer(ui.min_rect()) && ui.input(|i| i.pointer.primary_clicked()) {
            if let Some(ref callback) = self.on_click {
                callback();
            }
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn set_image(&mut self, image: &str) {
        self.image = Some(image.to_string());
    }

    pub fn set_online(&mut self, online: bool) {
        self.online = online;
    }
}

impl Default for Avatar {
    fn default() -> Self {
        Self::new("avatar", "User")
    }
}

/// Avatar builder for convenience
pub struct AvatarBuilder {
    avatar: Avatar,
}

impl AvatarBuilder {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            avatar: Avatar::new(id, name),
        }
    }

    pub fn image(mut self, image: &str) -> Self {
        self.avatar.image = Some(image.to_string());
        self
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.avatar.size = size;
        self
    }

    pub fn shape(mut self, shape: AvatarShape) -> Self {
        self.avatar.shape = shape;
        self
    }

    pub fn online(mut self, online: bool) -> Self {
        self.avatar.online = online;
        self
    }

    pub fn on_click<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.avatar.on_click = Some(Box::new(callback));
        self
    }

    pub fn build(self) -> Avatar {
        self.avatar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_avatar_creation() {
        let avatar = Avatar::new("test", "John Doe");
        assert_eq!(avatar.id, "test");
        assert_eq!(avatar.name, "John Doe");
        assert!(avatar.image.is_none());
        assert_eq!(avatar.size, AvatarSize::Md);
        assert_eq!(avatar.shape, AvatarShape::Circle);
        assert!(!avatar.online);
        assert!(avatar.on_click.is_none());
    }

    #[test]
    fn test_avatar_builder() {
        let avatar = AvatarBuilder::new("test", "Jane Doe")
            .image("avatar.jpg")
            .size(AvatarSize::Lg)
            .shape(AvatarShape::Square)
            .online(true)
            .build();

        assert_eq!(avatar.id, "test");
        assert_eq!(avatar.name, "Jane Doe");
        assert_eq!(avatar.image, Some("avatar.jpg".to_string()));
        assert_eq!(avatar.size, AvatarSize::Lg);
        assert_eq!(avatar.shape, AvatarShape::Square);
        assert!(avatar.online);
    }

    #[test]
    fn test_avatar_sizes() {
        assert_eq!(AvatarSize::Xs, AvatarSize::Xs);
        assert_eq!(AvatarSize::Sm, AvatarSize::Sm);
        assert_eq!(AvatarSize::Md, AvatarSize::Md);
        assert_eq!(AvatarSize::Lg, AvatarSize::Lg);
        assert_eq!(AvatarSize::Xl, AvatarSize::Xl);
    }

    #[test]
    fn test_avatar_shapes() {
        assert_eq!(AvatarShape::Circle, AvatarShape::Circle);
        assert_eq!(AvatarShape::Square, AvatarShape::Square);
    }
}
