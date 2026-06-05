# Technical Specification: Stream Mode

Status: Draft
Version: 1.0
Date: 2026-06-05
Source PRD: `docs/stream-mode/prd.md`

## Executive Summary

This specification defines the implementation plan for first-class stream mode in `multiagent`. Stream mode lets the active Specialized Agent publish progress into the TUI while an Execution Runtime step is still pending, without moving final output parsing, Harness Actions, Action Approval, or Session History ownership out of the Rust Harness.

The core change is replacing after-the-fact `RuntimeStepResult.stream_deltas` delivery with an app-consumed runtime event sink. Runtime adapters emit progress events through a bounded sink, the app loop updates `AppState.live_step` and coalesces durable history records, and the runtime step still returns one authoritative `RuntimeOutput`.

## References

- Product requirements: `docs/stream-mode/prd.md`
- Source planning doc: `docs/stream-mode.md`
- Codex integration techspec: `docs/codex-api/techspec.md`
- Codex integration planning doc: `docs/codex-api.md`
- TUI planning doc: `docs/tui-improvements.md`
- Domain glossary: `CONTEXT.md`
- OpenAI Streaming Responses: https://developers.openai.com/api/docs/guides/streaming-responses
- OpenAI Background mode: https://developers.openai.com/api/docs/guides/background

## 1. Background

The current runtime boundary is named `stream_step`, but concrete runtimes still produce stream deltas only after a step completes:

- `codex` waits for the child process to exit, then emits one final stdout delta.
- `zai` posts a non-streaming chat-completions request, then emits one final message delta.
- `fake` returns one final fake delta.
- The app records deltas through `record_runtime_stream_deltas` only after `execute_runtime_step` returns.

The TUI already has a live-step display shape through `AppState.live_step`, `LiveStepView`, and `LiveStreamView`. The missing piece is timing: live-step state is fed after runtime completion, so it cannot provide true streaming behavior yet.

The Codex subscription feature uses the local Codex CLI/app-server path, not a direct OpenAI Responses runtime. Stream mode should keep any future direct OpenAI API adapter separate and must not make OpenAI streaming a prerequisite for fake, Codex CLI, and Z.ai streaming.

## 2. Goals

- Publish active runtime output before runtime completion.
- Keep exactly one authoritative final `RuntimeOutput` per runtime step.
- Keep Capability Enforcement, Harness Actions, Action Approval, and Session History owned by the app/action/history layers.
- Preserve existing non-streaming behavior through a compatibility path while adapters migrate.
- Keep one active Run in v1.
- Support deterministic fake streaming first.
- Support Codex CLI stdout/stderr streaming.
- Support Z.ai/OpenAI-compatible SSE streaming where endpoint behavior allows it.
- Support future OpenAI Responses streaming through the same provider-neutral app event contract.
- Make cancellation visible, durable, and connected to active runtime work.
- Avoid token-level history flooding through coalesced durable stream events.

## 3. Non-Goals

- Building a web UI.
- Adding parallel active Runs.
- Letting runtimes execute file edits, shell commands, VCS actions, or other Harness Actions directly.
- Replacing the typed `OrchestratorDecision`, `AgentResult`, `ActionRequest`, and `ParseError` output contract.
- Implementing native OpenAI Responses function-call streaming in the first stream-mode pass.
- Implementing OpenAI background recovery as a dependency of basic stream mode.
- Reworking Session History outside runtime stream event payloads and coalescing behavior.
- Adding transcript search, copy commands, command palette behavior, or detail panes.

## 4. Current Architecture

### Runtime Contract

Current runtime trait:

```rust
#[async_trait]
pub trait Runtime: Send + Sync {
    async fn check_availability(&self) -> RuntimeAvailability;
    async fn stream_step(&self, request: RuntimeRequest) -> Result<RuntimeStepResult>;
}
```

`RuntimeStepResult` currently contains:

```rust
pub struct RuntimeStepResult {
    pub output: RuntimeOutput,
    pub stream_deltas: Vec<RuntimeStreamDelta>,
}
```

This shape makes stream deltas passive data attached to a completed result. It is the main contract that must change.

### App State

`AppState` already includes:

```rust
pub live_step: Option<LiveStepView>
```

with a live-step payload containing a run id, step id, agent id, and stream entries. The TUI renders that live step above durable Chat lines. This is the right TUI entry point, but the app needs to update it while the runtime future is pending.

### History

