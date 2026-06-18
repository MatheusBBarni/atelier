use crate::ids::new_id;
use crate::orchestrator::{ArtifactReference, RunState};
use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HistoryEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    /// Groups every event of one execution-graph (DAG) run, mirroring `group_id`
    /// for flat parallel groups (ADR-005). `#[serde(default)]` so legacy logs
    /// without it still deserialize at `schema_version 1` (the DAG is additive).
    #[serde(default)]
    pub graph_id: Option<String>,
    pub step_id: Option<String>,
    pub timestamp: String,
    pub kind: String,
    pub payload: Value,
    #[serde(default)]
    pub payload_truncated: bool,
}

impl HistoryEvent {
    pub fn new(
        session_id: impl Into<String>,
        run_id: Option<String>,
        step_id: Option<String>,
        kind: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self::new_with_group(session_id, run_id, None, step_id, kind, payload)
    }

    pub fn new_with_group(
        session_id: impl Into<String>,
        run_id: Option<String>,
        group_id: Option<String>,
        step_id: Option<String>,
        kind: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: 1,
            event_id: new_id(),
            session_id: session_id.into(),
            run_id,
            group_id,
            graph_id: None,
            step_id,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: kind.into(),
            payload,
            payload_truncated: false,
        }
    }

    /// Construct a graph-keyed event (ADR-005). The DAG sibling of
    /// [`new_with_group`]: it sets `graph_id` and leaves `group_id` `None` so
    /// orchestrator-altitude graph/node events are never mis-keyed through the
    /// flat-group path. Node events carry their `node_id` inside `payload`.
    pub fn new_with_graph(
        session_id: impl Into<String>,
        run_id: Option<String>,
        graph_id: Option<String>,
        step_id: Option<String>,
        kind: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: 1,
            event_id: new_id(),
            session_id: session_id.into(),
            run_id,
            group_id: None,
            graph_id,
            step_id,
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: kind.into(),
            payload,
            payload_truncated: false,
        }
    }
}

/// Redact secrets from an event payload before durable persistence (task_06).
/// Returns `Some(redacted)` when a secret was found — the persisted record then
/// also carries a `_redacted: true` flag when the payload is an object — or
/// `None` when the payload had no secrets (it is written unchanged, so
/// secret-free events pay no clone). The caller's in-memory payload is never
/// mutated; redaction is only for the on-disk record.
fn redact_event_payload(payload: &Value) -> Option<Value> {
    let (redacted, changed) = redact_json(payload);
    if !changed {
        return None;
    }
    let flagged = match redacted {
        Value::Object(mut map) => {
            map.insert("_redacted".to_string(), Value::Bool(true));
            Value::Object(map)
        }
        other => other,
    };
    Some(flagged)
}

/// Recursively redact every string in a JSON value using the runtime redaction
/// patterns (Bearer / `sk-` / `zai-`), covering nested MCP result content and
/// diagnostic strings. Returns the redacted value and whether anything changed.
fn redact_json(value: &Value) -> (Value, bool) {
    match value {
        Value::String(text) => {
            let redacted = crate::runtime::redact_sensitive_text(text);
            let changed = redacted != *text;
            (Value::String(redacted), changed)
        }
        Value::Array(items) => {
            let mut changed = false;
            let redacted = items
                .iter()
                .map(|item| {
                    let (value, item_changed) = redact_json(item);
                    changed |= item_changed;
                    value
                })
                .collect();
            (Value::Array(redacted), changed)
        }
        Value::Object(map) => {
            let mut changed = false;
            let mut redacted = serde_json::Map::with_capacity(map.len());
            for (key, item) in map {
                let (value, item_changed) = redact_json(item);
                changed |= item_changed;
                redacted.insert(key.clone(), value);
            }
            (Value::Object(redacted), changed)
        }
        other => (other.clone(), false),
    }
}

/// Cross-session UI flags persisted at the `.atelier/` data root (ADR-004).
/// Lives outside `sessions/`, so it survives `clean_sessions` and a fresh launch.
/// New show-once flags are added here with `#[serde(default)]` for forward
/// compatibility — never a new file per flag.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct PersistentUiState {
    #[serde(default)]
    first_approval_explainer_shown: bool,
}

/// Root-level filename for [`PersistentUiState`].
const UI_STATE_FILE: &str = "ui_state.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionMetadata {
    pub schema_version: u32,
    pub session_id: String,
    pub working_directory: PathBuf,
    pub started_at: String,
    /// Derived, self-healing cache (ADR-008): the session goal label, or `None`.
    /// The event log is authoritative; this is recomputed by folding the log when
    /// stale, never the source of truth. `#[serde(default)]` so pre-cache
    /// `metadata.json` files still deserialize.
    #[serde(default)]
    pub goal: Option<String>,
    /// Derived cache: terminal run outcome (e.g. `completed` / `failed`), or
    /// `None` while the session has no terminal run yet.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Derived cache: the `HEAD` sha recorded at the last write, the baseline for
    /// resume drift detection (ADR-007).
    #[serde(default)]
    pub last_head_sha: Option<String>,
}

/// Kind string for the lifecycle event that closes a dangling run on resume
/// (ADR-002/008). Additive — no schema bump.
pub const RUN_INTERRUPTED_KIND: &str = "run_interrupted";
/// Kind string for the tamper-evident resume-boundary lifecycle event
/// (ADR-002/007/008). Additive — no schema bump.
pub const SESSION_RESUMED_KIND: &str = "session_resumed";
/// Kind string for the audit event recording acknowledgment of workspace drift
/// at the first state-mutating action after a drifted resume (ADR-004/007).
/// Additive — no schema bump.
pub const RESUME_DRIFT_ACK_KIND: &str = "resume_drift_acknowledged";

/// Execution-graph (DAG) lifecycle event kinds (ADR-005). All additive at
/// history `schema_version 1`: the scheduler (task_04) emits them through the
/// `graph_id`-keyed recorder path and the projection (task_06) registers each
/// one explicitly. The `node_*` kinds carry their `node_id` inside `payload`.
pub const EXECUTION_GRAPH_PROPOSED_KIND: &str = "execution_graph_proposed";
pub const EXECUTION_GRAPH_APPROVED_KIND: &str = "execution_graph_approved";
pub const EXECUTION_GRAPH_REJECTED_KIND: &str = "execution_graph_rejected";
pub const EXECUTION_GRAPH_COMPLETED_KIND: &str = "execution_graph_completed";
pub const NODE_PENDING_KIND: &str = "node_pending";
pub const NODE_READY_KIND: &str = "node_ready";
pub const NODE_RUNNING_KIND: &str = "node_running";
pub const NODE_SUCCEEDED_KIND: &str = "node_succeeded";
pub const NODE_FAILED_KIND: &str = "node_failed";
pub const NODE_SKIPPED_KIND: &str = "node_skipped";
pub const NODE_CANCELLED_KIND: &str = "node_cancelled";

/// Payload for a `run_interrupted` event — closes a dangling run found on resume
/// (ADR-008). Emitted by task_11; folded by the existing run-summary projection
/// arm, which marks the run Interrupted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunInterruptedPayload {
    pub run_id: String,
    pub prior_state: RunState,
}

/// Payload for a `session_resumed` event — the tamper-evident resume boundary
/// (ADR-002/007/008). Emitted by task_11; folded into a visible "Resumed"
/// divider in the transcript.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionResumedPayload {
    pub resumed_at: String,
    pub cwd: PathBuf,
    #[serde(default)]
    pub head_sha: Option<String>,
    pub dirty: bool,
    pub prior_end_state: RunState,
    /// The approval mode the resumed session runs under (serialized form, e.g.
    /// `normal` — task_12 defaults resume to cautious).
    pub approval_mode: String,
    /// Hash of the prior log tail at resume time, for tamper-evidence (ADR-007).
    #[serde(default)]
    pub prior_tail_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HistoryStore {
    root: PathBuf,
    session_id: String,
    session_dir: PathBuf,
    events_path: PathBuf,
    artifacts_dir: PathBuf,
}

