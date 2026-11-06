mod capture;

fn main() {
    for w in capture::list_windows() {
        println!("{:>6}x{:<6} {}", w.width, w.height, w.title);
    }
    match capture::find_window("Play Together") {
        Some(w) => println!("found: {}", w.title),
        None => println!("game window not found"),
    }
}
