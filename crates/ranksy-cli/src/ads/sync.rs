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
use std::collections::{BTreeMap, HashSet};
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

/// The Ranksy app a Shopify app (by Partners API key, or by name) is mapped to.
pub fn lookup_app<'a>(ads: &'a AdsConfig, api_key: &str, name: &str) -> Option<&'a AdsApp> {
    ads.apps.get(api_key).or_else(|| ads.apps.get(name))
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

pub async fn run(
    ctx: &Ctx,
    ads: &AdsConfig,
    chrome: ChromeOptions,
    opts: &SyncOptions,
) -> Result<()> {
    let org = ads
        .org
        .clone()
        .ok_or_else(|| anyhow!(super::NOT_CONNECTED))?;
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

    std::fs::create_dir_all(&opts.out)
        .with_context(|| format!("cannot create {}", opts.out.display()))?;

    let tab = partners::open_tab(&chrome, &partners::ads_url(&org)).await?;
    let org = partners::wait_for_login(
        &tab,
        partners::LoginWait {
            timeout_secs: partners::SYNC_LOGIN_TIMEOUT_SECS,
            wanted_org: Some(&org),
            interactive: false,
        },
    )
    .await?;
    log(&format!(
        "Partners org {org}; syncing {} back to {} ({} days, newest first)",
        days[0],
        days[days.len() - 1],
        days.len()
    ));

    let partners::Snapshot {
        campaigns: snapshot,
        keyword_ideas,
    } = partners::snapshot(&tab, &org).await?;
    std::fs::write(
        opts.out.join("campaigns.json"),
        json!({ "campaigns": snapshot, "keyword_ideas": keyword_ideas }).to_string(),
    )?;
    log(&format!(
        "Snapshot: {} campaigns ({} archived)",
        snapshot.len(),
        snapshot
            .iter()
            .filter(|c| c.get("archived").and_then(Value::as_bool) == Some(true))
            .count()
    ));

    let mut pusher = client.as_ref().map(Pusher::new);
    let mut skipped_logged: HashSet<String> = HashSet::new();
    let mut pull_error: Option<anyhow::Error> = None;

    // Newest day first, pushed in chunks, so recent data lands in Ranksy within minutes
    // while a backfill keeps walking back in time.
    let chunks: Vec<&[String]> = days.chunks(opts.chunk_days).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        log(&format!(
            "chunk {}/{}: {}..{}",
            i + 1,
            chunks.len(),
            chunk[0],
            chunk[chunk.len() - 1]
        ));
        let pulled = match pull_chunk(&tab, &org, chunk, opts).await {
            Ok(p) => p,
            Err(e) => {
                pull_error = Some(e);
                break;
            }
        };

        let pulled: Vec<(String, Vec<Value>)> = chunk.iter().cloned().zip(pulled).collect();
        for (api_key, app) in payload::build_payloads(&snapshot, &keyword_ideas, &pulled) {
            if opts.dry_run {
                let file = opts.out.join(format!(
                    "payload-{}-{}.json",
                    payload::slug(&app.name),
                    chunk[0]
                ));
                std::fs::write(
                    &file,
                    json!({ "campaigns": app.campaigns, "keyword_ideas": app.keyword_ideas, "days": app.days })
                        .to_string(),
                )?;
                log(&format!(
                    "  {}: {} campaigns, {} campaign-days -> {}",
                    app.name,
                    app.campaigns.len(),
                    app.days.len(),
                    file.display()
                ));
                continue;
            }
            let Some(target) = lookup_app(ads, &api_key, &app.name) else {
                if skipped_logged.insert(api_key.clone()) {
                    log(&format!(
                        "skip \"{}\": not mapped to a Ranksy app (run `ranksy ads connect`)",
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

    tab.close().await;
    log(&format!("Done. Raw exports in {}", opts.out.display()));
    let (totals, failures) = pusher.map(|p| (p.totals, p.failures)).unwrap_or_default();
    print_summary(&totals, ads, &snapshot, opts.dry_run);

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
    opts: &SyncOptions,
) -> Result<Vec<Vec<Value>>> {
    futures_util::stream::iter(chunk)
        .map(|day| pull_day_cached(tab, org, day, &opts.out, opts.resume))
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
) -> Result<Vec<Value>> {
    let file = out.join(format!("{day}.json"));
    if resume {
        if let Some(campaigns) = read_cached_day(&file) {
            log(&format!("{day}: cached"));
            return Ok(campaigns);
        }
    }

    let campaigns = partners::pull_day(tab, org, day).await?;
    std::fs::write(
        &file,
        json!({ "day": day, "org": org, "campaigns": campaigns }).to_string(),
    )?;
    let t = payload::summarize(&campaigns);
    log(&format!(
        "{day}: {} active campaigns, {} search terms, {} breakdown rows, ${:.2} spend, {:.0} installs",
        t.active, t.terms, t.breakdowns, t.spend, t.installs
    ));

    Ok(campaigns)
}

fn read_cached_day(file: &Path) -> Option<Vec<Value>> {
    let text = std::fs::read_to_string(file).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;

    value.get("campaigns").and_then(Value::as_array).cloned()
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
    totals: &BTreeMap<String, Totals>,
    ads: &AdsConfig,
    snapshot: &[Value],
    dry_run: bool,
) {
    if dry_run || super::is_quiet() {
        return;
    }
    println!("Summary:");
    if totals.is_empty() {
        println!("  nothing was sent to Ranksy");
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
            "  {name}: {} campaign-days stored, {} search terms, {} breakdown rows, {} CSV reports replaced",
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

    #[test]
    fn looks_up_app_by_api_key_then_name() {
        let ads = AdsConfig {
            apps: BTreeMap::from([
                (
                    "key1".to_string(),
                    AdsApp {
                        ulid: "01A".into(),
                        name: None,
                    },
                ),
                (
                    "Named App".to_string(),
                    AdsApp {
                        ulid: "01B".into(),
                        name: None,
                    },
                ),
            ]),
            ..Default::default()
        };
        assert_eq!(lookup_app(&ads, "key1", "Whatever").unwrap().ulid, "01A");
        assert_eq!(lookup_app(&ads, "other", "Named App").unwrap().ulid, "01B");
        assert!(lookup_app(&ads, "other", "Nope").is_none());
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
