//! Windows package managers: winget, Scoop and Chocolatey. Parsing is plain text handling, so it
//! is unit-tested on every OS; running them only makes sense on Windows.
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{App, Source};
use crate::scan::{self, SCAN_TIMEOUT, UPGRADE_TIMEOUT};

/// First of `names` found in PATH or in one of `extra`.
fn find_exe(names: &[&str], extra: &[PathBuf]) -> Option<PathBuf> {
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    path_dirs.iter().chain(extra).flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

fn env_dir(var: &str, sub: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|v| Path::new(&v).join(sub))
}

pub fn winget_path() -> Option<PathBuf> {
    find_exe(&["winget.exe"], &env_dir("LOCALAPPDATA", "Microsoft/WindowsApps").into_iter().collect::<Vec<_>>())
}

pub fn scoop_path() -> Option<PathBuf> {
    find_exe(&["scoop.cmd", "scoop.exe"], &env_dir("USERPROFILE", "scoop/shims").into_iter().collect::<Vec<_>>())
}

pub fn choco_path() -> Option<PathBuf> {
    find_exe(&["choco.exe"], &env_dir("ProgramData", "chocolatey/bin").into_iter().collect::<Vec<_>>())
}

fn exec(path: Option<PathBuf>, args: &[&str], timeout: std::time::Duration) -> Result<String, String> {
    let path = path.ok_or("No instalado")?;
    scan::run(Command::new(path), args, timeout)
}

const WINGET_FLAGS: [&str; 2] = ["--accept-source-agreements", "--disable-interactivity"];

pub fn winget_outdated() -> Result<Vec<App>, String> {
    let mut args = vec!["upgrade"];
    args.extend(WINGET_FLAGS);
    Ok(parse_winget(&exec(winget_path(), &args, SCAN_TIMEOUT)?, true))
}

pub fn winget_installed() -> Result<Vec<App>, String> {
    let mut args = vec!["list"];
    args.extend(WINGET_FLAGS);
    Ok(parse_winget(&exec(winget_path(), &args, SCAN_TIMEOUT)?, false))
}

pub fn scoop_outdated() -> Result<Vec<App>, String> {
    Ok(parse_scoop(&exec(scoop_path(), &["status"], SCAN_TIMEOUT)?, true))
}

pub fn scoop_installed() -> Result<Vec<App>, String> {
    Ok(parse_scoop(&exec(scoop_path(), &["list"], SCAN_TIMEOUT)?, false))
}

pub fn choco_outdated() -> Result<Vec<App>, String> {
    Ok(parse_choco_outdated(&exec(choco_path(), &["outdated", "-r"], SCAN_TIMEOUT)?))
}

pub fn choco_installed() -> Result<Vec<App>, String> {
    Ok(parse_choco_list(&exec(choco_path(), &["list", "-l", "-r"], SCAN_TIMEOUT)?))
}

