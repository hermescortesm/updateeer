//! Free-text search for apps to install, through the package manager of this OS.
use crate::i18n::t;
use std::path::Path;

use crate::model::{App, Source};
use crate::scan::{self, SCAN_TIMEOUT};
use crate::{linuxpkg, winpkg};

/// What may reach a command line: short, no control characters, no flag, no regex/path syntax.
pub fn safe_query(q: &str) -> bool {
    let q = q.trim();
    q.chars().count() >= 2 && q.len() <= 60 && !q.starts_with('-') && !q.contains('/') && !q.chars().any(char::is_control)
}

pub fn search(q: &str) -> Result<Vec<App>, String> {
    let q = q.trim();
    if !safe_query(q) {
        return Err(t("Escribe al menos 2 letras").into());
    }
    if cfg!(target_os = "macos") {
        brew_search(q)
    } else if cfg!(windows) {
        let args = ["search", q, "--accept-source-agreements", "--disable-interactivity"];
        let out = std::process::Command::new(winpkg::winget_path().ok_or(t("winget no está instalado"))?);
        Ok(winpkg::parse_winget(&scan::run(out, &args, SCAN_TIMEOUT)?, false).into_iter().take(25).map(|mut a| {
            a.from.clear();
            a
        }).collect())
    } else {
        Ok(linuxpkg::search(q))
    }
}

fn brew_search(q: &str) -> Result<Vec<App>, String> {
    let casks = scan::brew(&["search", "--casks", q], SCAN_TIMEOUT, true)?;
    let formulae = scan::brew(&["search", "--formulae", q], SCAN_TIMEOUT, true)?;
    let mut apps = parse_brew("cask", &casks);
    apps.truncate(20);
    apps.extend(parse_brew("formula", &formulae).into_iter().take(10));
    Ok(apps)
}

/// `brew search` prints one token per line, with `==>` headings and "No ... found" notes.
pub fn parse_brew(kind: &str, out: &str) -> Vec<App> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("==>") && !l.contains(' ') && scan::safe_token(l))
        .map(|t| App {
            id: format!("{kind}:{t}"),
            hue: scan::hue(t),
            name: scan::clean(t),
            vendor: if kind == "cask" { crate::i18n::t("Homebrew · app").into() } else { crate::i18n::t("Homebrew · herramienta").into() },
            from: String::new(),
            to: String::new(),
            source: Source::Homebrew,
            size_mb: None,
            bundle: None,
            installed_at: None,
        })
        .collect()
}

/// Whether this OS has a package manager that can install from Discover.
pub fn available() -> bool {
    if cfg!(target_os = "macos") {
        scan::brew_path().is_some()
    } else if cfg!(windows) {
        winpkg::winget_path().is_some()
    } else {
        linuxpkg::path("flatpak").is_some() || ["/usr/bin/apt-get", "/bin/apt-get"].iter().any(|p| Path::new(p).exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brew_output_is_filtered() {
        let out = "==> Casks\nfirefox\nfirefox@developer-edition\n\nNo formulae found for \"x\".\nuser/tap/thing\n";
        let ids: Vec<String> = parse_brew("cask", out).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["cask:firefox", "cask:firefox@developer-edition", "cask:user/tap/thing"]);
    }

    #[test]
    fn queries_are_checked() {
        assert!(safe_query("firefox"));
        assert!(safe_query("  vs code "));
        assert!(!safe_query("a"));
        assert!(!safe_query("--cask"));
        assert!(!safe_query("/regex/"));
        assert!(search("-x").is_err());
    }
}
