//! `ranksy ads connect`: one-time, interactive setup.

use super::chrome::{self, ChromeOptions};
use super::partners::{self, PartnersApp};
use super::sync::{self, SyncOptions};
use super::{chrome_options, default_profile, describe_orgs, ranksy_error, Ctx, DEFAULT_PORT};
use crate::cli::ChromeArgs;
use crate::config::{self, AdsApp, AdsConfig};
use anyhow::{anyhow, bail, Context, Result};
use dialoguer::{Confirm, FuzzySelect};
use ranksy_api::RanksyClient;
use serde_json::Value;

const SKIP_LABEL: &str = "Skip (don't sync this app)";

#[derive(Debug, Clone, PartialEq)]
pub struct RanksyApp {
    pub ulid: String,
    pub name: String,
    pub slug: Option<String>,
}

pub async fn run(ctx: &Ctx, chrome_args: &ChromeArgs, org_flag: Option<&str>) -> Result<()> {
    let existing = ctx.file.ads.clone();
    let opts = chrome_options(chrome_args, existing.as_ref());

    // Check the API key and list the Ranksy apps before opening a browser.
    let client = ctx.client()?;
    let ranksy_apps = load_ranksy_apps(&client).await?;
    if ranksy_apps.is_empty() {
        bail!("No Ranksy apps are visible to this API key. The key's team must own the app(s) you want to import ads into.");
    }

    println!(
        "Opening Chrome with the Partners profile at {}",
        opts.profile.display()
    );
    let start_url = org_flag.map_or_else(
        || partners::ORGANIZATIONS_URL.to_string(),
        partners::ads_url,
    );
    let session = partners::open_tab(&opts, &start_url).await?;
    let launched = session.launched;
    let result = login_and_map(
        &session.tab,
        org_flag,
        existing.as_ref(),
        &opts,
        &ranksy_apps,
    )
    .await;
    // Leave the window open for the first sync; it quits Chrome at the end of `run` instead.
    let (ads, org) = match result {
        Ok(Some(done)) => done,
        Ok(None) => {
            session.finish().await;
            return Ok(());
        }
        Err(e) => {
            session.finish().await;
            return Err(e);
        }
    };
    session.tab.close().await;

    let run_now = Confirm::new()
        .with_prompt(format!(
            "Run the first sync for organization {org} now (last 3 complete days)?"
        ))
        .default(false)
        .interact()
        .unwrap_or(false);
    let synced = if run_now {
        sync::run(
            ctx,
            &ads,
            opts.clone(),
            &SyncOptions::first_run(),
            std::slice::from_ref(&org),
        )
        .await
    } else {
        Ok(())
    };
    if launched {
        if let Err(e) = chrome::close_browser(opts.port).await {
            eprintln!("could not close Chrome: {e:#}");
        }
    }

    synced
}

/// Login, app mapping and saving the config. `None` when nothing in `org` is mapped.
async fn login_and_map(
    tab: &super::cdp::Tab,
    org_flag: Option<&str>,
    existing: Option<&AdsConfig>,
    opts: &ChromeOptions,
    ranksy_apps: &[RanksyApp],
) -> Result<Option<(AdsConfig, String)>> {
    let org = partners::wait_for_login(
        tab,
        partners::LoginWait {
            timeout_secs: partners::CONNECT_LOGIN_TIMEOUT_SECS,
            wanted_org: org_flag,
            interactive: true,
        },
    )
    .await?;
    println!("Logged in to Partners organization {org}.");

    let partners_apps = partners::list_apps(tab, &org).await?;
    if partners_apps.is_empty() {
        bail!("No ad campaigns found in Partners organization {org}. Pass --org <id> if your ads live in another organization.");
    }

    let current = existing.map(|a| &a.apps);
    let mut choices: Vec<(PartnersApp, Option<RanksyApp>)> = Vec::new();
    for app in partners_apps {
        let picked =
            choose_ranksy_app(&app, ranksy_apps, current.and_then(|m| m.get(&app.api_key)))?;
        choices.push((app, picked));
    }

    let ads = build_ads_config(existing, &org, opts, &choices);
    config::save_ads(ads.clone()).map_err(|e| anyhow!(e.to_string()))?;
    println!("\nSaved to {}", config::config_path().display());
    print_mapping(&choices);

    println!(
        "\nConnected Partners organizations: {}.\n`ranksy ads sync` syncs all of them. To add another, run `ranksy ads connect --org <id>`.",
        describe_orgs(&ads.orgs)
    );
    if !ads.apps.values().any(|a| a.in_org(&org)) {
        println!("\nNo apps are mapped in organization {org}, so there is nothing to sync for it. Re-run `ranksy ads connect --org {org}` to map them.");
        return Ok(None);
    }
    println!("\nNext: `ranksy ads sync` (cron: `0 6 * * * ranksy ads sync`).");

    Ok(Some((ads, org)))
}

