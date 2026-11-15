//! The app window: seed picker, watching, alerts and the guide.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};

use crate::capture;
use crate::notify;
use crate::ocr;
use crate::seedlist::{self, SeedList, Source};
use crate::settings::{Settings, DEFAULT_WINDOW};
use crate::shop::Status;
use crate::watcher::{self, Check, Live};

pub const APP_NAME: &str = "PT Seed Watcher";
const VERSION: &str = env!("CARGO_PKG_VERSION");

const GREEN: Color32 = Color32::from_rgb(40, 170, 80);
const ORANGE: Color32 = Color32::from_rgb(230, 140, 40);
const RED: Color32 = Color32::from_rgb(210, 60, 60);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Watch,
    Alerts,
    Guide,
}

struct Preview {
    texture: egui::TextureHandle,
    size: [usize; 2],
    time: String,
    shop_open: bool,
    next_deal: Option<String>,
    seeds: Vec<(String, Status)>,
}

pub struct App {
    settings: Settings,
    saved: Settings,
    list: SeedList,
    list_source: Source,
    list_note: Option<String>,
    list_rx: Option<Receiver<Result<SeedList, String>>>,
    tab: Tab,
    search: String,
    new_seed: String,
    windows: Vec<(String, String)>,
    live: Arc<Mutex<Live>>,
    stop: Option<Arc<AtomicBool>>,
    running_settings: Option<Settings>,
    check_rx: Option<Receiver<Result<Check, String>>>,
    preview: Option<Preview>,
    preview_error: Option<String>,
    alert_rx: Option<Receiver<Vec<String>>>,
    alert_note: Option<String>,
}

impl App {
    pub fn new(ctx: &egui::Context) -> App {
        let (list, list_source) = match seedlist::saved() {
            Some(l) => (l, Source::Saved),
            None => (seedlist::built_in(), Source::BuiltIn),
        };
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(seedlist::download());
            repaint.request_repaint();
        });
        let settings = Settings::load();
        let tab = if settings.picked.is_empty() { Tab::Guide } else { Tab::Watch };
        App {
            saved: settings.clone(),
            settings,
            list,
            list_source,
            list_note: None,
            list_rx: Some(rx),
            tab,
            search: String::new(),
            new_seed: String::new(),
            windows: window_titles(),
            live: Arc::new(Mutex::new(Live::default())),
            stop: None,
            running_settings: None,
            check_rx: None,
            preview: None,
            preview_error: None,
            alert_rx: None,
            alert_note: None,
        }
    }

    /// Every seed name: the shared list plus the player's own.
    fn known(&self) -> Vec<String> {
        let mut all = self.list.seeds.clone();
        for s in &self.settings.custom_seeds {
            if !all.iter().any(|a| a.eq_ignore_ascii_case(s)) {
                all.push(s.clone());
            }
        }
        all
    }

    fn running(&self) -> bool {
        self.live.lock().unwrap().running
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new(format!("🌱 {APP_NAME}")).strong());
            ui.add_space(16.0);
            ui.selectable_value(&mut self.tab, Tab::Watch, RichText::new("Watch").size(16.0));
            ui.selectable_value(&mut self.tab, Tab::Alerts, RichText::new("Alerts").size(16.0));
            ui.selectable_value(&mut self.tab, Tab::Guide, RichText::new("How to use").size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, tip) = match self.list_source {
                    Source::Online => ("Seed list: up to date", "Downloaded just now."),
                    Source::Saved => ("Seed list: saved copy", "Couldn't download the latest list; using the last one."),
                    Source::BuiltIn => ("Seed list: built-in", "Couldn't download the latest list; using the one built into the app."),
                };
                let tip = match &self.list_note {
                    Some(n) if self.list_source != Source::Online => format!("{tip}\n{n}"),
                    _ if self.list_rx.is_some() => "Checking for an updated list...".to_string(),
                    _ => tip.to_string(),
                };
                let updated = if self.list.updated.is_empty() { String::new() } else { format!(" ({})", self.list.updated) };
                ui.label(RichText::new(format!("{text}{updated}")).weak()).on_hover_text(tip);
            });
        });
        if seedlist::is_newer(&self.list.latest_app_version, VERSION) {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("A new version ({}) is available.", self.list.latest_app_version))
                        .color(ORANGE)
                        .strong(),
                );
                if !self.list.download_url.is_empty() {
                    ui.hyperlink_to("Download it", &self.list.download_url);
                }
            });
        }
        if !self.list.message.trim().is_empty() {
            ui.label(RichText::new(self.list.message.trim()).color(ORANGE));
        }
    }

    fn seed_picker(&mut self, ui: &mut egui::Ui) {
        ui.heading("1. Pick your seeds");
        ui.label("You'll get an alert when any of these is in stock.");
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search seeds..."));
        ui.add_space(4.0);
        let search = self.search.to_lowercase();
        let known = self.known();
        egui::ScrollArea::vertical()
            .id_salt("seed-list")
            .max_height(ui.available_height() - 90.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for name in known.iter().filter(|n| n.to_lowercase().contains(&search)) {
                    let mut on = self.settings.picked.contains(name);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut on, name.as_str()).changed() {
                            if on {
                                self.settings.picked.push(name.clone());
                            } else {
                                self.settings.picked.retain(|p| p != name);
                            }
                        }
                        if self.settings.custom_seeds.contains(name)
                            && !self.list.seeds.contains(name)
                            && ui.small_button("remove").on_hover_text("Remove this seed you added").clicked()
                        {
                            self.settings.custom_seeds.retain(|c| c != name);
                            self.settings.picked.retain(|p| p != name);
                        }
                    });
                }
            });
        ui.separator();
        ui.label("Seed missing? Type its name exactly as the shop shows it:");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_seed).hint_text("e.g. Golden Corn Seed").desired_width(170.0));
            let name = self.new_seed.trim().to_string();
            let exists = self.known().iter().any(|k| k.eq_ignore_ascii_case(&name));
            if ui.add_enabled(!name.is_empty() && !exists, egui::Button::new("Add")).clicked() {
                self.settings.custom_seeds.push(name.clone());
                self.settings.picked.push(name);
                self.new_seed.clear();
            }
        });
    }

}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        if self.check_rx.is_some() || self.list_rx.is_some() || self.alert_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(6.0);
            self.header(ui);
            ui.add_space(4.0);
        });
        match self.tab {
            Tab::Watch => self.watch_tab(ui, &ctx),
            Tab::Alerts => {
                egui::CentralPanel::default_margins().show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.alerts_tab(ui));
                });
            }
            Tab::Guide => {
                egui::CentralPanel::default_margins().show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, guide);
                });
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop();
    }
}
