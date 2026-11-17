//! Docker and Colima: what is running, what is stopped or unused, and how old it is, so the user
//! can decide what to delete. Only local engines are ever touched (never an ssh/tcp context).
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::scan;

const QUICK: Duration = Duration::from_secs(30);
const SLOW: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Usage {
    /// Used by a container that is running.
    Running,
    /// Only used by containers that are stopped.
    Stopped,
    Unused,
}

#[derive(Clone)]
pub struct Image {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    pub created: Option<SystemTime>,
    pub usage: Usage,
}

/// A container that is not running.
#[derive(Clone)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub created: Option<SystemTime>,
}

/// A Colima virtual machine: its disk is one big file on the host.
#[derive(Clone)]
pub struct Vm {
    pub name: String,
    pub running: bool,
    pub disk_bytes: u64,
    /// When its disk was last written: a stopped VM untouched for months is a candidate to delete.
    pub last_used: Option<SystemTime>,
}

#[derive(Clone, Default)]
pub struct Engine {
    pub label: String,
    pub context: String,
    pub vm: Option<Vm>,
    /// The daemon answered, so images and containers are known.
    pub up: bool,
    pub images: Vec<Image>,
    pub stopped: Vec<Container>,
    pub cache_bytes: u64,
}

/// What the user can decide to delete. Ids and contexts were read from the engine itself.
#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    Images { context: String, ids: Vec<String> },
    Container { context: String, id: String },
    BuildCache { context: String },
    Vm { name: String },
}

pub fn valid_id(s: &str) -> bool {
    (4..=71).contains(&s.len()) && s.trim_start_matches("sha256:").chars().all(|c| c.is_ascii_hexdigit())
}

pub fn valid_name(s: &str) -> bool {
    // Never starts with a dash: it would be read as an option by the tool it is handed to.
    (1..=64).contains(&s.len()) && s.starts_with(|c: char| c.is_ascii_alphanumeric()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// The docker context Colima creates for a profile.
pub fn context_for(profile: &str) -> String {
    if profile == "default" { "colima".into() } else { format!("colima-{profile}") }
}

fn profile_of(context: &str) -> Option<&str> {
    if context == "colima" { Some("default") } else { context.strip_prefix("colima-") }
}

/// Where Colima keeps the disk of a profile.
fn disk_dir(profile: &str) -> Option<PathBuf> {
    let dir = if profile == "default" { "colima".to_string() } else { format!("colima-{profile}") };
    let home = crate::storage::home()?;
    let roots = [std::env::var_os("COLIMA_HOME").map(PathBuf::from), Some(home.join(".colima")), Some(home.join(".config/colima"))];
    roots.into_iter().flatten().map(|r| r.join("_lima/_disks").join(&dir)).find(|p| p.exists())
}

/// A command-line tool, also where a desktop launch (with a bare PATH) would not find it.
fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut dirs: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/snap/bin", "/Applications/Docker.app/Contents/Resources/bin"].iter().map(PathBuf::from).collect();
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        dirs.push(Path::new(&pf).join("Docker/Docker/resources/bin"));
    }
    dirs.extend(std::env::var_os("PATH").iter().flat_map(std::env::split_paths));
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

fn command(tool: &Path) -> Command {
    let mut cmd = Command::new(tool);
    // Colima calls limactl, docker calls its plugins: both live next to Homebrew's binaries.
    let extra = ["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from);
    let current = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::split_paths(&current).chain(extra);
    if let Ok(joined) = std::env::join_paths(path) {
        cmd.env("PATH", joined);
    }
    // The context asked for must be the one used, not whatever the environment points to.
    cmd.env_remove("DOCKER_HOST").env_remove("DOCKER_CONTEXT");
    cmd
}

fn docker_out(docker: &Path, context: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut all = vec!["--context", context];
    all.extend(args);
    scan::run(command(docker), &all, timeout)
}

/// Colima profiles: `(name, running)` from `colima list --json` (one JSON object per line).
pub fn parse_colima(out: &str) -> Vec<(String, bool)> {
    out.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| Some((v["name"].as_str()?.to_string(), v["status"].as_str()? == "Running")))
        .filter(|(name, _)| valid_name(name))
        .collect()
}

/// `95.8MB`, `1.07GB`, `20.5kB`: Docker prints decimal units.
pub fn parse_size(s: &str) -> u64 {
    let s = s.trim();
    let digits = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let value: f64 = s[..digits].parse().unwrap_or(0.0);
    let unit = match s[digits..].trim().to_ascii_lowercase().as_str() {
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        _ => 1.0,
    };
    (value * unit) as u64
}

/// Days since 1970-01-01 of a calendar date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn at(date: &str, time: &str, offset_secs: i64) -> Option<SystemTime> {
    let mut d = date.split('-').map(|n| n.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':');
    let (h, mi) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    let s = t.next()?.split('.').next()?.parse::<i64>().ok()?;
    let secs = days_from_civil(y, m, day) * 86_400 + h * 3600 + mi * 60 + s - offset_secs;
    u64::try_from(secs).ok().map(|s| UNIX_EPOCH + Duration::from_secs(s))
}

