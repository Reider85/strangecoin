//! Design tokens - single source of truth for the Strangecoin UI
//!
//! All UI components should use these tokens rather than hardcoded values.
//! This ensures consistency and enables easy theme switching.

/// Color palette with 7 hues × 10 shades
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorPalette {
    pub primary: [Color; 11],
    pub secondary: [Color; 11],
    pub accent: [Color; 11],
    pub success: [Color; 11],
    pub warning: [Color; 11],
    pub error: [Color; 11],
    pub neutral: [Color; 11],
}

impl Default for ColorPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorPalette {
    pub fn new() -> Self {
        Self {
            primary: [
                Color::new(0xef, 0xf6, 0xff), // 50
                Color::new(0xdb, 0xea, 0xfe), // 100
                Color::new(0xbf, 0xdb, 0xfe), // 200
                Color::new(0x93, 0xc5, 0xfd), // 300
                Color::new(0x60, 0xa5, 0xfa), // 400
                Color::new(0x3b, 0x82, 0xf6), // 500
                Color::new(0x25, 0x63, 0xeb), // 600
                Color::new(0x1d, 0x4e, 0xd8), // 700
                Color::new(0x1e, 0x40, 0xaf), // 800
                Color::new(0x1e, 0x3a, 0x8a), // 900
                Color::new(0x17, 0x25, 0x54), // 950
            ],
            secondary: [
                Color::new(0xf8, 0xfa, 0xfc), // 50
                Color::new(0xf1, 0xf5, 0xf9), // 100
                Color::new(0xe2, 0xe8, 0xf0), // 200
                Color::new(0xcb, 0xd5, 0xe1), // 300
                Color::new(0x94, 0xa3, 0xb8), // 400
                Color::new(0x64, 0x74, 0x8b), // 500
                Color::new(0x47, 0x55, 0x69), // 600
                Color::new(0x33, 0x41, 0x55), // 700
                Color::new(0x1e, 0x29, 0x3b), // 800
                Color::new(0x0f, 0x17, 0x2a), // 900
                Color::new(0x02, 0x06, 0x17), // 950
            ],
            accent: [
                Color::new(0xfd, 0xf4, 0xff), // 50
                Color::new(0xfa, 0xe8, 0xff), // 100
                Color::new(0xf5, 0xd0, 0xfe), // 200
                Color::new(0xf0, 0xab, 0xfc), // 300
                Color::new(0xe8, 0x79, 0xf9), // 400
                Color::new(0xd9, 0x46, 0xef), // 500
                Color::new(0xc0, 0x26, 0xd3), // 600
                Color::new(0xa2, 0x1c, 0xaf), // 700
                Color::new(0x86, 0x19, 0x8f), // 800
                Color::new(0x70, 0x1a, 0x75), // 900
                Color::new(0x4a, 0x04, 0x4e), // 950
            ],
            success: [
                Color::new(0xf0, 0xfd, 0xf4), // 50
                Color::new(0xdc, 0xfc, 0xe7), // 100
                Color::new(0xbb, 0xf7, 0xd0), // 200
                Color::new(0x86, 0xef, 0xac), // 300
                Color::new(0x4a, 0xde, 0x80), // 400
                Color::new(0x22, 0xc5, 0x5e), // 500
                Color::new(0x16, 0xa3, 0x4a), // 600
                Color::new(0x15, 0x80, 0x3d), // 700
                Color::new(0x16, 0x65, 0x34), // 800
                Color::new(0x14, 0x53, 0x2d), // 900
                Color::new(0x05, 0x2e, 0x16), // 950
            ],
            warning: [
                Color::new(0xff, 0xfb, 0xeb), // 50
                Color::new(0xfe, 0xf3, 0xc7), // 100
                Color::new(0xfd, 0xe6, 0x8a), // 200
                Color::new(0xfc, 0xd3, 0x4d), // 300
                Color::new(0xfb, 0xbf, 0x24), // 400
                Color::new(0xf5, 0x9e, 0x0b), // 500
                Color::new(0xd9, 0x77, 0x06), // 600
                Color::new(0xb4, 0x53, 0x09), // 700
                Color::new(0x92, 0x40, 0x0e), // 800
                Color::new(0x78, 0x35, 0x0f), // 900
                Color::new(0x45, 0x1a, 0x03), // 950
            ],
            error: [
                Color::new(0xfe, 0xf2, 0xf2), // 50
                Color::new(0xfe, 0xe2, 0xe2), // 100
                Color::new(0xfe, 0xca, 0xca), // 200
                Color::new(0xfc, 0xa5, 0xa5), // 300
                Color::new(0xf8, 0x71, 0x71), // 400
                Color::new(0xef, 0x44, 0x44), // 500
                Color::new(0xdc, 0x26, 0x26), // 600
                Color::new(0xb9, 0x1c, 0x1c), // 700
                Color::new(0x99, 0x1b, 0x1b), // 800
                Color::new(0x7f, 0x1d, 0x1d), // 900
                Color::new(0x45, 0x0a, 0x0a), // 950
            ],
            neutral: [
                Color::new(0xfa, 0xfa, 0xfa), // 50
                Color::new(0xf4, 0xf4, 0xf5), // 100
                Color::new(0xe4, 0xe4, 0xe7), // 200
                Color::new(0xd4, 0xd4, 0xd8), // 300
                Color::new(0xa1, 0xa1, 0xaa), // 400
                Color::new(0x71, 0x71, 0x7a), // 500
                Color::new(0x52, 0x52, 0x5b), // 600
                Color::new(0x3f, 0x3f, 0x46), // 700
                Color::new(0x27, 0x27, 0x2a), // 800
                Color::new(0x18, 0x18, 0x1b), // 900
                Color::new(0x09, 0x09, 0x0b), // 950
            ],
        }
    }

