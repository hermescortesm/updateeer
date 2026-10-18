//! User settings, kept in `~/.config/updateeer/settings.json`.
use std::path::PathBuf;

use serde_json::{Value, json};

pub const HOURS: [u32; 4] = [1, 6, 12, 24];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn from_name(s: &str) -> Self {
        match s {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub auto: bool,
    pub hours: u32,
    pub theme: ThemeMode,
    /// The user dismissed the "install Homebrew / mas" prompt.
    pub hide_setup: bool,
    /// Update apps that are not running on their own; ask about the ones that are open.
    pub auto_update: bool,
    /// The welcome screen (permissions) was seen and closed.
    pub onboarded: bool,
    /// Look for a newer Updateeer on GitHub with every search.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto: true, hours: 6, theme: ThemeMode::System, hide_setup: false, auto_update: false, onboarded: false, check_updates: true }
    }
}

pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Updateeer"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/updateeer"))
}

fn path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| Self::parse(&s)).unwrap_or_default()
    }

    fn parse(s: &str) -> Self {
        let v: Value = serde_json::from_str(s).unwrap_or_default();
        let d = Self::default();
        Self {
            auto: v["auto"].as_bool().unwrap_or(d.auto),
            auto_update: v["auto_update"].as_bool().unwrap_or(d.auto_update),
            check_updates: v["check_updates"].as_bool().unwrap_or(d.check_updates),
            onboarded: v["onboarded"].as_bool().unwrap_or(d.onboarded),
            hide_setup: v["hide_setup"].as_bool().unwrap_or(d.hide_setup),
            theme: v["theme"].as_str().map_or(d.theme, ThemeMode::from_name),
            hours: v["hours"].as_u64().map(|h| h as u32).filter(|h| HOURS.contains(h)).unwrap_or(d.hours),
        }
    }

    /// Best effort: a read-only home must not break the app.
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, json!({ "auto": self.auto, "hours": self.hours, "theme": self.theme.name(), "hide_setup": self.hide_setup, "auto_update": self.auto_update, "onboarded": self.onboarded, "check_updates": self.check_updates }).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_validates_and_defaults() {
        assert_eq!(Settings::parse(r#"{"auto":false,"hours":12,"theme":"light"}"#), Settings { auto: false, hours: 12, theme: ThemeMode::Light, hide_setup: false, auto_update: false, onboarded: false, check_updates: true });
        assert_eq!(Settings::parse(r#"{"hours":7}"#), Settings::default());
        assert_eq!(Settings::parse("garbage"), Settings::default());
    }
}