fn offset(text: &str) -> Option<i64> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    let (h, m) = (digits.get(..2)?.parse::<i64>().ok()?, digits.get(2..4).unwrap_or("00").parse::<i64>().ok()?);
    Some(sign * (h * 3600 + m * 60))
}

/// `2026-11-17 11:22:54 -0600 CST`, as `docker image ls` prints it.
pub fn parse_created_at(s: &str) -> Option<SystemTime> {
    let mut parts = s.split_whitespace();
    let (date, time) = (parts.next()?, parts.next()?);
    at(date, time, parts.next().and_then(offset).unwrap_or(0))
}

/// `2026-11-17T21:10:58.439244894Z`, as `docker inspect` prints it.
pub fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    let (date, rest) = s.split_once('T')?;
    let end = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let tz = if rest[end..].starts_with('Z') { 0 } else { offset(&rest[end..]).unwrap_or(0) };
    at(date, &rest[..end], tz)
}

/// Containers from `inspect --format '{{.Id}}|{{.Name}}|{{.State.Status}}|{{.Image}}|{{.Created}}'`:
/// `(container, image id, running)`.
pub fn parse_containers(out: &str) -> Vec<(Container, String, bool)> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(5, '|').collect();
            let [id, name, status, image, created] = f[..] else { return None };
            let running = matches!(status, "running" | "paused" | "restarting");
            let container = Container { id: id.chars().take(12).collect(), name: name.trim_start_matches('/').to_string(), image: String::new(), created: parse_rfc3339(created) };
            Some((container, image.trim_start_matches("sha256:").to_string(), running))
        })
        .collect()
}

/// Images from `image ls --format '{{json .}}'`, with how each is used by the containers.
pub fn parse_images(out: &str, containers: &[(Container, String, bool)]) -> Vec<Image> {
    out.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            let id: String = v["ID"].as_str()?.trim_start_matches("sha256:").chars().take(12).collect();
            let name = match (v["Repository"].as_str()?, v["Tag"].as_str().unwrap_or("")) {
                ("<none>", _) => format!("<none> ({id})"),
                (repo, "<none>" | "") => repo.to_string(),
                (repo, tag) => format!("{repo}:{tag}"),
            };
            let users = containers.iter().filter(|(_, image, _)| image.starts_with(&id));
            let usage = if users.clone().any(|(_, _, running)| *running) {
                Usage::Running
            } else if users.count() > 0 {
                Usage::Stopped
            } else {
                Usage::Unused
            };
            Some(Image { id, name, bytes: parse_size(v["Size"].as_str().unwrap_or("0")), created: v["CreatedAt"].as_str().and_then(parse_created_at), usage })
        })
        .collect()
}

/// The `Reclaimable:` line of `docker builder du`.
pub fn parse_cache(out: &str) -> u64 {
    out.lines().find_map(|l| l.strip_prefix("Reclaimable:")).map_or(0, |v| parse_size(v.split_whitespace().next().unwrap_or("0")))
}

fn fill(docker: &Path, e: &mut Engine) -> Result<(), String> {
    let ctx = e.context.clone();
    let ids = docker_out(docker, &ctx, &["container", "ls", "-a", "-q", "--no-trunc"], QUICK)?;
    let ids: Vec<&str> = ids.split_whitespace().filter(|i| valid_id(i)).collect();
    let mut containers = Vec::new();
    if !ids.is_empty() {
        let mut args = vec!["container", "inspect", "--format", "{{.Id}}|{{.Name}}|{{.State.Status}}|{{.Image}}|{{.Created}}"];
        args.extend(&ids);
        containers = parse_containers(&docker_out(docker, &ctx, &args, QUICK)?);
    }
    let images = docker_out(docker, &ctx, &["image", "ls", "--format", "{{json .}}"], QUICK)?;
    e.images = parse_images(&images, &containers);
    for (mut c, image, running) in containers {
        if !running {
            c.image = e.images.iter().find(|i| image.starts_with(&i.id)).map(|i| i.name.clone()).unwrap_or_default();
            e.stopped.push(c);
        }
    }
    // `system df` can hang for minutes on a big engine; the build cache has its own quick summary.
    e.cache_bytes = docker_out(docker, &ctx, &["builder", "du"], QUICK).map(|o| parse_cache(&o)).unwrap_or(0);
    e.up = true;
    Ok(())
}

