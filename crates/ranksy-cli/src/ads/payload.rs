//! Pure mapping from raw Partners GraphQL results to the Ranksy sync payload
//! (`POST /apps/{app}/shopify-ads/sync`), plus the request batching limits.
//! Ported from `buildPayloads` and friends in the Node export script.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Per request; the API caps at 1000 days and 50k search-term / breakdown rows.
pub const MAX_DAYS_PER_REQUEST: usize = 500;
pub const MAX_ROWS_PER_REQUEST: usize = 40_000;

/// Everything to send for one Shopify app.
#[derive(Debug, Default)]
pub struct AppPayload {
    pub name: String,
    pub campaigns: Vec<Value>,
    pub days: Vec<Value>,
}

/// A finite JSON number as-is, else 0.
fn num(v: Option<&Value>) -> Value {
    match v {
        Some(n) if n.is_number() => n.clone(),
        _ => json!(0),
    }
}

/// A finite JSON number as-is, else null.
fn opt(v: Option<&Value>) -> Value {
    match v {
        Some(n) if n.is_number() => n.clone(),
        _ => Value::Null,
    }
}

fn truncated(v: Option<&Value>, max_chars: usize) -> String {
    v.and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(max_chars)
        .collect()
}

fn metrics(m: Option<&Value>) -> Map<String, Value> {
    let get = |key: &str| m.and_then(|m| m.get(key));
    let mut out = Map::new();
    out.insert("impressions".into(), num(get("impressions")));
    out.insert("clicks".into(), num(get("clicks")));
    out.insert("installs".into(), num(get("installs")));
    out.insert("spend".into(), num(get("spend")));
    out.insert("click_through_rate".into(), num(get("clickthroughRate")));
    out.insert("install_rate".into(), num(get("installRate")));
    out.insert("cost_per_click".into(), num(get("costPerClick")));
    out.insert("cost_per_install".into(), num(get("costPerInstall")));
    out.insert("cost_per_mille".into(), num(get("costPerMille")));
    out.insert("average_position".into(), opt(get("averagePosition")));
    out.insert("visibility".into(), num(get("page1WinRate")));
    out.insert("customers".into(), opt(get("customers")));
    out.insert("revenue".into(), opt(get("revenue")));
    out.insert("return_on_spend".into(), opt(get("returnOnSpend")));
    out.insert("cost_per_customer".into(), opt(get("costPerCustomer")));
    out.insert("customer_rate".into(), opt(get("customerRate")));
    out.insert("conversions".into(), num(get("conversions")));
    out.insert("conversion_rate".into(), num(get("conversionRate")));
    out.insert("cost_per_conversion".into(), num(get("costPerConversion")));

    out
}

fn breakdown_rows(rows: &Value, target: &str) -> Vec<Value> {
    let Some(rows) = rows.as_array() else {
        return Vec::new();
    };

    rows.iter()
        .map(|r| {
            let targeting = r.get("targeting");
            let first = |key: &str| targeting.and_then(|t| t.get(key)).and_then(|v| v.get(0));
            let value = match target {
                "country" => first("geographicTargets").and_then(|g| g.get("code")),
                "shop_plan" => first("planTargets"),
                _ => first("deviceTypeTargets"),
            };
            let value = match value {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => "unknown".to_string(),
            };
            let m = metrics(r.get("metrics"));
            let pick = |keys: &[&str]| -> Map<String, Value> { keys.iter().map(|k| ((*k).to_string(), m[*k].clone())).collect() };

            json!({
                "value": value.chars().take(64).collect::<String>(),
                "metrics": pick(&["impressions", "clicks", "installs", "spend", "average_position", "visibility", "customers", "revenue", "conversions"]),
            })
        })
        .collect()
}

fn range(r: Option<&Value>) -> Value {
    match r {
        Some(r) if r.is_object() => {
            json!({ "lower": num(r.get("lower")), "upper": num(r.get("upper")) })
        }
        _ => Value::Null,
    }
}

