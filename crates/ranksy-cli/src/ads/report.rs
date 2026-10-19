//! `ranksy ads overview|trend|keywords|search-terms`: read the Shopify Ads performance that
//! `ads sync` stored, from `GET /apps/{app}/shopify-ads/*`.
//!
//! `--json` prints the response as is. The tables keep a column only when the API returned its
//! field: a key without `metrics:read` gets no `revenue` or `roas` at all, so those columns
//! disappear instead of showing blanks. A `null` value prints as `–`.

use crate::cli::{AdsCmd, AdsReportArgs};
use anyhow::{anyhow, Result};
use ranksy_api::{ApiError, RanksyClient};
use serde_json::Value;
use tabled::builder::Builder;
use tabled::settings::Style;

const NONE: &str = "–";

/// The endpoint and query of a read command, or `None` for connect, disconnect and sync.
pub fn request(cmd: &AdsCmd) -> Option<(&'static str, Vec<(&'static str, String)>)> {
    let (suffix, filters, mut extra) = match cmd {
        AdsCmd::Overview { filters, compare } => {
            ("shopify-ads/overview", filters, if *compare { vec![("compare", "true".to_string())] } else { vec![] })
        }
        AdsCmd::Trend { filters, grouping } => ("shopify-ads/trend", filters, opt("grouping", grouping)),
        AdsCmd::Keywords { filters, sort_by, limit } => {
            ("shopify-ads/keywords", filters, [opt("sort_by", sort_by), opt("limit", limit)].concat())
        }
        AdsCmd::SearchTerms { filters, flag, limit } => {
            ("shopify-ads/search-terms", filters, [opt("flag", flag), opt("limit", limit)].concat())
        }
        AdsCmd::Connect { .. } | AdsCmd::Disconnect { .. } | AdsCmd::Sync { .. } => return None,
    };

    let mut query = shared(filters);
    query.append(&mut extra);
    Some((suffix, query))
}

fn shared(f: &AdsReportArgs) -> Vec<(&'static str, String)> {
    [
        opt("start_date", &f.start_date.map(|d| d.to_string())),
        opt("end_date", &f.end_date.map(|d| d.to_string())),
        opt("campaign", &f.campaign),
        opt("ad_type", &f.ad_type),
    ]
    .concat()
}

fn opt<T: ToString>(key: &'static str, value: &Option<T>) -> Vec<(&'static str, String)> {
    value.iter().map(|v| (key, v.to_string())).collect()
}

/// Calls the endpoint of a read command.
pub async fn fetch(client: &RanksyClient, app: &str, cmd: &AdsCmd) -> Result<Value> {
    let (suffix, query) = request(cmd).ok_or_else(|| anyhow!("not an ads read command"))?;
    let query: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
    client.get_app(app, suffix, &query).await.map_err(read_error)
}

/// A readable error for the ads read endpoints.
pub fn read_error(e: ApiError) -> anyhow::Error {
    let ApiError::Status { code, body } = &e else {
        return e.into();
    };
    let error = serde_json::from_str::<Value>(body).ok().and_then(|v| v.get("error").cloned()).unwrap_or(Value::Null);
    let message = error.get("message").and_then(Value::as_str);

    match code {
        401 => anyhow!("Ranksy rejected the API key (401). Check `ranksy login`, RANKSY_API_KEY or --api-key."),
        403 => anyhow!("The API key is missing a scope (403): the ads commands need `ads:read`."),
        404 if error.get("param").and_then(Value::as_str) == Some("campaign") => anyhow!(
            "Campaign not found: {}",
            message.unwrap_or("this app has no campaign with that Shopify campaign id or name.")
        ),
        _ => match message {
            Some(m) => anyhow!("api returned {code}: {m}"),
            None => e.into(),
        },
    }
}

/// The human output of a read command.
pub fn table(cmd: &AdsCmd, v: &Value) -> String {
    let mut out = vec![header(v)];
    match cmd {
        AdsCmd::Overview { .. } => overview(v, &mut out),
        AdsCmd::Trend { .. } => {
            out.push(rows_or(v, "series", TREND, "No ads data in this period."));
        }
        AdsCmd::Keywords { .. } => {
            out.push(rows_or(v, "keywords", KEYWORDS, "No keywords in this period."));
            out.extend(shown(v, "keywords"));
        }
        AdsCmd::SearchTerms { .. } => {
            out.push(rows_or(v, "search_terms", SEARCH_TERMS, "No search terms in this period."));
            out.extend(shown(v, "search_terms"));
        }
        AdsCmd::Connect { .. } | AdsCmd::Disconnect { .. } | AdsCmd::Sync { .. } => {}
    }
    out.join("\n\n")
}

