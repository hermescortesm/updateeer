//! Tells the user when a newer Updateeer is published on GitHub. It only looks: installing is up
//! to the user, from the release page.
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::scan;

const LATEST: &str = "https://api.github.com/repos/hermescortesm/updateeer/releases/latest";
const PAGE_PREFIX: &str = "https://github.com/hermescortesm/updateeer/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

/// Newest published release; blocks, so call it from a worker thread. `curl` ships with macOS,
/// Windows 10+ and nearly every Linux.
pub fn latest() -> Result<Release, String> {
    let mut cmd = Command::new("curl");
    cmd.stdin(Stdio::null());
    let args = ["-fsSL", "--max-time", "15", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: updateeer", LATEST];
    parse(&scan::run(cmd, &args, Duration::from_secs(25))?)
}

fn parse(json: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("Respuesta inesperada de GitHub")?;
    let url = v["html_url"].as_str().filter(|u| u.starts_with(PAGE_PREFIX)).ok_or("Respuesta inesperada de GitHub")?;
    Ok(Release { version: tag.trim_start_matches('v').to_string(), url: url.to_string() })
}

fn parts(v: &str) -> Option<Vec<u64>> {
    v.trim_start_matches('v').split('.').map(|p| p.parse().ok()).collect()
}

/// Whether `latest` is a higher version than `current` (both like `0.1.6`).
pub fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parts(current), parts(latest)), (Some(c), Some(l)) if l > c)
}

/// Opens the release page in the browser.
pub fn open(url: &str) {
    if !url.starts_with(PAGE_PREFIX) {
        return;
    }
    let (tool, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("/usr/bin/open", vec![url])
    } else if cfg!(windows) {
        ("explorer", vec![url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = Command::new(tool).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.1.6", "0.1.7"));
        assert!(is_newer("0.1.9", "0.1.10"));
        assert!(is_newer("0.1.6", "v0.2.0"));
        assert!(!is_newer("0.1.6", "0.1.6"));
        assert!(!is_newer("0.1.7", "0.1.6"));
        assert!(!is_newer("0.1.6", "garbage"));
    }

    #[test]
    fn parses_the_github_answer_and_refuses_foreign_links() {
        let ok = r#"{"tag_name":"v0.1.7","html_url":"https://github.com/hermescortesm/updateeer/releases/tag/v0.1.7"}"#;
        assert_eq!(parse(ok).unwrap(), Release { version: "0.1.7".into(), url: "https://github.com/hermescortesm/updateeer/releases/tag/v0.1.7".into() });
        assert!(parse(r#"{"tag_name":"v1","html_url":"https://evil.example/x"}"#).is_err());
        assert!(parse("not json").is_err());
    }
}
