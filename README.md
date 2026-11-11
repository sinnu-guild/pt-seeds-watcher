# pt-seeds-watcher

Alerts you when a seed you want shows up in the Play Together seed shop.

It takes a picture of the game window every 20 seconds and looks for the
seeds you've chosen. That's all it does: it never clicks, types or buys
anything, and it doesn't read the game's memory or network traffic.

## Setup (once)

Run everything from **PowerShell** in this folder, not from WSL.

```powershell
cd D:\Codes\pt-seeds-watcher
cargo build --release
```

The first build downloads its libraries and takes a few minutes.

## 1. Check that the watcher can see the game

1. Open the game and open the seed shop.
2. Put the game where you'll keep it while you work (Desktop 2, or behind
   your other windows). **Don't minimize it**, because Windows stops drawing minimized windows.
3. Run:

   ```powershell
   cargo run --release -- snapshot
   ```

4. Open `snapshot.png`. If it shows the seed shop, you're good.

If it can't find the game, run `cargo run --release -- windows` to see the
window titles, and put the game's exact title in `window_title` in
`config.toml`.

If `snapshot.png` is black or out of date while the game is on another
desktop, move the game back to your main desktop behind your other windows
and try again.

## 2. Make a picture for each seed

1. Open `snapshot.png` in Paint (ideally taken while the seed is in stock).
2. Select a box around the seed's icon, then click **Crop**.
3. Save it as **PNG** in the `templates` folder, named after the seed,
   for example `templates\Golden Corn.png`.

Tips:
- Crop from `snapshot.png`, not from a normal screenshot, so the size
  matches what the watcher sees.
- Include only the parts that always look the same. Leave out stock counts,
  timers, prices that change and hover highlights.
- If the shop still shows sold-out seeds, crop something that only looks
  that way when the seed is buyable (for example, the icon together with a
  coloured Buy button).
- Don't change the game's window size afterwards. If you do, take a new
  snapshot and crop the pictures again.

## 3. List the seeds in config.toml

At the bottom of `config.toml`, add one block per seed:

```toml
[[seeds]]
name = "Golden Corn"
```

## 4. Test it

```powershell
cargo run --release -- test
```

This prints a score for each seed (1.00 = perfect match) and saves
`debug.png` with a box around where it found each one. Green means found,
red means the best guess wasn't good enough.

A seed that's on screen usually scores above 0.9. A seed that isn't on screen
usually scores well below 0.7. If yours are close together, crop a more distinctive
picture or change `threshold`.

## 5. Set up alerts

```powershell
cargo run --release -- test-alert
```

By default you get a Windows notification and a sound. To get alerts on
your phone, install the **ntfy** app, subscribe to a topic with a
hard-to-guess name, and put that name in `ntfy_topic` in `config.toml`.
Anyone who knows the topic name can read your alerts, so make it hard to guess.
You can also paste a Discord webhook URL into `discord_webhook`.

## 6. Run it

```powershell
cargo run --release
```

Leave the window open while you work. Each check prints one line with every
seed's score. When a seed appears you get one alert. You get another only after
the seed has been gone for a few checks and comes back. If the watcher can't see
the game window for about a minute (game closed or minimized), it alerts
you once so you know. It can't tell if the game closes the shop menu by
itself, though: then it just never finds any seeds. Check on it now and then.

Press **Ctrl+C** to stop.

You can also run the built program directly:
`.\target\release\pt-seeds-watcher.exe` (from this folder, so it finds
`config.toml`).

## Settings

All settings are in `config.toml` and explained there. The useful ones:

| Setting | What it does |
|---|---|
| `check_every_secs` | How often to look (default 20) |
| `threshold` | How close a match must be (default 0.85) |
| `region` | Search only part of the window, `[x, y, width, height]` |
| `rearm_after_misses` | Checks a seed must be gone before it can alert again |
