//! "Open at login". macOS: a LaunchAgent that starts the app hidden in the menu bar.
use std::path::PathBuf;

fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/LaunchAgents/com.hermescortes.updateeer.plist"))
}

pub fn enabled() -> bool {
    cfg!(target_os = "macos") && plist_path().is_some_and(|p| p.exists())
}

pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

pub fn set(on: bool) -> Result<(), String> {
    let path = plist_path().filter(|_| supported()).ok_or("No disponible en este sistema todavía")?;
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
    std::fs::create_dir_all(path.parent().ok_or("sin carpeta")?).map_err(|e| e.to_string())?;
    std::fs::write(&path, plist(&exe)).map_err(|e| e.to_string())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

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
    fn plist_escapes_the_path() {
        let p = plist("/Applications/A&B <x>/updateeer");
        assert!(p.contains("/Applications/A&amp;B &lt;x&gt;/updateeer"));
        assert!(p.contains("--background"));
    }
}
