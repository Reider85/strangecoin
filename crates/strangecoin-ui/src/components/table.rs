//! Table component for displaying data

use crate::accessibility::WidgetProbe;
use crate::focus::{self, WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::theme::ThemeManager;
use eframe::egui;

/// Table component for displaying data
pub struct Table<T> {
    columns: Vec<Column>,
    data: Vec<T>,
    selected_row: Option<usize>,
    sort_column: Option<usize>,
    sort_direction: SortDirection,
    row_height: f32,
    striped: bool,
    hoverable: bool,
}

/// Column definition
pub struct Column {
    pub id: String,
    pub label: String,
    pub width: Option<f32>,
    pub align: Align,
    pub sortable: bool,
}

/// Text alignment
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl<T: Clone + std::fmt::Debug> Table<T> {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            data: Vec::new(),
            selected_row: None,
            sort_column: None,
            sort_direction: SortDirection::Ascending,
            row_height: 32.0,
            striped: true,
            hoverable: true,
        }
    }

    pub fn column(mut self, id: &str, label: &str) -> Self {
        self.columns.push(Column {
            id: id.to_string(),
            label: label.into(),
            width: None,
            align: Align::Left,
            sortable: false,
        });
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        if let Some(col) = self.columns.last_mut() {
            col.width = Some(width);
        }
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        if let Some(col) = self.columns.last_mut() {
            col.align = align;
        }
        self
    }

    pub fn sortable(mut self, sortable: bool) -> Self {
        if let Some(col) = self.columns.last_mut() {
            col.sortable = sortable;
        }
        self
    }

    pub fn data(mut self, data: Vec<T>) -> Self {
        self.data = data;
        self
    }

    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height;
        self
    }

    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    pub fn hoverable(mut self, hoverable: bool) -> Self {
        self.hoverable = hoverable;
        self
    }

    fn a11y_meta(&self) -> WidgetMeta {
        let label = self
            .columns
            .first()
            .map(|column| column.label.clone())
            .unwrap_or_else(|| "table".to_string());
        WidgetMeta::new("table", "button", label)
    }

    pub fn a11y_probe(&self, theme: &ThemeManager) -> WidgetProbe {
        let colors = theme.colors();
        WidgetProbe::interactive(
            self.a11y_meta(),
            colors.text,
            colors.surface,
            MIN_TOUCH_TARGET,
            MIN_TOUCH_TARGET,
        )
        .with_focus(true, FOCUS_OUTLINE_WIDTH)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let focus_color = colors.text;
        let tokens = theme_manager.tokens();

        // Table container
        egui::Frame::default()
            .inner_margin(egui::Margin::symmetric(tokens.spacing.base, 0.0))
            .show(ui, |ui| {
                // Header row
                let header: Vec<(usize, String, bool, bool)> = self
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(i, col)| {
                        (
                            i,
                            col.label.clone(),
                            col.sortable,
                            self.sort_column == Some(i),
                        )
                    })
                    .collect();
                let sort_dir = self.sort_direction;
                let mut clicked_sort: Option<usize> = None;

                ui.horizontal(|ui| {
                    for (i, label, sortable, is_sorted) in &header {
                        ui.horizontal(|ui| {
                            ui.label(label);

                            if *sortable {
                                if *is_sorted {
                                    let icon = match sort_dir {
                                        SortDirection::Ascending => "↑",
                                        SortDirection::Descending => "↓",
                                    };
                                    ui.label(egui::RichText::new(icon).size(12.0));
                                }

                                let meta = WidgetMeta::new(
                                    format!("table-sort-{}", i),
                                    "button",
                                    label.to_string(),
                                );
                                let response = ui
                                    .push_id(focus::widget_id(&meta), |ui| {
                                        ui.add(
                                            egui::Button::new("↕")
                                                .small()
                                                .min_size(focus::min_touch_size(egui::Vec2::ZERO)),
                                        )
                                    })
                                    .inner;
                                focus::apply_focus_ring(ui, &response, focus_color.as_egui());
                                if response.clicked() {
                                    clicked_sort = Some(*i);
                                }
                            }
                        });
                    }
                });

                if let Some(i) = clicked_sort {
                    self.toggle_sort(i);
                }

                ui.add_space(tokens.spacing.sm);

                // Data rows
                egui::ScrollArea::vertical().show_rows(
                    ui,
                    self.row_height,
                    self.data.len(),
                    |ui, row_range| {
                        for i in row_range {
                            self.show_row(ui, i, theme_manager);
                        }
                    },
                );
            });
    }

    fn show_row(&mut self, ui: &mut egui::Ui, row_index: usize, theme_manager: &ThemeManager) {
        let colors = theme_manager.colors();
        let tokens = theme_manager.tokens();

        // Check if row is selected
        let is_selected = self.selected_row == Some(row_index);
        let is_hovered = self.hoverable && ui.rect_contains_pointer(ui.max_rect());

        // Row background
        if is_selected {
            ui.painter().rect_filled(
                ui.max_rect(),
                egui::Rounding::same(4.0),
                colors.primary.as_egui(),
            );
        } else if (self.striped && row_index % 2 == 1) || is_hovered {
            ui.painter().rect_filled(
                ui.max_rect(),
                egui::Rounding::same(4.0),
                colors.surface.as_egui(),
            );
        }

        // Row content
        ui.horizontal(|ui| {
            for col in &self.columns {
                let cell_response = ui
                    .horizontal(|ui| {
                        // Get cell value (simplified for example)
                        let value = format!("Row {} Col {}", row_index, col.id);
                        ui.label(value);
                    })
                    .response;

                // Handle row selection
                if cell_response.clicked() {
                    self.selected_row = Some(row_index);
                }
            }
        });

        ui.add_space(tokens.spacing.sm);
    }

    fn toggle_sort(&mut self, column_index: usize) {
        if self.sort_column == Some(column_index) {
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort_column = Some(column_index);
            self.sort_direction = SortDirection::Ascending;
        }

        // Sort data (simplified - in real implementation, this would use a proper comparator)
        // NOTE: generic `T` has no `Ord` bound, so stable sorting keys off the
        // Debug representation. Production tables sort by typed column accessors.
        self.data.sort_by(|a, b| {
            let ka = format!("{:?}", a);
            let kb = format!("{:?}", b);
            if self.sort_direction == SortDirection::Ascending {
                ka.cmp(&kb)
            } else {
                kb.cmp(&ka)
            }
        });
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.selected_row
    }

    pub fn set_selected_row(&mut self, index: Option<usize>) {
        self.selected_row = index;
    }

    pub fn rows(&self) -> &[T] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut Vec<T> {
        &mut self.data
    }
}

