//! Modal component for dialogs and overlays

use crate::components::Button;
use crate::theme::ThemeManager;
use eframe::egui;

/// Modal component for dialogs and overlays
pub struct Modal {
    id: String,
    title: Option<String>,
    content: Box<dyn ModalContent>,
    open: bool,
    size: ModalSize,
    closable: bool,
    on_close: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Modal content trait
pub trait ModalContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager);
}

/// Modal sizes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModalSize {
    Sm,
    Md,
    Lg,
    Full,
}

impl Modal {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            title: None,
            content: Box::new(EmptyModalContent),
            open: false,
            size: ModalSize::Md,
            closable: true,
            on_close: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn content<T: ModalContent + 'static>(mut self, content: T) -> Self {
        self.content = Box::new(content);
        self
    }

    pub fn size(mut self, size: ModalSize) -> Self {
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

        // Calculate modal size
        let (modal_width, modal_height) = match self.size {
            ModalSize::Sm => (300.0, 200.0),
            ModalSize::Md => (400.0, 300.0),
            ModalSize::Lg => (600.0, 400.0),
            ModalSize::Full => (ctx.screen_rect().width() * 0.9, ctx.screen_rect().height() * 0.9),
        };

        // Center the modal
        let screen_rect = ctx.screen_rect();
        let modal_rect = egui::Rect::from_min_size(
            screen_rect.center() - egui::vec2(modal_width / 2.0, modal_height / 2.0),
            egui::vec2(modal_width, modal_height),
        );

        // Overlay background
        ctx.layer_painter(egui::LayerId::background())
            .rect_filled(screen_rect, egui::Rounding::ZERO, colors.background.as_egui());

        // Modal frame
        egui::Window::new(self.title.as_deref().unwrap_or("Modal"))
            .id(egui::Id::new(&self.id))
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(modal_width, modal_height))
            .default_pos(modal_rect.min)
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
                ui.set_max_height(modal_height - 60.0); // Leave space for buttons
                self.content.show(ui, theme_manager);

                // Action buttons
                ui.add_space(tokens.spacing.lg);
                ui.horizontal(|ui| {
                    if self.closable {
                        Button::new("Cancel")
                            .variant(crate::components::ButtonVariant::Secondary)
                            .onclick(|| {
                                println!("Modal cancelled");
                            })
                            .show(ui, theme_manager);
                    }

                    Button::new("Confirm")
                        .variant(crate::components::ButtonVariant::Primary)
                        .onclick(|| {
                            println!("Modal confirmed");
                        })
                        .show(ui, theme_manager);
                });
            });
    }
}

impl Default for Modal {
    fn default() -> Self {
        Self::new("modal")
    }
}

/// Empty modal content
struct EmptyModalContent;

impl ModalContent for EmptyModalContent {
    fn show(&mut self, _ui: &mut egui::Ui, _theme_manager: &ThemeManager) {
        // Empty content
    }
}

/// Simple text modal content
pub struct TextModalContent {
    text: String,
}

impl TextModalContent {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl ModalContent for TextModalContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        ui.label(
            egui::RichText::new(&self.text)
                .size(tokens.typography.base.size)
        );
    }
}

/// Form modal content
pub struct FormModalContent {
    inputs: Vec<crate::components::Input>,
}

impl Default for FormModalContent {
    fn default() -> Self {
        Self::new()
    }
}

impl FormModalContent {
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
        }
    }

    pub fn input(mut self, input: crate::components::Input) -> Self {
        self.inputs.push(input);
        self
    }
}

impl ModalContent for FormModalContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        ui.spacing_mut().item_spacing.y = tokens.spacing.base;
        
        for input in &mut self.inputs {
            input.show(ui, theme_manager);
        }
    }
}

/// Modal builder for convenience
pub struct ModalBuilder {
    modal: Modal,
}

impl ModalBuilder {
    pub fn new(id: &str) -> Self {
        Self {
            modal: Modal::new(id),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.modal.title = Some(title.into());
        self
    }

    pub fn content<T: ModalContent + 'static>(mut self, content: T) -> Self {
        self.modal.content = Box::new(content);
        self
    }

    pub fn size(mut self, size: ModalSize) -> Self {
        self.modal.size = size;
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.modal.closable = closable;
        self
    }

    pub fn on_close<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.modal.on_close = Some(Box::new(callback));
        self
    }

    pub fn build(self) -> Modal {
        self.modal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_creation() {
        let modal = Modal::new("test_modal");
        assert_eq!(modal.id, "test_modal");
        assert!(modal.title.is_none());

        assert!(!modal.open);
        assert_eq!(modal.size, ModalSize::Md);
        assert!(modal.closable);
        assert!(modal.on_close.is_none());
    }

    #[test]
    fn test_modal_builder() {
        let modal = ModalBuilder::new("test_modal")
            .title("Test Modal")
            .size(ModalSize::Lg)
            .closable(false)
            .build();

        assert_eq!(modal.id, "test_modal");
        assert_eq!(modal.title, Some("Test Modal".to_string()));
        assert_eq!(modal.size, ModalSize::Lg);
        assert!(!modal.closable);
    }

    #[test]
    fn test_text_modal_content() {
        let content = TextModalContent::new("Hello, World!");
        assert_eq!(content.text, "Hello, World!");
    }

    #[test]
    fn test_form_modal_content() {
        let mut content = FormModalContent::new();
        content.inputs.push(crate::components::Input::new("Name"));
        assert_eq!(content.inputs.len(), 1);
    }

    #[test]
    fn test_modal_sizes() {
        assert_eq!(ModalSize::Sm, ModalSize::Sm);
        assert_eq!(ModalSize::Md, ModalSize::Md);
        assert_eq!(ModalSize::Lg, ModalSize::Lg);
        assert_eq!(ModalSize::Full, ModalSize::Full);
    }
}
