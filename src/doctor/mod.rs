use crate::config::{EffectiveConfig, RuntimeKind};
use crate::runtime::{check_runtime_availability, RuntimeAvailabilityStatus};
use anyhow::Result;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Ok,
    Warn,
    Error,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub id: String,
    pub title: String,
    pub status: DoctorStatus,
    pub severity: DoctorSeverity,
    pub message: String,
    pub remediation: Option<String>,
    pub context: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub working_directory: String,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn has_errors(&self) -> bool {
        self.checks
            .iter()
            .any(|check| matches!(check.status, DoctorStatus::Error))
    }
}

pub async fn run_doctor(config: &EffectiveConfig) -> DoctorReport {
    let mut checks = Vec::new();
    checks.push(working_directory_check(config));
    checks.push(history_writability_check(config));
    checks.extend(permission_checks(config));
    checks.push(selected_preset_check(config));
    checks.push(prompt_files_check(config));
    checks.push(model_fallback_check(config));
    checks.push(tool_access_check(config));

    for runtime in config.runtimes.values() {
        let availability = check_runtime_availability(runtime).await;
        let (status, severity) = match availability.status {
            RuntimeAvailabilityStatus::Available => (DoctorStatus::Ok, DoctorSeverity::Info),
            RuntimeAvailabilityStatus::Unknown => (DoctorStatus::Warn, DoctorSeverity::Warning),
            RuntimeAvailabilityStatus::Unavailable => (DoctorStatus::Warn, DoctorSeverity::Warning),
        };
        let title = match runtime.kind {
            RuntimeKind::Codex => "Codex Runtime",
            RuntimeKind::Zai => "Z.ai Runtime",
            RuntimeKind::Fake => "Fake Runtime",
        };
        checks.push(DoctorCheck {
            id: format!("runtime.{}", runtime.id),
            title: title.to_string(),
            status,
            severity,
            message: availability.message,
            remediation: availability.remediation,
            context: Some(serde_json::json!({
                "runtime_id": runtime.id,
                "runtime_type": runtime.kind,
                "api_key_env": runtime.api_key_env,
                "command": runtime.command,
            })),
        });
    }

    DoctorReport {
        schema_version: 1,
        generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        working_directory: config.working_directory.display().to_string(),
        checks,
    }
}

fn selected_preset_check(config: &EffectiveConfig) -> DoctorCheck {
    match &config.active_preset {
        Some(preset) => DoctorCheck {
            id: "config.preset".to_string(),
            title: "Selected Preset".to_string(),
            status: DoctorStatus::Ok,
            severity: DoctorSeverity::Info,
            message: format!("active preset: {preset}"),
            remediation: None,
            context: Some(serde_json::json!({ "preset": preset })),
        },
        None => DoctorCheck {
            id: "config.preset".to_string(),
            title: "Selected Preset".to_string(),
            status: DoctorStatus::Skipped,
            severity: DoctorSeverity::Info,
            message: "no preset selected".to_string(),
            remediation: None,
            context: None,
        },
    }
}

fn prompt_files_check(config: &EffectiveConfig) -> DoctorCheck {
    let agents = config
        .agents
        .values()
        .filter_map(|agent| {
            let metadata = &agent.prompt_metadata;
            if metadata.is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "agent": agent.id.clone(),
                "instructions_file": metadata.instructions_file.clone(),
                "instructions_append_file": metadata.instructions_append_file.clone(),
                "orchestrator_description_file": metadata.orchestrator_description_file.clone(),
            }))
        })
        .collect::<Vec<_>>();
    DoctorCheck {
        id: "config.prompt_files".to_string(),
        title: "Prompt Files".to_string(),
        status: DoctorStatus::Ok,
        severity: DoctorSeverity::Info,
        message: if agents.is_empty() {
            "no prompt files configured".to_string()
        } else {
            format!("{} agent prompt file configuration(s) active", agents.len())
        },
        remediation: None,
        context: Some(serde_json::json!({ "agents": agents })),
    }
}

