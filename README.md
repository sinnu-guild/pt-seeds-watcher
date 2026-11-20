# PT Seed Watcher

Get an alert when a Play Together seed you want is in stock, so you don't
have to keep checking the seed shop.

Pick your seeds, open the seed shop in the game, click **Start watching**,
and go do something else. When one of your seeds comes into stock you get a
Windows notification and a sound, and optionally a phone or Discord alert.

The app only looks at the screen. It never clicks, types, buys anything, or
touches the game's files or memory.

## Download and run

1. Download `pt-seeds-watcher.exe` from the
   [latest release](https://github.com/sinnu-guild/pt-seeds-watcher/releases/latest).
2. Double-click it. Nothing to install.
3. If Windows says **"Windows protected your PC"**, click **More info**, then
   **Run anyway**. This shows up for apps that aren't from a big publisher.

The **How to use** tab in the app walks you through the rest.

## Requirements

- Windows 10 or 11, with the game running on the same PC (the PC version or
  an emulator).
- The game set to **English**.
- The seed shop open, with your seeds visible in its list. The app can only
  check seeds that are on screen, so scroll the list to show them.
- The game not minimized. Other windows on top of it are fine.

## Phone alerts

On the **Alerts** tab, click **Make a topic**, install the free **ntfy** app
on your phone, and subscribe to the same topic name. Use **Send a test alert**
to check it works. You can also paste a Discord webhook URL there.

---

## For the maintainer

### Updating the seed list

The app downloads [seeds.json](seeds.json) from this repo's `main` branch
each time it starts, so players get new seeds without a new download.

1. Add the names to `seeds.json`, spelled exactly as the shop shows them.
2. Change `"updated"` to today's date.
3. Run `cargo test`. It fails if two names are too alike for the app to
   tell apart, or if a name isn't recognised as itself.
4. Commit and push to `main`.

`"message"` in `seeds.json` is shown at the top of everyone's app (leave it
empty for none).

This only works while the repo is **public**: players' apps download the
list without signing in.

### Releasing a new version

1. Bump `version` in `Cargo.toml`.
2. `cargo build --release` (from PowerShell) gives
   `target\release\pt-seeds-watcher.exe`. It runs on its own: the C runtime
   is built in ([.cargo/config.toml](.cargo/config.toml)), so players don't
   need the Visual C++ Redistributable.
3. Create a GitHub release and attach the exe.
4. Set `"latest_app_version"` in `seeds.json` to the new version and push.
   Older apps then show "A new version is available" with a download link.

### Testing without the app window

```powershell
.\target\release\pt-seeds-watcher.exe check game         # read the live game
.\target\release\pt-seeds-watcher.exe check shop.png     # read a saved screenshot
```

Both print every line of text the app read and each seed's status (for all
seeds in the built-in list), and save `check-preview.png` with boxes on what
it found. This runs the same code as the app's **Check now** button.

### How it works

- [src/capture.rs](src/capture.rs) takes a picture of the game window
  (`PrintWindow`, works when covered, not when minimized).
- [src/ocr.rs](src/ocr.rs) reads the left half of it (the seed list) with
  Windows' built-in text recognition, after making it gray, boosting the
  contrast and scaling it to 2160 pixels tall.
- [src/shop.rs](src/shop.rs) finds each seed's name and reads the line under
  it. Only "Stock: N" counts as in stock. "Out of Stock", or a missing or
  unreadable line (for example a row cut off at the bottom of the list), never
  alerts.
- [src/watcher.rs](src/watcher.rs) checks 7 seconds after each restock
  (the shop restocks every 5 minutes on the clock), retries a few times if
  the read was unclear, alerts once per restock, and says so if it can't see
  your seeds at a restock. "Every N seconds" is kept as a backup for PCs
  whose clock is off.
- [src/app.rs](src/app.rs) is the window (egui);
  [src/seedlist.rs](src/seedlist.rs) downloads the seed list;
  settings are saved in `%APPDATA%\pt-seeds-watcher\`.

## Support

The app is free. If it helped you get your seeds, you can buy me a coffee on
[Ko-fi](https://ko-fi.com/wasaque).
