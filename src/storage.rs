//! Disk space: what can be cleaned safely, what takes the most room, and how to get rid of it.
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use crate::scan;

const DAY: Duration = Duration::from_secs(24 * 3600);

/// A place whose contents the system or the apps rebuild on their own.
#[derive(Clone)]
pub struct Place {
    /// Spanish texts: they are the keys of the translations.
    pub label: &'static str,
    pub hint: &'static str,
    pub paths: Vec<PathBuf>,
    /// Only entries untouched for this long go (what is in use stays).
    pub min_age: Option<Duration>,
    pub bytes: u64,
}

/// One entry of the folder being explored.
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    pub is_dir: bool,
}

pub struct Disk {
    pub total: u64,
    pub free: u64,
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

fn place(label: &'static str, hint: &'static str, paths: Vec<PathBuf>, min_age: Option<Duration>) -> Place {
    Place { label, hint, paths, min_age, bytes: 0 }
}

/// The cleanable places of this OS (not measured yet).
pub fn places(home: &Path, temp: &Path) -> Vec<Place> {
    let rebuilt = "Se vuelven a crear solos cuando una app los necesita";
    let mut all = Vec::new();
    if cfg!(target_os = "macos") {
        all.push(place("Cachés de apps", rebuilt, vec![home.join("Library/Caches")], None));
        all.push(place("Registros", "Solo sirven para diagnosticar problemas", vec![home.join("Library/Logs")], None));
        all.push(place("Compilaciones de Xcode", "Se regeneran al compilar de nuevo", vec![home.join("Library/Developer/Xcode/DerivedData")], None));
        all.push(place("Archivos temporales", "Solo los de más de un día", vec![temp.to_path_buf()], Some(DAY)));
        all.push(place("Papelera", "Lo que ya borraste y sigue ocupando espacio", vec![home.join(".Trash")], None));
    } else if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Local"));
        all.push(place("Archivos temporales", "Solo los de más de un día", vec![temp.to_path_buf()], Some(DAY)));
        all.push(place("Caché de Internet", "Páginas y archivos guardados por el navegador", vec![local.join("Microsoft/Windows/INetCache")], None));
        all.push(place("Informes de errores", "Informes de cierres inesperados", vec![local.join("CrashDumps")], None));
    } else {
        all.push(place("Cachés de apps", rebuilt, vec![home.join(".cache")], None));
        all.push(place("Papelera", "Lo que ya borraste y sigue ocupando espacio", vec![home.join(".local/share/Trash/files"), home.join(".local/share/Trash/info")], None));
    }
    all
}

fn old_enough(meta: &fs::Metadata, min_age: Option<Duration>) -> bool {
    min_age.is_none_or(|age| meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|e| e >= age))
}

/// Disk space an entry takes (blocks on Unix, where files can be sparse; length elsewhere).
fn used(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::blocks(meta) * 512
    }
    #[cfg(not(unix))]
    {
        meta.len()
    }
}

/// Total size of a file or folder, never following links. What cannot be read counts as zero.
pub fn size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            if let Ok(entries) = fs::read_dir(&p) {
                stack.extend(entries.flatten().map(|e| e.path()));
            }
        } else {
            total += used(&meta);
        }
    }
    total
}

/// Measures the places and keeps the ones with something to clean.
pub fn cleanable() -> Vec<Place> {
    let Some(home) = home() else { return Vec::new() };
    let mut all = places(&home, &std::env::temp_dir());
    all.iter_mut().for_each(measure);
    all.retain(|p| p.bytes > 0);
    all.sort_by_key(|p| std::cmp::Reverse(p.bytes));
    all
}

/// Sets how much `clean` would free.
pub fn measure(p: &mut Place) {
    p.bytes = contents(p).iter().map(|e| size(e)).sum();
}