fn model_fallback_check(config: &EffectiveConfig) -> DoctorCheck {
    let missing = config
        .agents
        .values()
        .filter(|agent| agent.enabled && agent.model_fallbacks.is_empty())
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        DoctorCheck {
            id: "config.model_fallbacks".to_string(),
            title: "Model Fallbacks".to_string(),
            status: DoctorStatus::Ok,
            severity: DoctorSeverity::Info,
            message: "enabled agents have fallback models configured".to_string(),
            remediation: None,
            context: None,
        }
    } else {
        DoctorCheck {
            id: "config.model_fallbacks".to_string(),
            title: "Model Fallbacks".to_string(),
            status: DoctorStatus::Warn,
            severity: DoctorSeverity::Warning,
            message: format!(
                "enabled agents without model_fallbacks: {}",
                missing.join(", ")
            ),
            remediation: Some(
                "Set model_fallbacks on agents that should retry provider failures.".to_string(),
            ),
            context: Some(serde_json::json!({ "agents": missing })),
        }
    }
}

fn tool_access_check(config: &EffectiveConfig) -> DoctorCheck {
    let agents = config
        .agents
        .values()
        .map(|agent| {
            serde_json::json!({
                "agent": agent.id.clone(),
                "enabled": agent.enabled,
                "tools": agent.effective_tools(),
            })
        })
        .collect::<Vec<_>>();
    DoctorCheck {
        id: "config.tool_access".to_string(),
        title: "Tool Access".to_string(),
        status: DoctorStatus::Ok,
        severity: DoctorSeverity::Info,
        message: "effective tool access computed from capabilities and tool allowlists".to_string(),
        remediation: None,
        context: Some(serde_json::json!({ "agents": agents })),
    }
}

pub fn render_human(report: &DoctorReport) -> String {
    let mut output = String::new();
    output.push_str("multiagent doctor\n");
    output.push_str(&format!(
        "working_directory: {}\n\n",
        report.working_directory
    ));
    for check in &report.checks {
        output.push_str(&format!(
            "[{:?}] {}: {}\n",
            check.status, check.title, check.message
        ));
        if let Some(remediation) = &check.remediation {
            output.push_str(&format!("  remediation: {remediation}\n"));
        }
    }
    output
}

