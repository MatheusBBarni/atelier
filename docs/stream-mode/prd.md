## Problem Statement

The Harness already records runtime stream deltas, but active agent output is still effectively batch-oriented. A user starts a Run, sees step-start events, then waits until the Execution Runtime finishes before any meaningful agent output appears in the Event Stream. This makes the TUI feel unresponsive during long agent steps, hides useful progress, and makes interruption less clear because the user cannot see what the active Specialized Agent is doing while it is doing it.

The current behavior also blurs the intended streaming contract. Runtime adapters can attach deltas to a completed step result, but those deltas are delivered after the final Runtime Output is already available. For true chat-like behavior, streaming progress needs to reach the app while the step is pending, while the Rust Harness must continue to own Capability Enforcement, Harness Actions, Action Approval, final structured parsing, and Session History.

## Solution

Add first-class stream mode for active agent steps. Each Execution Runtime should be able to emit progress events while it is generating output. The app loop should consume those events while the runtime step is still pending, update live TUI state, coalesce durable Session History writes, and keep the final Runtime Output on the normal app-owned completion path.

From the user's perspective, the Event Stream should show a live block for the active Specialized Agent as output arrives. The Agent Roster should clearly show which agent is active. Interrupting the Run should cancel or stop the active runtime work when possible, record coherent cancellation events, and leave the TUI in a recoverable state.

The feature should preserve the existing action-policy boundary: runtimes may stream text, status, diagnostics, and tool-call progress, but they must not directly execute file writes, shell commands, approvals, or other Harness Actions. The Harness remains responsible for validating and executing Action Requests.

## User Stories

1. As a developer using the TUI, I want to see active agent output as it is generated, so that I know the Harness is making progress.
2. As a developer running a long agent step, I want the Event Stream to update before completion, so that I do not mistake a slow runtime for a frozen app.
3. As a developer watching the Agent Roster, I want the active Specialized Agent to be visually identifiable, so that I can understand which Agent Profile is currently working.
4. As a developer, I want live runtime output to appear in a stable TUI location, so that new chunks do not make the interface hard to follow.
5. As a developer, I want follow mode to keep recent stream output visible, so that I can monitor the latest progress without manual scrolling.
6. As a developer who scrolls up, I want the TUI to respect my scroll position, so that live output does not constantly pull me away from earlier events.
7. As a developer, I want streamed output to be associated with the correct Run, step, agent, and stream, so that progress remains understandable when reviewing the Session History.
8. As a developer, I want stdout, stderr, message, status, and diagnostic streams to be distinguishable, so that I can tell normal output from warnings or failures.
9. As a developer, I want the final Orchestrator Decision to remain parseable, so that stream mode does not break Routing Decisions.
10. As a developer, I want final Agent Results to remain typed and durable, so that downstream Orchestrator steps can continue reliably.
11. As a developer, I want Action Requests to continue through the existing app loop, so that Capability Enforcement and Action Approval are not bypassed.
12. As a developer, I want the Harness to pause correctly when an Action Request appears, so that streaming progress does not hide a pending approval or action.
13. As a developer, I want live output to stop or clearly pause while the app waits for Action Approval, so that I understand why the active step is not continuing.
14. As a developer, I want cancelled runtime work to show a cancellation state, so that I know my interrupt request was handled.
15. As a developer, I want cancellation events recorded durably, so that future Context Resume has accurate history.
16. As a developer, I want failed cancellation to be visible, so that I know when a runtime may still be running outside normal control.
17. As a developer, I want Codex CLI output to stream while the child process runs, so that CLI-backed agents feel interactive.
18. As a developer, I want Codex CLI stderr to be treated as diagnostics unless the process fails, so that normal progress text does not become a false error.
19. As a developer, I want Z.ai streaming chunks to appear as message deltas, so that API-backed agents feel responsive.
20. As a developer, I want Z.ai streaming fallback behavior to be explicit, so that endpoint incompatibility does not silently change runtime semantics.
21. As a developer, I want a deterministic fake runtime to emit multiple live deltas, so that stream-mode behavior can be tested without network or CLI dependencies.
22. As a developer, I want future OpenAI Responses streaming to map into the same runtime event model, so that adding that runtime does not require another TUI architecture.
23. As a maintainer, I want runtime progress events separated from final Runtime Output, so that adapters cannot accidentally make two sources authoritative.
24. As a maintainer, I want a bounded event channel, so that token streaming cannot exhaust memory or overwhelm the app.
25. As a maintainer, I want small chunks coalesced before history writes, so that Session History remains readable and efficient.
26. As a maintainer, I want large stream content stored through artifact storage, so that JSONL history events stay compact.
27. As a maintainer, I want stream event ordering to be stable, so that tests and history readers can reconstruct what happened.
28. As a maintainer, I want the app thread to own AppState updates, so that runtime adapters do not mutate UI state directly.
29. As a maintainer, I want the runtime contract to support true streaming and compatibility-mode runtimes, so that existing non-streaming adapters keep working during migration.
30. As a maintainer, I want the runtime driver to preserve model fallback behavior, so that retryable provider errors still try configured fallback models.
31. As a maintainer, I want final parsing errors to return through the existing step result path, so that malformed runtime output keeps the same error handling semantics.
32. As a maintainer, I want streaming tests to assert externally visible behavior, so that implementation details can change without brittle tests.
33. As a maintainer, I want the Event Stream to keep durable events distinct from live output, so that app state and Session History do not diverge.
34. As a maintainer, I want timeout handling to work with streaming child processes and HTTP requests, so that Run Limits remain enforceable.
35. As a maintainer, I want interrupted live output to be marked rather than silently disappearing, so that the user can trust the interface.
36. As a maintainer, I want live stream payloads to avoid leaking secrets beyond existing runtime-output handling, so that diagnostics remain safe to display and persist.
37. As a maintainer, I want the first implementation to work with one active Run, so that it matches the current Harness Session model.
38. As a maintainer, I want the design to allow future multiple-stream support, so that later parallel runs or richer tool progress do not require replacing the model.
39. As a maintainer, I want official API streaming concepts isolated behind runtime adapters, so that the app does not depend on provider-specific event names.
40. As a maintainer, I want stream mode to be rolled out in phases, so that internal plumbing can be verified before all adapters do true streaming.

