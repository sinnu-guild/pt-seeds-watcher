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

The **How to use** tab in the app walks you through the rest.

## Requirements

- Windows 10 or 11, with the game running on the same PC (the PC version or
  an emulator).
- The game set to **English**.
- The seed shop open, with your seeds visible in its list. The app can only
  check seeds that are on screen, so scroll the list to show them. It also
  reads the seed shown on the right (the one you clicked), so you can watch
  one more seed that's scrolled out of the list.
- The game not minimized. Other windows on top of it are fine.

## Phone alerts

On the **Alerts** tab, click **Make a topic**, install the free **ntfy** app
on your phone, and subscribe to the same topic name. Use **Send a test alert**
to check it works. You can also paste a Discord webhook URL there.

## A blue Windows box came up the first time?

That's Windows SmartScreen. It shows up for new apps from small, indie
developers that haven't paid for a code-signing certificate, not because
anything is wrong with the app. Click **More info**, then **Run anyway**. You
only see it once.

The full source code is right here in this repo if you want to check what the
app does.

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

1. Bump `version` in `Cargo.toml` (e.g. `1.0.1`), run `cargo build` so
   `Cargo.lock` updates too, then commit and push.
2. Tag it and push the tag:
   ```powershell
   git tag v1.0.1
   git push origin v1.0.1
   ```
3. That's it. [The release workflow](.github/workflows/release.yml) builds
   the exe on GitHub, publishes a release with it attached, then sets
   `"latest_app_version"` in `seeds.json`, so older apps show "A new version
   is available" with a download link. It takes about 10 minutes; watch it
   under the repo's **Actions** tab. Pull afterwards (`git pull`) to get its
   `seeds.json` commit.

The tag must match the version in `Cargo.toml`, or the workflow stops
without releasing anything. The exe runs on its own: the C runtime is built
in ([.cargo/config.toml](.cargo/config.toml)), so players don't need the
Visual C++ Redistributable.

To build it locally instead: `cargo build --release` gives
`target\release\pt-seeds-watcher.exe`.

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
- [src/ocr.rs](src/ocr.rs) reads it with Windows' built-in text recognition,
  after making it gray, boosting the contrast and scaling it to 2160 pixels
  tall.
- [src/shop.rs](src/shop.rs) finds each seed's name in the list (left half)
  and reads the line under it. It also reads the selected seed on the right:
  its name at the top and the "Stock: N" or gray "Out of Stock" button below.
  Only "Stock: N" counts as in stock. "Out of Stock", or a missing or
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
