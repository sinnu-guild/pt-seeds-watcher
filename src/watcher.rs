//! Checking the shop: once ("Check now"), or every few seconds on a
//! background thread while watching. Only reads the screen.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::capture::{self, Frame};
use crate::notify;
use crate::ocr::{self, Line};
use crate::settings::Settings;
use crate::shop::{self, ShopView, Status};

pub const ALERT_TITLE: &str = "Play Together seed alert";

/// One look at the shop.
pub struct Check {
    pub time: String,
    pub frame: Frame,
    pub lines: Vec<Line>,
    pub view: ShopView,
}

/// Captures the game window and reads the shop. Must run on a thread that
/// called `ocr::init_thread`.
pub fn check_once(window: &str, known: &[String], wanted: &[String]) -> Result<Check, String> {
    let win = capture::find_window(window).ok_or_else(|| {
        format!("Can't find the game window (\"{window}\"). Is the game open? You can pick it under \"Game window\".")
    })?;
    let frame = capture::capture(win.hwnd)?;
    if looks_blank(&frame) {
        return Err("The game window looks blank. Windows may not be drawing it: keep it on the same \
                    desktop, behind other windows if you like, but not minimized."
            .into());
    }
    let lines = ocr::read(&frame)?;
    let view = shop::read_shop(&lines, known, wanted);
    Ok(Check {
        time: capture::now_hms(),
        frame,
        lines,
        view,
    })
}

fn looks_blank(frame: &Frame) -> bool {
    let lum: Vec<f64> = frame
        .rgba
        .chunks_exact(4)
        .step_by(7)
        .map(|p| 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64)
        .collect();
    if lum.is_empty() {
        return true;
    }
    let mean = lum.iter().sum::<f64>() / lum.len() as f64;
    let var = lum.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / lum.len() as f64;
    var.sqrt() < 2.0
}

/// What the watcher shows in the window while it runs.
#[derive(Default)]
pub struct Live {
    pub running: bool,
    pub last_check: Option<String>,
    /// Latest status per watched seed.
    pub seeds: Vec<(String, Status)>,
    pub next_deal: Option<String>,
    /// Why the last check didn't work, or what the player should fix.
    pub problem: Option<String>,
    /// Newest last.
    pub log: Vec<String>,
}

impl Live {
    pub fn log(&mut self, msg: impl Into<String>) {
        self.log.push(format!("[{}] {}", capture::now_hms(), msg.into()));
        if self.log.len() > 300 {
            self.log.drain(..100);
        }
    }
}

/// Starts watching on a background thread. Set the returned flag to stop it.
pub fn start(
    settings: Settings,
    known: Vec<String>,
    live: Arc<Mutex<Live>>,
    repaint: impl Fn() + Send + 'static,
) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    {
        let mut l = live.lock().unwrap();
        l.running = true;
        l.problem = None;
        l.seeds = settings.picked.iter().map(|s| (s.clone(), Status::NotVisible)).collect();
        l.log(format!("Started watching: {}", settings.picked.join(", ")));
    }
    std::thread::spawn(move || {
        ocr::init_thread();
        watch_loop(&settings, &known, &live, &stop_flag, &repaint);
        live.lock().unwrap().running = false;
        live.lock().unwrap().log("Stopped.");
        repaint();
    });
    stop
}

fn watch_loop(settings: &Settings, known: &[String], live: &Mutex<Live>, stop: &AtomicBool, repaint: &dyn Fn()) {
    let wanted = &settings.picked;
    let interval = Duration::from_secs(settings.check_every_secs.clamp(5, 600));
    // Per seed: (already alerted for this stock, checks in a row without it).
    let mut state: Vec<(bool, u32)> = vec![(false, 0); wanted.len()];
    let mut last: Vec<Option<Status>> = vec![None; wanted.len()];
    let mut failures = 0u32;
    let mut lost_alert_sent = false;

    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let result = check_once(settings.window(), known, wanted);
        let mut newly_in_stock = Vec::new();
        let problem = match &result {
            Err(e) => Some(e.clone()),
            Ok(c) if c.view.seeds.iter().all(|s| s.status == Status::NotVisible) => Some(if c.view.shop_open {
                "The seed shop is open, but none of your seeds are in view. Scroll the list so they show.".to_string()
            } else {
                "Can't see the seed shop. Open it in the game and leave it open.".to_string()
            }),
            Ok(_) => None,
        };

        {
            let mut l = live.lock().unwrap();
            if let Ok(c) = &result {
                l.last_check = Some(c.time.clone());
                l.next_deal = c.view.next_deal.clone();
                for (i, seed) in c.view.seeds.iter().enumerate() {
                    let (alerted, misses) = &mut state[i];
                    match seed.status {
                        Status::InStock(n) => {
                            *misses = 0;
                            if !*alerted {
                                *alerted = true;
                                newly_in_stock.push(format!("{} ({n} left)", seed.name));
                            }
                        }
                        // A misread line says nothing either way.
                        Status::Unknown => {}
                        Status::OutOfStock | Status::NotVisible => {
                            *misses += 1;
                            if *alerted && *misses >= 3 {
                                *alerted = false;
                            }
                        }
                    }
                    if last[i] != Some(seed.status) {
                        l.log(format!("{}: {}", seed.name, seed.status.describe()));
                        last[i] = Some(seed.status);
                    }
                }
                l.seeds = c.view.seeds.iter().map(|s| (s.name.clone(), s.status)).collect();
            }
            if problem != l.problem {
                match &problem {
                    Some(p) => l.log(p.clone()),
                    None if l.problem.is_some() => l.log("Can see your seeds again."),
                    None => {}
                }
                l.problem = problem.clone();
            }
        }

        if !newly_in_stock.is_empty() {
            let body = format!("In stock now: {}", newly_in_stock.join(", "));
            let failed = notify::send(&settings.alerts, ALERT_TITLE, &body);
            let mut l = live.lock().unwrap();
            l.log(format!("ALERT sent: {body}"));
            for f in failed {
                l.log(f);
            }
        }

        if problem.is_some() {
            failures += 1;
        } else {
            failures = 0;
            lost_alert_sent = false;
        }
        if failures >= 3 && !lost_alert_sent && settings.alerts.notify_when_window_lost {
            lost_alert_sent = true;
            let body = problem.unwrap_or_default();
            let failed = notify::send(&settings.alerts, "Seed watcher needs attention", &body);
            let mut l = live.lock().unwrap();
            l.log("Sent a \"needs attention\" alert.");
            for f in failed {
                l.log(f);
            }
        }
        repaint();

        while started.elapsed() < interval && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}
