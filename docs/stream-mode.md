# Plan: Stream Mode for Chat

Status: Draft
Date: 2026-06-03

## Summary

`multiagent` already has a `RuntimeStreamDelta` type, but the current concrete
runtimes emit deltas only after a step finishes. This means the TUI "chat" feels
batch-oriented: the user sees step start events, then a final result.

Stream mode should make the active agent's output visible while it is being
generated, without weakening the existing action-policy boundary. Runtime
adapters may stream text, status, tool-call progress, and final structured
output, but file writes, shell commands, approvals, and history persistence stay
owned by the Rust harness.

## Current behavior

Relevant files:

- `src/runtime/mod.rs` defines `RuntimeStreamDelta` and
  `RuntimeStepResult.stream_deltas`.
- `src/runtime/codex.rs` waits for `child.wait_with_output()` and emits one
  final stdout delta.
- `src/runtime/zai.rs` sends `"stream": false` and emits one final message
  delta.
- `src/runtime/fake.rs` emits one final fake delta.
- `src/app/mod.rs` calls `record_runtime_stream_deltas` after
  `execute_runtime_step` returns.
- `src/tui/mod.rs` renders only `AppState.events`; it has no first-class live
  assistant message or step transcript view.

The existing history event kind `runtime_stream_delta` is useful, but its timing
is wrong for live chat: it is recorded after the runtime step has already
completed.

## Goals

- Show active agent output incrementally in the TUI.
- Keep final structured decisions/results parseable and durable.
- Keep action requests and approvals in the existing app loop.
- Support streaming for fake, Z.ai/OpenAI-compatible chat completions, Codex CLI
  stdout, and future OpenAI Responses runtime.
- Make cancellation visible and reliable.
- Avoid flooding history with tiny token events.

## Non-goals

- Do not let runtimes directly execute tools while streaming.
- Do not require every runtime to support true token streaming.
- Do not build a web UI.
- Do not add parallel active runs as part of stream mode.

## Target runtime contract

Replace the "return all deltas at the end" contract with a sink/channel model.

Proposed shape:

```rust
#[async_trait::async_trait]
pub trait Runtime: Send + Sync {
    async fn check_availability(&self) -> RuntimeAvailability;

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        sink: RuntimeEventSink,
    ) -> Result<RuntimeOutput>;
}
```

Runtime events:

```rust
pub enum RuntimeEvent {
    StepStarted { run_id: String, step_id: String, agent: String },
    Delta(RuntimeStreamDelta),
    Status { message: String },
    ToolCallProgress { name: String, summary: String },
    Error { diagnostic: String },
}
```

The exact enum can differ, but the important behavior is:

- deltas are sent before the final output is ready;
- the app owns conversion into `AppState` and history;
- `stream_step` returns the final `RuntimeOutput`;
- runtime events carry progress only, not the authoritative final result.

## App loop shape

The current app awaits a whole runtime step before publishing stream deltas. A
streaming implementation should instead run the adapter future alongside a
bounded event receiver:

- Spawn or poll the runtime step with a cancellation token.
- Drain `RuntimeEvent`s while the runtime is still pending.
- Publish live state from the app thread, not from the runtime.
- Keep event ordering stable with a per-step sequence number.
- Apply backpressure with a bounded channel and chunk coalescing instead of an
  unbounded token queue.

If spawning is used, the runtime must be `Send` and all final parsing errors
must still return through the app-owned step result path.

## App state changes

Add a live stream view to `AppState`:

```rust
pub struct StreamingMessageView {
    pub run_id: String,
    pub step_id: String,
    pub agent: String,
    pub stream: String,
    pub content: String,
    pub sequence: u32,
    pub is_final: bool,
}
```

Options:

- `current_stream: Option<StreamingMessageView>` for v1.
- Later, `streams: BTreeMap<String, StreamingMessageView>` if parallel runs or
  multiple streams are added.

App behavior:

- On `RuntimeEvent::Delta`, append to the live message and publish state.
- Coalesce tiny chunks before durable history writes.
- On final result, flush the live stream to history and clear/mark final.
- On action request, pause live output and show approval/action state.
- On cancellation, mark the live message interrupted and stop reading.

## History strategy

Do not append one history record per token.

Recommended coalescing:

- Flush every 250 ms while streaming, or
- Flush after 2 KB of accumulated content, or
- Flush immediately on final delta/error/action request.

