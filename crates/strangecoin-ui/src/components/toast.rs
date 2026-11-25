//! Toast component for notifications

use crate::accessibility::WidgetProbe;
use crate::focus::{self, WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::theme::{ThemeColors, ThemeManager};
use crate::tokens::{Color, Shade};
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Toast component for notifications
pub struct Toast {
    id: String,
    message: String,
    level: ToastLevel,
    visible: bool,
    duration: Option<Duration>,
    on_close: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Toast levels
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl Toast {
    pub fn new(id: &str, message: &str, level: ToastLevel) -> Self {
        Self {
            id: id.to_string(),
            message: message.to_string(),
            level,
            visible: false,
            duration: Some(Duration::from_secs(3)),
            on_close: None,
        }
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    pub fn on_close<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_close = Some(Box::new(callback));
        self
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        if let Some(ref callback) = self.on_close {
            callback();
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    fn a11y_meta(&self) -> WidgetMeta {
        WidgetMeta::new("toast", "button", self.message.clone())
    }

    pub fn a11y_probe(&self, theme: &ThemeManager) -> WidgetProbe {
        let colors = theme.colors();
        WidgetProbe::interactive(
            self.a11y_meta(),
            self.message_ink(theme),
            colors.surface,
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        )
        .with_focus(true, FOCUS_OUTLINE_WIDTH)
    }

    pub fn show_in_context(&mut self, ctx: &egui::Context, theme_manager: &ThemeManager) {
        if !self.visible {
            return;
        }

        let colors = theme_manager.colors();
        let focus_color = colors.text;
        let tokens = theme_manager.tokens();
        let message_ink = self.message_ink(theme_manager);

        // Calculate toast position (top-right corner)
        let screen_rect = ctx.screen_rect();
        let toast_width = 300.0;
        let toast_height = 60.0;
        let toast_rect = egui::Rect::from_min_size(
            egui::pos2(
                screen_rect.max.x - toast_width - tokens.spacing.lg,
                screen_rect.min.y + tokens.spacing.lg,
            ),
            egui::vec2(toast_width, toast_height),
        );

        // Toast frame
        egui::Window::new(&self.message)
            .id(egui::Id::new(&self.id))
            .collapsible(false)
            .resizable(false)
            .default_pos(toast_rect.min)
            .frame(
                egui::Frame::default()
                    .fill(self.get_color(&colors))
                    .rounding(egui::Rounding::same(tokens.radius.md))
                    .shadow(tokens.shadow.md.as_egui())
                    .inner_margin(egui::Margin::same(tokens.spacing.md)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Icon
                    let icon = match self.level {
                        ToastLevel::Info => "ℹ️",
                        ToastLevel::Success => "✅",
                        ToastLevel::Warning => "⚠️",
                        ToastLevel::Error => "❌",
                    };
                    ui.label(icon);

                    // Message
                    ui.label(
                        egui::RichText::new(&self.message)
                            .size(tokens.typography.sm.size)
                            .color(message_ink.as_egui()),
                    );

                    // Close button
                    let meta = WidgetMeta::new("toast-close", "button", "Close");
                    let response = ui
                        .push_id(focus::widget_id(&meta), |ui| {
                            ui.add(
                                egui::Button::new("×")
                                    .min_size(focus::min_touch_size(egui::Vec2::ZERO)),
                            )
                        })
                        .inner;
                    focus::apply_focus_ring(ui, &response, focus_color.as_egui());
                    if response.clicked() {
                        self.hide();
                    }
                });
            });
    }

    fn message_ink(&self, theme: &ThemeManager) -> Color {
        let colors = theme.colors();
        let ink = theme.tokens().colors.neutral(Shade::_950);
        if colors.surface.contrast_ratio(&colors.text) >= colors.surface.contrast_ratio(&ink) {
            colors.text
        } else {
            ink
        }
    }

    fn level_color(&self, colors: &ThemeColors) -> Color {
        match self.level {
            ToastLevel::Info => colors.info,
            ToastLevel::Success => colors.success,
            ToastLevel::Warning => colors.warning,
            ToastLevel::Error => colors.error,
        }
    }

    fn get_color(&self, colors: &ThemeColors) -> egui::Color32 {
        self.level_color(colors).as_egui()
    }
}

impl Default for Toast {
    fn default() -> Self {
        Self::new("toast", "Default toast message", ToastLevel::Info)
    }
}

/// Toast manager for handling multiple toasts
pub struct ToastManager {
    toasts: Vec<Toast>,
    sender: Sender<ToastAction>,
    receiver: Receiver<ToastAction>,
}

/// Toast actions
pub enum ToastAction {
    Show(String, String, ToastLevel),
    Hide(String),
    HideAll,
}

impl ToastManager {
    pub fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            toasts: Vec::new(),
            sender,
            receiver,
        }
    }

    pub fn add(&mut self, id: &str, message: &str, level: ToastLevel) -> Toast {
        let toast = Toast::new(id, message, level);
        self.toasts.push(Toast::new(id, message, level));
        toast
    }

    pub fn remove(&mut self, id: &str) {
        self.toasts.retain(|t| t.id != id);
    }

    pub fn clear(&mut self) {
        self.toasts.clear();
    }

    pub fn show(&mut self, ctx: &egui::Context, theme_manager: &ThemeManager) {
        // Process actions
        while let Ok(action) = self.receiver.try_recv() {
            match action {
                ToastAction::Show(id, message, level) => {
                    if let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) {
                        toast.message = message;
                        toast.level = level;
                        toast.show();
                    }
                }
                ToastAction::Hide(id) => {
                    if let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) {
                        toast.hide();
                    }
                }
                ToastAction::HideAll => {
                    for toast in &mut self.toasts {
                        toast.hide();
                    }
                }
            }
        }

        // Show visible toasts
        for toast in &mut self.toasts {
            if toast.visible {
                toast.show_in_context(ctx, theme_manager);
            }
        }
    }

    pub fn sender(&self) -> Sender<ToastAction> {
        self.sender.clone()
    }
}

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global toast manager instance (interior-mutability free: `OnceLock` gives
/// safe `Sync` access without `static mut` UB).
static TOAST_MANAGER: std::sync::OnceLock<Arc<Mutex<ToastManager>>> = std::sync::OnceLock::new();

/// Initialize global toast manager
pub fn init_toast_manager() -> Arc<Mutex<ToastManager>> {
    TOAST_MANAGER
        .get_or_init(|| Arc::new(Mutex::new(ToastManager::new())))
        .clone()
}

/// Get global toast manager
pub fn get_toast_manager() -> Option<Arc<Mutex<ToastManager>>> {
    TOAST_MANAGER.get().cloned()
}

/// Convenience functions for showing toasts
pub fn show_toast(id: &str, message: &str, level: ToastLevel) {
    if let Some(manager) = get_toast_manager() {
        let sender = manager.lock().unwrap().sender();
        {
            let _ = sender.send(ToastAction::Show(
                id.to_string(),
                message.to_string(),
                level,
            ));
        }
    }
}

pub fn hide_toast(id: &str) {
    if let Some(manager) = get_toast_manager() {
        let sender = manager.lock().unwrap().sender();
        {
            let _ = sender.send(ToastAction::Hide(id.to_string()));
        }
    }
}

pub fn hide_all_toasts() {
    if let Some(manager) = get_toast_manager() {
        let sender = manager.lock().unwrap().sender();
        {
            let _ = sender.send(ToastAction::HideAll);
        }
    }
}

/// Toast builder for convenience
pub struct ToastBuilder {
    toast: Toast,
}

impl ToastBuilder {
    pub fn new(id: &str, message: &str, level: ToastLevel) -> Self {
        Self {
            toast: Toast::new(id, message, level),
        }
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.toast.duration = Some(duration);
        self
    }

    pub fn on_close<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.toast.on_close = Some(Box::new(callback));
        self
    }

    pub fn build(self) -> Toast {
        self.toast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toast_creation() {
        let toast = Toast::new("test", "Hello", ToastLevel::Info);
        assert_eq!(toast.id, "test");
        assert_eq!(toast.message, "Hello");
        assert_eq!(toast.level, ToastLevel::Info);
        assert!(!toast.visible);
        assert_eq!(toast.duration, Some(Duration::from_secs(3)));
        assert!(toast.on_close.is_none());
    }

    #[test]
    fn test_toast_builder() {
        let toast = ToastBuilder::new("test", "Hello", ToastLevel::Success)
            .duration(Duration::from_secs(5))
            .build();

        assert_eq!(toast.id, "test");
        assert_eq!(toast.message, "Hello");
        assert_eq!(toast.level, ToastLevel::Success);
        assert_eq!(toast.duration, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_toast_levels() {
        assert_eq!(ToastLevel::Info, ToastLevel::Info);
        assert_eq!(ToastLevel::Success, ToastLevel::Success);
        assert_eq!(ToastLevel::Warning, ToastLevel::Warning);
        assert_eq!(ToastLevel::Error, ToastLevel::Error);
    }

    #[test]
    fn test_toast_manager() {
        let mut manager = ToastManager::new();
        let toast = manager.add("test", "Hello", ToastLevel::Info);

        assert_eq!(manager.toasts.len(), 1);
        assert_eq!(toast.id, "test");
        assert_eq!(toast.message, "Hello");
        assert_eq!(toast.level, ToastLevel::Info);
    }

    #[test]
    fn test_toast_manager_actions() {
        let mut manager = ToastManager::new();

        // Add toast
        manager.add("test", "Hello", ToastLevel::Info);
        assert_eq!(manager.toasts.len(), 1);

        // Remove toast
        manager.remove("test");
        assert_eq!(manager.toasts.len(), 0);

        // Clear all toasts
        manager.add("test1", "Hello1", ToastLevel::Info);
        manager.add("test2", "Hello2", ToastLevel::Info);
        manager.clear();
        assert_eq!(manager.toasts.len(), 0);
    }

    #[test]
    fn test_convenience_functions() {
        // These functions require a global toast manager to be initialized
        // In a real application, you would call init_toast_manager() first
        show_toast("test", "Hello", ToastLevel::Info);
        hide_toast("test");
        hide_all_toasts();
    }
}
