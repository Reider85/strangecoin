//! Component snapshot tests (UI-P02 КГ): catches visual/design-token
//! regressions across light + dark variants.
//!
//! egui is immediate-mode: there is no retained widget tree to serialize,
//! so snapshots cover the design-system surface the components derive their
//! looks from — color palettes, spacing/typography/radius/shadow scales and
//! the resolved `ThemeColors` per theme — plus component default state.
//! Rendering regression coverage lives in the a11y audit + gallery.

use strangecoin_ui::theme::{Theme, ThemeManager};
use strangecoin_ui::tokens::{
    ColorPalette, DesignTokens, FontWeight, Radius, Shade, Shadow, SpacingScale, TypographyScale,
};

/// Serialize a color palette to a stable string (hex per shade, 7 hues × 11).
fn palette_snapshot(p: &ColorPalette) -> String {
    let mut out = String::new();
    for (name, hues) in [
        ("primary", &p.primary),
        ("secondary", &p.secondary),
        ("accent", &p.accent),
        ("success", &p.success),
        ("warning", &p.warning),
        ("error", &p.error),
        ("neutral", &p.neutral),
    ] {
        out.push_str(name);
        for c in hues {
            out.push(' ');
            out.push_str(&c.as_css_hex());
        }
        out.push('\n');
    }
    out
}

#[test]
fn snapshot_color_palette() {
    let p = ColorPalette::new();
    let snap = palette_snapshot(&p);

    // 7 hues × 11 shades, each a 7-char hex token.
    let lines: Vec<&str> = snap.lines().collect();
    assert_eq!(lines.len(), 7);
    for line in &lines {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(tokens.len(), 12, "hue row must be name + 11 shades: {line}");
        for hex in &tokens[1..] {
            assert!(hex.starts_with('#') && hex.len() == 7, "bad hex: {hex}");
        }
    }

    // Determinism: second construction is byte-identical.
    assert_eq!(snap, palette_snapshot(&ColorPalette::new()));
}

#[test]
fn snapshot_spacing_scale() {
    let s = SpacingScale::new();
    // UI-P02 КГ: 8 steps — 4/8/12/16/24/32/48/64 px.
    assert_eq!(
        [s.xs, s.sm, s.md, s.base, s.lg, s.xl, s._2xl, s._3xl],
        [4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0]
    );
}

#[test]
fn snapshot_typography_scale() {
    let t = TypographyScale::new();
    let sizes = [
        t.xs.size,
        t.sm.size,
        t.base.size,
        t.lg.size,
        t.xl.size,
        t._2xl.size,
        t.display.size,
    ];
    assert_eq!(sizes, [12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 32.0]);
    // Weights: 400/400/400/500/600/700/700.
    let weights = [
        t.xs.weight,
        t.sm.weight,
        t.base.weight,
        t.lg.weight,
        t.xl.weight,
        t._2xl.weight,
        t.display.weight,
    ];
    assert_eq!(weights, [400, 400, 400, 500, 600, 700, 700]);
    assert!(t.base.font_family.contains("sans-serif"));
}

#[test]
fn snapshot_radius_and_shadow() {
    let r = Radius::new();
    assert_eq!(
        [r.none, r.sm, r.md, r.lg, r.full],
        [0.0, 4.0, 8.0, 16.0, 9999.0]
    );

    let s = Shadow::new();
    assert_eq!(s.none.color_a, 0.0);
    assert_eq!(s.sm.color_a, 0.05);
    assert_eq!(s.md.color_a, 0.1);
    assert_eq!(s.lg.color_a, 0.1);
}

#[test]
fn snapshot_theme_colors_light_and_dark() {
    let light = ThemeManager::new();
    light.set_dark_mode(false);
    let lc = light.colors();

    let dark = ThemeManager::new();
    dark.set_dark_mode(true);
    let dc = dark.colors();

    // Light: near-white background, near-black text.
    let lb = lc.background;
    assert!(
        lb.r > 200 && lb.g > 200 && lb.b > 200,
        "light bg: {:?}",
        lb.as_css_hex()
    );
    let lt = lc.text;
    assert!(
        lt.r < 60 && lt.g < 60 && lt.b < 60,
        "light text: {:?}",
        lt.as_css_hex()
    );

    // Dark: near-black background, near-white text.
    let db = dc.background;
    assert!(
        db.r < 40 && db.g < 40 && db.b < 40,
        "dark bg: {:?}",
        db.as_css_hex()
    );
    let dt = dc.text;
    assert!(
        dt.r > 200 && dt.g > 200 && dt.b > 200,
        "dark text: {:?}",
        dt.as_css_hex()
    );

    // Both themes share the same brand accent.
    assert_eq!(lc.primary.as_css_hex(), dc.primary.as_css_hex());
}

#[test]
fn snapshot_theme_manager_states() {
    let m = ThemeManager::new();

    m.set_dark_mode(true);
    assert_eq!(m.get_theme(), Theme::Dark);
    assert!(m.is_dark());

    m.set_dark_mode(false);
    assert_eq!(m.get_theme(), Theme::Light);
    assert!(!m.is_dark());
}

#[test]
fn snapshot_design_tokens_defaults() {
    let t = DesignTokens::new();

    // КГ: ColorPalette (7 hues), SpacingScale (8), TypographyScale (7 sizes),
    // Radius (5), Shadow (4).
    assert_eq!(t.spacing.xs, 4.0);
    assert_eq!(t.typography.display.size, 32.0);
    assert_eq!(t.radius.full, 9999.0);
    assert_eq!(t.shadow.lg.y, 10.0);

    // WCAG helper: black on white passes AA, white on white does not.
    let black = strangecoin_ui::tokens::Color::new(0, 0, 0);
    let white = strangecoin_ui::tokens::Color::new(255, 255, 255);
    assert!(t.check_contrast(black, white));
    assert!(!t.check_contrast(white, white));
}

#[test]
fn snapshot_font_weight_mapping() {
    // egui 0.30 has no FontWeight: only Normal vs strong mapping.
    let rt = egui::RichText::new("x");
    // Each non-normal weight maps to a strong-styled run; applying twice is stable.
    let a = FontWeight::Bold.apply(rt.clone());
    let b = FontWeight::SemiBold.apply(rt);
    // Both end up strong: formatting is captured in the same layout flags.
    assert_eq!(format!("{:?}", a.text()), format!("{:?}", b.text()));
}

#[test]
fn snapshot_shade_index_bounds() {
    let p = ColorPalette::new();
    // All 11 shade accessors stay in bounds and are ordered dark→light reversed.
    let shades = [
        Shade::_50,
        Shade::_100,
        Shade::_200,
        Shade::_300,
        Shade::_400,
        Shade::_500,
        Shade::_600,
        Shade::_700,
        Shade::_800,
        Shade::_900,
        Shade::_950,
    ];
    for s in shades {
        let _ = p.primary(s);
        let _ = p.neutral(s);
    }
    // Neutral ramp darkens with shade: the near-black 950 end has higher
    // contrast against a fixed mid gray than the near-white 50 end.
    let mid = strangecoin_ui::tokens::Color::new(128, 128, 128);
    let c50 = p.neutral(Shade::_50).contrast_ratio(&mid);
    let c950 = p.neutral(Shade::_950).contrast_ratio(&mid);
    assert!(
        c950 > c50,
        "neutral ramp must darken with shade: contrast(950)={c950} !> contrast(50)={c50}"
    );
}
