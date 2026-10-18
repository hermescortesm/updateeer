//! Finds outdated apps. Today: Homebrew casks and the formulae the user installed on purpose.
use std::collections::HashSet;
use std::path::Path;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{linuxpkg, local, mas, winpkg};
use crate::model::{App, Source};

const BREW_PATHS: [&str; 2] = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];

/// Bumped by `cancel_running`; a process started under an older value is killed.
static CANCEL_GEN: AtomicU64 = AtomicU64::new(0);

/// Kills every package manager process that is running right now.
pub fn cancel_running() {
    CANCEL_GEN.fetch_add(1, Ordering::SeqCst);
}

/// SIGTERM to the whole process group (brew spawns curl, ruby…), then the process itself.
fn kill_tree(child: &mut std::process::Child) {
    // Windows: take down the whole tree.
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill").args(["-TERM", "--", &format!("-{}", child.id())]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Output cap per stream: a runaway brew must not eat memory.
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(120);
pub const UPGRADE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Runs brew (absolute path, no shell, no stdin) with a timeout and capped output.
pub fn brew_path() -> Option<&'static str> {
    BREW_PATHS.into_iter().find(|p| Path::new(p).exists())
}

pub fn brew(args: &[&str], timeout: Duration, auto_update: bool) -> Result<String, String> {
    let bin = brew_path().ok_or("Homebrew no está instalado")?;
    let mut cmd = Command::new(bin);
    cmd.env("HOMEBREW_NO_ENV_HINTS", "1");
    if !auto_update {
        cmd.env("HOMEBREW_NO_AUTO_UPDATE", "1").env("HOMEBREW_NO_INSTALL_CLEANUP", "1");
    }
    run(cmd, args, timeout)
}

pub fn run(mut cmd: Command, args: &[&str], timeout: Duration) -> Result<String, String> {
    let generation = CANCEL_GEN.load(Ordering::SeqCst);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    // No console window flashing up for every command.
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
    cmd.args(args)
        // Never wait on a prompt (e.g. sudo): fail instead of hanging.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let (out_h, err_h) = (capture(child.stdout.take()), capture(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(st) => break st,
            None if CANCEL_GEN.load(Ordering::SeqCst) != generation => {
                kill_tree(&mut child);
                return Err("Cancelado".into());
            }
            None if Instant::now() >= deadline => {
                kill_tree(&mut child);
                return Err("El comando tardó demasiado y se canceló".into());
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    };
    let out = out_h.join().unwrap_or_default();
    let err = err_h.join().unwrap_or_default();
    if !status.success() {
        // winget and Chocolatey report errors on stdout.
        let msg = tail(&String::from_utf8_lossy(&err));
        return Err(if msg.is_empty() { tail(&String::from_utf8_lossy(&out)) } else { msg });
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}

/// Reads a stream on its own thread, up to MAX_OUTPUT bytes.
fn capture(stream: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = stream {
            let _ = s.by_ref().take(MAX_OUTPUT).read_to_end(&mut buf);
            // Keep draining so a chatty brew never blocks on a full pipe.
            let _ = std::io::copy(&mut s, &mut std::io::sink());
        }
        buf
    })
}

/// Last few lines of an error, so a tooltip never shows pages of brew output.
pub fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.trim().lines().collect();
    let t = lines[lines.len().saturating_sub(5)..].join("\n");
    let skip = t.chars().count().saturating_sub(400);
    t.chars().skip(skip).collect()
}

/// Apps found plus a non-fatal warning (one source failed while another worked).
pub type Found = (Vec<App>, Option<String>);

/// Homebrew and App Store together. Fails only when no source worked.
pub fn scan() -> Result<Found, String> {
    merge(sources(false))
}

