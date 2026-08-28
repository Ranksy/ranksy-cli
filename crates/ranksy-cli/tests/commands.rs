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
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
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
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("shopify seo"), "got: {s}");
}

#[tokio::test]
async fn keywords_track_is_stubbed_with_exit_one() {
    let server = MockServer::start().await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "keywords", "track", "checkout upsell"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no API v1 endpoint"), "stderr: {err}");
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
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
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
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
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
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("My App"), "got: {s}");
}

#[tokio::test]
async fn api_error_exits_one() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_123/reviews"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({"message": "forbidden"})))
        .mount(&server)
        .await;

    let out = ranksy(&server.uri(), &["--app", "app_123", "reviews", "list"]);
    assert_eq!(out.status.code(), Some(1));
}