    pub fn primary(&self, shade: Shade) -> Color {
        self.primary[shade as usize]
    }

    pub fn secondary(&self, shade: Shade) -> Color {
        self.secondary[shade as usize]
    }

    pub fn accent(&self, shade: Shade) -> Color {
        self.accent[shade as usize]
    }

    pub fn success(&self, shade: Shade) -> Color {
        self.success[shade as usize]
    }

    pub fn warning(&self, shade: Shade) -> Color {
        self.warning[shade as usize]
    }

    pub fn error(&self, shade: Shade) -> Color {
        self.error[shade as usize]
    }

    pub fn neutral(&self, shade: Shade) -> Color {
        self.neutral[shade as usize]
    }
}

/// RGB color representation
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn as_egui(&self) -> egui::Color32 {
        egui::Color32::from_rgb(self.r, self.g, self.b)
    }

    pub fn as_css_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Calculate contrast ratio against another color
    pub fn contrast_ratio(&self, other: &Color) -> f32 {
        let luminance1 = self.relative_luminance();
        let luminance2 = other.relative_luminance();

        let (lighter, darker) = if luminance1 > luminance2 {
            (luminance1, luminance2)
        } else {
            (luminance2, luminance1)
        };

        (lighter + 0.05) / (darker + 0.05)
    }

    fn relative_luminance(&self) -> f32 {
        let rsrgb = self.r as f32 / 255.0;
        let gsrgb = self.g as f32 / 255.0;
        let bsrgb = self.b as f32 / 255.0;

        let r = if rsrgb <= 0.03928 {
            rsrgb / 12.92
        } else {
            ((rsrgb + 0.055) / 1.055).powf(2.4)
        };
        let g = if gsrgb <= 0.03928 {
            gsrgb / 12.92
        } else {
            ((gsrgb + 0.055) / 1.055).powf(2.4)
        };
        let b = if bsrgb <= 0.03928 {
            bsrgb / 12.92
        } else {
            ((bsrgb + 0.055) / 1.055).powf(2.4)
        };

        0.2126 * r + 0.7152 * g + 0.0722 * b
    }
}

