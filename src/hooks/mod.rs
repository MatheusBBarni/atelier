//! Lifecycle hooks — the normalized, versioned payload contract plus the pure
//! [`normalize`] projection shared by the live event tap (task_05) and the
//! `atelier --events follow` reader (task_07).
//!
//! This module is the *contract foundation* for the hooks feature (ADR-001,
//! ADR-004). It deliberately contains no IO, no dispatch, and no config-merge —
//! those arrive in later tasks (config ladder — task_02, notifier — task_03,
//! dispatcher — task_04). Everything here is pure data plus one pure function,
//! so the public payload shape stays decoupled from atelier's refactorable
//! internal [`HistoryEvent`] kinds.

use crate::history::HistoryEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version of the [`HookPayload`] contract. Bumped only on a
/// backward-incompatible change to the normalized shape (ADR-004); additive,
/// optional fields do not require a bump.
pub const HOOK_PAYLOAD_SCHEMA_VERSION: u32 = 1;

/// The public event vocabulary (ADR-004): stable public names paired with the
/// internal [`HistoryEvent`] `kind` each maps to. This is the *contract surface*
/// — public names are decoupled from atelier's internal kinds so the latter
/// stay free to evolve. Any kind absent from this table (including
/// `hook_started`, `hook_completed`, and `runtime_stream_delta`) is outside the
/// vocabulary and so never triggers a hook.
pub const PUBLIC_EVENT_VOCABULARY: &[(&str, &str)] = &[
    ("run_started", "run_started"),
    ("step_started", "agent_step_started"),
    ("action_requested", "action_requested"),
    ("approval_required", "approval_requested"),
    ("clarification_required", "clarification_requested"),
    ("file_edited", "file_edit_applied"),
    ("run_completed", "run_completed"),
    ("run_failed", "run_failed"),
    ("run_limit_reached", "run_limit_reached"),
    ("run_interrupted", "run_interrupted"),
];

/// Resolve an internal [`HistoryEvent`] `kind` to its stable public event name,
/// or `None` when the kind is outside the public vocabulary.
pub fn public_name_for_kind(kind: &str) -> Option<&'static str> {
    PUBLIC_EVENT_VOCABULARY
        .iter()
        .find(|(_, internal)| *internal == kind)
        .map(|(public, _)| *public)
}

/// Resolve a public event name to the internal kind it maps to, or `None` for
/// an unknown public name.
pub fn internal_kind_for_public(public_name: &str) -> Option<&'static str> {
    PUBLIC_EVENT_VOCABULARY
        .iter()
        .find(|(public, _)| *public == public_name)
        .map(|(_, internal)| *internal)
}

/// The uniform actor across runtimes, as it appears in a [`HookPayload`]. Both
/// fields are `None` for orchestrator-level events emitted before any agent
/// step is active.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Actor {
    pub agent: Option<String>,
    pub runtime: Option<String>,
}

/// Actor context handed to [`normalize`]. The live tap (task_05) populates it
/// from `active_step.agent` + the agent profile's runtime; the follow reader
/// (task_07) reconstructs it by folding the session's run/step events. Both
/// fields are `None` for orchestrator-level events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorCtx {
    pub agent: Option<String>,
    pub runtime: Option<String>,
}

/// How much of an event a handler receives. Metadata-only is the default
/// (ADR-001 data-minimization); `Full` is opt-in per handler and additionally
/// carries the raw event body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadDetail {
    #[default]
    Metadata,
    Full,
}

/// The normalized, versioned payload a hook receives on stdin and that
/// `atelier --events follow` prints (ADR-004). Standards-aligned in spirit
/// (who / what / when / outcome + correlation) but atelier's own stable JSON,
/// not bound to an evolving upstream spec.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HookPayload {
    /// Contract version; see [`HOOK_PAYLOAD_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Public event name (ADR-004 vocabulary).
    pub event: String,
    /// Event time, RFC3339 (copied verbatim from the source event).
    pub time: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    /// Uniform actor across runtimes.
    pub actor: Actor,
    /// What the event acts on — e.g. a file path or command. Omitted when the
    /// event has no natural target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Decision/terminal outcome — `success` | `error` | `limit_reached` |
    /// `interrupted`. Omitted for in-flight events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// The raw event body, included only when the handler opts into
    /// [`PayloadDetail::Full`]; metadata-only handlers omit it entirely.
    /// Redaction (`redact_sensitive_text`) is applied downstream by the
    /// dispatcher (task_04) / follow reader (task_07) before this reaches any
    /// sink — [`normalize`] itself stays pure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