/// Every local engine: Colima profiles (running or not) and any other local docker context.
pub fn scan() -> Vec<Engine> {
    let docker = find_tool("docker");
    let mut engines = Vec::new();
    if let Some(colima) = find_tool("colima")
        && let Ok(out) = scan::run(command(&colima), &["list", "--json"], QUICK)
    {
        for (name, running) in parse_colima(&out) {
            let dir = disk_dir(&name);
            let vm = Vm { name: name.clone(), running, disk_bytes: dir.as_deref().map_or(0, crate::storage::size), last_used: dir.as_deref().and_then(|d| d.join("datadisk").metadata().or_else(|_| d.metadata()).ok()).and_then(|m| m.modified().ok()) };
            let mut e = Engine { label: format!("Colima · {name}"), context: context_for(&name), vm: Some(vm), ..Default::default() };
            if running && let Some(d) = &docker {
                let _ = fill(d, &mut e);
            }
            engines.push(e);
        }
    }
    if let Some(d) = &docker
        && let Ok(out) = scan::run(command(d), &["context", "ls", "--format", "{{json .}}"], QUICK)
    {
        for v in out.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
            let (name, endpoint) = (v["Name"].as_str().unwrap_or(""), v["DockerEndpoint"].as_str().unwrap_or(""));
            let local = endpoint.starts_with("unix://") || endpoint.starts_with("npipe://");
            if local && valid_name(name) && !name.starts_with("colima") {
                let mut e = Engine { label: "Docker".into(), context: name.to_string(), ..Default::default() };
                if fill(d, &mut e).is_ok() {
                    engines.push(e);
                }
            }
        }
    }
    engines
}

