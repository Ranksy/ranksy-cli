use std::process::Command;

fn ranksy(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ranksy"))
        .args(args)
        .env_remove("RANKSY_API_KEY")
        .output()
        .expect("run ranksy")
}

#[test]
fn docs_markdown_prints_the_reference_without_an_api_key() {
    let out = ranksy(&["docs", "markdown"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let md = String::from_utf8_lossy(&out.stdout);
    assert!(md.starts_with("## Global options\n"), "got: {md}");
    assert!(md.contains("## ranksy ads sync {{ tag: 'POST', label: '/api/v1/apps/:app/shopify-ads/sync' }}"));
}

#[test]
fn docs_command_is_hidden_from_help() {
    let out = ranksy(&["--help"]);
    assert!(out.status.success());
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(!help.contains("docs"), "got: {help}");
}
