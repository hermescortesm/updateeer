//! Linux package managers: apt, pacman, dnf, Flatpak and Snap. Parsing is plain text handling, so
//! it is unit-tested on every OS; running them only makes sense on Linux.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::friendly;
use crate::model::{App, Source};
use crate::sudobridge;
use crate::scan::{self, SCAN_TIMEOUT, UPGRADE_TIMEOUT};
use crate::winpkg::{app, safe_name};

mod desktop;
mod parse;
mod root;
use desktop::*;
use parse::*;
use root::*;
pub use desktop::{direct_apps, icon_file, is_gui};
pub use root::pacman_progress;

/// Every Linux package manager supported, as used in app ids (`pacman:firefox`).
pub const KINDS: [&str; 11] = ["apt", "pacman", "aur", "dnf", "zypper", "apk", "xbps", "flatpak", "snap", "nix", "guix"];

/// First `name` found in the usual binary directories.
fn find(name: &str) -> Option<PathBuf> {
    ["/usr/bin", "/bin", "/usr/local/bin", "/sbin", "/usr/sbin", "/snap/bin", "/var/lib/flatpak/exports/bin"].iter().map(|d| PathBuf::from(d).join(name)).find(|p| p.is_file())
}

pub fn path(kind: &str) -> Option<PathBuf> {
    find(match kind {
        "apt" => "apt-get",
        // AUR packages are the foreign ones of pacman; the helper is only needed to update them.
        "aur" => "pacman",
        "xbps" => "xbps-install",
        "guix" => "guix-package",
        k => k,
    })
}

/// The AUR helper installed, if any.
fn aur_helper() -> Option<&'static str> {
    ["paru", "yay"].into_iter().find(|h| find(h).is_some())
}

/// Whether a failure looks like a dropped or slow connection, worth trying again.
fn is_network_error(e: &str) -> bool {
    let e = e.to_lowercase();
    ["timeout was reached", "timed out", "connection reset", "could not connect", "couldn't connect", "temporary failure", "network is unreachable", "error from the server", "http/2 stream", "transferred a partial file"].iter().any(|m| e.contains(m))
}

/// Flathub often drops a big download halfway ("Timeout was reached"); the files already fetched
/// are kept, so another try carries on from there. Up to 4 attempts, unless cancelled.
fn retry_network(mut run: impl FnMut() -> Result<String, String>) -> Result<String, String> {
    let generation = scan::CANCEL_GEN.load(Ordering::SeqCst);
    let mut attempt = 1;
    loop {
        match run() {
            Err(e) if attempt < 4 && is_network_error(&e) && scan::CANCEL_GEN.load(Ordering::SeqCst) == generation => {
                thread::sleep(Duration::from_secs(if cfg!(test) { 0 } else { 3 * attempt }));
                attempt += 1;
            }
            Err(e) if attempt > 1 && is_network_error(&e) => return Err(format!("{e} (se intentó {attempt} veces; revisa tu conexión)")),
            r => return r,
        }
    }
}

/// Runs a tool with the C locale so the output format never depends on the user's language.
fn exec(tool: &str, args: &[&str], timeout: std::time::Duration) -> Result<String, String> {
    let bin = path(tool).ok_or("No instalado")?;
    let mut cmd = Command::new(bin);
    cmd.env("LC_ALL", "C");
    scan::run(cmd, args, timeout)
}

/// Some tools (`pacman -Qu`, `checkupdates`) fail with no message when there is nothing to list.
fn none_if_silent(r: Result<String, String>) -> Result<String, String> {
    r.or_else(|e| if e.trim().is_empty() { Ok(String::new()) } else { Err(e) })
}

/// `apt list --upgradable` only reads the local cache, so a cache older than this is refreshed first.
const APT_CACHE_MAX_AGE: Duration = Duration::from_secs(6 * 3600);

/// Age of the apt package lists, or `None` when unknown.
fn apt_cache_age() -> Option<Duration> {
    ["/var/lib/apt/periodic/update-success-stamp", "/var/lib/apt/lists"]
        .iter()
        .filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
        .max()
        .map(|t| t.elapsed().unwrap_or_default())
}

/// Runs `apt-get update` as root when the cache is stale. Asked once per session: a refused
/// password or a failure leaves the old cache in use instead of nagging on every scan.
fn refresh_apt() {
    static TRIED: AtomicBool = AtomicBool::new(false);
    if apt_cache_age().is_none_or(|a| a < APT_CACHE_MAX_AGE) || TRIED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = root(&["apt-get", "update", "-q"]);
}

