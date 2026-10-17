//! Finds outdated apps. Today: Homebrew casks and the formulae the user installed on purpose.
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::model::{App, Source};

const BREW_PATHS: [&str; 2] = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];

fn brew(args: &[&str]) -> Result<String, String> {
    // Absolute path, no shell: nothing from PATH or user text gets interpreted.
    let bin = BREW_PATHS
        .iter()
        .find(|p| Path::new(p).exists())
        .ok_or("Homebrew no está instalado")?;
    let out = Command::new(bin)
        .args(args)
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

pub fn scan() -> Result<Vec<App>, String> {
    let outdated = brew(&["outdated", "--greedy", "--json=v2"])?;
    let leaves = brew(&["leaves"])?;
    let leaves: HashSet<&str> = leaves.lines().collect();
    let mut apps = parse_outdated(&outdated, &leaves)?;

    let tokens: Vec<String> = apps.iter().filter_map(|a| a.id.strip_prefix("cask:")).filter(|t| safe_token(t)).map(String::from).collect();
    if !tokens.is_empty() {
        let mut args = vec!["info", "--cask", "--json=v2"];
        args.extend(tokens.iter().map(String::as_str));
        apply_cask_info(&mut apps, &brew(&args)?);
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok(apps)
}

/// Brew names are `[a-z0-9@._+-]`; reject anything that could pass as a flag.
fn safe_token(t: &str) -> bool {
    !t.starts_with('-') && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
}

fn first_version(v: &Value) -> String {
    match v {
        // Casks append build ids after a comma ("3.21.13,<hash>"); show only the version.
        Value::String(s) => s.split(',').next().unwrap_or_default().to_string(),
        Value::Array(a) => a.first().map(first_version).unwrap_or_default(),
        _ => String::new(),
    }
}

fn hue(name: &str) -> f32 {
    (name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 360) as f32
}

fn app(id: String, name: &str, vendor: &str, from: String, to: String) -> App {
    App { id, name: name.into(), vendor: vendor.into(), from, to, source: Source::Homebrew, size_mb: None, hue: hue(name) }
}

fn parse_outdated(json: &str, leaves: &HashSet<&str>) -> Result<Vec<App>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut apps = Vec::new();
    for f in v["formulae"].as_array().into_iter().flatten() {
        let Some(name) = f["name"].as_str().filter(|n| safe_token(n) && leaves.contains(n)) else { continue };
        apps.push(app(format!("formula:{name}"), name, "Homebrew", first_version(&f["installed_versions"]), first_version(&f["current_version"])));
    }
    for c in v["casks"].as_array().into_iter().flatten() {
        let Some(name) = c["name"].as_str().filter(|n| safe_token(n)) else { continue };
        apps.push(app(format!("cask:{name}"), name, "Homebrew", first_version(&c["installed_versions"]), first_version(&c["current_version"])));
    }
    // A change only in the build id looks like "5.3.7 -> 5.3.7": nothing to show.
    apps.retain(|a| a.from != a.to);
    Ok(apps)
}

/// Replaces the cask token with its display name and homepage domain.
fn apply_cask_info(apps: &mut [App], json: &str) {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return };
    for c in v["casks"].as_array().into_iter().flatten() {
        let (Some(token), Some(name)) = (c["token"].as_str(), c["name"][0].as_str()) else { continue };
        if let Some(a) = apps.iter_mut().find(|a| a.id.strip_prefix("cask:") == Some(token)) {
            a.name = name.into();
            a.hue = hue(name);
            if let Some(host) = c["homepage"].as_str().and_then(domain) {
                a.vendor = host;
            }
        }
    }
}

fn domain(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split('/').next()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTDATED: &str = r#"{"formulae":[
        {"name":"node","installed_versions":["20.17.0"],"current_version":"22.9.0"},
        {"name":"aom","installed_versions":["3.15.1"],"current_version":"3.15.2"}],
      "casks":[{"name":"visual-studio-code","installed_versions":"1.93.1","current_version":"1.94.2"},
               {"name":"--evil","installed_versions":"1","current_version":"2"}]}"#;

    #[test]
    fn parses_leaves_and_casks_and_drops_unsafe() {
        let leaves = HashSet::from(["node"]);
        let apps = parse_outdated(OUTDATED, &leaves).unwrap();
        let ids: Vec<_> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["formula:node", "cask:visual-studio-code"]);
        assert_eq!((apps[1].from.as_str(), apps[1].to.as_str()), ("1.93.1", "1.94.2"));
    }

    #[test]
    fn cask_info_sets_display_name_and_vendor() {
        let mut apps = parse_outdated(OUTDATED, &HashSet::new()).unwrap();
        let info = r#"{"casks":[{"token":"visual-studio-code","name":["Visual Studio Code"],"homepage":"https://www.code.visualstudio.com/"}]}"#;
        apply_cask_info(&mut apps, info);
        assert_eq!(apps[0].name, "Visual Studio Code");
        assert_eq!(apps[0].vendor, "code.visualstudio.com");
    }

    /// Manual check against the real Homebrew: `cargo test live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live() {
        for a in scan().unwrap() {
            println!("{:<28} {:<24} {} -> {}", a.name, a.vendor, a.from, a.to);
        }
    }
}
