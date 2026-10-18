//! Facts read from this machine: where an app lives and when its installed version arrived.
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::time::SystemTime;

use serde_json::Value;

use crate::model::{App, Source};
use crate::scan;

pub fn enrich(apps: &mut [App]) {
    for a in apps {
        a.installed_at = installed_at(a);
    }
}

/// `/opt/homebrew` or `/usr/local`.
fn prefix() -> Option<PathBuf> {
    Path::new(scan::brew_path()?).parent()?.parent().map(Path::to_path_buf)
}

/// Newest birth time among a directory's entries: one entry per installed version.
fn newest_birth(dir: &Path) -> Option<SystemTime> {
    fs::read_dir(dir).ok()?.flatten().filter_map(|e| e.metadata().ok()?.created().ok()).max()
}

fn installed_at(a: &App) -> Option<SystemTime> {
    let (kind, name) = a.id.split_once(':')?;
    let name = name.rsplit('/').next()?;
    match kind {
        // Linux: when the package files landed on disk.
        "apt" => ["", ":amd64", ":arm64", ":i386"].iter().find_map(|arch| fs::metadata(format!("/var/lib/dpkg/info/{name}{arch}.list")).ok()?.modified().ok()),
        "pacman" => fs::metadata(format!("/var/lib/pacman/local/{name}-{}", a.from)).ok()?.modified().ok(),
        "flatpak" => {
            let user = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/flatpak/app").join(name));
            [Some(PathBuf::from("/var/lib/flatpak/app").join(name)), user].into_iter().flatten().find_map(|p| fs::metadata(p).ok()?.modified().ok())
        }
        "cask" => newest_birth(&prefix()?.join("Caskroom").join(name)),
        "formula" => newest_birth(&prefix()?.join("Cellar").join(name)),
        // The App Store replaces the bundle on update, so its modification time is the update.
        "mas" | "direct" => fs::metadata(bundle(a)?).ok()?.modified().ok(),
        _ => None,
    }
}

fn app_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Applications"));
    [Some(PathBuf::from("/Applications")), home, Some(PathBuf::from("/Applications/Utilities"))].into_iter().flatten().collect()
}

/// Apps sitting in the usual folders that no package manager reported: direct downloads.
/// `known` are the apps already found (Homebrew casks, App Store), matched by bundle name.
pub fn direct_apps(known: &[App]) -> Vec<App> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let known: HashSet<String> = known
        .iter()
        .filter(|a| !a.id.starts_with("formula:"))
        .map(|a| a.bundle.clone().unwrap_or_else(|| format!("{}.app", a.name)).to_lowercase())
        .collect();
    let mut apps = Vec::new();
    for dir in app_dirs() {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let file = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            // Dot-folders are leftovers (e.g. updater backups), not apps.
            let Some(stem) = file.strip_suffix(".app").filter(|_| !file.starts_with('.')) else { continue };
            // The App Store's own receipt marks apps `mas` already covers.
            if known.contains(&file.to_lowercase()) || path.join("Contents/_MASReceipt").exists() || apps.iter().any(|a: &App| a.name == stem) {
                continue;
            }
            let Some(info) = plist(&path.join("Contents/Info.plist")) else { continue };
            if info["LSBackgroundOnly"].as_bool() == Some(true) || info["LSBackgroundOnly"].as_str() == Some("1") {
                continue;
            }
            let version = ["CFBundleShortVersionString", "CFBundleVersion"].iter().find_map(|k| info[k].as_str()).unwrap_or("");
            let name = scan::clean(stem);
            apps.push(App {
                id: format!("direct:{name}"),
                hue: scan::hue(&name),
                vendor: scan::clean(info["CFBundleIdentifier"].as_str().unwrap_or("Descarga directa")),
                from: scan::clean(version),
                to: String::new(),
                source: Source::Direct,
                size_mb: None,
                bundle: Some(file.clone()),
                installed_at: None,
                name,
            });
        }
    }
    enrich(&mut apps);
    apps
}

/// Info.plist as JSON through the system `plutil` (the file is usually binary).
fn plist(path: &Path) -> Option<Value> {
    let out = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

/// The `.app` folder of an app, when it sits in a standard location.
pub fn bundle(a: &App) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = [Some(PathBuf::from("/Applications")), home.map(|h| h.join("Applications")), Some(PathBuf::from("/Applications/Utilities"))];
    let names = [a.bundle.clone(), Some(format!("{}.app", a.name))];
    dirs.into_iter().flatten().flat_map(|d| names.iter().flatten().map(move |n| d.join(n))).find(|p| p.is_dir())
}
