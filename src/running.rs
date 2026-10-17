//! Is an app in use right now? Decided from the list of running processes.
use std::process::{Command, Stdio};

use crate::local;
use crate::model::App;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    InUse,
    /// The app's location is unknown, so nothing can be said: treated like "in use".
    Unknown,
}

/// Executable path of every running process (`ps` shows full paths for `comm` on macOS).
pub fn processes() -> Vec<String> {
    let out = Command::new("/bin/ps").args(["-axo", "comm="]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

pub fn state(app: &App, procs: &[String]) -> State {
    let (kind, name) = app.id.split_once(':').unwrap_or(("", &app.id));
    if kind == "formula" {
        let name = name.rsplit('/').next().unwrap_or(name);
        return if procs.iter().any(|p| p.rsplit('/').next() == Some(name)) { State::InUse } else { State::Idle };
    }
    match local::bundle(app) {
        // Helpers and the main executable all live under Contents/.
        Some(bundle) => {
            let prefix = format!("{}/Contents/", bundle.display());
            if procs.iter().any(|p| p.starts_with(&prefix)) { State::InUse } else { State::Idle }
        }
        None => State::Unknown,
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
    fn app_without_a_known_bundle_is_unknown() {
        let mut app = formula("x");
        app.id = "cask:definitely-not-installed-zzz".into();
        app.name = "Definitely Not Installed Zzz".into();
        assert_eq!(state(&app, &[]), State::Unknown);
    }
}
