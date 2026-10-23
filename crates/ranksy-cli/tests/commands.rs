use std::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ranksy(server: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ranksy"))
        .args(["--base-url", server])
        .args(args)
        .env("RANKSY_API_KEY", "k")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("run ranksy")
}

#[tokio::test]
async fn rankings_get_passes_app_and_renders_table() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/rankings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"rank": 4, "change": 2, "date": "2026-05-29"}
        ])))
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "rankings", "get"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains('4') && s.contains("2026-05-29"), "got: {s}");
}

#[tokio::test]
async fn keywords_list_passes_app_and_renders_table() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/keywords"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"keyword": "shopify seo", "organic_rank": 4, "sponsored_rank": 2}]
        })))
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "keywords", "list"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("shopify seo"), "got: {s}");
}

#[tokio::test]
async fn reviews_list_hits_v1_reviews() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"rating": 5, "author": "Acme Store", "body": "Works great"}]
        })))
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "reviews", "list"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("Acme Store"), "got: {s}");
}

#[tokio::test]
async fn installs_get_hits_v1_installs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/installs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"source": "search", "installs": 10, "share": 0.4}]
        })))
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "installs", "get"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("search"), "got: {s}");
}

#[tokio::test]
async fn apps_list_hits_v1_apps() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"ulid": "01HZX9A", "slug": "my-app", "name": "My App"}]
        })))
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["apps", "list"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("My App"), "got: {s}");
}

#[tokio::test]
async fn api_error_exits_one() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/reviews"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(serde_json::json!({"message": "forbidden"})),
        )
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "reviews", "list"]);
    assert_eq!(out.status.code(), Some(1));
}

#[tokio::test]
async fn keywords_track_posts_and_renders() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/apps/app_123/keywords"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "object": "tracked_keyword", "keyword": "shopify seo", "slug": "shopify-seo"
        })))
        .mount(&server)
        .await;

    let out = ranksy(
        &server.uri(),
        &["--app", "app_123", "keywords", "track", "shopify seo"],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("shopify-seo"));
}

#[tokio::test]
async fn keywords_untrack_deletes_by_slug() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/apps/app_123/keywords/shopify-seo"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let out = ranksy(
        &server.uri(),
        &["--app", "app_123", "keywords", "untrack", "Shopify SEO"],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("untracked_keyword"));
}

#[tokio::test]
async fn listing_get_reads_listing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/listing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "listing", "name": "My App"
        })))
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "listing", "get"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("My App"));
}

#[tokio::test]
async fn reads_api_key_and_base_url_from_cwd_dotenv() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps"))
        .and(wiremock::matchers::header(
            "authorization",
            "Bearer env-file-key",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": [{"id": 9}]})),
        )
        .mount(&server)
        .await;

    // A project-local .env, no exported RANKSY_* — the CLI must source it.
    let dir = std::env::temp_dir().join(format!("ranksy-dotenv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".env"),
        format!(
            "RANKSY_API_KEY=env-file-key\nRANKSY_BASE_URL={}\n",
            server.uri()
        ),
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_ranksy"))
        .current_dir(&dir)
        .args(["apps", "list"])
        .env_remove("RANKSY_API_KEY")
        .env_remove("RANKSY_BASE_URL")
        .output()
        .expect("run ranksy");

    std::fs::remove_dir_all(&dir).ok();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
async fn ads_keywords_sends_the_filters_and_renders_a_table() {
    use wiremock::matchers::query_param;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/shopify-ads/keywords"))
        .and(query_param("start_date", "2026-09-01"))
        .and(query_param("campaign", "9876543"))
        .and(query_param("sort_by", "cpi"))
        .and(query_param("limit", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "shopify_ads_keywords",
            "period": {"start_date": "2026-09-01", "end_date": "2026-09-30"},
            "campaign": {"id": 412, "shopify_campaign_id": "9876543", "name": "Search - core keywords"},
            "ad_type": null, "level": "campaign", "sort_by": "cpi", "total_count": 1,
            "keywords": [{"keyword": "email marketing", "match_type": "exact", "spend": 588.2, "cpi": null, "flags": []}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let out = ranksy(
        &server.uri(),
        &[
            "--app",
            "app_123",
            "ads",
            "keywords",
            "--start-date",
            "2026-09-01",
            "--campaign",
            "9876543",
            "--sort-by",
            "cpi",
            "--limit",
            "2",
        ],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("email marketing") && s.contains("Search - core keywords") && s.contains('–'),
        "got: {s}"
    );
}

#[tokio::test]
async fn ads_overview_json_passes_the_response_through() {
    use wiremock::matchers::query_param;
    let server = MockServer::start().await;
    let body = serde_json::json!({"object": "shopify_ads_overview", "kpis": {"current": {"spend": 0}}, "campaigns": []});
    Mock::given(method("GET"))
        .and(path("/apps/app_123/shopify-ads/overview"))
        .and(query_param("compare", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .expect(1)
        .mount(&server)
        .await;

    let out = ranksy(
        &server.uri(),
        &["--app", "app_123", "--json", "ads", "overview", "--compare"],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed, body);
}

#[tokio::test]
async fn ads_read_reports_an_unknown_campaign() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/shopify-ads/trend"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": {"type": "invalid_request_error", "code": "resource_not_found", "param": "campaign", "message": "No Shopify ads campaign 'nope' for this app."}
        })))
        .mount(&server)
        .await;

    let out = ranksy(
        &server.uri(),
        &["--app", "app_123", "ads", "trend", "--campaign", "nope"],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr)
        .contains("Campaign not found: No Shopify ads campaign 'nope'"));
}

#[test]
fn ads_read_rejects_an_unknown_ad_type_before_calling() {
    let out = ranksy(
        "http://127.0.0.1:9",
        &[
            "--app",
            "app_123",
            "ads",
            "search-terms",
            "--ad-type",
            "banner",
        ],
    );
    assert_eq!(out.status.code(), Some(2));
}
