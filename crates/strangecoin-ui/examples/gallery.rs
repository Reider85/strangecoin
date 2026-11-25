//! Design system gallery (UI-P02 КГ).
//!
//! Renders every component of `strangecoin-ui` in the current theme so the
//! design system can be reviewed visually:
//!
//! ```sh
//! cargo run -p strangecoin-ui --example gallery
//! ```
//!
//! Toolbar: light/dark toggle + locale switch (en/ru/zh) + a11y audit link.

use eframe::egui;
use std::collections::HashMap;
use strangecoin_ui::components::drawer::{DrawerPosition, DrawerSize, TextDrawerContent};
use strangecoin_ui::components::modal::{ModalSize, TextModalContent};
use strangecoin_ui::components::{
    Avatar, AvatarSize, Badge, BadgeVariant, Breadcrumb, Button, ButtonVariant, Card, CardText,
    Chart, ChartDataPoint, ChartType, CodeBlock, Drawer, Input, InputType, Modal, Stat, StatSize,
    StatTrend, Table, Tabs, TextTabContent, Toast, ToastLevel,
};
use strangecoin_ui::i18n::Localization;
use strangecoin_ui::theme::ThemeManager;

fn tr(localization: &Localization, key: &str) -> String {
    localization.tr(key, HashMap::new())
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0])
            .with_title("Strangecoin Design System Gallery"),
        ..Default::default()
    };
    eframe::run_native(
        "Strangecoin Design System Gallery",
        options,
        Box::new(|_cc| Ok(Box::new(GalleryApp::new()))),
    )
}

struct GalleryApp {
    theme: ThemeManager,
    localization: Localization,
    locale_idx: usize,
    input_value: String,
    toasts: Vec<(String, ToastLevel)>,
    modal_open: bool,
    drawer_open: bool,
}

impl GalleryApp {
    fn new() -> Self {
        Self {
            theme: ThemeManager::new(),
            localization: Localization::new(),
            locale_idx: 0,
            input_value: String::new(),
            toasts: vec![],
            modal_open: false,
            drawer_open: false,
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Gallery:");

            if ui
                .button(format!("🌙 {}", tr(&self.localization, "theme-dark")))
                .clicked()
            {
                self.theme.set_dark_mode(true);
            }
            if ui
                .button(format!("☀️ {}", tr(&self.localization, "theme-light")))
                .clicked()
            {
                self.theme.set_dark_mode(false);
            }

            ui.separator();
            if ui.button("Modal").clicked() {
                self.modal_open = true;
            }
            if ui.button("Drawer").clicked() {
                self.drawer_open = true;
            }

            ui.separator();
            for (i, code) in ["en", "ru", "zh"].iter().enumerate() {
                if ui
                    .selectable_label(self.locale_idx == i, code.to_uppercase())
                    .clicked()
                {
                    self.locale_idx = i;
                    let _ = self.localization.set_language(code);
                }
            }

            ui.separator();
            ui.label(format!(
                "Theme: {} | Locale: {}",
                self.theme.current(),
                self.localization.current_locale()
            ));
        });
    }

    fn section_title(ui: &mut egui::Ui, title: &str) {
        ui.add_space(12.0);
        ui.heading(title);
        ui.separator();
    }
}