/// The entries of a place that `clean` would remove.
fn contents(p: &Place) -> Vec<PathBuf> {
    p.paths
        .iter()
        .filter_map(|d| fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .filter(|e| fs::symlink_metadata(e.path()).is_ok_and(|m| old_enough(&m, p.min_age)))
        .map(|e| e.path())
        .collect()
}

/// Empties a place. Returns how many entries could not be removed (in use, no permission).
pub fn clean(p: &Place) -> usize {
    contents(p)
        .iter()
        .filter(|path| {
            let dir = fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
            (if dir { fs::remove_dir_all(path) } else { fs::remove_file(path) }).is_err()
        })
        .count()
}

/// The entries of a folder, biggest first. Sizes are measured on a few threads at once.
pub fn children(dir: &Path) -> Vec<Item> {
    let paths: Vec<PathBuf> = fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    let (next, found) = (AtomicUsize::new(0), Mutex::new(Vec::new()));
    thread::scope(|s| {
        for _ in 0..paths.len().min(8) {
            s.spawn(|| {
                while let Some(path) = paths.get(next.fetch_add(1, Ordering::SeqCst)) {
                    let is_dir = fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let item = Item { path: path.clone(), name, bytes: size(path), is_dir };
                    found.lock().unwrap_or_else(|e| e.into_inner()).push(item);
                }
            });
        }
    });
    let mut items = found.into_inner().unwrap_or_else(|e| e.into_inner());
    items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
    items
}

/// Only things inside the home folder, and never the home folder itself, may be removed.
pub fn can_remove(path: &Path, home: &Path) -> bool {
    path.starts_with(home) && path != home && !path.components().any(|c| matches!(c, Component::ParentDir))
}

/// Sends a file or folder to the trash (recoverable).
pub fn trash(path: &Path) -> Result<(), String> {
    let home = home().ok_or("Sin carpeta personal")?;
    if !can_remove(path, &home) {
        return Err("Solo se pueden quitar archivos de tu carpeta personal".into());
    }
    move_to_trash(path, &home)
}

#[cfg(target_os = "macos")]
fn move_to_trash(path: &Path, home: &Path) -> Result<(), String> {
    let dir = home.join(".Trash");
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or("Ruta no válida")?;
    let mut dest = dir.join(&name);
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{name} {n}"));
        n += 1;
    }
    if fs::rename(path, &dest).is_ok() {
        return Ok(());
    }
    // Another volume: Finder knows how to move it. The path travels as an argument, never inside the script.
    let script = ["on run argv", "tell application \"Finder\" to delete (POSIX file (item 1 of argv) as alias)", "end run"];
    let mut args = Vec::new();
    for line in script {
        args.extend(["-e", line]);
    }
    let path = path.to_string_lossy();
    args.push(&path);
    scan::run(Command::new("osascript"), &args, Duration::from_secs(60)).map(|_| ())
}