Session History uses append-only JSONL `HistoryEvent`s. The current `runtime_stream_delta` payload includes `agent`, `sequence`, `stream`, `final_delta`, and `content`, with large content spilled to artifact storage.

For true streaming, durable history must not append one event per token. It needs a coalescing layer before `HistoryStore::append_event`.

### Runtime Prior Art

The Z.ai adapter is the closest HTTP prior art for request building, bearer auth, retryable status classification, output parsing, and response redaction. The Codex CLI adapter is the prior art for child process execution, `kill_on_drop`, stdout/stderr handling, and final contract extraction. The fake runtime is the right deterministic harness for app-level stream tests.

## 5. Target Architecture

Stream mode adds three deep modules:

- Runtime event sink: provider-neutral interface adapters use to publish progress.
- Runtime step driver: app-owned loop that runs a runtime future and drains events until final output.
- Stream coalescer: app/history helper that aggregates small deltas into durable history records.

High-level flow:

```text
App sets active step
  -> App creates bounded RuntimeEvent channel and CancellationToken
  -> Runtime adapter emits progress into RuntimeEventSink
  -> App driver drains events while runtime is pending
  -> App updates AppState.live_step and StreamCoalescer
  -> Runtime returns one RuntimeOutput or error
  -> App flushes coalesced history
  -> App continues existing output/action/approval orchestration
```

The runtime adapter never writes Session History, mutates AppState, or executes Harness Actions directly.

## 6. Runtime Event Contract

### Type Shape

Add a provider-neutral runtime event type:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RuntimeEvent {
    Delta {
        sequence: u32,
        stream: String,
        content: String,
    },
    Status {
        sequence: u32,
        message: String,
    },
    ToolCallProgress {
        sequence: u32,
        name: String,
        summary: String,
    },
    Diagnostic {
        sequence: u32,
        stream: String,
        content: String,
    },
}
```

Do not include final `RuntimeOutput` in `RuntimeEvent`. The final output stays on the returned result path.

Do not include step-start events in `RuntimeEvent`. The app creates the active step before the runtime adapter starts, so `agent_step_started` history records, `LiveStepStatus::Starting`, and Agent Roster activation stay app-owned lifecycle events. Runtime adapters only emit progress after the step exists.

### Runtime Event Sink

Add a sink wrapper around a bounded `tokio::sync::mpsc::Sender<RuntimeEvent>`:

```rust
#[derive(Clone)]
pub struct RuntimeEventSink {
    sender: mpsc::Sender<RuntimeEvent>,
    next_sequence: Arc<AtomicU32>,
}
```

The sink should expose intent-specific methods instead of requiring adapters to construct events manually:

```rust
impl RuntimeEventSink {
    pub async fn delta(&self, stream: impl Into<String>, content: impl Into<String>) -> Result<()>;
    pub async fn status(&self, message: impl Into<String>) -> Result<()>;
    pub async fn diagnostic(
        &self,
        stream: impl Into<String>,
        content: impl Into<String>,
    ) -> Result<()>;
    pub async fn tool_call_progress(
        &self,
        name: impl Into<String>,
        summary: impl Into<String>,
    ) -> Result<()>;
}
```

The sink owns sequence assignment. This prevents concurrent stdout/stderr readers from producing duplicate or out-of-order sequence numbers through local counters.

### Channel Rules

- Use a bounded channel. Recommended initial capacity: 64 events.
- Sending should await when the channel is full to provide backpressure.
- If the receiver is closed because the Run was cancelled, sink methods should return a runtime cancellation-style error, not panic.
- The app driver may drop old UI-only content, but durable coalescing should preserve emitted content until flushed or artifacted.

## 7. Runtime Trait and Dispatch

### Target Trait

Change the runtime trait to:

```rust
#[async_trait]
pub trait Runtime: Send + Sync {
    async fn check_availability(&self) -> RuntimeAvailability;

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput>;
}
```

This makes the streaming path explicit: events are progress, return value is final output.

### Compatibility Helper

During migration, keep a compatibility helper for runtimes or tests that still produce completed deltas:

```rust
pub async fn emit_legacy_step_result(
    result: RuntimeStepResult,
    events: &RuntimeEventSink,
) -> Result<RuntimeOutput>;
```

This helper emits `result.stream_deltas` through the sink, then returns `result.output`. It lets Codex and Z.ai migrate in separate commits if needed.

### Runtime Dispatch Execution Function

Replace direct runtime dispatch calls with a streaming executor used by the app-owned step driver:

```rust
pub async fn execute_runtime_step_streaming(
    config: &EffectiveConfig,
    request: RuntimeRequest,
    events: RuntimeEventSink,
    cancellation: CancellationToken,
) -> Result<RuntimeOutput>;
```

It returns raw `RuntimeOutput` because it sits at the runtime boundary. App orchestration callers should use the `StepOutcome`-returning driver described below instead of calling this directly.

It must preserve:

- runtime existence checks;
- availability checks;
- model fallback chain;
- retryable provider error behavior;
- one final output or one error per attempt.

When model fallback occurs after a retryable provider error, the dispatcher should emit a status event before the next model attempt.

## 8. Runtime Step Driver

The app loop needs a driver that can wait for runtime completion and process events at the same time.

Suggested shape:

```rust
async fn drive_runtime_step_streaming(
    &mut self,
    request: RuntimeRequest,
    run: &RunDriveContext,
    step: &PausedStep,
    step_started_at: Instant,
) -> Result<StepOutcome>;
```

Behavior:

1. Create a bounded channel, `RuntimeEventSink`, and `CancellationToken`.
2. Start the runtime future.
3. Loop with `tokio::select!` over runtime completion, event receipt, and step deadline.
4. On event receipt, update live state and stream coalescer, then publish state.
5. On runtime success, drain remaining events, flush coalescer with final markers, and convert `RuntimeOutput` into the existing app-level `StepOutcome`.
6. On runtime error, drain available diagnostics, flush coalescer, and return the error.
7. On step time limit, cancel the runtime, record limit events, flush coalescer, and return the existing limit outcome.
8. On user interrupt, cancel the runtime, record cancellation events, mark live state interrupted, and stop processing the Run.

The current `await_with_step_limit` helper cannot be the only wait path for streamed steps because it blocks event draining until the runtime future resolves.

The driver must not return raw `RuntimeOutput` to app orchestration callers. Raw runtime output cannot represent limit-reached, waiting-for-approval, waiting-for-user, cancelled, or other app-owned step states. Keep `RuntimeOutput` as the runtime boundary and `StepOutcome` as the app orchestration boundary.

## 9. App State and TUI

### Live State Model

Evolve `LiveStepView` toward an appendable per-stream view:

```rust
pub struct LiveStepView {
    pub run_id: String,
    pub step_id: String,
    pub agent: String,
    pub status: LiveStepStatus,
    pub streams: Vec<LiveStreamView>,
}

