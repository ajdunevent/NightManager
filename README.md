# NightManager

NightManager keeps your phone from disturbing your sleep. At the evening time it
switches to the night settings you choose, and in the morning it puts your day
settings back. Built for the FuriLabs FLX1s with Phosh, and usable on GNOME.

## What it changes at night

* Night Light
* Automatic brightness (ambient light sensor)
* Dark style
* Accent color
* Legacy (GTK3) theme, such as [adw-gtk3](https://github.com/lassekongo83/adw-gtk3)
* Icon theme, such as [Adwaita-colors](https://github.com/dpejoh/Adwaita-colors)
* Feedback profile: Quiet or Silent
* Wallpaper: a picture, or none
* Lock screen picture, or none

Each one is optional. The values in effect when night mode starts are saved and
put back when it ends. A night wallpaper is set for both the default and the
dark style, so it shows whichever style is on. Chosen pictures are copied into
the app's data folder, where Phosh can read them. With **Keep Theme Changes Made at Night** on, an accent,
theme or icon change you make at night becomes the night value.

Rows for features the device lacks (no feedbackd, no Night Light) are hidden.

## When it runs

* **Schedule:** a start and an end time for each day, either of which can be off,
  or Night Light's own schedule (sunset to sunrise, or its manual times).
* **Manual triggers:** Dark Style, Night Light, or a feedback profile changed
  from the quick settings can start or end night mode. It stays that way until
  the next scheduled change.
* **The Night Mode switch** at the top of the window does the same.

Night Light changes while the screen is off can leave it stuck on some phones,
so those either turn the screen on for about two seconds or wait for the
screen to come on, as set in the app.

## Install

### Flatpak (recommended)

| <a href="https://ajdunevent.github.io/AJHub/"><picture><source media="(prefers-color-scheme: dark)" srcset="https://ajdunevent.github.io/AJHub/badges/button-dark.svg"><img src="https://ajdunevent.github.io/AJHub/badges/button-light.svg" alt="AJHub, published with FrogPak" height="48"></picture></a> | <a href="https://furihub.github.io/repo/"><picture><source media="(prefers-color-scheme: dark)" srcset="https://furihub.github.io/repo/badges/button-dark.svg"><img src="https://furihub.github.io/repo/badges/button-light.svg" alt="FuriHub, published with FrogPak" height="48"></picture></a> |
| --- | --- |
| x86_64 & aarch64 | aarch64 only |

Open the app once after installing: it asks to start its background service at
login.

To change the theme of legacy (GTK3) Flatpak apps too, install the matching
GTK3 theme Flatpak, for example
`flatpak install flathub org.gtk.Gtk3theme.adw-gtk3 org.gtk.Gtk3theme.adw-gtk3-dark`.

### Moving from the old package

The settings and schedule of the old script-based version are imported
automatically. NightManager stays idle until the old package is removed:

```bash
sudo apt remove nightmanager
```

Then press **Check Again** in the app. If the old version was in night mode,
its saved day settings are restored at the next scheduled end.

### If Night Light stops responding

On the FLX1s, a Night Light change made with the screen off can leave the
display's PQ service stuck. To recover without rebooting:

```bash
sudo lxc-attach -n android -- /system/bin/sh -c 'stop pq-2-2; sleep 1; start pq-2-2'
systemctl --user restart gsd-adapter pqdbus
```

## Building

For quick incremental builds, `build-aux/dev.sh` asks for the architecture,
whether to write a bundle to `dist/` and/or install it, and the build profile.
It needs `org.flatpak.Builder` (or `flatpak-builder`), and QEMU user emulation
for the other architecture.

A full build from the manifest, as the release workflow does it:

```bash
flatpak-builder --user --install --force-clean build-aux/.build build-aux/net.dunevent.NightManager.json
```

Tests:

```bash
cargo test
cargo build && tests/daemon.sh
```

`tests/sandbox-dconf.sh` checks, on a device with the Flatpak installed, that
the sandbox can read, write and watch the host's settings.

## Design

Two programs share the `nightmanager` library:

* `nightmanager-daemon`: no GTK. Started at login through the Background portal
  or on demand through D-Bus activation. It owns
  `net.dunevent.NightManager.Daemon`, keeps the schedule on a `CLOCK_REALTIME`
  timerfd, watches the trigger settings, and saves its state to
  `~/.var/app/net.dunevent.NightManager/.local/state/nightmanager/state.json`.
* `nightmanager`: the libadwaita settings window.

Host settings are reached through dconf, using a dconf GIO module built into
the Flatpak. It carries a small patch so that `DCONF_USER_CONFIG_DIR` points
its reads at the host's `~/.config/dconf`; without it, writes reach the host
but reads see only the schema defaults. The daemon checks this at startup and
won't run if a value it writes can't be read back. Trimmed copies of the gnome-settings-daemon and feedbackd schemas
are bundled because the GNOME runtime doesn't include them.

## Shout out

* [FuriLabs](https://furilabs.com) for making the FLX1s, the first mobile Linux
  phone many of us have been able to comfortably (enjoyably even!) daily drive.
