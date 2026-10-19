//! Lets a program that must run as the user (an AUR helper builds packages) use Updateeer's one
//! administrator shell for its `sudo pacman ...` calls, so the password is asked once per session
//! instead of once per call.
//!
//! The helper is started with a tiny `sh` script as its `sudo`. The script writes the command to a
//! FIFO in a private folder and waits for the answer on a second FIFO. A request must carry a
//! random token (given only to the helper through its environment) and may only run `pacman`.
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Environment variable that carries the token to the helper and from there to the script.
pub const TOKEN_VAR: &str = "UPDATEEER_BRIDGE_TOKEN";

const SCRIPT: &str = r#"#!/bin/sh
d=${0%/*}
while [ $# -gt 0 ]; do case $1 in -*) shift;; *) break;; esac; done
[ $# -gt 0 ] || exit 1
r="$d/resp.$$"
mkfifo "$r" || exit 1
trap 'rm -f "$r"' EXIT
printf '%s\n' "$UPDATEEER_BRIDGE_TOKEN" "$r" "$#" "$@" > "$d/req" || exit 1
{ IFS= read -r code; cat; } < "$r"
exit "${code:-1}"
"#;

pub struct Bridge {
    dir: PathBuf,
    token: String,
    stop: Arc<AtomicBool>,
}

impl Bridge {
    /// Starts serving; `run` executes an allowed command as root and returns its output.
    pub fn start(run: impl Fn(&[String]) -> Result<String, String> + Send + 'static) -> Result<Bridge, String> {
        let token = random_token()?;
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("updateeer-bridge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        make_private_dir(&dir)?;
        fs::write(dir.join("sudo.sh"), SCRIPT).map_err(|e| e.to_string())?;
        let req = dir.join("req");
        let made = Command::new("mkfifo").arg(&req).status().map_err(|e| e.to_string())?;
        if !made.success() {
            return Err("No se pudo preparar el puente de permisos".into());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, tok, folder) = (stop.clone(), token.clone(), dir.clone());
        // Read-write on a FIFO never sees end-of-file when a writer leaves.
        let fifo = fs::OpenOptions::new().read(true).write(true).open(&req).map_err(|e| e.to_string())?;
        thread::spawn(move || serve(BufReader::new(fifo), &folder, &tok, &flag, &run));
        Ok(Bridge { dir, token, stop })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Helper flags that make it call our script instead of `sudo`.
    pub fn flags(&self) -> [String; 4] {
        ["--sudo".into(), "/bin/sh".into(), "--sudoflags".into(), self.dir.join("sudo.sh").display().to_string()]
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the reader so its thread ends; the folder goes right after.
        if let Ok(mut f) = open_nonblocking(&self.dir.join("req")) {
            let _ = writeln!(f, "STOP");
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn serve(mut fifo: impl BufRead, dir: &Path, token: &str, stop: &AtomicBool, run: &dyn Fn(&[String]) -> Result<String, String>) {
    let mut line = || {
        let mut s = String::new();
        (fifo.read_line(&mut s).ok()? > 0).then(|| s.trim_end_matches('\n').to_string())
    };
    while let Some(first) = line() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let (Some(resp), Some(argc)) = (line(), line().and_then(|n| n.parse::<usize>().ok())) else { continue };
        let args: Vec<String> = (0..argc.min(256)).filter_map(|_| line()).collect();
        let resp = PathBuf::from(resp);
        // The answer may only go to one of our own response pipes.
        if resp.parent() != Some(dir) || !resp.file_name().is_some_and(|n| n.to_string_lossy().starts_with("resp.")) {
            continue;
        }
        let allowed = first == token && args.first().is_some_and(|a| a == "pacman");
        let (code, out) = if !allowed {
            (1, "Comando no permitido".to_string())
        } else {
            match run(&args) {
                Ok(out) => (0, out),
                Err(e) => (1, e),
            }
        };
        reply(&resp, code, &out);
    }
}

/// Writes `code` and the output into the caller's pipe, waiting a little for it to open it.
fn reply(resp: &Path, code: i32, out: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Err(e) = open_nonblocking(resp).and_then(|mut f| write!(f, "{code}\n{out}\n")) {
        if Instant::now() >= deadline || e.kind() == std::io::ErrorKind::NotFound {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Opens a FIFO for writing without waiting for a reader (fails if there is none yet).
fn open_nonblocking(path: &Path) -> std::io::Result<fs::File> {
    let mut o = fs::OpenOptions::new();
    o.write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, 0o4000);
    o.open(path)
}

fn make_private_dir(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700).create(dir).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir).map_err(|e| e.to_string())
    }
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom").map_err(|e| e.to_string())?, &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn call(bridge: &Bridge, token: &str, args: &[&str]) -> (i32, String) {
        let script = bridge.dir.join("sudo.sh");
        let out = Command::new("/bin/sh").arg(script).args(args).env(TOKEN_VAR, token).output().unwrap();
        (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    #[test]
    fn pacman_goes_through_and_the_rest_does_not() {
        let bridge = Bridge::start(|a| if a[1] == "fail" { Err("boom".into()) } else { Ok(format!("ran {}", a.join(" "))) }).unwrap();
        let t = bridge.token().to_string();
        assert_eq!(call(&bridge, &t, &["pacman", "-U", "a b.pkg"]), (0, "ran pacman -U a b.pkg".into()));
        assert_eq!(call(&bridge, &t, &["-v", "pacman", "fail"]), (1, "boom".into()));
        assert_eq!(call(&bridge, &t, &["rm", "-rf", "x"]), (1, "Comando no permitido".into()));
        assert_eq!(call(&bridge, "wrong", &["pacman", "-S", "x"]), (1, "Comando no permitido".into()));
        let dir = bridge.dir.clone();
        bridge.stop();
        assert!(!dir.exists());
    }
}
