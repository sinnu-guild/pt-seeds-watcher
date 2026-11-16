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

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.list_rx {
            if let Ok(result) = rx.try_recv() {
                self.list_rx = None;
                match result {
                    Ok(list) => {
                        self.list = list;
                        self.list_source = Source::Online;
                        self.list_note = None;
                    }
                    Err(e) => self.list_note = Some(e),
                }
            }
        }
        if let Some(rx) = &self.check_rx {
            if let Ok(result) = rx.try_recv() {
                self.check_rx = None;
                match result {
                    Ok(check) => {
                        self.preview = Some(make_preview(ctx, &check));
                        self.preview_error = None;
                    }
                    Err(e) => {
                        self.preview = None;
                        self.preview_error = Some(e);
                    }
                }
            }
        }
        if let Some(rx) = &self.alert_rx {
            if let Ok(failed) = rx.try_recv() {
                self.alert_rx = None;
                self.alert_note = Some(if failed.is_empty() {
                    "Test alert sent. Check that it arrived everywhere you turned on.".into()
                } else {
                    failed.join("\n")
                });
            }
        }
        if self.settings != self.saved {
            self.settings.save();
            self.saved = self.settings.clone();
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        self.stop = Some(watcher::start(self.settings.clone(), self.known(), self.live.clone(), move || {
            ctx.request_repaint()
        }));
        self.running_settings = Some(self.settings.clone());
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    fn check_now(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (window, known, wanted) = (self.settings.window().to_string(), self.known(), self.settings.picked.clone());
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            ocr::init_thread();
            let _ = tx.send(watcher::check_once(&window, &known, &wanted));
            ctx.request_repaint();
        });
        self.check_rx = Some(rx);
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

    fn watch_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::left("picker").resizable(false).exact_size(290.0).show(ui, |ui| {
            ui.add_space(6.0);
            self.seed_picker(ui);
        });
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("watch").auto_shrink([false, false]).show(ui, |ui| {
                self.watch_controls(ui, ctx);
            });
        });
    }

    fn watch_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let running = self.running();

        ui.heading("2. Set up the game");
        ui.label("Open the seed shop and scroll the list so your seeds show. Leave the game open (it can be behind other windows, but not minimized).");
        ui.horizontal(|ui| {
            ui.label("Game window:");
            let shown = if self.settings.window_title.is_empty() {
                format!("Find \"{DEFAULT_WINDOW}\" automatically")
            } else {
                self.settings.window_title.clone()
            };
            egui::ComboBox::from_id_salt("window").selected_text(shown).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.settings.window_title, String::new(), format!("Find \"{DEFAULT_WINDOW}\" automatically"));
                for (title, label) in &self.windows {
                    ui.selectable_value(&mut self.settings.window_title, title.clone(), label.as_str());
                }
            });
            if ui.button("Refresh").on_hover_text("Reload the list of open windows").clicked() {
                self.windows = window_titles();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Check every");
            ui.add(egui::Slider::new(&mut self.settings.check_every_secs, 10..=120).suffix(" s"));
        });
        ui.horizontal(|ui| {
            let busy = self.check_rx.is_some();
            if ui
                .add_enabled(!busy, egui::Button::new(RichText::new("Check now").size(15.0)))
                .on_hover_text("Look at the game once and show what the app can see")
                .clicked()
            {
                self.check_now(ctx);
            }
            if busy {
                ui.spinner();
                ui.label("Looking...");
            }
        });
        self.preview_ui(ui);

        ui.add_space(10.0);
        ui.separator();
        ui.heading("3. Watch");
        ui.horizontal(|ui| {
            if running {
                if ui.add(egui::Button::new(RichText::new("Stop watching").size(17.0)).min_size([180.0, 34.0].into())).clicked() {
                    self.stop();
                }
            } else {
                let can = !self.settings.picked.is_empty();
                let button = egui::Button::new(RichText::new("Start watching").size(17.0).color(Color32::WHITE))
                    .fill(GREEN)
                    .min_size([180.0, 34.0].into());
                if ui.add_enabled(can, button).on_disabled_hover_text("Pick at least one seed first").clicked() {
                    self.start(ctx);
                }
            }
            if self.stop.is_some() && !running {
                self.stop = None;
            }
        });
        if running && self.running_settings.as_ref() != Some(&self.settings) {
            ui.label(RichText::new("You changed something. Stop and start again to use the changes.").color(ORANGE));
        }

        let live = self.live.lock().unwrap();
        if running || live.last_check.is_some() {
            ui.add_space(6.0);
            if let Some(p) = &live.problem {
                ui.label(RichText::new(format!("⚠ {p}")).color(RED));
            }
            egui::Grid::new("status").striped(true).num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                for (name, status) in &live.seeds {
                    ui.label(RichText::new(name).size(15.0));
                    ui.label(status_text(*status).size(15.0));
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if let Some(t) = &live.last_check {
                    ui.label(RichText::new(format!("Last checked {t}")).weak());
                }
                if let Some(d) = &live.next_deal {
                    ui.label(RichText::new(format!("· Shop restocks in about {d}")).weak());
                }
            });
        }
        if !live.log.is_empty() {
            egui::CollapsingHeader::new("Activity").default_open(false).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("log").max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                    for line in &live.log {
                        ui.label(RichText::new(line).monospace().size(12.0));
                    }
                });
            });
        }
    }

    fn preview_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(e) = &self.preview_error {
            ui.label(RichText::new(format!("⚠ {e}")).color(RED));
        }
        let Some(p) = &self.preview else { return };
        ui.add_space(4.0);
        ui.label(RichText::new(format!("What the app saw at {}:", p.time)).strong());
        if !p.shop_open {
            ui.label(RichText::new("⚠ This doesn't look like the seed shop. Open it in the game.").color(RED));
        }
        if p.seeds.is_empty() {
            ui.label("Pick some seeds to see their status here.");
        }
        for (name, status) in &p.seeds {
            ui.horizontal(|ui| {
                ui.label(name);
                ui.label(status_text(*status));
            });
        }
        if let Some(d) = &p.next_deal {
            ui.label(RichText::new(format!("Shop restocks in about {d}")).weak());
        }
        let w = ui.available_width().min(760.0);
        let h = w * p.size[1] as f32 / p.size[0] as f32;
        ui.add(egui::Image::new(egui::load::SizedTexture::new(p.texture.id(), [w, h])));
        ui.label(
            RichText::new("Boxes: green = in stock, orange = out of stock, red = stock line not readable, gray = other seeds. The right half (shaded) is ignored.")
                .weak()
                .size(12.0),
        );
    }

    fn alerts_tab(&mut self, ui: &mut egui::Ui) {
        let a = &mut self.settings.alerts;
        ui.heading("On this computer");
        ui.checkbox(&mut a.desktop, "Show a Windows notification");
        ui.checkbox(&mut a.sound, "Play a sound");
        ui.checkbox(&mut a.notify_when_window_lost, "Alert me if the app can't see my seeds for about a minute (shop closed, game minimized...)");

        ui.add_space(12.0);
        ui.heading("On your phone (free, optional)");
        ui.label("1. Install the free \"ntfy\" app (Android or iPhone).");
        ui.label("2. Click \"Make a topic\" below (or type your own hard-to-guess name).");
        ui.label("3. In the ntfy app, tap + and subscribe to exactly the same topic name.");
        ui.label(RichText::new("Anyone who knows the topic name can read your alerts, so keep it hard to guess.").weak());
        ui.horizontal(|ui| {
            ui.label("Topic:");
            ui.add(egui::TextEdit::singleline(&mut a.ntfy_topic).desired_width(240.0).hint_text("leave empty to turn off"));
            if ui.button("Make a topic").clicked() {
                a.ntfy_topic = random_topic();
            }
            if !a.ntfy_topic.is_empty() && ui.button("Copy").clicked() {
                ui.ctx().copy_text(a.ntfy_topic.clone());
            }
        });

        ui.add_space(12.0);
        ui.heading("In Discord (optional)");
        ui.label("Paste a channel webhook URL (Channel settings > Integrations > Webhooks).");
        ui.add(egui::TextEdit::singleline(&mut a.discord_webhook).desired_width(480.0).hint_text("https://discord.com/api/webhooks/..."));

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let busy = self.alert_rx.is_some();
            if ui.add_enabled(!busy, egui::Button::new("Send a test alert")).clicked() {
                let alerts = self.settings.alerts.clone();
                let (tx, rx) = mpsc::channel();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let _ = tx.send(notify::send(&alerts, watcher::ALERT_TITLE, "Test alert. If you can read this, alerts work!"));
                    ctx.request_repaint();
                });
                self.alert_rx = Some(rx);
                self.alert_note = None;
            }
            if busy {
                ui.spinner();
            }
        });
        if let Some(n) = &self.alert_note {
            ui.label(n);
        }
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