/// Color shade levels (50-950)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shade {
    _50 = 0,
    _100 = 1,
    _200 = 2,
    _300 = 3,
    _400 = 4,
    _500 = 5,
    _600 = 6,
    _700 = 7,
    _800 = 8,
    _900 = 9,
    _950 = 10,
}

/// Spacing scale with 8 levels
#[derive(Debug, Clone, Copy)]
pub struct SpacingScale {
    pub xs: f32,   // 4px
    pub sm: f32,   // 8px
    pub md: f32,   // 12px
    pub base: f32, // 16px
    pub lg: f32,   // 24px
    pub xl: f32,   // 32px
    pub _2xl: f32, // 48px
    pub _3xl: f32, // 64px
}

impl Default for SpacingScale {
    fn default() -> Self {
        Self::new()
    }
}

impl SpacingScale {
    pub fn new() -> Self {
        Self {
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            base: 16.0,
            lg: 24.0,
            xl: 32.0,
            _2xl: 48.0,
            _3xl: 64.0,
        }
    }
}

/// Typography scale with 7 sizes
#[derive(Debug, Clone)]
pub struct TypographyScale {
    pub xs: TextStyle,
    pub sm: TextStyle,
    pub base: TextStyle,
    pub lg: TextStyle,
    pub xl: TextStyle,
    pub _2xl: TextStyle,
    pub display: TextStyle,
}

impl Default for TypographyScale {
    fn default() -> Self {
        Self::new()
    }
}

impl TypographyScale {
    pub fn new() -> Self {
        Self {
            xs: TextStyle {
                size: 12.0,
                weight: 400,
                line_height: 1.4,
                font_family: "Inter, system-ui, sans-serif".to_string(),
            },
            sm: TextStyle {
                size: 14.0,
                weight: 400,
                line_height: 1.4,
                font_family: "Inter, system-ui, sans-serif".to_string(),
            },
            base: TextStyle {
                size: 16.0,
                weight: 400,
                line_height: 1.5,
                font_family: "Inter, system-ui, sans-serif".to_string(),
            },
            lg: TextStyle {
                size: 18.0,
                weight: 500,
                line_height: 1.4,
                font_family: "Inter, system-ui, sans-serif".to_string(),
            },
            xl: TextStyle {
                size: 20.0,
                weight: 600,
                line_height: 1.3,
                font_family: "Inter, system-ui, sans-serif".to_string(),
            },
            _2xl: TextStyle {
                size: 24.0,
                weight: 700,
                line_height: 1.2,
                font_family: "Inter, system-ui, sans-serif".to_string(),
            },
            display: TextStyle {
                size: 32.0,
                weight: 700,
                line_height: 1.1,
                font_family: "Inter, system-ui, sans-serif".to_string(),
            },
        }
    }
}

/// Text style definition
#[derive(Debug, Clone)]
pub struct TextStyle {
    pub size: f32,
    pub weight: u32,
    pub line_height: f32,
    pub font_family: String,
}

/// Logical font weight (egui 0.30 has no `FontWeight`; mapped to `RichText::strong`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontWeight {
    Normal,
    Medium,
    SemiBold,
    Bold,
}

impl FontWeight {
    /// Apply this weight to a `RichText` run.
    ///
    /// egui only distinguishes normal vs strong, so Medium/SemiBold/Bold
    /// all map to `strong()`.
    pub fn apply(self, text: egui::RichText) -> egui::RichText {
        match self {
            FontWeight::Normal => text,
            FontWeight::Medium | FontWeight::SemiBold | FontWeight::Bold => text.strong(),
        }
    }
}

/// Border radius scale with 5 levels
#[derive(Debug, Clone, Copy)]
pub struct Radius {
    pub none: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub full: f32,
}

impl Default for Radius {
    fn default() -> Self {
        Self::new()
    }
}

impl Radius {
    pub fn new() -> Self {
        Self {
            none: 0.0,
            sm: 4.0,
            md: 8.0,
            lg: 16.0,
            full: 9999.0,
        }
    }
}

