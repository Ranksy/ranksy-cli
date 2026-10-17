//! Shopify App Store ads -> Ranksy.
//!
//! `ranksy ads connect` logs in to Shopify Partners once (in a Chrome window you drive) and
//! maps your Shopify apps to Ranksy apps. `ranksy ads sync` re-uses that Chrome profile to
//! pull ads data from Partners and push it to `POST /apps/{app}/shopify-ads/sync`.
//! Partners sits behind Cloudflare + hCaptcha, so every Partners request runs inside the
//! real Chrome page over CDP; credentials and cookies never reach this process.

mod cdp;
mod chrome;
mod connect;
mod disconnect;
mod partners;
mod payload;
mod sync;

use crate::cli::{AdsCmd, ChromeArgs};
use crate::config::{self, AdsConfig, ConfigFile};
use anyhow::{anyhow, Result};
use chrome::ChromeOptions;
use ranksy_api::{ApiError, ClientConfig, RanksyClient};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub const DEFAULT_PORT: u16 = 9333;
const NOT_CONNECTED: &str = "Shopify Partners is not connected. Run `ranksy ads connect` first";

static QUIET: AtomicBool = AtomicBool::new(false);

/// What `run` needs from the top-level CLI: key/base-url flags (env already folded in by clap).
pub struct Ctx {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub quiet: bool,
    pub file: ConfigFile,
}

impl Ctx {
    /// A Ranksy client from the usual key precedence: flag > env > config.
    fn client(&self) -> Result<RanksyClient> {
        let resolved = config::resolve(
            self.api_key.clone(),
            None,
            self.base_url.clone(),
            None,
            None,
            &self.file,
        )
        .map_err(|e| anyhow!(e.to_string()))?;

        Ok(RanksyClient::new(ClientConfig {
            api_key: resolved.api_key,
            base_url: resolved.base_url,
        })?)
    }
}

pub async fn run(cmd: &AdsCmd, ctx: Ctx) -> Result<()> {
    QUIET.store(ctx.quiet, Ordering::Relaxed);

    match cmd {
        AdsCmd::Connect { chrome, org } => connect::run(&ctx, chrome, org.as_deref()).await,
        AdsCmd::Disconnect { org } => disconnect::run(&ctx, org),
        AdsCmd::Sync {
            chrome,
            org,
            from,
            to,
            after,
            before,
            days,
            chunk_days,
            concurrency,
            dry_run,
            resume,
            out,
        } => {
            let ads = ctx.file.ads.clone().ok_or_else(|| anyhow!(NOT_CONNECTED))?;
            let orgs = sync::select_orgs(&ads, org.as_deref())?;
            let (from, to) = sync::inclusive_bounds(*from, *to, *after, *before)?;
            let opts = sync::SyncOptions {
                from,
                to,
                days: *days,
                chunk_days: *chunk_days as usize,
                concurrency: *concurrency as usize,
                dry_run: *dry_run,
                resume: *resume,
                out: out.clone().unwrap_or_else(default_cache_dir),
            };

            sync::run(&ctx, &ads, chrome_options(chrome, Some(&ads)), &opts, &orgs).await
        }
    }
}

/// "1, 2" for messages, or "none".
fn describe_orgs(orgs: &[String]) -> String {
    if orgs.is_empty() {
        return "none".to_string();
    }

    orgs.join(", ")
}

/// Flags win over the `[ads]` config, which wins over the defaults.
fn chrome_options(args: &ChromeArgs, ads: Option<&AdsConfig>) -> ChromeOptions {
    ChromeOptions {
        bin: args.chrome.clone(),
        port: args
            .port
            .or_else(|| ads.and_then(|a| a.port))
            .unwrap_or(DEFAULT_PORT),
        profile: args
            .profile
            .clone()
            .or_else(|| {
                ads.and_then(|a| a.chrome_profile.clone())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(default_profile),
    }
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "ranksy", "ranksy")
}

/// The dedicated Chrome profile that keeps the Partners login.
pub fn default_profile() -> PathBuf {
    project_dirs().map_or_else(
        || PathBuf::from(".ranksy/partners-chrome"),
        |d| d.data_dir().join("partners-chrome"),
    )
}

/// Where raw day exports and payload files go.
pub fn default_cache_dir() -> PathBuf {
    project_dirs().map_or_else(
        || PathBuf::from(".ranksy/ads-export"),
        |d| d.cache_dir().join("ads-export"),
    )
}

/// clap value parser for `--from` / `--to`.
pub fn parse_date(s: &str) -> Result<chrono::NaiveDate, String> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| format!("'{s}' is not a date; use YYYY-MM-DD"))
}

fn is_quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// A timestamped progress line, unless `--quiet`.
fn log(msg: &str) {
    if !is_quiet() {
        println!("[{}] {msg}", chrono::Utc::now().format("%H:%M:%S"));
    }
}

/// A readable error for a failed Ranksy call made while connecting.
fn ranksy_error(e: ApiError, what: &str) -> anyhow::Error {
    match e {
        ApiError::Status { code: 401, .. } => anyhow!(
            "Ranksy rejected the API key (401). Check `ranksy login`, RANKSY_API_KEY or --api-key."
        ),
        ApiError::Status { code: 403, .. } => {
            anyhow!("The API key is missing a scope (403): it needs `apps:read` and `ads:write`.")
        }
        other => anyhow!("{what}: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(port: Option<u16>, profile: Option<&str>) -> ChromeArgs {
        ChromeArgs {
            chrome: None,
            port,
            profile: profile.map(PathBuf::from),
        }
    }

    #[test]
    fn chrome_options_prefer_flags_then_config_then_defaults() {
        let ads = AdsConfig {
            port: Some(9444),
            chrome_profile: Some("/cfg/profile".into()),
            ..Default::default()
        };

        let o = chrome_options(&args(Some(9555), Some("/flag/profile")), Some(&ads));
        assert_eq!((o.port, o.profile), (9555, PathBuf::from("/flag/profile")));

        let o = chrome_options(&args(None, None), Some(&ads));
        assert_eq!((o.port, o.profile), (9444, PathBuf::from("/cfg/profile")));

        let o = chrome_options(&args(None, None), None);
        assert_eq!((o.port, o.profile), (DEFAULT_PORT, default_profile()));
    }

    #[test]
    fn describes_orgs() {
        assert_eq!(describe_orgs(&[]), "none");
        assert_eq!(describe_orgs(&["1".into(), "2".into()]), "1, 2");
    }

    #[test]
    fn parses_dates() {
        assert!(parse_date("2026-10-15").is_ok());
        assert!(parse_date("15/10/2026").is_err());
        assert!(parse_date("2026-13-01").is_err());
    }

    #[test]
    fn default_dirs_end_in_the_documented_names() {
        assert!(default_profile().ends_with("partners-chrome"));
        assert!(default_cache_dir().ends_with("ads-export"));
    }
}
