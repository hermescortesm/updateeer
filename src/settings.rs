//! User settings, kept in `~/.config/updateeer/settings.json`.
use std::path::PathBuf;

use serde_json::{Value, json};

pub const HOURS: [u32; 4] = [1, 6, 12, 24];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub auto: bool,
    pub hours: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto: true, hours: 6 }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/updateeer/settings.json"))
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
            hours: v["hours"].as_u64().map(|h| h as u32).filter(|h| HOURS.contains(h)).unwrap_or(d.hours),
        }
    }

    /// Best effort: a read-only home must not break the app.
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, json!({ "auto": self.auto, "hours": self.hours }).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_validates_and_defaults() {
        assert_eq!(Settings::parse(r#"{"auto":false,"hours":12}"#), Settings { auto: false, hours: 12 });
        assert_eq!(Settings::parse(r#"{"hours":7}"#), Settings::default());
        assert_eq!(Settings::parse("garbage"), Settings::default());
    }
}
