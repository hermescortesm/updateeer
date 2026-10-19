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
    } else if has("zypper") {
        Some(("zypper", ".rpm"))
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

/// The `.app` folder this copy runs from (macOS).
fn app_bundle() -> Option<PathBuf> {
    std::env::current_exe().ok()?.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// Whether this copy is an installed one that a new package can replace: a system package under
/// `/usr/bin` (Linux), a bundle in a writable folder (macOS) or the MSI install (Windows).
pub fn can_self_install() -> bool {
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(target_os = "linux") {
        std::env::consts::ARCH == "x86_64" && exe.starts_with("/usr/bin")
    } else if cfg!(target_os = "macos") {
        app_bundle().and_then(|b| b.parent().map(Path::to_path_buf)).is_some_and(|dir| {
            let probe = dir.join(".updateeer-write-check");
            let ok = std::fs::write(&probe, b"").is_ok();
            let _ = std::fs::remove_file(probe);
            ok
        })
    } else {
        std::env::consts::ARCH == "x86_64" && std::env::var_os("ProgramFiles").is_some_and(|pf| exe.starts_with(pf))
    }
}

/// Suffix of the release file for this system, and the Linux package manager that installs it.
fn wanted_file() -> Result<(String, &'static str), String> {
    if cfg!(target_os = "macos") {
        let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { "x86_64" };
        Ok((format!("macos-{arch}.dmg"), ""))
    } else if cfg!(windows) {
        Ok(("windows-x64.msi".into(), ""))
    } else {
        let (manager, suffix) = package_kind(|k| crate::linuxpkg::path(k).is_some()).ok_or("No se encontró apt, pacman, dnf ni zypper")?;
        Ok((suffix.to_string(), manager))
    }
}

/// Downloads the installer for this system, checks its SHA-256, installs it and starts the new
/// version; the caller then exits the old one. Blocks, so call it from a worker thread.
pub fn install(release: &Release) -> Result<(), String> {
    let (suffix, manager) = wanted_file()?;
    let asset = release.assets.iter().find(|a| a.name.ends_with(&suffix)).ok_or("Esta versión no trae un instalador para tu sistema")?;
    let want = expected_sha(asset, &release.notes).ok_or("La versión no publica una suma SHA-256 para verificar el instalador")?;
    let dir = std::env::temp_dir().join(format!("updateeer-update-{}", std::process::id()));
    let exe = exe_path()?;
    let file = download(asset, &want, &dir)?;
    // On Windows the installer outlives this process, so its folder is left in place.
    let result = if cfg!(target_os = "macos") {
        replace_bundle(&file, &dir)
    } else if cfg!(windows) {
        run_msi(&file, &exe)
    } else {
        crate::linuxpkg::install_file(manager, file.to_str().ok_or("Ruta no válida")?).and_then(|_| relaunch(&exe))
    };
    if !cfg!(windows) {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

fn download(asset: &Asset, want: &str, dir: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(dir);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700).create(dir).map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = dir.join(&asset.name);
    let file_str = file.to_str().ok_or("Ruta no válida")?;
    scan::run(Command::new("curl"), &["-fL", "--proto", "=https", "--max-time", "600", "-o", file_str, &asset.url], Duration::from_secs(620))?;
    if sha256(file_str)? != want {
        return Err("La suma SHA-256 del instalador no coincide; no se instaló".into());
    }
    Ok(file)
}

/// Lowercase hex SHA-256 of a file, from the tool every OS already ships.
fn sha256(file: &str) -> Result<String, String> {
    let t = Duration::from_secs(60);
    let hex = if cfg!(windows) {
        let out = scan::run(Command::new("certutil"), &["-hashfile", file, "SHA256"], t)?;
        out.lines().nth(1).unwrap_or("").split_whitespace().collect::<String>()
    } else if cfg!(target_os = "macos") {
        scan::run(Command::new("/usr/bin/shasum"), &["-a", "256", file], t)?.split_whitespace().next().unwrap_or("").to_string()
    } else {
        scan::run(Command::new("sha256sum"), &[file], t)?.split_whitespace().next().unwrap_or("").to_string()
    };
    Ok(hex.to_lowercase())
}

fn spawn_detached(cmd: &mut Command) -> Result<(), String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(cmd, 0x0800_0000);
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// This executable's path. Linux appends " (deleted)" once a package replaced the file, so the
/// suffix is dropped: the new version sits at the same path.
fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map(|e| clean_exe(&e)).map_err(|e| e.to_string())
}

fn clean_exe(exe: &Path) -> PathBuf {
    exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")).map_or_else(|| exe.to_path_buf(), PathBuf::from)
}

/// Starts `prog args` a moment later from a detached shell. The new copy must wait: while this
/// one is still alive it holds the single-instance lock and the newcomer would just exit.
fn start_after_exit(prog: &Path, args: &[&str]) -> Result<(), String> {
    spawn_detached(Command::new("/bin/sh").args(["-c", "sleep 1; exec \"$@\"", "sh"]).arg(prog).args(args))
}

fn relaunch(exe: &Path) -> Result<(), String> {
    start_after_exit(exe, &[])
}

/// macOS: mounts the dmg, swaps the running bundle for the new one and opens it.
fn replace_bundle(dmg: &Path, dir: &Path) -> Result<(), String> {
    let bundle = app_bundle().ok_or("No se encontró la app instalada")?;
    swap_from_dmg(dmg, &dir.join("mnt"), &bundle)?;
    start_after_exit(Path::new("/usr/bin/open"), &["-n", bundle.to_str().ok_or("Ruta no válida")?])
}

fn swap_from_dmg(dmg: &Path, mount: &Path, bundle: &Path) -> Result<(), String> {
    let (dmg_s, mount_s) = (dmg.to_str().ok_or("Ruta no válida")?, mount.to_str().ok_or("Ruta no válida")?);
    std::fs::create_dir_all(mount).map_err(|e| e.to_string())?;
    let t = Duration::from_secs(120);
    scan::run(Command::new("/usr/bin/hdiutil"), &["attach", "-nobrowse", "-readonly", "-noverify", "-mountpoint", mount_s, dmg_s], t)?;
    let result = (|| {
        let parent = bundle.parent().ok_or("Ruta no válida")?;
        let (new, old) = (parent.join(".Updateeer-new.app"), parent.join(".Updateeer-old.app"));
        let _ = std::fs::remove_dir_all(&new);
        let _ = std::fs::remove_dir_all(&old);
        // `ditto` keeps the signature, permissions and extended attributes of the bundle.
        scan::run(Command::new("/usr/bin/ditto"), &[mount.join("Updateeer.app").to_str().ok_or("Ruta no válida")?, new.to_str().ok_or("Ruta no válida")?], t)?;
        std::fs::rename(bundle, &old).map_err(|e| format!("No se pudo reemplazar la app: {e}"))?;
        if let Err(e) = std::fs::rename(&new, bundle) {
            let _ = std::fs::rename(&old, bundle);
            return Err(format!("No se pudo reemplazar la app: {e}"));
        }
        let _ = std::fs::remove_dir_all(&old);
        Ok(())
    })();
    let _ = scan::run(Command::new("/usr/bin/hdiutil"), &["detach", "-force", mount_s], t);
    result
}

/// Windows: the installer needs this app closed, so a PowerShell helper waits for it, runs the
/// MSI (Windows asks for permission) and opens the new version.
fn run_msi(msi: &Path, exe: &Path) -> Result<(), String> {
    let q = |p: &Path| p.display().to_string().replace('\'', "''");
    let script = format!("Start-Sleep -Seconds 2; Start-Process msiexec -ArgumentList '/i','\"{}\"','/passive','/norestart' -Wait; Start-Process '{}'", q(msi), q(exe));
    spawn_detached(Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]))
}

