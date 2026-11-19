// Embeds the icon and version info into the exe, so it shows in Explorer.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "PT Seed Watcher");
        res.set("FileDescription", "PT Seed Watcher");
        if let Err(e) = res.compile() {
            // Not fatal: the app works without an exe icon.
            println!("cargo:warning=couldn't embed the exe icon: {e}");
        }
    }
}
