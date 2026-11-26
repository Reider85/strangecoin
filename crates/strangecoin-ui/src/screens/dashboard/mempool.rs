//! Mempool panel (UI-P04) — tx count, total fee and a top-10-by-fee table.
//! RBF conflicts are rendered with both a badge and a text label (WCAG 1.4.1).
//! Rows are sorted by `(fee desc, txid asc)` for deterministic output.

use crate::accessibility::WidgetProbe;
use crate::backend::MempoolRow;
use crate::components::{Card, Stat, Table};
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;

pub const MAX_MEMPOOL_ROWS: usize = 10;

pub struct MempoolPanel {
    count: usize,
    total_fee: u64,
    rows: Vec<MempoolRow>,
}

impl MempoolPanel {
    pub fn new() -> Self {
        Self {
            count: 0,
            total_fee: 0,
            rows: Vec::new(),
        }
    }

    pub fn set_data(&mut self, count: usize, total_fee: u64, mut rows: Vec<MempoolRow>) {
        rows.sort_by(|left, right| {
            right
                .fee
                .cmp(&left.fee)
                .then_with(|| left.txid.cmp(&right.txid))
        });
        rows.truncate(MAX_MEMPOOL_ROWS);
        self.count = count;
        self.total_fee = total_fee;
        self.rows = rows;
    }

    pub fn rows(&self) -> &[MempoolRow] {
        &self.rows
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let tokens = theme.tokens();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = tokens.spacing.lg;
            Card::new()
                .title(loc.dashboard_mempool())
                .child(Stat::new("mempool-count", "", &self.count.to_string()))
                .show(ui, theme);
            Card::new()
                .title(loc.dashboard_total_fee())
                .child(Stat::new(
                    "mempool-total-fee",
                    "",
                    &format!("{} SC", self.total_fee),
                ))
                .show(ui, theme);
        });
        ui.add_space(tokens.spacing.base);

        if self.rows.is_empty() {
            ui.label(loc.dashboard_no_mempool());
            return;
        }

        let rbf_label = loc.dashboard_mempool_rbf();
        let rows = self.rows.clone();
        Table::new()
            .column("txid", &loc.dashboard_mempool_txid())
            .width(200.0)
            .column("from", &loc.dashboard_mempool_from())
            .width(110.0)
            .column("to", &loc.dashboard_mempool_to())
            .width(110.0)
            .column("amount", &loc.dashboard_mempool_amount())
            .width(80.0)
            .column("fee", &loc.dashboard_mempool_fee())
            .width(70.0)
            .column("gas", &loc.dashboard_mempool_gas())
            .width(70.0)
            .column("rbf", &loc.dashboard_mempool_rbf())
            .cell_renderer(
                move |row: &MempoolRow, column: &str, ui: &mut egui::Ui| match column {
                    "txid" => {
                        ui.label(truncate(&row.txid, 16));
                    }
                    "from" => {
                        ui.label(&row.sender);
                    }
                    "to" => {
                        ui.label(&row.receiver);
                    }
                    "amount" => {
                        ui.label(row.amount.to_string());
                    }
                    "fee" => {
                        ui.label(row.fee.to_string());
                    }
                    "gas" => {
                        ui.label(row.gas.to_string());
                    }
                    "rbf" => {
                        if row.rbf_conflict {
                            ui.label(egui::RichText::new(rbf_label.clone()).strong());
                        } else {
                            ui.label("-");
                        }
                    }
                    _ => {}
                },
            )
            .data(rows)
            .show(ui, theme);
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        let mut probes = vec![
            Stat::new("mempool-count", &loc.dashboard_mempool(), "0").a11y_probe(theme),
            Stat::new("mempool-total-fee", &loc.dashboard_total_fee(), "0 SC").a11y_probe(theme),
        ];
        for (id, label) in [
            ("dashboard-mempool-txid", loc.dashboard_mempool_txid()),
            ("dashboard-mempool-from", loc.dashboard_mempool_from()),
            ("dashboard-mempool-to", loc.dashboard_mempool_to()),
            ("dashboard-mempool-amount", loc.dashboard_mempool_amount()),
            ("dashboard-mempool-fee", loc.dashboard_mempool_fee()),
            ("dashboard-mempool-gas", loc.dashboard_mempool_gas()),
            ("dashboard-mempool-rbf", loc.dashboard_mempool_rbf()),
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
        probes
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let head: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        format!("{head}…")
    } else {
        head
    }
}

impl Default for MempoolPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(txid: &str, fee: u64, rbf: bool) -> MempoolRow {
        MempoolRow {
            txid: txid.to_string(),
            sender: "sc1sender".to_string(),
            receiver: "sc1receiver".to_string(),
            amount: 1,
            fee,
            gas: 21_000,
            rbf_conflict: rbf,
        }
    }

    #[test]
    fn rows_are_sorted_by_fee_then_txid_and_capped_at_ten() {
        let mut panel = MempoolPanel::new();
        let rows: Vec<_> = (0..12)
            .map(|index| row(&format!("0x{index:02}"), index % 3, index % 4 == 0))
            .collect();
        panel.set_data(12, 42, rows);
        assert_eq!(panel.rows().len(), MAX_MEMPOOL_ROWS);
        assert_eq!(panel.count, 12);
        assert_eq!(panel.total_fee, 42);
        assert!(panel.rows().windows(2).all(|pair| {
            pair[0].fee >= pair[1].fee
                && (pair[0].fee != pair[1].fee || pair[0].txid <= pair[1].txid)
        }));
    }
}