fn string_list(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// One campaign's metadata snapshot (with keyword / negative keyword lists for search campaigns).
pub fn campaign_snapshot(c: &Value) -> Value {
    let targeting = c.get("targeting");
    let keywords = c.get("keywords").and_then(Value::as_array).map(|list| {
        list.iter()
            .map(|k| {
                json!({
                    "shopify_keyword_id": k.get("id").cloned().unwrap_or(Value::Null),
                    "keyword": truncated(k.get("keyword"), 500),
                    "match_type": k.get("matchType").cloned().unwrap_or(Value::Null),
                    "bid": num(k.get("bid")),
                    "status": k.get("status").cloned().unwrap_or(Value::Null),
                    "relevance_category": k.get("relevanceCategory").cloned().unwrap_or(Value::Null),
                    "suggested_bid_range": range(k.get("suggestedBidRange")),
                })
            })
            .collect::<Vec<_>>()
    });
    let negatives = c
        .get("negativeKeywords")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|n| {
                    json!({
                        "shopify_negative_keyword_id": n.get("id").cloned().unwrap_or(Value::Null),
                        "keyword": truncated(n.get("keyword"), 500),
                        "match_type": n.get("matchType").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect::<Vec<_>>()
        });
    let field = |key: &str| c.get(key).cloned().unwrap_or(Value::Null);

    json!({
        "shopify_campaign_id": field("id"),
        "name": truncated(c.get("name"), 255),
        "app_api_key": c.pointer("/app/apiKey").cloned().unwrap_or(Value::Null),
        "surface": field("surface"),
        "subsurface": field("subsurface"),
        "status": field("status"),
        "archived": c.get("archived").cloned().unwrap_or(json!(false)),
        "daily_budget": num(c.get("dailyBudget")),
        "bid": opt(c.get("bid")),
        "suggested_bid_range": range(c.get("suggestedBidRange")),
        "starting_at": field("startingAt"),
        "ending_at": field("endingAt"),
        "created_at": field("createdAt"),
        "targeting": {
            "countries": targeting.and_then(|t| t.get("geographicTargets")).and_then(Value::as_array)
                .map(|g| g.iter().map(|g| g.get("code").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>()).unwrap_or_default(),
            "plans": string_list(targeting.and_then(|t| t.get("planTargets"))),
            "devices": string_list(targeting.and_then(|t| t.get("deviceTypeTargets"))),
        },
        "keywords": keywords,
        "negative_keywords": negatives,
    })
}

/// One campaign-day: campaign metrics, the three breakdowns and the search terms.
pub fn day_payload(c: &Value, day: &str) -> Value {
    let breakdowns: Map<String, Value> = c
        .get("breakdowns")
        .and_then(Value::as_object)
        .map(|b| {
            b.iter()
                .map(|(target, rows)| (target.clone(), Value::Array(breakdown_rows(rows, target))))
                .collect()
        })
        .unwrap_or_default();

    let search_terms: Vec<Value> = c
        .get("searchTerms")
        .and_then(Value::as_array)
        .map(|terms| {
            terms
                .iter()
                .filter_map(|t| {
                    // A term without its keyword can't be stored; skip rather than fail the whole batch.
                    let kw = t.get("keywordBidCriterion").filter(|k| k.is_object())?;
                    let mut row = Map::new();
                    row.insert("search_term".into(), json!(truncated(t.get("term"), 500)));
                    row.insert("keyword".into(), json!(truncated(kw.get("keyword"), 500)));
                    row.insert(
                        "match_type".into(),
                        kw.get("matchType").cloned().unwrap_or(Value::Null),
                    );
                    row.insert("bid".into(), num(kw.get("bid")));
                    row.insert(
                        "shopify_keyword_id".into(),
                        kw.get("id").cloned().unwrap_or(Value::Null),
                    );
                    row.extend(metrics(t.get("metrics")));
                    Some(Value::Object(row))
                })
                .collect()
        })
        .unwrap_or_default();

    json!({
        "shopify_campaign_id": c.get("id").cloned().unwrap_or(Value::Null),
        "date": day,
        "timezone": "UTC",
        "currency": "USD",
        "metrics": metrics(c.get("metrics")),
        "breakdowns": breakdowns,
        "search_terms": search_terms,
    })
}

fn impressions(c: &Value) -> f64 {
    c.pointer("/metrics/impressions")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

/// Group by Shopify app (keyed by its API key): campaign snapshots + campaign-days with
/// impressions. `pulled` is `(day, raw campaigns)` in the order days should be sent.
pub fn build_payloads(
    snapshot: &[Value],
    pulled: &[(String, Vec<Value>)],
) -> BTreeMap<String, AppPayload> {
    let mut by_app: BTreeMap<String, AppPayload> = BTreeMap::new();

    for c in snapshot {
        if let Some(entry) = entry_for(&mut by_app, c.get("app")) {
            entry.campaigns.push(campaign_snapshot(c));
        }
    }
    for (day, campaigns) in pulled {
        for c in campaigns {
            // Days without impressions are skipped so they never replace a CSV upload.
            if impressions(c) > 0.0 {
                if let Some(entry) = entry_for(&mut by_app, c.get("app")) {
                    entry.days.push(day_payload(c, day));
                }
            }
        }
    }

    by_app
}

fn entry_for<'a>(
    by_app: &'a mut BTreeMap<String, AppPayload>,
    app: Option<&Value>,
) -> Option<&'a mut AppPayload> {
    let app = app?;
    let name = app.get("name").and_then(Value::as_str).unwrap_or_default();
    let key = app.get("apiKey").and_then(Value::as_str).unwrap_or(name);
    if key.is_empty() {
        return None;
    }

    Some(by_app.entry(key.to_string()).or_insert_with(|| AppPayload {
        name: name.to_string(),
        ..Default::default()
    }))
}

fn row_count(day: &Value) -> usize {
    let terms = day
        .get("search_terms")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let breakdowns: usize = day
        .get("breakdowns")
        .and_then(Value::as_object)
        .map_or(0, |b| {
            b.values().filter_map(Value::as_array).map(Vec::len).sum()
        });

    terms + breakdowns
}

/// Splits an app's days into requests of at most 500 days / 40k rows (newest first, in order).
/// Always returns at least one (possibly empty) batch so a campaign snapshot can go out
/// even when there are no new days.
pub fn batches(days: &[Value]) -> Vec<&[Value]> {
    let mut out = Vec::new();
    let (mut start, mut rows) = (0, 0);
    for (i, day) in days.iter().enumerate() {
        let size = row_count(day);
        if i > start && (i - start >= MAX_DAYS_PER_REQUEST || rows + size > MAX_ROWS_PER_REQUEST) {
            out.push(&days[start..i]);
            start = i;
            rows = 0;
        }
        rows += size;
    }
    out.push(&days[start..]);

    out
}

#[derive(Debug, Default, PartialEq)]
pub struct DaySummary {
    pub active: usize,
    pub spend: f64,
    pub installs: f64,
    pub terms: usize,
    pub breakdowns: usize,
}

/// Totals for the per-day progress line (raw Partners campaigns).
pub fn summarize(campaigns: &[Value]) -> DaySummary {
    campaigns.iter().fold(DaySummary::default(), |mut a, c| {
        a.active += usize::from(impressions(c) > 0.0);
        a.spend += c
            .pointer("/metrics/spend")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        a.installs += c
            .pointer("/metrics/installs")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        a.terms += c
            .get("searchTerms")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        a.breakdowns += c
            .get("breakdowns")
            .and_then(Value::as_object)
            .map_or(0, |b| {
                b.values().filter_map(Value::as_array).map(Vec::len).sum()
            });
        a
    })
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in s.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(ch);
        } else {
            dash = true;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_metrics(impressions: u64) -> Value {
        json!({
            "impressions": impressions, "clicks": 2, "clickthroughRate": 0.05, "installs": 1, "installRate": 0.5,
            "costPerClick": 1.5, "costPerInstall": 3, "costPerMille": 7.5, "spend": 3.0, "averagePosition": 2.5,
            "page1WinRate": 0.9, "customers": null, "revenue": null, "returnOnSpend": null, "costPerCustomer": null,
            "customerRate": null, "conversions": 0, "conversionRate": 0, "costPerConversion": 0
        })
    }

    fn app(key: &str, name: &str) -> Value {
        json!({ "apiKey": key, "name": name })
    }

    /// A search campaign day as returned by `inPagePullDay`.
    fn raw_search_campaign(id: &str, key: &str, name: &str, impressions: u64) -> Value {
        json!({
            "id": id, "surface": "appstore_search", "archived": false, "app": app(key, name),
            "metrics": raw_metrics(impressions),
            "breakdowns": {
                "country": [{ "metrics": raw_metrics(3), "targeting": { "geographicTargets": [{ "code": "DE" }], "planTargets": [], "deviceTypeTargets": [] } }],
                "shop_plan": [{ "metrics": raw_metrics(3), "targeting": { "geographicTargets": [], "planTargets": ["basic"], "deviceTypeTargets": [] } }],
                "device_type": [{ "metrics": raw_metrics(3), "targeting": { "geographicTargets": [], "planTargets": [], "deviceTypeTargets": [] } }]
            },
            "searchTerms": [{
                "term": "block ip", "metrics": raw_metrics(10),
                "keywordBidCriterion": { "id": "99", "keyword": "block ip address", "matchType": "exact", "bid": 7 }
            }]
        })
    }

    fn raw_snapshot_campaign(id: &str, key: &str, name: &str) -> Value {
        json!({
            "__typename": "SearchCampaign", "id": id, "name": "Search", "status": "active", "surface": "appstore_search",
            "subsurface": "appstore_search", "dailyBudget": 75, "createdAt": "2025-08-21T12:07:50Z",
            "startingAt": "2025-08-21T12:07:50Z", "endingAt": null, "app": app(key, name), "archived": false,
            "targeting": { "geographicTargets": [{ "code": "US" }], "planTargets": ["basic"], "deviceTypeTargets": ["desktop"] },
            "keywords": [{ "id": "1561502", "keyword": "block ip address", "matchType": "exact", "bid": 7, "status": "active",
                "relevanceCategory": "high", "suggestedBidRange": { "lower": 6, "upper": 6 } }],
            "negativeKeywords": [{ "id": "509986", "keyword": "cod", "matchType": "broad" }]
        })
    }

    #[test]
    fn maps_campaign_snapshot() {
        let s = campaign_snapshot(&raw_snapshot_campaign("54871", "k1", "App One"));
        assert_eq!(s["shopify_campaign_id"], "54871");
        assert_eq!(s["app_api_key"], "k1");
        assert_eq!(s["daily_budget"], 75);
        assert_eq!(s["bid"], Value::Null);
        assert_eq!(s["suggested_bid_range"], Value::Null);
        assert_eq!(s["ending_at"], Value::Null);
        assert_eq!(
            s["targeting"],
            json!({ "countries": ["US"], "plans": ["basic"], "devices": ["desktop"] })
        );
        assert_eq!(s["keywords"][0]["shopify_keyword_id"], "1561502");
        assert_eq!(
            s["keywords"][0]["suggested_bid_range"],
            json!({ "lower": 6, "upper": 6 })
        );
        assert_eq!(
            s["negative_keywords"][0],
            json!({ "shopify_negative_keyword_id": "509986", "keyword": "cod", "match_type": "broad" })
        );
    }

    #[test]
    fn display_campaign_snapshot_has_null_keyword_lists_and_bid() {
        let c = json!({
            "id": "1", "name": "Home", "status": "paused", "surface": "appstore_homepage", "dailyBudget": 15, "bid": 3,
            "suggestedBidRange": { "lower": 1, "upper": 11 }, "app": app("k1", "App One"), "archived": true,
            "targeting": { "geographicTargets": [], "planTargets": [], "deviceTypeTargets": [] }
        });
        let s = campaign_snapshot(&c);
        assert_eq!(s["keywords"], Value::Null);
        assert_eq!(s["negative_keywords"], Value::Null);
        assert_eq!(s["bid"], 3);
        assert_eq!(s["suggested_bid_range"], json!({ "lower": 1, "upper": 11 }));
        assert_eq!(s["archived"], true);
    }

    #[test]
    fn maps_a_day() {
        let d = day_payload(
            &raw_search_campaign("54871", "k1", "App One", 37),
            "2026-10-15",
        );
        assert_eq!(d["date"], "2026-10-15");
        assert_eq!(d["timezone"], "UTC");
        assert_eq!(d["currency"], "USD");
        assert_eq!(d["metrics"]["impressions"], 37);
        assert_eq!(d["metrics"]["click_through_rate"], 0.05);
        assert_eq!(d["metrics"]["visibility"], 0.9);
        assert_eq!(d["metrics"]["customers"], Value::Null);
        assert_eq!(d["breakdowns"]["country"][0]["value"], "DE");
        assert_eq!(d["breakdowns"]["shop_plan"][0]["value"], "basic");
        assert_eq!(d["breakdowns"]["device_type"][0]["value"], "unknown");
        assert_eq!(
            d["breakdowns"]["country"][0]["metrics"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .len(),
            9
        );
        let term = &d["search_terms"][0];
        assert_eq!(term["search_term"], "block ip");
        assert_eq!(term["keyword"], "block ip address");
        assert_eq!(term["match_type"], "exact");
        assert_eq!(term["shopify_keyword_id"], "99");
        assert_eq!(term["impressions"], 10);
        assert_eq!(term["bid"], 7);
    }

    #[test]
    fn groups_by_app_and_skips_days_without_impressions() {
        let snapshot = vec![
            raw_snapshot_campaign("1", "k1", "App One"),
            raw_snapshot_campaign("2", "k2", "App Two"),
            raw_snapshot_campaign("3", "k1", "App One"),
        ];
        let pulled = vec![
            (
                "2026-10-15".to_string(),
                vec![
                    raw_search_campaign("1", "k1", "App One", 5),
                    raw_search_campaign("2", "k2", "App Two", 0),
                ],
            ),
            (
                "2026-10-14".to_string(),
                vec![raw_search_campaign("1", "k1", "App One", 4)],
            ),
        ];
        let payloads = build_payloads(&snapshot, &pulled);

        assert_eq!(payloads.len(), 2);
        let one = &payloads["k1"];
        assert_eq!(one.name, "App One");
        assert_eq!(one.campaigns.len(), 2);
        let dates: Vec<_> = one
            .days
            .iter()
            .map(|d| d["date"].as_str().unwrap())
            .collect();
        assert_eq!(dates, ["2026-10-15", "2026-10-14"]);
        let two = &payloads["k2"];
        assert_eq!(two.campaigns.len(), 1);
        assert!(two.days.is_empty());
    }

    fn day_with_rows(terms: usize) -> Value {
        json!({ "search_terms": vec![json!({}); terms], "breakdowns": { "country": [{}, {}], "shop_plan": [], "device_type": [{}] } })
    }

    #[test]
    fn batches_always_yield_once() {
        let b = batches(&[]);
        assert_eq!(b.len(), 1);
        assert!(b[0].is_empty());
    }

    #[test]
    fn batches_respect_day_limit() {
        let days: Vec<Value> = (0..1203).map(|_| day_with_rows(0)).collect();
        let sizes: Vec<_> = batches(&days).iter().map(|b| b.len()).collect();
        assert_eq!(sizes, [500, 500, 203]);
    }

    #[test]
    fn batches_respect_row_limit() {
        // 3 breakdown rows per day + 19_997 terms = 20_000 rows per day: two per batch.
        let days: Vec<Value> = (0..5).map(|_| day_with_rows(19_997)).collect();
        let sizes: Vec<_> = batches(&days).iter().map(|b| b.len()).collect();
        assert_eq!(sizes, [2, 2, 1]);
    }

    #[test]
    fn oversized_single_day_still_goes_out_alone() {
        let days = vec![day_with_rows(45_000), day_with_rows(1)];
        let sizes: Vec<_> = batches(&days).iter().map(|b| b.len()).collect();
        assert_eq!(sizes, [1, 1]);
    }

    #[test]
    fn summarizes_a_pulled_day() {
        let s = summarize(&[
            raw_search_campaign("1", "k", "A", 5),
            raw_search_campaign("2", "k", "A", 0),
        ]);
        assert_eq!(s.active, 1);
        assert_eq!(s.terms, 2);
        assert_eq!(s.breakdowns, 6);
        assert_eq!(s.installs, 2.0);
        assert_eq!(s.spend, 6.0);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("My App: Pro!"), "my-app-pro");
        assert_eq!(slug("  --  "), "");
    }
}