#[cfg(windows)]
fn move_to_trash(path: &Path, _home: &Path) -> Result<(), String> {
    const SCRIPT: &str = "Add-Type -AssemblyName Microsoft.VisualBasic; $p = $env:UPDATEEER_PATH; \
        if (Test-Path -LiteralPath $p -PathType Container) { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($p, 'OnlyErrorDialogs', 'SendToRecycleBin') } \
        else { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'SendToRecycleBin') }";
    let mut cmd = Command::new("powershell");
    cmd.env("UPDATEEER_PATH", path);
    scan::run(cmd, &["-NoProfile", "-Command", SCRIPT], Duration::from_secs(60)).map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn move_to_trash(path: &Path, _home: &Path) -> Result<(), String> {
    scan::run(Command::new("gio"), &["trash", "--", &path.to_string_lossy()], Duration::from_secs(60)).map(|_| ())
}

/// Shows a file in the system's file manager.
pub fn reveal(path: &Path) {
    let mut cmd;
    if cfg!(target_os = "macos") {
        cmd = Command::new("open");
        cmd.arg("-R").arg(path);
    } else if cfg!(windows) {
        cmd = Command::new("explorer");
        cmd.arg(format!("/select,{}", path.display()));
    } else {
        cmd = Command::new("xdg-open");
        cmd.arg(path.parent().unwrap_or(path));
    }
    let _ = cmd.spawn();
}

/// Total and free space of the disk that holds `path`.
pub fn disk(path: &Path) -> Option<Disk> {
    let timeout = Duration::from_secs(20);
    if cfg!(windows) {
        let drive = path.to_string_lossy().chars().next()?.to_string();
        let mut cmd = Command::new("powershell");
        cmd.env("UPDATEEER_DRIVE", drive);
        let script = "$d = Get-PSDrive -Name $env:UPDATEEER_DRIVE; \"$($d.Used) $($d.Free)\"";
        parse_used_free(&scan::run(cmd, &["-NoProfile", "-Command", script], timeout).ok()?)
    } else {
        parse_df(&scan::run(Command::new("df"), &["-kP", &path.to_string_lossy()], timeout).ok()?)
    }
}

/// `df -kP`: a header line, then `filesystem 1K-blocks used available capacity mount`.
pub fn parse_df(out: &str) -> Option<Disk> {
    let fields: Vec<&str> = out.lines().nth(1)?.split_whitespace().collect();
    let (total, free): (u64, u64) = (fields.get(1)?.parse().ok()?, fields.get(3)?.parse().ok()?);
    Some(Disk { total: total * 1024, free: free * 1024 })
}

fn parse_used_free(out: &str) -> Option<Disk> {
    let mut n = out.split_whitespace().map(|v| v.parse::<u64>());
    let (used, free) = (n.next()?.ok()?, n.next()?.ok()?);
    Some(Disk { total: used + free, free })
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (bytes as f64, 0);
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i < 3 { format!("{v:.0} {}", UNITS[i]) } else { format!("{v:.1} {}", UNITS[i]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("updateeer-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sizes_add_up_and_children_are_sorted() {
        let dir = scratch("sizes");
        fs::create_dir_all(dir.join("big/inner")).unwrap();
        fs::write(dir.join("big/inner/a.bin"), vec![1u8; 200_000]).unwrap();
        fs::write(dir.join("small.txt"), b"hi").unwrap();
        let items = children(&dir);
        assert_eq!(items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["big", "small.txt"]);
        assert!(items[0].is_dir && items[0].bytes >= 200_000 && !items[1].is_dir);
        assert!(size(&dir) >= items[0].bytes + items[1].bytes);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_empties_old_entries_and_keeps_recent_ones() {
        let dir = scratch("clean");
        fs::write(dir.join("a.tmp"), b"x").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/b.tmp"), b"y").unwrap();
        let mut p = place("t", "t", vec![dir.clone()], Some(DAY));
        assert_eq!(clean(&p), 0);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2, "fresh files are left alone");
        p.min_age = None;
        assert_eq!(clean(&p), 0);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        assert!(dir.exists(), "the folder itself stays");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_things_inside_home_can_be_removed() {
        let home = Path::new("/home/ana");
        assert!(can_remove(Path::new("/home/ana/Downloads/x.iso"), home));
        assert!(!can_remove(home, home));
        assert!(!can_remove(Path::new("/etc/passwd"), home));
        assert!(!can_remove(Path::new("/home/ana/../bob/x"), home));
        assert!(!can_remove(Path::new("/home/anabel/x"), home));
    }

    #[test]
    fn df_and_sizes_are_read() {
        let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 1000 400 600 40% /\n";
        let d = parse_df(df).unwrap();
        assert_eq!((d.total, d.free), (1000 * 1024, 600 * 1024));
        let d = parse_used_free("400 600\r\n").unwrap();
        assert_eq!((d.total, d.free), (1000, 600));
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    /// Reads this machine's real disk and folders; run it by hand with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_machine_smoke() {
        let home = home().unwrap();
        let d = disk(&home).expect("df works");
        println!("disk: {} free of {}", format_bytes(d.free), format_bytes(d.total));
        for p in cleanable() {
            println!("clean: {} = {}", p.label, format_bytes(p.bytes));
        }
        for i in children(&home).iter().take(5) {
            println!("home: {} = {}", i.name, format_bytes(i.bytes));
        }
    }
}
