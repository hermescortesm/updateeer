//! Curated apps for the Discover page, with the id each package manager knows them by:
//! Homebrew cask (macOS), winget id (Windows), Flathub id (Linux). An empty id means "not offered there".
use crate::i18n::t;
use crate::model::{App, Source};
use crate::scan;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    All,
    Browsers,
    Dev,
    Utilities,
    Chat,
    Media,
    Work,
    Games,
}

impl Cat {
    pub const ALL: [Cat; 8] = [Cat::All, Cat::Browsers, Cat::Dev, Cat::Utilities, Cat::Chat, Cat::Media, Cat::Work, Cat::Games];

    pub fn label(self) -> &'static str {
        match self {
            Cat::All => t("Todas"),
            Cat::Browsers => t("Navegadores"),
            Cat::Dev => t("Desarrollo"),
            Cat::Utilities => t("Utilidades"),
            Cat::Chat => t("Comunicación"),
            Cat::Media => t("Multimedia"),
            Cat::Work => t("Productividad"),
            Cat::Games => t("Juegos"),
        }
    }
}

pub struct Entry {
    pub name: &'static str,
    pub cat: Cat,
    cask: &'static str,
    winget: &'static str,
    flatpak: &'static str,
}

const fn e(name: &'static str, cat: Cat, cask: &'static str, winget: &'static str, flatpak: &'static str) -> Entry {
    Entry { name, cat, cask, winget, flatpak }
}

use Cat::*;

