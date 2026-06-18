# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`atelier` (crate name `multiagent`) is a terminal-native Rust harness that routes each user prompt through an orchestrator and a sequence of specialized agent profiles, backed by pluggable agent-CLI runtimes (Codex, Claude, Cursor, a Z.ai HTTP runtime, and a `fake` test runtime). The single binary is `atelier` (`src/main.rs` → `src/cli.rs`); library and tests live under the `multiagent` crate.

## Commands

Repo convention is to prefix shell commands with `rtk` (a token-compacting wrapper — see the RTK section at the bottom); the underlying tools are standard Cargo.

- Build: `cargo build` (release: `cargo build --release --bin atelier`)
- Run the TUI: `cargo run --bin atelier` — requires an interactive terminal. Non-interactive entry points: `atelier --doctor [--json]`, `atelier --print-config`, `atelier --init-config`, `atelier --codemap <init|changes|update>`.
- All tests: `cargo test`
- Library unit tests only (fast): `cargo test --lib`
- A single test: `cargo test --lib <substring>` — Cargo takes **one** filter substring, so run multiple invocations to target several names.
- One integration suite: `cargo test --test cli` (suites under `tests/`: `cli`, `skill_prompt_loading`, `skills_foundation`, `slash_command_catalog`, `runtime_integration`)
- Lint: `cargo clippy --all-targets` · Format: `cargo fmt` (check only: `cargo fmt --check`)
- Full pre-commit gate (mirrors CI in `.github/workflows/release.yml`): `cargo fmt --check && cargo clippy --all-targets && cargo test --locked && npm --prefix npm run check:skills` (the last guards that the `skills/atelier-config-setup/` discovery mirrors are in sync — regenerate with `npm --prefix npm run sync:skills`)

Testing notes:
- The **`fake` runtime** (`src/runtime/fake.rs`) drives deterministic end-to-end app/orchestrator tests: behavior is triggered by control phrases embedded in the prompt (e.g. `retryable provider error`, `non-retryable provider error`, plus clarification/workflow markers). Most `app`/`orchestrator` tests run a real run through `FakeRuntime` rather than mocking internals.
- Live-runtime tests in `tests/runtime_integration.rs` are `#[ignore]`d behind env vars (`MULTIAGENT_RUN_CODEX_INTEGRATION=1`, `MULTIAGENT_TEST_CLAUDE=1`, `MULTIAGENT_CURSOR_LIVE=1`, `MULTIAGENT_RUN_ZAI_INTEGRATION=1`).
- `runtime::codex` / `runtime::cursor` availability tests shell out to real CLIs and are environment-sensitive — they can fail/flake depending on what's installed locally. Treat those failures as environmental, not regressions.

## Versioning

**The package version must always be changed in `Cargo.toml` AND in every npm manifest together — they are required to match.** The npm manifests are `npm/package/package.json`, all `npm/platform/*/package.json`, and the `optionalDependencies` pins inside `npm/package/package.json`. Cargo's `[package].version` is the source of truth.

- Don't hand-edit the npm manifests: bump `Cargo.toml`, then run `npm --prefix npm run sync:versions` to propagate the version to every npm manifest, and `npm --prefix npm run check:versions` to verify Cargo, all npm manifests, the optional-dependency pins, and `atelier --version` agree.
- The release pipeline (`.github/workflows/release.yml`) enforces this via `check:versions`; a mismatch fails the run, so an unsynced bump will block the release. Commit the Cargo bump and the synced npm manifests in the same change.

## Architecture

**Run lifecycle (the core loop).** `App::submit_prompt` (`src/app/mod.rs`) first dispatches built-in commands (`/goal`, `/config`, `/subtask`, `/workflow`, `/queue`), otherwise compiles the prompt (skill injection, prefix handling), then `drive_and_replay` runs the orchestrator loop. The orchestrator (`src/orchestrator/mod.rs`) emits `OrchestratorDecision`s that advance `RunState` (`Idle → Planning → Running → WaitingForUser → Completed/Failed/Interrupted/LimitReached`). Each step is executed against a runtime via `execute_runtime_step_streaming`.

**Runtimes.** `Runtime` is a trait (`src/runtime/mod.rs`); concrete impls (`codex`, `claude`, `cursor`, `zai`, `fake`) are dispatched by `RuntimeKind`. Each agent carries a model fallback chain; `execute_runtime_step_streaming` retries the **same** model on *retryable* provider errors before falling back to the next model, and bails immediately on non-retryable ones. The Claude runtime strips Claude Code's default system prompt down to a minimal structured-adapter brief so smaller models stay in contract mode.