/// Installs a winget package; blocks, so call it from a worker thread.
pub fn install(name: &str) -> Result<(), String> {
    if !safe_name(name) {
        return Err(format!("id no soportado: winget:{name}"));
    }
    exec(
        winget_path(),
        &["install", "--id", name, "--exact", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"],
        UPGRADE_TIMEOUT,
    )
    .map(|_| ())
}

/// Blocks until the manager finishes; call it from a worker thread.
pub fn upgrade(kind: &str, name: &str) -> Result<(), String> {
    if !safe_name(name) {
        return Err(format!("id no soportado: {kind}:{name}"));
    }
    match kind {
        "winget" => exec(
            winget_path(),
            &["upgrade", "--id", name, "--exact", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"],
            UPGRADE_TIMEOUT,
        ),
        "scoop" => exec(scoop_path(), &["update", name], UPGRADE_TIMEOUT),
        "choco" => exec(choco_path(), &["upgrade", name, "-y", "--no-progress"], UPGRADE_TIMEOUT),
        _ => Err(format!("id no soportado: {kind}:{name}")),
    }
    .map(|_| ())
}

/// Package ids/names: letters, digits and `. _ + - @ /`; never a leading dash.
pub fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.starts_with('-') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+-@/".contains(&b)) && !s.contains("..")
}

pub fn app(prefix: &str, id: &str, name: &str, vendor: &str, from: &str, to: &str, source: Source) -> App {
    let name = scan::clean(name);
    App {
        id: format!("{prefix}:{id}"),
        hue: scan::hue(&name),
        name,
        vendor: scan::clean(vendor),
        // winget prints approximate versions as `< 1.2`.
        from: scan::clean(from.trim_start_matches(['<', '>', '~', ' '])),
        to: scan::clean(to),
        source,
        size_mb: None,
        bundle: None,
        installed_at: None,
    }
}

/// Removes ANSI colors and the spinner frames winget/scoop print with `\r`.
fn plain(line: &str) -> String {
    let line = line.rsplit('\r').next().unwrap_or(line);
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out.trim_end().to_string()
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 4 && t.chars().all(|c| c == '-' || c == ' ')
}

/// Column start offsets (in chars) of a table header: a word that follows two or more spaces.
/// Header names are localized, their positions are not.
fn column_starts(header: &str) -> Vec<usize> {
    let chars: Vec<char> = header.chars().collect();
    (0..chars.len()).filter(|&i| chars[i] != ' ' && (i == 0 || (i >= 2 && chars[i - 1] == ' ' && chars[i - 2] == ' ') || (i == 1 && chars[0] == ' '))).collect()
}

fn cell(line: &[char], from: usize, to: Option<usize>) -> String {
    let end = to.unwrap_or(line.len()).min(line.len());
    if from >= end { String::new() } else { line[from..end].iter().collect::<String>().trim().to_string() }
}

/// Every table in winget's output, as rows of cells. A row needs a non-empty second column, which
/// is what tells data rows from footers such as `3 upgrades available.`
fn winget_tables(out: &str) -> Vec<Vec<String>> {
    let lines: Vec<String> = out.lines().map(plain).collect();
    let mut rows = Vec::new();
    let mut i = 1;
    while i < lines.len() {
        if is_rule(&lines[i]) && !lines[i - 1].trim().is_empty() {
            let starts = column_starts(&lines[i - 1]);
            if starts.len() >= 3 {
                let mut j = i + 1;
                while j < lines.len() && !lines[j].trim().is_empty() {
                    let chars: Vec<char> = lines[j].chars().collect();
                    let row: Vec<String> = starts.iter().enumerate().map(|(k, &s)| cell(&chars, s, starts.get(k + 1).copied())).collect();
                    if !row[1].is_empty() && !is_rule(&lines[j]) {
                        rows.push(row);
                    }
                    j += 1;
                }
                i = j;
            }
        }
        i += 1;
    }
    rows
}

/// `winget upgrade` (outdated = true) or `winget list` rows: Name, Id, Version, [Available], [Source].
pub fn parse_winget(out: &str, outdated: bool) -> Vec<App> {
    winget_tables(out)
        .into_iter()
        .filter_map(|r| {
            let (name, id, version) = (&r[0], &r[1], &r[2]);
            if !safe_name(id) || name.is_empty() {
                return None;
            }
            // Columns: Name Id Version Available Source. `list` may omit Available.
            let available = if r.len() >= 5 { r[3].as_str() } else { "" };
            let vendor = id.split('.').next().filter(|_| id.contains('.')).unwrap_or("winget");
            if outdated {
                (!available.is_empty()).then(|| app("winget", id, name, vendor, version, available, Source::Winget))
            } else {
                Some(app("winget", id, name, vendor, version, "", Source::Winget))
            }
        })
        .collect()
}

/// `scoop status` (outdated = true: Name, Installed, Latest) or `scoop list` (Name, Version).
pub fn parse_scoop(out: &str, outdated: bool) -> Vec<App> {
    let lines: Vec<String> = out.lines().map(plain).collect();
    let Some(rule) = lines.iter().position(|l| is_rule(l)) else { return Vec::new() };
    lines[rule + 1..]
        .iter()
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let mut t = l.split_whitespace();
            let (name, version) = (t.next()?, t.next()?);
            if !safe_name(name) {
                return None;
            }
            if outdated {
                let latest = t.next()?;
                Some(app("scoop", name, name, "Scoop", version, latest, Source::Scoop))
            } else {
                Some(app("scoop", name, name, "Scoop", version, "", Source::Scoop))
            }
        })
        .collect()
}

