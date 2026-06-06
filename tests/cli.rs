use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

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
