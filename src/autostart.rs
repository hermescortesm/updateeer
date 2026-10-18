//! "Open at login". macOS: a LaunchAgent; Windows: a `Run` registry key; Linux: an XDG autostart
//! entry. All start the app with `--background`.
use std::path::PathBuf;

#[cfg_attr(windows, allow(dead_code))]
fn home_file(rel: &str) -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(rel))
}

#[cfg_attr(windows, allow(dead_code))]
fn plist_path() -> Option<PathBuf> {
    home_file("Library/LaunchAgents/com.hermescortes.updateeer.plist")
}

#[cfg_attr(windows, allow(dead_code))]
fn desktop_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home_file(".config"))?;
    Some(base.join("autostart/updateeer.desktop"))
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// `reg.exe` without a console window flashing up.
#[cfg(windows)]
fn reg(args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut cmd = std::process::Command::new("reg");
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
    cmd.args(args).stdin(std::process::Stdio::null()).output()
}

pub fn enabled() -> bool {
    #[cfg(windows)]
    return reg(&["query", RUN_KEY, "/v", "Updateeer"]).is_ok_and(|o| o.status.success());
    #[cfg(not(windows))]
    if cfg!(target_os = "macos") { plist_path() } else { desktop_path() }.is_some_and(|p| p.exists())
}

pub fn supported() -> bool {
    true
}

pub fn set(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let out = if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            reg(&["add", RUN_KEY, "/v", "Updateeer", "/t", "REG_SZ", "/d", &format!("\"{}\" --background", exe.display()), "/f"])
        } else {
            reg(&["delete", RUN_KEY, "/v", "Updateeer", "/f"])
        };
        // Deleting a value that is not there is fine.
        match out {
            Ok(o) if o.status.success() || !on => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => Err(e.to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let macos = cfg!(target_os = "macos");
        let path = if macos { plist_path() } else { desktop_path() }.ok_or("No se encontró la carpeta personal")?;
        if !on {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.to_string_lossy();
        // A path inside a mounted dmg or a translocated copy disappears: ask for a real install.
        if exe.starts_with("/Volumes/") || exe.contains("/AppTranslocation/") {
            return Err("Mueve Updateeer a Aplicaciones antes de activarlo".into());
        }
        let text = if macos { plist(&exe) } else { desktop(&exe).ok_or("La ruta de la app tiene caracteres no admitidos")? };
        std::fs::create_dir_all(path.parent().ok_or("sin carpeta")?).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }
}

/// XDG autostart entry. `Exec` is quoted; paths with characters the spec treats specially are refused.
#[cfg_attr(windows, allow(dead_code))]
fn desktop(exe: &str) -> Option<String> {
    if exe.chars().any(|c| matches!(c, '"' | '\\' | '`' | '$' | '%' | '\n')) {
        return None;
    }
    Some(format!("[Desktop Entry]\nType=Application\nName=Updateeer\nExec=\"{exe}\" --background\nIcon=updateeer\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"))
}

#[cfg_attr(windows, allow(dead_code))]
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn plist(exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.hermescortes.updateeer</string>
  <key>ProgramArguments</key>
  <array><string>{}</string><string>--background</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
        escape(exe)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_quotes_the_path_and_refuses_odd_ones() {
        let d = desktop("/opt/My App/updateeer").unwrap();
        assert!(d.contains("Exec=\"/opt/My App/updateeer\" --background"));
        assert!(desktop("/tmp/a\"b").is_none());
        assert!(desktop("/tmp/$HOME").is_none());
    }

    #[test]
    fn plist_escapes_the_path() {
        let p = plist("/Applications/A&B <x>/updateeer");
        assert!(p.contains("/Applications/A&amp;B &lt;x&gt;/updateeer"));
        assert!(p.contains("--background"));
    }
}
