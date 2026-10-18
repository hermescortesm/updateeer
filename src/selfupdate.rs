//! Tells the user when a newer Updateeer is published on GitHub. On Linux packages it can also
//! download, verify and install the new package itself; elsewhere the user installs from the
//! release page.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::scan;

const LATEST: &str = "https://api.github.com/repos/hermescortesm/updateeer/releases/latest";
const PAGE_PREFIX: &str = "https://github.com/hermescortesm/updateeer/";
const DOWNLOAD_PREFIX: &str = "https://github.com/hermescortesm/updateeer/releases/download/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// Lowercase hex SHA-256 published by GitHub, when it provides one.
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub assets: Vec<Asset>,
    /// The release notes, which carry a SHA-256 list.
    pub notes: String,
}

/// Newest published release; blocks, so call it from a worker thread. `curl` ships with macOS,
/// Windows 10+ and nearly every Linux.
pub fn latest() -> Result<Release, String> {
    let mut cmd = Command::new("curl");
    cmd.stdin(Stdio::null());
    let args = ["-fsSL", "--max-time", "15", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: updateeer", LATEST];
    parse(&scan::run(cmd, &args, Duration::from_secs(25))?)
}

fn parse(json: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("Respuesta inesperada de GitHub")?;
    let url = v["html_url"].as_str().filter(|u| u.starts_with(PAGE_PREFIX)).ok_or("Respuesta inesperada de GitHub")?;
    let assets = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let url = a["browser_download_url"].as_str().filter(|u| u.starts_with(DOWNLOAD_PREFIX))?;
            let sha256 = a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase);
            Some(Asset { name: a["name"].as_str()?.to_string(), url: url.to_string(), sha256 })
        })
        .collect();
    Ok(Release { version: tag.trim_start_matches('v').to_string(), url: url.to_string(), assets, notes: v["body"].as_str().unwrap_or("").to_string() })
}

/// The package kind this system installs and its file suffix: `(manager, suffix)`.
fn package_kind(has: impl Fn(&str) -> bool) -> Option<(&'static str, &'static str)> {
    if has("apt") {
        Some(("apt", ".deb"))
    } else if has("pacman") {
        Some(("pacman", ".pkg.tar.zst"))
    } else if has("dnf") {
        Some(("dnf", ".rpm"))
    } else {
        None
    }
}

/// The SHA-256 for `name`: GitHub's own digest, else the `<hex>  <name>` list in the notes.
fn expected_sha(asset: &Asset, notes: &str) -> Option<String> {
    asset.sha256.clone().or_else(|| {
        notes.lines().find_map(|l| {
            let (hex, name) = l.trim().split_once(char::is_whitespace)?;
            (name.trim() == asset.name && hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_lowercase())
        })
    })
}

/// Whether this copy was installed from a system package, so a new package can replace it.
pub fn can_self_install() -> bool {
    cfg!(target_os = "linux") && std::env::consts::ARCH == "x86_64" && std::env::current_exe().is_ok_and(|p| p.starts_with("/usr/bin"))
}

/// Downloads the package for this system, checks its SHA-256, installs it as administrator and
/// starts the new version. Blocks, so call it from a worker thread.
pub fn install(release: &Release) -> Result<(), String> {
    let (manager, suffix) = package_kind(|k| crate::linuxpkg::path(k).is_some()).ok_or("No se encontró apt, pacman ni dnf")?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(suffix) && a.name.contains("linux-"))
        .ok_or("Esta versión no trae un paquete para tu sistema")?;
    let want = expected_sha(asset, &release.notes).ok_or("La versión no publica una suma SHA-256 para verificar el paquete")?;
    let dir = std::env::temp_dir().join(format!("updateeer-update-{}", std::process::id()));
    let result = download_and_install(manager, asset, &want, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    result?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn download_and_install(manager: &str, asset: &Asset, want: &str, dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700).create(dir).map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file: PathBuf = dir.join(&asset.name);
    let file_str = file.to_str().ok_or("Ruta no válida")?;
    scan::run(Command::new("curl"), &["-fL", "--proto", "=https", "--max-time", "600", "-o", file_str, &asset.url], Duration::from_secs(620))?;
    let sum = scan::run(Command::new("sha256sum"), &[file_str], Duration::from_secs(60))?;
    if sum.split_whitespace().next().map(str::to_lowercase).as_deref() != Some(want) {
        return Err("La suma SHA-256 del paquete no coincide; no se instaló".into());
    }
    crate::linuxpkg::install_file(manager, file_str)
}

fn parts(v: &str) -> Option<Vec<u64>> {
    v.trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect()
}

/// Whether `latest` is a higher version than `current` (both like `0.1.6`).
pub fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parts(current), parts(latest)), (Some(c), Some(l)) if l > c)
}

/// Opens the release page in the browser.
pub fn open(url: &str) {
    if !url.starts_with(PAGE_PREFIX) {
        return;
    }
    let (tool, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("/usr/bin/open", vec![url])
    } else if cfg!(windows) {
        ("explorer", vec![url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = Command::new(tool).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.1.6", "0.1.7"));
        assert!(is_newer("0.1.9", "0.1.10"));
        assert!(is_newer("0.1.6", "v0.2.0"));
        assert!(!is_newer("0.1.6", "0.1.6"));
        assert!(!is_newer("0.1.7", "0.1.6"));
        assert!(!is_newer("0.1.6", "garbage"));
    }

    #[test]
    fn parses_the_github_answer_and_refuses_foreign_links() {
        let ok = r#"{"tag_name":"v0.1.7","html_url":"https://github.com/hermescortesm/updateeer/releases/tag/v0.1.7"}"#;
        assert_eq!(parse(ok).unwrap(), Release { version: "0.1.7".into(), url: "https://github.com/hermescortesm/updateeer/releases/tag/v0.1.7".into(), assets: vec![], notes: String::new() });
        assert!(parse(r#"{"tag_name":"v1","html_url":"https://evil.example/x"}"#).is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn picks_packages_and_checks_sums() {
        let json = r#"{"tag_name":"v0.1.9","html_url":"https://github.com/hermescortesm/updateeer/releases/tag/v0.1.9","body":"x","assets":[
            {"name":"Updateeer-0.1.9-linux-amd64.deb","browser_download_url":"https://github.com/hermescortesm/updateeer/releases/download/v0.1.9/a.deb","digest":"sha256:ABC"},
            {"name":"evil.rpm","browser_download_url":"https://evil.example/evil.rpm"}]}"#;
        let r = parse(json).unwrap();
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].sha256.as_deref(), Some("abc"));
        assert_eq!(package_kind(|k| k == "pacman"), Some(("pacman", ".pkg.tar.zst")));
        assert_eq!(package_kind(|_| false), None);
        let hex = "a".repeat(64);
        let plain = Asset { name: "p.rpm".into(), url: String::new(), sha256: None };
        assert_eq!(expected_sha(&plain, &format!("```\n{hex}  p.rpm\n```")), Some(hex));
        assert_eq!(expected_sha(&plain, "nothing here"), None);
    }
}
