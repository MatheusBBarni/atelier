//! The off-thread hook dispatcher (ADR-003). A long-lived task drains a bounded
//! channel of [`HookDispatch`] items — fed by the non-blocking event tap
//! (task_05) — and executes each matched handler off the worker thread: a
//! `command` runs as a timed subprocess with the normalized payload on **stdin
//! only** (never argv), or the [`Notifier`] (task_03) fires for a `notify`
//! handler. Every execution is bracketed by `hook_started` / `hook_completed`
//! records sent back to the worker over a channel — the dispatcher holds no
//! `&App` and never calls `record_event` directly (task_05 records them).
//!
//! Delivery is **best-effort**: the inbound channel is bounded and the tap drops
//! (incrementing [`DroppedHookCounter`]) when it is full, so the write path never
//! blocks. Every hook is bounded by a timeout and reaped via `kill_on_drop`, so a
//! slow or hung hook can never stall the dispatcher.

use super::{
    notification_text, HookAction, HookHandler, HookLifecyclePayload, HookPayload, Notifier,
};
use crate::runtime::redact_sensitive_text;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::mpsc;

/// Capacity of the bounded tap → dispatcher channel. Sized to absorb a normal
/// burst of events; beyond it the tap drops (ADR-003 best-effort delivery).
pub const HOOK_CHANNEL_CAPACITY: usize = 256;

/// Default per-hook timeout when the caller (task_05) does not override it.
pub const DEFAULT_HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest redacted stderr/stdout excerpt recorded on a failed command hook.
const EXCERPT_LIMIT: usize = 512;

/// One unit of work for the dispatcher: a normalized payload (with its body
/// populated, so per-handler `metadata`/`full` detail is applied here) plus the
/// handlers it matched at the tap.
#[derive(Clone, Debug)]
pub struct HookDispatch {
    pub payload: HookPayload,
    pub handlers: Vec<MatchedHandler>,
}

/// A handler that matched the event, carrying its index in the effective
/// `HooksConfig.handlers` list (recorded in the lifecycle events).
#[derive(Clone, Debug)]
pub struct MatchedHandler {
    pub index: usize,
    pub handler: HookHandler,
}

/// Whether a lifecycle record opens (`hook_started`) or closes (`hook_completed`)
/// a hook execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookLifecycleKind {
    Started,
    Completed,
}

impl HookLifecycleKind {
    /// The internal history-event kind. Deliberately outside the public
    /// vocabulary, so these never re-trigger hooks (structural recursion guard).
    pub fn event_kind(self) -> &'static str {
        match self {
            HookLifecycleKind::Started => "hook_started",
            HookLifecycleKind::Completed => "hook_completed",
        }
    }
}

/// A lifecycle record sent back to the worker to be recorded as a history event
/// (task_05). Carries the correlation ids copied from the triggering payload so
/// the worker can record it without resolving anything.
#[derive(Clone, Debug, PartialEq)]
pub struct HookLifecycleRecord {
    pub kind: HookLifecycleKind,
    pub session_id: String,
    pub run_id: Option<String>,
    pub step_id: Option<String>,
    pub payload: HookLifecyclePayload,
}

/// Shared best-effort drop counter, incremented by the tap via [`try_dispatch`]
/// when the channel is full. Surfaced by `--doctor` (task_08).
#[derive(Clone, Debug, Default)]
pub struct DroppedHookCounter(Arc<AtomicU64>);

impl DroppedHookCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    pub fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Create the bounded tap → dispatcher channel at [`HOOK_CHANNEL_CAPACITY`].
pub fn hook_channel() -> (mpsc::Sender<HookDispatch>, mpsc::Receiver<HookDispatch>) {
    mpsc::channel(HOOK_CHANNEL_CAPACITY)
}

/// Best-effort enqueue from the tap: `try_send`; on a full (or closed) channel,
/// increment the drop counter and return immediately — never block the write
/// path (ADR-003). Used by task_05's tap.
pub fn try_dispatch(
    sender: &mpsc::Sender<HookDispatch>,
    item: HookDispatch,
    dropped: &DroppedHookCounter,
) {
    if sender.try_send(item).is_err() {
        dropped.increment();
    }
}

/// The dispatcher task: drain `items` until the channel closes, executing each
/// matched handler and emitting `hook_started` / `hook_completed` over
/// `lifecycle_tx`. Holds no `&App`; lives beside `run_app_worker` (spawned by
/// task_05). Returns when the inbound channel closes.
pub async fn run_hook_dispatcher(
    mut items: mpsc::Receiver<HookDispatch>,
    lifecycle_tx: mpsc::Sender<HookLifecycleRecord>,
    notifier: Arc<dyn Notifier>,
    timeout: Duration,
) {
    while let Some(dispatch) = items.recv().await {
        for matched in &dispatch.handlers {
            dispatch_one(
                &dispatch.payload,
                matched,
                &notifier,
                &lifecycle_tx,
                timeout,
            )
            .await;
        }
    }
}

