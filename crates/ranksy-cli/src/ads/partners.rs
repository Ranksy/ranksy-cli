//! Driving the Shopify Partners ads GraphQL from inside a logged-in Chrome tab.
//! The login (Cloudflare + hCaptcha) is always done by a human in the browser window;
//! cookies and the CSRF token never leave the page.

use super::cdp::Tab;
use super::chrome::{self, ChromeOptions};
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

const INPAGE_JS: &str = include_str!("inpage.js");
const PARTNERS_HOST: &str = "https://partners.shopify.com";
pub const ORGANIZATIONS_URL: &str = "https://partners.shopify.com/organizations";

pub const CONNECT_LOGIN_TIMEOUT_SECS: u64 = 600;
pub const SYNC_LOGIN_TIMEOUT_SECS: u64 = 60;
/// How long a headless sync waits for a usable session before opening a window instead.
const HEADLESS_LOGIN_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Deserialize)]
pub struct Org {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PartnersApp {
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub name: String,
    pub campaigns: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PageState {
    /// A Partners organization page with a CSRF token: the session is usable.
    LoggedIn(String),
    /// The organization picker (only reachable when logged in).
    OrgPicker,
    /// Anything else: login form, captcha, still loading.
    Other,
}

pub fn ads_url(org: &str) -> String {
    format!("{PARTNERS_HOST}/{org}/ads")
}

/// Same detection as the Node script: `https://partners.shopify.com/<digits>` plus a CSRF meta tag.
pub fn classify(href: &str, csrf: bool) -> PageState {
    let Some(rest) = href.strip_prefix("https://partners.shopify.com/") else {
        return PageState::Other;
    };
    let first = rest.split(['/', '?', '#']).next().unwrap_or("");
    if !first.is_empty() && first.bytes().all(|b| b.is_ascii_digit()) {
        return if csrf {
            PageState::LoggedIn(first.to_string())
        } else {
            PageState::Other
        };
    }
    if first == "organizations" {
        return PageState::OrgPicker;
    }

    PageState::Other
}

/// A connected Partners tab, and whether this run started the browser it lives in.
pub struct Session {
    pub tab: Tab,
    pub launched: bool,
    port: u16,
}

impl Session {
    /// Drops the tab connection and quits the browser when this run started it; a Chrome
    /// that was already running stays open.
    pub async fn finish(self) {
        self.tab.close().await;
        if !self.launched {
            return;
        }
        if let Err(e) = chrome::close_browser(self.port).await {
            eprintln!("could not close Chrome: {e:#}");
        }
    }
}

/// Launches/attaches Chrome and returns a connected tab pointed at (or on its way to) `start_url`.
pub async fn open_tab(opts: &ChromeOptions, start_url: &str) -> Result<Session> {
    let launched = chrome::ensure_running(opts, start_url).await?;
    let target = chrome::find_or_open_tab(opts.port, start_url, launched).await?;
    let tab = match Tab::connect(&target.ws_url).await {
        Ok(tab) => tab,
        Err(e) => {
            if launched {
                let _ = chrome::close_browser(opts.port).await;
            }
            return Err(e);
        }
    };

    Ok(Session {
        tab,
        launched,
        port: opts.port,
    })
}

/// Opens the Partners tab for a sync. A Chrome started headless whose Partners session has
/// expired is quit and started again with a window, so the login can be done by hand.
pub async fn open_sync_tab(opts: &ChromeOptions, org: &str) -> Result<Session> {
    let session = open_tab(opts, &ads_url(org)).await?;
    if !session.launched || !opts.headless {
        return Ok(session);
    }
    let probe = wait_for_login(
        &session.tab,
        LoginWait {
            timeout_secs: HEADLESS_LOGIN_TIMEOUT_SECS,
            wanted_org: Some(org),
            interactive: false,
        },
    )
    .await;
    if probe.is_ok() {
        return Ok(session);
    }

    session.finish().await;
    println!(
        "The Partners session needs a login. Log in to Shopify Partners in the Chrome window… (waiting up to {SYNC_LOGIN_TIMEOUT_SECS} seconds)"
    );
    open_tab(
        &ChromeOptions {
            headless: false,
            ..opts.clone()
        },
        &ads_url(org),
    )
    .await
}

pub struct LoginWait<'a> {
    pub timeout_secs: u64,
    /// The org the caller wants (config / `--org`); otherwise whichever the session lands on.
    pub wanted_org: Option<&'a str>,
    /// Interactive: say that a login is needed and offer the org picker.
    pub interactive: bool,
}

/// Polls the tab until it is a logged-in Partners organization page on the ads section.
/// Returns the organization id.
pub async fn wait_for_login(tab: &Tab, wait: LoginWait<'_>) -> Result<String> {
    let start_url = wait
        .wanted_org
        .map(ads_url)
        .unwrap_or_else(|| ORGANIZATIONS_URL.to_string());
    let mut prompted = false;
    let mut chosen: Option<String> = wait.wanted_org.map(str::to_string);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait.timeout_secs);
    for i in 0u32.. {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        // A tab that is mid-navigation (or hung) must not stall the poll.
        let state = tokio::time::timeout(
            Duration::from_secs(10),
            tab.evaluate(r#"({ href: location.href, csrf: !!document.querySelector('meta[name="csrf-token"]') })"#),
        )
        .await
        .ok()
        .and_then(Result::ok);
        let href = state
            .as_ref()
            .and_then(|s| s.get("href"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let csrf = state
            .as_ref()
            .and_then(|s| s.get("csrf"))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if state.is_some() && i == 0 && !href.starts_with(PARTNERS_HOST) {
            // Parked on a blank/foreign page or a stale login screen: head for Partners.
            tab.navigate(&start_url).await?;
        }

        match classify(href, csrf) {
            PageState::LoggedIn(found) => {
                let org = chosen.clone().unwrap_or(found);
                if !href.starts_with(&ads_url(&org)) {
                    tab.navigate(&ads_url(&org)).await?;
                    tokio::time::sleep(Duration::from_secs(4)).await;
                }

                return Ok(org);
            }
            PageState::OrgPicker if chosen.is_none() && wait.interactive => {
                let orgs = list_orgs(tab).await.unwrap_or_default();
                if !orgs.is_empty() {
                    let org = pick_org(&orgs)?;
                    chosen = Some(org.clone());
                    tab.navigate(&ads_url(&org)).await?;
                    tokio::time::sleep(Duration::from_secs(4)).await;
                    continue;
                }
            }
            _ => {}
        }

        if wait.interactive && !prompted && state.is_some() && !href.starts_with(PARTNERS_HOST) {
            println!(
                "Log in to Shopify Partners in the Chrome window… (waiting up to {} minutes)",
                wait.timeout_secs / 60
            );
            prompted = true;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    if wait.interactive {
        bail!("timed out waiting for the Partners login");
    }
    bail!("Partners session expired — run `ranksy ads connect`")
}

fn pick_org(orgs: &[Org]) -> Result<String> {
    if let [only] = orgs {
        return Ok(only.id.clone());
    }
    let labels: Vec<String> = orgs
        .iter()
        .map(|o| {
            if o.name.is_empty() {
                o.id.clone()
            } else {
                format!("{} ({})", o.name, o.id)
            }
        })
        .collect();
    let picked = dialoguer::Select::new()
        .with_prompt("Which Partners organization runs your ads?")
        .items(&labels)
        .default(0)
        .interact()
        .context("could not read the organization choice (pass --org <id> to skip the prompt)")?;

    Ok(orgs[picked].id.clone())
}

/// The JavaScript sent to `Runtime.evaluate`: the in-page library, then one call.
/// Mirrors the Node script's `runInPage`: `func(gql, org, ...args)` with JSON-encoded arguments.
pub fn in_page_expression(func: &str, org: &str, args: &[Value]) -> String {
    let org_json = Value::String(org.to_string()).to_string();
    let call_args = std::iter::once(org_json.clone())
        .chain(args.iter().map(Value::to_string))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "(async () => {{\n{INPAGE_JS}\nconst gql = inPageGql({org_json});\ntry {{ return await {func}(gql, {call_args}); }} catch (e) {{ return {{ error: String(e.message ?? e) }}; }}\n}})()"
    )
}

pub async fn run_in_page(tab: &Tab, func: &str, org: &str, args: &[Value]) -> Result<Value> {
    let value = tab.evaluate(&in_page_expression(func, org, args)).await?;
    if let Some(error) = value.get("error") {
        bail!(
            "GraphQL pull failed: {} (if the Partners session expired, run `ranksy ads connect`)",
            error
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string())
        );
    }

    Ok(value)
}

async fn list_orgs(tab: &Tab) -> Result<Vec<Org>> {
    let value = run_in_page(tab, "inPageListOrgs", "", &[]).await?;

    Ok(serde_json::from_value(
        value.get("orgs").cloned().unwrap_or(json!([])),
    )?)
}

/// Apps that have ad campaigns in this organization.
pub async fn list_apps(tab: &Tab, org: &str) -> Result<Vec<PartnersApp>> {
    let value = run_in_page(tab, "inPageApps", org, &[]).await?;

    serde_json::from_value(value.get("apps").cloned().unwrap_or(json!([])))
        .map_err(|e| anyhow!("unexpected apps response: {e}"))
}

/// Campaign metadata + keyword snapshots (current state), raw as returned by Partners.
///
/// `only` limits the per-campaign pulls (keywords, negatives, recommendations, keyword ideas)
/// to the apps with those config keys; the campaign list itself is always organization-wide.
pub async fn snapshot(tab: &Tab, org: &str, only: Option<&[String]>) -> Result<Snapshot> {
    let value = run_in_page(tab, "inPageSnapshot", org, &[json!(only)]).await?;

    Ok(Snapshot::from_value(&value))
}

/// Current campaign settings plus Shopify's keyword ideas per app (by app API key).
#[derive(Debug, Default)]
pub struct Snapshot {
    pub campaigns: Vec<Value>,
    pub keyword_ideas: std::collections::BTreeMap<String, Vec<Value>>,
}

impl Snapshot {
    pub fn from_value(value: &Value) -> Self {
        Self {
            campaigns: value
                .get("campaigns")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            keyword_ideas: value
                .get("keywordIdeas")
                .and_then(Value::as_object)
                .map(|ideas| {
                    ideas
                        .iter()
                        .map(|(key, list)| {
                            (key.clone(), list.as_array().cloned().unwrap_or_default())
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// One day of campaign metrics, breakdowns and search terms, raw as returned by Partners.
///
/// `only` limits the per-campaign pulls (breakdowns, search terms) to the apps with those
/// config keys and drops the other campaigns; the day's campaign list is organization-wide.
pub async fn pull_day(
    tab: &Tab,
    org: &str,
    day: &str,
    only: Option<&[String]>,
) -> Result<Vec<Value>> {
    let value = run_in_page(tab, "inPagePullDay", org, &[json!(day), json!(only)]).await?;

    Ok(value
        .get("campaigns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_pages() {
        assert_eq!(
            classify("https://partners.shopify.com/1234567/ads", true),
            PageState::LoggedIn("1234567".into())
        );
        assert_eq!(
            classify("https://partners.shopify.com/1234567", true),
            PageState::LoggedIn("1234567".into())
        );
        assert_eq!(
            classify("https://partners.shopify.com/1234567/ads", false),
            PageState::Other
        );
        assert_eq!(
            classify("https://partners.shopify.com/organizations", true),
            PageState::OrgPicker
        );
        assert_eq!(
            classify("https://partners.shopify.com/organizations?x=1", false),
            PageState::OrgPicker
        );
        assert_eq!(
            classify("https://accounts.shopify.com/lookup", true),
            PageState::Other
        );
        assert_eq!(
            classify("https://partners.shopify.com/", true),
            PageState::Other
        );
    }

    #[test]
    fn builds_in_page_call_with_json_args() {
        let js = in_page_expression("inPagePullDay", "4521", &[json!("2026-10-15")]);
        assert!(js.starts_with("(async () => {"));
        assert!(js.contains("function inPageGql(org)"));
        assert!(js.contains("const gql = inPageGql(\"4521\");"));
        assert!(js.contains("await inPagePullDay(gql, \"4521\", \"2026-10-15\")"));
        assert!(js.ends_with("})()"));
    }

    #[test]
    fn passes_the_app_filter_to_the_page_or_null() {
        let only = ["key1".to_string(), "Named App".to_string()];
        let js = in_page_expression(
            "inPagePullDay",
            "4521",
            &[json!("2026-10-15"), json!(Some(&only[..]))],
        );
        assert!(js.contains(
            "await inPagePullDay(gql, \"4521\", \"2026-10-15\", [\"key1\",\"Named App\"])"
        ));

        let js = in_page_expression("inPageSnapshot", "4521", &[json!(None::<&[String]>)]);
        assert!(js.contains("await inPageSnapshot(gql, \"4521\", null)"));
    }

    #[test]
    fn in_page_library_defines_every_called_function() {
        for f in [
            "inPageGql",
            "inPageListOrgs",
            "inPageApps",
            "inPageSnapshot",
            "inPagePullDay",
            "inPageKeeps",
        ] {
            assert!(INPAGE_JS.contains(&format!("function {f}(")), "{f} missing");
        }
    }

    #[test]
    fn ads_url_targets_the_org() {
        assert_eq!(ads_url("99"), "https://partners.shopify.com/99/ads");
    }
}
