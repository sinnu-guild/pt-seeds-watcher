mod capture;

fn main() {
    let Some(w) = capture::find_window("Play Together") else {
        eprintln!("game window not found");
        return;
    };
    println!("found: {} ({}x{})", w.title, w.width, w.height);
    match capture::capture(w.hwnd) {
        Ok(f) => println!("captured {}x{}, {} bytes", f.width, f.height, f.rgba.len()),
        Err(e) => eprintln!("capture failed: {e}"),
    }
}