/// Pending pacman upgrades. `checkupdates` syncs a private copy of the databases; it fails with
/// "Cannot fetch updates" when a mirror hiccups or another tool (a panel module, say) is using the
/// same copy, so it gets its own folder and a few tries. As a last resort the local database is
/// read, but an empty answer from that is not trusted: it would claim "up to date" for nothing.
fn pacman_updates() -> Result<String, String> {
    let mut failure = None;
    if let Some(bin) = path("checkupdates") {
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        let db = std::env::temp_dir().join(format!("updateeer-checkup-db-{user}"));
        for attempt in 1..=3u64 {
            let mut cmd = Command::new(&bin);
            cmd.env("LC_ALL", "C").env("CHECKUPDATES_DB", &db);
            match none_if_silent(scan::run(cmd, &[], UPGRADE_TIMEOUT)) {
                Ok(out) => return Ok(out),
                Err(e) => {
                    failure = Some(e);
                    if attempt < 3 {
                        thread::sleep(Duration::from_secs(2 * attempt));
                    }
                }
            }
        }
    }
    let local = none_if_silent(exec("pacman", &["-Qu"], SCAN_TIMEOUT));
    match (local, failure) {
        (Ok(out), Some(e)) if out.trim().is_empty() => Err(e),
        (Ok(out), _) => Ok(out),
        (Err(e), Some(first)) => Err(if first.is_empty() { e } else { first }),
        (Err(e), None) => Err(e),
    }
}

pub fn outdated(kind: &str) -> Result<Vec<App>, String> {
    outdated_raw(kind).map(|apps| apps.into_iter().map(polish).collect())
}

fn outdated_raw(kind: &str) -> Result<Vec<App>, String> {
    Ok(match kind {
        "apt" => {
            refresh_apt();
            set_gui("apt", apt_gui().iter());
            parse_apt_upgradable(&exec("apt", &["list", "--upgradable"], SCAN_TIMEOUT)?)
        }
        // `-Qu` only knows the last synced database; `checkupdates` (pacman-contrib) syncs a private
        // copy first. Both exit non-zero with no output when nothing is outdated.
        "pacman" => {
            // A new look at the system: whatever was upgraded before no longer counts.
            *SYSTEM_UPGRADED.lock().unwrap_or_else(|e| e.into_inner()) = None;
            refresh_pacman_gui();
            parse_pacman_upgrades(&pacman_updates()?)
        }
        "aur" => {
            refresh_pacman_gui();
            match aur_helper() {
                Some(h) => parse_upgrades("aur", Source::Aur, &none_if_silent(exec(h, &["-Qua"], UPGRADE_TIMEOUT))?),
                None => Vec::new(),
            }
        }
        "dnf" => {
            set_gui("dnf", rpm_gui().iter().map(|(n, _)| n).collect::<Vec<_>>().into_iter());
            let mut apps = parse_dnf_upgrades(&exec("dnf", &["list", "--upgrades", "-q"], SCAN_TIMEOUT)?);
            fill_rpm_versions(&mut apps);
            apps
        }
        "zypper" => {
            set_gui("zypper", rpm_gui().iter().map(|(n, _)| n).collect::<Vec<_>>().into_iter());
            parse_zypper_updates(&exec("zypper", &["--non-interactive", "--quiet", "list-updates"], SCAN_TIMEOUT)?)
        }
        "apk" => parse_apk_updates(&none_if_silent(exec("apk", &["version", "-l", "<"], SCAN_TIMEOUT))?),
        "xbps" => {
            let new = none_if_silent(exec("xbps-install", &["-Mun"], SCAN_TIMEOUT))?;
            let old = exec("xbps-query", &["-l"], SCAN_TIMEOUT)?;
            parse_xbps_updates(&new, &old)
        }
        "flatpak" => {
            let new = exec("flatpak", &["remote-ls", "--updates", "--app", "--columns=application,name,version"], SCAN_TIMEOUT)?;
            let old = exec("flatpak", &["list", "--app", "--columns=application,name,version"], SCAN_TIMEOUT)?;
            parse_flatpak_updates(&new, &old)
        }
        "snap" => {
            let new = exec("snap", &["refresh", "--list"], SCAN_TIMEOUT)?;
            let old = exec("snap", &["list"], SCAN_TIMEOUT)?;
            parse_snap_refresh(&new, &old)
        }
        "nix" => parse_nix_upgrades(&exec("nix", &["upgrade", "--list", "--no-flake"], SCAN_TIMEOUT)?),
        "guix" => parse_guix_upgrades(&exec("guix", &["package", "--list-upgrades"], SCAN_TIMEOUT)?),
        _ => return Err(format!("gestor no soportado: {kind}")),
    })
}

