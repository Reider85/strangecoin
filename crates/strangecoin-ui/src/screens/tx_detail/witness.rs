//! Witness panel (UI-P04) — `StateWitness` (S3-P11) proof display and an
//! explicit Verify action. Witness reconstruction is O(height) replay, so it
//! is never performed on screen open: the proof appears only after Verify.

use crate::accessibility::WidgetProbe;
use crate::backend::{TxVerifyResult, WitnessProof};
use crate::components::toast::show_toast;
use crate::components::{Button, CodeBlock, ToastLevel};
use crate::focus::{WidgetMeta, FOCUS_OUTLINE_WIDTH, MIN_TOUCH_TARGET};
use crate::i18n::Localization;
use crate::theme::ThemeManager;
use eframe::egui;
use std::sync::{Arc, Mutex};

pub struct WitnessPanel {
    proof: Option<WitnessProof>,
    result: Option<TxVerifyResult>,
    verify_clicked: Arc<Mutex<bool>>,
}

impl WitnessPanel {
    pub fn new() -> Self {
        Self {
            proof: None,
            result: None,
            verify_clicked: Arc::new(Mutex::new(false)),
        }
    }

    pub fn reset(&mut self) {
        self.proof = None;
        self.result = None;
    }

    pub fn set_result(&mut self, result: TxVerifyResult) {
        if let Some(proof) = result.witness.clone() {
            self.proof = Some(proof);
        }
        self.result = Some(result);
    }

    pub fn proof(&self) -> Option<&WitnessProof> {
        self.proof.as_ref()
    }

    pub fn result(&self) -> Option<&TxVerifyResult> {
        self.result.as_ref()
    }

    /// The screen calls `TxDetailBackend::verify` when this flips to `true`.
    pub fn take_verify_request(&mut self) -> bool {
        match self.verify_clicked.lock() {
            Ok(mut flag) => std::mem::take(&mut *flag),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: &ThemeManager, loc: &Localization) {
        let flag = Arc::clone(&self.verify_clicked);
        Button::new(loc.tx_detail_verify())
            .onclick(move || {
                if let Ok(mut clicked) = flag.lock() {
                    *clicked = true;
                }
            })
            .show(ui, theme);
        ui.add_space(theme.tokens().spacing.sm);

        if let Some(result) = &self.result {
            let colors = theme.colors();
            let (text, color, level) = if result.verified {
                (
                    loc.tx_detail_verified(),
                    colors.success.as_egui(),
                    ToastLevel::Success,
                )
            } else {
                (
                    loc.tx_detail_verify_failed(),
                    colors.error.as_egui(),
                    ToastLevel::Error,
                )
            };
            ui.label(egui::RichText::new(format!("{text}: {}", result.message)).color(color));
            show_toast("tx-detail-verify", &result.message, level);
        } else {
            ui.label(loc.tx_detail_witness_unavailable());
            return;
        }

        let Some(proof) = &self.proof else {
            return;
        };
        for (label, values) in [
            (loc.tx_detail_witness_account(), &proof.account_proofs),
            (loc.tx_detail_witness_storage(), &proof.storage_proofs),
        ] {
            if !values.is_empty() {
                ui.label(egui::RichText::new(label).strong());
                for value in values {
                    ui.monospace(value);
                }
            }
        }
        ui.label(egui::RichText::new(loc.tx_detail_witness_raw()).strong());
        CodeBlock::new("tx-detail-witness-raw", &proof.raw)
            .language("hex")
            .show(ui, theme);
    }

    pub fn a11y_probes(&self, theme: &ThemeManager, loc: &Localization) -> Vec<WidgetProbe> {
        let colors = theme.colors();
        vec![
            Button::new(loc.tx_detail_verify()).a11y_probe(theme),
            WidgetProbe::interactive(
                WidgetMeta::new("tx-detail-verify", "button", loc.tx_detail_verify()),
                colors.text,
                colors.surface,
                MIN_TOUCH_TARGET,
                MIN_TOUCH_TARGET,
            )
            .with_focus(true, FOCUS_OUTLINE_WIDTH),
            WidgetProbe::non_interactive(
                WidgetMeta::new(
                    "tx-detail-witness-account",
                    "status",
                    loc.tx_detail_witness_account(),
                ),
                colors.text,
                colors.surface,
            ),
            WidgetProbe::non_interactive(
                WidgetMeta::new(
                    "tx-detail-witness-storage",
                    "status",
                    loc.tx_detail_witness_storage(),
                ),
                colors.text,
                colors.surface,
            ),
            CodeBlock::new("tx-detail-witness-raw", "0x").a11y_probe(theme),
            WidgetProbe::non_interactive(
                WidgetMeta::new(
                    "tx-detail-witness-raw",
                    "status",
                    loc.tx_detail_witness_raw(),
                ),
                colors.text,
                colors.surface,
            ),
        ]
    }
}

impl Default for WitnessPanel {
    fn default() -> Self {
        Self::new()
    }
}
