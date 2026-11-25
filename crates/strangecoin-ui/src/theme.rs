//! Theme management for Strangecoin UI
//!
//! Handles light/dark/system themes with persistence to config.

use crate::tokens::{Color, DesignTokens, Shade};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Theme variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
    System,
}

#[allow(clippy::derivable_impls)] // derive would pick the first variant (Light), System is the intended default
impl Default for Theme {
    fn default() -> Self {
        Self::System
    }
}

impl Theme {
    pub fn as_config_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::System => "system",
        }
    }

    pub fn from_config_str(raw: &str) -> Option<Self> {
        match raw {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            "system" => Some(Theme::System),
            _ => None,
        }
    }
}

impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Theme::Light => write!(f, "Light"),
            Theme::Dark => write!(f, "Dark"),
            Theme::System => write!(f, "System"),
        }
    }
}

/// Theme manager with persistence and application
pub struct ThemeManager {
    current: Mutex<Theme>,
    config_path: PathBuf,
    tokens: DesignTokens,
    system_dark: bool,
}

impl ThemeManager {
    pub fn new() -> Self {
        Self::with_config_path(Self::get_config_path())
    }

    pub fn with_config_path(config_path: PathBuf) -> Self {
        let current = Self::load_from_config(&config_path).unwrap_or_default();

        Self {
            current: Mutex::new(current),
            config_path,
            tokens: DesignTokens::new(),
            system_dark: false,
        }
    }

    pub fn with_theme(config_path: PathBuf, theme: Theme) -> Self {
        let manager = Self::with_config_path(config_path);
        if let Ok(mut current) = manager.current.lock() {
            *current = theme;
        }
        manager
    }

    pub fn with_system_dark(mut self, dark: bool) -> Self {
        self.system_dark = dark;
        self
    }

    pub fn set_theme(&self, theme: Theme) {
        if let Ok(mut current) = self.current.lock() {
            *current = theme;
        }
        self.save_to_config(theme);
    }

    pub fn get_theme(&self) -> Theme {
        self.current.lock().map(|t| *t).unwrap_or_default()
    }

    /// Sync accessor for UI code (legacy name kept for call sites).
    pub fn current(&self) -> Theme {
        self.get_theme()
    }

    pub fn set_dark_mode(&self, dark: bool) {
        self.set_theme(if dark { Theme::Dark } else { Theme::Light });
    }

    pub fn is_dark(&self) -> bool {
        match self.get_theme() {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => self.system_dark,
        }
    }

    /// Config file this manager persists ui.theme to (UI-P02 bridge).
    pub fn config_path(&self) -> &std::path::Path {
        &self.config_path
    }

    pub fn tokens(&self) -> &DesignTokens {
        &self.tokens
    }

