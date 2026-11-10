//! pt-seeds-watcher: looks at the Play Together seed shop every so often and
//! alerts you when a seed you care about shows up. It only reads the screen;
//! it never clicks, types or buys anything.

#[cfg(not(windows))]
compile_error!("pt-seeds-watcher only runs on Windows. Build it from PowerShell, not WSL.");

mod capture;
mod config;
mod matcher;
mod notify;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use config::Config;
use image::{Rgba, RgbaImage};
use matcher::{Gray, Match, Searcher, Template};

const ALERT_TITLE: &str = "Play Together seed alert";

const HELP: &str = "\
pt-seeds-watcher - alerts you when chosen seeds are in the Play Together seed shop

USAGE:
    pt-seeds-watcher [COMMAND] [--config <path>]

COMMANDS:
    watch        Keep checking and alert when a seed appears (default)
    snapshot     Save what the watcher sees to snapshot.png
    test         Check once, print each seed's score, save debug.png
    test-alert   Send a test alert to every enabled channel
    windows      List open windows (to find the game's exact title)

OPTIONS:
    -c, --config <path>   Config file to use (default: config.toml)
    -h, --help            Show this help
";

fn main() {
    capture::init_dpi();

    let mut args = std::env::args().skip(1);
    let mut config_path = PathBuf::from("config.toml");
    let mut command: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => match args.next() {
                Some(p) => config_path = PathBuf::from(p),
                None => exit_with("--config needs a file path"),
            },
            "-h" | "--help" | "help" => {
                print!("{HELP}");
                return;
            }
            other if command.is_none() => command = Some(other.to_string()),
            other => exit_with(&format!("unexpected argument '{other}'. Run with --help.")),
        }
    }

    let command = command.unwrap_or_else(|| "watch".into());
    let result = if command == "windows" {
        cmd_windows(&config_path)
    } else {
        load_config(&config_path).and_then(|(cfg, dir)| match command.as_str() {
            "snapshot" => cmd_snapshot(&cfg, &dir),
            "test" => cmd_test(&cfg, &dir),
            "test-alert" => cmd_test_alert(&cfg),
            other => Err(format!("unknown command '{other}'. Run with --help.")),
        })
    };
    if let Err(e) = result {
        exit_with(&e);
    }
}

fn exit_with(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

/// Reads config.toml. Returns it with the folder it lives in, which other
/// paths (templates, snapshot.png) are relative to.
fn load_config(path: &Path) -> Result<(Config, PathBuf), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("can't read {}: {e}. Run this from the project folder, or pass --config.", path.display()))?;
    let cfg: Config = toml::from_str(&text).map_err(|e| format!("problem in {}:\n{e}", path.display()))?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok((cfg, dir))
}

fn load_templates(cfg: &Config, dir: &Path) -> Result<Vec<Template>, String> {
    if cfg.seeds.is_empty() {
        return Err("no seeds are listed in config.toml yet. Add a [[seeds]] entry for each seed \
                    (see the examples at the bottom of the file)."
            .into());
    }
    let folder = dir.join(&cfg.templates_dir);
    cfg.seeds
        .iter()
        .map(|seed| {
            let file = seed.template.clone().unwrap_or_else(|| format!("{}.png", seed.name));
            let path = folder.join(&file);
            let img = image::open(&path)
                .map_err(|e| format!("can't open the picture for '{}' at {}: {e}", seed.name, path.display()))?
                .to_rgba8();
            let gray = Gray::from_rgba(img.width() as usize, img.height() as usize, img.as_raw());
            Template::new(&seed.name, gray, seed.threshold.unwrap_or(cfg.threshold))
        })
        .collect()
}

/// Finds the game window and captures it.
fn grab(cfg: &Config) -> Result<(capture::Frame, String), String> {
    let win = capture::find_window(&cfg.window_title).ok_or_else(|| {
        format!(
            "no open window titled like \"{}\". Is the game running? \
             Run the `windows` command to see window titles.",
            cfg.window_title
        )
    })?;
    let frame = capture::capture(win.hwnd)?;
    Ok((frame, win.title))
}

/// The part of the frame to search (the whole window, or `region`), as grayscale,
/// plus where that part starts inside the window.
fn search_area(cfg: &Config, frame: &capture::Frame) -> (Gray, usize, usize) {
    let gray = Gray::from_rgba(frame.width as usize, frame.height as usize, &frame.rgba);
    match cfg.region {
        Some([x, y, w, h]) => (gray.crop(x as usize, y as usize, w as usize, h as usize), x as usize, y as usize),
        None => (gray, 0, 0),
    }
}

fn looks_blank(gray: &Gray) -> bool {
    gray.std_dev() < 2.0
}

const BLANK_HELP: &str = "the captured picture is blank (all one colour). Windows may not be \
drawing the game where it is now; try keeping it on the same desktop behind your other windows";

