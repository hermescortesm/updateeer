//! Real app icons on macOS: the bundle's `.icns` converted to a small PNG by the system `sips`,
//! cached on disk. Anything missing falls back to the letter tile.
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::local;
use crate::model::App;

pub struct Icon {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// Command-line tools have no icon.
pub fn wants_icon(app: &App) -> bool {
    cfg!(target_os = "macos") && !app.id.starts_with("formula:")
}

fn cache_file(id: &str) -> Option<PathBuf> {
    let name: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || "@._+-".contains(c) { c } else { '_' }).collect();
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Caches/updateeer/icons").join(format!("{name}.png")))
}

pub fn load(app: &App) -> Option<Icon> {
    let cache = cache_file(&app.id)?;
    if !cache.exists() {
        let icns = icns_of(&local::bundle(app)?)?;
        std::fs::create_dir_all(cache.parent()?).ok()?;
        let ok = Command::new("/usr/bin/sips")
            .args(["-s", "format", "png", "-Z", "96"])
            .arg(&icns)
            .arg("--out")
            .arg(&cache)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?
            .success();
        if !ok {
            return None;
        }
    }
    decode(&cache)
}

/// The icon file named by the bundle's Info.plist (a folder of `.icns` may hold file-type icons too).
fn icns_of(bundle: &Path) -> Option<PathBuf> {
    let res = bundle.join("Contents/Resources");
    let out = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIconFile", "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let declared = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stem = bundle.file_stem()?.to_string_lossy().to_string();
    [declared.clone(), format!("{declared}.icns"), format!("{stem}.icns"), "AppIcon.icns".into()]
        .into_iter()
        .filter(|n| !n.is_empty() && !n.contains('/'))
        .map(|n| res.join(n))
        .find(|p| p.is_file() && p.extension().is_some_and(|e| e == "icns"))
}

fn decode(path: &Path) -> Option<Icon> {
    let mut reader = png::Decoder::new(std::io::BufReader::new(File::open(path).ok()?)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let px = &buf[..info.buffer_size()];
    let size = [info.width as usize, info.height as usize];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => return None,
    };
    (info.bit_depth == png::BitDepth::Eight && rgba.len() == size[0] * size[1] * 4).then_some(Icon { size, rgba })
}