    pub fn apply_to_egui(&self, ctx: &egui::Context) {
        let is_dark = self.is_dark();

        let mut visuals = if is_dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        let bg = self.tokens.background(is_dark).as_egui();
        let border = if is_dark {
            self.tokens.colors.neutral(Shade::_600).as_egui()
        } else {
            self.tokens.colors.neutral(Shade::_300).as_egui()
        };
        let text = self.tokens.body_text(is_dark).as_egui();

        visuals.dark_mode = is_dark;
        visuals.override_text_color = Some(text);
        visuals.window_fill = bg;
        visuals.panel_fill = bg;
        visuals.faint_bg_color = if is_dark {
            self.tokens.colors.neutral(Shade::_800).as_egui()
        } else {
            self.tokens.colors.neutral(Shade::_200).as_egui()
        };
        visuals.extreme_bg_color = if is_dark {
            self.tokens.colors.neutral(Shade::_900).as_egui()
        } else {
            self.tokens.colors.neutral(Shade::_100).as_egui()
        };
        visuals.code_bg_color = visuals.extreme_bg_color;
        visuals.hyperlink_color = self.tokens.colors.primary(Shade::_500).as_egui();
        visuals.window_stroke = egui::Stroke::new(1.0_f32, border);
        visuals.panel_fill = bg;

        let widget_rounding = egui::Rounding::same(4.0);
        let stroke = |color: egui::Color32| egui::Stroke::new(1.0_f32, color);

        visuals.widgets.noninteractive = egui::style::WidgetVisuals {
            weak_bg_fill: bg,
            bg_fill: bg,
            bg_stroke: stroke(border),
            fg_stroke: stroke(text),
            rounding: widget_rounding,
            expansion: 0.0,
        };
        visuals.widgets.inactive = egui::style::WidgetVisuals {
            weak_bg_fill: if is_dark {
                self.tokens.colors.neutral(Shade::_800).as_egui()
            } else {
                self.tokens.colors.neutral(Shade::_100).as_egui()
            },
            bg_fill: if is_dark {
                self.tokens.colors.neutral(Shade::_800).as_egui()
            } else {
                self.tokens.colors.neutral(Shade::_100).as_egui()
            },
            bg_stroke: stroke(border),
            fg_stroke: stroke(if is_dark {
                self.tokens.colors.neutral(Shade::_100).as_egui()
            } else {
                self.tokens.colors.neutral(Shade::_900).as_egui()
            }),
            rounding: widget_rounding,
            expansion: 0.0,
        };
        visuals.widgets.hovered = egui::style::WidgetVisuals {
            weak_bg_fill: if is_dark {
                self.tokens.colors.primary(Shade::_900).as_egui()
            } else {
                self.tokens.colors.primary(Shade::_100).as_egui()
            },
            bg_fill: if is_dark {
                self.tokens.colors.primary(Shade::_900).as_egui()
            } else {
                self.tokens.colors.primary(Shade::_100).as_egui()
            },
            bg_stroke: stroke(self.tokens.colors.primary(Shade::_400).as_egui()),
            fg_stroke: stroke(self.tokens.colors.primary(Shade::_700).as_egui()),
            rounding: widget_rounding,
            expansion: 1.0,
        };
        visuals.widgets.active = egui::style::WidgetVisuals {
            weak_bg_fill: self.tokens.colors.primary(Shade::_500).as_egui(),
            bg_fill: self.tokens.colors.primary(Shade::_500).as_egui(),
            bg_stroke: stroke(self.tokens.colors.primary(Shade::_700).as_egui()),
            fg_stroke: stroke(egui::Color32::WHITE),
            rounding: widget_rounding,
            expansion: 1.0,
        };
        visuals.widgets.open = egui::style::WidgetVisuals {
            weak_bg_fill: if is_dark {
                self.tokens.colors.neutral(Shade::_800).as_egui()
            } else {
                self.tokens.colors.neutral(Shade::_200).as_egui()
            },
            bg_fill: bg,
            bg_stroke: stroke(border),
            fg_stroke: stroke(text),
            rounding: widget_rounding,
            expansion: 0.0,
        };
        visuals.selection = egui::style::Selection {
            bg_fill: self.tokens.colors.primary(Shade::_500).as_egui(),
            stroke: stroke(self.tokens.colors.primary(Shade::_600).as_egui()),
        };
        visuals.warn_fg_color = self.tokens.colors.warning(Shade::_500).as_egui();
        visuals.error_fg_color = self.tokens.colors.error(Shade::_500).as_egui();

        ctx.set_visuals(visuals);
    }

