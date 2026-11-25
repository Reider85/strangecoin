use strangecoin_ui::accessibility::{WCAG_AA_CONTRAST, WCAG_AA_LARGE_CONTRAST};
use strangecoin_ui::tokens::{Color, DesignTokens, Shade};

#[test]
fn body_text_on_background_passes_aa_in_light_theme() {
    let tokens = DesignTokens::new();
    let ratio = tokens
        .body_text(false)
        .contrast_ratio(&tokens.background(false));
    assert!(
        ratio >= WCAG_AA_CONTRAST,
        "light body text contrast {ratio:.2}:1 must be >= {WCAG_AA_CONTRAST}:1"
    );
}

#[test]
fn body_text_on_background_passes_aa_in_dark_theme() {
    let tokens = DesignTokens::new();
    let ratio = tokens
        .body_text(true)
        .contrast_ratio(&tokens.background(true));
    assert!(
        ratio >= WCAG_AA_CONTRAST,
        "dark body text contrast {ratio:.2}:1 must be >= {WCAG_AA_CONTRAST}:1"
    );
}

#[test]
fn body_text_on_surface_passes_aa_in_both_themes() {
    let tokens = DesignTokens::new();
    for dark in [false, true] {
        let surface = if dark {
            tokens.colors.neutral(Shade::_800)
        } else {
            tokens.colors.neutral(Shade::_100)
        };
        let ratio = tokens.body_text(dark).contrast_ratio(&surface);
        assert!(
            ratio >= WCAG_AA_CONTRAST,
            "body text on surface (dark={dark}) {ratio:.2}:1 must be >= {WCAG_AA_CONTRAST}:1"
        );
    }
}

#[test]
fn status_ink_on_saturated_fills_passes_aa_in_both_themes() {
    let tokens = DesignTokens::new();
    let ink = tokens.colors.neutral(Shade::_950);
    for fill in [
        tokens.colors.primary(Shade::_500),
        tokens.colors.success(Shade::_500),
        tokens.colors.warning(Shade::_500),
        tokens.colors.error(Shade::_500),
    ] {
        let ratio = ink.contrast_ratio(&fill);
        assert!(
            ratio >= WCAG_AA_CONTRAST,
            "ink {} on fill {} is {ratio:.2}:1, below {WCAG_AA_CONTRAST}:1",
            ink.as_css_hex(),
            fill.as_css_hex()
        );
    }
}

#[test]
fn large_text_threshold_is_relaxed_to_three_to_one() {
    assert!((WCAG_AA_LARGE_CONTRAST - 3.0).abs() < f32::EPSILON);
    let tokens = DesignTokens::new();
    let ink = tokens.colors.neutral(Shade::_950);
    let fill = tokens.colors.primary(Shade::_500);
    let ratio = ink.contrast_ratio(&fill);
    assert!(
        ratio >= WCAG_AA_LARGE_CONTRAST,
        "large-text contrast {ratio:.2}:1 must be >= {WCAG_AA_LARGE_CONTRAST}:1"
    );
}

#[test]
fn contrast_ratio_is_symmetric_and_deterministic() {
    let tokens = DesignTokens::new();
    let fg = tokens.body_text(false);
    let bg = tokens.background(false);
    let forward = fg.contrast_ratio(&bg);
    let reverse = bg.contrast_ratio(&fg);
    assert!((forward - reverse).abs() < f32::EPSILON);
    assert_eq!(forward, fg.contrast_ratio(&bg));
    assert!(forward > 1.0);
}

#[test]
fn identical_colors_have_minimum_contrast() {
    let white = Color::new(255, 255, 255);
    assert!((white.contrast_ratio(&white) - 1.0).abs() < f32::EPSILON);
    assert!(white.contrast_ratio(&white) < WCAG_AA_CONTRAST);
}
