//! `atelier --events follow` — a standalone reader that tails the active
//! session's on-disk event log and prints the normalized [`HookPayload`] for
//! each public event, reusing the same [`normalize`](super::normalize) the live
//! tap uses (ADR-004). It is a faithful dry-run preview of exactly what a hook
//! receives on stdin: non-public events are skipped, the actor is reconstructed
//! by folding the session's own run/step events, and the JSON is redacted just
//! like the dispatcher redacts before writing to a hook.

use super::{normalize, ActorCtx, HookPayload, PayloadDetail};
use crate::config::EffectiveConfig;
use crate::history::{list_session_event_paths, read_events_from_path, HistoryEvent};
use crate::runtime::redact_sensitive_text;
use anyhow::Result;
use clap::ValueEnum;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The `--events <mode>` sub-mode. Only `follow` in V1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum EventsCommand {
    /// Tail the active session and print each public event's normalized payload.
    Follow,
}

/// Poll cadence for the live tail.
const FOLLOW_POLL: Duration = Duration::from_millis(250);

/// Build the `agent id → runtime` map from the effective config — the source of
/// `actor.runtime`, since the on-disk events do not carry it directly.
fn agent_runtime_map(config: &EffectiveConfig) -> BTreeMap<String, String> {
    config
        .agents
        .iter()
        .map(|(id, profile)| (id.clone(), profile.runtime.clone()))
        .collect()
}

/// Project a session's events into the normalized payloads a hook would see, in
/// order, skipping non-public kinds. Pure — reused by the live tail and tests,
/// and identical to what the live tap produces for the same events.
///
/// Folds `step_id → agent` from events carrying an `agent` field (e.g.
/// `agent_step_started`), then resolves each event's `actor` (agent + its
/// configured runtime). Events emitted before any agent step (e.g. `run_started`)
/// resolve to a null/orchestrator actor.
pub fn project_session_payloads(
    events: &[HistoryEvent],
    agent_runtimes: &BTreeMap<String, String>,
) -> Vec<HookPayload> {
    let mut step_agent: BTreeMap<String, String> = BTreeMap::new();
    let mut payloads = Vec::new();
    for event in events {
        // Fold this event's (step_id, agent) before resolving, so an
        // `agent_step_started` resolves its own actor too.
        if let (Some(step_id), Some(agent)) = (
            event.step_id.as_deref(),
            event.payload.get("agent").and_then(Value::as_str),
        ) {
            step_agent.insert(step_id.to_string(), agent.to_string());
        }
        let actor = resolve_follow_actor(event, &step_agent, agent_runtimes);
        if let Some(payload) = normalize(event, actor, PayloadDetail::Metadata) {
            payloads.push(payload);
        }
    }
    payloads
}

fn resolve_follow_actor(
    event: &HistoryEvent,
    step_agent: &BTreeMap<String, String>,
    agent_runtimes: &BTreeMap<String, String>,
) -> ActorCtx {
    let Some(agent) = event
        .step_id
        .as_deref()
        .and_then(|step_id| step_agent.get(step_id))
        .cloned()
    else {
        return ActorCtx::default();
    };
    let runtime = agent_runtimes.get(&agent).cloned();
    ActorCtx {
        agent: Some(agent),
        runtime,
    }
}

/// Render a payload as a single redacted JSON line — matching what a hook would
/// receive on stdin (the dispatcher redacts before writing).
pub fn render_payload_line(payload: &HookPayload) -> String {
    let json = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    redact_sensitive_text(&json)
}

/// The most-recently-modified session `events.jsonl` under the workspace, or
/// `None` when there is no session history yet. Session ids are random (not
/// time-ordered), so the active session is the one whose log was written last.
pub fn latest_session_events_path(working_directory: &Path) -> Result<Option<PathBuf>> {
    let root = working_directory.join(".atelier");
    let paths = list_session_event_paths(&root)?;
    Ok(paths
        .into_iter()
        .max_by_key(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok()))
}

/// `atelier --events follow`: tail the active session, printing each new public
/// event's normalized payload until Ctrl-C. Re-reads the append-only log on a
/// short poll and prints only payloads beyond the last-emitted index.
pub async fn follow_events(config: &EffectiveConfig) -> Result<()> {
    let Some(path) = latest_session_events_path(&config.working_directory)? else {
        eprintln!(
            "no atelier session history found under {}",
            config.working_directory.display()
        );
        return Ok(());
    };
    let agent_runtimes = agent_runtime_map(config);
    let mut emitted = 0usize;
    // Tail until interrupted: Ctrl-C (SIGINT) terminates the process, as with
    // `tail -f`. stdout is flushed every poll, so nothing is lost. We avoid
    // tokio's `signal` feature (and its transitive deps) for this read-only
    // preview tool — default SIGINT termination is the right UX here.
    loop {
        // A stdout write failure (e.g. the reader closed the pipe) ends follow
        // mode instead of spinning forever, matching `tail -f | head`.
        emitted = emit_new_payloads(&path, &agent_runtimes, emitted)?;
        tokio::time::sleep(FOLLOW_POLL).await;
    }
}