pub fn render_json(report: &DoctorReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

fn working_directory_check(config: &EffectiveConfig) -> DoctorCheck {
    match fs::metadata(&config.working_directory) {
        Ok(metadata) if metadata.is_dir() => DoctorCheck {
            id: "working_directory.exists".to_string(),
            title: "Working Directory".to_string(),
            status: DoctorStatus::Ok,
            severity: DoctorSeverity::Info,
            message: "working directory exists and is a directory".to_string(),
            remediation: None,
            context: None,
        },
        Ok(_) => DoctorCheck {
            id: "working_directory.exists".to_string(),
            title: "Working Directory".to_string(),
            status: DoctorStatus::Error,
            severity: DoctorSeverity::Error,
            message: "working directory is not a directory".to_string(),
            remediation: Some("Choose a directory with --cwd.".to_string()),
            context: None,
        },
        Err(error) => DoctorCheck {
            id: "working_directory.exists".to_string(),
            title: "Working Directory".to_string(),
            status: DoctorStatus::Error,
            severity: DoctorSeverity::Error,
            message: error.to_string(),
            remediation: Some("Choose a readable directory with --cwd.".to_string()),
            context: None,
        },
    }
}

fn history_writability_check(config: &EffectiveConfig) -> DoctorCheck {
    let history_root = config.working_directory.join(".multiagent");
    match probe_history_writable(&history_root) {
        Ok(()) => DoctorCheck {
            id: "history.writable".to_string(),
            title: "Session History".to_string(),
            status: DoctorStatus::Ok,
            severity: DoctorSeverity::Info,
            message: ".multiagent history directory can be created and written".to_string(),
            remediation: None,
            context: Some(serde_json::json!({ "path": history_root })),
        },
        Err(error) => DoctorCheck {
            id: "history.writable".to_string(),
            title: "Session History".to_string(),
            status: DoctorStatus::Error,
            severity: DoctorSeverity::Error,
            message: error.to_string(),
            remediation: Some("Check working-directory permissions.".to_string()),
            context: Some(serde_json::json!({ "path": history_root })),
        },
    }
}

fn probe_history_writable(history_root: &Path) -> std::io::Result<()> {
    let existed = history_root.exists();
    fs::create_dir_all(history_root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed {
            fs::set_permissions(history_root, fs::Permissions::from_mode(0o700))?;
        }
    }

    let probe_path = history_root.join(format!(".doctor-write-check-{}", crate::ids::new_id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&probe_path, fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(b"ok\n")?;
    file.flush()?;
    drop(file);
    fs::remove_file(probe_path)?;
    Ok(())
}

fn permission_checks(config: &EffectiveConfig) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    for (index, path) in config.config_sources.iter().enumerate() {
        checks.push(permission_check(
            &format!("config.permissions.{index}"),
            "Config Permissions",
            path,
        ));
    }
    let history_root = config.working_directory.join(".multiagent");
    if history_root.exists() {
        checks.push(permission_check(
            "history.permissions",
            "History Permissions",
            &history_root,
        ));
    }
    checks
}

fn permission_check(id: &str, title: &str, path: &std::path::Path) -> DoctorCheck {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(path) {
            Ok(metadata) => {
                let mode = metadata.permissions().mode() & 0o777;
                if mode & 0o077 == 0 {
                    DoctorCheck {
                        id: id.to_string(),
                        title: title.to_string(),
                        status: DoctorStatus::Ok,
                        severity: DoctorSeverity::Info,
                        message: format!("permissions are private ({mode:o})"),
                        remediation: None,
                        context: Some(
                            serde_json::json!({ "path": path, "mode": format!("{mode:o}") }),
                        ),
                    }
                } else {
                    DoctorCheck {
                        id: id.to_string(),
                        title: title.to_string(),
                        status: DoctorStatus::Warn,
                        severity: DoctorSeverity::Warning,
                        message: format!("permissions are broader than recommended ({mode:o})"),
                        remediation: Some(
                            "Consider restricting permissions to the current user.".to_string(),
                        ),
                        context: Some(
                            serde_json::json!({ "path": path, "mode": format!("{mode:o}") }),
                        ),
                    }
                }
            }
            Err(error) => DoctorCheck {
                id: id.to_string(),
                title: title.to_string(),
                status: DoctorStatus::Warn,
                severity: DoctorSeverity::Warning,
                message: error.to_string(),
                remediation: None,
                context: Some(serde_json::json!({ "path": path })),
            },
        }
    }

    #[cfg(not(unix))]
    {
        DoctorCheck {
            id: id.to_string(),
            title: title.to_string(),
            status: DoctorStatus::Skipped,
            severity: DoctorSeverity::Info,
            message: "permission check is Unix-specific".to_string(),
            remediation: None,
            context: Some(serde_json::json!({ "path": path })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use tempfile::tempdir;

    #[tokio::test]
    async fn doctor_json_is_structured() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let report = run_doctor(&config).await;
        let json = render_json(&report).unwrap();
        assert!(json.contains("\"schema_version\": 1"));
        assert!(report
            .checks
            .iter()
            .any(|check| check.id == "runtime.codex"));
    }

    #[tokio::test]
    async fn doctor_probes_history_writability() {
        let dir = tempdir().unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let report = run_doctor(&config).await;
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "history.writable")
            .unwrap();
        assert_eq!(check.status, DoctorStatus::Ok);
        assert!(dir.path().join(".multiagent").is_dir());
        assert!(fs::read_dir(dir.path().join(".multiagent"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".doctor-write-check-")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn doctor_warns_for_broad_config_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let config_path = dir.path().join("multiagent.toml");
        fs::write(&config_path, "schema_version = 1\n").unwrap();
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o644)).unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let report = run_doctor(&config).await;
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "config.permissions.0")
            .unwrap();

        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.message.contains("broader than recommended"));
    }

    #[tokio::test]
    async fn doctor_reports_preset_prompt_paths_fallbacks_and_tools() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("agents")).unwrap();
        fs::write(dir.path().join("agents/explorer.md"), "secret prompt").unwrap();
        let config_path = dir.path().join("multiagent.toml");
        fs::write(
            &config_path,
            r#"
preset = "research"

[runtimes.fake]
type = "fake"

[presets.research.agents.explorer]
runtime = "fake"
model_fallbacks = ["fallback-model"]
tools = ["read_file"]
instructions_file = "agents/explorer.md"
"#,
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let report = run_doctor(&config).await;
        let json = render_json(&report).unwrap();

        assert!(report
            .checks
            .iter()
            .any(|check| check.id == "config.preset" && check.message.contains("research")));
        assert!(report
            .checks
            .iter()
            .any(|check| check.id == "config.prompt_files"));
        assert!(report.checks.iter().any(
            |check| check.id == "config.model_fallbacks" && check.status == DoctorStatus::Warn
        ));
        assert!(report
            .checks
            .iter()
            .any(|check| check.id == "config.tool_access"));
        assert!(json.contains("agents/explorer.md"));
        assert!(!json.contains("secret prompt"));
    }
}