fn status_text(s: Status) -> RichText {
    let t = RichText::new(s.describe());
    match s {
        Status::InStock(_) => t.color(GREEN).strong(),
        Status::OutOfStock => t.color(ORANGE),
        Status::Unknown => t.color(RED),
        Status::NotVisible => t.weak(),
    }
}

/// Open windows worth offering in the game-window picker: (title, label).
fn window_titles() -> Vec<(String, String)> {
    let mut titles: Vec<(String, String)> = capture::list_windows()
        .into_iter()
        .filter(|w| (w.minimized || (w.width >= 300 && w.height >= 200)) && w.title != APP_NAME)
        .map(|w| {
            let label = if w.minimized {
                format!("{} (minimized)", w.title)
            } else if w.cloaked {
                format!("{} (on another desktop)", w.title)
            } else {
                w.title.clone()
            };
            (w.title, label)
        })
        .collect();
    titles.sort_by_key(|t| t.0.to_lowercase());
    titles.dedup_by(|a, b| a.0 == b.0);
    titles
}

fn random_topic() -> String {
    use std::hash::BuildHasher;
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut n = std::collections::hash_map::RandomState::new().hash_one(seed);
    let mut s = String::from("pt-seeds-");
    for _ in 0..12 {
        s.push(b"abcdefghijkmnpqrstuvwxyz23456789"[(n % 32) as usize] as char);
        n /= 32;
    }
    s
}

