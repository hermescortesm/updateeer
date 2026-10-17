#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    AppStore,
    Homebrew,
    Direct,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::AppStore => "App Store",
            Source::Homebrew => "Homebrew",
            Source::Direct => "Directa",
        }
    }
}

#[derive(Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub from: String,
    pub to: String,
    pub source: Source,
    pub size_mb: u32,
    /// Hue (degrees) for the placeholder icon tile.
    pub hue: f32,
}

pub fn format_size(mb: u32) -> String {
    if mb >= 1000 {
        format!("{:.1} GB", mb as f32 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

/// Placeholder data from the design until the macOS scanner lands.
pub fn mock_apps() -> Vec<App> {
    let a = |id: &str, name: &str, vendor: &str, from: &str, to: &str, source, size_mb, hue| App {
        id: id.into(),
        name: name.into(),
        vendor: vendor.into(),
        from: from.into(),
        to: to.into(),
        source,
        size_mb,
        hue,
    };
    use Source::*;
    vec![
        a("ff", "Firefox", "Mozilla", "130.0", "131.0.2", Direct, 138, 40.0),
        a("vs", "Visual Studio Code", "Microsoft", "1.93.1", "1.94.2", Homebrew, 112, 250.0),
        a("dk", "Docker Desktop", "Docker", "4.33.0", "4.34.2", Direct, 612, 230.0),
        a("sl", "Slack", "Slack Technologies", "4.40.126", "4.41.97", AppStore, 186, 330.0),
        a("vl", "VLC", "VideoLAN", "3.0.20", "3.0.21", Homebrew, 54, 65.0),
        a("fg", "Figma", "Figma", "124.3.2", "124.4.7", Direct, 98, 300.0),
        a("nd", "Node.js", "OpenJS Foundation", "20.17.0", "22.9.0", Homebrew, 41, 145.0),
        a("ob", "Obsidian", "Dynalist", "1.6.7", "1.7.4", Direct, 102, 280.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(format_size(138), "138 MB");
        assert_eq!(format_size(612), "612 MB");
        assert_eq!(format_size(2048), "2.0 GB");
    }
}
