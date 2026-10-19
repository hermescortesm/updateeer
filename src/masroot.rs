//! One administrator authorization for all App Store updates of a session.
//!
//! `mas upgrade` needs root, and macOS shows its password dialog for every `osascript ... with
//! administrator privileges`. So a small root loop is started once behind a single dialog; it
//! answers requests written to a FIFO in a private folder. It runs nothing but
//! `mas upgrade <digits>`, only for the token given to this app, and it stops when the app does.
//! `mas` also needs `SUDO_UID` to find the real user, which the dialog does not set.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::scan::{self, UPGRADE_TIMEOUT};

const LOOP: &str = r#"d="$1"; uid="$2"; gid="$3"; parent="$4"; token="$5"; mas="$6"; limit="$7"
exec 3<>"$d/req"
echo $$ > "$d/ready"
while kill -0 "$parent" 2>/dev/null; do
  if read -r -t 2 -u 3 tok resp id; then
    case "$id" in ''|*[!0-9]*) ok=0;; *) ok=1;; esac
    if [ "$tok" = "$token" ] && [ "$ok" = 1 ] && [ "${#id}" -le 12 ] && [ "$(dirname "$resp")" = "$d" ]; then
      # mas runs in the background so a hung download can be stopped: on cancel, or after `limit` seconds.
      SUDO_UID="$uid" SUDO_GID="$gid" "$mas" upgrade "$id" >"$resp.out" 2>&1 &
      pid=$!; n=0; code=
      while kill -0 "$pid" 2>/dev/null; do
        if [ -e "$resp.cancel" ] || [ "$n" -ge "$limit" ]; then
          kill "$pid" 2>/dev/null; sleep 1; kill -9 "$pid" 2>/dev/null; code=124; break
        fi
        sleep 1; n=$((n+1))
      done
      [ -n "$code" ] || { wait "$pid"; code=$?; }
      out=$(cat "$resp.out"); rm -f "$resp.out" "$resp.cancel"
    else
      out="not allowed"; code=1
    fi
    printf '%s\n%s\n' "$code" "$out" > "$resp.tmp" && mv "$resp.tmp" "$resp"
  fi
done
"#;

struct Helper {
    dir: PathBuf,
    token: String,
}

static HELPER: Mutex<Option<Helper>> = Mutex::new(None);

/// Runs `mas upgrade id` as root, asking for the password only when no helper is running.
/// Blocks, and serializes the upgrades (the App Store does one at a time anyway).
pub fn upgrade(mas_bin: &str, id: &str) -> Result<(), String> {
    upgrade_with(mas_bin, id, &admin_launcher)
}

fn upgrade_with(mas_bin: &str, id: &str, launcher: &dyn Fn(&str) -> Result<(), String>) -> Result<(), String> {
    let mut guard = HELPER.lock().unwrap_or_else(|e| e.into_inner());
    for fresh in [false, true] {
        if guard.is_none() {
            *guard = Some(start(mas_bin, launcher)?);
        }
        let helper = guard.as_ref().expect("just started");
        match ask(helper, id) {
            Ask::Done(result) => return result,
            // Nobody is listening: the helper died (or the app is a new session).
            Ask::Gone if !fresh => {
                if let Some(h) = guard.take() {
                    let _ = fs::remove_dir_all(h.dir);
                }
            }
            Ask::Gone => return Err(crate::i18n::t("No se obtuvieron permisos de administrador").to_string()),
        }
    }
    unreachable!("the loop returns on its second pass")
}

enum Ask {
    Done(Result<(), String>),
    Gone,
}