Continue to use `runtime_stream_delta` events, but add fields:

- `agent`
- `sequence_start`
- `sequence_end`
- `stream`
- `content`
- `final_delta`
- `coalesced`

Large content should continue to use artifact storage, matching the current
large delta behavior.

## Adapter plan

### Fake runtime

Purpose: deterministic tests.

- Emit 2-3 deltas with short delays.
- Final output remains the same fake decision/result.
- Add app tests that observe intermediate `AppState.current_stream`.

### Z.ai runtime

Current state: `stream: false` chat completions.

Plan:

- Add a runtime config or hardcoded default for streaming once app support
  exists.
- Send `"stream": true` to compatible endpoints.
- Parse SSE chunks for `choices[0].delta.content`.
- Accumulate the full content for final contract parsing.
- Fall back to non-streaming if the endpoint rejects streaming and config allows
  fallback.

### Codex CLI runtime

Current state: wait for full stdout/stderr.

Plan:

- Spawn stdout and stderr readers concurrently.
- Emit stdout/stderr line or byte chunks as deltas while the process runs.
- Accumulate stdout for final structured parse.
- Treat stderr as diagnostic stream, not necessarily failure.
- Keep timeout and `kill_on_drop` behavior.

Risk:

- Codex CLI may output progress text that is not the final JSON contract. This
  is already possible. The final parser should use the accumulated stdout and
  existing contract extraction, while streamed progress remains best-effort UI
  content.

### OpenAI Responses runtime

See `docs/codex-api.md`.

Plan:

- Use `stream: true`.
- Map `response.output_text.delta` to text deltas.
- Watch `response.completed` for final output.
- Watch `error` events for failure diagnostics.
- Later, map function-call streaming events into action progress.

Official docs reference:
https://developers.openai.com/api/docs/guides/streaming-responses

## TUI behavior

Initial TUI changes:

- Event Stream shows a live block for the active agent below existing durable
  events.
- The live block updates in place while streaming.
- The roster highlights the active agent.
- The footer/status line shows streaming, waiting for action, waiting for
  approval, or completed.
- Follow mode keeps the event stream pinned to the latest output unless the user
  manually scrolls up.

Later:

- Add a transcript model with event types instead of plain strings.
- Add a detail pane for command output, diffs, and raw stream content.
- Add `/copy-last`, `/clear`, or transcript search only after command mode
  exists.

## Cancellation

Current interrupt sets run state and clears pending state, but runtime work is
not represented as a cancellable task in the runtime contract.

Plan:

- Give each active runtime step a cancellation token.
- TUI `Ctrl-C` or future `Esc Esc` sends cancellation.
- Codex CLI: kill child process.
- HTTP runtimes: abort request; if background mode is used, call cancel endpoint.
- Record `step_cancel_requested` and `step_cancelled` or `step_cancel_failed`.

## Tests

Required tests:

- Fake runtime emits multiple deltas before final result.
- App state publishes live stream updates before run completion.
- History coalesces deltas instead of writing one event per tiny chunk.
- TUI render test shows live stream text and active agent status.
- Codex CLI adapter test with a mock process/script proves stdout is read before
  process exit.
- HTTP streaming parser test handles normal chunks, empty keepalives, final
  completion, and error event.

## Implementation phases

### Phase 1: Internal event plumbing

- Add `RuntimeEvent` and `RuntimeEventSink`.
- Update fake runtime.
- Update app state and tests.
- Keep Codex/Z.ai temporarily using a compatibility adapter that emits one final
  delta.

### Phase 2: TUI live stream

- Render `current_stream`.
- Add status/footer text for active step.
- Ensure scroll follow behavior remains stable.

### Phase 3: Real adapter streaming

- Implement Codex CLI stdout/stderr streaming.
- Implement Z.ai SSE streaming if endpoint-compatible.
- Add OpenAI Responses streaming when that runtime exists.

### Phase 4: History and cancellation polish

- Add coalesced durable stream events.
- Add cancellation tokens across runtime tasks.
- Add background response cancellation for OpenAI Responses runtime.

## Acceptance criteria

- A fake-runtime run visibly streams partial text before completion.
- `cargo test` covers runtime event ordering and TUI rendering.
- History remains readable and is not flooded by token-level events.
- Existing non-streaming runtimes still work through a compatibility path.
- Interrupting a streaming runtime leaves the app in a coherent state with a
  durable cancellation event.
