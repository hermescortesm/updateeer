//! Runs the actual update for an app id (`cask:<token>` / `formula:<name>`).
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