**Actions + capabilities.** Agents never touch the filesystem directly — they return `ActionRequest`s (`ReadFile`, `ListFiles`, `SearchText`, `RunCommand`, `ApplyPatch`, `WriteFile`, `RecordNote`; `src/actions/mod.rs`). Execution is gated by each agent's declared capabilities, the workspace read/write roots, and `ApprovalMode` (`yolo` default / `normal`). In `normal` mode, write/command actions surface an approval prompt in the TUI.

**Event sourcing → chat projection (key pattern).** The app records everything as events through `record_event` into durable per-session history (`src/history`, under `.atelier/`). Rendering reads no app internals: the TUI consumes a snapshot `AppState` (synced over a `watch` channel) and the chat transcript is *derived* by `src/app/chat/projection.rs`, which collapses the event stream into `ChatItemView`s keyed by `ChatLifecycleKey` — a run / workflow / clarification / queued-follow-up evolves a single chat item across its whole lifecycle. To make a feature appear in chat, emit events and extend the projection; do not render from app state directly.

**TUI.** `run_tui` (`src/tui/mod.rs`) spawns an app worker (`run_app_worker`) that owns the `App` and talks to the UI via an mpsc command channel plus the `watch<AppState>`; the render path is a pure function of `(AppState, TuiUiState)`. Key-routing precedence is: help modal → clarification → approval → `/agent:` dropdown → `/skill:` dropdown → `@` file-mention dropdown → command dropdown → queue focus → normal input. All color is centralized in `src/tui/theme.rs` as capability-detected semantic tokens (truecolor RGB / ANSI-256 / monochrome under `NO_COLOR`); the test `colors_live_only_in_theme_module` forbids inline `Color::` literals elsewhere in `src/tui/mod.rs`, so always use theme tokens.

**Slash commands.** `src/slash_commands.rs` is a metadata-only catalog that is the single source of visible command metadata for the TUI dropdown, the help overlay, and app unknown-command guidance — keep all three aligned through it. Execution ownership stays split: TUI-local (`/help`, `/reload:skills`), app commands (`/goal`, `/config`, `/subtask`, `/workflow`, `/queue`), and prompt prefixes (`/agent:`, `/skill:`) completed by their own specialized dropdowns.

**Config.** Merged in order: built-in defaults → home (`~/.config/.atelier/atelier.toml`) → local (`./atelier.toml`) → CLI flags (`src/config/mod.rs`). Sections: `[agents.*]` (profile, model + `model_fallbacks`, effort, capabilities, tools, prompt files), `[presets.*]` (agent overrides applied *before* local overrides), `[runtimes.*]`, `[council]` (serial review workflow — `council` is a routing target the orchestrator uses only for high-risk or explicitly-requested review), `[limits.*]`, and `[ui]` (e.g. `hide_banner`).

**Skills.** Discovered from project and personal roots (`.agents/skills`, `.claude/skills`; `src/skills/mod.rs`). `/skill:<name>` injects the skill's `SKILL.md` body into the compiled prompt wrapped in XML sections, with a runtime disclaimer that skills are guidance and do not bypass approvals/permissions. Suggestions are cached for the TUI dropdown and refreshed with `/reload:skills`.

## Feature workflow (.compozy)

Larger features are developed as PRD task packets under `.compozy/tasks/<feature>/`: `_prd.md` (product requirements), `_techspec.md` (technical design), `adrs/adr-NNN.md` (binding decisions), `_tasks.md` (the task index), and `task_NN.md` (one executable task each, with subtasks / tests / status). The `cy-*` skills (`cy-create-prd`, `cy-create-techspec`, `cy-create-tasks`, `cy-execute-task`, `cy-review-round`, `cy-fix-reviews`) drive this pipeline. When implementing against a task file, treat its ADRs as authoritative and keep the task's checklist and status updated; record scope changes as follow-up notes rather than silently expanding scope.

<!-- rtk-instructions v2 -->
# RTK (Rust Token Killer) - Token-Optimized Commands

## Golden Rule

**Always prefix commands with `rtk`**. If RTK has a dedicated filter, it uses it. If not, it passes through unchanged. This means RTK is always safe to use.

**Important**: Even in command chains with `&&`, use `rtk`:
```bash
# ❌ Wrong
git add . && git commit -m "msg" && git push

# ✅ Correct
rtk git add . && rtk git commit -m "msg" && rtk git push
```

## RTK Commands by Workflow

