//! PT Seed Watcher: watches the Play Together seed shop and alerts you when
//! a seed you picked is in stock. It only reads the screen; it never clicks,
//! types or buys anything.

// No console window when players double-click the exe.
#![windows_subsystem = "windows"]

#[cfg(not(windows))]
compile_error!("PT Seed Watcher only runs on Windows.");

mod app;
mod capture;
mod notify;
mod ocr;
mod seedlist;
mod settings;
mod shop;
mod watcher;

use eframe::egui;

const ICON: &[u8] = include_bytes!("../assets/icon.png");

fn main() {
    capture::init_dpi();
    // Not fatal: without it, alerts still play the sound and reach phone/Discord.
    let _ = notify::register(ICON);

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("test-alert") {
        attach_console();
        std::process::exit(cmd_test_alert());
    }
    if let [cmd, path] = args.as_slice() {
        if cmd == "check" {
            attach_console();
            std::process::exit(match cmd_check(path) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            });
        }
    }

    let icon = image::load_from_memory(ICON).map(|i| i.to_rgba8()).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(app::APP_NAME)
        .with_inner_size([1000.0, 720.0])
        .with_min_inner_size([760.0, 520.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(egui::IconData {
            width: icon.width(),
            height: icon.height(),
            rgba: icon.into_raw(),
        });
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let _ = eframe::run_native(
        app::APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(&cc.egui_ctx)))),
    );
}

/// Lets `check` print to the terminal it was started from.
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// For troubleshooting, `pt-seeds-watcher.exe test-alert`: shows a Windows
/// notification from a watcher-like thread and says how long it took. Only
/// the Windows notification: it doesn't post to ntfy or Discord.
fn cmd_test_alert() -> i32 {
    if let Err(e) = notify::register(ICON) {
        eprintln!("{e}");
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        ocr::init_thread();
        let started = std::time::Instant::now();
        let result = notify::desktop(watcher::ALERT_TITLE, "Test alert from the command line.");
        let _ = tx.send((result, started.elapsed()));
    });
    match rx.recv_timeout(std::time::Duration::from_secs(15)) {
        Ok((Ok(()), took)) => {
            println!("Windows notification sent in {} ms. Did it show up?", took.as_millis());
            0
        }
        Ok((Err(e), took)) => {
            eprintln!("Windows notification failed after {} ms: {e}", took.as_millis());
            1
        }
        Err(_) => {
            eprintln!("Windows notification is stuck: no answer after 15 s.");
            2
        }
    }
}

/// For testing, `pt-seeds-watcher.exe check <picture.png | game>`: reads a
/// saved screenshot (or the live game window, like "Check now"), prints what
/// it found for every seed in the built-in list, and saves check-preview.png.
fn cmd_check(source: &str) -> Result<(), String> {
    ocr::init_thread();
    let known = seedlist::built_in().seeds;
    let started = std::time::Instant::now();
    let check = if source == "game" {
        watcher::check_once(settings::DEFAULT_WINDOW, &known, &known)?
    } else {
        let img = image::open(source).map_err(|e| format!("can't open {source}: {e}"))?.to_rgba8();
        let frame = capture::Frame {
            width: img.width(),
            height: img.height(),
            rgba: img.into_raw(),
        };
        let lines = ocr::read(&frame)?;
        let view = shop::read_shop(&lines, &known, &known);
        watcher::Check {
            time: capture::now_hms(),
            frame,
            lines,
            view,
        }
    };
    println!("Read {} line(s) in {} ms:", check.lines.len(), started.elapsed().as_millis());
    for l in &check.lines {
        println!("  {:>5.0},{:>5.0}  {}", l.x, l.y, l.text);
    }
    println!("\nShop open: {}", check.view.shop_open);
    if let Some(d) = &check.view.next_deal {
        println!("Next restock in: {d}");
    }
    for s in &check.view.seeds {
        println!("  {:<22} {}", s.name, s.status.describe());
    }
    let (w, h) = (check.frame.width, check.frame.height);
    image::RgbaImage::from_raw(w, h, app::annotate(&check))
        .ok_or("preview had the wrong size")?
        .save("check-preview.png")
        .map_err(|e| format!("couldn't save check-preview.png: {e}"))?;
    println!("\nSaved check-preview.png");
    Ok(())
}
