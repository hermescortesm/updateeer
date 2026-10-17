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
    pub size_mb: Option<u32>,
    /// Hue (degrees) for the placeholder icon tile.
    pub hue: f32,
}

pub fn format_size(mb: Option<u32>) -> String {
    let Some(mb) = mb else { return "—".into() };
    if mb >= 1000 {
        format!("{:.1} GB", mb as f32 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(format_size(Some(138)), "138 MB");
        assert_eq!(format_size(Some(612)), "612 MB");
        assert_eq!(format_size(Some(2048)), "2.0 GB");
        assert_eq!(format_size(None), "—");
    }
}