impl eframe::App for GalleryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.theme.apply_to_egui(ctx);

        egui::CentralPanel::default().show(ctx, |panel| {
            egui::ScrollArea::vertical()
                .id_salt("gallery")
                .show(panel, |ui| {
                    self.toolbar(ui);

                    // ------------------------------------------------------------------
                    // Buttons
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Buttons");
                    ui.horizontal_wrapped(|ui| {
                        let mut primary = Button::new(self.localization.button_ok());
                        primary.show(ui, &self.theme);

                        let mut secondary =
                            Button::new("Secondary").variant(ButtonVariant::Secondary);
                        secondary.show(ui, &self.theme);

                        let mut danger = Button::new("Danger").variant(ButtonVariant::Danger);
                        danger.show(ui, &self.theme);

                        let mut ghost = Button::new("Ghost").variant(ButtonVariant::Ghost);
                        ghost.show(ui, &self.theme);

                        let mut loading = Button::new("Loading").loading(true);
                        loading.show(ui, &self.theme);

                        let mut disabled = Button::new("Disabled").disabled(true);
                        disabled.show(ui, &self.theme);
                    });

                    // ------------------------------------------------------------------
                    // Inputs
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Inputs");
                    ui.horizontal_wrapped(|ui| {
                        let mut text = Input::new("Text field")
                            .placeholder("Type here…")
                            .value(&self.input_value);
                        text.show(ui, &self.theme);

                        let mut required =
                            Input::new(tr(&self.localization, "wallet-send-recipient"))
                                .input_type(InputType::Bech32)
                                .required(true)
                                .placeholder("sc1…");
                        required.show(ui, &self.theme);

                        let mut number = Input::new(tr(&self.localization, "wallet-send-amount"))
                            .input_type(InputType::Number)
                            .placeholder("0");
                        number.show(ui, &self.theme);
                    });

                    // ------------------------------------------------------------------
                    // Stats
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Stats");
                    ui.horizontal_wrapped(|ui| {
                        let mut balance = Stat::new(
                            "balance",
                            &tr(&self.localization, "dashboard-total-balance"),
                            "12,345 SC",
                        )
                        .icon("💰")
                        .size(StatSize::Lg)
                        .trend(StatTrend::Up);
                        balance.show(ui, &self.theme);

                        let mut peers =
                            Stat::new("peers", &tr(&self.localization, "dashboard-peers"), "8")
                                .icon("🌐")
                                .change(12.5, true)
                                .trend(StatTrend::Up);
                        peers.show(ui, &self.theme);

                        let mut mempool = Stat::new(
                            "mempool",
                            &tr(&self.localization, "dashboard-mempool"),
                            "42",
                        )
                        .icon("📦")
                        .change(-3.0, true)
                        .trend(StatTrend::Down);
                        mempool.show(ui, &self.theme);
                    });

                    // ------------------------------------------------------------------
                    // Badges + Avatar
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Badges & Avatar");
                    ui.horizontal_wrapped(|ui| {
                        for (label, variant) in [
                            ("Default".to_string(), BadgeVariant::Default),
                            ("Primary".to_string(), BadgeVariant::Primary),
                            (tr(&self.localization, "success"), BadgeVariant::Success),
                            (tr(&self.localization, "warning"), BadgeVariant::Warning),
                            (tr(&self.localization, "error"), BadgeVariant::Error),
                            (tr(&self.localization, "info"), BadgeVariant::Info),
                        ] {
                            let mut badge = Badge::new(&label, &label).variant(variant);
                            badge.show(ui, &self.theme);
                        }

                        for size in [
                            AvatarSize::Xs,
                            AvatarSize::Sm,
                            AvatarSize::Md,
                            AvatarSize::Lg,
                        ] {
                            let mut avatar = Avatar::new("agent_1", "Alice").size(size);
                            avatar.show(ui, &self.theme);
                        }
                    });

                    // ------------------------------------------------------------------
                    // Cards
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Cards");
                    let mut card = Card::new()
                        .title("Card title")
                        .child(CardText::new("Card body text — nested CardText child."))
                        .child(CardText::new("Secondary line with default styling."));
                    card.show(ui, &self.theme);

                    // ------------------------------------------------------------------
                    // Table
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Table");
                    let mut table: Table<Vec<String>> = Table::new()
                        .column("name", "Name")
                        .column("status", "Status")
                        .column("score", "Score")
                        .width(180.0)
                        .data(vec![
                            vec!["Validator 1".into(), "Online".into(), "95.0".into()],
                            vec!["Validator 2".into(), "Offline".into(), "88.5".into()],
                            vec!["Validator 3".into(), "Error".into(), "71.2".into()],
                        ]);
                    table.show(ui, &self.theme);

                    // ------------------------------------------------------------------
                    // Tabs
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Tabs");
                    let mut tabs = Tabs::new("gallery_tabs")
                        .tab(
                            "one",
                            &tr(&self.localization, "tx-detail-overview"),
                            Box::new(TextTabContent::new("Overview content.")),
                        )
                        .tab(
                            "two",
                            "Details",
                            Box::new(TextTabContent::new("Details content.")),
                        )
                        .tab(
                            "three",
                            "Logs",
                            Box::new(TextTabContent::new("Logs content.")),
                        );
                    tabs.show(ui, &self.theme);

                    // ------------------------------------------------------------------
                    // Breadcrumb
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Breadcrumb");
                    let mut breadcrumb = Breadcrumb::new("gallery_crumb")
                        .item("home", &tr(&self.localization, "nav-home"))
                        .item("wallet", &tr(&self.localization, "wallet-title"))
                        .item("tx", &tr(&self.localization, "tx-detail-title"));
                    breadcrumb.show(ui, &self.theme);

                    // ------------------------------------------------------------------
                    // CodeBlock
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "CodeBlock");
                    let mut code = CodeBlock::new(
                        "example",
                        "fn main() {\n    println!(\"hello strangecoin\");\n}",
                    )
                    .language("rust");
                    code.show(ui, &self.theme);

                    // ------------------------------------------------------------------
                    // Chart
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Chart");
                    let mut chart = Chart::new("gallery_chart", ChartType::Line)
                        .title("Throughput")
                        .x_axis("block")
                        .y_axis("tx/s")
                        .size(ui.available_width().min(600.0), 220.0)
                        .data(vec![
                            ChartDataPoint {
                                x: 1.0,
                                y: 10.0,
                                label: None,
                                color: None,
                            },
                            ChartDataPoint {
                                x: 2.0,
                                y: 14.0,
                                label: None,
                                color: None,
                            },
                            ChartDataPoint {
                                x: 3.0,
                                y: 12.0,
                                label: None,
                                color: None,
                            },
                            ChartDataPoint {
                                x: 4.0,
                                y: 18.0,
                                label: None,
                                color: None,
                            },
                            ChartDataPoint {
                                x: 5.0,
                                y: 16.0,
                                label: None,
                                color: None,
                            },
                        ]);
                    chart.show(ui, &self.theme);

                    Self::section_title(ui, "Modal & Drawer");
                    if self.modal_open {
                        let mut modal = Modal::new("gallery_modal")
                            .title(tr(&self.localization, "confirm-title"))
                            .size(ModalSize::Sm)
                            .content(TextModalContent::new(&tr(
                                &self.localization,
                                "unsaved-changes",
                            )));
                        modal.open();
                        modal.show(ctx, &self.theme);
                        self.modal_open = modal.is_open();
                    }
                    if self.drawer_open {
                        let mut drawer = Drawer::new("gallery_drawer")
                            .title(tr(&self.localization, "settings-general"))
                            .position(DrawerPosition::Right)
                            .size(DrawerSize::Sm)
                            .content(TextDrawerContent::new(&tr(
                                &self.localization,
                                "settings-config",
                            )));
                        drawer.open();
                        drawer.show(ctx, &self.theme);
                        self.drawer_open = drawer.is_open();
                    }

                    // ------------------------------------------------------------------
                    // Toasts
                    // ------------------------------------------------------------------
                    Self::section_title(ui, "Toast");
                    ui.horizontal_wrapped(|ui| {
                        for (level, key) in [
                            (ToastLevel::Info, "info"),
                            (ToastLevel::Success, "success"),
                            (ToastLevel::Warning, "warning"),
                            (ToastLevel::Error, "error"),
                        ] {
                            let label = format!("{} toast", tr(&self.localization, key));
                            if ui.button(&label).clicked() {
                                self.toasts.push((label, level));
                            }
                        }
                    });
                    for (i, (msg, level)) in self.toasts.iter().enumerate() {
                        let mut toast = Toast::new(&format!("t{i}"), msg, *level);
                        toast.show_in_context(ctx, &self.theme);
                    }

                    ui.add_space(32.0);
                    ui.separator();
                    ui.weak("Strangecoin design system gallery — UI-P02");
                });
        });
    }
}