    fn get_config_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_default()
            .join("config.toml")
    }

    pub fn load_from_config(path: &Path) -> Option<Theme> {
        let content = fs::read_to_string(path).ok()?;
        let value: toml::Value = toml::from_str(&content).ok()?;
        let raw = value.get("ui")?.get("theme")?.as_str()?;
        Theme::from_config_str(raw)
    }

    pub fn save_to_config(&self, theme: Theme) {
        let content = fs::read_to_string(&self.config_path).unwrap_or_default();
        let mut value: toml::Value = if content.trim().is_empty() {
            toml::Value::Table(toml::map::Map::new())
        } else {
            match toml::from_str(&content) {
                Ok(value) => value,
                Err(e) => {
                    tracing::warn!(error = %e, "cannot parse config for ui.theme persistence");
                    return;
                }
            }
        };
        let table = value.as_table_mut().expect("toml root is a table");
        let ui = table
            .entry("ui")
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        let ui_table = ui.as_table_mut().expect("ui section is a table");
        ui_table.insert(
            "theme".to_string(),
            toml::Value::String(theme.as_config_str().to_string()),
        );
        let serialized = match toml::to_string(&value) {
            Ok(serialized) => serialized,
            Err(e) => {
                tracing::warn!(error = %e, "cannot serialize ui.theme to config");
                return;
            }
        };
        if let Err(e) = fs::write(&self.config_path, serialized) {
            tracing::warn!(error = %e, "cannot persist ui.theme to config");
        }
    }

    /// Get theme-specific colors
    pub fn colors(&self) -> ThemeColors {
        let is_dark = self.is_dark();
        ThemeColors {
            background: self.tokens.background(is_dark),
            surface: if is_dark {
                self.tokens.colors.neutral(Shade::_800)
            } else {
                self.tokens.colors.neutral(Shade::_100)
            },
            text: self.tokens.body_text(is_dark),
            text_secondary: if is_dark {
                self.tokens.colors.neutral(Shade::_400)
            } else {
                self.tokens.colors.neutral(Shade::_600)
            },
            border: if is_dark {
                self.tokens.colors.neutral(Shade::_600)
            } else {
                self.tokens.colors.neutral(Shade::_300)
            },
            primary: self.tokens.colors.primary(Shade::_500),
            secondary: self.tokens.colors.secondary(Shade::_500),
            accent: self.tokens.colors.accent(Shade::_500),
            success: self.tokens.colors.success(Shade::_500),
            warning: self.tokens.colors.warning(Shade::_500),
            error: self.tokens.colors.error(Shade::_500),
            info: self.tokens.colors.primary(Shade::_500),
        }
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Theme-specific color collection
#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    pub background: Color,
    pub surface: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub border: Color,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contrast_ratio() {
        let tokens = DesignTokens::new();
        let bg = tokens.colors.neutral(Shade::_50);
        let fg = tokens.colors.neutral(Shade::_900);

        assert!(tokens.check_contrast(fg, bg));
    }

    #[test]
    fn test_theme_colors() {
        let manager = ThemeManager::new();
        let colors = manager.colors();

        assert_ne!(colors.background, Color::new(0, 0, 0)); // Not black
        assert_ne!(colors.text, Color::new(255, 255, 255)); // Not white
    }

    fn temp_config() -> PathBuf {
        std::env::temp_dir().join(format!(
            "strangecoin-ui-theme-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn test_theme_switching() {
        let path = temp_config();
        let _ = fs::remove_file(&path);
        let manager = ThemeManager::with_config_path(path.clone());
        manager.set_dark_mode(true);
        assert_eq!(manager.get_theme(), Theme::Dark);
        assert!(manager.is_dark());
        manager.set_dark_mode(false);
        assert_eq!(manager.get_theme(), Theme::Light);
        assert!(!manager.is_dark());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn theme_persists_to_ui_table_and_preserves_other_keys() {
        let path = temp_config();
        let _ = fs::remove_file(&path);
        fs::write(&path, "network_id = 3\n[ui]\ntheme = \"system\"\n").expect("fixture");
        let manager = ThemeManager::with_config_path(path.clone());
        assert_eq!(manager.get_theme(), Theme::System);
        manager.set_theme(Theme::Dark);
        assert_eq!(ThemeManager::load_from_config(&path), Some(Theme::Dark));
        let contents = fs::read_to_string(&path).expect("saved config");
        assert!(contents.contains("network_id = 3"));
        assert!(contents.contains("theme = \"dark\""));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn system_theme_resolves_from_injected_value() {
        let path = temp_config();
        let _ = fs::remove_file(&path);
        let manager = ThemeManager::with_config_path(path.clone()).with_system_dark(true);
        assert_eq!(manager.get_theme(), Theme::System);
        assert!(manager.is_dark());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn invalid_config_theme_is_ignored() {
        assert_eq!(Theme::from_config_str("neon"), None);
        assert_eq!(Theme::from_config_str("dark"), Some(Theme::Dark));
    }
}
