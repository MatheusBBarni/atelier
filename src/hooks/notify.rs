//! The built-in notifier — the one V1 "battery" (ADR-002), delivered per
//! ADR-005. An injectable [`Notifier`] trait with two backends:
//!
//! - [`OscNotifier`] (default): writes a terminal OSC escape sequence, so a ping
//!   is rendered by the *terminal application* and therefore works locally even
//!   when atelier runs over SSH — no external binary, zero dependency.
//! - [`CommandNotifier`] (fallback): spawns a user-configured notifier command
//!   for terminals (or tmux configs) that strip OSC.
//!
//! tmux strips OSC 9/777 unless `set -g allow-passthrough on`; that is the
//! documented passthrough requirement, with `notify_fallback_command` (ADR-005)
//! as the deterministic escape hatch where OSC is unavailable.

use super::HookPayload;
use anyhow::{anyhow, bail, Context, Result};
use std::io::{self, Write};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// A notification backend. Injectable so the dispatcher (task_04) can swap a test
/// double, and unit tests assert the emitted sequence / constructed command
/// without real OS delivery. `Send + Sync` so it lives on the dispatcher task.
pub trait Notifier: Send + Sync {
    fn notify(&self, title: &str, body: &str) -> Result<()>;
}

/// BEL (`\x07`), the terminator for the OSC sequences used here.
const BEL: char = '\u{0007}';

/// Build the OSC 9 desktop-notification sequence: `ESC ] 9 ; <text> BEL`. OSC 9
/// carries a single text payload, so `title` and `body` are folded into one.
/// Rendered by the terminal application (not the remote host), so it works over
/// SSH (ADR-005). This is the default backend's wire format.
pub fn osc9_sequence(title: &str, body: &str) -> Vec<u8> {
    format!("\u{1b}]9;{}{BEL}", join_title_body(title, body)).into_bytes()
}

/// Build the OSC 777 `notify` variant: `ESC ] 777 ; notify ; <title> ; <body> BEL`.
/// Some terminals (urxvt, foot) take this title+body form instead; provided for
/// terminals/callers where it applies (ADR-005 "OSC 777 variant where
/// applicable"). The default [`OscNotifier`] emits OSC 9 only — auto-detecting
/// which a terminal supports is unreliable (ADR-005 rejects it) and emitting
/// both would double-notify where both are understood.
pub fn osc777_sequence(title: &str, body: &str) -> Vec<u8> {
    format!("\u{1b}]777;notify;{title};{body}{BEL}").into_bytes()
}

fn join_title_body(title: &str, body: &str) -> String {
    match (title.is_empty(), body.is_empty()) {
        (false, false) => format!("{title}: {body}"),
        (false, true) => title.to_string(),
        (true, false) => body.to_string(),
        (true, true) => String::new(),
    }
}

/// Default notifier: writes an OSC 9 sequence to a sink — the controlling
/// terminal by default. Generic over the sink (behind a `Mutex`, since the trait
/// method takes `&self`) so tests inject an in-memory buffer.
pub struct OscNotifier<W: Write + Send> {
    sink: Mutex<W>,
}

impl OscNotifier<io::Stderr> {
    /// The production notifier: OSC to the controlling terminal via stderr —
    /// the conventional escape-sequence sink, which is the TTY in an interactive
    /// run and is not consumed by piped stdout.
    pub fn to_terminal() -> Self {
        Self::with_sink(io::stderr())
    }
}

impl<W: Write + Send> OscNotifier<W> {
    pub fn with_sink(sink: W) -> Self {
        Self {
            sink: Mutex::new(sink),
        }
    }
}

impl<W: Write + Send> Notifier for OscNotifier<W> {
    fn notify(&self, title: &str, body: &str) -> Result<()> {
        let sequence = osc9_sequence(title, body);
        let mut sink = self
            .sink
            .lock()
            .map_err(|_| anyhow!("notifier sink mutex poisoned"))?;
        sink.write_all(&sequence)
            .context("failed to write OSC notification sequence")?;
        sink.flush().context("failed to flush OSC notification")?;
        Ok(())
    }
}

