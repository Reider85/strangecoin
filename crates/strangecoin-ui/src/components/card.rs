//! Card component for content containers

use crate::theme::ThemeManager;
use eframe::egui;

/// Card component for content containers
pub struct Card {
    title: Option<String>,
    children: Vec<Box<dyn CardChild>>,
    padding: Option<f32>,
    margin: Option<f32>,
    border_radius: Option<f32>,
    shadow: bool,
    background: Option<egui::Color32>,
}

/// Trait for card children
pub trait CardChild {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager);
}

impl Card {
    pub fn new() -> Self {
        Self {
            title: None,
            children: Vec::new(),
            padding: None,
            margin: None,
            border_radius: None,
            shadow: true,
            background: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn child<T: CardChild + 'static>(mut self, child: T) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn margin(mut self, margin: f32) -> Self {
        self.margin = Some(margin);
        self
    }

    pub fn border_radius(mut self, radius: f32) -> Self {
        self.border_radius = Some(radius);
        self
    }

    pub fn shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }

    pub fn background(mut self, color: egui::Color32) -> Self {
        self.background = Some(color);
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        self.show_with(ui, theme_manager, |_| {});
    }

    /// Render the card frame with additional inline content after the children.
    ///
    /// Used for content that must be laid out inside the card frame but cannot
    /// be stored as a `'static` [`CardChild`] (e.g. `ScrollArea` closures that
    /// borrow screen state).
    pub fn show_with<F>(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, add_contents: F)
    where
        F: FnOnce(&mut egui::Ui),
    {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Calculate dimensions
        let padding = self.padding.unwrap_or(tokens.spacing.lg);
        let border_radius = self.border_radius.unwrap_or(tokens.radius.md);

        // Frame styling
        let frame = egui::Frame::default()
            .fill(self.background.unwrap_or(colors.surface.as_egui()))
            .rounding(egui::Rounding::same(border_radius))
            .inner_margin(egui::Margin::same(padding));

        if self.shadow {
            frame.shadow(tokens.shadow.md.as_egui()).show(ui, |ui| {
                self.render_content(ui, theme_manager);
                add_contents(ui);
            });
        } else {
            frame.show(ui, |ui| {
                self.render_content(ui, theme_manager);
                add_contents(ui);
            });
        }
    }

    fn render_content(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        
        // Title
        if let Some(ref title) = self.title {
            ui.label(
                egui::RichText::new(title)
                    .size(tokens.typography.lg.size)
                    .strong(),
            );
            ui.add_space(tokens.spacing.sm);
        }
        
        // Children
        for child in &mut self.children {
            child.show(ui, theme_manager);
        }
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple text card child
pub struct CardText {
    text: String,
    size: Option<f32>,
    weight: Option<crate::tokens::FontWeight>,
    color: Option<egui::Color32>,
}

impl CardText {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            size: None,
            weight: None,
            color: None,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn weight(mut self, weight: crate::tokens::FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }

    pub fn color(mut self, color: egui::Color32) -> Self {
        self.color = Some(color);
        self
    }
}

impl CardChild for CardText {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        let colors = theme_manager.colors();
        
        let mut rich_text = egui::RichText::new(&self.text);
        
        if let Some(size) = self.size {
            rich_text = rich_text.size(size);
        } else {
            rich_text = rich_text.size(tokens.typography.base.size);
        }
        
        if let Some(weight) = self.weight {
            rich_text = weight.apply(rich_text);
        }
        
        if let Some(color) = self.color {
            rich_text = rich_text.color(color);
        } else {
            rich_text = rich_text.color(colors.text.as_egui());
        }
        
        ui.label(rich_text);
    }
}

/// Card spacer child for adding space
pub struct CardSpacer {
    height: f32,
}

impl CardSpacer {
    pub fn new(height: f32) -> Self {
        Self { height }
    }
}

impl CardChild for CardSpacer {
    fn show(&mut self, ui: &mut egui::Ui, _theme_manager: &ThemeManager) {
        ui.add_space(self.height);
    }
}

/// Card divider child
pub struct CardDivider;

impl CardChild for CardDivider {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        let _colors = theme_manager.colors();
        
        ui.add_space(tokens.spacing.sm);
        ui.horizontal(|ui| {
            ui.separator();
        });
        ui.add_space(tokens.spacing.sm);
    }
}

/// Card section child
pub struct CardSection {
    title: String,
    children: Vec<Box<dyn CardChild>>,
}

impl CardSection {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            children: Vec::new(),
        }
    }

    pub fn child<T: CardChild + 'static>(mut self, child: T) -> Self {
        self.children.push(Box::new(child));
        self
    }
}

impl CardChild for CardSection {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        let colors = theme_manager.colors();

        ui.label(
            egui::RichText::new(&self.title)
                .size(tokens.typography.sm.size)
                .strong()
                .color(colors.text_secondary.as_egui()),
        );
        ui.add_space(tokens.spacing.sm);

        for child in &mut self.children {
            child.show(ui, theme_manager);
        }
    }
}

