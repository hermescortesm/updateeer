//! Is an app in use right now? Decided from the list of running processes.
use std::process::{Command, Stdio};

use crate::local;
use crate::model::{App, Source};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    InUse,
    /// The app's location is unknown, so nothing can be said: treated like "in use".
    Unknown,
}

/// Running processes: full executable paths on macOS (`ps` shows them for `comm`), bare names
/// on Linux and Windows.
pub fn processes() -> Vec<String> {
    #[cfg(windows)]
    let out = {
        let mut cmd = Command::new("tasklist");
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
        cmd.args(["/FO", "CSV", "/NH"]).stdin(Stdio::null()).stderr(Stdio::null()).output().map(|o| parse_tasklist(&String::from_utf8_lossy(&o.stdout)))
    };
    #[cfg(not(windows))]
    let out = {
        let flag = if cfg!(target_os = "macos") { "-axo" } else { "-eo" };
        Command::new("/bin/ps").args([flag, "comm="]).stdin(Stdio::null()).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
    };
    out.unwrap_or_default()
}

/// `tasklist /FO CSV /NH` lines look like `"chrome.exe","1234","Console","1","200,000 K"`.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_tasklist(out: &str) -> Vec<String> {
    out.lines().filter_map(|l| l.strip_prefix('"')?.split_once('"').map(|(image, _)| image.to_string())).collect()
}

/// Package managers that ask for administrator rights: they never update on their own.
pub fn needs_admin(app: &App) -> bool {
    matches!(app.source, Source::Apt | Source::Pacman | Source::Aur | Source::Dnf | Source::Zypper | Source::Apk | Source::Xbps | Source::Chocolatey)
}

/// Lowercase names a running process of this app could have: the package name, or the last part
/// of a reverse-DNS / `Vendor.Product` id.
fn names(app: &App) -> Vec<String> {
    let id = app.id.split_once(':').map_or(app.id.as_str(), |(_, n)| n);
    let mut out = vec![id.rsplit('/').next().unwrap_or(id).to_lowercase()];
    if let Some(last) = id.rsplit('.').next() {
        out.push(last.to_lowercase());
    }
    out.push(app.name.to_lowercase().replace(' ', ""));
    out.retain(|n| n.len() >= 3);
    out.dedup();
    out
}

/// A process name matches when it is equal (ignoring `.exe`) or, because Linux cuts `comm` at 15
/// characters, when it is the start of a longer name.
fn matches_process(proc: &str, name: &str) -> bool {
    let base = proc.rsplit(['/', '\\']).next().unwrap_or(proc).to_lowercase();
    let base = base.strip_suffix(".exe").unwrap_or(&base);
    base == name || (base.len() == 15 && name.starts_with(base))
}

pub fn state(app: &App, procs: &[String]) -> State {
    let (kind, name) = app.id.split_once(':').unwrap_or(("", &app.id));
    if kind == "formula" {
        let name = name.rsplit('/').next().unwrap_or(name);
        return if procs.iter().any(|p| p.rsplit('/').next() == Some(name)) { State::InUse } else { State::Idle };
    }
    if cfg!(target_os = "macos") {
        return match local::bundle(app) {
            // Helpers and the main executable all live under Contents/.
            Some(bundle) => {
                let prefix = format!("{}/Contents/", bundle.display());
                if procs.iter().any(|p| p.starts_with(&prefix)) { State::InUse } else { State::Idle }
            }
            None => State::Unknown,
        };
    }
    let names = names(app);
    if procs.iter().any(|p| names.iter().any(|n| matches_process(p, n))) {
        State::InUse
    // Windows ids and executables rarely share a name, so "not found" proves nothing there.
    } else if cfg!(windows) {
        State::Unknown
    } else {
        State::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;

    fn formula(name: &str) -> App {
        App { id: format!("formula:{name}"), name: name.into(), vendor: String::new(), from: String::new(), to: String::new(), source: Source::Homebrew, size_mb: None, hue: 0.0, bundle: None, installed_at: None }
    }

    #[test]
    fn formula_is_in_use_when_a_process_has_its_name() {
        let procs = vec!["/opt/homebrew/bin/colima".to_string(), "/usr/sbin/cfprefsd".to_string()];
        assert_eq!(state(&formula("colima"), &procs), State::InUse);
        assert_eq!(state(&formula("user/tap/colima"), &procs), State::InUse);
        assert_eq!(state(&formula("fzf"), &procs), State::Idle);
    }

    #[test]
    fn tasklist_and_name_matching() {
        assert_eq!(parse_tasklist("\"chrome.exe\",\"1\",\"Console\",\"1\",\"2 K\"\r\n\"a b.exe\",\"2\"\n"), ["chrome.exe", "a b.exe"]);
        let a = App { id: "flatpak:org.mozilla.firefox".into(), name: "Firefox".into(), ..formula("x") };
        assert!(names(&a).contains(&"firefox".to_string()));
        assert!(matches_process("firefox", "firefox") && matches_process("C:\\Apps\\Firefox.EXE", "firefox"));
        assert!(matches_process("gnome-shell-cal", "gnome-shell-calendar"));
        assert!(!matches_process("firefox-esr", "firefox"));
        assert!(needs_admin(&App { source: Source::Apt, ..formula("curl") }));
    }

    // A cask without a bundle on disk is only "unknown" where bundles exist.
    #[cfg(target_os = "macos")]
    #[test]
    fn app_without_a_known_bundle_is_unknown() {
        let mut app = formula("x");
        app.id = "cask:definitely-not-installed-zzz".into();
        app.name = "Definitely Not Installed Zzz".into();
        assert_eq!(state(&app, &[]), State::Unknown);
    }
}
