use std::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn whoami_renders_json_with_env_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/apps"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"email": "goran@ranksyapp.com"})))
        .mount(&server)
        .await;

    let out = Command::new(env!("CARGO_BIN_EXE_ranksy"))
        .args(["--json", "--base-url", &server.uri(), "whoami"])
        .env("RANKSY_API_KEY", "test-key")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("run ranksy");

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("goran@ranksyapp.com"), "got: {stdout}");
}

#[test]
fn missing_key_exits_nonzero() {
    let out = Command::new(env!("CARGO_BIN_EXE_ranksy"))
        .args(["whoami"])
        .env_remove("RANKSY_API_KEY")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", "/tmp/ranksy-empty-home")
        .output()
        .expect("run ranksy");
    assert!(!out.status.success());
    assert_ne!(out.status.code(), Some(2));
}
