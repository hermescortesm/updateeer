//! Linux package managers: apt, pacman, dnf, Flatpak and Snap. Parsing is plain text handling, so
//! it is unit-tested on every OS; running them only makes sense on Linux.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::friendly;
use crate::model::{App, Source};
use crate::scan::{self, SCAN_TIMEOUT, UPGRADE_TIMEOUT};
use crate::winpkg::{app, safe_name};

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
        k => k,
    })
}

/// The AUR helper installed, if any.
fn aur_helper() -> Option<&'static str> {
    ["paru", "yay"].into_iter().find(|h| find(h).is_some())
}

/// Ids of system packages that ship a launcher (graphical apps), per manager prefix.
static GUI: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn set_gui<'a>(prefix: &str, names: impl Iterator<Item = &'a String>) {
    let mut g = GUI.lock().unwrap_or_else(|e| e.into_inner());
    let set = g.get_or_insert_with(HashSet::new);
    set.retain(|id| !id.starts_with(&format!("{prefix}:")));
    set.extend(names.map(|n| format!("{prefix}:{n}")));
}

/// Whether a system package id is known to ship a launcher.
pub fn is_gui(id: &str) -> bool {
    GUI.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.contains(id))
}

/// Launchers (`usr/share/applications/*.desktop`) in a package file list, by file name.
pub fn desktops(files: &str) -> Vec<String> {
    files.lines().filter_map(|l| l.trim().trim_start_matches('/').strip_prefix("usr/share/applications/").filter(|f| f.ends_with(".desktop") && !f.contains('/')).map(str::to_string)).collect()
}

/// Launcher details of installed packages, `group:package` -> (name, categories, icon), filled while
/// the package databases are read.
type Meta = (String, String, String);

static META: Mutex<Option<HashMap<String, Meta>>> = Mutex::new(None);

/// Package managers sharing one package database.
fn group(prefix: &str) -> &str {
    match prefix {
        "aur" => "pacman",
        "dnf" | "zypper" => "rpm",
        p => p,
    }
}

/// `Name` (Spanish when there is one), `Categories` and `Icon` of a launcher file.
pub fn parse_desktop_meta(text: &str) -> Option<Meta> {
    let (mut in_entry, mut name, mut local, mut cats, mut icon) = (false, None, None, String::new(), String::new());
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            match k {
                "Name" => name = Some(v.to_string()),
                k if k.starts_with("Name[es") => local = local.or(Some(v.to_string())),
                "Categories" => cats = v.to_string(),
                "Icon" => icon = v.to_string(),
                _ => {}
            }
        }
    }
    local.or(name).map(|n| (n, cats, icon))
}

/// Remembers the launcher of `package` (read from the first of `files` that exists).
fn note_desktop(prefix: &str, package: &str, files: &[String]) {
    let found = files.iter().find_map(|f| fs::read_to_string(format!("/usr/share/applications/{f}")).ok().as_deref().and_then(parse_desktop_meta));
    if let Some(meta) = found {
        META.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(format!("{}:{package}", group(prefix)), meta);
    }
}

/// Flatpak and Snap export their launchers to fixed folders.
fn exported_desktop(prefix: &str, id: &str) -> Option<Meta> {
    let home = std::env::var("HOME").unwrap_or_default();
    let paths = match prefix {
        "flatpak" => vec![format!("/var/lib/flatpak/exports/share/applications/{id}.desktop"), format!("{home}/.local/share/flatpak/exports/share/applications/{id}.desktop")],
        "snap" => vec![format!("/var/lib/snapd/desktop/applications/{id}_{id}.desktop")],
        _ => return None,
    };
    paths.iter().find_map(|p| fs::read_to_string(p).ok().as_deref().and_then(parse_desktop_meta))
}

/// The PNG icon of an installed app, found from its launcher's `Icon=` entry.
pub fn icon_file(a: &App) -> Option<PathBuf> {
    let (prefix, id) = a.id.split_once(':')?;
    let meta = META.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&format!("{}:{id}", group(prefix))).cloned()).or_else(|| exported_desktop(prefix, id))?;
    resolve_icon(&meta.2)
}

