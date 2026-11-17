//! Output parsers for every Linux package manager. Pure text handling, tested on every OS.
use super::*;

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
pub(super) fn flatpak_vendor(id: &str) -> &str {
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
pub(super) fn apk_split(full: &str) -> Option<(&str, String)> {
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
pub(super) fn xbps_split(full: &str) -> Option<(&str, &str)> {
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

pub(super) fn parse_flatpak_rows(out: &str) -> Vec<(String, String, String)> {
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
pub(super) fn snap_rows(out: &str) -> Vec<(String, String, String)> {
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

/// `nix upgrade --list --no-flake` rows: `name old-version – new-version`.
pub fn parse_nix_upgrades(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            // "nixpkgs.curl 8.5.0 – 8.6.0" or "curl 8.5.0 – 8.6.0"
            let parts: Vec<&str> = l.split('–').collect();
            if parts.len() != 2 {
                return None;
            }
            let left: Vec<&str> = parts[0].trim().rsplitn(2, ' ').collect();
            let to = parts[1].trim();
            if left.len() != 2 {
                return None;
            }
            let from = left[0];
            let full = left[1];
            // Strip nixpkgs. prefix if present
            let name = full.strip_prefix("nixpkgs.").unwrap_or(full);
            safe_name(name).then(|| app("nix", name, name, "Nix", from, to, Source::Nix))
        })
        .collect()
}

/// `nix profile list --install-profile` rows: `index  name  version  system`.
pub fn parse_nix_installed(out: &str) -> Vec<App> {
    out.lines()
        .skip(1) // skip header
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.len() < 3 {
                return None;
            }
            let full = t[1];
            let version = t[2];
            let name = full.strip_prefix("nixpkgs.").unwrap_or(full);
            safe_name(name).then(|| app("nix", name, name, "Nix", version, "", Source::Nix))
        })
        .collect()
}

/// `guix package --list-upgrades` rows: `name version … -> new-version …`.
pub fn parse_guix_upgrades(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.splitn(2, " -> ").collect();
            if parts.len() != 2 {
                return None;
            }
            let left: Vec<&str> = parts[0].trim().split_whitespace().collect();
            let to = parts[1].trim().split_whitespace().next()?;
            if left.is_empty() {
                return None;
            }
            let name = left[0];
            let from = if left.len() > 1 { left[1] } else { "" };
            safe_name(name).then(|| app("guix", name, name, "Guix", from, to, Source::Guix))
        })
        .collect()
}

/// `guix package --list-installed` rows: `name version …`.
pub fn parse_guix_installed(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if t.len() < 2 {
                return None;
            }
            let name = t[0];
            let version = t[1];
            safe_name(name).then(|| app("guix", name, name, "Guix", version, "", Source::Guix))
        })
        .collect()
}
