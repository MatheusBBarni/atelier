use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

/// Orchestrator on the always-available `fake` runtime → a healthy doctor report.
const HEALTHY_CONFIG: &str = "schema_version = 1\n\
     [runtimes.fake]\ntype = \"fake\"\n\
     [agents.orchestrator]\nruntime = \"fake\"\n";

/// Orchestrator pointed at a codex runtime whose command does not exist →
/// the required runtime is Unavailable → the report has an error.
const ORCHESTRATOR_DOWN_CONFIG: &str = "schema_version = 1\n\
     [runtimes.codex]\ncommand = \"atelier-nonexistent-binary-xyz-12345\"\n\
     [agents.orchestrator]\nruntime = \"codex\"\n";

fn write_config(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("atelier.toml");
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn doctor_strict_succeeds_on_healthy_config() {
    let (work, config) = write_config(HEALTHY_CONFIG);
    Command::cargo_bin("atelier")
        .unwrap()
        .current_dir(work.path())
        .args(["--doctor", "--strict", "--config"])
        .arg(&config)
        .assert()
        .success();
}

#[test]
fn doctor_strict_fails_when_orchestrator_runtime_unavailable() {
    let (work, config) = write_config(ORCHESTRATOR_DOWN_CONFIG);
    Command::cargo_bin("atelier")
        .unwrap()
        .current_dir(work.path())
        .args(["--doctor", "--strict", "--config"])
        .arg(&config)
        .assert()
        .failure();
}

#[test]
fn plain_doctor_with_errors_exits_zero_and_nudges_on_stderr() {
    let (work, config) = write_config(ORCHESTRATOR_DOWN_CONFIG);
    Command::cargo_bin("atelier")
        .unwrap()
        .current_dir(work.path())
        .args(["--doctor", "--config"])
        .arg(&config)
        .assert()
        .success()
        .stderr(predicate::str::contains("--strict"));
}

#[test]
fn strict_without_doctor_fails_with_bail_message() {
    let (work, config) = write_config(HEALTHY_CONFIG);
    Command::cargo_bin("atelier")
        .unwrap()
        .current_dir(work.path())
        .args(["--strict", "--config"])
        .arg(&config)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--strict is only valid with --doctor",
        ));
}

#[test]
fn doctor_strict_json_keeps_stdout_clean_and_exits_nonzero() {
    let (work, config) = write_config(ORCHESTRATOR_DOWN_CONFIG);
    let output = Command::cargo_bin("atelier")
        .unwrap()
        .current_dir(work.path())
        .args(["--doctor", "--strict", "--json", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "strict + errors must exit non-zero"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be clean JSON");
    assert_eq!(parsed["schema_version"], 1);
}

#[test]
fn atelier_version_prints_cargo_package_version() {
    Command::cargo_bin("atelier")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("atelier")
                .and(predicate::str::contains(env!("CARGO_PKG_VERSION"))),
        );
}

#[test]
fn init_config_tells_user_to_review_and_run_doctor() {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("multiagent.toml");

    Command::cargo_bin("atelier")
        .unwrap()
        .arg("--init-config")
        .arg("--config")
        .arg(&config_path)
        .assert()
        .success()
        .stdout(
            predicate::str::contains(format!("review {}", config_path.display())).and(
                predicate::str::contains("then run atelier --doctor to check runtime setup"),
            ),
        );
}

#[test]
fn print_config_includes_approval_floor() {
    // The resolved approval posture (mode + gray-area floor) must be visible in
    // --print-config so users can see and tune it (ADR-002).
    let home_dir = tempdir().unwrap();
    let config_path = home_dir.path().join("home.toml");
    std::fs::write(
        &config_path,
        "schema_version = 1\n[approval]\nfloor = \"enforce\"\n",
    )
    .unwrap();
    let work_dir = tempdir().unwrap();

    Command::cargo_bin("atelier")
        .unwrap()
        .current_dir(work_dir.path())
        .arg("--print-config")
        .arg("--config")
        .arg(&config_path)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("approval_mode")
                .and(predicate::str::contains("[approval]"))
                .and(predicate::str::contains("floor = \"enforce\"")),
        );
}

#[test]
fn print_config_includes_execution_graph_flag() {
    // The DAG enable flag must be visible in --print-config so users can see
    // and tune its posture (ADR-005). It surfaces automatically via the
    // [features] clone, defaulting to false and reflecting an explicit opt-in.
    let home_dir = tempdir().unwrap();
    let config_path = home_dir.path().join("home.toml");
    std::fs::write(
        &config_path,
        "schema_version = 1\n[features]\nexecution_graph = true\n",
    )
    .unwrap();
    let work_dir = tempdir().unwrap();

    Command::cargo_bin("atelier")
        .unwrap()
        .current_dir(work_dir.path())
        .arg("--print-config")
        .arg("--config")
        .arg(&config_path)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("[features]")
                .and(predicate::str::contains("execution_graph = true")),
        );
}

#[test]
fn help_lists_update_flag() {
    Command::cargo_bin("atelier")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--update"));
}

#[test]
fn native_update_points_to_npm_launcher() {
    Command::cargo_bin("atelier")
        .unwrap()
        .arg("--update")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("atelier --update is handled by the npm launcher").and(
                predicate::str::contains(
                    "npm install -g @matheusbbarni/atelier@latest --include=optional",
                ),
            ),
        );
}