/// An `Icon=` value is a path or a theme icon name; only PNG is read (no SVG renderer here).
fn resolve_icon(icon: &str) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    if icon.starts_with('/') {
        return (icon.ends_with(".png") && fs::metadata(icon).is_ok()).then(|| PathBuf::from(icon));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let roots = ["/usr/share/icons/hicolor".to_string(), format!("{home}/.local/share/icons/hicolor"), "/var/lib/flatpak/exports/share/icons/hicolor".into(), format!("{home}/.local/share/flatpak/exports/share/icons/hicolor")];
    let sizes = ["64x64", "48x48", "96x96", "128x128", "72x72", "256x256", "32x32", "192x192", "512x512"];
    let themed = sizes.iter().flat_map(|sz| roots.iter().map(move |r| PathBuf::from(format!("{r}/{sz}/apps/{icon}.png"))));
    let loose = ["/usr/share/pixmaps", "/usr/share/icons"].iter().map(|d| PathBuf::from(format!("{d}/{icon}.png")));
    themed.chain(loose).find(|p| p.is_file())
}

/// Readable name and short Spanish description instead of the package name: from the launcher
/// when the package has one, from the curated list, or cleaned from the package name.
fn polish(mut a: App) -> App {
    let Some((prefix, id)) = a.id.split_once(':') else { return a };
    if prefix == "direct" {
        return a;
    }
    let meta = META.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&format!("{}:{id}", group(prefix))).cloned()).or_else(|| exported_desktop(prefix, id));
    let known = friendly::known(id);
    // Flatpak and Snap already list a real name; the others only know the package name.
    let own = matches!(prefix, "flatpak" | "snap");
    let name = match (&known, &meta) {
        (Some((n, _)), _) => n.to_string(),
        (None, _) if own => a.name.clone(),
        (None, Some((n, _, _))) => friendly::tidy(n),
        (None, None) => friendly::display(id),
    };
    let generic = ["", "pacman", "AUR", "apt", "dnf", "zypper", "apk", "xbps", "snap", "Flatpak"].contains(&a.vendor.as_str()) || own;
    let desc = known.map(|(_, d)| d).or_else(|| meta.as_ref().and_then(|(_, c, _)| friendly::describe(c)));
    if let Some(d) = desc {
        a.vendor = d.to_string();
    } else if generic && own {
        a.vendor = if prefix == "snap" { "Snap".into() } else { "Flatpak".into() };
    }
    a.hue = scan::hue(&name);
    a.name = name;
    a
}

/// `name-1.2-3` (a pacman database folder) into `(name, "1.2-3")`.
pub fn pacman_dir(dir: &str) -> Option<(String, String)> {
    let mut p = dir.rsplitn(3, '-');
    let (rel, ver, name) = (p.next()?, p.next()?, p.next()?);
    Some((name.to_string(), format!("{ver}-{rel}")))
}

/// Native and AUR packages that ship a launcher: `(name, version)`, from pacman's database.
fn pacman_gui() -> Vec<(String, String)> {
    fs::read_dir("/var/lib/pacman/local")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let (name, version) = pacman_dir(&e.file_name().to_string_lossy())?;
            let launchers = desktops(&fs::read_to_string(e.path().join("files")).ok()?);
            note_desktop("pacman", &name, &launchers);
            (!launchers.is_empty()).then_some((name, version))
        })
        .collect()
}

/// Debian packages that ship a launcher, from dpkg's file lists.
fn apt_gui() -> HashSet<String> {
    fs::read_dir("/var/lib/dpkg/info")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().to_string();
            let name = file.strip_suffix(".list")?.split(':').next()?.to_string();
            let launchers = desktops(&fs::read_to_string(e.path()).ok()?);
            note_desktop("apt", &name, &launchers);
            (!launchers.is_empty()).then_some(name)
        })
        .collect()
}

/// rpm packages that own a launcher in `/usr/share/applications`: `(name, version)`.
fn rpm_gui() -> Vec<(String, String)> {
    let (Some(bin), Ok(dir)) = (find("rpm"), fs::read_dir("/usr/share/applications")) else { return Vec::new() };
    let files: Vec<String> = dir.flatten().map(|e| e.path().to_string_lossy().to_string()).filter(|p| p.ends_with(".desktop")).collect();
    // Files nobody owns make rpm exit with an error but still print the rest, so read stdout anyway.
    let out = Command::new(bin).env("LC_ALL", "C").args(["-qf", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}\t[%{FILENAMES} ]\n"]).args(&files).stdin(std::process::Stdio::null()).output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let mut seen = HashSet::new();
    text.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let (n, v, owned) = (f.next()?, f.next()?, f.next().unwrap_or(""));
            (safe_name(n) && seen.insert(n.to_string())).then(|| {
                let launchers: Vec<String> = owned.split_whitespace().filter_map(|p| p.strip_prefix("/usr/share/applications/")).filter(|f| f.ends_with(".desktop") && !f.contains('/')).map(str::to_string).collect();
                note_desktop("rpm", n, &launchers);
                (n.to_string(), v.to_string())
            })
        })
        .collect()
}

