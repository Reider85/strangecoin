//! Tabs component for navigation

use crate::theme::ThemeManager;
use eframe::egui;

/// Tabs component for navigation
pub struct Tabs {
    id: String,
    tabs: Vec<Tab>,
    active_tab: usize,
    orientation: TabOrientation,
    position: TabPosition,
}

/// Tab definition
pub struct Tab {
    id: String,
    label: String,
    content: Box<dyn TabContent>,
    disabled: bool,
}

/// Tab content trait
pub trait TabContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager);
}

impl Tab {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Tab orientations
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabOrientation {
    Horizontal,
    Vertical,
}

/// Tab positions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabPosition {
    Top,
    Bottom,
    Left,
    Right,
}

impl Tabs {
    /// Stable identifier (used for widget ids / a11y labels).
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            tabs: Vec::new(),
            active_tab: 0,
            orientation: TabOrientation::Horizontal,
            position: TabPosition::Top,
        }
    }

    pub fn tab(mut self, id: &str, label: &str, content: Box<dyn TabContent>) -> Self {
        self.tabs.push(Tab {
            id: id.to_string(),
            label: label.into(),
            content,
            disabled: false,
        });
        self
    }

    pub fn orientation(mut self, orientation: TabOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn position(mut self, position: TabPosition) -> Self {
        self.position = position;
        self
    }

    pub fn active_tab(mut self, index: usize) -> Self {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
        self
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let _colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Tab navigation
        match self.orientation {
            TabOrientation::Horizontal => {
                ui.horizontal(|ui| {
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let response = ui.selectable_label(
                            i == self.active_tab,
                            &tab.label,
                        );

                        if response.clicked() && !tab.disabled {
                            self.active_tab = i;
                        }
                    }
                });
            }
            TabOrientation::Vertical => {
                ui.vertical(|ui| {
                    for (i, tab) in self.tabs.iter().enumerate() {
                        let response = ui.selectable_label(
                            i == self.active_tab,
                            &tab.label,
                        );

                        if response.clicked() && !tab.disabled {
                            self.active_tab = i;
                        }
                    }
                });
            }
        }

        ui.add_space(tokens.spacing.sm);

        // Tab content
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.content.show(ui, theme_manager);
        }
    }

    pub fn active_tab_content(&mut self) -> Option<&mut dyn TabContent> {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            Some(tab.content.as_mut())
        } else {
            None
        }
    }

    pub fn active_tab_index(&self) -> usize {
        self.active_tab
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut Vec<Tab> {
        &mut self.tabs
    }
}

impl Default for Tabs {
    fn default() -> Self {
        Self::new("tabs")
    }
}

/// Simple text tab content
pub struct TextTabContent {
    text: String,
}

impl TextTabContent {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl TabContent for TextTabContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        ui.label(
            egui::RichText::new(&self.text)
                .size(tokens.typography.base.size)
        );
    }
}

/// Form tab content
pub struct FormTabContent {
    inputs: Vec<crate::components::Input>,
}

impl Default for FormTabContent {
    fn default() -> Self {
        Self::new()
    }
}

impl FormTabContent {
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

impl TabContent for FormTabContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        ui.spacing_mut().item_spacing.y = tokens.spacing.base;
        
        for input in &mut self.inputs {
            input.show(ui, theme_manager);
        }
    }
}

impl<T: Clone> Default for ListTabContent<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// List tab content
pub struct ListTabContent<T> {
    items: Vec<T>,
    selected_item: Option<usize>,
}

impl<T: Clone> ListTabContent<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_item: None,
        }
    }

    pub fn item(mut self, item: T) -> Self {
        self.items.push(item);
        self
    }

    pub fn selected_item(mut self, index: Option<usize>) -> Self {
        self.selected_item = index;
        self
    }
}

impl<T: Clone + std::fmt::Display> TabContent for ListTabContent<T> {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        
        for (i, item) in self.items.iter().enumerate() {
            let response = ui.selectable_label(
                self.selected_item == Some(i),
                item.to_string(),
            );

            if response.clicked() {
                self.selected_item = Some(i);
            }

            ui.add_space(tokens.spacing.sm);
        }
    }
}

/// Tabs builder for convenience
pub struct TabsBuilder {
    tabs: Tabs,
}

impl TabsBuilder {
    pub fn new(id: &str) -> Self {
        Self {
            tabs: Tabs::new(id),
        }
    }

    pub fn tab<T: TabContent + 'static>(mut self, id: &str, label: &str, content: T) -> Self {
        self.tabs.tabs.push(Tab {
            id: id.to_string(),
            label: label.into(),
            content: Box::new(content),
            disabled: false,
        });
        self
    }

    pub fn orientation(mut self, orientation: TabOrientation) -> Self {
        self.tabs.orientation = orientation;
        self
    }

    pub fn position(mut self, position: TabPosition) -> Self {
        self.tabs.position = position;
        self
    }

    pub fn active_tab(mut self, index: usize) -> Self {
        self.tabs.active_tab = index;
        self
    }

    pub fn build(self) -> Tabs {
        self.tabs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tabs_creation() {
        let tabs = Tabs::new("test_tabs");
        assert_eq!(tabs.id, "test_tabs");
        assert!(tabs.tabs.is_empty());
        assert_eq!(tabs.active_tab, 0);
        assert_eq!(tabs.orientation, TabOrientation::Horizontal);
        assert_eq!(tabs.position, TabPosition::Top);
    }

    #[test]
    fn test_tabs_builder() {
        let tabs = TabsBuilder::new("test_tabs")
            .tab("home", "Home", TextTabContent::new("Home tab"))
            .tab("settings", "Settings", TextTabContent::new("Settings tab"))
            .orientation(TabOrientation::Vertical)
            .position(TabPosition::Left)
            .active_tab(1)
            .build();

        assert_eq!(tabs.id, "test_tabs");
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.tabs[0].id, "home");
        assert_eq!(tabs.tabs[0].label, "Home");
        assert_eq!(tabs.tabs[1].id, "settings");
        assert_eq!(tabs.tabs[1].label, "Settings");
        assert_eq!(tabs.orientation, TabOrientation::Vertical);
        assert_eq!(tabs.position, TabPosition::Left);
        assert_eq!(tabs.active_tab, 1);
    }

    #[test]
    fn test_tab_content() {
        let text_content = TextTabContent::new("Hello, World!");
        assert_eq!(text_content.text, "Hello, World!");

        let mut form_content = FormTabContent::new();
        form_content.inputs.push(crate::components::Input::new("Name"));
        assert_eq!(form_content.inputs.len(), 1);
    }

    #[test]
    fn test_list_tab_content() {
        let list_content = ListTabContent::new()
            .item("Item 1")
            .item("Item 2")
            .selected_item(Some(1));

        assert_eq!(list_content.items.len(), 2);
        assert_eq!(list_content.selected_item, Some(1));
    }

    #[test]
    fn test_tab_orientations() {
        assert_eq!(TabOrientation::Horizontal, TabOrientation::Horizontal);
        assert_eq!(TabOrientation::Vertical, TabOrientation::Vertical);
    }

    #[test]
    fn test_tab_positions() {
        assert_eq!(TabPosition::Top, TabPosition::Top);
        assert_eq!(TabPosition::Bottom, TabPosition::Bottom);
        assert_eq!(TabPosition::Left, TabPosition::Left);
        assert_eq!(TabPosition::Right, TabPosition::Right);
    }
}
