//! One password dialog per session for the `sudo` calls brew makes (cask `.pkg` installers).
//!
//! brew runs without a terminal, so sudo needs an askpass program. The program is a tiny script
//! that asks this process over a private Unix socket. The first request shows the macOS dialog,
//! checks the password with sudo and keeps it in memory; later requests are answered from memory.
//! The password never touches the disk. The socket folder is 0700 and requests carry a random token.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static SCRIPT: OnceLock<Option<PathBuf>> = OnceLock::new();
static PASSWORD: Mutex<Option<String>> = Mutex::new(None);
/// When the user last cancelled the dialog: sudo retries the askpass, and one cancel is enough.
static CANCELLED: Mutex<Option<Instant>> = Mutex::new(None);

/// Path for `SUDO_ASKPASS`; starts the server on first use.
pub fn script() -> Option<&'static PathBuf> {
    SCRIPT.get_or_init(start).as_ref()
}

fn start() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!("ua-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700).create(&dir).ok()?;
    let token = crate::masroot::random_token().ok()?;
    let sock = dir.join("s");
    let listener = UnixListener::bind(&sock).ok()?;
    let file = dir.join("askpass.sh");
    std::fs::write(&file, format!("#!/bin/sh\nprintf '%s\\n' '{token}' | /usr/bin/nc -U '{}'\n", sock.display())).ok()?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).ok()?;
    std::thread::spawn(move || {
        // One request at a time, so two sudo calls never open two dialogs.
        for conn in listener.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(&conn).read_line(&mut line).is_ok() && line.trim() == token {
                if let Some(pw) = password() {
                    let _ = writeln!(&conn, "{pw}");
                }
            }
        }
    });
    Some(file)
}

fn password() -> Option<String> {
    let mut cached = PASSWORD.lock().unwrap_or_else(|e| e.into_inner());
    if cached.is_some() {
        return cached.clone();
    }
    if CANCELLED.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < Duration::from_secs(30)) {
        return None;
    }
    for attempt in 0..3 {
        let Some(pw) = ask(attempt > 0) else {
            *CANCELLED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            return None;
        };
        if valid(&pw) {
            *cached = Some(pw.clone());
            return Some(pw);
        }
    }
    None
}

fn ask(retry: bool) -> Option<String> {
    let msg = crate::i18n::t(if retry {
        "Contraseña incorrecta. Updateeer necesita tu contraseña para instalar esta actualización"
    } else {
        "Updateeer necesita tu contraseña para instalar esta actualización"
    })
    .replace(['\\', '"'], "");
    let script = format!(
        "text returned of (display dialog \"{msg}\" default answer \"\" with hidden answer with icon caution buttons {{\"Cancel\", \"OK\"}} default button \"OK\")"
    );
    let out = Command::new("/usr/bin/osascript").args(["-e", &script]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string()).filter(|p| !p.is_empty())
}

/// `-k` ignores any cached sudo ticket, so only the right password passes.
fn valid(pw: &str) -> bool {
    let Ok(mut child) = Command::new("/usr/bin/sudo")
        .args(["-k", "-S", "-p", "", "/usr/bin/true"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{pw}");
    }
    child.wait().is_ok_and(|s| s.success())
}
