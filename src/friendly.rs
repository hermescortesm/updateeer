//! Readable names and short Spanish descriptions for Linux packages (`brave-bin` -> "Brave",
//! "Navegador de internet"). Pure text handling, so it is tested on every OS.

/// `stem` -> (name, description) for packages whose name is not obvious.
const KNOWN: &[(&str, &str, &str)] = &[
    ("brave", "Brave", "Navegador de internet"),
    ("google-chrome", "Google Chrome", "Navegador de internet"),
    ("chromium", "Chromium", "Navegador de internet"),
    ("firefox", "Firefox", "Navegador de internet"),
    ("firefox-esr", "Firefox ESR", "Navegador de internet"),
    ("librewolf", "LibreWolf", "Navegador de internet"),
    ("vivaldi", "Vivaldi", "Navegador de internet"),
    ("opera", "Opera", "Navegador de internet"),
    ("microsoft-edge", "Microsoft Edge", "Navegador de internet"),
    ("zen", "Zen", "Navegador de internet"),
    ("thunderbird", "Thunderbird", "Correo electrónico"),
    ("discord", "Discord", "Chat y voz"),
    ("telegram", "Telegram", "Mensajería"),
    ("signal", "Signal", "Mensajería privada"),
    ("slack", "Slack", "Mensajería de trabajo"),
    ("zoom", "Zoom", "Videollamadas"),
    ("skypeforlinux", "Skype", "Videollamadas"),
    ("spotify", "Spotify", "Música"),
    ("vlc", "VLC", "Reproductor multimedia"),
    ("mpv", "mpv", "Reproductor de video"),
    ("obs-studio", "OBS Studio", "Grabación y transmisión"),
    ("audacity", "Audacity", "Editor de audio"),
    ("gimp", "GIMP", "Editor de imágenes"),
    ("inkscape", "Inkscape", "Dibujo vectorial"),
    ("krita", "Krita", "Pintura digital"),
    ("blender", "Blender", "Modelado y animación 3D"),
    ("kdenlive", "Kdenlive", "Editor de video"),
    ("handbrake", "HandBrake", "Convertidor de video"),
    ("visual-studio-code", "Visual Studio Code", "Editor de código"),
    ("code", "Visual Studio Code", "Editor de código"),
    ("vscodium", "VSCodium", "Editor de código"),
    ("sublime-text", "Sublime Text", "Editor de código"),
    ("neovim", "Neovim", "Editor de texto"),
    ("vim", "Vim", "Editor de texto"),
    ("emacs", "Emacs", "Editor de texto"),
    ("git", "Git", "Control de versiones"),
    ("github-cli", "GitHub CLI", "GitHub en la terminal"),
    ("docker", "Docker", "Contenedores"),
    ("docker-compose", "Docker Compose", "Contenedores"),
    ("podman", "Podman", "Contenedores"),
    ("nodejs", "Node.js", "Entorno de JavaScript"),
    ("npm", "npm", "Paquetes de JavaScript"),
    ("python", "Python", "Lenguaje de programación"),
    ("rust", "Rust", "Lenguaje de programación"),
    ("go", "Go", "Lenguaje de programación"),
    ("jdk-openjdk", "Java (OpenJDK)", "Entorno de Java"),
    ("curl", "curl", "Descargas desde la terminal"),
    ("wget", "wget", "Descargas desde la terminal"),
    ("htop", "htop", "Monitor de procesos"),
    ("btop", "btop", "Monitor del sistema"),
    ("ripgrep", "ripgrep", "Búsqueda en archivos"),
    ("fzf", "fzf", "Búsqueda interactiva"),
    ("tmux", "tmux", "Terminales múltiples"),
    ("zsh", "Zsh", "Terminal"),
    ("fish", "Fish", "Terminal"),
    ("alacritty", "Alacritty", "Terminal"),
    ("kitty", "kitty", "Terminal"),
    ("wezterm", "WezTerm", "Terminal"),
    ("libreoffice-fresh", "LibreOffice", "Suite ofimática"),
    ("libreoffice-still", "LibreOffice", "Suite ofimática"),
    ("libreoffice", "LibreOffice", "Suite ofimática"),
    ("onlyoffice", "ONLYOFFICE", "Suite ofimática"),
    ("obsidian", "Obsidian", "Notas"),
    ("steam", "Steam", "Tienda de juegos"),
    ("lutris", "Lutris", "Biblioteca de juegos"),
    ("heroic-games-launcher", "Heroic", "Biblioteca de juegos"),
    ("keepassxc", "KeePassXC", "Gestor de contraseñas"),
    ("bitwarden", "Bitwarden", "Gestor de contraseñas"),
    ("1password", "1Password", "Gestor de contraseñas"),
    ("transmission-gtk", "Transmission", "Cliente de torrents"),
    ("transmission-qt", "Transmission", "Cliente de torrents"),
    ("qbittorrent", "qBittorrent", "Cliente de torrents"),
    ("virtualbox", "VirtualBox", "Máquinas virtuales"),
    ("anydesk", "AnyDesk", "Escritorio remoto"),
    ("teamviewer", "TeamViewer", "Escritorio remoto"),
    ("dropbox", "Dropbox", "Archivos en la nube"),
    ("nextcloud-client", "Nextcloud", "Archivos en la nube"),
    ("syncthing", "Syncthing", "Sincronización de archivos"),
    ("postman", "Postman", "Pruebas de API"),
    ("dbeaver", "DBeaver", "Cliente de bases de datos"),
    ("filezilla", "FileZilla", "Cliente FTP"),
    ("timeshift", "Timeshift", "Copias de seguridad del sistema"),
    ("yay", "Yay", "Ayudante de AUR"),
    ("paru", "Paru", "Ayudante de AUR"),
    ("flatpak", "Flatpak", "Aplicaciones en contenedor"),
];

