//! Finished upgrades, kept in `~/.config/updateeer/history.json` so they survive restarts.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::model::{App, Source};
use crate::{scan, settings};

const MAX_ENTRIES: usize = 500;

pub struct Entry {
    pub app: App,
    pub error: Option<String>,
    pub at: SystemTime,
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn load() -> Vec<Entry> {
    let text = settings::config_dir().and_then(|d| std::fs::read_to_string(d.join("history.json")).ok());
    text.map(|t| parse(&t)).unwrap_or_default()
}

fn parse(text: &str) -> Vec<Entry> {
    let v: Value = serde_json::from_str(text).unwrap_or_default();
    let text = |e: &Value, k: &str| e[k].as_str().map(scan::clean).unwrap_or_default();
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let (id, name) = (text(e, "id"), text(e, "name"));
            if id.is_empty() || name.is_empty() {
                return None;
            }
            let source = Source::from_id(&id);
            let app = App {
                hue: scan::hue(&name),
                vendor: text(e, "vendor"),
                from: text(e, "from"),
                to: text(e, "to"),
                id,
                name,
                source,
                size_mb: None,
                bundle: None,
                installed_at: None,
            };
            Some(Entry { app, error: e["error"].as_str().map(String::from), at: UNIX_EPOCH + Duration::from_secs(e["at"].as_u64()?) })
        })
        .collect()
}

/// Best effort, keeps the newest `MAX_ENTRIES`.
pub fn save(entries: &[Entry]) {
    let Some(dir) = settings::config_dir() else { return };
    let skip = entries.len().saturating_sub(MAX_ENTRIES);
    let items: Vec<Value> = entries[skip..]
        .iter()
        .map(|e| json!({ "id": e.app.id, "name": e.app.name, "vendor": e.app.vendor, "from": e.app.from, "to": e.app.to, "at": secs(e.at), "error": e.error }))
        .collect();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("history.json"), Value::Array(items).to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_input() {
        let json = r#"[{"id":"cask:warp","name":"Warp","vendor":"warp.dev","from":"1","to":"2","at":1760000000,"error":null},
                       {"id":"","name":"x","at":1},{"name":"no id","at":1},{"id":"a","name":"b"}]"#;
        let entries = parse(json);
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].app.name.as_str(), entries[0].error.clone()), ("Warp", None));
        assert!(parse("garbage").is_empty());
    }
}