/// The release notes as plain text for the in-app "what's new": headings and bullets tidied,
/// the checksum list left out.
pub fn summary(notes: &str) -> String {
    let mut out = Vec::new();
    for line in notes.lines().map(str::trim) {
        if line.starts_with("### SHA-256") {
            break;
        }
        if line.is_empty() || line.starts_with("## ") || line.starts_with("```") {
            continue;
        }
        out.push(match (line.strip_prefix("### "), line.strip_prefix("- ")) {
            (Some(h), _) => format!("{h}:"),
            (_, Some(b)) => format!("• {b}"),
            _ => line.to_string(),
        });
    }
    out.join("\n")
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

    /// Needs a real dmg: `UPDATEEER_TEST_DMG=dist/Updateeer-x.dmg cargo test swaps_a_bundle -- --ignored`.
    #[test]
    #[ignore]
    fn swaps_a_bundle() {
        let dmg = PathBuf::from(std::env::var("UPDATEEER_TEST_DMG").unwrap()).canonicalize().unwrap();
        let root = std::env::temp_dir().join("updateeer-swap-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Updateeer.app/Contents")).unwrap();
        std::fs::write(root.join("Updateeer.app/Contents/old.txt"), "old").unwrap();
        swap_from_dmg(&dmg, &root.join("mnt"), &root.join("Updateeer.app")).unwrap();
        assert!(root.join("Updateeer.app/Contents/MacOS/updateeer").is_file());
        assert!(!root.join("Updateeer.app/Contents/old.txt").exists());
        assert!(!root.join(".Updateeer-old.app").exists());
    }

    #[test]
    fn hashes_a_file_with_the_system_tool() {
        let f = std::env::temp_dir().join("updateeer-sha-test");
        std::fs::write(&f, "abc").unwrap();
        assert_eq!(sha256(f.to_str().unwrap()).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn notes_become_plain_text() {
        let notes = "## Updateeer 0.1.85\n\n### Added\n- A banner\n\n### SHA-256\n```\nabc  file\n```\n";
        assert_eq!(summary(notes), "Added:\n• A banner");
    }

    #[test]
    fn a_replaced_binary_keeps_its_path() {
        // What current_exe() returns on Linux after the package manager swapped the file.
        assert_eq!(clean_exe(Path::new("/usr/bin/updateeer (deleted)")), PathBuf::from("/usr/bin/updateeer"));
        assert_eq!(clean_exe(Path::new("/usr/bin/updateeer")), PathBuf::from("/usr/bin/updateeer"));
    }
}
