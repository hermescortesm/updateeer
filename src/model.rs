use crate::i18n::t;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    AppStore,
    Homebrew,
    Direct,
    Winget,
    Scoop,
    Chocolatey,
    Apt,
    Pacman,
    Aur,
    Dnf,
    Zypper,
    Apk,
    Xbps,
    Flatpak,
    Snap,
}

impl Source {
    /// Source implied by an app id prefix (`mas:`, `winget:`…); Homebrew otherwise.
    pub fn from_id(id: &str) -> Self {
        match id.split_once(':').map_or("", |(k, _)| k) {
            "mas" => Source::AppStore,
            "direct" => Source::Direct,
            "winget" => Source::Winget,
            "scoop" => Source::Scoop,
            "choco" => Source::Chocolatey,
            "apt" => Source::Apt,
            "pacman" => Source::Pacman,
            "aur" => Source::Aur,
            "dnf" => Source::Dnf,
            "zypper" => Source::Zypper,
            "apk" => Source::Apk,
            "xbps" => Source::Xbps,
            "flatpak" => Source::Flatpak,
            "snap" => Source::Snap,
            _ => Source::Homebrew,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::AppStore => "App Store",
            Source::Homebrew => "Homebrew",
            Source::Direct => t("Directa"),
            Source::Winget => "winget",
            Source::Scoop => "Scoop",
            Source::Chocolatey => "Chocolatey",
            Source::Apt => "apt",
            Source::Pacman => "pacman",
            Source::Aur => "AUR",
            Source::Dnf => "dnf",
            Source::Zypper => "zypper",
            Source::Apk => "apk",
            Source::Xbps => "xbps",
            Source::Flatpak => "Flatpak",
            Source::Snap => "Snap",
        }
    }
}

#[derive(Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub from: String,
    pub to: String,
    pub source: Source,
    pub size_mb: Option<u32>,
    /// Hue (degrees) for the placeholder icon tile.
    pub hue: f32,
    /// `Name.app` as declared by a cask, when known; used to find the real icon.
    pub bundle: Option<String>,
    /// When the installed version landed on this machine, when it can be told.
    pub installed_at: Option<SystemTime>,
}

pub fn format_size(mb: Option<u32>) -> String {
    let Some(mb) = mb else { return "—".into() };
    if mb >= 1000 {
        format!("{:.1} GB", mb as f32 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

/// `12 oct 2026`, in UTC (no timezone database: day precision is enough here).
pub fn date(t: SystemTime) -> String {
    let days = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    match crate::i18n::get() {
        crate::i18n::Lang::Ja | crate::i18n::Lang::Zh => format!("{year}年{month}月{day}日"),
        _ => format!("{day} {} {year}", crate::i18n::months()[month as usize - 1]),
    }
}

/// `hoy`, `hace 3 d`, `hace 2 sem`, `hace 5 m`, `hace 1 año`.
pub fn ago(t: SystemTime, now: SystemTime) -> String {
    let days = now.duration_since(t).unwrap_or_default().as_secs() / 86_400;
    match days {
        0 => crate::i18n::t("hoy").into(),
        1..=6 => crate::i18n::tf1("hace {0} d", &days.to_string()),
        7..=29 => crate::i18n::tf1("hace {0} sem", &(days / 7).to_string()),
        30..=364 => crate::i18n::tf1("hace {0} m", &(days / 30).to_string()),
        _ => crate::i18n::tf1("hace {0} a", &(days / 365).to_string()),
    }
}

/// The same without the "ago" wording, for narrow columns: `3 d`, `2 sem`.
pub fn ago_short(t: SystemTime, now: SystemTime) -> String {
    let days = now.duration_since(t).unwrap_or_default().as_secs() / 86_400;
    let tf1 = crate::i18n::tf1;
    match days {
        0 => crate::i18n::t("hoy").into(),
        1..=6 => tf1("{0} d", &days.to_string()),
        7..=29 => tf1("{0} sem", &(days / 7).to_string()),
        30..=364 => tf1("{0} m", &(days / 30).to_string()),
        _ => tf1("{0} a", &(days / 365).to_string()),
    }
}

pub fn days_ago(t: SystemTime, now: SystemTime) -> u64 {
    now.duration_since(t).unwrap_or(Duration::ZERO).as_secs() / 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        let at = |d: u64| UNIX_EPOCH + Duration::from_secs(d * 86_400);
        assert_eq!(date(at(0)), "1 ene 1970");
        assert_eq!(date(at(20_743)), "17 oct 2026");
        assert_eq!(date(at(19_782)), "29 feb 2024");
        assert_eq!(ago(at(100), at(100)), t("hoy"));
        assert_eq!(ago(at(100), at(115)), "hace 2 sem");
        assert_eq!(ago(at(0), at(400)), "hace 1 a");
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(Some(138)), "138 MB");
        assert_eq!(format_size(Some(612)), "612 MB");
        assert_eq!(format_size(Some(2048)), "2.0 GB");
        assert_eq!(format_size(None), "—");
    }
}
