use std::process::Command;

#[test]
fn prints_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_ranksy"))
        .arg("--version")
        .output()
        .expect("run ranksy");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ranksy"), "got: {stdout}");
}