/// Effective hooks configuration after the config ladder (filled by task_02).
#[derive(Clone, Debug, Default)]
pub struct HooksConfig {
    pub handlers: Vec<HookHandler>,
    /// Optional notifier command for terminals/tmux that strip OSC (ADR-005).
    pub notify_fallback_command: Option<String>,
}

/// One configured handler: the public events it fires on (exact match), the
/// single action to run, and how much payload it receives.
#[derive(Clone, Debug)]
pub struct HookHandler {
    pub on: Vec<String>,
    pub action: HookAction,
    pub payload: PayloadDetail,
}

impl HookHandler {
    /// Whether this handler fires for the given public event name (exact match;
    /// glob/regex are deferred per ADR-004).
    pub fn matches(&self, public_event: &str) -> bool {
        self.on.iter().any(|name| name == public_event)
    }
}

/// Exactly one action per handler (ADR-004): the built-in notifier or a shell
/// command.
#[derive(Clone, Debug)]
pub enum HookAction {
    Notify(NotifyConfig),
    Command(String),
}

impl HookAction {
    /// Stable action-kind label recorded in hook-lifecycle events (`notify` /
    /// `command`), consumed by the dispatcher (task_04) and projection
    /// (task_06).
    pub fn kind_str(&self) -> &'static str {
        match self {
            HookAction::Notify(_) => "notify",
            HookAction::Command(_) => "command",
        }
    }
}

/// Optional title/body templating for the built-in notifier (ADR-005). A bare
/// `notify = true` yields the default (`None` / `None`); a `[hooks.notify]`
/// table can override either field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotifyConfig {
    pub title: Option<String>,
    pub body: Option<String>,
}

/// Payload recorded on `hook_started` / `hook_completed` history events for
/// transcript transparency (ADR-001). Written by the dispatcher (task_04) and
/// read back by the chat projection (task_06). These events are outside the
/// public vocabulary, so they never re-trigger hooks (structural recursion
/// guard).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HookLifecyclePayload {
    /// Index of the handler in the effective [`HooksConfig::handlers`] list.
    pub handler_index: usize,
    /// Action kind: `notify` | `command`.
    pub action: String,
    /// The public event name that triggered this hook.
    pub event: String,
    /// Completion status (`ok` | `error` | `timeout`); `None` on `hook_started`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Wall-clock duration; `None` on `hook_started`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Subprocess exit code for a `command` action, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Redacted stderr excerpt from a failed command hook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_excerpt: Option<String>,
}

/// Project an internal [`HistoryEvent`] into the normalized [`HookPayload`], or
/// `None` when the event kind is outside the public vocabulary (so it never
/// triggers a hook and never prints in `atelier --events follow`).
///
/// Pure: no `App`, no IO. `actor` is supplied by the caller — the tap resolves
/// it from the active step; the follow reader folds it from the stream. When
/// `detail` is [`PayloadDetail::Full`] the raw event body is attached;
/// metadata-only omits it. Redaction is applied downstream, never here.
///
/// Note: the `detail` parameter extends the two-argument sketch in the techspec
/// "Core Interfaces" — it is required so the one shared projection can serve
/// both the per-handler tap (which knows its handler's [`PayloadDetail`]) and
/// the follow reader, and so the metadata-vs-full contract is exercised in this
/// single pure function.
pub fn normalize(
    event: &HistoryEvent,
    actor: ActorCtx,
    detail: PayloadDetail,
) -> Option<HookPayload> {
    let public_event = public_name_for_kind(&event.kind)?;
    Some(HookPayload {
        schema_version: HOOK_PAYLOAD_SCHEMA_VERSION,
        event: public_event.to_string(),
        time: event.timestamp.clone(),
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        step_id: event.step_id.clone(),
        actor: Actor {
            agent: actor.agent,
            runtime: actor.runtime,
        },
        target: target_for(&event.kind, &event.payload),
        outcome: outcome_for(&event.kind),
        body: match detail {
            PayloadDetail::Metadata => None,
            PayloadDetail::Full => Some(event.payload.clone()),
        },
    })
}

/// Best-effort `target` — the file path or command an event acts on. `None`
/// when the kind carries no natural target. Tolerant of absent fields (odd or
/// older payloads degrade to `None` rather than panicking).
fn target_for(kind: &str, payload: &Value) -> Option<String> {
    match kind {
        "action_requested" => payload.get("params").and_then(|params| {
            string_field(params, "command").or_else(|| string_field(params, "path"))
        }),
        "file_edit_applied" => string_field(payload, "path").or_else(|| {
            payload
                .get("changed_files")
                .and_then(Value::as_array)
                .and_then(|files| files.first())
                .and_then(Value::as_str)
                .map(str::to_string)
        }),
        _ => None,
    }
}

