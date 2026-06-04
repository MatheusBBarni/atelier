# Implementation Plan: Improvements From oh-my-opencode-slim

Status: Draft
Date: 2026-06-03

## Summary

This document captures improvements worth bringing into `multiagent` after
reading `../oh-my-opencode-slim`. The sibling project is an OpenCode plugin, so
most implementation details cannot be copied directly. The useful parts are the
product patterns: stronger agent role definitions, preset-based model routing,
per-agent tool access, prompt override files, resumable delegated context,
council-style review, subtasks, and richer terminal visibility.

The immediate goal is not to turn `multiagent` into an OpenCode plugin. The goal
is to evolve the Rust harness while preserving its current architecture:
runtimes produce typed outputs or action requests, and the Rust harness owns
actions, permissions, limits, history, and UI state.

## Current multiagent baseline

Relevant current files:

- `src/config/mod.rs`: built-in runtimes, agents, config merge, capabilities,
  limits, and validation.
- `src/runtime/mod.rs`: runtime trait, `RuntimeRequest`, `RuntimeStepResult`,
  `RuntimeStreamDelta`, prompt envelope, and runtime dispatch.
- `src/runtime/codex.rs`: short-lived Codex CLI process per agent step.
- `src/runtime/zai.rs`: non-streaming OpenAI-compatible chat-completions call.
- `src/app/mod.rs`: run state machine, action loop, approvals, history events,
  and TUI state publishing.
- `src/actions/mod.rs`: harness-owned read/list/search/command/patch/write/note
  actions with capability and command policy checks.
- `src/tui/mod.rs`: three-zone Ratatui interface with roster, event stream, and
  input composer.
- `docs/initial-prd.md` and `docs/initial-techspec.md`: v1 explicitly
  prioritizes one active run and harness-owned effects.

## oh-my-opencode-slim patterns worth adopting

### 1. Presets and model fallback chains

Slim supports named presets, model arrays, runtime preset switching, and fallback
chains per agent. `multiagent` already has per-agent `runtime`, `model`,
`effort`, and `thinking`, but no named preset layer or fallback chain.

Plan:

- Add `[presets.<name>.agents.<agent>]` config support.
- Add a top-level `preset = "<name>"` selector.
- Keep arrays replacing arrays during merge, matching current config behavior.
- Add optional per-agent `model_fallbacks = ["..."]` or a runtime-level
  fallback table.
- Add optional `display_name` for UI-friendly labels without changing stable
  agent IDs.
- Add `multiagent --print-config` coverage showing the active preset after merge.
- Defer in-session `/preset` switching until the TUI command layer exists.

Acceptance checks:

- Config tests prove preset merge order is built-in -> home -> local -> CLI,
  with local agent overrides winning over active preset fields.
- Doctor reports selected preset and missing fallback models without exposing
  secrets.
- Runtime execution retries the next configured model only for provider errors
  classified as retryable.

### 2. Dynamic orchestrator prompt from enabled agents

Slim builds its orchestrator prompt from only the enabled agents and injects
role-specific delegation rules. `multiagent` currently gives the Orchestrator a
short static instruction string.

Plan:

- Add an `orchestrator_prompt` builder in `src/orchestrator` or `src/config`.
- Generate a compact role table from enabled `AgentProfile`s:
  id, name, capabilities, runtime/model, and delegation guidance.
- Add optional `orchestrator_instructions` or per-agent
  `orchestrator_description` config.
- Keep the runtime output schema contract separate from the routing prompt.

Acceptance checks:

- Disabled agents are not offered to the Orchestrator.
- Custom agents appear in the generated routing prompt after validation.
- The Orchestrator validation path still rejects unknown or disabled agents.

### 3. Prompt override files and append instructions

Slim supports project-local prompt overrides and append files. `multiagent`
already has `instructions` and `instructions_file`, but it does not have a
layered convention for per-agent prompt customization.

Plan:

- Keep TOML as the source of truth for which file paths are active.
- Add optional `instructions_append_file` on agents.
- Add optional `orchestrator_description_file` for dynamic routing text.
- Resolve relative paths from the config file that declared them, matching other
  config-source behavior.