pub fn installed(kind: &str) -> Result<Vec<App>, String> {
    installed_raw(kind).map(|apps| apps.into_iter().map(polish).collect())
}

fn installed_raw(kind: &str) -> Result<Vec<App>, String> {
    Ok(match kind {
        "apt" => {
            let manual = exec("apt-mark", &["showmanual"], SCAN_TIMEOUT)?;
            let all = exec("dpkg-query", &["-W", "-f=${Package}\t${Version}\n"], SCAN_TIMEOUT)?;
            let gui = apt_gui();
            set_gui("apt", gui.iter());
            parse_apt_installed(&manual, &all, &gui)
        }
        "pacman" | "aur" => {
            let (native, aur) = pacman_installed()?;
            if kind == "pacman" { native } else { aur }
        }
        "dnf" => {
            let mut apps = parse_pairs("dnf", &exec("dnf", &["repoquery", "--userinstalled", "-q", "--queryformat", "%{name}\t%{version}-%{release}\n"], SCAN_TIMEOUT)?, Source::Dnf, '\t');
            let gui = rpm_gui();
            set_gui("dnf", gui.iter().map(|(n, _)| n).collect::<Vec<_>>().into_iter());
            let have: HashSet<String> = apps.iter().map(|a| a.name.clone()).collect();
            apps.extend(gui.iter().filter(|(n, _)| !have.contains(n)).map(|(n, v)| app("dnf", n, n, "dnf", v, "", Source::Dnf)));
            apps
        }
        // Everything rpm knows would drown the list, so zypper shows the apps with a launcher.
        "zypper" => {
            let gui = rpm_gui();
            set_gui("zypper", gui.iter().map(|(n, _)| n).collect::<Vec<_>>().into_iter());
            gui.iter().map(|(n, v)| app("zypper", n, n, "zypper", v, "", Source::Zypper)).collect()
        }
        "apk" => parse_apk_installed(&std::fs::read_to_string("/etc/apk/world").unwrap_or_default(), &exec("apk", &["info", "-v"], SCAN_TIMEOUT)?),
        "xbps" => parse_xbps_manual(&exec("xbps-query", &["-m"], SCAN_TIMEOUT)?),
        "flatpak" => parse_flatpak_list(&exec("flatpak", &["list", "--app", "--columns=application,name,version"], SCAN_TIMEOUT)?),
        "snap" => parse_snap_list(&exec("snap", &["list"], SCAN_TIMEOUT)?),
        "nix" => parse_nix_installed(&exec("nix", &["profile", "list", "--install-profile"], SCAN_TIMEOUT)?),
        "guix" => parse_guix_installed(&exec("guix", &["package", "--list-installed"], SCAN_TIMEOUT)?),
        _ => return Err(format!("gestor no soportado: {kind}")),
    })
}

/// Remembers which pacman/AUR packages ship a launcher.
fn refresh_pacman_gui() {
    let foreign = foreign_names();
    let gui = pacman_gui();
    set_gui("pacman", gui.iter().map(|(n, _)| n).filter(|n| !foreign.contains(*n)).collect::<Vec<_>>().into_iter());
    set_gui("aur", gui.iter().map(|(n, _)| n).filter(|n| foreign.contains(*n)).collect::<Vec<_>>().into_iter());
}