/// Run one matched handler: emit `hook_started`, execute (bounded by `timeout`),
/// then emit `hook_completed` with status / duration / exit code.
async fn dispatch_one(
    payload: &HookPayload,
    matched: &MatchedHandler,
    notifier: &Arc<dyn Notifier>,
    lifecycle_tx: &mpsc::Sender<HookLifecycleRecord>,
    timeout: Duration,
) {
    let action = matched.handler.action.kind_str();

    // hook_started — best-effort send; a closed back-channel just means the
    // worker is gone, so dropping is correct.
    let _ = lifecycle_tx
        .send(lifecycle_record(
            HookLifecycleKind::Started,
            payload,
            matched.index,
            action,
            None,
            None,
            None,
            None,
        ))
        .await;

    let started = Instant::now();
    let outcome = execute_handler(&matched.handler, payload, notifier, timeout).await;
    let duration_ms = started.elapsed().as_millis() as u64;

    let _ = lifecycle_tx
        .send(lifecycle_record(
            HookLifecycleKind::Completed,
            payload,
            matched.index,
            action,
            Some(outcome.status.as_str().to_string()),
            Some(duration_ms),
            outcome.exit_code,
            outcome.excerpt,
        ))
        .await;
}

/// Status of a finished hook, mapped to the `status` field of `hook_completed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HookStatus {
    Ok,
    Error,
    Timeout,
}

impl HookStatus {
    fn as_str(self) -> &'static str {
        match self {
            HookStatus::Ok => "ok",
            HookStatus::Error => "error",
            HookStatus::Timeout => "timeout",
        }
    }
}

struct HookOutcome {
    status: HookStatus,
    exit_code: Option<i32>,
    excerpt: Option<String>,
}

async fn execute_handler(
    handler: &HookHandler,
    payload: &HookPayload,
    notifier: &Arc<dyn Notifier>,
    timeout: Duration,
) -> HookOutcome {
    match &handler.action {
        HookAction::Command(command) => {
            let stdin = payload_stdin(payload, handler.payload);
            run_command_hook(command, &stdin, timeout).await
        }
        HookAction::Notify(_) => run_notify_hook(notifier, payload, timeout).await,
    }
}

/// The exact bytes written to a command hook's stdin: the payload projected to
/// the handler's detail (body stripped for `metadata`), serialized, then
/// redacted — so no secret reaches the hook process (ADR-001).
fn payload_stdin(payload: &HookPayload, detail: super::PayloadDetail) -> String {
    let projected = match detail {
        super::PayloadDetail::Metadata => {
            let mut projected = payload.clone();
            projected.body = None;
            projected
        }
        super::PayloadDetail::Full => payload.clone(),
    };
    let json = serde_json::to_string(&projected).unwrap_or_else(|_| "{}".to_string());
    redact_sensitive_text(&json)
}

/// Run a `command` hook as `sh -c <command>` (the command is user-authored,
/// user-scope config — see ADR-001), piping `stdin_payload` to **stdin only**.
/// Bounded by `timeout`: on expiry the owning future is dropped and
/// `kill_on_drop` reaps the child. stdout/stderr are captured and redacted; a
/// short excerpt is recorded only on failure.
async fn run_command_hook(command: &str, stdin_payload: &str, timeout: Duration) -> HookOutcome {
    let spawned = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            return HookOutcome {
                status: HookStatus::Error,
                exit_code: None,
                excerpt: Some(redact_excerpt(&format!("failed to spawn hook: {err}"))),
            }
        }
    };

    let payload_line = format!("{stdin_payload}\n");
    // `async move` owns the child, so a timeout that drops this future also drops
    // the child → kill_on_drop reaps it. Writing stdin lives inside the timed
    // future too, so even a child that never drains stdin cannot block us.
    let exec = async move {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload_line.as_bytes()).await;
            // stdin dropped here → EOF for the hook.
        }
        child.wait_with_output().await
    };

    let output = tokio::select! {
        _ = tokio::time::sleep(timeout) => {
            return HookOutcome { status: HookStatus::Timeout, exit_code: None, excerpt: None };
        }
        result = exec => result,
    };

    match output {
        Ok(output) if output.status.success() => HookOutcome {
            status: HookStatus::Ok,
            exit_code: output.status.code(),
            excerpt: None,
        },
        Ok(output) => {
            let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
            HookOutcome {
                status: HookStatus::Error,
                exit_code: output.status.code(),
                excerpt: Some(redact_excerpt(&combined)),
            }
        }
        Err(err) => HookOutcome {
            status: HookStatus::Error,
            exit_code: None,
            excerpt: Some(redact_excerpt(&format!("hook io error: {err}"))),
        },
    }
}