fn make_preview(ctx: &egui::Context, c: &Check) -> Preview {
    let (w, h) = (c.frame.width as usize, c.frame.height as usize);
    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &annotate(c));
    Preview {
        texture: ctx.load_texture("preview", image, egui::TextureOptions::LINEAR),
        size: [w, h],
        time: c.time.clone(),
        shop_open: c.view.shop_open,
        next_deal: c.view.next_deal.clone(),
        seeds: c.view.seeds.iter().map(|s| (s.name.clone(), s.status)).collect(),
    }
}

/// The captured picture (RGBA) with boxes on what was read and the ignored
/// right half shaded.
pub fn annotate(c: &Check) -> Vec<u8> {
    let (w, h) = (c.frame.width as usize, c.frame.height as usize);
    let mut rgba = c.frame.rgba.clone();
    // Shade the ignored right half.
    for y in 0..h {
        for x in w / 2..w {
            let i = (y * w + x) * 4;
            for k in 0..3 {
                rgba[i + k] /= 3;
            }
        }
    }
    let mut boxed = |i: usize, color: [u8; 3]| {
        let l = &c.lines[i];
        let pad = 4.0;
        draw_box(&mut rgba, w, h, l.x - pad, l.y - pad, l.w + 2.0 * pad, l.h + 2.0 * pad, color);
    };
    for (i, _) in &c.view.names {
        boxed(*i, [150, 150, 150]);
    }
    for s in &c.view.seeds {
        let color = match s.status {
            Status::InStock(_) => [40, 200, 80],
            Status::OutOfStock => [240, 140, 30],
            _ => [220, 50, 50],
        };
        if let Some(i) = s.name_line {
            boxed(i, color);
        }
        if let Some(i) = s.stock_line {
            boxed(i, color);
        }
    }
    rgba
}

#[allow(clippy::too_many_arguments)]
fn draw_box(rgba: &mut [u8], w: usize, h: usize, x: f32, y: f32, bw: f32, bh: f32, color: [u8; 3]) {
    let (x0, y0) = (x.max(0.0) as usize, y.max(0.0) as usize);
    let (x1, y1) = (((x + bw) as usize).min(w.saturating_sub(1)), ((y + bh) as usize).min(h.saturating_sub(1)));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let mut put = |px: usize, py: usize| {
        let i = (py * w + px) * 4;
        rgba[i..i + 3].copy_from_slice(&color);
    };
    for t in 0..3 {
        for px in x0..=x1 {
            put(px, (y0 + t).min(y1));
            put(px, y1.saturating_sub(t).max(y0));
        }
        for py in y0..=y1 {
            put((x0 + t).min(x1), py);
            put(x1.saturating_sub(t).max(x0), py);
        }
    }
}