impl HistoryStore {
    pub fn create(working_directory: &Path) -> Result<Self> {
        let root = working_directory.join(".atelier");
        let session_id = new_id();
        let session_dir = root.join("sessions").join(&session_id);
        let artifacts_dir = session_dir.join("artifacts");
        let runs_dir = root.join("runs");

        create_private_dir(&artifacts_dir)?;
        create_private_dir(&runs_dir)?;

        let metadata = SessionMetadata {
            schema_version: 1,
            session_id: session_id.clone(),
            working_directory: working_directory.to_path_buf(),
            started_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            goal: None,
            outcome: None,
            last_head_sha: None,
        };
        let metadata_path = session_dir.join("metadata.json");
        write_private_file(
            &metadata_path,
            serde_json::to_string_pretty(&metadata)?.as_bytes(),
        )?;

        let events_path = session_dir.join("events.jsonl");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
            .with_context(|| format!("failed to create {}", events_path.display()))?;
        set_private_file_permissions(&events_path)?;

        Ok(Self {
            root,
            session_id,
            session_dir,
            events_path,
            artifacts_dir,
        })
    }

    /// Bind a store to an *existing* session under `root` (the `.atelier` data
    /// root) by id, without creating anything — the sibling of [`create`]. Reads
    /// and schema-validates `metadata.json`, failing loud on a missing file or
    /// `schema_version != 1` (the same validation boundary as the event reader,
    /// ADR-003). Used by the session browser (task_03/06) and resume
    /// (task_10/11).
    pub fn open(root: &Path, session_id: &str) -> Result<Self> {
        let session_dir = root.join("sessions").join(session_id);
        let metadata = read_metadata(&session_dir)?;
        if metadata.schema_version != 1 {
            return Err(anyhow!(
                "unsupported session metadata schema_version {} in {}",
                metadata.schema_version,
                session_dir.join("metadata.json").display()
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            session_id: session_id.to_string(),
            events_path: session_dir.join("events.jsonl"),
            artifacts_dir: session_dir.join("artifacts"),
            session_dir,
        })
    }

    /// Read this session's `metadata.json` (including the derived cache fields).
    pub fn read_metadata(&self) -> Result<SessionMetadata> {
        read_metadata(&self.session_dir)
    }

    /// Self-healing cache write (ADR-008): rewrite the derived metadata fields
    /// (`goal`, `outcome`, `last_head_sha`) while preserving the authoritative
    /// fields and the file's private `0600` permissions. The event log stays the
    /// source of truth; callers recompute these by folding the log and persist
    /// the result here.
    pub fn update_metadata_cache(
        &self,
        goal: Option<String>,
        outcome: Option<String>,
        last_head_sha: Option<String>,
    ) -> Result<()> {
        let mut metadata = read_metadata(&self.session_dir)?;
        metadata.goal = goal;
        metadata.outcome = outcome;
        metadata.last_head_sha = last_head_sha;
        write_private_file(
            &self.session_dir.join("metadata.json"),
            serde_json::to_string_pretty(&metadata)?.as_bytes(),
        )
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn append_event(&self, event: &HistoryEvent) -> Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.events_path)
            .with_context(|| format!("failed to open {}", self.events_path.display()))?;
        // Record-time redaction (ADR-006, task_06): secrets are stripped from the
        // payload BEFORE it is persisted, so the durable audit log can never hold
        // a credential in cleartext. The caller's in-memory `event` is untouched —
        // we serialize a redacted clone only when a secret was actually present.
        match redact_event_payload(&event.payload) {
            Some(redacted) => {
                let mut record = event.clone();
                record.payload = redacted;
                serde_json::to_writer(&mut file, &record)?;
            }
            None => serde_json::to_writer(&mut file, event)?,
        }
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    pub fn append_debug_event(&self, event: &HistoryEvent) -> Result<()> {
        let path = self.root.join("debug.log");
        let existed = path.exists();
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        if !existed {
            set_private_file_permissions(&path)?;
        }
        // Redact the payload here too: the debug log is on-disk like the event
        // log (task_06).
        let payload = redact_event_payload(&event.payload).unwrap_or_else(|| event.payload.clone());
        let record = serde_json::json!({
            "schema_version": 1,
            "timestamp": event.timestamp,
            "event_id": event.event_id,
            "session_id": event.session_id,
            "run_id": event.run_id,
            "group_id": event.group_id,
            "graph_id": event.graph_id,
            "step_id": event.step_id,
            "kind": event.kind,
            "payload": payload,
        });
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    pub fn read_events(&self) -> Result<Vec<HistoryEvent>> {
        read_events_from_path(&self.events_path)
    }

    fn ui_state_path(&self) -> PathBuf {
        self.root.join(UI_STATE_FILE)
    }

    fn read_ui_state(&self) -> PersistentUiState {
        // A missing or unparseable file degrades to defaults rather than failing
        // the run — at worst the show-once explainer shows one extra time.
        fs::read_to_string(self.ui_state_path())
            .ok()
            .and_then(|raw| serde_json::from_str::<PersistentUiState>(&raw).ok())
            .unwrap_or_default()
    }

    /// Whether the first-approval explainer has already been shown once for this
    /// user (cross-session show-once latch, ADR-004).
    pub fn first_approval_explainer_shown(&self) -> bool {
        self.read_ui_state().first_approval_explainer_shown
    }

    /// Idempotently latch the first-approval explainer as shown. A no-op (no
    /// write) when the flag is already set.
    pub fn mark_first_approval_explainer_shown(&self) -> Result<()> {
        let mut state = self.read_ui_state();
        if state.first_approval_explainer_shown {
            return Ok(());
        }
        state.first_approval_explainer_shown = true;
        write_private_file(
            &self.ui_state_path(),
            serde_json::to_string_pretty(&state)?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn write_run_record<T: Serialize>(&self, run_id: &str, record: &T) -> Result<PathBuf> {
        let path = self.root.join("runs").join(format!("{run_id}.json"));
        write_private_file(&path, serde_json::to_string_pretty(record)?.as_bytes())?;
        Ok(path)
    }

    pub fn write_artifact(
        &self,
        extension: &str,
        media_type: &str,
        contents: &[u8],
        redaction_status: &str,
    ) -> Result<ArtifactReference> {
        let artifact_id = new_id();
        let extension = extension.trim_start_matches('.');
        let filename = format!("{artifact_id}.{extension}");
        let path = self.artifacts_dir.join(&filename);
        write_private_file(&path, contents)?;

        let sha256 = Sha256::digest(contents)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        // Report the path relative to the workspace root (so it includes the
        // `.atelier/` prefix) — that form is findable and copyable from the
        // project root, unlike a path relative to the hidden data dir.
        let relative_path = self
            .root
            .parent()
            .and_then(|workspace| path.strip_prefix(workspace).ok())
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        Ok(ArtifactReference {
            artifact_id,
            path: relative_path,
            media_type: media_type.to_string(),
            byte_length: contents.len(),
            sha256,
            redaction_status: redaction_status.to_string(),
        })
    }
}

pub fn read_events_from_path(path: &Path) -> Result<Vec<HistoryEvent>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: HistoryEvent = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse history event in {}", path.display()))?;
        if event.schema_version != 1 {
            return Err(anyhow!(
                "unsupported history schema_version {} in {}",
                event.schema_version,
                path.display()
            ));
        }
        events.push(event);
    }
    Ok(events)
}

/// Enumerate every session's event log under `<root>/sessions/*/events.jsonl`.
///
/// `root` is the `.atelier` data root. Returns an empty list (not an error) when
/// the `sessions/` directory is absent — a fresh project simply has no history
/// yet. Paths are sorted for determinism; the recall ordering itself is decided
/// later by event timestamp in [`project_prompt_history`]. This is the one new
/// reader primitive, reusable by future Ctrl-R / scope-cycling / outcome views
/// (ADR-004).
pub fn list_session_event_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let sessions_dir = root.join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(&sessions_dir)
        .with_context(|| format!("failed to read {}", sessions_dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", sessions_dir.display()))?;
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let events_path = entry.path().join("events.jsonl");
        if events_path.is_file() {
            paths.push(events_path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Project a per-project prompt-recall list from the event log (ADR-001, ADR-004).
///
/// Reads every session log under `root`, keeps `prompt_submitted` events, drops
/// prompts beginning with a leading space (the secrets escape hatch), sorts by
/// event `timestamp` descending (RFC3339 millis are lexically sortable),
/// collapses *consecutive* duplicates, and truncates to `max`. Pure and
/// read-only — it adds no persistence.
///
/// Tolerant by construction: a single unreadable or legacy-schema file is
/// skipped (`read_events_from_path` errors on `schema_version != 1`), so one bad
/// file can never empty the whole projection. A missing `sessions/` directory
/// yields an empty list. Memory is kept proportional to the prompt count rather
/// than the full event log — only `(timestamp, prompt)` pairs are retained past
/// each per-file fold, and the result is capped to `max`.
pub fn project_prompt_history(root: &Path, max: usize) -> Vec<String> {
    if max == 0 {
        return Vec::new();
    }
    let paths = match list_session_event_paths(root) {
        Ok(paths) => paths,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<(String, String)> = Vec::new();
    for path in paths {
        // Per-file tolerance: skip a file that fails to read or carries an
        // unsupported schema_version, rather than failing the whole projection.
        let Ok(events) = read_events_from_path(&path) else {
            continue;
        };
        for event in events {
            if event.kind != "prompt_submitted" {
                continue;
            }
            let Some(prompt) = event.payload.get("prompt").and_then(Value::as_str) else {
                continue;
            };
            // Leading-space-skip: a prompt beginning with a space is the secrets
            // escape hatch and never surfaces in recall (the durable event log is
            // unchanged — this is a filter, not a deletion).
            if prompt.starts_with(' ') {
                continue;
            }
            entries.push((event.timestamp, prompt.to_string()));
        }
    }

    // Newest first by event timestamp (lexical sort is valid for RFC3339 millis).
    entries.sort_by(|a, b| b.0.cmp(&a.0));

    let mut history: Vec<String> = Vec::with_capacity(entries.len().min(max));
    for (_, prompt) in entries {
        // Consecutive-dedup: collapse an immediately repeated prompt; a
        // non-consecutive repeat (a different prompt in between) is preserved.
        if history.last().map(String::as_str) == Some(prompt.as_str()) {
            continue;
        }
        history.push(prompt);
        if history.len() >= max {
            break;
        }
    }
    history
}

/// A newest-first browse row for the session picker (task_07). All fields are
/// derived from the session's event log (the source of truth, ADR-008); the
/// `metadata.json` cache is a self-healing accelerator, never authoritative.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_id: String,
    /// Human-scannable label: the goal if set, else the first user prompt
    /// (truncated), else `started_at · outcome`.
    pub label: String,
    pub started_at: String,
    /// Terminal run outcome folded from the log (`Idle` when no run finished).
    pub outcome: RunState,
    pub working_directory: PathBuf,
}

/// Longest label rendered in the picker before truncation.
const SESSION_LABEL_MAX_CHARS: usize = 72;

/// Build newest-first [`SessionSummary`] rows for every session under `root`
/// (the `.atelier` data root). Tolerant: a session that fails to open, parse, or
/// read is skipped rather than failing the whole list. Self-healing: each
/// session's `goal`/`outcome` cache is recomputed from its log and rewritten when
/// missing or disagreeing (the log wins, ADR-008).
///
/// Newest-first ordering leverages the ULID-like session ids (lexicographically
/// sortable = chronological): the directory names are sorted and reversed.
pub fn list_session_summaries(root: &Path) -> Vec<SessionSummary> {
    let sessions_dir = root.join("sessions");
    let Ok(entries) = fs::read_dir(&sessions_dir) else {
        return Vec::new(); // No sessions/ yet → empty, not an error.
    };
    let mut session_ids: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    // ULID-like ids: lexicographic order is chronological; reverse → newest first.
    session_ids.sort();
    session_ids.reverse();

    session_ids
        .iter()
        .filter_map(|session_id| summarize_session(root, session_id))
        .collect()
}

/// Summarize one session, self-healing its metadata cache from the log. Returns
/// `None` (skip) when the session can't be opened/read.
fn summarize_session(root: &Path, session_id: &str) -> Option<SessionSummary> {
    let store = HistoryStore::open(root, session_id).ok()?;
    let metadata = store.read_metadata().ok()?;
    // The log is authoritative; an unreadable log degrades to "no events".
    let events = store.read_events().unwrap_or_default();

    let goal = derive_goal(&events);
    let outcome = derive_outcome(&events);
    let outcome_label = run_state_label(&outcome);

    // Self-heal the derived cache when missing or disagreeing with the log.
    if metadata.goal != goal || metadata.outcome.as_deref() != Some(outcome_label.as_str()) {
        let _ = store.update_metadata_cache(
            goal.clone(),
            Some(outcome_label.clone()),
            metadata.last_head_sha.clone(),
        );
    }

    let label = goal
        .clone()
        .or_else(|| first_prompt_label(&events))
        .unwrap_or_else(|| format!("{} · {}", metadata.started_at, outcome_label));

    Some(SessionSummary {
        session_id: session_id.to_string(),
        label,
        started_at: metadata.started_at,
        outcome,
        working_directory: metadata.working_directory,
    })
}

/// The PRD resume success metrics, derived **entirely by folding the durable log**
/// — no external telemetry (ADR-002/005). Counts across every session under a
/// data root; the rates are the headline KPIs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResumeMetrics {
    /// Sessions that ended non-terminally (a run was interrupted or left dangling).
    pub crashed: usize,
    /// Crashed sessions later recovered (their log carries a `session_resumed`).
    pub recovered: usize,
    /// Sessions that were resumed at least once.
    pub resumed: usize,
    /// Resumed sessions that reached a terminal `Completed` outcome.
    pub resumed_completed: usize,
}

impl ResumeMetrics {
    /// Crash-recovery adoption: `recovered / crashed`. `None` when nothing crashed
    /// (the ratio is undefined, not zero).
    pub fn crash_recovery_rate(&self) -> Option<f64> {
        (self.crashed > 0).then(|| self.recovered as f64 / self.crashed as f64)
    }

    /// Resumed-session completion: `resumed_completed / resumed`. `None` when
    /// nothing was resumed.
    pub fn resumed_completion_rate(&self) -> Option<f64> {
        (self.resumed > 0).then(|| self.resumed_completed as f64 / self.resumed as f64)
    }
}

/// Fold every session under `root` into [`ResumeMetrics`]. Tolerant: a session
/// whose log can't be read is skipped (mirrors [`list_session_summaries`]).
pub fn resume_metrics(root: &Path) -> ResumeMetrics {
    let mut metrics = ResumeMetrics::default();
    let Ok(entries) = fs::read_dir(root.join("sessions")) else {
        return metrics;
    };
    for entry in entries.filter_map(Result::ok) {
        let Ok(events) = read_events_from_path(&entry.path().join("events.jsonl")) else {
            continue;
        };
        if events.is_empty() {
            continue;
        }
        let resumed = events
            .iter()
            .any(|event| event.kind == SESSION_RESUMED_KIND);
        if session_ended_non_terminal(&events) {
            metrics.crashed += 1;
            if resumed {
                metrics.recovered += 1;
            }
        }
        if resumed {
            metrics.resumed += 1;
            if derive_outcome(&events) == RunState::Completed {
                metrics.resumed_completed += 1;
            }
        }
    }
    metrics
}

/// Whether a session ended non-terminally — a run was interrupted
/// (`run_interrupted`, including the resume reconciliation) or left dangling. The
/// durable "crashed" signal behind the crash-recovery metric.
fn session_ended_non_terminal(events: &[HistoryEvent]) -> bool {
    events
        .iter()
        .any(|event| event.kind == RUN_INTERRUPTED_KIND)
        || dangling_run_from_events(events).is_some()
}

/// Time-to-continue: the delay from a session's `session_resumed` boundary to the
/// next `prompt_submitted` (the user actually continuing in the resumed session).
/// `None` when the session was never resumed, no prompt followed the resume, or a
/// timestamp won't parse. Derived from the RFC3339 event timestamps.
pub fn time_to_continue(events: &[HistoryEvent]) -> Option<chrono::Duration> {
    let resumed_index = events
        .iter()
        .position(|event| event.kind == SESSION_RESUMED_KIND)?;
    let resumed_at = chrono::DateTime::parse_from_rfc3339(&events[resumed_index].timestamp).ok()?;
    let prompt = events[resumed_index + 1..]
        .iter()
        .find(|event| event.kind == "prompt_submitted")?;
    let prompt_at = chrono::DateTime::parse_from_rfc3339(&prompt.timestamp).ok()?;
    Some(prompt_at - resumed_at)
}

/// The active session goal folded from the log: the last `session_goal_set`
/// value, cleared by a later `session_goal_cleared`.
pub(crate) fn derive_goal(events: &[HistoryEvent]) -> Option<String> {
    let mut goal = None;
    for event in events {
        match event.kind.as_str() {
            "session_goal_set" => {
                goal = event
                    .payload
                    .get("goal")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            "session_goal_cleared" => goal = None,
            _ => {}
        }
    }
    goal
}

/// The session outcome folded from the log: the last *terminal* run state
/// (`is_terminal()`, task_01), or the terminal `run_state` in a `session_ended`
/// payload. `Idle` when no run reached a terminal state.
pub(crate) fn derive_outcome(events: &[HistoryEvent]) -> RunState {
    let mut outcome = RunState::Idle;
    for event in events {
        let candidate = match event.kind.as_str() {
            "run_completed" => Some(RunState::Completed),
            "run_failed" => Some(RunState::Failed),
            "run_limit_reached" => Some(RunState::LimitReached),
            "run_interrupted" => Some(RunState::Interrupted),
            "session_ended" => event
                .payload
                .get("run_state")
                .and_then(|value| serde_json::from_value::<RunState>(value.clone()).ok()),
            _ => None,
        };
        if let Some(candidate) = candidate {
            if candidate.is_terminal() {
                outcome = candidate;
            }
        }
    }
    outcome
}

/// A SHA-256 digest of the pre-resume event log, recorded in the `session_resumed`
/// boundary as tamper-evidence for the prior tail (ADR-002/007). Hashing the
/// serialized parsed events (which atelier solely reads and writes) detects added,
/// removed, reordered, or payload-edited prior events. `None` for an empty log.
pub(crate) fn hash_events_digest(events: &[HistoryEvent]) -> Option<String> {
    if events.is_empty() {
        return None;
    }
    let bytes = serde_json::to_vec(events).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

/// The most-recently-started run together with the state it was last seen in,
/// **only when that run never reached a terminal state** — i.e. a run left
/// dangling by a crash or a quit mid-run (ADR-002/008). `None` when the last run
/// closed terminally or no run ever started.
///
/// Resume folds this to decide whether to append a reconciling `run_interrupted`
/// event; the `!is_terminal()` filter (task_01) is the "detect dangling" gate.
/// A `run_started` marks a run at least `Running`; a later terminal run event
/// (or a `session_ended` whose own `run_state` is terminal) closes it. A
/// graceful quit *mid-run* records the in-flight `active_run_id` + `run_state`
/// in `session_ended`, which is trusted; a clean between-runs quit carries
/// `active_run_id: null` and must not resurrect the just-closed run.
pub(crate) fn dangling_run_from_events(events: &[HistoryEvent]) -> Option<(String, RunState)> {
    let mut current: Option<(String, RunState)> = None;
    for event in events {
        match event.kind.as_str() {
            "run_started" => {
                if let Some(run_id) = event.run_id.clone() {
                    current = Some((run_id, RunState::Running));
                }
            }
            "run_completed" => close_current_run(&mut current, event, RunState::Completed),
            "run_failed" => close_current_run(&mut current, event, RunState::Failed),
            "run_limit_reached" => close_current_run(&mut current, event, RunState::LimitReached),
            "run_interrupted" => close_current_run(&mut current, event, RunState::Interrupted),
            "session_ended" => {
                if let Some(run_id) = event.payload.get("active_run_id").and_then(Value::as_str) {
                    let state = event
                        .payload
                        .get("run_state")
                        .and_then(|value| serde_json::from_value::<RunState>(value.clone()).ok())
                        .unwrap_or(RunState::Running);
                    current = Some((run_id.to_string(), state));
                }
            }
            _ => {}
        }
    }
    current.filter(|(_, state)| !state.is_terminal())
}

/// Mark the run currently being folded by [`dangling_run_from_events`] as closed
/// in `state`. Runs are sequential (the one-active-run guard), so a terminal
/// event closes the open run; the `run_id` is matched when the event carries one
/// to stay robust against any out-of-order tail.
fn close_current_run(
    current: &mut Option<(String, RunState)>,
    event: &HistoryEvent,
    state: RunState,
) {
    if let Some((run_id, slot)) = current.as_mut() {
        if event
            .run_id
            .as_deref()
            .map(|id| id == run_id)
            .unwrap_or(true)
        {
            *slot = state;
        }
    }
}

/// The first non-secret user prompt as a truncated label, or `None`. Mirrors
/// [`project_prompt_history`]'s extraction, including the leading-space secrets
/// escape hatch (such prompts never surface).
fn first_prompt_label(events: &[HistoryEvent]) -> Option<String> {
    events
        .iter()
        .filter(|event| event.kind == "prompt_submitted")
        .find_map(|event| {
            let prompt = event.payload.get("prompt").and_then(Value::as_str)?;
            if prompt.starts_with(' ') {
                return None; // Secrets escape hatch — skip, try the next prompt.
            }
            Some(truncate_label(prompt))
        })
}

fn truncate_label(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= SESSION_LABEL_MAX_CHARS {
        return text.to_string();
    }
    format!(
        "{}…",
        text.chars()
            .take(SESSION_LABEL_MAX_CHARS.saturating_sub(1))
            .collect::<String>()
    )
}

/// The snake_case label for a [`RunState`] (e.g. `completed`, `limit_reached`),
/// matching its serde representation and the cached `outcome` string.
fn run_state_label(state: &RunState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

/// The most recent short HEAD recorded in the log — the resume drift baseline
/// (ADR-007). Folds for the last event carrying a non-empty `head_sha` payload
/// field (recorded on `run_started` at run boundaries). `None` when no run
/// recorded a HEAD (e.g. a non-git workspace).
pub fn last_recorded_head_sha(events: &[HistoryEvent]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        event
            .payload
            .get("head_sha")
            .and_then(Value::as_str)
            .filter(|sha| !sha.is_empty())
            .map(str::to_string)
    })
}

pub fn clean_sessions(working_directory: &Path) -> Result<Vec<PathBuf>> {
    let root = working_directory.join(".atelier");
    let targets = [root.join("sessions"), root.join("runs")];
    let mut deleted = Vec::new();
    for target in targets {
        if target.exists() {
            fs::remove_dir_all(&target)
                .with_context(|| format!("failed to delete {}", target.display()))?;
            deleted.push(target);
        }
    }
    Ok(deleted)
}

/// Read and parse a session's `metadata.json` from its session directory.
fn read_metadata(session_dir: &Path) -> Result<SessionMetadata> {
    let metadata_path = session_dir.join("metadata.json");
    let raw = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))
}

fn create_private_dir(path: &Path) -> Result<()> {
    let existed = path.exists();
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !existed {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
                format!("failed to set private permissions on {}", path.display())
            })?;
        }
    }
    Ok(())
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    let mut file =
        File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    file.write_all(contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.flush()?;
    set_private_file_permissions(path)?;
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set private permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn appends_and_reads_jsonl_events() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let event = HistoryEvent::new(
            store.session_id().to_string(),
            Some("run".to_string()),
            None,
            "run_started",
            json!({"prompt": "hello"}),
        );
        store.append_event(&event).unwrap();
        let events = store.read_events().unwrap();
        assert_eq!(events, vec![event]);
    }

    // ── Record-time redaction (task_06) ──

    #[test]
    fn redact_json_redacts_bearer_token() {
        let (redacted, changed) =
            redact_json(&json!({ "header": "Authorization: Bearer abc123secret" }));
        assert!(changed);
        let rendered = redacted.to_string();
        assert!(
            !rendered.contains("abc123secret"),
            "token leaked: {rendered}"
        );
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn redact_json_redacts_nested_sk_token_in_mcp_content() {
        let payload =
            json!({ "content": [ { "type": "text", "text": "key sk-ABCDEF0123456789" } ] });
        let (redacted, changed) = redact_json(&payload);
        assert!(changed);
        assert!(
            !redacted.to_string().contains("sk-ABCDEF0123456789"),
            "nested secret leaked"
        );
    }

    #[test]
    fn redact_json_leaves_clean_payload_unchanged() {
        let payload = json!({ "summary": "all good", "count": 3 });
        let (redacted, changed) = redact_json(&payload);
        assert!(!changed);
        assert_eq!(redacted, payload);
    }

    #[test]
    fn append_event_redacts_secret_on_disk_but_not_in_memory() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let secret = "sk-DEADBEEF0123456789ABCDEF";
        let event = HistoryEvent::new(
            store.session_id().to_string(),
            Some("run".to_string()),
            None,
            "mcp_tool_result",
            json!({
                "content": [ { "type": "text", "text": format!("token {secret}") } ],
                "is_error": false,
            }),
        );
        store.append_event(&event).unwrap();

        // On-disk: the secret is gone and the redaction flag is recorded.
        let raw = fs::read_to_string(store.session_dir().join("events.jsonl")).unwrap();
        assert!(!raw.contains(secret), "secret must never reach disk: {raw}");
        assert!(
            raw.contains("_redacted"),
            "redaction flag should be recorded"
        );

        // The caller's in-memory event is unaffected (control flow is unchanged).
        assert!(event.payload.to_string().contains(secret));

        // Reading the durable log back reflects the redacted record.
        let read = store.read_events().unwrap();
        assert!(!read[0].payload.to_string().contains(secret));
    }

    #[test]
    fn append_event_with_no_secret_is_written_unchanged() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let event = HistoryEvent::new(
            store.session_id().to_string(),
            Some("run".to_string()),
            None,
            "mcp_tool_result",
            json!({ "content": "ordinary output", "is_error": false }),
        );
        store.append_event(&event).unwrap();
        let raw = fs::read_to_string(store.session_dir().join("events.jsonl")).unwrap();
        assert!(
            !raw.contains("_redacted"),
            "clean payload must not be flagged"
        );
        assert_eq!(store.read_events().unwrap(), vec![event]);
    }

