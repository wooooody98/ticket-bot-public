use std::path::PathBuf;
use std::process::Command;

fn example_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config.yaml.example")
}

#[test]
fn show_config_smoke_uses_repo_example_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_ticket-bot2"))
        .arg("--config")
        .arg(example_config_path())
        .arg("show-config")
        .output()
        .expect("show-config should execute");

    assert!(output.status.success(), "{output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config loaded"), "stdout={stdout}");
    assert!(stdout.contains("selected event: 活動名稱"), "stdout={stdout}");
    assert!(
        stdout.contains("event url: https://tixcraft.com/activity/game/your_event"),
        "stdout={stdout}"
    );
}

#[test]
fn api_watch_dry_run_smoke_builds_plan_from_repo_example_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_ticket-bot2"))
        .arg("--config")
        .arg(example_config_path())
        .arg("api-watch-dry-run")
        .output()
        .expect("api-watch-dry-run should execute");

    assert!(output.status.success(), "{output:?}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("api watch dry-run"), "stdout={stdout}");
    assert!(stdout.contains("event: 活動名稱"), "stdout={stdout}");
    assert!(stdout.contains("probe skipped"), "stdout={stdout}");
    assert!(stdout.contains("watch preview skipped"), "stdout={stdout}");
}