pub enum LiveStepStatus {
    Starting,
    Streaming,
    WaitingForAction,
    WaitingForApproval,
    Cancelling,
    Interrupted,
    Completed,
    Failed,
}

pub struct LiveStreamView {
    pub stream: String,
    pub content: String,
    pub sequence_end: u32,
    pub final_delta: bool,
}
```

The app should append content by stream name rather than pushing an unbounded list of chunks. Keep a display cap per stream, initially 16 KB, and preserve full content through coalescer/history handling.

### Agent Roster Status

Agent Roster state should be driven from the same app-owned lifecycle transitions as `LiveStepView.status`:

| Lifecycle state | Agent roster status |
|---|---|
| Step starts | `running` |
| First runtime progress event arrives | `streaming` |
| Final output is `ActionRequest` and needs execution | `waiting_action` |
| Action requires approval | `waiting_approval` |
| Cancellation requested | `cancelling` |
| Cancellation completed | `interrupted` |
| Runtime or parsing fails | `failed` |
| Step completes without pending app work | `completed` |

Only the active agent for the current step should receive these statuses. When the step ends, non-active agents should return to their normal idle/available status without retaining stale streaming state.

### Event Handling

Map runtime events into state:

- `Delta`: append to the named stream and mark live step `Streaming`.
- `Diagnostic`: append to the diagnostic stream and mark live step `Streaming`.
- `Status`: append a concise status line or status stream entry.
- `ToolCallProgress`: append progress text but do not execute tools.

When an `ActionRequest` final output arrives, mark the live step `WaitingForAction` or `WaitingForApproval` before entering the existing action/approval flow.

### TUI Rendering

The current live block in Chat should remain the v1 rendering surface. Update it to show:

- active agent;
- run id and step id;
- live status;
- latest visible content by stream;
- final/interrupted marker when applicable.

Follow mode should keep Chat pinned to the bottom only when the user has not manually scrolled. Existing event scroll state can continue to own that behavior.

## 10. History and Coalescing

### Coalescer

Add a `RuntimeStreamCoalescer` owned by the app step driver:

```rust
pub struct RuntimeStreamCoalescer {
    agent: String,
    buffers: BTreeMap<String, StreamBuffer>,
    last_flush_at: Instant,
}
```

Flush rules:

- flush every 250 ms while content is arriving;
- flush when a stream buffer reaches 2 KB;
- flush immediately when final output arrives;
- flush immediately on runtime error, action request, cancellation, or time limit.

### Durable Event Payload

Continue using `runtime_stream_delta`, but change payloads to coalesced records:

```json
{
  "agent": "fixer",
  "sequence_start": 1,
  "sequence_end": 8,
  "stream": "stdout",
  "content": "chunked content",
  "final_delta": false,
  "coalesced": true
}
```

If content is too large, store it as an artifact and set `content` to null:

```json
{
  "agent": "fixer",
  "sequence_start": 9,
  "sequence_end": 19,
  "stream": "stdout",
  "content": null,
  "artifact": { "...": "..." },
  "final_delta": true,
  "coalesced": true
}
```

The current runtime-history compaction code should keep `agent`, `sequence_start`, `sequence_end`, `stream`, `final_delta`, `coalesced`, and `artifact`. It should not include large raw content in runtime context sent back to adapters.

### Compatibility

History readers should tolerate old payloads with `sequence` instead of `sequence_start` and `sequence_end`. New writers should use the coalesced shape.

## 11. Adapter Implementation

### Fake Runtime

Fake runtime is phase 1 and the primary deterministic proof.

Implementation:

- Emit two or three short `fake` stream deltas with small sleeps.
- Return the same final `RuntimeOutput` variants as today.
- Support cancellation by checking the cancellation token before each delayed emission.
- Preserve fake provider retry and parse-error behaviors.

Tests should prove app state observes at least one live stream update before final output handling.

### Codex CLI Runtime

Codex CLI streaming reads process output while the child is running.

Implementation:

- Spawn the configured command with piped stdin/stdout/stderr and `kill_on_drop(true)`.
- Write the prompt envelope to stdin and close it.
- Read stdout and stderr concurrently.
- Emit stdout chunks as `delta("stdout", chunk)`.
- Emit stderr chunks as diagnostics or a `stderr` stream.
- Accumulate full stdout for final structured contract parsing.
- Accumulate stderr for failure diagnostics.
- On process success, parse accumulated stdout only.
- On process failure, return an error using concise combined stdout/stderr diagnostics.
- On cancellation, kill the child, wait for exit, and return a cancellation error.

Reading by line is acceptable for phase 1. Byte chunks can be introduced if line buffering hides too much progress in practice.

### Z.ai Runtime

Z.ai streaming should be implemented after app plumbing and fake runtime are stable.

Implementation:

- Send `"stream": true` when stream mode is enabled for the adapter.
- Parse OpenAI-compatible chat-completions SSE frames.
- Map `choices[0].delta.content` to `delta("message", content)`.
- Accumulate full message content for final contract parsing.
- Treat keepalives and empty frames as no-ops.
- Treat provider error frames as diagnostics plus a returned provider error.
- If the endpoint rejects streaming and fallback is configured, retry once with `"stream": false` and emit a status event explaining the fallback.
- If fallback is not configured, fail clearly.

Do not make fallback silent. The user should be able to diagnose whether a runtime truly streamed.

### Future OpenAI Responses Runtime

A future direct OpenAI Platform API runtime can add Responses streaming after the provider-neutral sink exists. That runtime is not the Codex subscription path and is not part of the current Codex integration plan.

Implementation, when such an adapter exists:

- Send `stream = true` to the Responses endpoint when upgrading that adapter.
- Map semantic output-text delta events into runtime text deltas.
- Map created, in-progress, completed, failed, and error events into status or diagnostic runtime events.
- Buffer all final text required for contract parsing.
- Do not enable native function-call streaming in the first stream-mode pass.
- If background mode is later configured, preserve provider sequence cursors in history so streams can be resumed from the last seen provider cursor.

Synchronous OpenAI response cancellation is connection termination. Background response cancellation should use the provider cancellation endpoint only after background mode has explicit config and storage semantics.

## 12. Cancellation and Limits

Use `tokio_util::sync::CancellationToken`, already available in the dependency set, for app-to-runtime cancellation.

Cancellation behavior:

- TUI interrupt requests call the app interrupt path.
- App marks active live step `Cancelling`.
- App cancels the runtime token.
- Codex CLI kills the child process.
- HTTP runtimes abort the request by dropping the response future/client request path.
- Background OpenAI Responses runtimes later call the provider cancel endpoint when a response id exists.
- App records `step_cancel_requested`.
- App records `step_cancelled` on successful runtime stop or `step_cancel_failed` if a runtime cannot be stopped cleanly.
- App records `run_interrupted` and clears active run state.

Step time limits should use the same cancellation plumbing. A step limit is not a user interrupt, so its durable event kind should remain limit-specific.

## 13. Security and Privacy

- Runtimes emit progress only; they do not execute local effects.
- Harness Actions still flow through `ActionRequest`, capability checks, approvals, and action execution.
- Raw API keys must not appear in live output, diagnostics, or history.
- Existing redaction behavior for provider errors must remain in Z.ai and future OpenAI adapters.
- Codex stderr may contain useful diagnostics; treat it as user-visible content but continue concise display caps and artifact storage for large output.
- Session History may contain user prompts and model output. Continue writing files with private permissions through `HistoryStore`.
- OpenAI background mode requires storage-enabled provider behavior and must not be enabled implicitly.

## 14. Implementation Plan

### Phase 1: Internal Event Plumbing

- Add `RuntimeEvent`, `RuntimeEventSink`, and bounded channel creation.
- Change or extend the runtime trait to accept event sink and cancellation token.
- Add compatibility helper for current `RuntimeStepResult.stream_deltas`.
- Add app runtime step driver that drains events while runtime is pending.
- Make that driver return the existing app-level `StepOutcome`, not raw `RuntimeOutput`.
- Replace direct `execute_runtime_step` awaits in normal agent steps with the streaming driver.
- Keep council member steps on compatibility mode unless live council streaming is explicitly in scope for the same phase.

### Phase 2: Fake Runtime and App Tests

- Update fake runtime to emit delayed live deltas.
- Add tests that observe live `AppState` before final result.
- Add tests for runtime event ordering.
- Add tests for model fallback status events.
- Preserve existing fake action-request and approval flows.

### Phase 3: TUI and History Coalescing

- Update `LiveStepView` to aggregate streams by name.
- Update TUI render tests for live, final, interrupted, and pending approval states.
- Implement `RuntimeStreamCoalescer`.
- Write coalesced `runtime_stream_delta` payloads.
- Update runtime-history compaction to preserve new coalesced metadata.
- Preserve old payload compatibility in tests.

### Phase 4: Codex CLI Streaming

- Read stdout and stderr concurrently while the process runs.
- Emit live stdout and diagnostic/stderr events.
- Accumulate stdout for final parser.
- Preserve timeout and kill behavior.
- Add mock process tests proving stdout arrives before process exit.

### Phase 5: HTTP SSE Streaming

- Add reusable SSE parser for HTTP streaming adapters.
- Implement Z.ai streaming with explicit non-streaming fallback behavior.
- Add parser tests for normal frames, split frames, keepalives, provider errors, malformed JSON, and completion markers.
- Add mock HTTP tests proving live deltas arrive before final output.

### Phase 6: Future OpenAI Responses Integration

- If a separate direct OpenAI API adapter is added later, integrate it after the provider-neutral sink exists.
- Map Responses semantic events through the provider-neutral event sink.
- Add tests for output-text deltas, completion, error events, and final parsing.
- Defer background resume and native function-call streaming unless a later PRD expands scope.

### Phase 7: Cancellation Polish

- Wire cancellation token into all runtime adapters.
- Add cancellation tests for fake runtime and Codex CLI child process.
- Add HTTP cancellation tests using a delayed mock response.
- Add durable `step_cancel_failed` behavior if a runtime does not stop cleanly.

## 15. Test Plan

### Unit Tests

- `RuntimeEventSink` assigns monotonic sequences.
- Bounded sink backpressure does not drop events silently.
- Coalescer flushes on interval, byte threshold, final output, error, cancellation, and action request.
- Coalesced history payload includes `sequence_start`, `sequence_end`, `stream`, `agent`, `final_delta`, and `coalesced`.
- Runtime history compaction preserves coalesced stream metadata.
- Legacy `sequence` payloads remain readable.

### App Tests

- Fake-runtime Run publishes live state before final completion.
- Final `RuntimeOutput` remains authoritative and is handled once.
- Action Request output enters the existing action and approval flow after streaming.
- Pending approval state is visible and does not get hidden by the live stream block.
- Step time limit cancels the active runtime and records limit events.
- User interrupt records cancellation events and clears active run state.
- Model fallback emits a status event and still returns final output from the successful fallback.

### Runtime Adapter Tests

- Fake runtime emits multiple delayed deltas.
- Codex mock process writes stdout before exit and app observes the output before completion.
- Codex mock process writes stderr and success output; stderr is diagnostic, stdout is parsed.
- Codex process cancellation kills the child.
- Z.ai SSE parser handles normal content deltas, split frames, keepalives, completion, provider error frames, and malformed JSON.
- Z.ai fallback behavior is explicit and covered for both allowed and denied fallback.
- OpenAI Responses parser maps semantic output text delta, completion, failed, and error events after the non-streaming adapter is upgraded.

### TUI Render Tests

- Live active step renders above durable Chat lines.
- Live stream content wraps without shifting the Input Composer.
- Final stream marker renders.
- Interrupted stream marker renders.
- Agent Roster shows active/interrupted/completed status coherently.
- Follow mode remains pinned only when the user has not manually scrolled.

### Verification Commands

Minimum local gate for stream-mode implementation:

```text
cargo fmt -- --check
cargo test
```

Adapter-specific gates should include targeted tests while developing:

```text
cargo test runtime::fake
cargo test runtime::codex
cargo test runtime::zai
cargo test app::
cargo test tui::
```

Real provider integration tests must stay ignored or environment-gated.

## 16. Acceptance Criteria

- A fake-runtime Run visibly updates live-step state before the runtime returns final output.
- The TUI renders live output for the active Specialized Agent.
- The app still handles `OrchestratorDecision`, `AgentResult`, `ActionRequest`, and `ParseError` exactly once per runtime step.
- The streaming app driver represents action/approval pauses, limit-reached states, cancellation, and runtime completion through app-level step outcomes.
- Runtime events never bypass Harness Action validation or Action Approval.
- History writes coalesced runtime stream records instead of one event per tiny delta.
- Large stream content spills to artifact storage.
- Existing non-streaming runtimes work through compatibility mode during migration.
- Codex CLI stdout and stderr can be observed while the child process is running.
- Z.ai streaming either streams compatible SSE output or reports/falls back explicitly.
- Interrupting a streaming runtime leaves the app in a coherent interrupted state with durable cancellation evidence.
- `cargo test` covers runtime event ordering, app live-state publishing, history coalescing, TUI rendering, and adapter parsing.

## 17. Risks and Mitigations

- Risk: runtime adapters accidentally make streamed text authoritative.
  Mitigation: runtime events cannot carry final output variants; final parsing remains the returned `RuntimeOutput`.

- Risk: token-level streaming floods Session History.
  Mitigation: coalescer flushes by time, size, and lifecycle boundaries.

- Risk: stdout/stderr concurrent readers produce unstable sequence ordering.
  Mitigation: sink assigns sequence numbers centrally.

- Risk: Codex CLI progress text interferes with final JSON parsing.
  Mitigation: parse only accumulated stdout with the existing contract extraction path.

- Risk: HTTP streaming endpoints differ by provider.
  Mitigation: isolate SSE parsing per adapter behind provider-neutral runtime events.

- Risk: cancellation leaves child processes running.
  Mitigation: pass cancellation token into adapters and keep `kill_on_drop` for Codex CLI.

- Risk: current in-progress OpenAI Responses work changes while stream mode is being implemented.
  Mitigation: implement stream mode provider-neutrally and keep OpenAI Responses streaming as an adapter upgrade layered on the shared sink.

## 18. Open Questions

- Should Codex CLI streaming read by line for phase 1, or should it read fixed-size byte chunks immediately?
- Should council member runtime steps stream into the same live-step view in phase 1, or remain compatibility-only until the main agent step path is stable?
- Should Z.ai streaming fallback be enabled by config, hardcoded for one release, or disallowed until endpoint compatibility is verified?
- What display cap should `LiveStreamView.content` use after real provider output volume is observed?
- Should `runtime_stream_delta` remain the durable event kind indefinitely, or should a future schema add a more general `runtime_stream_batch` kind?
