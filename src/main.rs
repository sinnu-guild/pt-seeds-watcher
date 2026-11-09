//! pt-seeds-watcher: looks at the Play Together seed shop every so often and
//! alerts you when a seed you care about shows up. It only reads the screen;
//! it never clicks, types or buys anything.

#[cfg(not(windows))]
compile_error!("pt-seeds-watcher only runs on Windows. Build it from PowerShell, not WSL.");

mod capture;
mod config;
mod matcher;

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
