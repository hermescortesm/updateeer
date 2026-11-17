//! Administrator commands through one polkit-approved root shell per session.
use super::*;
use crate::fail::Fail;

/// One root shell for the whole session: polkit asks for the password once, on the first
/// administrator action, instead of once per update. Holding the lock also serializes the
/// system package managers, which cannot run two at a time.
pub(super) struct RootShell {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

/// What pacman has printed so far in the running command (`LC_ALL=C`, no terminal): the package
/// count, how many downloads started and which package is being installed.
#[derive(Default)]
struct Pacman {
    total: u32,
    downloaded: u32,
    step: Option<(u32, u32)>,
}

static PACMAN: Mutex<Option<Pacman>> = Mutex::new(None);

impl Pacman {
    fn observe(&mut self, line: &str) {
        if let Some(n) = line.strip_prefix("Packages (").and_then(|r| r.split(')').next()).and_then(|n| n.parse().ok()) {
            *self = Pacman { total: n, ..Default::default() };
        } else if line.starts_with(' ') && line.trim_end().ends_with(" downloading...") {
            self.downloaded += 1;
        } else if let Some((counts, action)) = line.strip_prefix('(').and_then(|r| r.split_once(") "))
            && ["upgrading ", "installing ", "reinstalling ", "downgrading "].iter().any(|a| action.starts_with(a))
            && let Some((n, m)) = counts.split_once('/')
            && let (Ok(n), Ok(m)) = (n.parse(), m.parse())
        {
            self.step = Some((n, m));
        }
    }

    /// 0..70 while downloading, 70..99 while installing, matching the "Descargando"/"Instalando" labels.
    fn percent(&self) -> Option<f32> {
        match self.step {
            Some((n, m)) if m > 0 => Some(70.0 + 29.0 * (n as f32 / m as f32).min(1.0)),
            _ if self.total > 0 => Some(69.0 * (self.downloaded as f32 / self.total as f32).min(1.0)),
            _ => None,
        }
    }
}

/// Real progress of the pacman command running now, when its output has said enough.
pub fn pacman_progress() -> Option<f32> {
    PACMAN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(Pacman::percent)
}

fn track(line: Option<&str>) {
    let mut state = PACMAN.lock().unwrap_or_else(|e| e.into_inner());
    match line {
        Some(l) => state.get_or_insert_with(Pacman::default).observe(l),
        None => *state = None,
    }
}

pub(super) static ROOT: Mutex<Option<RootShell>> = Mutex::new(None);
pub(super) const DONE: &str = "__UPDATEEER_DONE__";

pub(super) fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// The polkit policy the packages install: it names Updateeer in the dialog and lets polkit
/// remember the password for a few minutes.
pub(super) const POLICY: &str = "/usr/share/polkit-1/actions/mx.hermescortes.updateeer.policy";
pub(super) const INSTALLED_EXE: &str = "/usr/bin/updateeer";

/// What pkexec runs: Updateeer's own `--root-shell` when the policy is installed and this is the
/// installed binary (so both are the same version), a plain shell otherwise.
pub(super) fn root_shell_args(policy: bool, installed_exe: bool) -> &'static [&'static str] {
    if policy && installed_exe { &[INSTALLED_EXE, "--root-shell"] } else { &["/bin/sh"] }
}

pub(super) fn start_root() -> Result<RootShell, String> {
    let bin = find("pkexec").ok_or("Falta pkexec (polkit) para pedir permisos de administrador")?;
    let installed_exe = crate::selfupdate::exe_path().is_ok_and(|p| p == Path::new(INSTALLED_EXE));
    spawn_shell(Command::new(bin).args(root_shell_args(Path::new(POLICY).is_file(), installed_exe)))
}

pub(super) fn spawn_shell(cmd: &mut Command) -> Result<RootShell, String> {
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
pub(super) fn root(args: &[&str]) -> Result<String, String> {
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
pub(super) struct Ran(Result<String, String>, bool);

pub(super) fn run_in(sh: &mut RootShell, args: &[&str]) -> Ran {
    let generation = scan::CANCEL_GEN.load(Ordering::SeqCst);
    let cmd = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    if writeln!(sh.stdin, "{{ {cmd}; }} 2>&1 </dev/null; echo {DONE}$?").and_then(|_| sh.stdin.flush()).is_err() {
        return Ran(Err("No se pudo contactar al shell de administrador".into()), true);
    }
    let deadline = Instant::now() + UPGRADE_TIMEOUT;
    let mut out = Vec::new();
    track(None);
    let ran = loop {
        if scan::CANCEL_GEN.load(Ordering::SeqCst) != generation {
            break Ran(Err(Fail::Cancelled.into()), true);
        }
        if Instant::now() >= deadline {
            break Ran(Err("El comando tardó demasiado y se canceló".into()), true);
        }
        match sh.lines.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => match line.strip_prefix(DONE) {
                Some("0") => break Ran(Ok(out.join("\n")), false),
                Some(_) => break Ran(Err(scan::tail(&out.join("\n"))), false),
                None => {
                    track(Some(&line));
                    out.push(line);
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            // pkexec ended: password refused, dialog dismissed or polkit missing.
            Err(RecvTimeoutError::Disconnected) => {
                let why = scan::tail(&out.join("\n"));
                break Ran(Err(if why.is_empty() { "No se obtuvieron permisos de administrador".into() } else { why }), true);
            }
        }
    };
    track(None);
    ran
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn one_shell_runs_many_commands() {
        let mut sh = spawn_shell(&mut Command::new("/bin/sh")).unwrap();
        assert_eq!(run_in(&mut sh, &["echo", "it's", "one"]).0, Ok("it's one".into()));
        assert_eq!(run_in(&mut sh, &["sh", "-c", "echo boom >&2; exit 3"]).0, Err("boom".into()));
        assert_eq!(run_in(&mut sh, &["echo", "still", "alive"]).0, Ok("still alive".into()));
        let _ = sh.child.kill();
    }

    #[test]
    fn pacman_output_becomes_progress() {
        let mut p = Pacman::default();
        assert_eq!(p.percent(), None);
        for l in ["Packages (4) a-1 -> a-2", " a-2-1-x86_64 downloading...", " b-2-1-x86_64 downloading..."] {
            p.observe(l);
        }
        assert_eq!(p.percent(), Some(34.5));
        p.observe("(1/4) checking keys in keyring");
        assert_eq!(p.percent(), Some(34.5), "checks are not installs");
        p.observe("(2/4) upgrading a");
        assert_eq!(p.percent(), Some(84.5));
        p.observe("(4/4) installing d");
        assert_eq!(p.percent(), Some(99.0));
    }

    #[test]
    fn the_root_shell_uses_the_policy_only_for_the_installed_binary() {
        assert_eq!(root_shell_args(true, true), ["/usr/bin/updateeer", "--root-shell"]);
        assert_eq!(root_shell_args(true, false), ["/bin/sh"]);
        assert_eq!(root_shell_args(false, true), ["/bin/sh"]);
        // The policy must name exactly what pkexec is asked to run.
        let policy = include_str!("../../../packaging/linux/mx.hermescortes.updateeer.policy");
        assert!(policy.contains(&format!(">{INSTALLED_EXE}<")) && policy.contains(">--root-shell<") && policy.contains("auth_admin_keep"));
    }
}
