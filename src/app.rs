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