fn choose_ranksy_app(
    app: &PartnersApp,
    ranksy_apps: &[RanksyApp],
    existing: Option<&AdsApp>,
) -> Result<Option<RanksyApp>> {
    let preselected = preselect(&app.name, existing, ranksy_apps);
    if preselected.is_none() {
        println!(
            "\nNo Ranksy app is named like \"{}\". Pick one, or skip; the API key's team must own the Ranksy app.",
            app.name
        );
    }

    let mut items = vec![SKIP_LABEL.to_string()];
    items.extend(ranksy_apps.iter().map(|a| {
        a.slug
            .as_ref()
            .map_or_else(|| a.name.clone(), |s| format!("{} ({s})", a.name))
    }));
    let picked = FuzzySelect::new()
        .with_prompt(format!(
            "Import ads of Shopify app \"{}\" ({} campaigns) into",
            app.name, app.campaigns
        ))
        .items(&items)
        .default(preselected.map_or(0, |i| i + 1))
        .interact()
        .context(
            "could not read your choice; `ranksy ads connect` needs an interactive terminal",
        )?;

    Ok(picked.checked_sub(1).map(|i| ranksy_apps[i].clone()))
}

/// Index into `ranksy_apps` to pre-select: the app already mapped, else a name match.
pub fn preselect(
    partners_name: &str,
    existing: Option<&AdsApp>,
    ranksy_apps: &[RanksyApp],
) -> Option<usize> {
    if let Some(mapped) = existing {
        if let Some(i) = ranksy_apps.iter().position(|a| a.ulid == mapped.ulid) {
            return Some(i);
        }
    }
    let wanted = partners_name.trim().to_lowercase();
    if let Some(i) = ranksy_apps
        .iter()
        .position(|a| a.name.trim().to_lowercase() == wanted)
    {
        return Some(i);
    }

    // One unambiguous "contains" match ("Block Country" vs "Block Country: Geo Blocker").
    let mut loose = ranksy_apps.iter().enumerate().filter(|(_, a)| {
        let name = a.name.trim().to_lowercase();
        !name.is_empty() && (wanted.contains(&name) || name.contains(&wanted))
    });
    match (loose.next(), loose.next()) {
        (Some((i, _)), None) => Some(i),
        _ => None,
    }
}

/// The `[ads]` table to save: adds `org` to the connected ones (keeping the others and their
/// mappings), keeps earlier mappings for Shopify apps not seen this time, replaces the ones
/// that were, and drops apps that were skipped.
pub fn build_ads_config(
    existing: Option<&AdsConfig>,
    org: &str,
    opts: &ChromeOptions,
    choices: &[(PartnersApp, Option<RanksyApp>)],
) -> AdsConfig {
    let mut ads = existing.cloned().unwrap_or_default();

    // Mappings from before multi-org support belong to the one org connected then.
    if let [only] = ads.orgs.as_slice() {
        let only = only.clone();
        for app in ads.apps.values_mut() {
            app.org.get_or_insert_with(|| only.clone());
        }
    }
    ads.add_org(org);

    for (partners_app, ranksy_app) in choices {
        match ranksy_app {
            Some(r) => {
                ads.apps.insert(
                    partners_app.api_key.clone(),
                    AdsApp {
                        ulid: r.ulid.clone(),
                        name: Some(partners_app.name.clone()),
                        org: Some(org.to_string()),
                    },
                );
            }
            None => {
                ads.apps.remove(&partners_app.api_key);
            }
        }
    }

    ads.chrome_profile =
        (opts.profile != default_profile()).then(|| opts.profile.display().to_string());
    ads.port = (opts.port != DEFAULT_PORT).then_some(opts.port);

    ads
}

