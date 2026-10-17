//! `ranksy ads sync`: pull days newest-first from Shopify Partners (inside Chrome),
//! build per-app payloads, and push them to `POST /apps/{app}/shopify-ads/sync`.

use super::chrome::ChromeOptions;
use super::payload::{self, AppPayload};
use super::{log, partners, Ctx};
use crate::config::{AdsApp, AdsConfig};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{Duration, NaiveDate};
use futures_util::{StreamExt, TryStreamExt};
use ranksy_api::ApiError;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Guards against a typo (`--from 0001-01-01`) turning into millions of Partners requests.
const MAX_RANGE_DAYS: i64 = 36_500;

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub days: Option<u32>,
    pub chunk_days: usize,
    pub concurrency: usize,
    pub dry_run: bool,
    pub resume: bool,
    pub out: PathBuf,
    /// Config keys of the mapped apps picked with `--only-app` (sorted), or `None` for all.
    pub only_apps: Option<Vec<String>>,
}

impl SyncOptions {
    /// The defaults of a bare `ranksy ads sync` (last 3 complete days).
    pub fn first_run() -> Self {
        Self {
            from: None,
            to: None,
            days: None,
            chunk_days: 7,
            concurrency: 1,
            dry_run: false,
            resume: false,
            out: super::default_cache_dir(),
            only_apps: None,
        }
    }
}

/// Folds the exclusive `--after` / `--before` bounds into inclusive `--from` / `--to`
/// (clap already rejects `--after` with `--from` and `--before` with `--to`).
pub fn inclusive_bounds(
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    after: Option<NaiveDate>,
    before: Option<NaiveDate>,
) -> Result<(Option<NaiveDate>, Option<NaiveDate>)> {
    let from = match after {
        Some(after) => Some(
            after
                .succ_opt()
                .ok_or_else(|| anyhow!("--after is out of range"))?,
        ),
        None => from,
    };
    let to = match before {
        Some(before) => Some(
            before
                .pred_opt()
                .ok_or_else(|| anyhow!("--before is out of range"))?,
        ),
        None => to,
    };

    Ok((from, to))
}

/// Days to pull, newest first. `to` defaults to yesterday (UTC), `from` to `days`
/// (default 3) complete days ending at `to`.
pub fn day_range(
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    days: Option<u32>,
    today: NaiveDate,
) -> Result<Vec<NaiveDate>> {
    let to = match to {
        Some(to) => to,
        None => today
            .checked_sub_signed(Duration::days(1))
            .ok_or_else(|| anyhow!("date out of range"))?,
    };
    let from = match from {
        Some(from) => from,
        None => to
            .checked_sub_signed(Duration::days(i64::from(days.unwrap_or(3)) - 1))
            .ok_or_else(|| anyhow!("--days reaches before the earliest supported date"))?,
    };
    if from > to {
        bail!("Invalid --from/--to range: {from} is after {to} (use YYYY-MM-DD).");
    }
    if (to - from).num_days() >= MAX_RANGE_DAYS {
        bail!("Range {from}..{to} is longer than {MAX_RANGE_DAYS} days.");
    }

    let mut out = Vec::new();
    let mut day = to;
    while day >= from {
        out.push(day);
        day -= Duration::days(1);
    }

    Ok(out)
}

/// The Partners organizations to sync: all connected ones, or just `--org`.
pub fn select_orgs(ads: &AdsConfig, org: Option<&str>) -> Result<Vec<String>> {
    if ads.orgs.is_empty() {
        bail!(super::NOT_CONNECTED);
    }
    let Some(org) = org else {
        return Ok(ads.orgs.clone());
    };
    if !ads.orgs.iter().any(|o| o == org) {
        bail!(
            "Partners organization {org} is not connected (connected: {}). Run `ranksy ads connect --org {org}` to add it.",
            super::describe_orgs(&ads.orgs)
        );
    }

    Ok(vec![org.to_string()])
}

/// Whether `value` names a mapped app: its Shopify app id (the config key, the Partners
/// API key), its Ranksy app id or its Shopify app name, ignoring case.
fn app_matches(key: &str, app: &AdsApp, value: &str) -> bool {
    let value = value.trim().to_lowercase();

    key.to_lowercase() == value
        || app.ulid.to_lowercase() == value
        || app
            .name
            .as_deref()
            .is_some_and(|name| name.to_lowercase() == value)
}