/// Runs a fully-resolved notifier argv. Injected so tests assert the constructed
/// command without spawning a real process.
pub type NotifyRunner = Arc<dyn Fn(&[String]) -> Result<()> + Send + Sync>;

/// Fallback notifier: spawns a user-configured notifier command (ADR-005), for
/// terminals/tmux that strip OSC. The command string is whitespace-split into
/// program + base args; the derived `title` and `body` are appended as the final
/// two arguments — never interpolated into a shell, so there is no command-
/// injection surface (ADR-001 posture).
pub struct CommandNotifier {
    command: String,
    runner: NotifyRunner,
}

impl CommandNotifier {
    /// Production notifier: spawns the command via `std::process::Command`,
    /// surfacing a non-zero exit (or spawn failure) as an error.
    pub fn new(command: String) -> Self {
        Self {
            command,
            runner: Arc::new(spawn_notifier_command),
        }
    }

    /// Inject a custom runner — tests assert the argv without real delivery.
    pub fn with_runner(command: String, runner: NotifyRunner) -> Self {
        Self { command, runner }
    }
}

impl Notifier for CommandNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<()> {
        let argv = build_notifier_argv(&self.command, title, body);
        if argv.is_empty() {
            bail!("notify_fallback_command is empty");
        }
        (self.runner)(&argv)
    }
}

/// Split the fallback command into program + base args and append `title` then
/// `body` as the final two arguments. Returns empty for a blank command.
pub fn build_notifier_argv(command: &str, title: &str, body: &str) -> Vec<String> {
    let mut argv: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    if argv.is_empty() {
        return argv;
    }
    argv.push(title.to_string());
    argv.push(body.to_string());
    argv
}

/// Default runner: spawn the argv (no shell), wait, and map a non-zero exit or
/// spawn failure to an error. A brief, bounded spawn — the heavyweight
/// kill_on_drop + timeout idiom for arbitrary `command` *hooks* lives in the
/// dispatcher (task_04); a notifier binary is expected to return promptly.
fn spawn_notifier_command(argv: &[String]) -> Result<()> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| anyhow!("empty notifier command"))?;
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn notifier command `{program}`"))?;
    if !status.success() {
        bail!("notifier command `{program}` exited with {status}");
    }
    Ok(())
}

/// Derive a notification `(title, body)` from a normalized [`HookPayload`]. The
/// title is a friendly event label; the body folds the available actor / target
/// / outcome fields, falling back to the session id so the ping is still
/// meaningful for a bare orchestrator event.
pub fn notification_text(payload: &HookPayload) -> (String, String) {
    (
        notification_title(&payload.event),
        notification_body(payload),
    )
}

fn notification_title(event: &str) -> String {
    let label = match event {
        "run_started" => "Run started",
        "step_started" => "Step started",
        "action_requested" => "Action requested",
        "approval_required" => "Approval required",
        "clarification_required" => "Input needed",
        "file_edited" => "File edited",
        "run_completed" => "Run completed",
        "run_failed" => "Run failed",
        "run_limit_reached" => "Run limit reached",
        "run_interrupted" => "Run interrupted",
        other => other,
    };
    format!("atelier — {label}")
}

