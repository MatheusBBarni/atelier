use assert_cmd::Command;
use predicates::prelude::*;

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
