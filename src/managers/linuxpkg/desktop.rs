//! Which Linux packages are apps with a window: launchers (`.desktop` files), their names,
//! descriptions and icons, and apps dropped in by hand.
use super::*;

/// Ids of system packages that ship a launcher (graphical apps), per manager prefix.
pub(super) static GUI: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub(super) fn set_gui<'a>(prefix: &str, names: impl Iterator<Item = &'a String>) {
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
pub(super) type Meta = (String, String, String);

pub(super) static META: Mutex<Option<HashMap<String, Meta>>> = Mutex::new(None);

/// Package managers sharing one package database.
pub(super) fn group(prefix: &str) -> &str {
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
pub(super) fn note_desktop(prefix: &str, package: &str, files: &[String]) {
    let found = files.iter().find_map(|f| fs::read_to_string(format!("/usr/share/applications/{f}")).ok().as_deref().and_then(parse_desktop_meta));
    if let Some(meta) = found {
        META.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(format!("{}:{package}", group(prefix)), meta);
    }
}

/// Flatpak and Snap export their launchers to fixed folders.
pub(super) fn exported_desktop(prefix: &str, id: &str) -> Option<Meta> {
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
pub(super) fn resolve_icon(icon: &str) -> Option<PathBuf> {
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
pub(super) fn polish(mut a: App) -> App {
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
    let generic = ["", "pacman", "AUR", "apt", "dnf", "zypper", "apk", "xbps", "snap", "Flatpak", "Nix", "Guix"].contains(&a.vendor.as_str()) || own;
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
pub(super) fn pacman_gui() -> Vec<(String, String)> {
    pacman_gui_in(Path::new("/var/lib/pacman/local"))
}

pub(super) fn pacman_gui_in(db: &Path) -> Vec<(String, String)> {
    fs::read_dir(db)
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
pub(super) fn apt_gui() -> HashSet<String> {
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
pub(super) fn rpm_gui() -> Vec<(String, String)> {
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