type SourceResult = (&'static str, Option<Result<Vec<App>, String>>);

/// The package managers of this OS, each `None` when not installed. `installed`: list what is
/// installed instead of what is outdated.
fn sources(installed: bool) -> Vec<SourceResult> {
    if cfg!(windows) {
        vec![
            ("winget", winpkg::winget_path().map(|_| if installed { winpkg::winget_installed() } else { winpkg::winget_outdated() })),
            ("Scoop", winpkg::scoop_path().map(|_| if installed { winpkg::scoop_installed() } else { winpkg::scoop_outdated() })),
            ("Chocolatey", winpkg::choco_path().map(|_| if installed { winpkg::choco_installed() } else { winpkg::choco_outdated() })),
        ]
    } else if cfg!(target_os = "linux") {
        ["apt", "pacman", "dnf", "flatpak", "snap"]
            .into_iter()
            .map(|k| (k, linuxpkg::path(k).map(|_| if installed { linuxpkg::installed(k) } else { linuxpkg::outdated(k) })))
            .collect()
    } else {
        vec![
            ("Homebrew", brew_path().map(|_| if installed { installed_brew() } else { scan_brew() })),
            ("App Store", mas::path().map(|_| if installed { mas::installed() } else { mas::outdated() })),
        ]
    }
}

/// A source that is not installed (`None`) is skipped; one that fails only warns if another worked.
fn merge(sources: Vec<SourceResult>) -> Result<Found, String> {
    let mut apps = Vec::new();
    let mut errors = Vec::new();
    let mut worked = false;
    for (name, r) in sources {
        match r {
            Some(Ok(a)) => {
                worked = true;
                apps.extend(a);
            }
            Some(Err(e)) => errors.push((name, e)),
            None => {}
        }
    }
    let text = errors.iter().map(|(n, e)| format!("{n}: {e}")).collect::<Vec<_>>().join(" · ");
    if !worked {
        return Err(if text.is_empty() { "No se encontró ningún gestor de paquetes compatible (Homebrew, winget, Scoop, Chocolatey, apt, pacman, dnf, Flatpak o Snap)".into() } else { text });
    }
    local::enrich(&mut apps);
    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok((apps, (!text.is_empty()).then_some(text)))
}

fn scan_brew() -> Result<Vec<App>, String> {
    let outdated = brew(&["outdated", "--greedy", "--json=v2"], SCAN_TIMEOUT, true)?;
    let leaves = brew(&["leaves"], SCAN_TIMEOUT, true)?;
    let leaves: HashSet<&str> = leaves.lines().collect();
    let mut apps = parse_outdated(&outdated, &leaves)?;

    let tokens: Vec<String> = apps.iter().filter_map(|a| a.id.strip_prefix("cask:")).filter(|t| safe_token(t)).map(String::from).collect();
    if !tokens.is_empty() {
        let mut args = vec!["info", "--cask", "--json=v2"];
        args.extend(tokens.iter().map(String::as_str));
        apply_cask_info(&mut apps, &brew(&args, SCAN_TIMEOUT, true)?);
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok(apps)
}

/// Everything installed through Homebrew: casks plus the formulae the user asked for.
pub fn installed() -> Result<Found, String> {
    let (mut apps, warning) = match merge(sources(true)) {
        Ok(found) => found,
        Err(e) => (Vec::new(), Some(e)),
    };
    apps.extend(local::direct_apps(&apps));
    if apps.is_empty() {
        return Err(warning.unwrap_or_else(|| "No se encontraron apps".into()));
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok((apps, warning))
}

fn installed_brew() -> Result<Vec<App>, String> {
    let leaves = brew(&["leaves"], SCAN_TIMEOUT, false)?;
    // `leaves` may print `user/tap/name`; `list` prints the short name.
    let leaves: HashSet<&str> = leaves.lines().map(|l| l.rsplit('/').next().unwrap_or(l)).collect();
    let formulae = brew(&["list", "--formula", "--versions"], SCAN_TIMEOUT, false)?;
    let casks = brew(&["list", "--cask", "--versions"], SCAN_TIMEOUT, false)?;
    let mut apps = parse_list("formula", &formulae, &leaves);
    apps.extend(parse_list("cask", &casks, &leaves));

    let tokens: Vec<&str> = apps.iter().filter_map(|a| a.id.strip_prefix("cask:")).collect();
    if !tokens.is_empty() {
        let mut args = vec!["info", "--cask", "--json=v2"];
        args.extend(tokens);
        // Display names are a nicety: keep the tokens if this fails.
        if let Ok(json) = brew(&args, SCAN_TIMEOUT, false) {
            apply_cask_info(&mut apps, &json);
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    Ok(apps)
}

/// Parses `brew list --versions` lines (`name v1 v2`); the last version is the newest.
fn parse_list(kind: &str, out: &str, leaves: &HashSet<&str>) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next().filter(|n| safe_token(n))?;
            // Casks append build ids after a comma ("3.21.13,<hash>").
            let version = it.last()?.split(',').next()?;
            if kind == "formula" && !leaves.contains(name) {
                return None;
            }
            Some(app(format!("{kind}:{name}"), name, "Homebrew", clean(version), String::new()))
        })
        .collect()
}

/// Brew names are `[a-z0-9@._+-]`, optionally `user/tap/name`. Rejects anything that could
/// pass as a flag or climb out of a path.
pub fn safe_token(t: &str) -> bool {
    !t.is_empty()
        && t.split('/').count() <= 3
        && t.split('/').all(|seg| {
            !seg.is_empty()
                && !seg.starts_with('-')
                && seg != "."
                && seg != ".."
                && seg.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
        })
}

/// Text from brew that ends up on screen: no control characters, bounded length.
pub fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(80).collect()
}

fn first_version(v: &Value) -> String {
    match v {
        // Casks append build ids after a comma ("3.21.13,<hash>"); show only the version.
        Value::String(s) => s.split(',').next().unwrap_or_default().to_string(),
        Value::Array(a) => a.first().map(first_version).unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn hue(name: &str) -> f32 {
    (name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 360) as f32
}

fn app(id: String, name: &str, vendor: &str, from: String, to: String) -> App {
    App { id, name: name.into(), vendor: vendor.into(), from, to, source: Source::Homebrew, size_mb: None, hue: hue(name), bundle: None, installed_at: None }
}

fn parse_outdated(json: &str, leaves: &HashSet<&str>) -> Result<Vec<App>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut apps = Vec::new();
    for f in v["formulae"].as_array().into_iter().flatten() {
        let Some(name) = f["name"].as_str().filter(|n| safe_token(n) && leaves.contains(n)) else { continue };
        apps.push(app(format!("formula:{name}"), name, "Homebrew", first_version(&f["installed_versions"]), first_version(&f["current_version"])));
    }
    for c in v["casks"].as_array().into_iter().flatten() {
        let Some(name) = c["name"].as_str().filter(|n| safe_token(n)) else { continue };
        apps.push(app(format!("cask:{name}"), name, "Homebrew", first_version(&c["installed_versions"]), first_version(&c["current_version"])));
    }
    // A change only in the build id looks like "5.3.7 -> 5.3.7": nothing to show.
    apps.retain(|a| a.from != a.to);
    Ok(apps)
}

/// Replaces the cask token with its display name and homepage domain.
fn apply_cask_info(apps: &mut [App], json: &str) {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return };
    for c in v["casks"].as_array().into_iter().flatten() {
        let (Some(token), Some(name)) = (c["token"].as_str(), c["name"][0].as_str()) else { continue };
        if let Some(a) = apps.iter_mut().find(|a| a.id.strip_prefix("cask:") == Some(token)) {
            let name = clean(name);
            a.hue = hue(&name);
            a.name = name;
            if let Some(host) = c["homepage"].as_str().and_then(domain) {
                a.vendor = clean(&host);
            }
            a.bundle = c["artifacts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|art| art["app"][0].as_str())
                .find(|n| n.ends_with(".app") && !n.contains('/') && !n.starts_with('.'))
                .map(String::from);
        }
    }
}

fn domain(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split('/').next()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTDATED: &str = r#"{"formulae":[
        {"name":"node","installed_versions":["20.17.0"],"current_version":"22.9.0"},
        {"name":"aom","installed_versions":["3.15.1"],"current_version":"3.15.2"}],
      "casks":[{"name":"visual-studio-code","installed_versions":"1.93.1","current_version":"1.94.2"},
               {"name":"--evil","installed_versions":"1","current_version":"2"}]}"#;

    #[test]
    fn parses_leaves_and_casks_and_drops_unsafe() {
        let leaves = HashSet::from(["node"]);
        let apps = parse_outdated(OUTDATED, &leaves).unwrap();
        let ids: Vec<_> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["formula:node", "cask:visual-studio-code"]);
        assert_eq!((apps[1].from.as_str(), apps[1].to.as_str()), ("1.93.1", "1.94.2"));
    }

    #[test]
    fn parses_list_and_hides_dependencies() {
        let leaves = HashSet::from(["node"]);
        let out = "node 22.9.0 22.10.0\nicu4c 75.1\n--evil 1\nlonely\n";
        let apps = parse_list("formula", out, &leaves);
        assert_eq!(apps.len(), 1);
        assert_eq!((apps[0].id.as_str(), apps[0].from.as_str()), ("formula:node", "22.10.0"));
        assert_eq!(parse_list("cask", "iterm2 3.5.0\n", &leaves)[0].id, "cask:iterm2");
    }

    #[test]
    fn merge_skips_missing_and_warns_on_partial_failure() {
        let a = |n: &str| app(format!("cask:{n}"), n, "x", "1".into(), "2".into());
        let (apps, warn) = merge(vec![("Homebrew", Some(Ok(vec![a("b")]))), ("App Store", Some(Err("sin sesión".into())))]).unwrap();
        assert_eq!((apps.len(), warn.as_deref()), (1, Some("App Store: sin sesión")));
        assert!(merge(vec![("a", Some(Err("x".into()))), ("b", Some(Err("y".into())))]).is_err());
        assert_eq!(merge(vec![("a", None), ("b", Some(Ok(vec![])))]).unwrap().1, None);
        assert!(merge(vec![("a", None), ("b", None)]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cancel_kills_a_running_command() {
        let worker = thread::spawn(|| run(Command::new("/bin/sleep"), &["30"], Duration::from_secs(60)));
        thread::sleep(Duration::from_millis(400));
        let started = Instant::now();
        cancel_running();
        assert_eq!(worker.join().unwrap(), Err("Cancelado".to_string()));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn token_rules() {
        assert!(safe_token("node@22"));
        assert!(safe_token("user/tap/engram"));
        assert!(!safe_token("--evil"));
        assert!(!safe_token("a/../b"));
        assert!(!safe_token("a//b"));
        assert!(!safe_token("a/b/c/d"));
        assert!(!safe_token(""));
    }

    #[test]
    fn error_tail_is_bounded() {
        let long = (0..50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        assert_eq!(tail(&long).lines().count(), 5);
        assert!(tail(&"x".repeat(1000)).chars().count() <= 400);
    }

    #[test]
    fn cask_info_sets_display_name_and_vendor() {
        let mut apps = parse_outdated(OUTDATED, &HashSet::new()).unwrap();
        let info = r#"{"casks":[{"token":"visual-studio-code","name":["Visual Studio Code"],"homepage":"https://www.code.visualstudio.com/"}]}"#;
        apply_cask_info(&mut apps, info);
        assert_eq!(apps[0].name, "Visual Studio Code");
        assert_eq!(apps[0].vendor, "code.visualstudio.com");
    }

    /// Manual check against the real Homebrew: `cargo test live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live() {
        for a in scan().unwrap().0 {
            println!("{:<28} {:<24} {} -> {}", a.name, a.vendor, a.from, a.to);
        }
    }

    #[test]
    #[ignore]
    fn live_installed() {
        let apps = installed().unwrap().0;
        println!("{} installed", apps.len());
        for a in apps.iter().take(15) {
            println!("{:<28} {:<24} {}", a.name, a.vendor, a.from);
        }
    }
}