/// The `Name` of a `.desktop` file that is a visible application (not a Chrome web-app shortcut).
pub fn parse_desktop(text: &str) -> Option<String> {
    let (mut in_entry, mut name, mut app, mut hidden, mut web) = (false, None, false, false, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((k, v)) = line.split_once('=') {
            match k {
                "Type" => app = v == "Application",
                "Name" => name = Some(v.to_string()),
                "NoDisplay" | "Hidden" => hidden |= v == "true",
                "Exec" => web |= v.contains("--app-id=") || v.contains("--app="),
                _ => {}
            }
        }
    }
    name.filter(|_| app && !hidden && !web)
}

/// Launchers nobody packaged (AppImages, tarballs, installer scripts): apps no package manager
/// reported. `known` are the apps already found, matched by name.
pub fn direct_apps(known: &[App]) -> Vec<App> {
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/applications"));
    let dirs = [home, Some(PathBuf::from("/usr/local/share/applications"))];
    let mut seen: HashSet<String> = known.iter().flat_map(|a| [a.name.to_lowercase(), a.id.split_once(':').map_or(String::new(), |(_, n)| n.to_lowercase())]).collect();
    let mut apps = Vec::new();
    for dir in dirs.into_iter().flatten() {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Some(stem) = path.file_name().and_then(|f| f.to_str()).and_then(|f| f.strip_suffix(".desktop")) else { continue };
            let Ok(text) = fs::read_to_string(&path) else { continue };
            let Some(name) = parse_desktop(&text) else { continue };
            if seen.contains(&name.to_lowercase()) || seen.contains(&stem.to_lowercase()) {
                continue;
            }
            seen.insert(name.to_lowercase());
            if let Some(meta) = parse_desktop_meta(&text) {
                META.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(format!("direct:{name}"), meta);
            }
            apps.push(app("direct", &name, &name, "Descarga directa", "", "", Source::Direct));
        }
    }
    apps
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

