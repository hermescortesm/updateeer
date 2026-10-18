//! App Store apps through `mas` (unofficial CLI: `brew install mas`). Ids look like `mas:497799835`.
use std::path::Path;
use std::process::Command;

use crate::model::{App, Source};
use crate::scan::{self, SCAN_TIMEOUT, UPGRADE_TIMEOUT};

const MAS_PATHS: [&str; 2] = ["/opt/homebrew/bin/mas", "/usr/local/bin/mas"];

pub fn path() -> Option<&'static str> {
    MAS_PATHS.into_iter().find(|p| Path::new(p).exists())
}

fn mas(args: &[&str], timeout: std::time::Duration) -> Result<String, String> {
    let bin = path().ok_or("mas no está instalado (brew install mas)")?;
    scan::run(Command::new(bin), args, timeout)
}

/// App Store ids are plain numbers; anything else never reaches the command line.
pub fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())
}

pub fn outdated() -> Result<Vec<App>, String> {
    Ok(parse(&mas(&["outdated"], SCAN_TIMEOUT)?, true))
}

pub fn installed() -> Result<Vec<App>, String> {
    Ok(parse(&mas(&["list"], SCAN_TIMEOUT)?, false))
}

/// Blocks until mas finishes; call it from a worker thread.
pub fn upgrade(id: &str) -> Result<(), String> {
    if !safe_id(id) {
        return Err(format!("id no soportado: {id}"));
    }
    match mas(&["upgrade", id], UPGRADE_TIMEOUT) {
        // Recent `mas` versions call sudo, which cannot ask for a password here.
        Err(e) if needs_admin(&e) => admin_upgrade(id),
        r => r.map(|_| ()),
    }
}

fn needs_admin(error: &str) -> bool {
    error.contains("password is required") || error.contains("a terminal is required")
}

/// Runs the upgrade as root behind the system's own password dialog.
fn admin_upgrade(id: &str) -> Result<(), String> {
    let bin = path().ok_or("mas no está instalado (brew install mas)")?;
    let script = format!("do shell script {} with administrator privileges", crate::setup::applescript_string(&format!("{bin} upgrade {id}")));
    scan::run(Command::new("/usr/bin/osascript"), &["-e", &script], UPGRADE_TIMEOUT).map(|_| ()).map_err(|e| {
        if e.contains("-128") { "Cancelaste la petición de contraseña".to_string() } else { e }
    })
}

/// Lines look like `497799835  Xcode  (27.0 -> 27.1)` (outdated) or `...  (27.0)` (list).
/// Names may contain spaces and parentheses, so the version is the last `(...)` group.
fn parse(out: &str, outdated: bool) -> Vec<App> {
    out.lines()
        .filter_map(|line| {
            let (id, rest) = line.trim().split_once(char::is_whitespace)?;
            let rest = rest.trim().strip_suffix(')')?;
            let (name, versions) = rest.rsplit_once('(')?;
            let name = scan::clean(name.trim());
            if !safe_id(id) || name.is_empty() {
                return None;
            }
            let (from, to) = match versions.split_once(" -> ") {
                Some((f, t)) if outdated => (f, t),
                None if !outdated => (versions, ""),
                _ => return None,
            };
            Some(App {
                id: format!("mas:{id}"),
                hue: scan::hue(&name),
                name,
                vendor: "App Store".into(),
                from: scan::clean(from.trim()),
                to: scan::clean(to.trim()),
                source: Source::AppStore,
                size_mb: None,
                bundle: None,
                installed_at: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_outdated_and_list() {
        let out = " 497799835  Xcode  (27.0 -> 27.1)\n1176074088  Termius (SSH) (10.1.3   -> 10.2.1)\nnoise\n--x  Evil  (1 -> 2)\n";
        let apps = parse(out, true);
        assert_eq!(apps.len(), 2);
        assert_eq!((apps[0].id.as_str(), apps[0].from.as_str(), apps[0].to.as_str()), ("mas:497799835", "27.0", "27.1"));
        assert_eq!(apps[1].name, "Termius (SSH)");
        let list = parse("937984704  Amphetamine     (5.3.2)\n", false);
        assert_eq!((list[0].name.as_str(), list[0].from.as_str(), list[0].to.as_str()), ("Amphetamine", "5.3.2", ""));
        assert!(parse("937984704  Amphetamine (5.3.2)\n", true).is_empty());
    }

    #[test]
    fn sudo_failures_are_recognized() {
        assert!(needs_admin("sudo: a terminal is required to read the password; either use the -S option\nsudo: a password is required"));
        assert!(!needs_admin("Error: no such app"));
    }

    #[test]
    fn ids_are_numeric_only() {
        assert!(safe_id("497799835"));
        assert!(!safe_id("-1"));
        assert!(!safe_id("12 34"));
        assert!(!safe_id(""));
        assert_eq!(upgrade("--all"), Err("id no soportado: --all".into()));
    }
}