/// `Period: 2026-09-19 to 2026-10-18 · Campaign: X (123) · Ad type: search · Grouping: day`.
fn header(v: &Value) -> String {
    let mut parts = vec![format!(
        "Period: {} to {}",
        text(&v["period"]["start_date"]),
        text(&v["period"]["end_date"])
    )];
    if let Some(c) = v.get("campaign").filter(|c| !c.is_null()) {
        parts.push(format!("Campaign: {} ({})", text(&c["name"]), text(&c["shopify_campaign_id"])));
    }
    for (label, key) in [("Ad type", "ad_type"), ("Grouping", "grouping"), ("Level", "level"), ("Sort", "sort_by"), ("Flag", "flag")] {
        if let Some(s) = v.get(key).and_then(Value::as_str) {
            parts.push(format!("{label}: {s}"));
        }
    }
    parts.join(" · ")
}

const KPIS: &[(&str, &str)] = &[
    ("Impressions", "impressions"),
    ("Clicks", "clicks"),
    ("Installs", "installs"),
    ("Paying customers", "customers"),
    ("Spend", "spend"),
    ("Revenue", "revenue"),
    ("CTR %", "ctr"),
    ("Install rate %", "install_rate"),
    ("CPC", "cpc"),
    ("CPI", "cpi"),
    ("ROAS", "roas"),
    ("Avg position", "avg_position"),
];

const CAMPAIGNS: &[(&str, &str)] = &[
    ("Campaign", "name"),
    ("Shopify id", "shopify_campaign_id"),
    ("Type", "ad_type"),
    ("Status", "status"),
    ("Budget", "daily_budget"),
    ("Pacing", "budget_pacing"),
    ("Spend", "spend"),
    ("Share %", "share_of_spend"),
    ("Impressions", "impressions"),
    ("Clicks", "clicks"),
    ("Installs", "installs"),
    ("Customers", "customers"),
    ("CTR %", "ctr"),
    ("CPC", "cpc"),
    ("CPI", "cpi"),
    ("CAC", "cac"),
    ("Avg position", "avg_position"),
    ("Revenue", "revenue"),
    ("ROAS", "roas"),
];

const TREND: &[(&str, &str)] = &[
    ("Date", "date"),
    ("Spend", "spend"),
    ("Impressions", "impressions"),
    ("Clicks", "clicks"),
    ("Installs", "installs"),
    ("CTR %", "ctr"),
    ("CPI", "cpi"),
    ("Avg position", "avg_position"),
    ("Revenue", "revenue"),
    ("ROAS", "roas"),
];

const KEYWORDS: &[(&str, &str)] = &[
    ("Keyword", "keyword"),
    ("Match", "match_type"),
    ("Status", "status"),
    ("Bid", "bid"),
    ("Bid vs suggested", "bid_position"),
    ("Spend", "spend"),
    ("Share %", "share_of_spend"),
    ("Impressions", "impressions"),
    ("Clicks", "clicks"),
    ("Installs", "installs"),
    ("Customers", "customers"),
    ("CTR %", "ctr"),
    ("Install rate %", "install_rate"),
    ("CPC", "cpc"),
    ("CPI", "cpi"),
    ("Avg position", "avg_position"),
    ("Revenue", "revenue"),
    ("ROAS", "roas"),
    ("Flags", "flags"),
];

const SEARCH_TERMS: &[(&str, &str)] = &[
    ("Search term", "search_term"),
    ("Keyword", "keyword"),
    ("Match", "match_type"),
    ("Spend", "spend"),
    ("Impressions", "impressions"),
    ("Clicks", "clicks"),
    ("Installs", "installs"),
    ("CTR %", "ctr"),
    ("Install rate %", "install_rate"),
    ("CPC", "cpc"),
    ("CPI", "cpi"),
    ("Avg position", "avg_position"),
    ("Flags", "flags"),
];

