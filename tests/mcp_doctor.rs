//! Integration tests for the MCP doctor checks (task_10): run `run_doctor`
//! against a config whose MCP server points at the in-repo fake stdio server.

use std::path::Path;

use multiagent::config::{load_effective_config, ConfigLoadOptions, EffectiveConfig};
use multiagent::doctor::{run_doctor, DoctorStatus};
use tempfile::tempdir;

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake-mcp-server");

fn doctor_config(dir: &Path, server_env_line: &str) -> EffectiveConfig {
    let config_path = dir.join("home.toml");
    std::fs::write(
        &config_path,
        format!(
            "[features]\nmcp_enabled = true\n\
             [runtimes.fake]\ntype = \"fake\"\n\
             [agents.orchestrator]\nruntime = \"fake\"\ncapabilities = [\"plan\"]\ninstructions = \"o\"\n\
             [mcp.servers.fake]\ntransport = \"stdio\"\ncommand = \"{FAKE_SERVER}\"\n{server_env_line}"
        ),
    )
    .unwrap();
    load_effective_config(ConfigLoadOptions {
        working_directory: dir.to_path_buf(),
        config_path: Some(config_path),
    })
    .unwrap()
}

#[tokio::test]
async fn doctor_reports_reachable_fake_server_ok() {
    let dir = tempdir().unwrap();
    let config = doctor_config(dir.path(), "");
    let report = run_doctor(&config).await;
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "mcp_server.fake")
        .expect("mcp_server.fake check present");
    assert_eq!(check.status, DoctorStatus::Ok);
}

#[tokio::test]
async fn doctor_treats_stderr_logging_server_as_ok() {
    // A healthy server that writes a startup line to stderr must NOT be a false
    // failure — only the stdout initialize handshake matters.
    let dir = tempdir().unwrap();
    let config = doctor_config(dir.path(), "env = { FAKE_MCP_STDERR_NOISE = \"1\" }\n");
    let report = run_doctor(&config).await;
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "mcp_server.fake")
        .expect("mcp_server.fake check present");
    assert_eq!(
        check.status,
        DoctorStatus::Ok,
        "a server logging to stderr but handshaking fine must be Ok"
    );
}

#[tokio::test]
async fn doctor_json_has_server_check_parity_matrix_and_metric() {
    let dir = tempdir().unwrap();
    let config = doctor_config(dir.path(), "");
    let report = run_doctor(&config).await;
    let json = serde_json::to_value(&report).unwrap();
    let checks = json["checks"].as_array().unwrap();

    assert!(
        checks.iter().any(|check| check["id"] == "mcp_server.fake"),
        "report should include the per-server check"
    );
    let parity = checks
        .iter()
        .find(|check| check["id"] == "mcp_parity")
        .expect("mcp_parity check present");
    assert!(
        parity["context"]["matrix"].is_object(),
        "parity context should carry a runtimes×servers matrix"
    );
    assert!(
        parity["context"]["trusted_completion_count"].is_number(),
        "parity context should carry the local trusted-completion metric"
    );
}
