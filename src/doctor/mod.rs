use crate::config::{ApprovalMode, EffectiveConfig, FloorPolicy, RuntimeKind};
use crate::history::{list_session_event_paths, read_events_from_path, HistoryEvent};
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
    checks.push(approval_check(config));
    checks.push(governance_metrics_check(config));

    for runtime in config.runtimes.values() {
        let availability = check_runtime_availability(runtime).await;
        let (status, severity) = match availability.status {
            RuntimeAvailabilityStatus::Available => (DoctorStatus::Ok, DoctorSeverity::Info),
            RuntimeAvailabilityStatus::Unknown => (DoctorStatus::Warn, DoctorSeverity::Warning),
            RuntimeAvailabilityStatus::Unavailable => (DoctorStatus::Warn, DoctorSeverity::Warning),
        };
        let title = match runtime.kind {
            RuntimeKind::Codex => "Codex Runtime",
            RuntimeKind::Claude => "Claude Runtime",
            RuntimeKind::Cursor => "Cursor Runtime",
            RuntimeKind::Zai => "Z.ai Runtime",
            RuntimeKind::Fake => "Fake Runtime",
        };
        let protected_defaults = match runtime.kind {
            RuntimeKind::Claude => Some(crate::runtime::claude::protected_defaults_summary()),
            RuntimeKind::Cursor => Some(crate::runtime::cursor::protected_defaults_summary()),
            _ => None,
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
                "protected_defaults": protected_defaults,
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

fn approval_check(config: &EffectiveConfig) -> DoctorCheck {
    let mode = match config.approval_mode {
        ApprovalMode::Yolo => "yolo",
        ApprovalMode::Normal => "normal",
    };
    let floor = match config.approval.floor {
        FloorPolicy::Warn => "warn",
        FloorPolicy::Enforce => "enforce",
    };
    DoctorCheck {
        id: "config.approval".to_string(),
        title: "Approval Policy".to_string(),
        status: DoctorStatus::Ok,
        severity: DoctorSeverity::Info,
        message: format!(
            "approval_mode = {mode}; gray-area floor = {floor} (catastrophic core always prompts)"
        ),
        remediation: None,
        context: Some(serde_json::json!({
            "approval_mode": mode,
            "floor": floor,
        })),
    }
}

pub fn render_human(report: &DoctorReport) -> String {
    let mut output = String::new();
    output.push_str("atelier doctor\n");
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
    let history_root = config.working_directory.join(".atelier");
    match probe_history_writable(&history_root) {
        Ok(()) => DoctorCheck {
            id: "history.writable".to_string(),
            title: "Session History".to_string(),
            status: DoctorStatus::Ok,
            severity: DoctorSeverity::Info,
            message: ".atelier history directory can be created and written".to_string(),
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
    let history_root = config.working_directory.join(".atelier");
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

/// Intervention rate above this fraction of all runs raises the high-band alarm:
/// the gate is firing on too many runs.
const GOVERNANCE_INTERVENTION_HIGH_BAND: f64 = 0.5;

/// Aggregated governance signals derived from the local event log. All counts
/// are exact; only the trusted-outcome figure is a labeled proxy (ADR-005).
#[derive(Clone, Debug, Default, PartialEq)]
struct GovernanceMetrics {
    total_runs: usize,
    governed_runs: usize,
    early_aborts_fired: usize,
    accepts: usize,
    rejects: usize,
    write_intent_runs: usize,
    aborts_on_write_runs: usize,
    kept: usize,
    against: usize,
    reverts: usize,
}

impl GovernanceMetrics {
    /// Trusted Outcome Rate **proxy**: kept governed runs ÷ governed runs.
    /// `None` when no run has been governed yet.
    fn trusted_outcome_rate_proxy(&self) -> Option<f64> {
        (self.governed_runs > 0).then(|| self.kept as f64 / self.governed_runs as f64)
    }

    /// Early-abort catch rate: rejects ÷ early-aborts fired. `None` with no fires.
    fn early_abort_catch_rate(&self) -> Option<f64> {
        (self.early_aborts_fired > 0).then(|| self.rejects as f64 / self.early_aborts_fired as f64)
    }

    /// Intervention rate: early-aborts fired ÷ all runs. `0.0` with no runs.
    fn intervention_rate(&self) -> f64 {
        if self.total_runs == 0 {
            0.0
        } else {
            self.early_aborts_fired as f64 / self.total_runs as f64
        }
    }

    /// Gate precision: fired aborts that landed on genuine write-intent runs ÷
    /// all fired aborts. Read-only runs that never fired are excluded from the
    /// denominator. `None` with no fires.
    fn gate_precision(&self) -> Option<f64> {
        (self.early_aborts_fired > 0)
            .then(|| self.aborts_on_write_runs as f64 / self.early_aborts_fired as f64)
    }

    /// Dual-alarm: too many interventions (high band) OR none while runs went
    /// bad (near-zero with reverts present — the gate is not catching problems).
    fn raises_alarm(&self) -> bool {
        self.intervention_rate() > GOVERNANCE_INTERVENTION_HIGH_BAND
            || (self.early_aborts_fired == 0 && self.reverts > 0)
    }
}

fn decision_payload_requires_write(payload: &Value) -> bool {
    payload
        .get("required_capabilities")
        .and_then(Value::as_array)
        .is_some_and(|capabilities| {
            capabilities
                .iter()
                .filter_map(Value::as_str)
                .any(|capability| capability == "edit" || capability == "command")
        })
}

/// Aggregate governance signals from a flat, ordered event stream. Pure over the
/// events so it is unit-testable without touching the filesystem; safe (all
/// zeros) on an empty stream.
fn governance_metrics_from_events(events: &[HistoryEvent]) -> GovernanceMetrics {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct RunAgg {
        started: bool,
        first_decision_seen: bool,
        write_intent: bool,
        early_abort: bool,
        accepted: bool,
        rejected: bool,
        completed: bool,
        reverted: bool,
    }

    let mut runs: BTreeMap<String, RunAgg> = BTreeMap::new();
    let mut early_aborts_fired = 0usize;
    let mut accepts = 0usize;
    let mut rejects = 0usize;
    let mut reverts = 0usize;

    for event in events {
        let Some(run_id) = event.run_id.clone() else {
            continue;
        };
        let run = runs.entry(run_id).or_default();
        match event.kind.as_str() {
            "run_started" => run.started = true,
            // The complexity signal is read from the run's *first* decision.
            "orchestrator_decision" if !run.first_decision_seen => {
                run.first_decision_seen = true;
                run.write_intent = decision_payload_requires_write(&event.payload);
            }
            "governance_decision_requested" => {
                run.early_abort = true;
                early_aborts_fired += 1;
            }
            "governance_decision_resolved" => {
                match event.payload.get("outcome").and_then(Value::as_str) {
                    Some("accept") => {
                        run.accepted = true;
                        accepts += 1;
                    }
                    Some("reject") => {
                        run.rejected = true;
                        rejects += 1;
                    }
                    _ => {}
                }
            }
            "run_completed" => run.completed = true,
            "run_interrupted" | "run_failed" => {
                run.reverted = true;
                reverts += 1;
            }
            _ => {}
        }
    }

    let mut metrics = GovernanceMetrics {
        early_aborts_fired,
        accepts,
        rejects,
        reverts,
        ..Default::default()
    };
    for run in runs.values() {
        if run.started {
            metrics.total_runs += 1;
        }
        if run.write_intent {
            metrics.write_intent_runs += 1;
        }
        if run.early_abort {
            metrics.governed_runs += 1;
            if run.write_intent {
                metrics.aborts_on_write_runs += 1;
            }
            // Proxy classification (governed runs only): an early-abort reject,
            // or an abort/interrupt after an accept, count against; an accepted
            // run that completed clean counts as kept.
            if run.rejected || (run.accepted && run.reverted) {
                metrics.against += 1;
            } else if run.accepted && run.completed && !run.reverted {
                metrics.kept += 1;
            }
        }
    }
    metrics
}

fn read_all_session_events(root: &Path) -> Vec<HistoryEvent> {
    let Ok(paths) = list_session_event_paths(root) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for path in paths {
        // A single unreadable/corrupt session log degrades to "skip it" rather
        // than failing the whole doctor check.
        if let Ok(mut session_events) = read_events_from_path(&path) {
            events.append(&mut session_events);
        }
    }
    events
}

/// Local-only governance health: the Trusted Outcome Rate **proxy** plus the
/// exact calibration metrics (intervention rate with a dual-alarm band,
/// early-abort catch rate, gate precision). Reads only the `.atelier` event log;
/// no network, no telemetry (ADR-005).
fn governance_metrics_check(config: &EffectiveConfig) -> DoctorCheck {
    let root = config.working_directory.join(".atelier");
    let metrics = governance_metrics_from_events(&read_all_session_events(&root));

    let proxy = metrics.trusted_outcome_rate_proxy();
    let context = serde_json::json!({
        "trusted_outcome_rate_proxy": proxy,
        "trusted_outcome_rate_is_proxy": true,
        "trusted_outcome_rate_note": "Proxy derived from local events (kept vs reverted); not a measured revert signal.",
        "governed_runs": metrics.governed_runs,
        "kept": metrics.kept,
        "against": metrics.against,
        "early_aborts_fired": metrics.early_aborts_fired,
        "accepts": metrics.accepts,
        "rejects": metrics.rejects,
        "early_abort_catch_rate": metrics.early_abort_catch_rate(),
        "intervention_rate": metrics.intervention_rate(),
        "intervention_high_band": GOVERNANCE_INTERVENTION_HIGH_BAND,
        "gate_precision": metrics.gate_precision(),
        "write_intent_runs": metrics.write_intent_runs,
        "total_runs": metrics.total_runs,
        "reverts": metrics.reverts,
        "source": "local .atelier event log",
    });

    let (status, severity, message) = if metrics.early_aborts_fired == 0 && metrics.reverts == 0 {
        (
            DoctorStatus::Ok,
            DoctorSeverity::Info,
            "no governance activity recorded yet".to_string(),
        )
    } else if metrics.raises_alarm() {
        (
            DoctorStatus::Warn,
            DoctorSeverity::Warning,
            format!(
                "governance calibration alarm: intervention rate {:.2} over {} runs ({} aborts, {} reverts)",
                metrics.intervention_rate(),
                metrics.total_runs,
                metrics.early_aborts_fired,
                metrics.reverts,
            ),
        )
    } else {
        (
            DoctorStatus::Ok,
            DoctorSeverity::Info,
            format!(
                "trusted-outcome proxy {} over {} governed runs",
                proxy
                    .map(|rate| format!("{rate:.2}"))
                    .unwrap_or_else(|| "n/a".to_string()),
                metrics.governed_runs,
            ),
        )
    };

    DoctorCheck {
        id: "governance.metrics".to_string(),
        title: "Governance Metrics".to_string(),
        status,
        severity,
        message,
        remediation: None,
        context: Some(context),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_effective_config, ConfigLoadOptions};
    use serde_json::json;
    use tempfile::tempdir;

    fn gov_event(run_id: &str, kind: &str, payload: Value) -> HistoryEvent {
        HistoryEvent::new("session", Some(run_id.to_string()), None, kind, payload)
    }

    /// A complete kept governed run: write-intent, fired, accepted, completed.
    fn kept_run_events(run_id: &str) -> Vec<HistoryEvent> {
        vec![
            gov_event(run_id, "run_started", json!({})),
            gov_event(
                run_id,
                "orchestrator_decision",
                json!({ "required_capabilities": ["read", "edit"] }),
            ),
            gov_event(run_id, "governance_decision_requested", json!({})),
            gov_event(
                run_id,
                "governance_decision_resolved",
                json!({ "outcome": "accept" }),
            ),
            gov_event(run_id, "run_completed", json!({})),
        ]
    }

    #[test]
    fn proxy_counts_completed_accepted_run_as_kept() {
        let metrics = governance_metrics_from_events(&kept_run_events("run-1"));
        assert_eq!(metrics.governed_runs, 1);
        assert_eq!(metrics.kept, 1);
        assert_eq!(metrics.against, 0);
        assert_eq!(metrics.trusted_outcome_rate_proxy(), Some(1.0));
    }

    #[test]
    fn proxy_counts_early_abort_reject_against() {
        let events = vec![
            gov_event("run-1", "run_started", json!({})),
            gov_event(
                "run-1",
                "orchestrator_decision",
                json!({ "required_capabilities": ["edit"] }),
            ),
            gov_event("run-1", "governance_decision_requested", json!({})),
            gov_event(
                "run-1",
                "governance_decision_resolved",
                json!({ "outcome": "reject" }),
            ),
        ];
        let metrics = governance_metrics_from_events(&events);
        assert_eq!(metrics.governed_runs, 1);
        assert_eq!(metrics.kept, 0);
        assert_eq!(metrics.against, 1);
        assert_eq!(metrics.trusted_outcome_rate_proxy(), Some(0.0));
    }

    #[test]
    fn proxy_counts_abort_after_accept_against() {
        let events = vec![
            gov_event("run-1", "run_started", json!({})),
            gov_event(
                "run-1",
                "orchestrator_decision",
                json!({ "required_capabilities": ["edit"] }),
            ),
            gov_event("run-1", "governance_decision_requested", json!({})),
            gov_event(
                "run-1",
                "governance_decision_resolved",
                json!({ "outcome": "accept" }),
            ),
            gov_event("run-1", "run_interrupted", json!({})),
        ];
        let metrics = governance_metrics_from_events(&events);
        assert_eq!(metrics.kept, 0);
        assert_eq!(metrics.against, 1);
    }

    #[test]
    fn early_abort_catch_rate_is_rejects_over_fires() {
        let mut events = kept_run_events("run-keep"); // accept (fired)
        events.extend(vec![
            gov_event("run-rej", "run_started", json!({})),
            gov_event(
                "run-rej",
                "orchestrator_decision",
                json!({ "required_capabilities": ["edit"] }),
            ),
            gov_event("run-rej", "governance_decision_requested", json!({})),
            gov_event(
                "run-rej",
                "governance_decision_resolved",
                json!({ "outcome": "reject" }),
            ),
        ]);
        let metrics = governance_metrics_from_events(&events);
        assert_eq!(metrics.early_aborts_fired, 2);
        assert_eq!(metrics.rejects, 1);
        assert_eq!(metrics.early_abort_catch_rate(), Some(0.5));
    }

    #[test]
    fn gate_precision_excludes_read_only_runs() {
        // One write-intent run fired; one read-only run never fired. The
        // read-only run must not lower precision.
        let mut events = vec![
            gov_event("write-run", "run_started", json!({})),
            gov_event(
                "write-run",
                "orchestrator_decision",
                json!({ "required_capabilities": ["edit"] }),
            ),
            gov_event("write-run", "governance_decision_requested", json!({})),
            gov_event(
                "write-run",
                "governance_decision_resolved",
                json!({ "outcome": "accept" }),
            ),
            gov_event("write-run", "run_completed", json!({})),
        ];
        events.extend(vec![
            gov_event("read-run", "run_started", json!({})),
            gov_event(
                "read-run",
                "orchestrator_decision",
                json!({ "required_capabilities": ["read"] }),
            ),
            gov_event("read-run", "run_completed", json!({})),
        ]);
        let metrics = governance_metrics_from_events(&events);
        assert_eq!(metrics.early_aborts_fired, 1);
        assert_eq!(metrics.aborts_on_write_runs, 1);
        assert_eq!(metrics.write_intent_runs, 1);
        assert_eq!(metrics.gate_precision(), Some(1.0));
    }

    #[test]
    fn intervention_rate_high_band_raises_alarm() {
        // Two runs, both fired → intervention rate 1.0 (> high band).
        let mut events = kept_run_events("run-1");
        events.extend(kept_run_events("run-2"));
        let metrics = governance_metrics_from_events(&events);
        assert!(metrics.intervention_rate() > GOVERNANCE_INTERVENTION_HIGH_BAND);
        assert!(metrics.raises_alarm());
    }

    #[test]
    fn intervention_rate_near_zero_with_reverts_raises_alarm() {
        // A run that went bad with no governance pause at all.
        let events = vec![
            gov_event("run-1", "run_started", json!({})),
            gov_event(
                "run-1",
                "orchestrator_decision",
                json!({ "required_capabilities": ["edit"] }),
            ),
            gov_event("run-1", "run_failed", json!({})),
        ];
        let metrics = governance_metrics_from_events(&events);
        assert_eq!(metrics.early_aborts_fired, 0);
        assert!(metrics.reverts > 0);
        assert!(metrics.raises_alarm());
    }

    #[test]
    fn empty_event_log_is_safe() {
        let metrics = governance_metrics_from_events(&[]);
        assert_eq!(metrics, GovernanceMetrics::default());
        assert_eq!(metrics.trusted_outcome_rate_proxy(), None);
        assert_eq!(metrics.early_abort_catch_rate(), None);
        assert_eq!(metrics.gate_precision(), None);
        assert_eq!(metrics.intervention_rate(), 0.0);
        assert!(!metrics.raises_alarm());
    }

    #[tokio::test]
    async fn doctor_json_includes_governance_metrics_with_labeled_proxy() {
        let dir = tempdir().unwrap();
        // Seed a session log with one kept governed run.
        let sessions = dir.path().join(".atelier").join("sessions").join("s1");
        fs::create_dir_all(&sessions).unwrap();
        let body = kept_run_events("run-1")
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(sessions.join("events.jsonl"), body).unwrap();

        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: None,
        })
        .unwrap();
        let report = run_doctor(&config).await;

        let check = report
            .checks
            .iter()
            .find(|check| check.id == "governance.metrics")
            .expect("governance metrics check present");
        let context = check.context.as_ref().unwrap();
        assert_eq!(context["trusted_outcome_rate_is_proxy"], json!(true));
        assert_eq!(context["trusted_outcome_rate_proxy"], json!(1.0));
        assert_eq!(context["governed_runs"], json!(1));
        assert_eq!(context["kept"], json!(1));
        // Calibration figures present.
        assert!(context.get("intervention_rate").is_some());
        assert!(context.get("early_abort_catch_rate").is_some());
        assert!(context.get("gate_precision").is_some());

        // The proxy label survives JSON rendering.
        let rendered = render_json(&report).unwrap();
        assert!(rendered.contains("trusted_outcome_rate_is_proxy"));
    }

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
    async fn doctor_reports_approval_mode_and_floor() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            "schema_version = 1\napproval_mode = \"normal\"\n[approval]\nfloor = \"enforce\"\n",
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();
        let report = run_doctor(&config).await;
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "config.approval")
            .expect("approval check present");
        assert_eq!(check.status, DoctorStatus::Ok);
        assert!(
            check.message.contains("normal"),
            "message: {}",
            check.message
        );
        assert!(
            check.message.contains("enforce"),
            "message: {}",
            check.message
        );
        let context = check.context.as_ref().unwrap();
        assert_eq!(context["approval_mode"], "normal");
        assert_eq!(context["floor"], "enforce");
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
        assert!(dir.path().join(".atelier").is_dir());
        assert!(fs::read_dir(dir.path().join(".atelier"))
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
        let config_path = dir.path().join("atelier.toml");
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
        let config_path = dir.path().join("atelier.toml");
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

    #[cfg(unix)]
    #[tokio::test]
    async fn doctor_reports_codex_login_status() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("codex-status.sh");
        fs::write(
            &script_path,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "codex-cli 0.137.0"
  exit 0
fi
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  echo "Logged in using ChatGPT"
  exit 0
fi
echo "unexpected args: $@" >&2
exit 64
"#,
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();

        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            format!(
                r#"
[runtimes.codex]
type = "codex"
command = "{}"

[agents.explorer]
runtime = "codex"
model = "default"
"#,
                script_path.display()
            ),
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let report = run_doctor(&config).await;
        let json = render_json(&report).unwrap();
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "runtime.codex")
            .unwrap();

        assert_eq!(check.title, "Codex Runtime");
        assert_eq!(check.status, DoctorStatus::Ok);
        assert!(check.message.contains("codex-cli 0.137.0"));
        assert!(check.message.contains("Logged in using ChatGPT"));
        assert!(json.contains("\"runtime_type\": \"codex\""));
        assert!(!json.contains("CODEX_ACCESS_TOKEN"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn doctor_reports_claude_runtime_with_protected_default_summary() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("claude-status.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'claude 2.0.0'; exit 0; fi\nif [ \"$1\" = \"--help\" ]; then echo '{}'; exit 0; fi\necho unexpected >&2\nexit 64\n",
                crate::runtime::claude::REQUIRED_HELP_FLAGS.join(" ")
            ),
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();

        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            format!(
                r#"
[runtimes.claude]
command = "{}"
"#,
                script_path.display()
            ),
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let report = run_doctor(&config).await;
        let json = render_json(&report).unwrap();
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "runtime.claude")
            .unwrap();

        assert_eq!(check.title, "Claude Runtime");
        assert_eq!(check.status, DoctorStatus::Warn);
        assert!(check.message.contains("claude 2.0.0"));
        assert!(check.message.contains("tools disabled"));
        assert!(json.contains("\"runtime_type\": \"claude\""));
        assert!(json.contains("partial messages enabled"));
        assert!(!json.contains("stream-json --include-partial-messages"));
        assert!(!json.contains("CLAUDE_API_KEY"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn doctor_reports_cursor_runtime_with_status_and_protected_default_summary() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("cursor-status.sh");
        fs::write(
            &script_path,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "cursor-agent 1.0.0"
  exit 0
fi
if [ "$1" = "status" ]; then
  echo "Authenticated as dev@example.com"
  exit 0
fi
echo "unexpected args: $@" >&2
exit 64
"#,
        )
        .unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700)).unwrap();

        let config_path = dir.path().join("atelier.toml");
        fs::write(
            &config_path,
            format!(
                r#"
[runtimes.cursor]
command = "{}"
"#,
                script_path.display()
            ),
        )
        .unwrap();
        let config = load_effective_config(ConfigLoadOptions {
            working_directory: dir.path().to_path_buf(),
            config_path: Some(config_path),
        })
        .unwrap();

        let report = run_doctor(&config).await;
        let json = render_json(&report).unwrap();
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "runtime.cursor")
            .unwrap();

        assert_eq!(check.title, "Cursor Runtime");
        assert_eq!(check.status, DoctorStatus::Ok);
        assert!(check.message.contains("cursor-agent 1.0.0"));
        assert!(check.message.contains("Authenticated"));
        assert!(json.contains("\"runtime_type\": \"cursor\""));
        assert!(json.contains("stream-json enabled"));
        assert!(json.contains("Cursor tools must not bypass Harness Actions"));
        assert!(!json.contains("CURSOR_API_KEY"));
    }
}