/// One line per mapped app, for the "no such app" error.
fn describe_apps(ads: &AdsConfig) -> String {
    if ads.apps.is_empty() {
        return "  none".to_string();
    }

    ads.apps
        .iter()
        .map(|(key, app)| {
            let org = app
                .org
                .as_deref()
                .map_or(String::new(), |o| format!(", org {o}"));
            format!(
                "  {}: Shopify app {key}, Ranksy app {}{org}",
                app.name.as_deref().unwrap_or("(unnamed)"),
                app.ulid
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The mapped apps (by config key, sorted) that the `--only-app` values pick, or `None`
/// without the flag. A value that matches no mapped app is an error.
pub fn select_apps(ads: &AdsConfig, only: &[String]) -> Result<Option<Vec<String>>> {
    if only.is_empty() {
        return Ok(None);
    }
    let mut keys = BTreeSet::new();
    for value in only {
        let matched: Vec<&String> = ads
            .apps
            .iter()
            .filter(|(key, app)| app_matches(key, app, value))
            .map(|(key, _)| key)
            .collect();
        if matched.is_empty() {
            bail!(
                "--only-app {value:?} matches no mapped app. Use a Ranksy app id, Shopify app id or Shopify app name. Mapped apps:\n{}",
                describe_apps(ads)
            );
        }
        keys.extend(matched.into_iter().cloned());
    }

    Ok(Some(keys.into_iter().collect()))
}

/// Keeps the organizations that hold at least one selected app. Without a filter every org stays.
pub fn orgs_with_apps(
    ads: &AdsConfig,
    orgs: Vec<String>,
    only: Option<&[String]>,
    org_flag: Option<&str>,
) -> Result<Vec<String>> {
    let Some(only) = only else {
        return Ok(orgs);
    };
    let kept: Vec<String> = orgs
        .iter()
        .filter(|org| !keys_for_org(ads, org, only).is_empty())
        .cloned()
        .collect();
    if kept.is_empty() {
        let scope = match org_flag {
            Some(org) => format!("in organization {org}"),
            None => "in any connected organization".to_string(),
        };
        bail!(
            "--only-app matches no app {scope}. Mapped apps:\n{}",
            describe_apps(ads)
        );
    }

    Ok(kept)
}

/// The selected apps that belong to `org` (a legacy mapping without an org belongs to all).
fn keys_for_org(ads: &AdsConfig, org: &str, only: &[String]) -> Vec<String> {
    only.iter()
        .filter(|key| ads.apps.get(*key).is_some_and(|app| app.in_org(org)))
        .cloned()
        .collect()
}

/// Whether a raw Partners campaign belongs to one of the selected apps (by API key, or by
/// name when the mapping is keyed by name). The in-page JavaScript applies the same rule.
fn campaign_selected(campaign: &Value, keys: &[String]) -> bool {
    let field = |name: &str| {
        campaign
            .pointer(&format!("/app/{name}"))
            .and_then(Value::as_str)
    };

    [field("apiKey"), field("name")]
        .into_iter()
        .flatten()
        .any(|v| keys.iter().any(|k| k == v))
}

/// Raw campaigns of the selected apps only; everything when there is no filter.
fn keep_apps(campaigns: Vec<Value>, only: Option<&[String]>) -> Vec<Value> {
    match only {
        Some(keys) => campaigns
            .into_iter()
            .filter(|c| campaign_selected(c, keys))
            .collect(),
        None => campaigns,
    }
}

/// Raw day exports and payload files of one organization, so `--resume` never mixes them.
pub fn org_dir(out: &Path, org: &str) -> PathBuf {
    out.join(org)
}

/// The Ranksy app a Shopify app (by Partners API key, or by name) in `org` is mapped to.
pub fn lookup_app<'a>(
    ads: &'a AdsConfig,
    org: &str,
    api_key: &str,
    name: &str,
) -> Option<&'a AdsApp> {
    ads.apps
        .get(api_key)
        .or_else(|| ads.apps.get(name))
        .filter(|app| app.in_org(org))
}

/// Human message for a failed sync POST, and whether retrying other batches for the app is pointless.
pub fn describe_post_error(e: &ApiError) -> (String, bool) {
    match e {
        ApiError::Status { code: 401, .. } => ("Ranksy rejected the API key (401)".to_string(), true),
        ApiError::Status { code: 403, .. } => ("the API key needs the `ads:write` and `apps:read` scopes, and its team must own the app (403)".to_string(), true),
        ApiError::Status { code: 404, .. } => ("Ranksy app not found for this API key (404); run `ranksy ads connect` again".to_string(), true),
        ApiError::Status { code, body } => (format!("Ranksy returned {code}: {}", body.chars().take(500).collect::<String>()), false),
        other => (other.to_string(), false),
    }
}

#[derive(Debug, Default)]
struct Totals {
    stored: u64,
    search_terms: u64,
    breakdown_rows: u64,
    csv_replaced: u64,
}

impl Totals {
    fn add(&mut self, data: &Value) {
        let count = |path: &str| data.pointer(path).and_then(Value::as_u64).unwrap_or(0);
        self.stored += count("/days/stored");
        self.search_terms += count("/days/search_terms");
        self.breakdown_rows += count("/days/breakdown_rows");
        self.csv_replaced += data
            .get("csv_reports_replaced")
            .and_then(Value::as_array)
            .map_or(0, |r| r.len() as u64);
    }

    fn merge(&mut self, other: &Totals) {
        self.stored += other.stored;
        self.search_terms += other.search_terms;
        self.breakdown_rows += other.breakdown_rows;
        self.csv_replaced += other.csv_replaced;
    }
}

/// Syncs `orgs` one after the other in a single Chrome tab. A failing org is reported and
/// the next one still runs; the error at the end names every org that failed.
pub async fn run(
    ctx: &Ctx,
    ads: &AdsConfig,
    chrome: ChromeOptions,
    opts: &SyncOptions,
    orgs: &[String],
) -> Result<()> {
    let Some(first) = orgs.first() else {
        bail!(super::NOT_CONNECTED);
    };
    if !opts.dry_run && ads.apps.is_empty() {
        bail!("{} (no apps are mapped yet)", super::NOT_CONNECTED);
    }
    let client = if opts.dry_run {
        None
    } else {
        Some(ctx.client()?)
    };
    let days = day_range(
        opts.from,
        opts.to,
        opts.days,
        chrono::Utc::now().date_naive(),
    )?;
    let days: Vec<String> = days.iter().map(NaiveDate::to_string).collect();

    let tab = partners::open_tab(&chrome, &partners::ads_url(first)).await?;
    let mut overall = Totals::default();
    let mut failed: Vec<&str> = Vec::new();
    for org in orgs {
        let outcome = sync_org(&tab, client.as_ref(), ads, org, &days, opts).await;
        overall.merge(&outcome.totals);
        if let Some(e) = outcome.error {
            eprintln!("org {org} failed: {e:#}");
            failed.push(org);
        }
    }
    tab.close().await;

    if orgs.len() > 1 && !opts.dry_run && !super::is_quiet() {
        println!(
            "Total, all organizations: {} campaign-days stored, {} search terms, {} breakdown rows, {} CSV reports replaced",
            overall.stored, overall.search_terms, overall.breakdown_rows, overall.csv_replaced
        );
    }
    if !failed.is_empty() {
        bail!("sync failed for organization(s): {}", failed.join(", "));
    }

    Ok(())
}

struct OrgOutcome {
    totals: Totals,
    error: Option<anyhow::Error>,
}

/// One organization: log in (or switch to it), pull its days, push the apps that belong to it.
async fn sync_org(
    tab: &super::cdp::Tab,
    client: Option<&ranksy_api::RanksyClient>,
    ads: &AdsConfig,
    org: &str,
    days: &[String],
    opts: &SyncOptions,
) -> OrgOutcome {
    let mut totals = Totals::default();
    let error = sync_org_inner(tab, client, ads, org, days, opts, &mut totals)
        .await
        .err();

    OrgOutcome { totals, error }
}

async fn sync_org_inner(
    tab: &super::cdp::Tab,
    client: Option<&ranksy_api::RanksyClient>,
    ads: &AdsConfig,
    org: &str,
    days: &[String],
    opts: &SyncOptions,
    overall: &mut Totals,
) -> Result<()> {
    let say = |msg: &str| log(&format!("[{org}] {msg}"));
    let dir = org_dir(&opts.out, org);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

    let org = partners::wait_for_login(
        tab,
        partners::LoginWait {
            timeout_secs: partners::SYNC_LOGIN_TIMEOUT_SECS,
            wanted_org: Some(org),
            interactive: false,
        },
    )
    .await?;
    let only: Option<Vec<String>> = opts
        .only_apps
        .as_deref()
        .map(|keys| keys_for_org(ads, &org, keys));
    let only = only.as_deref();
    say(&format!(
        "Partners org {org}; syncing {} back to {} ({} days, newest first){}",
        days[0],
        days[days.len() - 1],
        days.len(),
        only.map_or(String::new(), |keys| format!(
            ", only {}",
            keys.iter()
                .map(|k| ads
                    .apps
                    .get(k)
                    .and_then(|a| a.name.clone())
                    .unwrap_or_else(|| k.clone()))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    ));

    let partners::Snapshot {
        campaigns: snapshot,
        keyword_ideas,
    } = partners::snapshot(tab, &org, only).await?;
    let snapshot = keep_apps(snapshot, only);
    std::fs::write(
        dir.join("campaigns.json"),
        json!({ "campaigns": snapshot, "keyword_ideas": keyword_ideas }).to_string(),
    )?;
    say(&format!(
        "Snapshot: {} campaigns ({} archived)",
        snapshot.len(),
        snapshot
            .iter()
            .filter(|c| c.get("archived").and_then(Value::as_bool) == Some(true))
            .count()
    ));

    let mut pusher = client.map(Pusher::new);
    let mut skipped_logged: HashSet<String> = HashSet::new();
    let mut pull_error: Option<anyhow::Error> = None;

    // Newest day first, pushed in chunks, so recent data lands in Ranksy within minutes
    // while a backfill keeps walking back in time.
    let chunks: Vec<&[String]> = days.chunks(opts.chunk_days).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        say(&format!(
            "chunk {}/{}: {}..{}",
            i + 1,
            chunks.len(),
            chunk[0],
            chunk[chunk.len() - 1]
        ));
        let pulled = match pull_chunk(tab, &org, chunk, &dir, opts, only).await {
            Ok(p) => p,
            Err(e) => {
                pull_error = Some(e);
                break;
            }
        };

        let pulled: Vec<(String, Vec<Value>)> = chunk.iter().cloned().zip(pulled).collect();
        for (api_key, app) in payload::build_payloads(&snapshot, &keyword_ideas, &pulled) {
            if opts.dry_run {
                let file = dir.join(format!(
                    "payload-{}-{}.json",
                    payload::slug(&app.name),
                    chunk[0]
                ));
                std::fs::write(
                    &file,
                    json!({ "campaigns": app.campaigns, "keyword_ideas": app.keyword_ideas, "days": app.days })
                        .to_string(),
                )?;
                say(&format!(
                    "  {}: {} campaigns, {} campaign-days -> {}",
                    app.name,
                    app.campaigns.len(),
                    app.days.len(),
                    file.display()
                ));
                continue;
            }
            let Some(target) = lookup_app(ads, &org, &api_key, &app.name) else {
                if skipped_logged.insert(api_key.clone()) {
                    say(&format!(
                        "skip \"{}\": not mapped to a Ranksy app (run `ranksy ads connect --org {org}`)",
                        app.name
                    ));
                }
                continue;
            };
            if let Some(pusher) = pusher.as_mut() {
                pusher.push_app(&target.ulid, &api_key, &app).await;
            }
        }
    }

    say(&format!("Done. Raw exports in {}", dir.display()));
    let (totals, failures) = pusher.map(|p| (p.totals, p.failures)).unwrap_or_default();
    print_summary(&org, &totals, ads, &snapshot, opts.dry_run);
    for t in totals.values() {
        overall.merge(t);
    }

    if let Some(e) = pull_error {
        return Err(e);
    }
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("failed: {f}");
        }
        bail!("{} request(s) to Ranksy failed", failures.len());
    }

    Ok(())
}

/// Pulls (or loads from the cache) every day of a chunk, keeping `concurrency` in flight.
async fn pull_chunk(
    tab: &super::cdp::Tab,
    org: &str,
    chunk: &[String],
    dir: &Path,
    opts: &SyncOptions,
    only: Option<&[String]>,
) -> Result<Vec<Vec<Value>>> {
    futures_util::stream::iter(chunk)
        .map(|day| pull_day_cached(tab, org, day, dir, opts.resume, only))
        .buffered(opts.concurrency)
        .try_collect()
        .await
}

async fn pull_day_cached(
    tab: &super::cdp::Tab,
    org: &str,
    day: &str,
    out: &Path,
    resume: bool,
    only: Option<&[String]>,
) -> Result<Vec<Value>> {
    if resume {
        if let Some(campaigns) = load_cached_day(out, day, only) {
            log(&format!("{day}: cached"));
            return Ok(campaigns);
        }
    }

    let campaigns = keep_apps(partners::pull_day(tab, org, day, only).await?, only);
    std::fs::write(
        day_file(out, day, only.is_some()),
        day_json(org, day, &campaigns, only).to_string(),
    )?;
    let t = payload::summarize(&campaigns);
    log(&format!(
        "{day}: {} active campaigns, {} search terms, {} breakdown rows, ${:.2} spend, {:.0} installs",
        t.active, t.terms, t.breakdowns, t.spend, t.installs
    ));

    Ok(campaigns)
}

/// A day pulled for every app lives in `<day>.json`. A day pulled with `--only-app` lives in
/// `<day>.only.json` and records its apps, so it can never be mistaken for a full day (and a
/// filtered pull never overwrites a full one).
fn day_file(dir: &Path, day: &str, filtered: bool) -> PathBuf {
    if filtered {
        return dir.join(format!("{day}.only.json"));
    }

    dir.join(format!("{day}.json"))
}

fn day_json(org: &str, day: &str, campaigns: &[Value], only: Option<&[String]>) -> Value {
    let mut value = json!({ "day": day, "org": org, "campaigns": campaigns });
    if let Some(keys) = only {
        value["apps"] = json!(keys);
    }

    value
}

/// The cached campaigns of a day for the wanted apps, or `None` when the day must be pulled.
/// A full day serves any filter (narrowed in memory). A filtered day serves only a filter
/// whose apps it all contains. Neither serves a request for all apps.
fn load_cached_day(dir: &Path, day: &str, only: Option<&[String]>) -> Option<Vec<Value>> {
    if let Some((None, campaigns)) = read_cached_day(&day_file(dir, day, false)) {
        return Some(keep_apps(campaigns, only));
    }
    let wanted = only?;
    let (Some(apps), campaigns) = read_cached_day(&day_file(dir, day, true))? else {
        return None;
    };

    wanted
        .iter()
        .all(|key| apps.contains(key))
        .then(|| keep_apps(campaigns, only))
}

/// `(apps the file was pulled for, campaigns)`; the apps are `None` in a full day.
fn read_cached_day(file: &Path) -> Option<(Option<Vec<String>>, Vec<Value>)> {
    let text = std::fs::read_to_string(file).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let campaigns = value.get("campaigns").and_then(Value::as_array)?.clone();
    let apps = value.get("apps").and_then(Value::as_array).map(|apps| {
        apps.iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    });

    Some((apps, campaigns))
}

/// Pushes payloads to Ranksy and remembers what happened. Failures are recorded, never
/// raised, so one broken app doesn't stop the others.
struct Pusher<'a> {
    client: &'a ranksy_api::RanksyClient,
    /// Apps whose campaign snapshot already went out (it is sent with an app's first request only).
    snapshot_sent: HashSet<String>,
    /// Apps that can't succeed this run (bad key, missing scope, unknown app).
    aborted: HashSet<String>,
    totals: BTreeMap<String, Totals>,
    failures: Vec<String>,
}

