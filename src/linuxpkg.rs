//! Linux package managers: apt, pacman, dnf, Flatpak and Snap. Parsing is plain text handling, so
//! it is unit-tested on every OS; running them only makes sense on Linux.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use crate::model::{App, Source};
use crate::scan::{self, SCAN_TIMEOUT, UPGRADE_TIMEOUT};
use crate::winpkg::{app, safe_name};

/// System package managers hold a global lock: two upgrades at once would fail one of them.
static SYSTEM_LOCK: Mutex<()> = Mutex::new(());

/// First `name` found in the usual binary directories.
fn find(name: &str) -> Option<PathBuf> {
    ["/usr/bin", "/bin", "/usr/local/bin", "/snap/bin", "/var/lib/flatpak/exports/bin"].iter().map(|d| PathBuf::from(d).join(name)).find(|p| p.is_file())
}

pub fn path(kind: &str) -> Option<PathBuf> {
    find(match kind {
        "apt" => "apt-get",
        k => k,
    })
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

/// `pkexec` asks for the password through the desktop's polkit dialog.
fn root(args: &[&str]) -> Result<String, String> {
    let _guard = SYSTEM_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let bin = find("pkexec").ok_or("Falta pkexec (polkit) para pedir permisos de administrador")?;
    let mut cmd = Command::new(bin);
    cmd.env("LC_ALL", "C");
    scan::run(cmd, args, UPGRADE_TIMEOUT)
}

pub fn outdated(kind: &str) -> Result<Vec<App>, String> {
    Ok(match kind {
        "apt" => parse_apt_upgradable(&exec("apt", &["list", "--upgradable"], SCAN_TIMEOUT)?),
        // `-Qu` only knows the last synced database; `checkupdates` (pacman-contrib) syncs a private
        // copy first. Both exit non-zero with no output when nothing is outdated.
        "pacman" => {
            let out = if path("checkupdates").is_some() { exec("checkupdates", &[], UPGRADE_TIMEOUT) } else { exec("pacman", &["-Qu"], SCAN_TIMEOUT) };
            parse_pacman_upgrades(&none_if_silent(out)?)
        }
        "dnf" => {
            let mut apps = parse_dnf_upgrades(&exec("dnf", &["list", "--upgrades", "-q"], SCAN_TIMEOUT)?);
            fill_rpm_versions(&mut apps);
            apps
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
    Ok(match kind {
        "apt" => {
            let manual = exec("apt-mark", &["showmanual"], SCAN_TIMEOUT)?;
            let all = exec("dpkg-query", &["-W", "-f=${Package}\t${Version}\n"], SCAN_TIMEOUT)?;
            parse_apt_installed(&manual, &all)
        }
        "pacman" => parse_pairs("pacman", &exec("pacman", &["-Qe"], SCAN_TIMEOUT)?, Source::Pacman, ' '),
        "dnf" => parse_pairs("dnf", &exec("dnf", &["repoquery", "--userinstalled", "-q", "--queryformat", "%{name}\t%{version}-%{release}\n"], SCAN_TIMEOUT)?, Source::Dnf, '\t'),
        "flatpak" => parse_flatpak_list(&exec("flatpak", &["list", "--app", "--columns=application,name,version"], SCAN_TIMEOUT)?),
        "snap" => parse_snap_list(&exec("snap", &["list"], SCAN_TIMEOUT)?),
        _ => return Err(format!("gestor no soportado: {kind}")),
    })
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
        "flatpak" => exec("flatpak", &["update", "-y", "--noninteractive", name], UPGRADE_TIMEOUT),
        // snapd asks polkit for the permission by itself.
        "snap" => exec("snap", &["refresh", name], UPGRADE_TIMEOUT),
        _ => Err(format!("id no soportado: {kind}:{name}")),
    }
    .map(|_| ())
}

/// Installs a Flathub app or an apt package; blocks, so call it from a worker thread.
pub fn install(kind: &str, name: &str) -> Result<(), String> {
    if !safe_name(name) {
        return Err(format!("id no soportado: {kind}:{name}"));
    }
    match kind {
        "flatpak" => exec("flatpak", &["install", "-y", "--noninteractive", "flathub", name], UPGRADE_TIMEOUT),
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
    out
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
pub fn parse_apt_installed(manual: &str, all: &str) -> Vec<App> {
    let manual: HashSet<&str> = manual.lines().map(str::trim).collect();
    all.lines()
        .filter_map(|l| {
            let (name, version) = l.split_once('\t')?;
            (manual.contains(name) && safe_name(name)).then(|| app("apt", name, name, "apt", version, "", Source::Apt))
        })
        .collect()
}

/// `pacman -Qu`: `name old -> new`.
pub fn parse_pacman_upgrades(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            let [name, from, "->", to, ..] = t[..] else { return None };
            safe_name(name).then(|| app("pacman", name, name, "pacman", from, to, Source::Pacman))
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
    let mut args = vec!["-q", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}\n"];
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
        let apps = parse_apt_installed("git\nvim\n", "git\t1:2.43\nlibc6\t2.39\nvim\t2:9.1\n");
        assert_eq!(rows(&apps), [("apt:git", "1:2.43", ""), ("apt:vim", "2:9.1", "")]);
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