fn foreign_names() -> HashSet<String> {
    let out = none_if_silent(exec("pacman", &["-Qm"], SCAN_TIMEOUT)).unwrap_or_default();
    out.lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

/// pacman's explicit packages plus every package with a launcher, split into repository
/// packages and AUR (foreign) ones.
fn pacman_installed() -> Result<(Vec<App>, Vec<App>), String> {
    let explicit = none_if_silent(exec("pacman", &["-Qe"], SCAN_TIMEOUT))?;
    let foreign = foreign_names();
    let gui = pacman_gui();
    refresh_pacman_gui();
    Ok(split_pacman(&explicit, &foreign, &gui))
}

fn split_pacman(explicit: &str, foreign: &HashSet<String>, gui: &[(String, String)]) -> (Vec<App>, Vec<App>) {
    let mut seen = HashSet::new();
    let (mut native, mut aur) = (Vec::new(), Vec::new());
    let listed = explicit.lines().filter_map(|l| l.trim().split_once(' ')).map(|(n, v)| (n.to_string(), v.trim().to_string()));
    for (name, version) in listed.chain(gui.iter().cloned()) {
        if !safe_name(&name) || !seen.insert(name.clone()) {
            continue;
        }
        if foreign.contains(&name) {
            aur.push(app("aur", &name, &name, "AUR", &version, "", Source::Aur));
        } else {
            native.push(app("pacman", &name, &name, "pacman", &version, "", Source::Pacman));
        }
    }
    (native, aur)
}

/// When `pacman -Syu` last finished OK. Every pacman entry of the queue asks for the same full
/// upgrade, and repeating it only re-syncs the mirrors, so it runs once until the next scan.
static SYSTEM_UPGRADED: Mutex<Option<Instant>> = Mutex::new(None);
const SYSTEM_UPGRADE_FRESH: Duration = Duration::from_secs(15 * 60);

fn upgrade_system() -> Result<String, String> {
    // Held for the whole upgrade, so the second queue worker waits and then finds it done.
    let mut last = SYSTEM_UPGRADED.lock().unwrap_or_else(|e| e.into_inner());
    if is_fresh(*last, SYSTEM_UPGRADE_FRESH) {
        return Ok(String::new());
    }
    let out = root(&["pacman", "-Syu", "--noconfirm"])?;
    *last = Some(Instant::now());
    Ok(out)
}

fn is_fresh(at: Option<Instant>, within: Duration) -> bool {
    at.is_some_and(|t| t.elapsed() < within)
}

/// Blocks until the manager finishes; call it from a worker thread.
pub fn upgrade(kind: &str, name: &str) -> Result<(), String> {
    if !safe_name(name) {
        return Err(format!("id no soportado: {kind}:{name}"));
    }
    match kind {
        "apt" => root(&["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "--only-upgrade", "-y", name]),
        // Arch has no partial upgrades and the scan used a fresh database, so sync and upgrade
        // everything; later queue entries then find nothing left to do.
        "pacman" => upgrade_system(),
        "dnf" => root(&["dnf", "upgrade", "-y", name]),
        "zypper" => root(&["zypper", "--non-interactive", "update", name]),
        "apk" => root(&["apk", "add", "-u", name]),
        "xbps" => root(&["xbps-install", "-y", name]),
        // AUR packages build as the user; the helper asks polkit whenever it needs root. It shares
        // the root lock so it never meets a `pacman -Syu` on the database lock.
        "aur" => {
            let helper = aur_helper().ok_or("Falta paru o yay para actualizar paquetes del AUR")?;
            // The helper builds as the user and calls `sudo pacman ...` several times; the bridge
            // sends those calls to the shared root shell, so the password is asked once.
            let bridge = sudobridge::Bridge::start(|argv| root(&argv.iter().map(String::as_str).collect::<Vec<_>>()))?;
            let mut args: Vec<String> = ["-S", "--noconfirm", "--needed"].map(String::from).into();
            args.extend(bridge.flags());
            args.extend(if helper == "paru" { vec!["--skipreview"] } else { vec!["--answerdiff", "None", "--answerclean", "None", "--nodiffmenu", "--noeditmenu"] }.into_iter().map(String::from));
            args.push(name.to_string());
            let mut cmd = Command::new(path(helper).ok_or("Falta paru o yay")?);
            cmd.env("LC_ALL", "C").env(sudobridge::TOKEN_VAR, bridge.token());
            let result = scan::run(cmd, &args.iter().map(String::as_str).collect::<Vec<_>>(), UPGRADE_TIMEOUT);
            bridge.stop();
            result
        }
        "flatpak" => retry_network(|| exec("flatpak", &["update", "-y", "--noninteractive", name], UPGRADE_TIMEOUT)),
        // Through the root shell, so snapd does not ask polkit for a password of its own.
        "snap" => root(&["snap", "refresh", name]),
        "nix" => exec("nix", &["upgrade", name], UPGRADE_TIMEOUT),
        "guix" => exec("guix", &["package", "-u", name], UPGRADE_TIMEOUT),
        _ => Err(format!("id no soportado: {kind}:{name}")),
    }
    .map(|_| ())
}

/// Removes an installed package; blocks, so call it from a worker thread.
pub fn uninstall(kind: &str, name: &str) -> Result<(), String> {
    if !safe_name(name) {
        return Err(format!("id no soportado: {kind}:{name}"));
    }
    match kind {
        "apt" => root(&["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "remove", "-y", name]),
        // `-Rs` also drops the dependencies nothing else needs, like a normal uninstall.
        "pacman" | "aur" => root(&["pacman", "-Rs", "--noconfirm", name]),
        "dnf" => root(&["dnf", "remove", "-y", name]),
        "zypper" => root(&["zypper", "--non-interactive", "remove", name]),
        "apk" => root(&["apk", "del", name]),
        "xbps" => root(&["xbps-remove", "-y", name]),
        "snap" => root(&["snap", "remove", name]),
        "flatpak" => exec("flatpak", &["uninstall", "-y", "--noninteractive", name], UPGRADE_TIMEOUT),
        "nix" => exec("nix", &["remove", name], UPGRADE_TIMEOUT),
        "guix" => exec("guix", &["package", "-r", name], UPGRADE_TIMEOUT),
        _ => Err(format!("id no soportado: {kind}:{name}")),
    }
    .map(|_| ())
}

/// Installs a local package file (Updateeer's own update); blocks, so call it from a worker thread.
pub fn install_file(kind: &str, file: &str) -> Result<(), String> {
    match kind {
        "apt" => root(&["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y", file]),
        "pacman" => root(&["pacman", "-U", "--noconfirm", file]),
        "dnf" => root(&["dnf", "install", "-y", file]),
        "zypper" => root(&["zypper", "--non-interactive", "install", "--allow-unsigned-rpm", file]),
        _ => Err(format!("gestor no soportado: {kind}")),
    }
    .map(|_| ())
}

/// Installs a Flathub app or an apt package; blocks, so call it from a worker thread.
pub fn install(kind: &str, name: &str) -> Result<(), String> {
    if !safe_name(name) {
        return Err(format!("id no soportado: {kind}:{name}"));
    }
    match kind {
        // Flathub may exist both per-user and system-wide, which flatpak refuses to choose in
        // non-interactive mode; always use the user installation (no root needed).
        "flatpak" => {
            let url = "https://dl.flathub.org/repo/flathub.flatpakrepo";
            exec("flatpak", &["remote-add", "--user", "--if-not-exists", "flathub", url], SCAN_TIMEOUT)?;
            retry_network(|| exec("flatpak", &["install", "--user", "-y", "--noninteractive", "flathub", name], UPGRADE_TIMEOUT))
        }
        "apt" => root(&["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y", name]),
        _ => Err(format!("id no soportado: {kind}:{name}")),
    }
    .map(|_| ())
}

/// Discover's online search: Flatpak and apt together.
pub fn search(q: &str) -> Vec<App> {
    let mut out = Vec::new();
    if path("flatpak").is_some()
        && let Ok(text) = exec("flatpak", &["search", "--columns=application,name", q], SCAN_TIMEOUT)
    {
        out.extend(parse_flatpak_search(&text).into_iter().take(15));
    }
    if let Ok(text) = exec("apt-cache", &["search", "--names-only", q], SCAN_TIMEOUT) {
        out.extend(parse_apt_search(&text).into_iter().take(15));
    }
    out.into_iter().map(polish).collect()
}

/// dnf does not print the installed version; ask rpm.
fn fill_rpm_versions(apps: &mut [App]) {
    if apps.is_empty() {
        return;
    }
    let names: Vec<String> = apps.iter().filter_map(|a| a.id.strip_prefix("dnf:")).map(String::from).collect();
    let mut args = vec!["-q", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}\t[%{FILENAMES} ]\n"];
    args.extend(names.iter().map(String::as_str));
    // rpm exits non-zero when one of the names is not installed; the others are still printed.
    let Ok(bin) = path("rpm").ok_or("") else { return };
    let mut cmd = Command::new(bin);
    cmd.env("LC_ALL", "C");
    let out = scan::run(cmd, &args, SCAN_TIMEOUT).unwrap_or_default();
    let mut versions: HashMap<&str, &str> = HashMap::new();
    for l in out.lines() {
        if let Some((n, v)) = l.split_once('\t') {
            versions.entry(n).or_insert(v);
        }
    }
    for a in apps {
        if let Some(v) = a.id.strip_prefix("dnf:").and_then(|n| versions.get(n)) {
            a.from = scan::clean(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(apps: &[App]) -> Vec<(&str, &str, &str)> {
        apps.iter().map(|a| (a.id.as_str(), a.from.as_str(), a.to.as_str())).collect()
    }

    #[test]
    fn apt_upgradable() {
        let out = "Listing...\ncurl/noble-updates,noble-security 8.5.0-2ubuntu10.5 amd64 [upgradable from: 8.5.0-2ubuntu10.4]\nbash/noble-updates 5.2.21-2ubuntu4.1 amd64 [upgradable from: 5.2.21-2ubuntu4]\n";
        assert_eq!(rows(&parse_apt_upgradable(out)), [("apt:curl", "8.5.0-2ubuntu10.4", "8.5.0-2ubuntu10.5"), ("apt:bash", "5.2.21-2ubuntu4", "5.2.21-2ubuntu4.1")]);
    }

    #[test]
    fn apt_manual_only() {
        let none = HashSet::new();
        let apps = parse_apt_installed("git\nvim\n", "git\t1:2.43\nlibc6\t2.39\nvim\t2:9.1\n", &none);
        assert_eq!(rows(&apps), [("apt:git", "1:2.43", ""), ("apt:vim", "2:9.1", "")]);
        // A package with a launcher counts even when it was installed as a dependency.
        let gui: HashSet<String> = ["libc6".to_string()].into();
        assert_eq!(parse_apt_installed("git\n", "git\t1\nlibc6\t2\n", &gui).len(), 2);
    }

    #[test]
    fn launchers_and_aur() {
        assert_eq!(desktops("usr/bin/chrome\nusr/share/applications/google-chrome.desktop\n"), ["google-chrome.desktop"]);
        assert!(desktops("usr/share/applications/\nusr/share/applications/sub/x.desktop\nusr/bin/x\n").is_empty());
        assert_eq!(parse_desktop_meta("[Desktop Entry]\nName=Brave Web Browser\nName[es]=Navegador Brave\nCategories=Network;WebBrowser;\n"), Some(("Navegador Brave".into(), "Network;WebBrowser;".into(), String::new())));
        let a = polish(app("aur", "brave-bin", "brave-bin", "AUR", "1", "", Source::Aur));
        assert_eq!((a.name.as_str(), a.vendor.as_str(), a.id.as_str()), ("Brave", "Navegador de internet", "aur:brave-bin"));
        assert_eq!(resolve_icon("/nonexistent/x.png"), None);
        assert_eq!(resolve_icon(""), None);
        let b = polish(app("pacman", "linux-firmware", "linux-firmware", "pacman", "1", "", Source::Pacman));
        assert_eq!((b.name.as_str(), b.vendor.as_str()), ("Linux Firmware", "pacman"));
        assert_eq!(pacman_dir("google-chrome-126.0.1-1"), Some(("google-chrome".into(), "126.0.1-1".into())));
        let entry = "[Desktop Entry]\nType=Application\nName=Chrome\nName[es]=Cromo\nExec=chrome %U\n";
        assert_eq!(parse_desktop(entry), Some("Chrome".into()));
        assert_eq!(parse_desktop("[Desktop Entry]\nType=Application\nName=X\nNoDisplay=true\n"), None);
        assert_eq!(parse_desktop("[Desktop Entry]\nType=Application\nName=Gmail\nExec=chrome --app-id=abc\n"), None);
        let foreign: HashSet<String> = ["google-chrome".to_string(), "paru".to_string()].into();
        let gui = vec![("google-chrome".to_string(), "126-1".to_string()), ("firefox".to_string(), "127-1".to_string())];
        let (native, aur) = split_pacman("git 2.45-1\nparu 2.0-1\n", &foreign, &gui);
        assert_eq!(rows(&native), [("pacman:git", "2.45-1", ""), ("pacman:firefox", "127-1", "")]);
        assert_eq!(rows(&aur), [("aur:paru", "2.0-1", ""), ("aur:google-chrome", "126-1", "")]);
        assert_eq!(parse_upgrades("aur", Source::Aur, "paru 1.0-1 -> 2.0-1\n")[0].id, "aur:paru");
    }

    #[test]
    fn pacman_lines() {
        let apps = parse_pacman_upgrades("linux 6.9.1.arch1-1 -> 6.9.2.arch1-1\nfirefox 126.0-1 -> 127.0-1\nweird line\n");
        assert_eq!(rows(&apps), [("pacman:linux", "6.9.1.arch1-1", "6.9.2.arch1-1"), ("pacman:firefox", "126.0-1", "127.0-1")]);
        assert_eq!(parse_pairs("pacman", "git 2.45.1-1\n--evil 1\n", Source::Pacman, ' ').len(), 1);
    }

    #[test]
    fn silent_failure_means_no_updates() {
        assert_eq!(none_if_silent(Err(String::new())), Ok(String::new()));
        assert_eq!(none_if_silent(Err("error: boom".into())), Err("error: boom".into()));
    }

    #[test]
    fn a_recent_system_upgrade_is_not_repeated() {
        assert!(!is_fresh(None, Duration::from_secs(60)));
        assert!(is_fresh(Some(Instant::now()), Duration::from_secs(60)));
        assert!(!is_fresh(Some(Instant::now()), Duration::ZERO));
    }

    #[test]
    fn network_failures_are_retried() {
        let mut calls = 0;
        let r = retry_network(|| {
            calls += 1;
            if calls < 3 { Err("error: Failed to install org.gimp.GIMP: While fetching [28] Timeout was reached".into()) } else { Ok("done".into()) }
        });
        assert_eq!((r, calls), (Ok("done".into()), 3));
        let mut calls = 0;
        assert!(retry_network(|| { calls += 1; Err("No space left on device".into()) }).is_err() && calls == 1);
    }

    /// Needs a Debian-like system with a launcher package: `cargo test live_apt_gui -- --ignored`.
    #[test]
    #[ignore]
    fn live_apt_gui() {
        let gui = apt_gui();
        assert!(gui.contains("xterm"), "{gui:?}");
        let apps = installed("apt").unwrap();
        assert!(apps.iter().any(|a| a.name == "xterm" && !a.from.is_empty()));
        assert!(is_gui("apt:xterm"));
    }

    /// Needs an Arch system with a launcher package: `cargo test live_pacman_gui -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_pacman_gui() {
        for kind in ["pacman", "aur"] {
            let apps = installed(kind).unwrap();
            println!("{kind}: {:?}", apps.iter().map(|a| (&a.id, &a.name, &a.vendor)).collect::<Vec<_>>());
        }
        println!("gui: {:?}", pacman_gui());
        let all: Vec<_> = installed("pacman").unwrap().into_iter().chain(installed("aur").unwrap()).collect();
        assert!(all.iter().any(|a| a.name == "Google Chrome"), "Chrome missing");
    }

    #[test]
    fn chrome_from_the_aur_is_found() {
        let db = std::env::temp_dir().join(format!("updateeer-pacman-db-{}", std::process::id()));
        let pkg = db.join("google-chrome-155.0.8059.84-1");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("files"), "%FILES%\nopt/\nopt/google/\nusr/\nusr/bin/\nusr/bin/google-chrome-stable\nusr/share/applications\nusr/share/applications/com.google.Chrome.desktop\nusr/share/applications/google-chrome.desktop\n").unwrap();
        fs::write(db.join("ALPM_DB_VERSION"), "9").unwrap();
        let gui = pacman_gui_in(&db);
        assert_eq!(gui, [("google-chrome".to_string(), "155.0.8059.84-1".to_string())]);
        let foreign: HashSet<String> = ["google-chrome".to_string()].into();
        let (native, aur) = split_pacman("", &foreign, &gui);
        assert_eq!((native.len(), aur.len()), (0, 1));
        let chrome = polish(aur.into_iter().next().unwrap());
        assert_eq!((chrome.id.as_str(), chrome.name.as_str(), chrome.vendor.as_str()), ("aur:google-chrome", "Google Chrome", "Navegador de internet"));
        let _ = fs::remove_dir_all(&db);
    }

    #[test]
    fn zypper_apk_xbps() {
        let z = "Loading repository data...\nS | Repository | Name | Current Version | Available Version | Arch\n--+------------+------+-----------------+-------------------+-------\nv | Main       | curl | 8.0-1.1         | 8.1-1.1           | x86_64\n";
        assert_eq!(rows(&parse_zypper_updates(z)), [("zypper:curl", "8.0-1.1", "8.1-1.1")]);
        assert_eq!(rows(&parse_apk_updates("Installed: Available:\ncurl-8.0.1-r0 < 8.1.0-r0\n")), [("apk:curl", "8.0.1-r0", "8.1.0-r0")]);
        assert_eq!(rows(&parse_apk_installed("curl\nvim=1.0\n", "curl-8.0.1-r0\nmusl-1.2-r0\nvim-9.1-r1\n")), [("apk:curl", "8.0.1-r0", ""), ("apk:vim", "9.1-r1", "")]);
        let installed = "ii firefox-126.0_1 Web browser\nii git-2.45_1 VCS\n";
        assert_eq!(rows(&parse_xbps_updates("firefox-127.0_1 update x86_64 https://r 1 2\ngit-2.45_1 hold x86_64 r 1 2\n", installed)), [("xbps:firefox", "126.0_1", "127.0_1")]);
        assert_eq!(rows(&parse_xbps_manual("git-2.45_1\nnot a package\n")), [("xbps:git", "2.45_1", "")]);
    }

    #[test]
    fn dnf_rows() {
        let out = "Available Upgrades\ncurl.x86_64                 8.6.0-9.fc40      updates\nkernel-core.x86_64   6.9.3-200.fc40   updates\n";
        let apps = parse_dnf_upgrades(out);
        assert_eq!(rows(&apps), [("dnf:curl", "", "8.6.0-9.fc40"), ("dnf:kernel-core", "", "6.9.3-200.fc40")]);
        assert_eq!(parse_pairs("dnf", "git\t2.45.1-1.fc40\n", Source::Dnf, '\t')[0].from, "2.45.1-1.fc40");
    }

    #[test]
    fn flatpak_join_and_dedupe() {
        let new = "org.mozilla.firefox\tFirefox\t127.0\norg.mozilla.firefox\tFirefox\t127.0\ncom.spotify.Client\tSpotify\t1.2.40\n";
        let old = "org.mozilla.firefox\tFirefox\t126.0\ncom.spotify.Client\tSpotify\t1.2.30\n";
        let apps = parse_flatpak_updates(new, old);
        assert_eq!(rows(&apps), [("flatpak:org.mozilla.firefox", "126.0", "127.0"), ("flatpak:com.spotify.Client", "1.2.30", "1.2.40")]);
        assert_eq!(apps[0].vendor, "mozilla");
        assert_eq!(parse_flatpak_list(old).len(), 2);
    }

    #[test]
    fn snap_join_and_plumbing() {
        let list = "Name      Version   Rev    Tracking       Publisher   Notes\ncore22    20240111  1380   latest/stable  canonical✓  base\nsnapd     2.63      21759  latest/stable  canonical✓  snapd\nspotify   1.2.30    75     latest/stable  spotify✓    -\n";
        let refresh = "Name     Version  Rev  Size  Publisher  Notes\nspotify  1.2.40   76   200MB spotify✓   -\n";
        assert_eq!(rows(&parse_snap_list(list)), [("snap:spotify", "1.2.30", "")]);
        assert_eq!(rows(&parse_snap_refresh(refresh, list)), [("snap:spotify", "1.2.30", "1.2.40")]);
    }

    #[test]
    fn search_outputs() {
        let f = parse_flatpak_search("org.mozilla.firefox\tFirefox\ncom.x.Y\tY App\n");
        assert_eq!((f[0].id.as_str(), f[0].name.as_str()), ("flatpak:org.mozilla.firefox", "Firefox"));
        let a = parse_apt_search("firefox-esr - Mozilla Firefox web browser\n-bad - x\nnoise\n");
        assert_eq!((a.len(), a[0].id.as_str(), a[0].vendor.as_str()), (1, "apt:firefox-esr", "Mozilla Firefox web browser"));
    }

    #[test]
    fn bad_names_never_run() {
        assert_eq!(upgrade("apt", "--allow-remove-essential"), Err("id no soportado: apt:--allow-remove-essential".into()));
    }

    #[test]
    fn nix_and_guix_parsers() {
        // Nix upgrades: "nixpkgs.curl 8.5.0 – 8.6.0"
        let nix = "nixpkgs.curl 8.5.0 – 8.6.0\nnixpkgs.firefox 126.0 – 127.0\nbad line\n";
        assert_eq!(rows(&parse_nix_upgrades(nix)), [("nix:curl", "8.5.0", "8.6.0"), ("nix:firefox", "126.0", "127.0")]);
        // Nix installed: header + rows
        let nix_installed = "index  name  version  system\n0  nixpkgs.curl  8.5.0  x86_64-linux\n1  nixpkgs.vim  9.1  x86_64-linux\n";
        assert_eq!(rows(&parse_nix_installed(nix_installed)), [("nix:curl", "8.5.0", ""), ("nix:vim", "9.1", "")]);
        // Guix upgrades: "curl 8.5.0 -> 8.6.0"
        let guix = "curl 8.5.0 -> 8.6.0\nfirefox 126.0 -> 127.0\n";
        assert_eq!(rows(&parse_guix_upgrades(guix)), [("guix:curl", "8.5.0", "8.6.0"), ("guix:firefox", "126.0", "127.0")]);
        // Guix installed: "name version ..."
        let guix_installed = "curl 8.5.0 x86_64-linux\ngit 2.45.1 x86_64-linux\n";
        assert_eq!(rows(&parse_guix_installed(guix_installed)), [("guix:curl", "8.5.0", ""), ("guix:git", "2.45.1", "")]);
    }
}