impl<'a> Pusher<'a> {
    fn new(client: &'a ranksy_api::RanksyClient) -> Self {
        Self {
            client,
            snapshot_sent: HashSet::new(),
            aborted: HashSet::new(),
            totals: BTreeMap::new(),
            failures: Vec::new(),
        }
    }

    async fn push_app(&mut self, ulid: &str, api_key: &str, app: &AppPayload) {
        if self.aborted.contains(api_key) {
            return;
        }
        for batch in payload::batches(&app.days) {
            let send_snapshot = !self.snapshot_sent.contains(api_key);
            if !send_snapshot && batch.is_empty() {
                continue;
            }
            let campaigns: &[Value] = if send_snapshot { &app.campaigns } else { &[] };
            let mut body = json!({ "campaigns": campaigns, "days": batch });
            if send_snapshot {
                body["keyword_ideas"] = json!(app.keyword_ideas);
            }
            let range = format!(
                "{}..{}",
                batch.last().and_then(|d| d["date"].as_str()).unwrap_or("-"),
                batch
                    .first()
                    .and_then(|d| d["date"].as_str())
                    .unwrap_or("-")
            );

            match self
                .client
                .post_json(&format!("/apps/{ulid}/shopify-ads/sync"), &body)
                .await
            {
                Ok(res) => {
                    self.snapshot_sent.insert(api_key.to_string());
                    let data = res.get("data").cloned().unwrap_or(Value::Null);
                    let mut batch_totals = Totals::default();
                    batch_totals.add(&data);
                    log(&format!(
                        "  -> {} {range}: {} campaign-days, {} search terms, {} breakdown rows",
                        app.name,
                        batch_totals.stored,
                        batch_totals.search_terms,
                        batch_totals.breakdown_rows
                    ));
                    for r in data
                        .get("csv_reports_replaced")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        log(&format!(
                            "     replaced CSV report #{} \"{}\" {}..{}",
                            r["id"],
                            r["ad_name"].as_str().unwrap_or(""),
                            r["start_date"].as_str().unwrap_or(""),
                            r["end_date"].as_str().unwrap_or("")
                        ));
                    }
                    self.totals
                        .entry(api_key.to_string())
                        .or_default()
                        .merge(&batch_totals);
                }
                Err(e) => {
                    let (message, fatal) = describe_post_error(&e);
                    self.failures
                        .push(format!("{} {range}: {message}", app.name));
                    if fatal {
                        self.aborted.insert(api_key.to_string());
                        return;
                    }
                }
            }
        }
    }
}

