//! Launching (or attaching to) a visible Chrome with a persistent profile and a
//! remote-debugging port, and finding the Partners tab to drive over CDP.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

const PATH_CANDIDATES: [&str; 4] = [
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
];
const MACOS_CHROME: &str = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

#[derive(Debug, Clone)]
pub struct ChromeOptions {
    /// Explicit browser binary (`--chrome` / `CHROME_BIN`); searched for on PATH when not a path.
    pub bin: Option<PathBuf>,
    pub port: u16,
    pub profile: PathBuf,
}

/// A debuggable tab: its WebSocket URL, and whether we started the browser for it.
pub struct TabTarget {
    pub ws_url: String,
}

fn http() -> Result<reqwest::Client> {
    // Never route the local debug port through a proxy from the environment.
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?)
}

async fn is_up(port: u16) -> bool {
    let Ok(client) = http() else { return false };
    match client
        .get(format!("http://127.0.0.1:{port}/json/version"))
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(_) => false,
    }
}

/// Makes sure something is listening on the debug port, launching Chrome when nothing is.
/// Returns true when this call launched the browser.
pub async fn ensure_running(opts: &ChromeOptions, start_url: &str) -> Result<bool> {
    if is_up(opts.port).await {
        return Ok(false);
    }

    let bin = resolve_binary(
        opts.bin.as_deref(),
        std::env::var_os("PATH").as_deref(),
        &[PathBuf::from(MACOS_CHROME)],
    )?;
    std::fs::create_dir_all(&opts.profile).with_context(|| {
        format!(
            "cannot create Chrome profile dir {}",
            opts.profile.display()
        )
    })?;

    let mut cmd = std::process::Command::new(&bin);
    cmd.args(launch_args(
        &opts.profile,
        opts.port,
        start_url,
        cfg!(target_os = "linux"),
    ))
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        // Own process group: Ctrl+C in the terminal must not close the browser (and its login).
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .with_context(|| format!("cannot start {}", bin.display()))?;

    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if is_up(opts.port).await {
            return Ok(true);
        }
    }
    bail!(
        "Chrome did not open a debug port on {}. If Chrome is already running with this profile ({}), close it first; or pass --chrome / set CHROME_BIN.",
        opts.port,
        opts.profile.display()
    );
}

/// Chrome's command-line for the dedicated Partners profile.
pub fn launch_args(profile: &Path, port: u16, start_url: &str, linux: bool) -> Vec<String> {
    let mut args = vec![
        format!("--user-data-dir={}", profile.display()),
        format!("--remote-debugging-port={port}"),
    ];
    if linux {
        args.push("--ozone-platform-hint=auto".to_string());
    }
    args.push("--no-first-run".to_string());
    args.push("--no-default-browser-check".to_string());
    args.push(start_url.to_string());

    args
}

/// Picks the Chrome binary: the explicit one, else the first candidate on PATH, else known app paths.
pub fn resolve_binary(
    explicit: Option<&Path>,
    path_var: Option<&OsStr>,
    app_paths: &[PathBuf],
) -> Result<PathBuf> {
    if let Some(bin) = explicit {
        if bin.components().count() > 1 {
            return Ok(bin.to_path_buf());
        }
        return find_in_path(bin.as_os_str(), path_var).ok_or_else(|| {
            anyhow!(
                "{} not found on PATH (from --chrome / CHROME_BIN)",
                bin.display()
            )
        });
    }
    for name in PATH_CANDIDATES {
        if let Some(found) = find_in_path(OsStr::new(name), path_var) {
            return Ok(found);
        }
    }
    if let Some(found) = app_paths.iter().find(|p| p.is_file()) {
        return Ok(found.clone());
    }

    Err(anyhow!(
        "could not find Chrome (tried {}). Install it, or pass --chrome <path> / set CHROME_BIN.",
        PATH_CANDIDATES.join(", ")
    ))
}

