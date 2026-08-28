#[test]
fn generated_client_constructs() {
    let http = reqwest::Client::new();
    let _client = ranksy_api::generated::Client::new_with_client(
        "https://example.test/api",
        http,
    );
}