## Implementation Decisions

- Build stream mode around a runtime event sink/channel contract. Runtime adapters emit progress events into the sink while the app awaits the final step result.
- Keep the final Runtime Output authoritative as the return value of the runtime step. Runtime events carry progress only and must not carry the final Orchestrator Decision, Agent Result, or Action Request as a competing completion path.
- Introduce a small app/runtime event model that separates app-owned lifecycle events from runtime-originated progress events. Step start, completion, cancellation, limit, and approval states are app-owned; runtime adapters emit text deltas, status, tool-call progress, and diagnostics. Provider-specific streaming events should be normalized before they reach the app loop.
- Treat the runtime event sink as a deep module with a stable interface: adapters only send events, while the app decides how to publish state, persist history, and handle backpressure.
- Add or adapt a runtime step driver that runs the adapter future and drains a bounded receiver while the runtime remains pending. The app-facing driver must return an app-level step outcome, not only the final runtime output, so it can represent completion, action/approval pauses, run limits, cancellation, and runtime failure while preserving runtime availability checks, model fallback behavior, step time limits, and error propagation.
- Apply backpressure through bounded channels and coalescing rather than allowing unbounded token queues.
- Add stable per-step ordering to runtime events. Sequence data should be sufficient for display, history coalescing, and tests.
- Reuse the existing live-step state concept where possible. It should represent the currently active Run step, its agent, and recent stream content.
- Keep app-owned AppState updates. Runtime adapters must not mutate app state, TUI state, or Session History directly.
- Render a live active-step block in the Event Stream area. The block should update in place while streaming and remain visually distinct from durable event lines.
- Keep the Agent Roster status synchronized with the active step through the same app-owned lifecycle transitions that drive the live stream view. Active, streaming, waiting for action, waiting for approval, cancelling, interrupted, failed, and completed states should remain clear.
- Persist stream progress through coalesced Session History events rather than one event per token or byte chunk.
- Coalescing should flush on a short interval, after a reasonable byte threshold, and immediately on final delta, runtime error, Action Request, or cancellation.
- Continue spilling large stream content to artifact storage using the existing artifact strategy, with history events containing metadata and references rather than oversized payloads.
- Extend runtime stream history payloads to include the agent, stream name, sequence range, final-delta marker, and coalescing marker.
- Keep Harness Actions outside the streaming adapter. If a runtime needs repository data, command execution, file mutation, or note recording, it must return an Action Request through the existing structured output path.
- The fake runtime should be the first true streaming adapter. It should emit deterministic delayed deltas before returning the same final output semantics it has today.
- Codex CLI streaming should read stdout and stderr while the process runs, accumulate stdout for final structured parsing, and treat stderr as diagnostic stream content unless the process exits unsuccessfully.
- Z.ai streaming should use compatible server-sent events when enabled, accumulate message content for final structured parsing, and explicitly handle endpoint rejection or fallback configuration.
- OpenAI Responses streaming should later map output text deltas, completion events, and error events into the same runtime event model. Native tool streaming remains future work.
- Cancellation should be represented with a cancellation token or equivalent runtime-control handle for each active step.
- Interrupting the active Run should request cancellation, update live state, record cancellation events, and clear or mark the live stream consistently.
- Codex CLI cancellation should terminate the child process. HTTP runtime cancellation should abort the request, and future background response mode should use the provider cancel path when configured.
- Existing compatibility behavior should remain available during migration. Runtimes that cannot stream yet may send one final progress delta after completion through an adapter path.
- The implementation should be phased: internal plumbing and fake runtime first, TUI live rendering second, real adapter streaming third, then history and cancellation polish.
- The core deep modules for implementation are the runtime event sink, runtime step driver, stream coalescer, provider streaming parsers, and live-step view model.