/// The terminal/decision `outcome` for an event kind, where one is defined.
fn outcome_for(kind: &str) -> Option<String> {
    let outcome = match kind {
        "run_completed" | "file_edit_applied" => "success",
        "run_failed" => "error",
        "run_limit_reached" => "limit_reached",
        "run_interrupted" => "interrupted",
        _ => return None,
    };
    Some(outcome.to_string())
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a deterministic-timestamp event with the standard correlation ids.
    fn event(kind: &str, payload: Value) -> HistoryEvent {
        let mut event = HistoryEvent::new(
            "sess-1",
            Some("run-1".to_string()),
            Some("step-1".to_string()),
            kind,
            payload,
        );
        event.timestamp = "2026-06-17T00:00:00.000Z".to_string();
        event
    }

    fn actor(agent: &str, runtime: &str) -> ActorCtx {
        ActorCtx {
            agent: Some(agent.to_string()),
            runtime: Some(runtime.to_string()),
        }
    }

    #[test]
    fn maps_agent_step_started_to_public_step_started_with_actor() {
        let event = event("agent_step_started", json!({ "agent": "coder" }));
        let payload = normalize(&event, actor("coder", "codex"), PayloadDetail::Metadata)
            .expect("agent_step_started is in the public vocabulary");

        assert_eq!(payload.event, "step_started");
        assert_eq!(payload.schema_version, HOOK_PAYLOAD_SCHEMA_VERSION);
        assert_eq!(payload.time, "2026-06-17T00:00:00.000Z");
        assert_eq!(payload.session_id, "sess-1");
        assert_eq!(payload.run_id.as_deref(), Some("run-1"));
        assert_eq!(payload.step_id.as_deref(), Some("step-1"));
        assert_eq!(payload.actor.agent.as_deref(), Some("coder"));
        assert_eq!(payload.actor.runtime.as_deref(), Some("codex"));
    }

    #[test]
    fn returns_none_for_non_public_kinds() {
        // Stream deltas and hook-lifecycle events are outside the vocabulary, so
        // hooks never fire on them (recursion guard is structural).
        for kind in ["runtime_stream_delta", "hook_started", "hook_completed"] {
            let event = event(kind, json!({ "agent": "coder" }));
            assert!(
                normalize(&event, actor("coder", "codex"), PayloadDetail::Full).is_none(),
                "{kind} must not normalize into a hook payload"
            );
        }
    }

    #[test]
    fn metadata_omits_body_and_full_includes_it() {
        let body = json!({ "agent": "coder", "step_label": "implement" });
        let event = event("agent_step_started", body.clone());

        let metadata = normalize(&event, ActorCtx::default(), PayloadDetail::Metadata).unwrap();
        assert_eq!(metadata.body, None, "metadata default omits the body");

        let full = normalize(&event, ActorCtx::default(), PayloadDetail::Full).unwrap();
        assert_eq!(full.body, Some(body), "full opts into the raw body");
    }

    #[test]
    fn every_public_name_round_trips_and_unknowns_are_absent() {
        // The table itself must match ADR-004's ten-row mapping exactly.
        assert_eq!(PUBLIC_EVENT_VOCABULARY.len(), 10);
        for (public, internal) in PUBLIC_EVENT_VOCABULARY {
            assert_eq!(internal_kind_for_public(public), Some(*internal));
            assert_eq!(public_name_for_kind(internal), Some(*public));
        }
        // The rename is in the table: public `step_started` ↔ internal
        // `agent_step_started`.
        assert_eq!(
            internal_kind_for_public("step_started"),
            Some("agent_step_started")
        );
        // Unknown names resolve nowhere in either direction.
        assert_eq!(internal_kind_for_public("totally_unknown"), None);
        assert_eq!(public_name_for_kind("totally_unknown"), None);
        // Hook-lifecycle kinds are deliberately absent from the public surface.
        assert_eq!(public_name_for_kind("hook_started"), None);
        assert_eq!(public_name_for_kind("hook_completed"), None);
    }

    #[test]
    fn actor_is_orchestrator_for_a_pre_agent_event() {
        // `run_started` fires before any agent step → actor is null/orchestrator.
        let event = event("run_started", json!({ "prompt": "hello" }));
        let payload = normalize(&event, ActorCtx::default(), PayloadDetail::Metadata).unwrap();

        assert_eq!(payload.event, "run_started");
        assert_eq!(payload.actor.agent, None);
        assert_eq!(payload.actor.runtime, None);
        assert_eq!(payload.outcome, None);
        assert_eq!(payload.target, None);
    }

    #[test]
    fn extracts_target_and_outcome_for_file_edit() {
        let event = event(
            "file_edit_applied",
            json!({ "action_id": "a1", "operation": "write_file", "path": "src/main.rs", "bytes": 42 }),
        );
        let payload = normalize(&event, actor("coder", "codex"), PayloadDetail::Metadata).unwrap();

        assert_eq!(payload.event, "file_edited");
        assert_eq!(payload.target.as_deref(), Some("src/main.rs"));
        assert_eq!(payload.outcome.as_deref(), Some("success"));
    }

    #[test]
    fn extracts_command_target_for_action_requested() {
        let event = event(
            "action_requested",
            json!({ "action_id": "a1", "kind": "run_command", "params": { "command": "cargo test" } }),
        );
        let payload = normalize(&event, actor("coder", "codex"), PayloadDetail::Metadata).unwrap();

        assert_eq!(payload.event, "action_requested");
        assert_eq!(payload.target.as_deref(), Some("cargo test"));
        assert_eq!(payload.outcome, None);
    }

    #[test]
    fn terminal_kinds_carry_their_outcome() {
        for (kind, public, outcome) in [
            ("run_completed", "run_completed", "success"),
            ("run_failed", "run_failed", "error"),
            ("run_limit_reached", "run_limit_reached", "limit_reached"),
            ("run_interrupted", "run_interrupted", "interrupted"),
        ] {
            let payload = normalize(
                &event(kind, json!({})),
                ActorCtx::default(),
                PayloadDetail::Metadata,
            )
            .unwrap();
            assert_eq!(payload.event, public);
            assert_eq!(payload.outcome.as_deref(), Some(outcome));
        }
    }

    #[test]
    fn metadata_payload_serializes_without_optional_fields() {
        let event = event("run_started", json!({ "prompt": "hi" }));
        let payload = normalize(&event, ActorCtx::default(), PayloadDetail::Metadata).unwrap();
        let value = serde_json::to_value(&payload).unwrap();

        assert_eq!(value["schema_version"], json!(1));
        assert_eq!(value["event"], json!("run_started"));
        // `target`, `outcome`, and `body` are absent (skip_serializing_if) when
        // unset, keeping the metadata shape minimal.
        assert!(value.get("target").is_none());
        assert!(value.get("outcome").is_none());
        assert!(value.get("body").is_none());
        // The actor is always present, even when null/orchestrator.
        assert_eq!(value["actor"], json!({ "agent": null, "runtime": null }));
    }

    #[test]
    fn payload_detail_defaults_to_metadata() {
        assert_eq!(PayloadDetail::default(), PayloadDetail::Metadata);
    }

    #[test]
    fn payload_detail_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(PayloadDetail::Metadata).unwrap(),
            json!("metadata")
        );
        assert_eq!(
            serde_json::to_value(PayloadDetail::Full).unwrap(),
            json!("full")
        );
    }

    #[test]
    fn hook_action_reports_kind_str() {
        assert_eq!(
            HookAction::Notify(NotifyConfig::default()).kind_str(),
            "notify"
        );
        assert_eq!(
            HookAction::Command("echo".to_string()).kind_str(),
            "command"
        );
    }

    #[test]
    fn handler_matches_only_listed_events() {
        let handler = HookHandler {
            on: vec!["run_completed".to_string(), "run_failed".to_string()],
            action: HookAction::Notify(NotifyConfig::default()),
            payload: PayloadDetail::default(),
        };
        assert!(handler.matches("run_completed"));
        assert!(handler.matches("run_failed"));
        assert!(!handler.matches("run_started"));
    }

    #[test]
    fn hook_lifecycle_payload_round_trips() {
        // `hook_started`: status/duration absent (skipped on serialize).
        let started = HookLifecyclePayload {
            handler_index: 2,
            action: "command".to_string(),
            event: "run_completed".to_string(),
            ..Default::default()
        };
        let value = serde_json::to_value(&started).unwrap();
        assert_eq!(value["handler_index"], json!(2));
        assert!(value.get("status").is_none());
        assert!(value.get("duration_ms").is_none());

        // `hook_completed`: a fully-populated record round-trips byte-for-byte.
        let completed = HookLifecyclePayload {
            handler_index: 2,
            action: "command".to_string(),
            event: "run_completed".to_string(),
            status: Some("error".to_string()),
            duration_ms: Some(12),
            exit_code: Some(1),
            stderr_excerpt: Some("boom".to_string()),
        };
        let round_tripped: HookLifecyclePayload =
            serde_json::from_value(serde_json::to_value(&completed).unwrap()).unwrap();
        assert_eq!(round_tripped, completed);
    }
}
