use ranksy_api::{ClientConfig, RanksyClient};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn whoami_sends_bearer_and_returns_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": [{"id": 7}]})))
        .mount(&server)
        .await;

    let client = RanksyClient::new(ClientConfig {
        api_key: "test-key".into(),
        base_url: server.uri(),
    })
    .unwrap();

    let v = client.whoami().await.unwrap();
    assert_eq!(v["data"][0]["id"], 7);
}

#[tokio::test]
async fn retries_on_500_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/apps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": [{"id": 1}]})))
        .mount(&server)
        .await;

    let client = RanksyClient::new(ClientConfig { api_key: "k".into(), base_url: server.uri() }).unwrap();
    assert_eq!(client.whoami().await.unwrap()["data"][0]["id"], 1);
}

#[tokio::test]
async fn track_keyword_posts_the_raw_keyword() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/apps/app_1/keywords"))
        .and(body_json(serde_json::json!({ "keyword": "Email Marketing" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(
            serde_json::json!({"object": "tracked_keyword", "keyword": "Email Marketing", "slug": "email-marketing"}),
        ))
        .mount(&server)
        .await;

    let client = RanksyClient::new(ClientConfig { api_key: "k".into(), base_url: server.uri() }).unwrap();
    let v = client.track_keyword("app_1", "Email Marketing").await.unwrap();
    assert_eq!(v["slug"], "email-marketing");
}

#[tokio::test]
async fn untrack_keyword_deletes_by_slug_and_confirms() {
    let server = MockServer::start().await;
    // Human keyword slugifies to the path segment the endpoint keys on.
    Mock::given(method("DELETE"))
        .and(path("/apps/app_1/keywords/email-marketing"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let client = RanksyClient::new(ClientConfig { api_key: "k".into(), base_url: server.uri() }).unwrap();
    let v = client.untrack_keyword("app_1", "Email Marketing").await.unwrap();
    assert_eq!(v["object"], "untracked_keyword");
    assert_eq!(v["slug"], "email-marketing");
    assert_eq!(v["keyword"], "Email Marketing");
}

#[tokio::test]
async fn untrack_missing_keyword_surfaces_404() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/apps/app_1/keywords/nope"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({"error": "not tracked"})))
        .mount(&server)
        .await;

    let client = RanksyClient::new(ClientConfig { api_key: "k".into(), base_url: server.uri() }).unwrap();
    let err = client.untrack_keyword("app_1", "nope").await.unwrap_err();
    assert!(matches!(err, ranksy_api::ApiError::Status { code: 404, .. }));
}

#[tokio::test]
async fn get_listing_reads_the_listing_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_1/listing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"object": "listing", "name": "My App"})))
        .mount(&server)
        .await;

    let client = RanksyClient::new(ClientConfig { api_key: "k".into(), base_url: server.uri() }).unwrap();
    let v = client.get_listing("app_1").await.unwrap();
    assert_eq!(v["name"], "My App");
}

#[tokio::test]
async fn rankings_get_with_keyword_hits_by_keyword_slug() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps/app_1/rankings/by-keyword/email-marketing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{"rank": 3}])))
        .mount(&server)
        .await;

    let client = RanksyClient::new(ClientConfig { api_key: "k".into(), base_url: server.uri() }).unwrap();
    let v = client.get_rankings("app_1", Some("Email Marketing")).await.unwrap();
    assert_eq!(v[0]["rank"], 3);
}
