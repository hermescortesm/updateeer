//! Runs the actual update for an app id (`cask:<token>`, `formula:<name>` or `mas:<number>`).
use crate::{linuxpkg, mas, winpkg};
use crate::scan::{brew, safe_token, UPGRADE_TIMEOUT};
use std::sync::Mutex;

/// Package managers lock their database: installs from "Discover" go one at a time.
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

fn args(id: &str) -> Option<[&str; 3]> {
    let (kind, name) = id.split_once(':')?;
    if !safe_token(name) {
        return None;
    }
    match kind {
        "cask" => Some(["upgrade", "--cask", name]),
        "formula" => Some(["upgrade", "--formula", name]),
        _ => None,
    }
}

/// Blocks until brew finishes; call it from a worker thread.
pub fn run(id: &str) -> Result<(), String> {
    if let Some(app_id) = id.strip_prefix("mas:") {
        return mas::upgrade(app_id);
    }
    if let Some((kind @ ("winget" | "scoop" | "choco"), name)) = id.split_once(':') {
        return winpkg::upgrade(kind, name);
    }
    if let Some((kind @ ("apt" | "pacman" | "aur" | "dnf" | "flatpak" | "snap"), name)) = id.split_once(':') {
        return linuxpkg::upgrade(kind, name);
    }
    let args = args(id).ok_or_else(|| format!("id no soportado: {id}"))?;
    brew(&args, UPGRADE_TIMEOUT, false).map(|_| ())
}

/// Installs a new app (`cask:`, `formula:`, `winget:`, `flatpak:` or `apt:` id). Blocks.
pub fn install(id: &str) -> Result<(), String> {
    let _one_at_a_time = INSTALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (kind, name) = id.split_once(':').ok_or_else(|| format!("id no soportado: {id}"))?;
    match kind {
        "cask" | "formula" if safe_token(name) => brew(&["install", if kind == "cask" { "--cask" } else { "--formula" }, name], UPGRADE_TIMEOUT, false).map(|_| ()),
        "winget" => winpkg::install(name),
        "flatpak" | "apt" => linuxpkg::install(kind, name),
        _ => Err(format!("id no soportado: {id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_args_and_rejects_bad_ids() {
        assert_eq!(args("cask:firefox"), Some(["upgrade", "--cask", "firefox"]));
        assert_eq!(args("formula:node@22"), Some(["upgrade", "--formula", "node@22"]));
        assert_eq!(args("formula:user/tap/engram"), Some(["upgrade", "--formula", "user/tap/engram"]));
        assert_eq!(args("cask:--force"), None);
        assert_eq!(args("cask:a b"), None);
        assert_eq!(args("mas:123"), None);
    }

    #[test]
    fn install_rejects_bad_ids() {
        assert!(install("cask:--force").is_err());
        assert!(install("winget:--all").is_err());
        assert!(install("nonsense").is_err());
    }
}