fn start(mas_bin: &str, launcher: &dyn Fn(&str) -> Result<(), String>) -> Result<Helper, String> {
    let dir = std::env::temp_dir().join(format!("updateeer-mas-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    make_private_dir(&dir)?;
    if !Command::new("mkfifo").arg(dir.join("req")).status().map_err(|e| e.to_string())?.success() {
        return Err("No se pudo preparar el ayudante de permisos".into());
    }
    let token = random_token()?;
    let (uid, gid) = (id_of("-u")?, id_of("-g")?);
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let command = format!(
        "/bin/bash -c {} bash {} {uid} {gid} {} {token} {} {} >/dev/null 2>&1 &",
        q(LOOP),
        q(&dir.to_string_lossy()),
        std::process::id(),
        q(mas_bin),
        UPGRADE_TIMEOUT.as_secs()
    );
    launcher(&command)?;
    let ready = dir.join("ready");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.exists() {
        if Instant::now() >= deadline {
            let _ = fs::remove_dir_all(&dir);
            return Err(crate::i18n::t("No se obtuvieron permisos de administrador").to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(Helper { dir, token })
}

/// Sends one request and waits for its answer file.
fn ask(helper: &Helper, id: &str) -> Ask {
    let generation = scan::CANCEL_GEN.load(Ordering::SeqCst);
    let n = REQUESTS.fetch_add(1, Ordering::SeqCst);
    let resp = helper.dir.join(format!("resp.{n}"));
    match open_nonblocking(&helper.dir.join("req")) {
        Ok(mut req) => {
            if writeln!(req, "{} {} {id}", helper.token, resp.display()).is_err() {
                return Ask::Gone;
            }
        }
        Err(_) => return Ask::Gone,
    }
    let deadline = Instant::now() + UPGRADE_TIMEOUT;
    loop {
        if let Ok(text) = fs::read_to_string(&resp) {
            let _ = fs::remove_file(&resp);
            let (code, out) = text.split_once('\n').unwrap_or(("1", &text));
            let out = out.trim();
            return Ask::Done(match code.trim() {
                "0" => Ok(()),
                "124" => Err(crate::i18n::t("El comando tardó demasiado y se canceló").to_string()),
                _ => Err(scan::tail(out)),
            });
        }
        if scan::CANCEL_GEN.load(Ordering::SeqCst) != generation {
            let _ = fs::write(format!("{}.cancel", resp.display()), "");
            return Ask::Done(Err("Cancelado".into()));
        }
        if Instant::now() >= deadline + Duration::from_secs(10) {
            let _ = fs::write(format!("{}.cancel", resp.display()), "");
            return Ask::Done(Err(crate::i18n::t("El comando tardó demasiado y se canceló").to_string()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

static REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The password dialog of macOS, once.
fn admin_launcher(command: &str) -> Result<(), String> {
    let script = format!("do shell script {} with administrator privileges", crate::setup::applescript_string(command));
    scan::run(Command::new("/usr/bin/osascript"), &["-e", &script], Duration::from_secs(120)).map(|_| ()).map_err(|e| {
        if e.contains("-128") { crate::i18n::t("Cancelaste la petición de contraseña").to_string() } else { e }
    })
}

fn id_of(flag: &str) -> Result<String, String> {
    let out = Command::new("id").arg(flag).output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom").map_err(|e| e.to_string())?, &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
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

/// Opens a FIFO for writing without waiting for a reader (fails if there is none).
fn open_nonblocking(path: &Path) -> std::io::Result<fs::File> {
    let mut o = fs::OpenOptions::new();
    o.write(true);
    // O_NONBLOCK: 4 on macOS, 0o4000 on Linux.
    #[cfg(target_os = "macos")]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, 0x4);
    #[cfg(all(unix, not(target_os = "macos")))]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, 0o4000);
    o.open(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Same loop, started as the current user (no dialog) with a stand-in for `mas`.
    #[test]
    fn requests_run_mas_with_sudo_uid_and_bad_ones_do_not() {
        let _turn = scan::CANCEL_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let fake = std::env::temp_dir().join(format!("fake-mas-{}", std::process::id()));
        fs::write(&fake, "#!/bin/sh\nif [ \"$2\" = 999 ]; then echo 'boom' >&2; exit 3; fi\necho \"upgraded $2 as $SUDO_UID:$SUDO_GID\"\n").unwrap();
        fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let launcher = |cmd: &str| Command::new("/bin/sh").arg("-c").arg(cmd).status().map(|_| ()).map_err(|e| e.to_string());
        let bin = fake.to_string_lossy().to_string();
        assert_eq!(upgrade_with(&bin, "123", &launcher), Ok(()));
        assert_eq!(upgrade_with(&bin, "999", &launcher), Err("boom".into()));
        // A request with the wrong token or a non-numeric id never reaches `mas`.
        let helper = HELPER.lock().unwrap();
        let h = helper.as_ref().unwrap();
        let resp = h.dir.join("resp.bad");
        let mut req = open_nonblocking(&h.dir.join("req")).unwrap();
        writeln!(req, "wrong {} 5", resp.display()).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(fs::read_to_string(&resp).unwrap(), "1\nnot allowed\n");
        drop(helper);
        let dir = HELPER.lock().unwrap().take().unwrap().dir;
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(fake);
    }
}