fn frame_to_image(frame: capture::Frame) -> Result<RgbaImage, String> {
    RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
        .ok_or_else(|| "captured picture had the wrong size".to_string())
}

fn cmd_windows(config_path: &Path) -> Result<(), String> {
    let wanted = load_config(config_path)
        .map(|(c, _)| c.window_title)
        .unwrap_or_else(|_| Config::default().window_title);
    let chosen = capture::find_window(&wanted).map(|w| w.hwnd);
    println!("Open windows (the watcher looks for a title like \"{wanted}\"):\n");
    for w in capture::list_windows() {
        let mut notes = Vec::new();
        if w.minimized {
            notes.push("minimized");
        }
        if w.cloaked {
            notes.push("hidden / other desktop");
        }
        let mark = if Some(w.hwnd) == chosen { "=> " } else { "   " };
        let notes = if notes.is_empty() { String::new() } else { format!("  [{}]", notes.join(", ")) };
        println!("{mark}{}  ({}x{}){notes}", w.title, w.width, w.height);
    }
    match chosen {
        Some(_) => println!("\n=> marks the window the watcher will use."),
        None => println!("\nNo match. Set window_title in config.toml to the game's title from this list."),
    }
    Ok(())
}

fn cmd_snapshot(cfg: &Config, dir: &Path) -> Result<(), String> {
    let (frame, title) = grab(cfg)?;
    let (gray, _, _) = search_area(cfg, &frame);
    let blank = looks_blank(&gray);
    let (w, h) = (frame.width, frame.height);
    let path = dir.join("snapshot.png");
    frame_to_image(frame)?
        .save(&path)
        .map_err(|e| format!("couldn't save {}: {e}", path.display()))?;

    println!("Captured \"{title}\" ({w}x{h}) to {}", path.display());
    if blank {
        println!("\nWARNING: {BLANK_HELP}.");
    } else {
        println!(
            "\nOpen snapshot.png. If it shows the seed shop, capture works from where the game is now.\n\
             Next: crop each seed you want from snapshot.png and save it as templates\\<Seed Name>.png"
        );
    }
    Ok(())
}

fn cmd_test(cfg: &Config, dir: &Path) -> Result<(), String> {
    let templates = load_templates(cfg, dir)?;
    let (frame, title) = grab(cfg)?;
    let (gray, ox, oy) = search_area(cfg, &frame);
    println!("Captured \"{title}\" ({}x{})", frame.width, frame.height);
    if looks_blank(&gray) {
        println!("WARNING: {BLANK_HELP}.");
    }

    let started = Instant::now();
    let mut searcher = Searcher::new(gray);
    let results: Vec<Option<Match>> = templates.iter().map(|t| searcher.find(t)).collect();
    println!("Checked {} seed(s) in {} ms\n", templates.len(), started.elapsed().as_millis());

    let mut img = frame_to_image(frame)?;
    if let Some([x, y, w, h]) = cfg.region {
        draw_box(&mut img, x, y, w, h, Rgba([60, 140, 255, 255]));
    }
    for (t, m) in templates.iter().zip(&results) {
        match m {
            Some(m) => {
                let hit = m.score >= t.threshold;
                println!(
                    "  {:<24} score {:.3} (needs {:.2}) at {},{}  {}",
                    t.name,
                    m.score,
                    t.threshold,
                    m.x + ox,
                    m.y + oy,
                    if hit { "<- FOUND" } else { "" }
                );
                let color = if hit { Rgba([0, 220, 0, 255]) } else { Rgba([255, 40, 40, 255]) };
                draw_box(&mut img, (m.x + ox) as u32, (m.y + oy) as u32, t.w as u32, t.h as u32, color);
            }
            None => println!("  {:<24} picture is bigger than the search area", t.name),
        }
    }
    let path = dir.join("debug.png");
    img.save(&path).map_err(|e| format!("couldn't save {}: {e}", path.display()))?;
    println!(
        "\nSaved {}: green box = found, red box = best guess that wasn't good enough, blue = region.",
        path.display()
    );
    Ok(())
}

fn cmd_test_alert(cfg: &Config) -> Result<(), String> {
    println!("Sending a test alert...");
    notify::send(&cfg.alerts, ALERT_TITLE, "Test alert from pt-seeds-watcher. Alerts are working!");
    println!("Done. Any channel that failed is listed above.");
    Ok(())
}

fn draw_box(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    let (iw, ih) = img.dimensions();
    let mut put = |px: u32, py: u32| {
        if px < iw && py < ih {
            img.put_pixel(px, py, color);
        }
    };
    for t in 0..2 {
        for i in x..x.saturating_add(w) {
            put(i, y + t);
            put(i, (y + h).saturating_sub(1 + t));
        }
        for j in y..y.saturating_add(h) {
            put(x + t, j);
            put((x + w).saturating_sub(1 + t), j);
        }
    }
}