fn print_mapping(choices: &[(PartnersApp, Option<RanksyApp>)]) {
    println!("\nShopify app -> Ranksy app");
    for (partners_app, ranksy_app) in choices {
        match ranksy_app {
            Some(r) => println!("  {} -> {} ({})", partners_app.name, r.name, r.ulid),
            None => println!("  {} -> skipped", partners_app.name),
        }
    }
}

/// Every Ranksy app visible to the key (proves the key works and has `apps:read`).
async fn load_ranksy_apps(client: &RanksyClient) -> Result<Vec<RanksyApp>> {
    let mut apps = Vec::new();
    for page in 1..20u32 {
        let response = client
            .list_apps_page(page, 100)
            .await
            .map_err(|e| ranksy_error(e, "could not list your Ranksy apps"))?;
        apps.extend(parse_apps(&response));
        if !has_more(&response, page) {
            break;
        }
    }

    Ok(apps)
}

pub fn parse_apps(response: &Value) -> Vec<RanksyApp> {
    response
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some(RanksyApp {
                ulid: a
                    .get("ulid")?
                    .as_str()
                    .filter(|s| !s.is_empty())?
                    .to_string(),
                name: a
                    .get("name")?
                    .as_str()
                    .filter(|s| !s.is_empty())?
                    .to_string(),
                slug: a.get("slug").and_then(Value::as_str).map(str::to_string),
            })
        })
        .collect()
}