## Testing Decisions

- Good tests should assert externally visible behavior: live app state updates before completion, rendered TUI output, ordered history events, cancellation events, adapter output parsing, and fallback behavior. Tests should not depend on private task scheduling details or exact internal channel implementations.
- Test the fake runtime as the deterministic proof of true streaming. A fake-runtime run should expose multiple deltas before final result handling.
- Test the runtime step driver with a controlled adapter that emits progress before completing, fails after partial progress, and returns a final output after progress.
- Test app state publishing by observing that live-step state changes before the Run reaches completion.
- Test history coalescing by feeding many small deltas and asserting that durable history uses fewer coalesced events with correct sequence ranges.
- Test large streamed content by asserting that artifact storage is used and history remains compact.
- Test TUI rendering with live step content, active agent status, final-delta labeling, and interrupted state.
- Test scroll/follow behavior at the TUI boundary through render and state-transition tests rather than provider-specific streaming tests.
- Test Codex CLI streaming with a mock process or script that writes stdout before exit and stderr diagnostics separately.
- Test Codex CLI final parsing against accumulated stdout, not against individual progress chunks.
- Test Z.ai or OpenAI-compatible SSE parsing as a deep module with normal chunks, empty keepalives, multi-line data frames, completion events, malformed chunks, and error events.
- Test Z.ai streaming fallback only at the adapter behavior boundary: endpoint rejection should either fall back when configured or fail clearly when fallback is disabled.
- Test cancellation with fake or controlled runtimes so the app records cancellation-requested and cancellation-completed events and leaves Agent Roster status coherent.
- Test timeout behavior for streaming runtimes so long-lived streams remain subject to configured Run Limits.
- Reuse prior art from existing runtime adapter tests, app event/history tests, TUI render tests, and artifact spill tests.

## Out of Scope

- Building a web UI.
- Supporting multiple active Runs at the same time.
- Allowing Specialized Agents to call each other directly.
- Letting runtimes execute Harness Actions directly while streaming.
- Replacing the existing structured Orchestrator Decision and Agent Result contracts.
- Replacing the existing Action Request and Action Approval flow.
- Implementing native OpenAI Responses function-call streaming in the first pass.
- Adding transcript search, copy commands, command palette behavior, or a detail pane as part of this PRD.
- Changing authentication semantics for Codex CLI, Z.ai, or future OpenAI API runtimes.
- Changing server-side storage policy for OpenAI Responses beyond the future runtime's explicit configuration.
- Reworking Session History storage outside the narrow coalesced runtime-stream event changes.

## Further Notes

- The important architectural rule is that streaming improves progress visibility but does not move ownership of final behavior into runtime adapters.
- The current live-step state and render behavior provide a useful starting point, but they must be fed while a runtime step is pending to satisfy this PRD.
- The existing runtime delta type is still useful, but its timing and delivery path need to change from after-the-fact collection to app-consumed live events.
- The first useful milestone is a fake-runtime Run that visibly streams partial output before completion and has tests proving the intermediate AppState exists.
- Provider docs and model recommendations can change. OpenAI Responses details should be refreshed when that adapter is implemented, while the app-level streaming contract should remain provider-neutral.