/// Does what the user decided. Colima gets its disk trimmed afterwards so the host gets the room back.
pub fn run(action: &Action) -> Result<String, String> {
    let docker = || find_tool("docker").ok_or("Docker no está instalado");
    let result = match action {
        Action::Images { context, ids } => {
            if !valid_name(context) || ids.is_empty() || !ids.iter().all(|i| valid_id(i)) {
                return Err("Datos no válidos".into());
            }
            let mut args = vec!["image", "rm"];
            args.extend(ids.iter().map(String::as_str));
            docker_out(&docker()?, context, &args, SLOW)
        }
        Action::Container { context, id } => {
            if !valid_name(context) || !valid_id(id) {
                return Err("Datos no válidos".into());
            }
            docker_out(&docker()?, context, &["container", "rm", id], SLOW)
        }
        Action::BuildCache { context } => {
            if !valid_name(context) {
                return Err("Datos no válidos".into());
            }
            docker_out(&docker()?, context, &["builder", "prune", "-a", "-f"], SLOW)
        }
        Action::Vm { name } => {
            if !valid_name(name) {
                return Err("Datos no válidos".into());
            }
            let colima = find_tool("colima").ok_or("Colima no está instalado")?;
            return scan::run(command(&colima), &["delete", "--force", name], SLOW);
        }
    };
    if let Some(profile) = match action {
        Action::Images { context, .. } | Action::Container { context, .. } | Action::BuildCache { context } => profile_of(context),
        Action::Vm { .. } => None,
    } && result.is_ok()
        && let Some(colima) = find_tool("colima")
    {
        // Best effort: freed space inside the VM only shrinks the host's disk file once it is trimmed.
        let _ = scan::run(command(&colima), &["ssh", "--profile", profile, "--", "sudo", "fstrim", "-a"], SLOW);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colima_profiles_and_contexts() {
        let out = "{\"name\":\"default\",\"status\":\"Running\",\"disk\":1}\n{\"name\":\"iltc-run\",\"status\":\"Stopped\"}\nnot json\n{\"name\":\"bad name\",\"status\":\"Running\"}\n";
        assert_eq!(parse_colima(out), [("default".to_string(), true), ("iltc-run".to_string(), false)]);
        assert_eq!(context_for("default"), "colima");
        assert_eq!(context_for("iltc-run"), "colima-iltc-run");
        assert_eq!(profile_of("colima-iltc-run"), Some("iltc-run"));
        assert_eq!(profile_of("desktop-linux"), None);
    }

    #[test]
    fn sizes_and_dates_are_read() {
        assert_eq!(parse_size("95.8MB"), 95_800_000);
        assert_eq!(parse_size("1.07GB"), 1_070_000_000);
        assert_eq!(parse_size("20.5kB"), 20_500);
        assert_eq!(parse_size("0B"), 0);
        let secs = |t: Option<SystemTime>| t.unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs();
        // 2026-11-17 17:22:54 UTC
        assert_eq!(secs(parse_created_at("2026-11-17 11:22:54 -0600 CST")), 1_794_936_174);
        assert_eq!(secs(parse_rfc3339("2026-11-17T17:22:54.439244894Z")), 1_794_936_174);
        assert_eq!(secs(parse_rfc3339("2026-11-17T11:22:54-06:00")), 1_794_936_174);
        assert_eq!(secs(parse_created_at("1970-01-02 00:00:00 +0000 UTC")), 86_400);
    }

    #[test]
    fn images_know_whether_they_are_in_use() {
        let containers = "aaaaaaaaaaaaaaaa|/web|running|sha256:1111111111119999|2026-11-17T17:22:54Z\nbbbbbbbbbbbbbbbb|/old|exited|sha256:2222222222228888|2026-11-01T10:00:00Z\n";
        let c = parse_containers(containers);
        assert_eq!(c.len(), 2);
        assert_eq!((c[1].0.name.as_str(), c[1].2), ("old", false));
        let images = concat!(
            "{\"ID\":\"111111111111\",\"Repository\":\"nginx\",\"Tag\":\"1.27\",\"Size\":\"190MB\",\"CreatedAt\":\"2026-11-10 10:00:00 +0000 UTC\"}\n",
            "{\"ID\":\"222222222222\",\"Repository\":\"app\",\"Tag\":\"dev\",\"Size\":\"1.07GB\",\"CreatedAt\":\"2026-10-01 10:00:00 +0000 UTC\"}\n",
            "{\"ID\":\"333333333333\",\"Repository\":\"<none>\",\"Tag\":\"<none>\",\"Size\":\"5MB\",\"CreatedAt\":\"2026-09-01 10:00:00 +0000 UTC\"}\n",
        );
        let images = parse_images(images, &c);
        assert_eq!(images.iter().map(|i| i.usage).collect::<Vec<_>>(), [Usage::Running, Usage::Stopped, Usage::Unused]);
        assert_eq!(images[0].name, "nginx:1.27");
        assert_eq!(images[2].name, "<none> (333333333333)");
        assert_eq!(images[1].bytes, 1_070_000_000);
    }

    #[test]
    fn build_cache_and_ids() {
        assert_eq!(parse_cache("Shared:\t0B\nPrivate:\t371.6MB\nReclaimable:\t37.1GB\nTotal:\t\t37.1GB\n"), 37_100_000_000);
        assert!(valid_id("d8934cd7ba44") && valid_id("sha256:abcdef0123456789"));
        assert!(!valid_id("--force") && !valid_id("abc") && !valid_id("zz12345"));
        assert!(valid_name("colima-iltc-run") && valid_name("desktop-linux"));
        assert!(!valid_name("a b") && !valid_name("") && !valid_name("x;y") && !valid_name("--all") && !valid_name("-f"));
        assert!(run(&Action::Images { context: "colima".into(), ids: vec!["--all".into()] }).is_err());
        assert!(run(&Action::Vm { name: "x y".into() }).is_err());
    }

    /// Reads the real engines of this machine (read-only); run by hand with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_machine_smoke() {
        let now = SystemTime::now();
        for e in scan() {
            let vm = e.vm.as_ref().map(|v| format!("running={} disk={} last_used={:?}d", v.running, crate::storage::format_bytes(v.disk_bytes), v.last_used.map(|t| crate::model::days_ago(t, now)))).unwrap_or_default();
            println!("engine {} [{}] up={} {vm} cache={}", e.label, e.context, e.up, crate::storage::format_bytes(e.cache_bytes));
            for i in &e.images {
                println!("  image {} {:?} {} age={:?}d", i.name, i.usage, crate::storage::format_bytes(i.bytes), i.created.map(|t| crate::model::days_ago(t, now)));
            }
            for c in &e.stopped {
                println!("  stopped {} ({}) age={:?}d", c.name, c.image, c.created.map(|t| crate::model::days_ago(t, now)));
            }
        }
    }
}