fn find_in_path(name: &OsStr, path_var: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path_var?)
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Finds the Partners tab (or opens one at `start_url`) and returns its WebSocket URL.
pub async fn find_or_open_tab(port: u16, start_url: &str, launched: bool) -> Result<TabTarget> {
    let client = http()?;
    let list: Vec<Value> = client
        .get(format!("http://127.0.0.1:{port}/json/list"))
        .send()
        .await?
        .json()
        .await
        .context("unexpected response from Chrome's /json/list")?;
    if let Some(ws_url) = pick_tab(&list, launched) {
        return Ok(TabTarget { ws_url });
    }

    let created: Value = client
        .put(format!("http://127.0.0.1:{port}/json/new?{start_url}"))
        .send()
        .await?
        .json()
        .await
        .context("unexpected response from Chrome's /json/new")?;
    let ws_url = created
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Chrome did not return a debugger URL for the new tab"))?;

    Ok(TabTarget {
        ws_url: ws_url.to_string(),
    })
}

/// An existing Shopify tab; for a browser we just launched, its first page (the start URL may
/// not have committed yet, which would otherwise leave us with two tabs).
fn pick_tab(list: &[Value], launched: bool) -> Option<String> {
    let pages = || {
        list.iter()
            .filter(|t| t.get("type").and_then(Value::as_str) == Some("page"))
    };
    let ws = |t: &Value| {
        t.get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let shopify = |t: &&Value| {
        t.get("url").and_then(Value::as_str).is_some_and(|u| {
            u.starts_with("https://partners.shopify.com")
                || u.starts_with("https://accounts.shopify.com")
        })
    };

    pages().find(shopify).and_then(ws).or_else(|| {
        if launched {
            pages().next().and_then(ws)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn launch_args_for_linux_and_other() {
        let args = launch_args(
            Path::new("/p"),
            9333,
            "https://partners.shopify.com/organizations",
            true,
        );
        assert_eq!(
            args,
            [
                "--user-data-dir=/p",
                "--remote-debugging-port=9333",
                "--ozone-platform-hint=auto",
                "--no-first-run",
                "--no-default-browser-check",
                "https://partners.shopify.com/organizations"
            ]
        );
        assert!(!launch_args(Path::new("/p"), 1, "u", false)
            .contains(&"--ozone-platform-hint=auto".to_string()));
    }

    #[test]
    fn picks_existing_shopify_tab_first() {
        let list = vec![
            json!({ "type": "page", "url": "https://example.com", "webSocketDebuggerUrl": "ws://a" }),
            json!({ "type": "service_worker", "url": "https://partners.shopify.com/x", "webSocketDebuggerUrl": "ws://sw" }),
            json!({ "type": "page", "url": "https://accounts.shopify.com/lookup", "webSocketDebuggerUrl": "ws://b" }),
        ];
        assert_eq!(pick_tab(&list, false).as_deref(), Some("ws://b"));
    }

    #[test]
    fn falls_back_to_first_page_only_for_a_browser_we_launched() {
        let list =
            vec![json!({ "type": "page", "url": "about:blank", "webSocketDebuggerUrl": "ws://a" })];
        assert_eq!(pick_tab(&list, true).as_deref(), Some("ws://a"));
        assert_eq!(pick_tab(&list, false), None);
    }

    #[cfg(unix)]
    #[test]
    fn resolves_binary_from_path_and_explicit_name() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ranksy-chrome-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("chromium");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path_var = std::env::join_paths([&dir]).unwrap();

        assert_eq!(resolve_binary(None, Some(&path_var), &[]).unwrap(), fake);
        assert_eq!(
            resolve_binary(Some(Path::new("chromium")), Some(&path_var), &[]).unwrap(),
            fake
        );
        assert_eq!(
            resolve_binary(Some(Path::new("/opt/chrome/chrome")), Some(&path_var), &[]).unwrap(),
            PathBuf::from("/opt/chrome/chrome")
        );
        assert!(resolve_binary(Some(Path::new("nope")), Some(&path_var), &[]).is_err());
        assert!(resolve_binary(None, Some(OsStr::new("/nonexistent")), &[]).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
