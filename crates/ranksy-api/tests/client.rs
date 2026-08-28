use ranksy_api::{ClientConfig, RanksyClient};
use wiremock::matchers::{header, method, path};
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
