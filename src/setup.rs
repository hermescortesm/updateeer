//! Installing the tools Updateeer relies on, always on the user's request.
use std::process::{Command, Stdio};

use crate::scan;

/// Official Homebrew installer. It asks for the admin password, so it runs in a visible Terminal.
const HOMEBREW_INSTALL: &str = r#"/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)""#;

fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Opens Terminal running the official Homebrew installer.
pub fn open_homebrew_installer() -> Result<(), String> {
    let script = format!("tell application \"Terminal\" to do script {}", applescript_string(HOMEBREW_INSTALL));
    let ok = Command::new("/usr/bin/osascript")
        .args(["-e", "tell application \"Terminal\" to activate", "-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?
        .success();
    if ok { Ok(()) } else { Err("No se pudo abrir Terminal".into()) }
}

/// `brew install mas`; blocks, so call it from a worker thread.
pub fn install_mas() -> Result<(), String> {
    scan::brew(&["install", "mas"], scan::UPGRADE_TIMEOUT, true).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_string_escapes_quotes_and_backslashes() {
        assert_eq!(applescript_string(r#"a"b\c"#), r#""a\"b\\c""#);
        let s = applescript_string(HOMEBREW_INSTALL);
        assert!(s.starts_with("\"/bin/bash -c \\\"$(curl"));
    }
}