fn has_more(response: &Value, page: u32) -> bool {
    response
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || response
            .pointer("/meta/last_page")
            .and_then(Value::as_u64)
            .is_some_and(|last| last > u64::from(page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn mapped(ulid: &str, org: Option<&str>) -> AdsApp {
        AdsApp {
            ulid: ulid.into(),
            name: None,
            org: org.map(str::to_string),
        }
    }

    fn default_opts() -> ChromeOptions {
        ChromeOptions {
            bin: None,
            headless: false,
            port: DEFAULT_PORT,
            profile: default_profile(),
        }
    }

    fn apps() -> Vec<RanksyApp> {
        vec![
            RanksyApp {
                ulid: "01A".into(),
                name: "Block Country".into(),
                slug: Some("block-country".into()),
            },
            RanksyApp {
                ulid: "01B".into(),
                name: "Other App".into(),
                slug: None,
            },
        ]
    }

    fn partners(key: &str, name: &str) -> PartnersApp {
        PartnersApp {
            api_key: key.into(),
            name: name.into(),
            campaigns: 3,
        }
    }

    #[test]
    fn preselects_case_insensitive_name_match() {
        assert_eq!(preselect("  block COUNTRY ", None, &apps()), Some(0));
        assert_eq!(preselect("Nothing Alike", None, &apps()), None);
    }

    #[test]
    fn preselect_prefers_existing_mapping() {
        let existing = mapped("01B", None);
        assert_eq!(
            preselect("Block Country", Some(&existing), &apps()),
            Some(1)
        );
        // A mapping to an app that's no longer visible falls back to the name match.
        let gone = mapped("01Z", None);
        assert_eq!(preselect("Block Country", Some(&gone), &apps()), Some(0));
    }

    #[test]
    fn preselects_a_single_loose_match_only() {
        assert_eq!(
            preselect("Block Country: Geo Blocker", None, &apps()),
            Some(0)
        );
        let two = vec![
            RanksyApp {
                ulid: "1".into(),
                name: "Geo".into(),
                slug: None,
            },
            RanksyApp {
                ulid: "2".into(),
                name: "Geo Blocker".into(),
                slug: None,
            },
        ];
        assert_eq!(preselect("Geo Blocker Pro", None, &two), None);
    }

    #[test]
    fn builds_config_with_non_default_chrome_settings_only() {
        let choices = vec![
            (partners("k1", "App One"), Some(apps()[0].clone())),
            (partners("k2", "App Two"), None),
        ];
        let ads = build_ads_config(None, "777", &default_opts(), &choices);
        assert_eq!(ads.orgs, ["777"]);
        assert_eq!(ads.chrome_profile, None);
        assert_eq!(ads.port, None);
        assert_eq!(ads.apps.len(), 1);
        assert_eq!(
            ads.apps["k1"],
            AdsApp {
                ulid: "01A".into(),
                name: Some("App One".into()),
                org: Some("777".into())
            }
        );

        let custom = ChromeOptions {
            bin: None,
            headless: false,
            port: 9444,
            profile: "/custom/profile".into(),
        };
        let ads = build_ads_config(None, "777", &custom, &[]);
        assert_eq!(
            (ads.port, ads.chrome_profile.as_deref()),
            (Some(9444), Some("/custom/profile"))
        );
    }

    #[test]
    fn reconnecting_keeps_unseen_mappings_and_drops_skipped_ones() {
        let existing = AdsConfig {
            orgs: vec!["1".into()],
            apps: BTreeMap::from([
                ("old".to_string(), mapped("01OLD", Some("1"))),
                ("k2".to_string(), mapped("01B", Some("1"))),
            ]),
            ..Default::default()
        };
        let choices = vec![
            (partners("k1", "App One"), Some(apps()[0].clone())),
            (partners("k2", "App Two"), None),
        ];
        let ads = build_ads_config(Some(&existing), "1", &default_opts(), &choices);
        assert_eq!(ads.orgs, ["1"]);
        assert_eq!(ads.apps.keys().collect::<Vec<_>>(), ["k1", "old"]);
    }

    #[test]
    fn connecting_a_second_org_keeps_the_first_orgs_mappings() {
        let existing = AdsConfig {
            orgs: vec!["1".into()],
            apps: BTreeMap::from([
                ("a".to_string(), mapped("01A", Some("1"))),
                // Written before multi-org support: it belongs to the only org then.
                ("legacy".to_string(), mapped("01L", None)),
            ]),
            ..Default::default()
        };
        let choices = vec![(partners("b", "App B"), Some(apps()[1].clone()))];
        let ads = build_ads_config(Some(&existing), "2", &default_opts(), &choices);

        assert_eq!(ads.orgs, ["1", "2"]);
        assert_eq!(ads.apps.keys().collect::<Vec<_>>(), ["a", "b", "legacy"]);
        assert_eq!(ads.apps["a"].org.as_deref(), Some("1"));
        assert_eq!(ads.apps["legacy"].org.as_deref(), Some("1"));
        assert_eq!(ads.apps["b"].org.as_deref(), Some("2"));
        assert_eq!(ads.apps["b"].ulid, "01B");
    }

    #[test]
    fn reconnecting_an_org_does_not_duplicate_it() {
        let existing = AdsConfig {
            orgs: vec!["1".into(), "2".into()],
            apps: BTreeMap::from([("a".to_string(), mapped("01A", Some("1")))]),
            ..Default::default()
        };
        let ads = build_ads_config(Some(&existing), "1", &default_opts(), &[]);
        assert_eq!(ads.orgs, ["1", "2"]);
        assert_eq!(ads.apps.len(), 1);
    }

    #[test]
    fn skipping_an_app_removes_only_its_mapping() {
        let existing = AdsConfig {
            orgs: vec!["1".into(), "2".into()],
            apps: BTreeMap::from([
                ("a".to_string(), mapped("01A", Some("1"))),
                ("b".to_string(), mapped("01B", Some("2"))),
            ]),
            ..Default::default()
        };
        let choices = vec![(partners("b", "App B"), None)];
        let ads = build_ads_config(Some(&existing), "2", &default_opts(), &choices);
        assert_eq!(ads.apps.keys().collect::<Vec<_>>(), ["a"]);
        assert_eq!(ads.orgs, ["1", "2"]);
    }

    #[test]
    fn parses_apps_pages() {
        let page = json!({ "data": [{ "ulid": "01A", "name": "A", "slug": "a" }, { "name": "no ulid" }, { "ulid": "01B", "name": "" }], "has_more": true });
        assert_eq!(
            parse_apps(&page),
            vec![RanksyApp {
                ulid: "01A".into(),
                name: "A".into(),
                slug: Some("a".into())
            }]
        );
        assert!(has_more(&page, 1));
        assert!(has_more(&json!({ "meta": { "last_page": 3 } }), 2));
        assert!(!has_more(&json!({ "meta": { "last_page": 2 } }), 2));
        assert!(!has_more(&json!({ "data": [] }), 1));
    }
}