### Build & Compile (80-90% savings)
```bash
rtk cargo build         # Cargo build output
rtk cargo check         # Cargo check output
rtk cargo clippy        # Clippy warnings grouped by file (80%)
rtk tsc                 # TypeScript errors grouped by file/code (83%)
rtk lint                # ESLint/Biome violations grouped (84%)
rtk prettier --check    # Files needing format only (70%)
rtk next build          # Next.js build with route metrics (87%)
```

### Test (60-99% savings)
```bash
rtk cargo test          # Cargo test failures only (90%)
rtk go test             # Go test failures only (90%)
rtk jest                # Jest failures only (99.5%)
rtk vitest              # Vitest failures only (99.5%)
rtk playwright test     # Playwright failures only (94%)
rtk pytest              # Python test failures only (90%)
rtk rake test           # Ruby test failures only (90%)
rtk rspec               # RSpec test failures only (60%)
rtk test <cmd>          # Generic test wrapper - failures only
```

### Git (59-80% savings)
```bash
rtk git status          # Compact status
rtk git log             # Compact log (works with all git flags)
rtk git diff            # Compact diff (80%)
rtk git show            # Compact show (80%)
rtk git add             # Ultra-compact confirmations (59%)
rtk git commit          # Ultra-compact confirmations (59%)
rtk git push            # Ultra-compact confirmations
rtk git pull            # Ultra-compact confirmations
rtk git branch          # Compact branch list
rtk git fetch           # Compact fetch
rtk git stash           # Compact stash
rtk git worktree        # Compact worktree
```

Note: Git passthrough works for ALL subcommands, even those not explicitly listed.

### GitHub (26-87% savings)
```bash
rtk gh pr view <num>    # Compact PR view (87%)
rtk gh pr checks        # Compact PR checks (79%)
rtk gh run list         # Compact workflow runs (82%)
rtk gh issue list       # Compact issue list (80%)
rtk gh api              # Compact API responses (26%)
```

### JavaScript/TypeScript Tooling (70-90% savings)
```bash
rtk pnpm list           # Compact dependency tree (70%)
rtk pnpm outdated       # Compact outdated packages (80%)
rtk pnpm install        # Compact install output (90%)
rtk npm run <script>    # Compact npm script output
rtk npx <cmd>           # Compact npx command output
rtk prisma              # Prisma without ASCII art (88%)
```

### Files & Search (60-75% savings)
```bash
rtk ls <path>           # Tree format, compact (65%)
rtk read <file>         # Code reading with filtering (60%)
rtk grep <pattern>      # Search grouped by file (75%). Format flags (-c, -l, -L, -o, -Z) run raw.
rtk find <pattern>      # Find grouped by directory (70%)
```

### Analysis & Debug (70-90% savings)
```bash
rtk err <cmd>           # Filter errors only from any command
rtk log <file>          # Deduplicated logs with counts
rtk json <file>         # JSON structure without values
rtk deps                # Dependency overview
rtk env                 # Environment variables compact
rtk summary <cmd>       # Smart summary of command output
rtk diff                # Ultra-compact diffs
```

### Infrastructure (85% savings)
```bash
rtk docker ps           # Compact container list
rtk docker images       # Compact image list
rtk docker logs <c>     # Deduplicated logs
rtk kubectl get         # Compact resource list
rtk kubectl logs        # Deduplicated pod logs
```

### Network (65-70% savings)
```bash
rtk curl <url>          # Compact HTTP responses (70%)
rtk wget <url>          # Compact download output (65%)
```

### Meta Commands
```bash
rtk gain                # View token savings statistics
rtk gain --history      # View command history with savings
rtk discover            # Analyze Claude Code sessions for missed RTK usage
rtk proxy <cmd>         # Run command without filtering (for debugging)
rtk init                # Add RTK instructions to CLAUDE.md
rtk init --global       # Add RTK to ~/.claude/CLAUDE.md
```

## Token Savings Overview

| Category | Commands | Typical Savings |
|----------|----------|-----------------|
| Tests | vitest, playwright, cargo test | 90-99% |
| Build | next, tsc, lint, prettier | 70-87% |
| Git | status, log, diff, add, commit | 59-80% |
| GitHub | gh pr, gh run, gh issue | 26-87% |
| Package Managers | pnpm, npm, npx | 70-90% |
| Files | ls, read, grep, find | 60-75% |
| Infrastructure | docker, kubectl | 85% |
| Network | curl, wget | 65-70% |

Overall average: **60-90% token reduction** on common development operations.
<!-- /rtk-instructions -->