/// Specific freedesktop categories first, general ones last: the first one present wins.
const CATEGORIES: &[(&str, &str)] = &[
    ("WebBrowser", "Navegador de internet"),
    ("Email", "Correo electrónico"),
    ("VideoConference", "Videollamadas"),
    ("InstantMessaging", "Mensajería"),
    ("Chat", "Mensajería"),
    ("IRCClient", "Chat IRC"),
    ("P2P", "Cliente de torrents"),
    ("FileTransfer", "Transferencia de archivos"),
    ("TerminalEmulator", "Terminal"),
    ("IDE", "Entorno de desarrollo"),
    ("TextEditor", "Editor de texto"),
    ("Debugger", "Herramienta de desarrollo"),
    ("Development", "Herramienta de desarrollo"),
    ("AudioVideoEditing", "Edición de audio y video"),
    ("Recorder", "Grabador"),
    ("Player", "Reproductor multimedia"),
    ("Photography", "Fotografía"),
    ("3DGraphics", "Gráficos 3D"),
    ("VectorGraphics", "Dibujo vectorial"),
    ("RasterGraphics", "Editor de imágenes"),
    ("Viewer", "Visor"),
    ("Graphics", "Gráficos"),
    ("WordProcessor", "Procesador de texto"),
    ("Spreadsheet", "Hojas de cálculo"),
    ("Presentation", "Presentaciones"),
    ("Calculator", "Calculadora"),
    ("Calendar", "Calendario"),
    ("Archiving", "Compresión de archivos"),
    ("FileManager", "Administrador de archivos"),
    ("Security", "Seguridad"),
    ("PackageManager", "Gestor de paquetes"),
    ("Monitor", "Monitor del sistema"),
    ("Emulator", "Emulador"),
    ("Virtualization", "Máquinas virtuales"),
    ("Settings", "Configuración"),
    ("Game", "Juego"),
    ("Education", "Educación"),
    ("Science", "Ciencia"),
    ("Office", "Ofimática"),
    ("Audio", "Audio"),
    ("Video", "Video"),
    ("AudioVideo", "Multimedia"),
    ("Network", "Red"),
    ("System", "Sistema"),
    ("Utility", "Utilidad"),
];

/// Package name without packaging noise: `google-chrome-stable` -> `google-chrome`.
fn stem(id: &str) -> String {
    let mut s = id.to_lowercase();
    loop {
        let cut = ["-bin", "-git", "-stable", "-beta", "-nightly", "-appimage", "-desktop", "-browser"].iter().find_map(|x| s.strip_suffix(x).filter(|r| !r.is_empty()).map(str::to_string));
        match cut {
            Some(r) => s = r,
            None => return s,
        }
    }
}

/// Curated `(name, description)` for a package name or a reverse-DNS id (`com.brave.Browser`).
pub fn known(id: &str) -> Option<(&'static str, &'static str)> {
    let mut keys = vec![stem(id)];
    if id.matches('.').count() >= 2 {
        keys.extend(id.split('.').skip(1).map(stem));
    }
    keys.iter().find_map(|k| KNOWN.iter().find(|(s, _, _)| s == k)).map(|&(_, n, d)| (n, d))
}

/// Spanish description for a `.desktop` `Categories=` value.
pub fn describe(categories: &str) -> Option<&'static str> {
    let have: Vec<&str> = categories.split(';').map(str::trim).collect();
    CATEGORIES.iter().find(|(c, _)| have.contains(c)).map(|&(_, d)| d)
}

/// Name from a package id when nothing better is known: `linux-firmware` -> "Linux Firmware".
pub fn display(id: &str) -> String {
    stem(id)
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().next().map_or(String::new(), |c| c.to_uppercase().chain(w.chars().skip(1)).collect()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A launcher's name without "Web Browser"-style tails: "Brave Web Browser" -> "Brave".
pub fn tidy(name: &str) -> String {
    let n = name.trim();
    [" Web Browser", " Browser"].iter().find_map(|t| n.strip_suffix(t).filter(|r| !r.is_empty())).unwrap_or(n).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_descriptions() {
        assert_eq!(known("brave-bin"), Some(("Brave", "Navegador de internet")));
        assert_eq!(known("google-chrome-stable"), Some(("Google Chrome", "Navegador de internet")));
        assert_eq!(known("com.brave.Browser"), Some(("Brave", "Navegador de internet")));
        assert_eq!(known("zzz-unknown"), None);
        assert_eq!(display("linux-firmware"), "Linux Firmware");
        assert_eq!(display("some-tool-git"), "Some Tool");
        assert_eq!(tidy("Brave Web Browser"), "Brave");
        assert_eq!(tidy("Files"), "Files");
        assert_eq!(describe("Network;WebBrowser;"), Some("Navegador de internet"));
        assert_eq!(describe("Utility;TerminalEmulator;System;"), Some("Terminal"));
        assert_eq!(describe("X-Foo;"), None);
    }
}
