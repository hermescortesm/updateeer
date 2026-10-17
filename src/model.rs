use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    AppStore,
    Homebrew,
    Direct,
    Winget,
    Scoop,
    Chocolatey,
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
            _ => Source::Homebrew,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::AppStore => "App Store",
            Source::Homebrew => "Homebrew",
            Source::Direct => "Directa",
            Source::Winget => "winget",
            Source::Scoop => "Scoop",
            Source::Chocolatey => "Chocolatey",
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
    const MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
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
    format!("{day} {} {year}", MONTHS[month as usize - 1])
}

/// `hoy`, `hace 3 d`, `hace 2 sem`, `hace 5 m`, `hace 1 año`.
pub fn ago(t: SystemTime, now: SystemTime) -> String {
    let days = now.duration_since(t).unwrap_or_default().as_secs() / 86_400;
    match days {
        0 => "hoy".into(),
        1..=6 => format!("hace {days} d"),
        7..=29 => format!("hace {} sem", days / 7),
        30..=364 => format!("hace {} m", days / 30),
        _ => format!("hace {} a", days / 365),
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
        assert_eq!(ago(at(100), at(100)), "hoy");
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