/// One root shell for the whole session: polkit asks for the password once, on the first
/// administrator action, instead of once per update. Holding the lock also serializes the
/// system package managers, which cannot run two at a time.
struct RootShell {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

static ROOT: Mutex<Option<RootShell>> = Mutex::new(None);
const DONE: &str = "__UPDATEEER_DONE__";

fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

fn start_root() -> Result<RootShell, String> {
    let bin = find("pkexec").ok_or("Falta pkexec (polkit) para pedir permisos de administrador")?;
    spawn_shell(Command::new(bin).arg("/bin/sh"))
}

fn spawn_shell(cmd: &mut Command) -> Result<RootShell, String> {
    let mut child = cmd
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().ok_or("sin entrada")?;
    let (tx, lines) = mpsc::channel();
    for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
        let tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    Ok(RootShell { child, stdin, lines })
}

/// Runs a command as root through the shared shell. Cancelling or timing out drops the shell
/// (the next action asks for the password again).
fn root(args: &[&str]) -> Result<String, String> {
    let mut guard = ROOT.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_mut().is_some_and(|sh| sh.child.try_wait().ok().flatten().is_some()) {
        *guard = None;
    }
    if guard.is_none() {
        *guard = Some(start_root()?);
    }
    let result = run_in(guard.as_mut().expect("just started"), args);
    if result.1
        && let Some(mut sh) = guard.take()
    {
        let _ = sh.child.kill();
        let _ = sh.child.wait();
    }
    result.0
}

/// Result plus whether the shell must be thrown away.
struct Ran(Result<String, String>, bool);

fn run_in(sh: &mut RootShell, args: &[&str]) -> Ran {
    let generation = scan::CANCEL_GEN.load(Ordering::SeqCst);
    let cmd = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    if writeln!(sh.stdin, "{{ {cmd}; }} 2>&1 </dev/null; echo {DONE}$?").and_then(|_| sh.stdin.flush()).is_err() {
        return Ran(Err("No se pudo contactar al shell de administrador".into()), true);
    }
    let deadline = Instant::now() + UPGRADE_TIMEOUT;
    let mut out = Vec::new();
    loop {
        if scan::CANCEL_GEN.load(Ordering::SeqCst) != generation {
            return Ran(Err("Cancelado".into()), true);
        }
        if Instant::now() >= deadline {
            return Ran(Err("El comando tardó demasiado y se canceló".into()), true);
        }
        match sh.lines.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => match line.strip_prefix(DONE) {
                Some("0") => return Ran(Ok(out.join("\n")), false),
                Some(_) => return Ran(Err(scan::tail(&out.join("\n"))), false),
                None => out.push(line),
            },
            Err(RecvTimeoutError::Timeout) => {}
            // pkexec ended: password refused, dialog dismissed or polkit missing.
            Err(RecvTimeoutError::Disconnected) => {
                let why = scan::tail(&out.join("\n"));
                return Ran(Err(if why.is_empty() { "No se obtuvieron permisos de administrador".into() } else { why }), true);
            }
        }
    }
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
            refresh_pacman_gui();
            let out = if path("checkupdates").is_some() { exec("checkupdates", &[], UPGRADE_TIMEOUT) } else { exec("pacman", &["-Qu"], SCAN_TIMEOUT) };
            parse_pacman_upgrades(&none_if_silent(out)?)
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

/// Blocks until the manager finishes; call it from a worker thread.
pub fn upgrade(kind: &str, name: &str) -> Result<(), String> {
    if !safe_name(name) {
        return Err(format!("id no soportado: {kind}:{name}"));
    }
    match kind {
        "apt" => root(&["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "--only-upgrade", "-y", name]),
        // Arch has no partial upgrades and the scan used a fresh database, so sync and upgrade
        // everything; later queue entries then find nothing left to do.
        "pacman" => root(&["pacman", "-Syu", "--noconfirm"]),
        "dnf" => root(&["dnf", "upgrade", "-y", name]),
        "zypper" => root(&["zypper", "--non-interactive", "update", name]),
        "apk" => root(&["apk", "add", "-u", name]),
        "xbps" => root(&["xbps-install", "-y", name]),
        // AUR packages build as the user; the helper asks polkit whenever it needs root. It shares
        // the root lock so it never meets a `pacman -Syu` on the database lock.
        "aur" => {
            let helper = aur_helper().ok_or("Falta paru o yay para actualizar paquetes del AUR")?;
            let mut args = vec!["-S", "--noconfirm", "--needed", "--sudo", "/usr/bin/pkexec"];
            args.extend(if helper == "paru" { vec!["--skipreview"] } else { vec!["--answerdiff", "None", "--answerclean", "None", "--nodiffmenu", "--noeditmenu"] });
            args.push(name);
            let _guard = ROOT.lock().unwrap_or_else(|e| e.into_inner());
            exec(helper, &args, UPGRADE_TIMEOUT)
        }
        "flatpak" => retry_network(|| exec("flatpak", &["update", "-y", "--noninteractive", name], UPGRADE_TIMEOUT)),
        // snapd asks polkit for the permission by itself.
        "snap" => exec("snap", &["refresh", name], UPGRADE_TIMEOUT),
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

/// `flatpak search --columns=application,name`: tab separated.
pub fn parse_flatpak_search(out: &str) -> Vec<App> {
    parse_flatpak_rows(out).into_iter().map(|(id, name, _)| app("flatpak", &id, &name, flatpak_vendor(&id), "", "", Source::Flatpak)).collect()
}

/// `apt-cache search --names-only`: `name - description`.
pub fn parse_apt_search(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let (name, desc) = l.split_once(" - ")?;
            safe_name(name).then(|| app("apt", name, name, desc, "", "", Source::Apt))
        })
        .collect()
}

/// Reverse-DNS ids (`org.mozilla.firefox`): the second part names the publisher.
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

fn flatpak_vendor(id: &str) -> &str {
    id.split('.').nth(1).filter(|_| id.matches('.').count() >= 2).unwrap_or("Flatpak")
}

/// `apt list --upgradable`: `name/suite new-version arch [upgradable from: old-version]`.
pub fn parse_apt_upgradable(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once('/')?;
            let to = rest.split_whitespace().nth(1)?;
            let from = rest.split_once("upgradable from: ")?.1.trim_end_matches(']').trim();
            safe_name(name).then(|| app("apt", name, name, "apt", from, to, Source::Apt))
        })
        .collect()
}

/// Manually installed packages (`apt-mark showmanual`) with their versions from `dpkg-query`.
pub fn parse_apt_installed(manual: &str, all: &str, gui: &HashSet<String>) -> Vec<App> {
    let manual: HashSet<&str> = manual.lines().map(str::trim).collect();
    all.lines()
        .filter_map(|l| {
            let (name, version) = l.split_once('\t')?;
            ((manual.contains(name) || gui.contains(name)) && safe_name(name)).then(|| app("apt", name, name, "apt", version, "", Source::Apt))
        })
        .collect()
}

/// `pacman -Qu`: `name old -> new`.
pub fn parse_pacman_upgrades(out: &str) -> Vec<App> {
    parse_upgrades("pacman", Source::Pacman, out)
}

/// The same `name old -> new` lines from pacman, `checkupdates` or an AUR helper.
pub fn parse_upgrades(prefix: &str, source: Source, out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            let [name, from, "->", to, ..] = t[..] else { return None };
            safe_name(name).then(|| app(prefix, name, name, prefix, from, to, source))
        })
        .collect()
}