/// Shadow scale with 4 levels
#[derive(Debug, Clone, Copy)]
pub struct Shadow {
    pub none: ShadowValue,
    pub sm: ShadowValue,
    pub md: ShadowValue,
    pub lg: ShadowValue,
}

impl Default for Shadow {
    fn default() -> Self {
        Self::new()
    }
}

impl Shadow {
    pub fn new() -> Self {
        Self {
            none: ShadowValue::none(),
            sm: ShadowValue::new(0.0, 1.0, 2.0, 0.0, 0.05),
            md: ShadowValue::new_double(0.0, 4.0, 6.0, -1.0, 0.1, 2.0, 0.0, 0.1),
            lg: ShadowValue::new_double(0.0, 10.0, 15.0, -3.0, 0.1, 4.0, -4.0, 0.1),
        }
    }
}

/// Shadow value representation
#[derive(Debug, Clone, Copy)]
pub struct ShadowValue {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color_r: f32,
    pub color_g: f32,
    pub color_b: f32,
    pub color_a: f32,
}

impl ShadowValue {
    pub fn none() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color_r: 0.0,
            color_g: 0.0,
            color_b: 0.0,
            color_a: 0.0,
        }
    }

    pub fn new(x: f32, y: f32, blur: f32, spread: f32, color_a: f32) -> Self {
        Self {
            x,
            y,
            blur,
            spread,
            color_r: 0.0,
            color_g: 0.0,
            color_b: 0.0,
            color_a,
        }
    }

    #[allow(clippy::too_many_arguments)] // mirrors CSS box-shadow's two-layer shape
    pub fn new_double(
        x: f32,
        y: f32,
        blur1: f32,
        spread1: f32,
        color_a1: f32,
        _blur2: f32,
        _spread2: f32,
        _color_a2: f32,
    ) -> Self {
        Self {
            x,
            y,
            blur: blur1,
            spread: spread1,
            color_r: 0.0,
            color_g: 0.0,
            color_b: 0.0,
            color_a: color_a1,
        }
    }

    pub fn as_egui(&self) -> egui::epaint::Shadow {
        egui::epaint::Shadow {
            color: egui::Color32::from_rgba_unmultiplied(
                (self.color_r * 255.0) as u8,
                (self.color_g * 255.0) as u8,
                (self.color_b * 255.0) as u8,
                (self.color_a * 255.0) as u8,
            ),
            offset: egui::vec2(self.x, self.y),
            blur: self.blur,
            spread: self.spread,
        }
    }
}

/// Complete design tokens collection
#[derive(Debug, Clone)]
pub struct DesignTokens {
    pub colors: ColorPalette,
    pub spacing: SpacingScale,
    pub typography: TypographyScale,
    pub radius: Radius,
    pub shadow: Shadow,
}

impl DesignTokens {
    pub fn new() -> Self {
        Self {
            colors: ColorPalette::new(),
            spacing: SpacingScale::new(),
            typography: TypographyScale::new(),
            radius: Radius::new(),
            shadow: Shadow::new(),
        }
    }

    /// Get body text color for current theme
    pub fn body_text(&self, is_dark: bool) -> Color {
        if is_dark {
            self.colors.neutral(Shade::_50) // Light text on dark background
        } else {
            self.colors.neutral(Shade::_900) // Dark text on light background
        }
    }

    /// Get background color for current theme
    pub fn background(&self, is_dark: bool) -> Color {
        if is_dark {
            self.colors.neutral(Shade::_950) // Dark background
        } else {
            self.colors.neutral(Shade::_50) // Light background
        }
    }

    /// Check if color contrast meets WCAG AA requirements
    pub fn check_contrast(&self, fg: Color, bg: Color) -> bool {
        fg.contrast_ratio(&bg) >= 4.5
    }
}

impl Default for DesignTokens {
    fn default() -> Self {
        Self::new()
    }
}
