//! Facts read from this machine: where an app lives and when its installed version arrived.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::model::App;
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
        "cask" => newest_birth(&prefix()?.join("Caskroom").join(name)),
        "formula" => newest_birth(&prefix()?.join("Cellar").join(name)),
        // The App Store replaces the bundle on update, so its modification time is the update.
        "mas" => fs::metadata(bundle(a)?).ok()?.modified().ok(),
        _ => None,
    }
}

/// The `.app` folder of an app, when it sits in a standard location.
pub fn bundle(a: &App) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = [Some(PathBuf::from("/Applications")), home.map(|h| h.join("Applications")), Some(PathBuf::from("/Applications/Utilities"))];
    let names = [a.bundle.clone(), Some(format!("{}.app", a.name))];
    dirs.into_iter().flatten().flat_map(|d| names.iter().flatten().map(move |n| d.join(n))).find(|p| p.is_dir())
}