/// Fire a `notify` handler via the injected [`Notifier`] (task_03), bounded by
/// the same timeout. The notifier is synchronous (it may spawn a fallback
/// process), so it runs on a blocking thread and is raced against the timeout.
async fn run_notify_hook(
    notifier: &Arc<dyn Notifier>,
    payload: &HookPayload,
    timeout: Duration,
) -> HookOutcome {
    let (title, body) = notification_text(payload);
    let notifier = Arc::clone(notifier);
    let task = tokio::task::spawn_blocking(move || notifier.notify(&title, &body));

    tokio::select! {
        _ = tokio::time::sleep(timeout) => HookOutcome {
            status: HookStatus::Timeout,
            exit_code: None,
            excerpt: None,
        },
        joined = task => match joined {
            Ok(Ok(())) => HookOutcome { status: HookStatus::Ok, exit_code: None, excerpt: None },
            Ok(Err(err)) => HookOutcome {
                status: HookStatus::Error,
                exit_code: None,
                excerpt: Some(redact_excerpt(&err.to_string())),
            },
            Err(_) => HookOutcome { status: HookStatus::Error, exit_code: None, excerpt: None },
        },
    }
}

/// Redact and cap a stderr/stdout excerpt for the lifecycle record.
fn redact_excerpt(text: &str) -> String {
    redact_sensitive_text(text.trim())
        .chars()
        .take(EXCERPT_LIMIT)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn lifecycle_record(
    kind: HookLifecycleKind,
    payload: &HookPayload,
    handler_index: usize,
    action: &str,
    status: Option<String>,
    duration_ms: Option<u64>,
    exit_code: Option<i32>,
    stderr_excerpt: Option<String>,
) -> HookLifecycleRecord {
    HookLifecycleRecord {
        kind,
        session_id: payload.session_id.clone(),
        run_id: payload.run_id.clone(),
        step_id: payload.step_id.clone(),
        payload: HookLifecyclePayload {
            handler_index,
            action: action.to_string(),
            event: payload.event.clone(),
            status,
            duration_ms,
            exit_code,
            stderr_excerpt,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryEvent;
    use crate::hooks::{normalize, ActorCtx, NotifyConfig, PayloadDetail};
    use tempfile::tempdir;

    fn payload(kind: &str) -> HookPayload {
        let mut event = HistoryEvent::new(
            "sess-1",
            Some("run-1".to_string()),
            None,
            kind,
            serde_json::json!({}),
        );
        event.timestamp = "2026-06-17T00:00:00.000Z".to_string();
        normalize(&event, ActorCtx::default(), PayloadDetail::Metadata).unwrap()
    }

    fn command_handler(index: usize, command: &str) -> MatchedHandler {
        MatchedHandler {
            index,
            handler: HookHandler {
                on: vec!["run_completed".to_string()],
                action: HookAction::Command(command.to_string()),
                payload: PayloadDetail::Metadata,
            },
        }
    }

    /// Drive a set of dispatch items through the dispatcher to completion and
    /// return every lifecycle record emitted, in order.
    async fn drive(
        items: Vec<HookDispatch>,
        notifier: Arc<dyn Notifier>,
        timeout: Duration,
    ) -> Vec<HookLifecycleRecord> {
        let (item_tx, item_rx) = mpsc::channel(16);
        // Large back-channel so the dispatcher never blocks before we drain.
        let (life_tx, mut life_rx) = mpsc::channel(256);
        let handle = tokio::spawn(run_hook_dispatcher(item_rx, life_tx, notifier, timeout));
        for item in items {
            item_tx.send(item).await.unwrap();
        }
        drop(item_tx);
        handle.await.unwrap();
        let mut records = Vec::new();
        while let Some(record) = life_rx.recv().await {
            records.push(record);
        }
        records
    }

    /// A notifier that records how many times it was invoked.
    struct CountingNotifier(Arc<AtomicU64>);
    impl Notifier for CountingNotifier {
        fn notify(&self, _title: &str, _body: &str) -> anyhow::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A no-op notifier for command-only tests.
    struct NoopNotifier;
    impl Notifier for NoopNotifier {
        fn notify(&self, _title: &str, _body: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn command_hook_receives_exact_normalized_json_on_stdin() {
        let dir = tempdir().unwrap();
        let sentinel = dir.path().join("stdin.json");
        let payload = payload("run_completed");
        let command = format!("cat > {}", sentinel.display());
        let dispatch = HookDispatch {
            payload: payload.clone(),
            handlers: vec![command_handler(0, &command)],
        };

        drive(vec![dispatch], Arc::new(NoopNotifier), DEFAULT_HOOK_TIMEOUT).await;

        let written = std::fs::read_to_string(&sentinel).unwrap();
        let expected = format!(
            "{}\n",
            redact_sensitive_text(&serde_json::to_string(&payload).unwrap())
        );
        assert_eq!(written, expected);
        // Sanity: it really is the normalized payload (not empty).
        assert!(written.contains("\"event\":\"run_completed\""));
        assert!(written.contains("\"session_id\":\"sess-1\""));
    }

    #[tokio::test]
    async fn command_hook_exceeding_timeout_is_killed_and_reported() {
        let dispatch = HookDispatch {
            payload: payload("run_completed"),
            handlers: vec![command_handler(0, "sleep 30")],
        };
        let started = Instant::now();
        let records = drive(
            vec![dispatch],
            Arc::new(NoopNotifier),
            Duration::from_millis(150),
        )
        .await;
        // Returns promptly (well under the 30s sleep) — the hook was killed.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "dispatcher hung on a slow hook"
        );
        let completed = records
            .iter()
            .find(|r| r.kind == HookLifecycleKind::Completed)
            .unwrap();
        assert_eq!(completed.payload.status.as_deref(), Some("timeout"));
    }

    #[test]
    fn try_send_on_full_channel_increments_dropped_counter() {
        let (tx, _rx) = mpsc::channel(1);
        let dropped = DroppedHookCounter::new();
        let item = || HookDispatch {
            payload: payload("run_completed"),
            handlers: vec![command_handler(0, "true")],
        };
        // First fits in the capacity-1 channel; the second has nowhere to go.
        try_dispatch(&tx, item(), &dropped);
        try_dispatch(&tx, item(), &dropped);
        assert_eq!(dropped.count(), 1);
    }

    #[tokio::test]
    async fn notify_handler_invokes_injected_notifier_once() {
        let calls = Arc::new(AtomicU64::new(0));
        let notifier = Arc::new(CountingNotifier(calls.clone()));
        let dispatch = HookDispatch {
            payload: payload("run_completed"),
            handlers: vec![MatchedHandler {
                index: 0,
                handler: HookHandler {
                    on: vec!["run_completed".to_string()],
                    action: HookAction::Notify(NotifyConfig::default()),
                    payload: PayloadDetail::Metadata,
                },
            }],
        };
        let records = drive(vec![dispatch], notifier, DEFAULT_HOOK_TIMEOUT).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let completed = records
            .iter()
            .find(|r| r.kind == HookLifecycleKind::Completed)
            .unwrap();
        assert_eq!(completed.payload.action, "notify");
        assert_eq!(completed.payload.status.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn failed_hook_output_with_bearer_token_is_redacted() {
        let dispatch = HookDispatch {
            payload: payload("run_completed"),
            handlers: vec![command_handler(
                0,
                "echo 'Authorization: Bearer sk-supersecret123'; exit 1",
            )],
        };
        let records = drive(vec![dispatch], Arc::new(NoopNotifier), DEFAULT_HOOK_TIMEOUT).await;
        let completed = records
            .iter()
            .find(|r| r.kind == HookLifecycleKind::Completed)
            .unwrap();
        let excerpt = completed.payload.stderr_excerpt.as_deref().unwrap();
        assert!(excerpt.contains("Bearer <redacted>"), "excerpt: {excerpt}");
        assert!(
            !excerpt.contains("supersecret123"),
            "secret leaked: {excerpt}"
        );
    }

    #[tokio::test]
    async fn command_hook_emits_started_then_completed_with_exit_code() {
        let dispatch = HookDispatch {
            payload: payload("run_completed"),
            handlers: vec![command_handler(2, "true")],
        };
        let records = drive(vec![dispatch], Arc::new(NoopNotifier), DEFAULT_HOOK_TIMEOUT).await;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].kind, HookLifecycleKind::Started);
        assert_eq!(records[0].payload.handler_index, 2);
        assert_eq!(records[0].payload.event, "run_completed");
        assert_eq!(records[0].session_id, "sess-1");
        assert_eq!(records[1].kind, HookLifecycleKind::Completed);
        assert_eq!(records[1].payload.status.as_deref(), Some("ok"));
        assert_eq!(records[1].payload.exit_code, Some(0));
        assert!(records[1].payload.duration_ms.is_some());
    }
}