- Show active prompt file paths in `--print-config` and doctor output without
  printing full prompt content.

Acceptance checks:

- Missing prompt files are validation errors with source-path context.
- Append files cannot silently replace the base instruction.
- Generated orchestrator prompts include custom descriptions only for enabled
  agents.

### 4. More complete role set, but phased

Slim has roles beyond the current harness set: `librarian`, `designer`,
`observer`, `council`, and `councillor`.

Plan:

- Phase 1: add optional built-in `librarian`.
  - Capabilities: `read`, `answer`.
  - Purpose: current official docs/API lookup and library research.
  - Runtime: configurable API runtime; no edits.
- Phase 2: add optional built-in `designer`.
  - Capabilities: `read`, `edit`, `verify`.
  - Purpose: user-facing UI/TUI work.
- Phase 3: add optional `observer` only if image/PDF support is added to a
  runtime; otherwise keep it out.
- Phase 4: add `council` as a harness workflow, not just another agent.

Acceptance checks:

- New roles are disabled or unavailable gracefully if their runtime is missing.
- Role additions do not grant new action capabilities to existing agents.
- Routing docs and generated prompt describe when not to use each role.

### 5. Per-agent tool and MCP policy

Slim has per-agent skills and MCP allowlists. `multiagent` has capabilities but
not named tool groups or MCPs.

Plan:

- Keep `Capability` as the hard authorization floor.
- Add optional `tools = [...]` on agents for harness tools that are narrower than
  a capability. Example: `search_text`, `read_file`, `run_command`.
- Add optional `mcps = [...]` only after a generic MCP action boundary exists.
- Use allow/exclude syntax only if there is enough value; otherwise prefer
  explicit lists for TOML clarity.

Acceptance checks:

- An agent with `Capability::Read` but no `search_text` tool cannot request
  search.
- Doctor can show effective tool access per agent.
- Tool policy failures are durable `action_denied` events.

### 6. Session goals and subtasks

Slim's `/goal` and `/subtask` patterns map well to this harness. They improve
long-running alignment without requiring parallel active runs immediately.

Plan:

- Add an optional session goal stored in `AppState` and session history.
- Add TUI commands:
  - `/goal <text>`
  - `/goal`
  - `/goal clear`
- Include the active goal in `RuntimeRequest`.
- Add a sequential bounded subtask workflow that creates a child
  `RunDriveContext` with `parent_run_id`, a narrow prompt, and inherited read
  context.
- Return the child summary to the parent as an artifact or history event.
- Defer real parallel subtasks until the app event loop can manage multiple
  concurrent run contexts safely.

Acceptance checks:

- Goal changes are written to session history.
- Runtime prompt envelopes include the goal.
- Child/subtask summaries cannot broaden scope beyond the subtask request.

### 7. Council workflow for high-risk decisions

Slim's council runs multiple councillors and synthesizes the results. This is
useful for architecture, security, data integrity, and difficult reviews.

Plan:

- Model council as a workflow in the harness, not as ordinary agent delegation.
- Config shape:
  - `[council] default_preset, timeout_seconds, execution_mode`
  - `[council.presets.<name>.<councillor>] model, runtime, effort, prompt`
- Start with serial execution to preserve one-active-run simplicity.
- Add parallel execution only after runtime cancellation, streaming, and state
  isolation are in place.
- Store individual councillor outputs as artifacts if large, then pass compact
  summaries to the synthesizer.

Acceptance checks:

- If all councillors fail, the council returns a blocked/failed result with
  per-councillor diagnostics.
- If some councillors succeed, synthesis still proceeds and notes partial
  confidence.
- Council is only routed by Orchestrator for high-risk or user-requested cases.

### 8. Codemap-style repository mapping

Slim ships a codemap skill that maintains folder-level `codemap.md` files and a
hash state file. `multiagent` can use the same idea as a harness command or
agent workflow.

Plan:

- Add `multiagent --codemap init|changes|update` only after core runtime work,
  or expose it as a `record_note`/artifact workflow first.