fn notification_body(payload: &HookPayload) -> String {
    let mut parts = Vec::new();
    if let Some(agent) = &payload.actor.agent {
        parts.push(agent.clone());
    }
    if let Some(target) = &payload.target {
        parts.push(target.clone());
    }
    if let Some(outcome) = &payload.outcome {
        parts.push(outcome.clone());
    }
    if parts.is_empty() {
        parts.push(format!("session {}", payload.session_id));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryEvent;
    use crate::hooks::{normalize, ActorCtx, PayloadDetail};
    use serde_json::json;

    /// A `Write` sink backed by a shared buffer the test can inspect afterward.
    #[derive(Clone)]
    struct SharedSink(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedSink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn normalized(kind: &str, actor: ActorCtx, payload: serde_json::Value) -> HookPayload {
        let event = HistoryEvent::new("sess-1", Some("run-1".to_string()), None, kind, payload);
        normalize(&event, actor, PayloadDetail::Metadata).unwrap()
    }

    #[test]
    fn osc_notifier_writes_exact_osc9_sequence() {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let notifier = OscNotifier::with_sink(SharedSink(buf.clone()));
        notifier.notify("Run done", "ok").unwrap();
        // ESC ] 9 ; <title>: <body> BEL
        assert_eq!(buf.lock().unwrap().as_slice(), b"\x1b]9;Run done: ok\x07");
    }

    #[test]
    fn osc9_folds_title_and_body_and_handles_empties() {
        assert_eq!(osc9_sequence("", "ok"), b"\x1b]9;ok\x07");
        assert_eq!(osc9_sequence("Title", ""), b"\x1b]9;Title\x07");
    }

    #[test]
    fn osc777_variant_carries_title_and_body_separately() {
        assert_eq!(osc777_sequence("T", "B"), b"\x1b]777;notify;T;B\x07");
    }

    #[test]
    fn command_notifier_builds_expected_argv() {
        assert_eq!(
            build_notifier_argv("notify-send", "T", "B"),
            vec!["notify-send", "T", "B"]
        );
        // The base args of the command are preserved; title/body are appended.
        assert_eq!(
            build_notifier_argv("terminal-notifier -message", "T", "B"),
            vec!["terminal-notifier", "-message", "T", "B"]
        );
    }

    #[test]
    fn command_notifier_invokes_runner_with_constructed_argv() {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        let runner: NotifyRunner = Arc::new(move |argv: &[String]| {
            *sink.lock().unwrap() = argv.to_vec();
            Ok(())
        });
        let notifier = CommandNotifier::with_runner("notify-send".to_string(), runner);
        notifier.notify("Run done", "ok").unwrap();
        assert_eq!(
            captured.lock().unwrap().as_slice(),
            &[
                "notify-send".to_string(),
                "Run done".to_string(),
                "ok".to_string()
            ]
        );
    }

    #[test]
    fn command_notifier_non_zero_exit_surfaces_error() {
        // `false` exits non-zero (and ignores the appended args) — the default
        // runner must surface that as an error rather than panicking.
        let notifier = CommandNotifier::new("false".to_string());
        assert!(notifier.notify("title", "body").is_err());
    }

    #[test]
    fn command_notifier_spawn_failure_surfaces_error() {
        let notifier = CommandNotifier::new("definitely-not-a-real-binary-xyzzy".to_string());
        assert!(notifier.notify("title", "body").is_err());
    }

    #[test]
    fn notification_text_from_run_completed_has_sensible_defaults() {
        let payload = normalized(
            "run_completed",
            ActorCtx::default(),
            json!({ "summary": "done" }),
        );
        let (title, body) = notification_text(&payload);
        assert!(title.contains("Run completed"), "title was {title:?}");
        // No actor/target on a terminal orchestrator event → body is the outcome.
        assert_eq!(body, "success");
    }

    #[test]
    fn notification_body_folds_actor_target_and_outcome() {
        let actor = ActorCtx {
            agent: Some("coder".to_string()),
            runtime: Some("codex".to_string()),
        };
        let payload = normalized(
            "file_edit_applied",
            actor,
            json!({ "action_id": "a1", "operation": "write_file", "path": "src/x.rs" }),
        );
        let (title, body) = notification_text(&payload);
        assert!(title.contains("File edited"), "title was {title:?}");
        assert_eq!(body, "coder · src/x.rs · success");
    }
}