fn overview(v: &Value, out: &mut Vec<String>) {
    if let Some(m) = v.get("message").and_then(Value::as_str) {
        out.push(m.to_string());
    }

    let current = &v["kpis"]["current"];
    let previous = v["kpis"].get("previous").filter(|p| !p.is_null());
    let mut kpis = Builder::default();
    kpis.push_record(if previous.is_some() { vec!["", "Current", "Previous"] } else { vec!["", "Value"] });
    for (label, key) in KPIS.iter().filter(|(_, k)| current.get(*k).is_some()) {
        let mut row = vec![label.to_string(), text(&current[*key])];
        if let Some(p) = previous {
            row.push(p.get(*key).map_or_else(|| NONE.to_string(), text));
        }
        kpis.push_record(row);
    }
    out.push(kpis.build().with(Style::rounded()).to_string());

    if let Some(steps) = v["funnel"]["steps"].as_array().filter(|s| !s.is_empty()) {
        let compare = steps.iter().any(|s| s.get("previous_rate").is_some());
        let mut funnel = Builder::default();
        let mut head = vec!["Funnel step", "Base", "Rate %"];
        if compare {
            head.extend(["Previous %", "Change (pts)"]);
        }
        funnel.push_record(head);
        for s in steps {
            let mut row = vec![text(&s["label"]), text(&s["base"]), text(&s["rate"])];
            if compare {
                row.extend([text(&s["previous_rate"]), text(&s["delta_points"])]);
            }
            funnel.push_record(row);
        }
        out.push(funnel.build().with(Style::rounded()).to_string());
    }

    let costs = &v["funnel"]["stage_costs"];
    if costs.is_object() {
        out.push(format!("Cost per click {} · per install {} · per paying customer {}", text(&costs["cpc"]), text(&costs["cpi"]), text(&costs["cac"])));
    }

    out.push(rows_or(v, "campaigns", CAMPAIGNS, "No campaigns."));

    if let Some(h) = v.get("hidden_archived").filter(|h| h.is_object()) {
        out.push(format!("Left out: {} archived campaign(s) that spent {}.", text(&h["count"]), text(&h["spend"])));
    }
    if let Some(n) = v["excluded_csv_reports"].as_array().map(Vec::len).filter(|n| *n > 0) {
        out.push(format!("Left out: {n} uploaded CSV report(s) that do not fit inside the period."));
    }
}

/// A table of `v[key]`, or `empty` when the list has no rows.
fn rows_or(v: &Value, key: &str, columns: &[(&str, &str)], empty: &str) -> String {
    match v.get(key).and_then(Value::as_array) {
        Some(rows) if !rows.is_empty() => rows_table(rows, columns),
        _ => empty.to_string(),
    }
}

/// Keeps a column only when at least one row has its field, so absent fields (revenue and ROAS
/// without `metrics:read`, campaign-only keyword fields) leave no empty column.
fn rows_table(rows: &[Value], columns: &[(&str, &str)]) -> String {
    let present: Vec<&(&str, &str)> = columns.iter().filter(|(_, k)| rows.iter().any(|r| r.get(*k).is_some() || flags_key(r, k))).collect();
    let mut b = Builder::default();
    b.push_record(present.iter().map(|(h, _)| h.to_string()));
    for row in rows {
        b.push_record(present.iter().map(|(_, k)| if *k == "flags" { flags(row) } else { text(row.get(*k).unwrap_or(&Value::Null)) }));
    }
    b.build().with(Style::rounded()).to_string()
}

/// Search-term rows carry their flags as three booleans rather than a `flags` list.
const TERM_FLAGS: &[(&str, &str)] = &[
    ("not_a_keyword", "not a keyword"),
    ("spending_no_installs", "no installs"),
    ("converting_not_yet_keyword", "add as keyword"),
];

fn flags_key(row: &Value, key: &str) -> bool {
    key == "flags" && TERM_FLAGS.iter().any(|(k, _)| row.get(*k).is_some())
}

fn flags(row: &Value) -> String {
    let list: Vec<String> = match row.get("flags").and_then(Value::as_array) {
        Some(a) => a.iter().map(text).collect(),
        None => TERM_FLAGS.iter().filter(|(k, _)| row[*k].as_bool() == Some(true)).map(|(_, label)| label.to_string()).collect(),
    };
    list.join(", ")
}

/// `Showing 2 of 37.` when the list was cut by --limit.
fn shown(v: &Value, key: &str) -> Option<String> {
    let n = v.get(key)?.as_array()?.len();
    let total = v.get("total_count")?.as_u64()?;
    (total > n as u64).then(|| format!("Showing {n} of {total}. Raise --limit for more."))
}

