//! Runs the actual update for an app id (`cask:<token>`, `formula:<name>` or `mas:<number>`).
use crate::{mas, winpkg};
use crate::scan::{brew, safe_token, UPGRADE_TIMEOUT};

fn args(id: &str) -> Option<[&str; 3]> {
    let (kind, name) = id.split_once(':')?;
    if !safe_token(name) {
        return None;
    }
    match kind {
        "cask" => Some(["upgrade", "--cask", name]),
        "formula" => Some(["upgrade", "--formula", name]),
        _ => None,
    }
}

/// Blocks until brew finishes; call it from a worker thread.
pub fn run(id: &str) -> Result<(), String> {
    if let Some(app_id) = id.strip_prefix("mas:") {
        return mas::upgrade(app_id);
    }
    if let Some((kind @ ("winget" | "scoop" | "choco"), name)) = id.split_once(':') {
        return winpkg::upgrade(kind, name);
    }
    let args = args(id).ok_or_else(|| format!("id no soportado: {id}"))?;
    brew(&args, UPGRADE_TIMEOUT, false).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_args_and_rejects_bad_ids() {
        assert_eq!(args("cask:firefox"), Some(["upgrade", "--cask", "firefox"]));
        assert_eq!(args("formula:node@22"), Some(["upgrade", "--formula", "node@22"]));
        assert_eq!(args("formula:user/tap/engram"), Some(["upgrade", "--formula", "user/tap/engram"]));
        assert_eq!(args("cask:--force"), None);
        assert_eq!(args("cask:a b"), None);
        assert_eq!(args("mas:123"), None);
    }
}