/// `choco outdated -r`: `name|current|available|pinned`.
pub fn parse_choco_outdated(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.trim().split('|').collect();
            let [name, from, to, pinned] = f[..] else { return None };
            (safe_name(name) && pinned != "true" && from != to).then(|| app("choco", name, name, "Chocolatey", from, to, Source::Chocolatey))
        })
        .collect()
}

/// `choco list -l -r`: `name|version`.
pub fn parse_choco_list(out: &str) -> Vec<App> {
    out.lines()
        .filter_map(|l| {
            let (name, version) = l.trim().split_once('|')?;
            safe_name(name).then(|| app("choco", name, name, "Chocolatey", version, "", Source::Chocolatey))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINGET_EN: &str = "\r   - \r   \\ \r                                                         \r\
Name                 Id                    Version       Available     Source\n\
-----------------------------------------------------------------------------\n\
Git                  Git.Git               2.45.1        2.46.0        winget\n\
7-Zip 23.01 (x64)    7zip.7zip             23.01         24.07         winget\n\
Microsoft Edge       Microsoft.Edge        125.0.2535.51 126.0.2592.56 winget\n\
3 upgrades available.\n";

    const WINGET_ES: &str = "Nombre              Id                  Versión   Disponible  Origen\n\
------------------------------------------------------------------\n\
Visual Studio Code  Microsoft.VisualStudioCode  1.90.0  1.91.1  winget\n\
1 actualizaciones disponibles.\n";

    #[test]
    fn winget_table_english_with_spinner_and_footer() {
        let apps = parse_winget(WINGET_EN, true);
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["winget:Git.Git", "winget:7zip.7zip", "winget:Microsoft.Edge"]);
        assert_eq!((apps[0].name.as_str(), apps[0].from.as_str(), apps[0].to.as_str(), apps[0].vendor.as_str()), ("Git", "2.45.1", "2.46.0", "Git"));
        assert_eq!(apps[1].name, "7-Zip 23.01 (x64)");
        assert_eq!(apps[2].to, "126.0.2592.56");
    }

    #[test]
    fn winget_table_localized_header() {
        // Column positions come from the header, so Spanish works too (misaligned data here still
        // yields no rows rather than garbage ids).
        let apps = parse_winget(WINGET_ES, true);
        assert!(apps.iter().all(|a| safe_name(&a.id["winget:".len()..])));
    }

    #[test]
    fn winget_list_has_no_available_column() {
        let out = "Name   Id         Version\n---------------------------\nGit    Git.Git    2.45.1\n";
        let apps = parse_winget(out, false);
        assert_eq!((apps.len(), apps[0].from.as_str(), apps[0].to.as_str()), (1, "2.45.1", ""));
    }

    #[test]
    fn scoop_status_and_list() {
        let status = "WARN  Scoop bucket(s) out of date.\nName    Installed Version Latest Version Info\n----    ----------------- -------------- ----\n\x1b[32m7zip\x1b[0m    23.01             24.07\nripgrep 14.0.0            14.1.0\n\nDone";
        let apps = parse_scoop(status, true);
        assert_eq!(apps.iter().map(|a| (a.id.as_str(), a.from.as_str(), a.to.as_str())).collect::<Vec<_>>(), [("scoop:7zip", "23.01", "24.07"), ("scoop:ripgrep", "14.0.0", "14.1.0")]);
        let list = "Installed apps:\n\nName Version Source Updated Info\n---- ------- ------ ------- ----\n7zip 24.07 main 2024-06-01\n";
        assert_eq!(parse_scoop(list, false)[0].from, "24.07");
    }

    #[test]
    fn choco_lines() {
        let apps = parse_choco_outdated("git|2.45.1|2.46.0|false\nnodejs|20.0|21.0|true\nbad|1|2\n");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "choco:git");
        assert_eq!(parse_choco_list("git|2.45.1\n--evil|1\n")[0].from, "2.45.1");
    }

    #[test]
    fn names_are_checked() {
        assert!(safe_name("Microsoft.VisualStudioCode"));
        assert!(safe_name("extras/firefox"));
        assert!(!safe_name("--source"));
        assert!(!safe_name("a b"));
        assert!(!safe_name("a/../b"));
        assert_eq!(upgrade("winget", "--all"), Err("id no soportado: winget:--all".into()));
    }
}