impl<T: Clone + std::fmt::Debug> Default for Table<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + std::fmt::Debug> Default for TableBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Table builder for convenience
pub struct TableBuilder<T> {
    table: Table<T>,
}

impl<T: Clone + std::fmt::Debug> TableBuilder<T> {
    pub fn new() -> Self {
        Self {
            table: Table::new(),
        }
    }

    pub fn column(mut self, id: &str, label: &str) -> Self {
        self.table.columns.push(Column {
            id: id.to_string(),
            label: label.into(),
            width: None,
            align: Align::Left,
            sortable: false,
        });
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        if let Some(col) = self.table.columns.last_mut() {
            col.width = Some(width);
        }
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        if let Some(col) = self.table.columns.last_mut() {
            col.align = align;
        }
        self
    }

    pub fn sortable(mut self, sortable: bool) -> Self {
        if let Some(col) = self.table.columns.last_mut() {
            col.sortable = sortable;
        }
        self
    }

    pub fn data(mut self, data: Vec<T>) -> Self {
        self.table.data = data;
        self
    }

    pub fn row_height(mut self, height: f32) -> Self {
        self.table.row_height = height;
        self
    }

    pub fn striped(mut self, striped: bool) -> Self {
        self.table.striped = striped;
        self
    }

    pub fn hoverable(mut self, hoverable: bool) -> Self {
        self.table.hoverable = hoverable;
        self
    }

    pub fn build(self) -> Table<T> {
        self.table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_creation() {
        let table = Table::<String>::new();
        assert!(table.columns.is_empty());
        assert!(table.data.is_empty());
        assert_eq!(table.selected_row, None);
        assert_eq!(table.sort_column, None);
        assert_eq!(table.sort_direction, SortDirection::Ascending);
    }

    #[test]
    fn test_table_builder() {
        let table = TableBuilder::new()
            .column("name", "Name")
            .column("age", "Age")
            .width(100.0)
            .align(Align::Right)
            .sortable(true)
            .data(vec!["Alice".to_string(), "Bob".to_string()])
            .build();

        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.columns[0].id, "name");
        assert_eq!(table.columns[0].label, "Name");
        assert_eq!(table.columns[0].width, None);
        assert_eq!(table.columns[0].align, Align::Left);
        assert!(!table.columns[0].sortable);

        assert_eq!(table.columns[1].id, "age");
        assert_eq!(table.columns[1].label, "Age");
        assert_eq!(table.columns[1].width, Some(100.0));
        assert_eq!(table.columns[1].align, Align::Right);
        assert!(table.columns[1].sortable);

        assert_eq!(table.data.len(), 2);
    }

    #[test]
    fn test_table_column_methods() {
        let table = Table::<String>::new()
            .column("name", "Name")
            .width(150.0)
            .align(Align::Center)
            .sortable(true);

        assert_eq!(table.columns.len(), 1);
        assert_eq!(table.columns[0].id, "name");
        assert_eq!(table.columns[0].label, "Name");
        assert_eq!(table.columns[0].width, Some(150.0));
        assert_eq!(table.columns[0].align, Align::Center);
        assert!(table.columns[0].sortable);
    }

    #[test]
    fn test_table_selection() {
        let mut table = Table::<String>::new();

        assert_eq!(table.selected_row(), None);

        table.set_selected_row(Some(1));
        assert_eq!(table.selected_row(), Some(1));

        table.set_selected_row(None);
        assert_eq!(table.selected_row(), None);
    }
}