pub const CATALOG: &[Entry] = &[
    // Browsers
    e("Google Chrome", Browsers, "google-chrome", "Google.Chrome", "com.google.Chrome"),
    e("Firefox", Browsers, "firefox", "Mozilla.Firefox", "org.mozilla.firefox"),
    e("Brave", Browsers, "brave-browser", "Brave.Brave", "com.brave.Browser"),
    e("Microsoft Edge", Browsers, "microsoft-edge", "Microsoft.Edge", "com.microsoft.Edge"),
    e("Opera", Browsers, "opera", "Opera.Opera", ""),
    e("Vivaldi", Browsers, "vivaldi", "Vivaldi.Vivaldi", "com.vivaldi.Vivaldi"),
    e("Tor Browser", Browsers, "tor-browser", "TorProject.TorBrowser", "org.torproject.torbrowser-launcher"),
    e("Chromium", Browsers, "chromium", "", "org.chromium.Chromium"),
    e("LibreWolf", Browsers, "librewolf", "LibreWolf.LibreWolf", "io.gitlab.librewolf-community"),
    e("Arc", Browsers, "arc", "TheBrowserCompany.Arc", ""),
    // Development
    e("Visual Studio Code", Dev, "visual-studio-code", "Microsoft.VisualStudioCode", "com.visualstudio.code"),
    e("Cursor", Dev, "cursor", "Anysphere.Cursor", ""),
    e("Zed", Dev, "zed", "ZedIndustries.Zed", "dev.zed.Zed"),
    e("Sublime Text", Dev, "sublime-text", "SublimeHQ.SublimeText.4", "com.sublimetext.three"),
    e("IntelliJ IDEA Community", Dev, "intellij-idea-ce", "JetBrains.IntelliJIDEA.Community", "com.jetbrains.IntelliJ-IDEA-Community"),
    e("PyCharm Community", Dev, "pycharm-ce", "JetBrains.PyCharm.Community", "com.jetbrains.PyCharm-Community"),
    e("Android Studio", Dev, "android-studio", "Google.AndroidStudio", "com.google.AndroidStudio"),
    e("JetBrains Toolbox", Dev, "jetbrains-toolbox", "JetBrains.Toolbox", ""),
    e("Docker Desktop", Dev, "docker-desktop", "Docker.DockerDesktop", ""),
    e("GitHub Desktop", Dev, "github", "GitHub.GitHubDesktop", "io.github.shiftey.Desktop"),
    e("Postman", Dev, "postman", "Postman.Postman", "com.getpostman.Postman"),
    e("Insomnia", Dev, "insomnia", "Insomnia.Insomnia", "rest.insomnia.Insomnia"),
    e("DBeaver", Dev, "dbeaver-community", "DBeaver.DBeaver.Community", "io.dbeaver.DBeaverCommunity"),
    e("TablePlus", Dev, "tableplus", "TablePlus.TablePlus", ""),
    e("iTerm2", Dev, "iterm2", "", ""),
    e("Warp", Dev, "warp", "Warp.Warp", ""),
    e("Ghostty", Dev, "ghostty", "", ""),
    e("Windows Terminal", Dev, "", "Microsoft.WindowsTerminal", ""),
    e("Notepad++", Dev, "", "Notepad++.Notepad++", ""),
    e("Git", Dev, "", "Git.Git", ""),
    e("Node.js LTS", Dev, "", "OpenJS.NodeJS.LTS", ""),
    e("Python 3", Dev, "", "Python.Python.3.12", ""),
    e("Go", Dev, "", "GoLang.Go", ""),
    e("Rustup", Dev, "", "Rustlang.Rustup", ""),
    e("Wireshark", Dev, "wireshark-app", "WiresharkFoundation.Wireshark", "org.wireshark.Wireshark"),
    e("VirtualBox", Dev, "virtualbox", "Oracle.VirtualBox", ""),
    e("UTM", Dev, "utm", "", ""),
    e("Fork", Dev, "fork", "Fork.Fork", ""),
    // Utilities
    e("Raycast", Utilities, "raycast", "", ""),
    e("Rectangle", Utilities, "rectangle", "", ""),
    e("AltTab", Utilities, "alt-tab", "", ""),
    e("Stats", Utilities, "stats", "", ""),
    e("AppCleaner", Utilities, "appcleaner", "", ""),
    e("The Unarchiver", Utilities, "the-unarchiver", "", ""),
    e("Keka", Utilities, "keka", "", ""),
    e("Maccy", Utilities, "maccy", "", ""),
    e("BetterDisplay", Utilities, "betterdisplay", "", ""),
    e("Balena Etcher", Utilities, "balenaetcher", "Balena.Etcher", ""),
    e("7-Zip", Utilities, "", "7zip.7zip", ""),
    e("PowerToys", Utilities, "", "Microsoft.PowerToys", ""),
    e("Everything", Utilities, "", "voidtools.Everything", ""),
    e("ShareX", Utilities, "", "ShareX.ShareX", ""),
    e("WinSCP", Utilities, "", "WinSCP.WinSCP", ""),
    e("PuTTY", Utilities, "", "PuTTY.PuTTY", ""),
    e("Flatseal", Utilities, "", "", "com.github.tchx84.Flatseal"),
    e("Bottles", Utilities, "", "", "com.usebottles.bottles"),
    e("RustDesk", Utilities, "rustdesk", "", "com.rustdesk.RustDesk"),
    e("Tailscale", Utilities, "tailscale-app", "Tailscale.Tailscale", ""),
    e("qBittorrent", Utilities, "qbittorrent", "qBittorrent.qBittorrent", "org.qbittorrent.qBittorrent"),
    e("Bitwarden", Utilities, "bitwarden", "Bitwarden.Bitwarden", "com.bitwarden.desktop"),
    e("1Password", Utilities, "1password", "AgileBits.1Password", ""),
    e("KeePassXC", Utilities, "keepassxc", "KeePassXCTeam.KeePassXC", "org.keepassxc.KeePassXC"),
    // Communication
    e("Slack", Chat, "slack", "SlackTechnologies.Slack", "com.slack.Slack"),
    e("Discord", Chat, "discord", "Discord.Discord", "com.discordapp.Discord"),
    e("Zoom", Chat, "zoom", "Zoom.Zoom", "us.zoom.Zoom"),
    e("Telegram", Chat, "telegram", "Telegram.TelegramDesktop", "org.telegram.desktop"),
    e("WhatsApp", Chat, "whatsapp", "", ""),
    e("Signal", Chat, "signal", "OpenWhisperSystems.Signal", "org.signal.Signal"),
    e("Microsoft Teams", Chat, "microsoft-teams", "Microsoft.Teams", ""),
    e("Thunderbird", Chat, "thunderbird", "Mozilla.Thunderbird", "org.mozilla.Thunderbird"),
    e("Element", Chat, "element", "Element.Element", "im.riot.Riot"),
    // Media
    e("Spotify", Media, "spotify", "Spotify.Spotify", "com.spotify.Client"),
    e("VLC", Media, "vlc", "VideoLAN.VLC", "org.videolan.VLC"),
    e("IINA", Media, "iina", "", ""),
    e("OBS Studio", Media, "obs", "OBSProject.OBSStudio", "com.obsproject.Studio"),
    e("HandBrake", Media, "handbrake-app", "HandBrake.HandBrake", "fr.handbrake.ghb"),
    e("Audacity", Media, "audacity", "Audacity.Audacity", "org.audacityteam.Audacity"),
    e("GIMP", Media, "gimp", "GIMP.GIMP", "org.gimp.GIMP"),
    e("Inkscape", Media, "inkscape", "Inkscape.Inkscape", "org.inkscape.Inkscape"),
    e("Blender", Media, "blender", "BlenderFoundation.Blender", "org.blender.Blender"),
    e("Krita", Media, "krita", "KDE.Krita", "org.kde.krita"),
    e("Kdenlive", Media, "kdenlive", "KDE.Kdenlive", "org.kde.kdenlive"),
    e("Figma", Media, "figma", "Figma.Figma", ""),
    // Productivity
    e("Notion", Work, "notion", "Notion.Notion", ""),
    e("Obsidian", Work, "obsidian", "Obsidian.Obsidian", "md.obsidian.Obsidian"),
    e("LibreOffice", Work, "libreoffice", "TheDocumentFoundation.LibreOffice", "org.libreoffice.LibreOffice"),
    e("OnlyOffice", Work, "onlyoffice", "ONLYOFFICE.DesktopEditors", "org.onlyoffice.desktopeditors"),
    e("Logseq", Work, "logseq", "Logseq.Logseq", "com.logseq.Logseq"),
    e("Joplin", Work, "joplin", "Joplin.Joplin", "net.cozic.joplin_desktop"),
    e("Anki", Work, "anki", "Anki.Anki", "net.ankiweb.Anki"),
    e("calibre", Work, "calibre", "calibre.calibre", "com.calibre_ebook.calibre"),
    e("Dropbox", Work, "dropbox", "Dropbox.Dropbox", "com.dropbox.Client"),
    e("Google Drive", Work, "google-drive", "Google.GoogleDrive", ""),
    // Games
    e("Steam", Games, "steam", "Valve.Steam", "com.valvesoftware.Steam"),
    e("Epic Games Launcher", Games, "epic-games", "EpicGames.EpicGamesLauncher", ""),
    e("Prism Launcher", Games, "prismlauncher", "PrismLauncher.PrismLauncher", "org.prismlauncher.PrismLauncher"),
    e("Heroic Games Launcher", Games, "heroic", "HeroicGamesLauncher.HeroicGamesLauncher", "com.heroicgameslauncher.hgl"),
    e("Lutris", Games, "", "", "net.lutris.Lutris"),
];