fn text(v: &Value) -> String {
    match v {
        Value::Null => NONE.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde_json::json;

    fn filters() -> AdsReportArgs {
        AdsReportArgs { start_date: None, end_date: None, campaign: None, ad_type: None }
    }

    fn overview_cmd() -> AdsCmd {
        AdsCmd::Overview { filters: filters(), compare: false }
    }

    #[test]
    fn sends_only_the_flags_that_were_given() {
        let (suffix, query) = request(&AdsCmd::Trend { filters: filters(), grouping: None }).unwrap();
        assert_eq!(suffix, "shopify-ads/trend");
        assert!(query.is_empty());
    }

    #[test]
    fn builds_the_shared_and_endpoint_query() {
        let cmd = AdsCmd::Keywords {
            filters: AdsReportArgs {
                start_date: NaiveDate::from_ymd_opt(2026, 9, 1),
                end_date: NaiveDate::from_ymd_opt(2026, 9, 30),
                campaign: Some("Search - core".into()),
                ad_type: Some("search".into()),
            },
            sort_by: Some("cpi".into()),
            limit: Some(200),
        };
        let (suffix, query) = request(&cmd).unwrap();
        assert_eq!(suffix, "shopify-ads/keywords");
        let query: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(
            query,
            [
                ("start_date", "2026-09-01"),
                ("end_date", "2026-09-30"),
                ("campaign", "Search - core"),
                ("ad_type", "search"),
                ("sort_by", "cpi"),
                ("limit", "200"),
            ]
        );
    }

    #[test]
    fn overview_sends_compare_and_search_terms_its_flag() {
        let (_, q) = request(&AdsCmd::Overview { filters: filters(), compare: true }).unwrap();
        assert_eq!(q, [("compare", "true".to_string())]);
        let (suffix, q) = request(&AdsCmd::SearchTerms { filters: filters(), flag: Some("converting".into()), limit: None }).unwrap();
        assert_eq!(suffix, "shopify-ads/search-terms");
        assert_eq!(q, [("flag", "converting".to_string())]);
    }

    #[test]
    fn sync_is_not_a_read_command() {
        assert!(request(&AdsCmd::Disconnect { org: "1".into() }).is_none());
    }

    fn overview_body(revenue: bool) -> Value {
        let mut kpis = json!({"impressions": 100, "clicks": 4, "installs": 0, "spend": 12.5, "customers": 0, "ctr": 4.0, "install_rate": 0.0, "cpc": 3.13, "cpi": 0, "avg_position": null});
        let mut campaign = json!({"name": "Search - core", "shopify_campaign_id": "987", "ad_type": "search", "status": "active", "daily_budget": null, "budget_pacing": null, "spend": 12.5, "cpi": null});
        if revenue {
            kpis["revenue"] = json!(40.0);
            kpis["roas"] = json!(3.2);
            campaign["revenue"] = json!(40.0);
            campaign["roas"] = json!(3.2);
        }
        json!({
            "object": "shopify_ads_overview",
            "period": {"start_date": "2026-09-19", "end_date": "2026-10-18"},
            "campaign": null,
            "ad_type": null,
            "kpis": {"current": kpis, "previous": null},
            "funnel": {"steps": [{"label": "CTR", "base": 100, "rate": 4.0}], "stage_costs": {"cpc": 3.13, "cpi": null, "cac": null}},
            "campaigns": [campaign],
            "hidden_archived": null,
            "excluded_csv_reports": []
        })
    }

    #[test]
    fn overview_shows_revenue_when_the_key_can_read_it() {
        let out = table(&overview_cmd(), &overview_body(true));
        assert!(out.contains("Period: 2026-09-19 to 2026-10-18"), "{out}");
        assert!(out.contains("Revenue") && out.contains("ROAS") && out.contains("3.2"), "{out}");
    }

    #[test]
    fn overview_drops_revenue_and_roas_when_absent() {
        let out = table(&overview_cmd(), &overview_body(false));
        assert!(!out.contains("Revenue") && !out.contains("ROAS"), "{out}");
        assert!(out.contains("Spend") && out.contains("12.5"), "{out}");
    }

    #[test]
    fn nulls_render_as_a_dash() {
        let out = table(&overview_cmd(), &overview_body(false));
        assert!(out.contains("per install –"), "{out}");
        let avg = out.lines().find(|l| l.contains("Avg position")).unwrap();
        assert!(avg.contains('–'), "{avg}");
    }

    #[test]
    fn overview_prints_the_api_message_for_an_empty_period() {
        let mut body = overview_body(false);
        body["message"] = json!("No ads activity in this period.");
        body["campaigns"] = json!([]);
        let out = table(&overview_cmd(), &body);
        assert!(out.contains("No ads activity in this period.") && out.contains("No campaigns."), "{out}");
    }

    #[test]
    fn overview_with_compare_shows_the_previous_period() {
        let mut body = overview_body(false);
        body["kpis"]["previous"] = json!({"spend": 10.0});
        let out = table(&AdsCmd::Overview { filters: filters(), compare: true }, &body);
        assert!(out.contains("Previous") && out.contains("10.0"), "{out}");
    }

    #[test]
    fn trend_drops_absent_columns_and_dashes_nulls() {
        let body = json!({
            "period": {"start_date": "2026-10-16", "end_date": "2026-10-17"},
            "grouping": "day",
            "series": [{"date": "2026-10-16", "spend": 5.0, "impressions": 10, "clicks": 0, "installs": 0, "ctr": 0.0, "cpi": null, "avg_position": null}]
        });
        let out = table(&AdsCmd::Trend { filters: filters(), grouping: None }, &body);
        assert!(out.contains("Grouping: day") && out.contains("2026-10-16"), "{out}");
        assert!(!out.contains("ROAS") && out.contains('–'), "{out}");
    }

    #[test]
    fn keywords_show_flags_and_the_limit_note() {
        let body = json!({
            "period": {"start_date": "2026-10-01", "end_date": "2026-10-18"},
            "level": "campaign",
            "total_count": 37,
            "keywords": [{"keyword": "newsletter app", "match_type": "exact", "bid": 0.9, "cpi": null, "flags": ["spend_no_installs", "bid_below_suggested_range"]}]
        });
        let out = table(&AdsCmd::Keywords { filters: filters(), sort_by: None, limit: None }, &body);
        assert!(out.contains("spend_no_installs, bid_below_suggested_range"), "{out}");
        assert!(out.contains("Showing 1 of 37."), "{out}");
        assert!(!out.contains("Revenue") && !out.contains("Customers"), "{out}");
    }

    #[test]
    fn search_terms_turn_the_booleans_into_flags() {
        let body = json!({
            "period": {"start_date": "2026-10-01", "end_date": "2026-10-18"},
            "total_count": 1,
            "search_terms": [{"search_term": "abandoned cart email", "keyword": "email marketing", "match_type": null, "not_a_keyword": true, "spending_no_installs": false, "converting_not_yet_keyword": true}]
        });
        let out = table(&AdsCmd::SearchTerms { filters: filters(), flag: None, limit: None }, &body);
        assert!(out.contains("not a keyword, add as keyword"), "{out}");
        assert!(!out.contains("Showing"), "{out}");
    }

    #[test]
    fn empty_lists_say_so() {
        let body = json!({"period": {"start_date": "2026-10-01", "end_date": "2026-10-18"}, "keywords": [], "total_count": 0});
        let out = table(&AdsCmd::Keywords { filters: filters(), sort_by: None, limit: None }, &body);
        assert!(out.contains("No keywords in this period."), "{out}");
    }

    fn status(code: u16, body: Value) -> ApiError {
        ApiError::Status { code, body: body.to_string() }
    }

    #[test]
    fn maps_a_missing_campaign_to_a_clear_message() {
        let e = read_error(status(404, json!({"error": {"code": "resource_not_found", "param": "campaign", "message": "No Shopify ads campaign 'x' for this app."}})));
        assert_eq!(e.to_string(), "Campaign not found: No Shopify ads campaign 'x' for this app.");
    }

    #[test]
    fn maps_forbidden_to_the_missing_scope() {
        let e = read_error(status(403, json!({"error": {"code": "insufficient_permissions"}})));
        assert!(e.to_string().contains("`ads:read`"), "{e}");
    }

    #[test]
    fn other_errors_keep_the_api_message() {
        let e = read_error(status(404, json!({"error": {"code": "resource_not_found", "message": "App not found."}})));
        assert_eq!(e.to_string(), "api returned 404: App not found.");
        let e = read_error(ApiError::Status { code: 500, body: "boom".into() });
        assert_eq!(e.to_string(), "api returned 500: boom");
    }
}