/// `name version` or `name<TAB>version` per line.
pub fn parse_pairs(prefix: &str, out: &str, source: Source, sep: char) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let (name, version) = l.trim().split_once(sep)?;
            let name = name.trim();
            safe_name(name).then(|| app(prefix, name, name, prefix, version.trim(), "", source))
        })
        .collect()
}

/// `zypper list-updates`: `S | Repository | Name | Current | Available | Arch` rows.
pub fn parse_zypper_updates(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split('|').map(str::trim).collect();
            let [status, _repo, name, from, to, _arch] = c[..] else { return None };
            (status.len() == 1 && status != "S" && safe_name(name)).then(|| app("zypper", name, name, "zypper", from, to, Source::Zypper))
        })
        .collect()
}

/// `name-1.2.3-r0` into `(name, "1.2.3-r0")`.
fn apk_split(full: &str) -> Option<(&str, String)> {
    let mut p = full.trim().rsplitn(3, '-');
    let (rel, ver, name) = (p.next()?, p.next()?, p.next()?);
    ver.starts_with(|c: char| c.is_ascii_digit()).then(|| (name, format!("{ver}-{rel}")))
}

/// `apk version -l '<'`: `name-1.0-r0 < 1.1-r0`.
pub fn parse_apk_updates(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let (old, new) = l.split_once(" < ")?;
            let (name, from) = apk_split(old)?;
            safe_name(name).then(|| app("apk", name, name, "apk", &from, new.trim(), Source::Apk))
        })
        .collect()
}

/// `/etc/apk/world` (what the user asked for) joined with `apk info -v` (`name-version` lines).
pub fn parse_apk_installed(world: &str, info: &str) -> Vec<App> {
    let wanted: HashSet<&str> = world.split_whitespace().map(|w| w.split(['=', '<', '>', '~']).next().unwrap_or(w)).collect();
    info.lines()
        .filter_map(|l| {
            let (name, ver) = apk_split(l)?;
            (wanted.contains(name) && safe_name(name)).then(|| app("apk", name, name, "apk", &ver, "", Source::Apk))
        })
        .collect()
}

/// `xbps-query -l` rows are `ii name-1.0_1  description`.
fn xbps_split(full: &str) -> Option<(&str, &str)> {
    let (name, ver) = full.rsplit_once('-')?;
    ver.starts_with(|c: char| c.is_ascii_digit()).then_some((name, ver))
}

/// `xbps-install -un` rows (`name-2.0_1 update x86_64 repo ...`) against the installed list.
pub fn parse_xbps_updates(new: &str, installed: &str) -> Vec<App> {
    let old: HashMap<&str, &str> = installed.lines().filter_map(|l| xbps_split(l.split_whitespace().nth(1)?)).collect();
    new.lines()
        .filter_map(|l| {
            let mut t = l.split_whitespace();
            let (name, to) = xbps_split(t.next()?)?;
            (t.next()? == "update" && safe_name(name)).then(|| app("xbps", name, name, "xbps", old.get(name).copied().unwrap_or(""), to, Source::Xbps))
        })
        .collect()
}