impl Entry {
    /// The package id on this OS, in the `kind:name` form the rest of the app uses.
    pub fn id(&self) -> Option<String> {
        let (prefix, id) = if cfg!(target_os = "macos") {
            ("cask", self.cask)
        } else if cfg!(windows) {
            ("winget", self.winget)
        } else {
            ("flatpak", self.flatpak)
        };
        (!id.is_empty()).then(|| format!("{prefix}:{id}"))
    }

    pub fn app(&self) -> Option<App> {
        let id = self.id()?;
        Some(App {
            hue: scan::hue(self.name),
            name: self.name.into(),
            vendor: self.cat.label().into(),
            from: String::new(),
            to: String::new(),
            source: Source::from_id(&id),
            id,
            size_mb: None,
            bundle: None,
            installed_at: None,
        })
    }
}

/// Catalog apps offered on this OS.
pub fn for_this_os() -> Vec<App> {
    CATALOG.iter().filter_map(Entry::app).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_safe_and_names_unique() {
        let mut names = HashSet::new();
        for e in CATALOG {
            assert!(names.insert(e.name), "duplicate {}", e.name);
            for id in [e.cask, e.winget, e.flatpak].into_iter().filter(|i| !i.is_empty()) {
                assert!(crate::winpkg::safe_name(id), "{id}");
            }
        }
    }

    #[test]
    fn every_os_has_a_decent_catalog() {
        for (name, n) in [("cask", CATALOG.iter().filter(|e| !e.cask.is_empty()).count()), ("winget", CATALOG.iter().filter(|e| !e.winget.is_empty()).count()), ("flatpak", CATALOG.iter().filter(|e| !e.flatpak.is_empty()).count())] {
            assert!(n >= 50, "{name}: {n}");
        }
    }
}