/// Read the log, project, and print payloads beyond `already_emitted`; return the
/// new emitted count. A transiently unreadable log (mid-write) is tolerated by
/// keeping the prior count and retrying next poll.
fn emit_new_payloads(
    path: &Path,
    agent_runtimes: &BTreeMap<String, String>,
    already_emitted: usize,
) -> Result<usize> {
    // A transiently unreadable log (mid-write) is tolerated — keep the prior
    // count and retry next poll. A stdout write failure, however, propagates so
    // the caller can stop the loop rather than spin on a broken pipe.
    let Ok(events) = read_events_from_path(path) else {
        return Ok(already_emitted);
    };
    let payloads = project_session_payloads(&events, agent_runtimes);
    let mut stdout = io::stdout().lock();
    for payload in payloads.iter().skip(already_emitted) {
        writeln!(stdout, "{}", render_payload_line(payload))?;
    }
    stdout.flush()?;
    Ok(payloads.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        kind: &str,
        run_id: Option<&str>,
        step_id: Option<&str>,
        payload: Value,
    ) -> HistoryEvent {
        let mut event = HistoryEvent::new(
            "sess-1",
            run_id.map(str::to_string),
            step_id.map(str::to_string),
            kind,
            payload,
        );
        event.timestamp = "2026-06-17T00:00:00.000Z".to_string();
        event
    }

    fn runtimes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(agent, runtime)| (agent.to_string(), runtime.to_string()))
            .collect()
    }

    #[test]
    fn actor_fold_resolves_runtime_for_a_later_event_at_the_same_step() {
        let events = [
            event(
                "agent_step_started",
                Some("run-1"),
                Some("s1"),
                serde_json::json!({ "agent": "explorer" }),
            ),
            event(
                "action_requested",
                Some("run-1"),
                Some("s1"),
                serde_json::json!({ "action_id": "a1", "kind": "run_command", "params": { "command": "ls" } }),
            ),
        ];
        let payloads = project_session_payloads(&events, &runtimes(&[("explorer", "codex")]));
        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0].event, "step_started");
        // The later action_requested inherits the step's agent + configured runtime.
        assert_eq!(payloads[1].event, "action_requested");
        assert_eq!(payloads[1].actor.agent.as_deref(), Some("explorer"));
        assert_eq!(payloads[1].actor.runtime.as_deref(), Some("codex"));
    }

    #[test]
    fn runtime_stream_delta_line_is_skipped() {
        let payloads = project_session_payloads(
            &[event(
                "runtime_stream_delta",
                Some("run-1"),
                Some("s1"),
                serde_json::json!({ "agent": "explorer", "content": "x" }),
            )],
            &runtimes(&[("explorer", "codex")]),
        );
        assert!(payloads.is_empty());
    }

    #[test]
    fn pre_agent_run_started_has_null_actor() {
        let payloads = project_session_payloads(
            &[event(
                "run_started",
                Some("run-1"),
                None,
                serde_json::json!({ "prompt": "hi" }),
            )],
            &BTreeMap::new(),
        );
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].event, "run_started");
        assert!(payloads[0].actor.agent.is_none());
        assert!(payloads[0].actor.runtime.is_none());
    }

    #[test]
    fn follow_over_fixture_log_emits_expected_normalized_lines() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let session = dir.path().join(".atelier/sessions/sess-1");
        fs::create_dir_all(&session).unwrap();
        let log: String = [
            event(
                "run_started",
                Some("run-1"),
                None,
                serde_json::json!({ "prompt": "go" }),
            ),
            event(
                "agent_step_started",
                Some("run-1"),
                Some("s1"),
                serde_json::json!({ "agent": "explorer" }),
            ),
            // Non-public: must be skipped in the stream.
            event(
                "runtime_stream_delta",
                Some("run-1"),
                Some("s1"),
                serde_json::json!({ "agent": "explorer", "content": "x" }),
            ),
            event(
                "run_completed",
                Some("run-1"),
                None,
                serde_json::json!({ "summary": "done" }),
            ),
        ]
        .iter()
        .map(|event| format!("{}\n", serde_json::to_string(event).unwrap()))
        .collect();
        fs::write(session.join("events.jsonl"), log).unwrap();

        let path = latest_session_events_path(dir.path()).unwrap().unwrap();
        let parsed = read_events_from_path(&path).unwrap();
        let lines: Vec<String> =
            project_session_payloads(&parsed, &runtimes(&[("explorer", "codex")]))
                .iter()
                .map(render_payload_line)
                .collect();

        // run_started, step_started, run_completed — the stream-delta is dropped.
        assert_eq!(lines.len(), 3, "lines: {lines:?}");
        assert!(lines[0].contains("\"event\":\"run_started\""));
        assert!(lines[1].contains("\"event\":\"step_started\""));
        assert!(lines[1].contains("\"runtime\":\"codex\""));
        assert!(lines[2].contains("\"event\":\"run_completed\""));
        assert!(lines[2].contains("\"outcome\":\"success\""));
    }
}