    // ── HistoryStore::open + self-healing metadata cache (task_02) ──

    #[test]
    fn open_loads_existing_session_and_reads_same_events_in_order() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let session_id = store.session_id().to_string();
        let first = HistoryEvent::new(
            session_id.clone(),
            Some("run".to_string()),
            None,
            "run_started",
            json!({ "prompt": "hi" }),
        );
        let second = HistoryEvent::new(
            session_id.clone(),
            Some("run".to_string()),
            None,
            "run_completed",
            json!({ "summary": "done" }),
        );
        store.append_event(&first).unwrap();
        store.append_event(&second).unwrap();

        // Re-open the same session by id without going through create().
        let opened = HistoryStore::open(&dir.path().join(".atelier"), &session_id).unwrap();
        assert_eq!(opened.session_id(), session_id);
        assert_eq!(opened.read_events().unwrap(), vec![first, second]);
    }

    #[test]
    fn open_rejects_unsupported_metadata_schema_version() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let session_id = store.session_id().to_string();
        let root = dir.path().join(".atelier");
        let metadata_path = root
            .join("sessions")
            .join(&session_id)
            .join("metadata.json");
        fs::write(
            &metadata_path,
            r#"{"schema_version":2,"session_id":"x","working_directory":".","started_at":"t"}"#,
        )
        .unwrap();
        let err = HistoryStore::open(&root, &session_id).unwrap_err();
        assert!(
            err.to_string().contains("schema_version 2"),
            "expected a loud schema error, got: {err}"
        );
    }

    #[test]
    fn legacy_metadata_without_cache_fields_defaults_to_none() {
        // A pre-cache metadata.json (no goal/outcome/last_head_sha) still loads.
        let legacy = r#"{"schema_version":1,"session_id":"s","working_directory":"/tmp","started_at":"2026-06-17T00:00:00.000Z"}"#;
        let metadata: SessionMetadata = serde_json::from_str(legacy).unwrap();
        assert_eq!(metadata.goal, None);
        assert_eq!(metadata.outcome, None);
        assert_eq!(metadata.last_head_sha, None);
    }

    #[test]
    fn update_metadata_cache_persists_fields_and_preserves_permissions() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        store
            .update_metadata_cache(
                Some("ship it".to_string()),
                Some("completed".to_string()),
                None,
            )
            .unwrap();

        let metadata = store.read_metadata().unwrap();
        assert_eq!(metadata.goal.as_deref(), Some("ship it"));
        assert_eq!(metadata.outcome.as_deref(), Some("completed"));
        assert_eq!(metadata.last_head_sha, None);
        // Authoritative fields untouched; still schema v1 (additive, no bump).
        assert_eq!(metadata.schema_version, 1);
        assert_eq!(metadata.session_id, store.session_id());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = dir
                .path()
                .join(".atelier")
                .join("sessions")
                .join(store.session_id())
                .join("metadata.json");
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "metadata.json should stay private 0600");
        }
    }

    #[test]
    fn update_metadata_cache_round_trips_last_head_sha() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        store
            .update_metadata_cache(None, None, Some("abc123".to_string()))
            .unwrap();
        assert_eq!(
            store.read_metadata().unwrap().last_head_sha.as_deref(),
            Some("abc123")
        );
    }

    // ── session summaries (task_03) ──

    fn append(store: &HistoryStore, kind: &str, payload: Value) {
        let event = HistoryEvent::new(
            store.session_id().to_string(),
            Some("run".to_string()),
            None,
            kind,
            payload,
        );
        store.append_event(&event).unwrap();
    }

    #[test]
    fn list_session_summaries_orders_newest_first_by_id() {
        let dir = tempdir().unwrap();
        let ids: Vec<String> = (0..3)
            .map(|_| {
                HistoryStore::create(dir.path())
                    .unwrap()
                    .session_id()
                    .to_string()
            })
            .collect();
        let summaries = list_session_summaries(&dir.path().join(".atelier"));
        let got: Vec<String> = summaries.iter().map(|s| s.session_id.clone()).collect();
        // The contract is reverse-lexicographic by id; for ULID ids that is
        // newest-created first.
        let mut expected = ids.clone();
        expected.sort();
        expected.reverse();
        assert_eq!(got, expected);
    }

    #[test]
    fn summary_label_uses_goal_when_set() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        append(
            &store,
            "prompt_submitted",
            json!({ "prompt": "do something" }),
        );
        append(
            &store,
            "session_goal_set",
            json!({ "goal": "ship the feature" }),
        );
        let summaries = list_session_summaries(&dir.path().join(".atelier"));
        assert_eq!(summaries[0].label, "ship the feature");
    }

    #[test]
    fn summary_label_falls_back_to_first_prompt() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        // A leading-space (secret) prompt is skipped; the first real prompt wins.
        append(
            &store,
            "prompt_submitted",
            json!({ "prompt": " secret token" }),
        );
        append(
            &store,
            "prompt_submitted",
            json!({ "prompt": "fix the parser" }),
        );
        let summaries = list_session_summaries(&dir.path().join(".atelier"));
        assert_eq!(summaries[0].label, "fix the parser");
    }

    #[test]
    fn summary_label_falls_back_to_timestamp_and_outcome() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        // No goal, no prompt — only a terminal run.
        append(&store, "run_completed", json!({ "summary": "done" }));
        let summaries = list_session_summaries(&dir.path().join(".atelier"));
        let summary = &summaries[0];
        assert_eq!(summary.outcome, RunState::Completed);
        assert!(
            summary.label.contains("completed"),
            "label: {}",
            summary.label
        );
        assert!(
            summary.label.contains(&summary.started_at),
            "label: {}",
            summary.label
        );
    }

    #[test]
    fn summary_self_heals_missing_outcome_cache_from_log() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        append(&store, "run_failed", json!({ "reason": "boom" }));
        // Fresh metadata has no cached outcome.
        assert_eq!(store.read_metadata().unwrap().outcome, None);

        let summaries = list_session_summaries(&dir.path().join(".atelier"));
        assert_eq!(summaries[0].outcome, RunState::Failed);
        // The browse pass rewrote the cache from the log (log wins, ADR-008).
        assert_eq!(
            store.read_metadata().unwrap().outcome.as_deref(),
            Some("failed")
        );
    }

    #[test]
    fn corrupt_session_is_skipped_without_failing_the_list() {
        let dir = tempdir().unwrap();
        let good = HistoryStore::create(dir.path()).unwrap();
        append(
            &good,
            "prompt_submitted",
            json!({ "prompt": "valid session" }),
        );
        let good_id = good.session_id().to_string();

        // A corrupt session: unparseable metadata.json.
        let root = dir.path().join(".atelier");
        let corrupt_dir = root.join("sessions").join("corrupt");
        fs::create_dir_all(&corrupt_dir).unwrap();
        fs::write(corrupt_dir.join("metadata.json"), "not json").unwrap();

        let summaries = list_session_summaries(&root);
        let ids: Vec<&str> = summaries.iter().map(|s| s.session_id.as_str()).collect();
        assert!(ids.contains(&good_id.as_str()));
        assert!(
            !ids.contains(&"corrupt"),
            "corrupt session should be skipped"
        );
    }

    #[test]
    fn list_session_summaries_over_multi_session_fixture() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");

        let a = HistoryStore::create(dir.path()).unwrap();
        append(&a, "prompt_submitted", json!({ "prompt": "first session" }));
        append(&a, "run_completed", json!({ "summary": "ok" }));

        let b = HistoryStore::create(dir.path()).unwrap();
        append(
            &b,
            "session_goal_set",
            json!({ "goal": "second session goal" }),
        );
        append(&b, "run_failed", json!({ "reason": "nope" }));

        let summaries = list_session_summaries(&root);
        assert_eq!(summaries.len(), 2);
        // Order is reverse-lexicographic by id (deterministic; back-to-back ULIDs
        // can share a millisecond, so don't assume creation order == id order).
        let mut expected_order = vec![a.session_id().to_string(), b.session_id().to_string()];
        expected_order.sort();
        expected_order.reverse();
        assert_eq!(
            summaries
                .iter()
                .map(|s| s.session_id.clone())
                .collect::<Vec<_>>(),
            expected_order
        );
        // Content by id.
        let by_id = |id: &str| summaries.iter().find(|s| s.session_id == id).unwrap();
        assert_eq!(by_id(a.session_id()).label, "first session");
        assert_eq!(by_id(a.session_id()).outcome, RunState::Completed);
        assert_eq!(by_id(b.session_id()).label, "second session goal");
        assert_eq!(by_id(b.session_id()).outcome, RunState::Failed);
    }

    // ── HEAD baseline fold (task_05) ──

    #[test]
    fn last_recorded_head_sha_returns_the_most_recent_non_empty() {
        let head_a = HistoryEvent::new(
            "s",
            Some("r1".to_string()),
            None,
            "run_started",
            json!({ "run_id": "r1", "head_sha": "aaa1111" }),
        );
        let head_b = HistoryEvent::new(
            "s",
            Some("r2".to_string()),
            None,
            "run_started",
            json!({ "run_id": "r2", "head_sha": "bbb2222" }),
        );
        let no_head = HistoryEvent::new(
            "s",
            Some("r2".to_string()),
            None,
            "run_completed",
            json!({ "summary": "done" }),
        );
        // Empty log → no baseline.
        assert_eq!(last_recorded_head_sha(&[]), None);
        // Most recent recorded HEAD wins; events without a head_sha are ignored.
        assert_eq!(
            last_recorded_head_sha(&[head_a.clone(), head_b, no_head]),
            Some("bbb2222".to_string())
        );
        // A null head_sha (non-git run) is skipped, falling back to an earlier one.
        let null_head = HistoryEvent::new(
            "s",
            Some("r3".to_string()),
            None,
            "run_started",
            json!({ "run_id": "r3", "head_sha": null }),
        );
        assert_eq!(
            last_recorded_head_sha(&[head_a, null_head]),
            Some("aaa1111".to_string())
        );
    }

    // ── Dangling-run detection (task_10) ──

    fn run_event(run_id: &str, kind: &str, payload: Value) -> HistoryEvent {
        HistoryEvent::new("s", Some(run_id.to_string()), None, kind, payload)
    }

    #[test]
    fn dangling_run_is_none_for_an_empty_or_runless_log() {
        assert_eq!(dangling_run_from_events(&[]), None);
        let goal_only = HistoryEvent::new(
            "s",
            None,
            None,
            "session_goal_set",
            json!({ "goal": "ship it" }),
        );
        assert_eq!(dangling_run_from_events(&[goal_only]), None);
    }

    #[test]
    fn dangling_run_is_none_when_the_last_run_closed_terminally() {
        let events = [
            run_event("r1", "run_started", json!({ "run_id": "r1" })),
            run_event("r1", "run_completed", json!({ "summary": "done" })),
        ];
        assert_eq!(dangling_run_from_events(&events), None);
    }

    #[test]
    fn dangling_run_reports_a_crash_mid_run_as_running() {
        // A started run with no terminal event after it (the process died).
        let events = [run_event("r1", "run_started", json!({ "run_id": "r1" }))];
        assert_eq!(
            dangling_run_from_events(&events),
            Some(("r1".to_string(), RunState::Running))
        );
    }

    #[test]
    fn dangling_run_trusts_a_graceful_quit_mid_run() {
        // end_session records the in-flight run via active_run_id + run_state.
        let events = [
            run_event("r1", "run_started", json!({ "run_id": "r1" })),
            HistoryEvent::new(
                "s",
                None,
                None,
                "session_ended",
                json!({ "run_state": RunState::Running, "active_run_id": "r1" }),
            ),
        ];
        assert_eq!(
            dangling_run_from_events(&events),
            Some(("r1".to_string(), RunState::Running))
        );
    }

    #[test]
    fn dangling_run_is_none_for_a_clean_between_runs_quit() {
        // A run completed, then the user quit while Idle: session_ended carries a
        // null active_run_id and must not resurrect the just-closed run.
        let events = [
            run_event("r1", "run_started", json!({ "run_id": "r1" })),
            run_event("r1", "run_completed", json!({ "summary": "done" })),
            HistoryEvent::new(
                "s",
                None,
                None,
                "session_ended",
                json!({ "run_state": RunState::Idle, "active_run_id": null }),
            ),
        ];
        assert_eq!(dangling_run_from_events(&events), None);
    }

    #[test]
    fn dangling_run_tracks_only_the_most_recent_run() {
        // First run completed; a second started and never closed → the second is
        // the dangling one.
        let events = [
            run_event("r1", "run_started", json!({ "run_id": "r1" })),
            run_event("r1", "run_completed", json!({ "summary": "done" })),
            run_event("r2", "run_started", json!({ "run_id": "r2" })),
        ];
        assert_eq!(
            dangling_run_from_events(&events),
            Some(("r2".to_string(), RunState::Running))
        );
    }

    #[test]
    fn dangling_run_is_none_when_session_ended_recorded_a_terminal_state() {
        // Quit captured a terminal run_state (e.g. right after failure): the
        // is_terminal() filter rejects it, so there is nothing to reconcile.
        let events = [
            run_event("r1", "run_started", json!({ "run_id": "r1" })),
            HistoryEvent::new(
                "s",
                None,
                None,
                "session_ended",
                json!({ "run_state": RunState::Failed, "active_run_id": "r1" }),
            ),
        ];
        assert_eq!(dangling_run_from_events(&events), None);
    }

    // ── Resume metrics (task_13) ──

    fn seed_metrics_session(working_dir: &Path, kinds: &[(&str, Value)]) {
        let store = HistoryStore::create(working_dir).unwrap();
        for (kind, payload) in kinds {
            let event = HistoryEvent::new(
                store.session_id(),
                Some("r".to_string()),
                None,
                *kind,
                payload.clone(),
            );
            store.append_event(&event).unwrap();
        }
    }

    fn event_at(kind: &str, timestamp: &str) -> HistoryEvent {
        HistoryEvent {
            schema_version: 1,
            event_id: "e".to_string(),
            session_id: "s".to_string(),
            run_id: None,
            group_id: None,
            graph_id: None,
            step_id: None,
            timestamp: timestamp.to_string(),
            kind: kind.to_string(),
            payload: json!({}),
            payload_truncated: false,
        }
    }

    #[test]
    fn resume_metrics_fold_crash_recovery_and_completion() {
        let dir = tempdir().unwrap();
        // 1: crashed (interrupted), never resumed.
        seed_metrics_session(
            dir.path(),
            &[
                ("run_started", json!({ "run_id": "r" })),
                ("run_interrupted", json!({ "run_id": "r" })),
            ],
        );
        // 2: crashed + resumed, not completed.
        seed_metrics_session(
            dir.path(),
            &[
                ("run_interrupted", json!({})),
                ("session_resumed", json!({})),
            ],
        );
        // 3: crashed + resumed + completed.
        seed_metrics_session(
            dir.path(),
            &[
                ("run_interrupted", json!({})),
                ("session_resumed", json!({})),
                ("run_completed", json!({})),
            ],
        );
        // 4: clean run, never crashed/resumed → counts in nothing.
        seed_metrics_session(
            dir.path(),
            &[
                ("run_started", json!({ "run_id": "r" })),
                ("run_completed", json!({})),
            ],
        );

        let metrics = resume_metrics(&dir.path().join(".atelier"));
        assert_eq!(metrics.crashed, 3, "sessions 1–3 ended non-terminally");
        assert_eq!(metrics.recovered, 2, "sessions 2 and 3 were resumed");
        assert_eq!(metrics.resumed, 2);
        assert_eq!(metrics.resumed_completed, 1, "only session 3 completed");
        assert_eq!(metrics.crash_recovery_rate(), Some(2.0 / 3.0));
        assert_eq!(metrics.resumed_completion_rate(), Some(0.5));
    }

    #[test]
    fn resume_metrics_on_empty_root_are_zero_with_undefined_rates() {
        let dir = tempdir().unwrap();
        let metrics = resume_metrics(&dir.path().join(".atelier"));
        assert_eq!(metrics, ResumeMetrics::default());
        assert_eq!(metrics.crash_recovery_rate(), None);
        assert_eq!(metrics.resumed_completion_rate(), None);
    }

    #[test]
    fn time_to_continue_is_resume_to_next_prompt_delta() {
        let events = [
            event_at("run_interrupted", "2026-06-17T10:00:00.000Z"),
            event_at("session_resumed", "2026-06-17T10:00:02.000Z"),
            event_at("prompt_submitted", "2026-06-17T10:00:07.000Z"),
        ];
        // 10:00:02 → 10:00:07 = 5s.
        assert_eq!(
            time_to_continue(&events).map(|delta| delta.num_seconds()),
            Some(5)
        );

        // A session never resumed has no time-to-continue.
        let unresumed = [event_at("prompt_submitted", "2026-06-17T10:00:00.000Z")];
        assert_eq!(time_to_continue(&unresumed), None);

        // Resumed but no following prompt yet (still idle after reopening).
        let no_prompt = [event_at("session_resumed", "2026-06-17T10:00:00.000Z")];
        assert_eq!(time_to_continue(&no_prompt), None);
    }

    #[test]
    fn reads_legacy_jsonl_events_without_group_id() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let path = store.session_dir().join("events.jsonl");
        fs::write(
            &path,
            r#"{"schema_version":1,"event_id":"event","session_id":"session","run_id":"run","step_id":"step","timestamp":"2026-06-06T00:00:00.000Z","kind":"agent_result","payload":{}}"#,
        )
        .unwrap();

        let events = read_events_from_path(&path).unwrap();

        assert_eq!(events[0].group_id, None);
        assert!(!events[0].payload_truncated);
    }

    // ── execution-graph (DAG) events + graph_id (task_03) ──

    #[test]
    fn reads_legacy_jsonl_events_without_graph_id() {
        // A pre-DAG line (no graph_id column) deserializes with graph_id == None
        // at schema_version 1 — the DAG is additive, no schema bump.
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let path = store.session_dir().join("events.jsonl");
        fs::write(
            &path,
            r#"{"schema_version":1,"event_id":"event","session_id":"session","run_id":"run","group_id":null,"step_id":"step","timestamp":"2026-06-06T00:00:00.000Z","kind":"agent_result","payload":{}}"#,
        )
        .unwrap();

        let events = read_events_from_path(&path).unwrap();
        assert_eq!(events[0].graph_id, None);
    }

    #[test]
    fn graph_id_round_trips_through_append_and_read() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let event = HistoryEvent::new_with_graph(
            store.session_id().to_string(),
            Some("run".to_string()),
            Some("graph-1".to_string()),
            None,
            EXECUTION_GRAPH_PROPOSED_KIND,
            json!({ "graph_id": "graph-1", "nodes": [] }),
        );
        store.append_event(&event).unwrap();
        let events = store.read_events().unwrap();
        assert_eq!(events, vec![event]);
        assert_eq!(events[0].graph_id.as_deref(), Some("graph-1"));
        // new_with_graph keeps the group path clear (graph events aren't mis-keyed).
        assert_eq!(events[0].group_id, None);
    }

    #[test]
    fn append_debug_event_includes_graph_id_in_lockstep() {
        // append_debug_event hand-builds its JSON; it must carry graph_id or the
        // debug log silently drops it (the lockstep risk called out in the spec).
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let event = HistoryEvent::new_with_graph(
            store.session_id().to_string(),
            Some("run".to_string()),
            Some("graph-7".to_string()),
            None,
            NODE_RUNNING_KIND,
            json!({ "graph_id": "graph-7", "node_id": "a" }),
        );
        store.append_debug_event(&event).unwrap();

        let debug = fs::read_to_string(store.root.join("debug.log")).unwrap();
        let record: Value = serde_json::from_str(debug.trim()).unwrap();
        assert_eq!(record["graph_id"], json!("graph-7"));
        assert_eq!(record["kind"], json!(NODE_RUNNING_KIND));
    }

    #[test]
    fn mixed_legacy_parallel_and_dag_events_all_parse() {
        // Old parallel_* events and new node_*/execution_graph_* events coexist in
        // one events.jsonl at schema_version 1.
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let session = store.session_id().to_string();
        let legacy = HistoryEvent::new_with_group(
            session.clone(),
            Some("run".to_string()),
            Some("group-1".to_string()),
            None,
            "parallel_group_started",
            json!({ "group_id": "group-1" }),
        );
        let proposed = HistoryEvent::new_with_graph(
            session.clone(),
            Some("run".to_string()),
            Some("graph-1".to_string()),
            None,
            EXECUTION_GRAPH_PROPOSED_KIND,
            json!({ "graph_id": "graph-1" }),
        );
        let node = HistoryEvent::new_with_graph(
            session.clone(),
            Some("run".to_string()),
            Some("graph-1".to_string()),
            None,
            NODE_SUCCEEDED_KIND,
            json!({ "graph_id": "graph-1", "node_id": "a" }),
        );
        store.append_event(&legacy).unwrap();
        store.append_event(&proposed).unwrap();
        store.append_event(&node).unwrap();

        let events = store.read_events().unwrap();
        assert_eq!(events, vec![legacy, proposed, node]);
    }

    #[test]
    fn read_events_still_rejects_non_v1_schema() {
        // The schema gate is unchanged by the DAG additions.
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let path = store.session_dir().join("events.jsonl");
        fs::write(
            &path,
            r#"{"schema_version":2,"event_id":"e","session_id":"s","run_id":null,"step_id":null,"timestamp":"2026-06-06T00:00:00.000Z","kind":"node_running","payload":{}}"#,
        )
        .unwrap();
        let err = read_events_from_path(&path).unwrap_err();
        assert!(err
            .to_string()
            .contains("unsupported history schema_version 2"));
    }

    #[test]
    fn dag_event_sequence_replays_through_read_back() {
        // A full proposed → node_* → completed sequence replays cleanly, all
        // grouped under one graph_id.
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let session = store.session_id().to_string();
        let kinds = [
            EXECUTION_GRAPH_PROPOSED_KIND,
            EXECUTION_GRAPH_APPROVED_KIND,
            NODE_PENDING_KIND,
            NODE_READY_KIND,
            NODE_RUNNING_KIND,
            NODE_SUCCEEDED_KIND,
            EXECUTION_GRAPH_COMPLETED_KIND,
        ];
        for kind in kinds {
            let event = HistoryEvent::new_with_graph(
                session.clone(),
                Some("run".to_string()),
                Some("graph-9".to_string()),
                None,
                kind,
                json!({ "graph_id": "graph-9" }),
            );
            store.append_event(&event).unwrap();
        }

        let events = store.read_events().unwrap();
        let replayed: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(replayed, kinds);
        assert!(events
            .iter()
            .all(|e| e.graph_id.as_deref() == Some("graph-9")));
    }

    #[test]
    fn writes_artifact_with_sha256_metadata() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        let artifact = store
            .write_artifact("txt", "text/plain", b"hello", "contains_user_content")
            .unwrap();
        assert_eq!(artifact.byte_length, 5);
        assert_eq!(
            artifact.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert!(store.session_dir().join("artifacts").exists());
        // The reported path is workspace-relative (keeps the `.atelier/` prefix)
        // and resolves back to the file from the project root, so it is findable
        // and copyable.
        assert!(artifact.path.starts_with(".atelier/sessions/"));
        assert!(dir.path().join(&artifact.path).is_file());
    }

    #[test]
    fn first_approval_explainer_latch_defaults_unset_then_persists() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();

        // Fresh install: latch unset.
        assert!(!store.first_approval_explainer_shown());

        store.mark_first_approval_explainer_shown().unwrap();
        assert!(store.first_approval_explainer_shown());

        // Persists across sessions: a new store on the same workspace root (a
        // fresh session dir) still sees the latch — the flag lives at the root.
        let next_session = HistoryStore::create(dir.path()).unwrap();
        assert!(next_session.first_approval_explainer_shown());
        // And it survives session cleanup (the flag is not under sessions/).
        clean_sessions(dir.path()).unwrap();
        let after_clean = HistoryStore::create(dir.path()).unwrap();
        assert!(after_clean.first_approval_explainer_shown());
    }

    #[test]
    fn marking_first_approval_explainer_is_idempotent() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        store.mark_first_approval_explainer_shown().unwrap();
        // Repeat calls are a no-op and keep the latch set.
        store.mark_first_approval_explainer_shown().unwrap();
        assert!(store.first_approval_explainer_shown());
    }

    #[test]
    fn first_approval_latch_degrades_to_unset_on_corrupt_file() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::create(dir.path()).unwrap();
        // Latch once so the flag file exists at its real (root-agnostic) path,
        // then corrupt it in place.
        store.mark_first_approval_explainer_shown().unwrap();
        let flag_path = store.ui_state_path();
        fs::write(&flag_path, "not json").unwrap();
        // A corrupt flag file must not panic or error — it reads as not-yet-shown.
        assert!(!store.first_approval_explainer_shown());
    }

    fn prompt_event(timestamp: &str, prompt: &str) -> HistoryEvent {
        let mut event = HistoryEvent::new(
            "session",
            None,
            None,
            "prompt_submitted",
            json!({ "prompt": prompt }),
        );
        event.timestamp = timestamp.to_string();
        event
    }

    fn write_session(root: &Path, session_id: &str, events: &[HistoryEvent]) {
        let dir = root.join("sessions").join(session_id);
        fs::create_dir_all(&dir).unwrap();
        let mut contents = String::new();
        for event in events {
            contents.push_str(&serde_json::to_string(event).unwrap());
            contents.push('\n');
        }
        fs::write(dir.join("events.jsonl"), contents).unwrap();
    }

    #[test]
    fn lists_session_event_paths_and_empty_when_sessions_absent() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");

        // No sessions/ yet → empty, no error.
        assert!(list_session_event_paths(&root).unwrap().is_empty());

        write_session(
            &root,
            "a",
            &[prompt_event("2026-06-06T00:00:00.000Z", "one")],
        );
        write_session(
            &root,
            "b",
            &[prompt_event("2026-06-06T00:00:01.000Z", "two")],
        );
        // A session directory without an events.jsonl is ignored.
        fs::create_dir_all(root.join("sessions").join("c")).unwrap();

        let paths = list_session_event_paths(&root).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths
            .iter()
            .all(|path| path.file_name().unwrap() == "events.jsonl"));
    }

    #[test]
    fn projects_prompts_newest_first_across_sessions() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        write_session(
            &root,
            "a",
            &[
                prompt_event("2026-06-06T00:00:00.000Z", "one"),
                prompt_event("2026-06-06T00:00:02.000Z", "two"),
            ],
        );
        write_session(
            &root,
            "b",
            &[prompt_event("2026-06-06T00:00:01.000Z", "three")],
        );

        let history = project_prompt_history(&root, 10);
        assert_eq!(history, vec!["two", "three", "one"]);
    }

    #[test]
    fn collapses_consecutive_duplicates_but_keeps_non_consecutive() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        // Sorted newest-first: x(t4), x(t3), y(t2), x(t1).
        write_session(
            &root,
            "a",
            &[
                prompt_event("2026-06-06T00:00:01.000Z", "x"),
                prompt_event("2026-06-06T00:00:02.000Z", "y"),
                prompt_event("2026-06-06T00:00:03.000Z", "x"),
                prompt_event("2026-06-06T00:00:04.000Z", "x"),
            ],
        );

        // The consecutive x/x collapses to one; the earlier x (with y between) stays.
        assert_eq!(project_prompt_history(&root, 10), vec!["x", "y", "x"]);
    }

    #[test]
    fn truncates_to_max_keeping_newest() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        write_session(
            &root,
            "a",
            &[
                prompt_event("2026-06-06T00:00:01.000Z", "p1"),
                prompt_event("2026-06-06T00:00:02.000Z", "p2"),
                prompt_event("2026-06-06T00:00:03.000Z", "p3"),
                prompt_event("2026-06-06T00:00:04.000Z", "p4"),
                prompt_event("2026-06-06T00:00:05.000Z", "p5"),
            ],
        );

        assert_eq!(project_prompt_history(&root, 2), vec!["p5", "p4"]);
    }

    #[test]
    fn excludes_leading_space_prompts() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        write_session(
            &root,
            "a",
            &[
                prompt_event("2026-06-06T00:00:01.000Z", "visible"),
                prompt_event("2026-06-06T00:00:02.000Z", " secret"),
            ],
        );

        assert_eq!(project_prompt_history(&root, 10), vec!["visible"]);
    }

    #[test]
    fn skips_unreadable_or_legacy_files_without_emptying_result() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        write_session(
            &root,
            "good",
            &[prompt_event("2026-06-06T00:00:01.000Z", "valid")],
        );
        // A future schema version errors in read_events_from_path → file skipped.
        fs::create_dir_all(root.join("sessions").join("legacy")).unwrap();
        fs::write(
            root.join("sessions").join("legacy").join("events.jsonl"),
            r#"{"schema_version":2,"event_id":"e","session_id":"s","run_id":null,"step_id":null,"timestamp":"2026-06-06T00:00:09.000Z","kind":"prompt_submitted","payload":{"prompt":"from-future"}}"#,
        )
        .unwrap();
        // A malformed line also fails its file's read → file skipped.
        fs::create_dir_all(root.join("sessions").join("garbage")).unwrap();
        fs::write(
            root.join("sessions").join("garbage").join("events.jsonl"),
            "not json at all\n",
        )
        .unwrap();

        // The bad files are skipped; the valid prompt still surfaces.
        assert_eq!(project_prompt_history(&root, 10), vec!["valid"]);
    }

    #[test]
    fn ignores_non_prompt_events_and_missing_prompt_field() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        let mut other = HistoryEvent::new("session", None, None, "run_started", json!({}));
        other.timestamp = "2026-06-06T00:00:09.000Z".to_string();
        let mut no_prompt = HistoryEvent::new(
            "session",
            None,
            None,
            "prompt_submitted",
            json!({ "other": 1 }),
        );
        no_prompt.timestamp = "2026-06-06T00:00:08.000Z".to_string();
        write_session(
            &root,
            "a",
            &[
                other,
                no_prompt,
                prompt_event("2026-06-06T00:00:01.000Z", "real"),
            ],
        );

        assert_eq!(project_prompt_history(&root, 10), vec!["real"]);
    }

    #[test]
    fn missing_sessions_dir_projects_empty() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        assert!(project_prompt_history(&root, 10).is_empty());
    }

    #[test]
    fn cleanup_deletes_only_sessions_and_runs() {
        let dir = tempdir().unwrap();
        let root = dir.path().join(".atelier");
        fs::create_dir_all(root.join("sessions/a")).unwrap();
        fs::create_dir_all(root.join("runs")).unwrap();
        fs::write(root.join("debug.log"), "keep").unwrap();

        let deleted = clean_sessions(dir.path()).unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(!root.join("sessions").exists());
        assert!(!root.join("runs").exists());
        assert!(root.join("debug.log").exists());
    }
}
