//! macOS "App Management": without it the system blocks any change to an app's bundle, so
//! `brew upgrade --cask` fails halfway. There is no API to ask, so it is probed by writing a
//! scratch file inside an app the user owns.
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Granted,
    Denied,
    /// No app of the user's to try on (or an unrelated error).
    Unknown,
}

/// Where the permission lives in System Settings.
#[cfg(target_os = "macos")]
pub fn open_settings() {
    let _ = std::process::Command::new("/usr/bin/open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AppBundles").status();
}

#[cfg(not(target_os = "macos"))]
pub fn open_settings() {}

/// Only macOS asks for it; elsewhere it is always fine.
pub fn app_management() -> Access {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        let Some(me) = std::env::var_os("HOME").and_then(|h| std::fs::metadata(h).ok()).map(|m| m.uid()) else { return Access::Unknown };
        let candidate = std::fs::read_dir("/Applications").into_iter().flatten().flatten().map(|e| e.path().join("Contents")).find(|c| c.metadata().is_ok_and(|m| m.uid() == me));
        candidate.map_or(Access::Unknown, |dir| probe(&dir))
    }
    #[cfg(not(target_os = "macos"))]
    Access::Granted
}

/// Creates and removes a hidden file in `dir`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn probe(dir: &Path) -> Access {
    let file = dir.join(".updateeer-check");
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&file) {
        Ok(_) => {
            let _ = std::fs::remove_file(file);
            Access::Granted
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Access::Denied,
        Err(_) => Access::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_tells_writable_from_not() {
        let dir = std::env::temp_dir().join(format!("updateeer-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(probe(&dir), Access::Granted);
        assert!(!dir.join(".updateeer-check").exists());
        assert_eq!(probe(&dir.join("missing")), Access::Unknown);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
