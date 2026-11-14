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