- Store state under `.multiagent/codemap.json` instead of `.slim`.
- Generate concise folder maps for active agents to read.
- Treat maps as user-editable docs, not hidden model memory.

Acceptance checks:

- Codemap generation excludes `.git`, `.multiagent`, target build output, and
  common dependency folders.
- Hash changes identify stale maps.
- Explorer can reference maps without assuming they are current.

### 9. Multiplexer-like visibility

Slim mirrors child sessions into tmux/zellij panes. Since `multiagent` already
owns the TUI, the first implementation should improve in-app visibility before
spawning external panes.

Plan:

- Add live step detail and streaming output in the TUI first.
- Add optional `multiplexer.type = "tmux" | "zellij" | "none"` later if
  parallel child runs are added.
- If added, keep pane management best-effort and never required for correctness.

Acceptance checks:

- The core run succeeds identically with multiplexer disabled.
- Pane failures produce diagnostics, not run failures.
- External panes never bypass harness action policy.

### 10. Config diagnostics and validation visibility

Slim surfaces invalid configuration quickly in the UI. `multiagent` already has
validation and doctor paths, but the TUI should expose enough status to avoid
running with surprising defaults.

Plan:

- Add a compact config status line in the TUI header or footer.
- Surface active config files, preset, and validation warnings in `/config`.
- Keep full validation output in doctor and `--print-config`.

Acceptance checks:

- Invalid config blocks startup before runtime work begins.
- Non-fatal warnings are visible in doctor and TUI command output.
- Secrets and raw prompt bodies are not printed in status surfaces.

### 11. Tooling improvements

Slim includes useful tool behavior: web fetch, AST search, apply-patch rescue,
and command helpers.

Plan:

- Add `ast_search` only after `ast-grep` availability and install policy are
  decided.
- Add a docs/web fetch tool through MCP or a dedicated runtime only after
  network policy is explicit.
- Improve `apply_patch` validation before adding tolerant patch repair.
- Keep command execution allow/approve/deny policy centralized in
  `src/actions/mod.rs`.

Acceptance checks:

- New tools have explicit capabilities or tool names.
- Network-capable tools are unavailable by default or clearly shown by doctor.
- Patch rescue fails closed on ambiguity.

## Phased roadmap

### Phase 1: Config and routing ergonomics

- Add presets.
- Add prompt override/append file support.
- Add generated orchestrator prompt from enabled agents.
- Add optional `librarian`.
- Expand doctor and config tests.

### Phase 2: Better continuity

- Add session goal.
- Add richer runtime request context with compact recent files/actions.
- Add command-layer slash commands in the TUI.

### Phase 3: Tools and research

- Add per-agent tool allowlists.
- Add optional docs/web research path.
- Add codemap artifacts or command.

### Phase 4: Council and subtasks

- Add serial council workflow.
- Add bounded subtask runs.
- Add parallelism only after cancellation, streaming, and UI state are ready.

### Phase 5: External visibility

- Add optional tmux/zellij pane mirroring if parallel child runs exist.
- Keep the TUI as the authoritative control surface.

## Non-goals

- Do not copy OpenCode plugin hooks or SDK assumptions directly.
- Do not add auto-update behavior.
- Do not add Divoom or external device integrations.
- Do not allow model runtimes to execute shell commands or write files directly.
- Do not add write-heavy parallel agents until merge/conflict handling exists.

## Risks

- Presets and custom agents can make config harder to reason about. Mitigate
  with `--print-config`, schema validation, and doctor output.
- More roles can create routing noise. Mitigate with generated prompts that
  include "do not delegate when" rules.
- Council and subtasks can consume large token budgets. Mitigate with explicit
  routing thresholds and user-visible cost/speed tradeoffs.
- Tool allowlists can overlap confusingly with capabilities. Mitigate by
  treating capabilities as coarse authorization and tools as narrower exposure.

## Required implementation evidence

- Unit tests for config presets, generated orchestrator prompt, and tool policy.
- App tests for goal persistence and history events.
- Runtime tests showing new roles do not bypass action policy.
- TUI render tests for any new command/status surfaces at 80x24 and 120x40.
- Updated docs or ADRs for any new runtime, council, or parallelism behavior.
