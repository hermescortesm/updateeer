<p align="center">
  <img src="assets/icon.png" alt="Updateeer logo" width="128" height="128">
</p>

<h1 align="center">Updateeer</h1>

<p align="center"><strong>One window to see every outdated app on your computer and update them from a single queue.</strong></p>

<p align="center">Free and open source · macOS · Windows · Linux · MIT License</p>

---

## What it does

- **Finds what is outdated.** Scans your package managers, compares installed and available versions and lists every app with an update.
- **Updates from a queue.** Update one app, a selection, or everything at once. Two updates run in parallel, failed ones are retried once, and each row shows its progress.
- **Cancel any time.** Cancelling stops the running updates (and everything they launched) and puts the apps back to "available".
- **Shows everything installed.** An *Installed* page lists every app with its source, version and install date, including apps you downloaded by hand (macOS).
- **Keeps a history.** Every update, with its date and result, survives restarts.
- **Detail page for each app.** Versions, source, vendor, size and dates in one place.
- **Search, filter and sort.** By name, source (App Store, Homebrew, winget, apt, Flatpak…), type (apps or command-line tools), install date and more.
- **Real icons.** Uses each app's own icon, a built-in library of 125 brand logos, or a colored letter tile.
- **Automatic updates (optional).** Apps that are closed update on their own. For apps that are open — or that need administrator rights (apt, pacman, dnf, Chocolatey) — Updateeer asks once, *update now* or *later*, grouped in a single question.
- **Runs in the background.** Menu bar (macOS) or notification-area (Windows) icon with the number of pending updates, and optional *open at login* on every system. On Linux the window minimizes instead (no tray icon yet).
- **Light and dark themes**, following your system by default.
- **Small and fast.** Native Rust + [egui](https://github.com/emilk/egui), a single binary of a few MB, and no CPU use while idle.

## Supported sources

| System  | Sources |
|---------|---------|
| macOS   | Homebrew (casks and formulae), Mac App Store (via [`mas`](https://github.com/mas-cli/mas)), apps downloaded directly (listed, not yet updated) |
| Windows | winget, Scoop, Chocolatey |
| Linux   | apt, pacman, dnf, Flatpak, Snap |

On macOS, Updateeer can install Homebrew and `mas` for you — only after you click the button.

## Install

Download the installer for your system from the [latest release](https://github.com/hermescortesm/updateeer/releases/latest):

| System | File |
|--------|------|
| macOS (Apple silicon) | `Updateeer-<version>-macos-arm64.dmg` |
| macOS (Intel) | `Updateeer-<version>-macos-x86_64.dmg` |
| Windows (x64) | `Updateeer-<version>-windows-x64.msi` |
| Debian / Ubuntu / Mint | `Updateeer-<version>-linux-amd64.deb` |
| Arch / Manjaro | `Updateeer-<version>-linux-x86_64.pkg.tar.zst` |

The installers are not code-signed yet, so macOS and Windows will warn you the first time (macOS: right-click → Open; Windows: *More info* → *Run anyway*).

## Build from source

```sh
cargo build --release     # needs Rust 1.89 or newer
cargo test
```

Packaging scripts: `tools/package-macos.sh <target>` (needs `pip install dmgbuild`; builds the drag-to-Applications installer), `tools/package-deb.sh`, `packaging/windows/main.wxs` (WiX 5) and `packaging/arch/PKGBUILD`.

## Privacy

Updateeer talks only to your local package managers. It has no account, no telemetry and no network code of its own.

## Author

Created by **Hermes Cortes** — <admin@hermescortes.com>.

## License

[MIT](LICENSE) — free to use, copy, modify and distribute.