// ----------------------------------------------------------------------
// CardChild adapters for the reusable components
//
// Every component that renders into a `Ui` with a `(ui, theme_manager)`
// signature can be nested inside a `Card` via `.child(...)`.
// ----------------------------------------------------------------------

impl CardChild for Card {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        Card::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Button {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Button::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Input {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Input::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Stat {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Stat::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Chart {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Chart::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::CodeBlock {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::CodeBlock::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Tabs {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Tabs::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Badge {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Badge::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Avatar {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Avatar::show(self, ui, theme_manager);
    }
}

impl CardChild for crate::components::Breadcrumb {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Breadcrumb::show(self, ui, theme_manager);
    }
}

impl<T: Clone + std::fmt::Debug + 'static> CardChild for crate::components::Table<T> {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        crate::components::Table::show(self, ui, theme_manager);
    }
}

/// Card builder for convenience
pub struct CardBuilder {
    card: Card,
}

impl Default for CardBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl CardBuilder {
    pub fn new() -> Self {
        Self {
            card: Card::new(),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.card.title = Some(title.into());
        self
    }

    pub fn padding(mut self, padding: f32) -> Self {
        self.card.padding = Some(padding);
        self
    }

    pub fn margin(mut self, margin: f32) -> Self {
        self.card.margin = Some(margin);
        self
    }

    pub fn border_radius(mut self, radius: f32) -> Self {
        self.card.border_radius = Some(radius);
        self
    }

    pub fn shadow(mut self, shadow: bool) -> Self {
        self.card.shadow = shadow;
        self
    }

    pub fn background(mut self, color: egui::Color32) -> Self {
        self.card.background = Some(color);
        self
    }

    pub fn text(mut self, text: &str) -> Self {
        self.card.children.push(Box::new(CardText::new(text)));
        self
    }

    pub fn text_styled(mut self, text: &str, size: f32, weight: crate::tokens::FontWeight) -> Self {
        self.card.children.push(Box::new(
            CardText::new(text).size(size).weight(weight)
        ));
        self
    }

    pub fn spacer(mut self, height: f32) -> Self {
        self.card.children.push(Box::new(CardSpacer::new(height)));
        self
    }

    pub fn divider(mut self) -> Self {
        self.card.children.push(Box::new(CardDivider));
        self
    }

    pub fn section(self, title: &str) -> SectionBuilder {
        SectionBuilder::new(self, title)
    }

    pub fn build(self) -> Card {
        self.card
    }
}

/// Builder for card sections
pub struct SectionBuilder {
    card_builder: CardBuilder,
    section: CardSection,
}

impl SectionBuilder {
    pub fn new(card_builder: CardBuilder, title: &str) -> Self {
        Self {
            card_builder,
            section: CardSection::new(title),
        }
    }

    pub fn text(mut self, text: &str) -> Self {
        self.section.children.push(Box::new(CardText::new(text)));
        self
    }

    pub fn text_styled(mut self, text: &str, size: f32, weight: crate::tokens::FontWeight) -> Self {
        self.section.children.push(Box::new(
            CardText::new(text).size(size).weight(weight)
        ));
        self
    }

    pub fn spacer(mut self, height: f32) -> Self {
        self.section.children.push(Box::new(CardSpacer::new(height)));
        self
    }

    pub fn done(mut self) -> CardBuilder {
        self.card_builder.card.children.push(Box::new(self.section));
        self.card_builder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_creation() {
        let card = Card::new();
        assert!(card.title.is_none());
        assert!(card.children.is_empty());
        assert!(card.padding.is_none());
        assert!(card.margin.is_none());
        assert_eq!(card.border_radius, None);
        assert!(card.shadow);
        assert!(card.background.is_none());
    }

    #[test]
    fn test_card_builder() {
        let card = CardBuilder::new()
            .title("Test Card")
            .padding(16.0)
            .margin(8.0)
            .shadow(false)
            .text("Hello, World!")
            .build();

        assert_eq!(card.title, Some("Test Card".to_string()));
        assert_eq!(card.padding, Some(16.0));
        assert_eq!(card.margin, Some(8.0));
        assert!(!card.shadow);
        assert_eq!(card.children.len(), 1);
    }

    #[test]
    fn test_card_section_builder() {
        let card = CardBuilder::new()
            .title("Main Card")
            .section("Header")
                .text("Welcome")
                .done()
            .section("Content")
                .text("Hello, World!")
                .done()
            .build();

        assert_eq!(card.children.len(), 2); // Two sections
    }

    #[test]
    fn test_card_text() {
        let text = CardText::new("Hello");
        assert_eq!(text.text, "Hello");
        assert!(text.size.is_none());
        assert!(text.weight.is_none());
        assert!(text.color.is_none());
    }

    #[test]
    fn test_card_text_styling() {
        let text = CardText::new("Hello")
            .size(16.0)
            .weight(crate::tokens::FontWeight::Bold)
            .color(egui::Color32::RED);

        assert_eq!(text.size, Some(16.0));
        assert_eq!(text.weight, Some(crate::tokens::FontWeight::Bold));
        assert_eq!(text.color, Some(egui::Color32::RED));
    }
}