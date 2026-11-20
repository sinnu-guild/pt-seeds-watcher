//! Checking the shop: once ("Check now"), or on a background thread while
//! watching (right after each restock, or every few seconds). Only reads the
//! screen.

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
    /// When the next check happens (local HH:MM:SS).
    pub next_check: Option<String>,
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
        {
            let mut l = live.lock().unwrap();
            l.running = false;
            l.next_check = None;
            l.log("Stopped.");
        }
        repaint();
    });
    stop
}

/// The shop restocks on the clock every 5 minutes (:00, :05, :10, ...).
pub const RESTOCK_EVERY_SECS: u64 = 300;
/// After a restock check that couldn't read everything, try again this often...
const RETRY_EVERY_SECS: u64 = 10;
/// ...this many times, before giving up on that restock.
const RETRIES: u32 = 5;

/// The next restock check after `now` (Unix seconds): a restock time plus
/// `delay` seconds. Restocks line up with the Unix clock because every time
/// zone is a whole number of 5 minutes off UTC.
pub fn next_restock_check(now: u64, delay: u64) -> u64 {
    let delay = delay % RESTOCK_EVERY_SECS;
    now.saturating_sub(delay) / RESTOCK_EVERY_SECS * RESTOCK_EVERY_SECS + delay + RESTOCK_EVERY_SECS
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Sleeps until the Unix time `target`. Returns false if asked to stop.
fn sleep_until(target: u64, stop: &AtomicBool) -> bool {
    while unix_now() < target {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !stop.load(Ordering::Relaxed)
}

/// Sleeps for `wait`. Returns false if asked to stop.
fn sleep_for(wait: Duration, stop: &AtomicBool) -> bool {
    let started = Instant::now();
    while started.elapsed() < wait {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !stop.load(Ordering::Relaxed)
}

/// Remembers what was alerted between checks.
struct Tracker<'a> {
    settings: &'a Settings,
    known: &'a [String],
    live: &'a Mutex<Live>,
    /// Re-arm a seed as soon as it reads "Out of Stock" (each restock check is
    /// a new restock), instead of after 3 misses.
    rearm_on_out_of_stock: bool,
    /// Per seed: (already alerted for this stock, checks in a row without it).
    state: Vec<(bool, u32)>,
    last: Vec<Option<Status>>,
    failures: u32,
    lost_alert_sent: bool,
}

/// How a check went.
struct Outcome {
    problem: Option<String>,
    /// Some picked seed was found but its stock line couldn't be read.
    unreadable: bool,
}

impl Outcome {
    fn clean(&self) -> bool {
        self.problem.is_none() && !self.unreadable
    }
}

impl Tracker<'_> {
    /// Looks at the shop once, updates the window and sends any stock alert.
    fn check(&mut self) -> Outcome {
        let wanted = &self.settings.picked;
        let result = check_once(self.settings.window(), self.known, wanted);
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
        let unreadable = result
            .as_ref()
            .is_ok_and(|c| c.view.seeds.iter().any(|s| s.status == Status::Unknown));

        {
            let mut l = self.live.lock().unwrap();
            if let Ok(c) = &result {
                l.last_check = Some(c.time.clone());
                l.next_deal = c.view.next_deal.clone();
                for (i, seed) in c.view.seeds.iter().enumerate() {
                    let (alerted, misses) = &mut self.state[i];
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
                        Status::OutOfStock if self.rearm_on_out_of_stock => *alerted = false,
                        Status::OutOfStock | Status::NotVisible => {
                            *misses += 1;
                            if *alerted && *misses >= 3 {
                                *alerted = false;
                            }
                        }
                    }
                    if self.last[i] != Some(seed.status) {
                        l.log(format!("{}: {}", seed.name, seed.status.describe()));
                        self.last[i] = Some(seed.status);
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
            let failed = notify::send(&self.settings.alerts, ALERT_TITLE, &body);
            let mut l = self.live.lock().unwrap();
            l.log(format!("ALERT sent: {body}"));
            for f in failed {
                l.log(f);
            }
        }
        Outcome { problem, unreadable }
    }

    /// Counts a failed (or good) check, and sends one "needs attention" alert
    /// once `limit` failures happen in a row.
    fn count(&mut self, problem: &Option<String>, limit: u32) {
        let Some(p) = problem else {
            self.failures = 0;
            self.lost_alert_sent = false;
            return;
        };
        self.failures += 1;
        if self.failures >= limit && !self.lost_alert_sent && self.settings.alerts.notify_when_window_lost {
            self.lost_alert_sent = true;
            let failed = notify::send(&self.settings.alerts, "Seed watcher needs attention", p);
            let mut l = self.live.lock().unwrap();
            l.log("Sent a \"needs attention\" alert.");
            for f in failed {
                l.log(f);
            }
        }
    }

    fn set_next_check(&self, in_secs: u64) {
        self.live.lock().unwrap().next_check = Some(capture::local_hms_in(in_secs));
    }
}

fn watch_loop(settings: &Settings, known: &[String], live: &Mutex<Live>, stop: &AtomicBool, repaint: &dyn Fn()) {
    let n = settings.picked.len();
    let mut t = Tracker {
        settings,
        known,
        live,
        rearm_on_out_of_stock: settings.follow_restock,
        state: vec![(false, 0); n],
        last: vec![None; n],
        failures: 0,
        lost_alert_sent: false,
    };

    if settings.follow_restock {
        // One look right away so the player sees it working.
        t.check();
        loop {
            let next = next_restock_check(unix_now(), settings.restock_delay_secs);
            t.set_next_check(next.saturating_sub(unix_now()));
            repaint();
            if !sleep_until(next, stop) {
                break;
            }
            // A single bad read would miss a whole restock, so retry a few times.
            let mut outcome = t.check();
            for _ in 0..RETRIES {
                if outcome.clean() {
                    break;
                }
                repaint();
                if !sleep_for(Duration::from_secs(RETRY_EVERY_SECS), stop) {
                    return;
                }
                outcome = t.check();
            }
            t.count(&outcome.problem, 1);
        }
    } else {
        let interval = Duration::from_secs(settings.check_every_secs.clamp(5, 600));
        while !stop.load(Ordering::Relaxed) {
            let started = Instant::now();
            let outcome = t.check();
            t.count(&outcome.problem, 3);
            t.set_next_check(interval.as_secs());
            repaint();
            if !sleep_for(interval.saturating_sub(started.elapsed()), stop) {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-20 00:00:00 UTC.
    const DAY: u64 = 1_795_132_800;

    fn at(h: u64, m: u64, s: u64) -> u64 {
        DAY + h * 3600 + m * 60 + s
    }

    #[test]
    fn restock_checks_land_seven_seconds_after_each_restock() {
        assert_eq!(next_restock_check(at(11, 55, 6), 7), at(11, 55, 7));
        assert_eq!(next_restock_check(at(11, 55, 7), 7), at(12, 0, 7));
        assert_eq!(next_restock_check(at(11, 55, 8), 7), at(12, 0, 7));
        assert_eq!(next_restock_check(at(11, 59, 59), 7), at(12, 0, 7));
        assert_eq!(next_restock_check(at(12, 0, 0), 7), at(12, 0, 7));
    }

    #[test]
    fn restock_checks_cross_midnight() {
        assert_eq!(next_restock_check(at(23, 58, 0), 7), at(24, 0, 7));
    }

    #[test]
    fn restock_check_with_no_delay() {
        assert_eq!(next_restock_check(at(11, 55, 0), 0), at(12, 0, 0));
        assert_eq!(next_restock_check(at(11, 54, 59), 0), at(11, 55, 0));
    }
}
