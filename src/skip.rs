//! Updates the user chose to skip: app id -> the new version that is skipped. The app comes back
//! when a newer version than that one is published. Kept in `~/.config/updateeer/skipped.json`.
use std::collections::HashMap;

use serde_json::Value;

use crate::settings;

pub fn load() -> HashMap<String, String> {
    settings::config_dir().and_then(|d| std::fs::read_to_string(d.join("skipped.json")).ok()).map(|t| parse(&t)).unwrap_or_default()
}

fn parse(text: &str) -> HashMap<String, String> {
    let v: Value = serde_json::from_str(text).unwrap_or_default();
    v.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()
}

/// Best effort, like the settings.
pub fn save(skipped: &HashMap<String, String>) {
    let Some(dir) = settings::config_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("skipped.json"), serde_json::to_string(skipped).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_string_versions() {
        let m = parse(r#"{"cask:a":"1.2","cask:b":3}"#);
        assert_eq!(m.len(), 1);
        assert_eq!(m["cask:a"], "1.2");
        assert!(parse("garbage").is_empty());
    }
}
