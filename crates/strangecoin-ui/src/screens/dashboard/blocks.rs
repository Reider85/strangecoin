//! Blocks panel (UI-P04) — recent 10 blocks with a click-through block detail
//! modal (header + tx list). Rows are ordered by height descending.

use crate::accessibility::WidgetProbe;
use crate::backend::BlockRow;
use crate::components::modal::{Modal, ModalContent, ModalSize};
use crate::components::{Card, Stat, Table};
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub const RECENT_BLOCKS: usize = 10;

pub struct BlocksPanel {
    rows: Vec<BlockRow>,
    selected_block: Option<BlockRow>,
    modal: Modal,
}

struct BlockDetailContent {
    block: BlockRow,
    labels: [String; 7],
}

impl ModalContent for BlockDetailContent {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager) {
        let tokens = theme_manager.tokens();
        for (label, value) in [
            (&self.labels[0], self.block.height.to_string()),
            (&self.labels[1], self.block.hash.clone()),
            (&self.labels[2], self.block.tx_count.to_string()),
            (&self.labels[3], self.block.gas_used.to_string()),
            (&self.labels[4], self.block.miner.clone()),
            (&self.labels[5], self.block.timestamp.to_string()),
        ] {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).strong());
                ui.label(value);
            });
        }
        ui.add_space(tokens.spacing.sm);
        ui.label(egui::RichText::new(&self.labels[6]).strong());
        for txid in &self.block.txids {
            ui.monospace(txid);
        }
    }
}

impl BlocksPanel {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            selected_block: None,
            modal: Modal::new("block-detail")
                .size(ModalSize::Md)
                .closable(true),
        }
    }

    pub fn set_rows(&mut self, mut rows: Vec<BlockRow>) {
        rows.sort_by_key(|row| std::cmp::Reverse(row.height));
        rows.truncate(RECENT_BLOCKS);
        self.rows = rows;
        self.selected_block = None;
    }

    pub fn rows(&self) -> &[BlockRow] {
        &self.rows
    }

    pub fn selected_block(&self) -> Option<&BlockRow> {
        self.selected_block.as_ref()
    }

    pub fn clear_selection(&mut self) {
        self.selected_block = None;
        self.modal.close();
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        if self.rows.is_empty() {
            ui.label(loc.dashboard_no_blocks());
            return;
        }

        let rows = self.rows.clone();
        let mut table = Table::new()
            .column("height", &loc.dashboard_block_height())
            .width(70.0)
            .column("hash", &loc.dashboard_block_hash())
            .width(160.0)
            .column("txs", &loc.dashboard_block_txs())
            .width(60.0)
            .column("gas", &loc.dashboard_block_gas())
            .width(80.0)
            .column("miner", &loc.dashboard_block_miner())
            .width(110.0)
            .column("timestamp", &loc.dashboard_block_timestamp())
            .cell_renderer(
                move |row: &BlockRow, column: &str, ui: &mut egui::Ui| match column {
                    "height" => {
                        ui.label(row.height.to_string());
                    }
                    "hash" => {
                        ui.monospace(&row.hash);
                    }
                    "txs" => {
                        ui.label(row.tx_count.to_string());
                    }
                    "gas" => {
                        ui.label(row.gas_used.to_string());
                    }
                    "miner" => {
                        ui.label(&row.miner);
                    }
                    "timestamp" => {
                        ui.label(row.timestamp.to_string());
                    }
                    _ => {}
                },
            )
            .data(rows.clone());
        table.show(ui, theme);

        if let Some(index) = table.selected_row() {
            if let Some(block) = rows.get(index) {
                self.selected_block = Some(block.clone());
            }
        }

        if let Some(block) = self.selected_block.clone() {
            let content = BlockDetailContent {
                block,
                labels: [
                    loc.dashboard_block_height(),
                    loc.dashboard_block_hash(),
                    loc.dashboard_block_txs(),
                    loc.dashboard_block_gas(),
                    loc.dashboard_block_miner(),
                    loc.dashboard_block_timestamp(),
                    loc.dashboard_block_tx_list(),
                ],
            };
            self.modal.set_title(loc.dashboard_block_detail());
            self.modal.set_content(content);
            self.modal.open();
        }
        self.modal.show(ui.ctx(), theme);
        if !self.modal.is_open() {
            self.selected_block = None;
        }
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        let mut probes = vec![Card::new().title(loc.dashboard_blocks()).a11y_probe(theme)];
        for (id, label) in [
            ("dashboard-block-height", loc.dashboard_block_height()),
            ("dashboard-block-hash", loc.dashboard_block_hash()),
            ("dashboard-block-txs", loc.dashboard_block_txs()),
            ("dashboard-block-gas", loc.dashboard_block_gas()),
            ("dashboard-block-miner", loc.dashboard_block_miner()),
            ("dashboard-block-timestamp", loc.dashboard_block_timestamp()),
        ] {
            probes.push(
                WidgetProbe::interactive(
                    WidgetMeta::new(id, "columnheader", label),
                    colors.text,
                    colors.surface,
                    MIN_TOUCH_TARGET,
                    MIN_TOUCH_TARGET,
                )
                .with_focus(true, FOCUS_OUTLINE_WIDTH),
            );
        }
        probes.push(
            Modal::new("block-detail")
                .title(loc.dashboard_block_detail())
                .a11y_probe(theme),
        );
        probes.push(WidgetProbe::non_interactive(
            WidgetMeta::new(
                "dashboard-block-detail",
                "status",
                loc.dashboard_block_detail(),
            ),
            colors.text,
            colors.surface,
        ));
        probes.push(
            Stat::new("dashboard-block-count", &loc.dashboard_blocks(), "10").a11y_probe(theme),
        );
        probes
    }
}

impl Default for BlocksPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_sorted_by_height_descending_and_capped() {
        let mut panel = BlocksPanel::new();
        let rows: Vec<_> = (0..12)
            .map(|height| BlockRow {
                height,
                hash: format!("block-{height}"),
                tx_count: 1,
                gas_used: 21_000,
                miner: "sc1miner".to_string(),
                timestamp: 1_700_000_000,
                txids: Vec::new(),
            })
            .collect();
        panel.set_rows(rows);
        assert_eq!(panel.rows().len(), RECENT_BLOCKS);
        assert_eq!(panel.rows()[0].height, 11);
        assert!(panel.rows().windows(2).all(|w| w[0].height > w[1].height));
    }
}