fn print_summary(
    org: &str,
    totals: &BTreeMap<String, Totals>,
    ads: &AdsConfig,
    snapshot: &[Value],
    dry_run: bool,
) {
    if dry_run || super::is_quiet() {
        return;
    }
    println!("[{org}] Summary:");
    if totals.is_empty() {
        println!("[{org}]   nothing was sent to Ranksy");
    }
    for (api_key, t) in totals {
        let name = ads
            .apps
            .get(api_key)
            .and_then(|a| a.name.clone())
            .or_else(|| {
                snapshot
                    .iter()
                    .find(|c| c.pointer("/app/apiKey").and_then(Value::as_str) == Some(api_key))
                    .and_then(|c| c.pointer("/app/name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| api_key.clone());
        println!(
            "[{org}]   {name}: {} campaign-days stored, {} search terms, {} breakdown rows, {} CSV reports replaced",
            t.stored, t.search_terms, t.breakdown_rows, t.csv_replaced
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn default_range_is_last_three_complete_days_newest_first() {
        let days = day_range(None, None, None, d("2026-10-16")).unwrap();
        assert_eq!(days, [d("2026-10-15"), d("2026-10-14"), d("2026-10-13")]);
    }

    #[test]
    fn days_flag_counts_back_from_to() {
        let days = day_range(None, Some(d("2026-10-10")), Some(2), d("2026-10-16")).unwrap();
        assert_eq!(days, [d("2026-10-10"), d("2026-10-09")]);
    }

    #[test]
    fn explicit_range_is_inclusive_and_descending() {
        let days = day_range(
            Some(d("2026-02-27")),
            Some(d("2026-03-02")),
            Some(99),
            d("2026-10-16"),
        )
        .unwrap();
        assert_eq!(
            days,
            [
                d("2026-03-02"),
                d("2026-03-01"),
                d("2026-02-28"),
                d("2026-02-27")
            ]
        );
    }

    #[test]
    fn single_day_and_default_to_with_from() {
        assert_eq!(
            day_range(Some(d("2026-10-15")), None, None, d("2026-10-16")).unwrap(),
            [d("2026-10-15")]
        );
    }

    #[test]
    fn inverted_or_huge_ranges_fail() {
        assert!(day_range(
            Some(d("2026-10-16")),
            Some(d("2026-10-15")),
            None,
            d("2026-10-16")
        )
        .is_err());
        assert!(day_range(Some(d("1900-01-01")), None, None, d("2026-10-16")).is_err());
    }

    #[test]
    fn after_and_before_are_exclusive() {
        let (from, to) =
            inclusive_bounds(None, None, Some(d("2026-10-01")), Some(d("2026-10-05"))).unwrap();
        let days = day_range(from, to, None, d("2026-10-16")).unwrap();
        assert_eq!(days, [d("2026-10-04"), d("2026-10-03"), d("2026-10-02")]);
    }

    #[test]
    fn after_alone_runs_up_to_yesterday() {
        let (from, to) = inclusive_bounds(None, None, Some(d("2026-10-12")), None).unwrap();
        let days = day_range(from, to, None, d("2026-10-16")).unwrap();
        assert_eq!(days, [d("2026-10-15"), d("2026-10-14"), d("2026-10-13")]);
    }

    #[test]
    fn before_combines_with_from_and_days() {
        let (from, to) =
            inclusive_bounds(Some(d("2026-09-29")), None, None, Some(d("2026-10-01"))).unwrap();
        assert_eq!((from, to), (Some(d("2026-09-29")), Some(d("2026-09-30"))));

        let (from, to) = inclusive_bounds(None, None, None, Some(d("2026-10-01"))).unwrap();
        let days = day_range(from, to, Some(2), d("2026-10-16")).unwrap();
        assert_eq!(days, [d("2026-09-30"), d("2026-09-29")]);
    }

    #[test]
    fn chunks_keep_newest_first_order() {
        let days: Vec<String> = day_range(None, None, Some(10), d("2026-10-16"))
            .unwrap()
            .iter()
            .map(NaiveDate::to_string)
            .collect();
        let chunks: Vec<&[String]> = days.chunks(7).collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0][0], "2026-10-15");
        assert_eq!(chunks[0].len(), 7);
        assert_eq!(chunks[1], ["2026-10-08", "2026-10-07", "2026-10-06"]);
    }

    fn mapped(ulid: &str, org: Option<&str>) -> AdsApp {
        AdsApp {
            ulid: ulid.into(),
            name: None,
            org: org.map(str::to_string),
        }
    }

    #[test]
    fn looks_up_app_by_api_key_then_name() {
        let ads = AdsConfig {
            orgs: vec!["1".into()],
            apps: BTreeMap::from([
                ("key1".to_string(), mapped("01A", None)),
                ("Named App".to_string(), mapped("01B", None)),
            ]),
            ..Default::default()
        };
        assert_eq!(
            lookup_app(&ads, "1", "key1", "Whatever").unwrap().ulid,
            "01A"
        );
        assert_eq!(
            lookup_app(&ads, "1", "other", "Named App").unwrap().ulid,
            "01B"
        );
        assert!(lookup_app(&ads, "1", "other", "Nope").is_none());
    }

    #[test]
    fn only_apps_of_the_synced_org_are_pushed() {
        let ads = AdsConfig {
            orgs: vec!["1".into(), "2".into()],
            apps: BTreeMap::from([
                ("a".to_string(), mapped("01A", Some("1"))),
                ("b".to_string(), mapped("01B", Some("2"))),
                ("legacy".to_string(), mapped("01L", None)),
            ]),
            ..Default::default()
        };
        assert_eq!(lookup_app(&ads, "1", "a", "A").unwrap().ulid, "01A");
        assert!(lookup_app(&ads, "1", "b", "B").is_none());
        assert_eq!(lookup_app(&ads, "2", "b", "B").unwrap().ulid, "01B");
        assert!(lookup_app(&ads, "2", "a", "A").is_none());
        // A mapping without an org (older config) is looked up by API key in any org.
        assert_eq!(lookup_app(&ads, "1", "legacy", "L").unwrap().ulid, "01L");
        assert_eq!(lookup_app(&ads, "2", "legacy", "L").unwrap().ulid, "01L");
    }

    #[test]
    fn selects_every_org_without_a_flag_and_one_with_it() {
        let ads = AdsConfig {
            orgs: vec!["1".into(), "2".into()],
            ..Default::default()
        };
        assert_eq!(select_orgs(&ads, None).unwrap(), ["1", "2"]);
        assert_eq!(select_orgs(&ads, Some("2")).unwrap(), ["2"]);
    }

    #[test]
    fn unknown_org_flag_lists_the_connected_orgs() {
        let ads = AdsConfig {
            orgs: vec!["1".into(), "2".into()],
            ..Default::default()
        };
        let err = select_orgs(&ads, Some("9")).unwrap_err().to_string();
        assert!(err.contains("9") && err.contains("1, 2"));
    }

    #[test]
    fn selecting_orgs_needs_a_connection() {
        let err = select_orgs(&AdsConfig::default(), None).unwrap_err();
        assert!(err.to_string().contains("ranksy ads connect"));
        assert!(select_orgs(&AdsConfig::default(), Some("1")).is_err());
    }

    #[test]
    fn each_org_caches_in_its_own_folder() {
        let out = Path::new("/cache/ads-export");
        assert_eq!(org_dir(out, "1"), Path::new("/cache/ads-export/1"));
        assert_ne!(org_dir(out, "1"), org_dir(out, "2"));
    }

    fn two_org_ads() -> AdsConfig {
        let named = |ulid: &str, name: &str, org: &str| AdsApp {
            ulid: ulid.into(),
            name: Some(name.into()),
            org: Some(org.into()),
        };
        AdsConfig {
            orgs: vec!["1".into(), "2".into()],
            apps: BTreeMap::from([
                ("key-a".to_string(), named("01AAA", "Block Country", "1")),
                ("key-b".to_string(), named("01BBB", "Email Pro", "1")),
                ("key-c".to_string(), named("01CCC", "Chat Tool", "2")),
            ]),
            ..Default::default()
        }
    }

    fn values(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_only_app_flag_means_no_filter() {
        assert_eq!(select_apps(&two_org_ads(), &[]).unwrap(), None);
    }

    #[test]
    fn only_app_matches_ranksy_id_shopify_id_or_name_ignoring_case() {
        let ads = two_org_ads();
        for value in ["01aaa", "KEY-A", "block country", " Block Country "] {
            assert_eq!(
                select_apps(&ads, &values(&[value])).unwrap(),
                Some(values(&["key-a"])),
                "{value}"
            );
        }
    }

    #[test]
    fn only_app_is_repeatable_and_deduplicated() {
        let picked =
            select_apps(&two_org_ads(), &values(&["chat tool", "01AAA", "key-a"])).unwrap();
        assert_eq!(picked, Some(values(&["key-a", "key-c"])));
    }

    #[test]
    fn unknown_only_app_lists_the_mapped_apps() {
        let err = select_apps(&two_org_ads(), &values(&["01AAA", "nope"]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("\"nope\""));
        assert!(err.contains("Block Country: Shopify app key-a, Ranksy app 01AAA, org 1"));
        assert!(err.contains("Chat Tool: Shopify app key-c, Ranksy app 01CCC, org 2"));
    }

    #[test]
    fn only_app_with_nothing_mapped_says_so() {
        let err = select_apps(&AdsConfig::default(), &values(&["x"])).unwrap_err();
        assert!(err.to_string().contains("none"));
    }

    #[test]
    fn only_orgs_holding_a_selected_app_are_synced() {
        let ads = two_org_ads();
        let all = || vec!["1".to_string(), "2".to_string()];

        let only = Some(values(&["key-c"]));
        assert_eq!(
            orgs_with_apps(&ads, all(), only.as_deref(), None).unwrap(),
            ["2"]
        );
        let only = Some(values(&["key-a", "key-c"]));
        assert_eq!(
            orgs_with_apps(&ads, all(), only.as_deref(), None).unwrap(),
            ["1", "2"]
        );
        assert_eq!(orgs_with_apps(&ads, all(), None, None).unwrap(), ["1", "2"]);
    }

    #[test]
    fn only_app_and_org_intersect() {
        let ads = two_org_ads();
        let only = Some(values(&["key-c"]));
        let orgs = select_orgs(&ads, Some("1")).unwrap();
        let err = orgs_with_apps(&ads, orgs, only.as_deref(), Some("1")).unwrap_err();
        assert!(err.to_string().contains("in organization 1"));

        let orgs = select_orgs(&ads, Some("2")).unwrap();
        assert_eq!(
            orgs_with_apps(&ads, orgs, only.as_deref(), Some("2")).unwrap(),
            ["2"]
        );
    }

    #[test]
    fn selected_apps_are_limited_to_the_org_being_synced() {
        let ads = two_org_ads();
        let only = values(&["key-a", "key-c"]);
        assert_eq!(keys_for_org(&ads, "1", &only), ["key-a"]);
        assert_eq!(keys_for_org(&ads, "2", &only), ["key-c"]);

        // A legacy mapping without an org can be synced from any org.
        let mut legacy = two_org_ads();
        legacy.apps.insert("old".into(), mapped("01OLD", None));
        assert_eq!(keys_for_org(&legacy, "2", &values(&["old"])), ["old"]);
    }

    fn raw_campaign(id: &str, key: &str, name: &str) -> Value {
        json!({ "id": id, "app": { "apiKey": key, "name": name }, "metrics": { "impressions": 5 } })
    }

    #[test]
    fn keep_apps_matches_by_api_key_or_name() {
        let campaigns = vec![
            raw_campaign("1", "key-a", "Block Country"),
            raw_campaign("2", "key-b", "Email Pro"),
            raw_campaign("3", "key-x", "Named App"),
        ];
        let ids = |kept: &[Value]| -> Vec<String> {
            kept.iter()
                .map(|c| c["id"].as_str().unwrap().to_string())
                .collect()
        };

        assert_eq!(ids(&keep_apps(campaigns.clone(), None)), ["1", "2", "3"]);
        // A mapping keyed by app name (older configs) still selects its campaigns.
        let kept = keep_apps(campaigns, Some(&values(&["key-a", "Named App"])));
        assert_eq!(ids(&kept), ["1", "3"]);
    }

    #[test]
    fn payloads_built_from_a_filtered_pull_hold_only_the_selected_app() {
        let only = values(&["key-a"]);
        let snapshot = keep_apps(
            vec![
                raw_campaign("1", "key-a", "Block Country"),
                raw_campaign("2", "key-b", "Email Pro"),
            ],
            Some(&only),
        );
        let day = keep_apps(
            vec![
                raw_campaign("1", "key-a", "Block Country"),
                raw_campaign("2", "key-b", "Email Pro"),
            ],
            Some(&only),
        );
        let payloads = payload::build_payloads(
            &snapshot,
            &BTreeMap::new(),
            &[("2026-10-15".to_string(), day)],
        );
        assert_eq!(payloads.keys().collect::<Vec<_>>(), ["key-a"]);
        assert_eq!(payloads["key-a"].days.len(), 1);
    }

    fn cache_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ranksy-sync-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_day(dir: &Path, day: &str, campaigns: &[Value], only: Option<&[String]>) {
        std::fs::write(
            day_file(dir, day, only.is_some()),
            day_json("1", day, campaigns, only).to_string(),
        )
        .unwrap();
    }

    fn cached_ids(cached: Option<Vec<Value>>) -> Option<Vec<String>> {
        cached.map(|c| {
            c.iter()
                .map(|c| c["id"].as_str().unwrap().to_string())
                .collect()
        })
    }

    #[test]
    fn a_full_cached_day_serves_every_request_narrowed_to_the_filter() {
        let dir = cache_dir("full");
        let both = [
            raw_campaign("1", "key-a", "A"),
            raw_campaign("2", "key-b", "B"),
        ];
        write_day(&dir, "2026-10-15", &both, None);

        let all = cached_ids(load_cached_day(&dir, "2026-10-15", None));
        assert_eq!(all.unwrap(), ["1", "2"]);
        let only = values(&["key-b"]);
        let one = cached_ids(load_cached_day(&dir, "2026-10-15", Some(&only)));
        assert_eq!(one.unwrap(), ["2"]);
        assert!(load_cached_day(&dir, "2026-10-14", None).is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_filtered_cached_day_never_serves_a_full_request() {
        let dir = cache_dir("filtered");
        let only = values(&["key-a"]);
        write_day(
            &dir,
            "2026-10-15",
            &[raw_campaign("1", "key-a", "A")],
            Some(&only),
        );

        // Same filter: hit. Full request: miss, and the full file is not touched.
        let hit = cached_ids(load_cached_day(&dir, "2026-10-15", Some(&only)));
        assert_eq!(hit.unwrap(), ["1"]);
        assert!(load_cached_day(&dir, "2026-10-15", None).is_none());
        assert!(!day_file(&dir, "2026-10-15", false).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_filtered_cached_day_serves_only_filters_it_covers() {
        let dir = cache_dir("covers");
        let both = values(&["key-a", "key-b"]);
        let campaigns = [
            raw_campaign("1", "key-a", "A"),
            raw_campaign("2", "key-b", "B"),
        ];
        write_day(&dir, "2026-10-15", &campaigns, Some(&both));

        let subset = values(&["key-b"]);
        let hit = cached_ids(load_cached_day(&dir, "2026-10-15", Some(&subset)));
        assert_eq!(hit.unwrap(), ["2"]);
        let wider = values(&["key-a", "key-c"]);
        assert!(load_cached_day(&dir, "2026-10-15", Some(&wider)).is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cached_day_from_before_the_filter_existed_counts_as_full() {
        let dir = cache_dir("legacy");
        std::fs::write(
            dir.join("2026-10-15.json"),
            json!({ "day": "2026-10-15", "org": "1", "campaigns": [raw_campaign("1", "key-a", "A")] }).to_string(),
        )
        .unwrap();

        assert!(load_cached_day(&dir, "2026-10-15", None).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn forbidden_names_the_scopes_and_stops_the_app() {
        let (msg, fatal) = describe_post_error(&ApiError::Status {
            code: 403,
            body: String::new(),
        });
        assert!(msg.contains("ads:write") && msg.contains("apps:read"));
        assert!(fatal);
        let (msg, fatal) = describe_post_error(&ApiError::Status {
            code: 422,
            body: "bad".into(),
        });
        assert!(msg.contains("422") && msg.contains("bad"));
        assert!(!fatal);
    }

    #[test]
    fn totals_read_the_response_data() {
        let mut t = Totals::default();
        t.add(&json!({ "days": { "stored": 5, "search_terms": 40, "breakdown_rows": 12 }, "csv_reports_replaced": [{ "id": 1 }, { "id": 2 }] }));
        t.add(&json!({ "days": { "stored": 1 } }));
        assert_eq!(
            (t.stored, t.search_terms, t.breakdown_rows, t.csv_replaced),
            (6, 40, 12, 2)
        );
    }
}