/// `xbps-query -m` (manually installed): `name-1.0_1` per line.
pub fn parse_xbps_manual(out: &str) -> Vec<App> {
    out.lines().filter_map(|l| xbps_split(l.trim())).filter(|(n, _)| safe_name(n)).map(|(n, v)| app("xbps", n, n, "xbps", v, "", Source::Xbps)).collect()
}

/// `dnf list --upgrades`: `name.arch  version  repo` rows after an "Available Upgrades" header.
pub fn parse_dnf_upgrades(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            let [name_arch, to, _repo] = t[..] else { return None };
            let name = name_arch.rsplit_once('.')?.0;
            safe_name(name).then(|| app("dnf", name, name, "dnf", "", to, Source::Dnf))
        })
        .collect()
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

fn parse_flatpak_rows(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter_map(|l| {
            let mut c = l.split('\t');
            let (id, name, version) = (c.next()?.trim(), c.next()?.trim(), c.next().unwrap_or("").trim());
            (id.contains('.') && safe_name(id)).then(|| (id.to_string(), name.to_string(), version.to_string()))
        })
        .collect()
}

pub fn parse_flatpak_list(out: &str) -> Vec<App> {
    parse_flatpak_rows(out).into_iter().map(|(id, name, v)| app("flatpak", &id, &name, flatpak_vendor(&id), &v, "", Source::Flatpak)).collect()
}

/// `remote-ls --updates` rows carry the new version; `list` rows the installed one.
pub fn parse_flatpak_updates(new: &str, installed: &str) -> Vec<App> {
    let old: HashMap<String, String> = parse_flatpak_rows(installed).into_iter().map(|(id, _, v)| (id, v)).collect();
    let mut seen = HashSet::new();
    parse_flatpak_rows(new)
        .into_iter()
        .filter(|(id, ..)| seen.insert(id.clone()))
        .map(|(id, name, to)| {
            let from = old.get(&id).map_or("", String::as_str);
            app("flatpak", &id, &name, flatpak_vendor(&id), from, &to, Source::Flatpak)
        })
        .collect()
}

/// `snap list` rows: `Name Version Rev Tracking Publisher Notes`. Base snaps and snapd are plumbing.
fn snap_rows(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.len() < 6 || matches!(t.last(), Some(n) if n.contains("base") || n.contains("snapd")) {
                return None;
            }
            // The publisher can carry a verified mark in a separate token.
            Some((t[0].to_string(), t[1].to_string(), t[4].trim_end_matches(['✓', '*']).to_string()))
        })
        .collect()
}

pub fn parse_snap_list(out: &str) -> Vec<App> {
    snap_rows(out).into_iter().filter(|(n, ..)| safe_name(n)).map(|(n, v, p)| app("snap", &n, &n, &p, &v, "", Source::Snap)).collect()
}

/// `snap refresh --list` rows: `Name Version Rev Size Publisher Notes`.
pub fn parse_snap_refresh(new: &str, installed: &str) -> Vec<App> {
    let old: HashMap<String, String> = snap_rows(installed).into_iter().map(|(n, v, _)| (n, v)).collect();
    new.lines()
        .skip(1)
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.len() < 5 || !safe_name(t[0]) {
                return None;
            }
            let from = old.get(t[0]).map_or("", String::as_str);
            Some(app("snap", t[0], t[0], t[4].trim_end_matches(['✓', '*']), from, t[1], Source::Snap))
        })
        .collect()
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

    #[cfg(unix)]
    #[test]
    fn one_shell_runs_many_commands() {
        let mut sh = spawn_shell(&mut Command::new("/bin/sh")).unwrap();
        assert_eq!(run_in(&mut sh, &["echo", "it's", "one"]).0, Ok("it's one".into()));
        assert_eq!(run_in(&mut sh, &["sh", "-c", "echo boom >&2; exit 3"]).0, Err("boom".into()));
        assert_eq!(run_in(&mut sh, &["echo", "still", "alive"]).0, Ok("still alive".into()));
        let _ = sh.child.kill();
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
}
