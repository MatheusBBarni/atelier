# Atelier

<p align="center">
  <img src="web/public/images/atelier-logo.png" alt="Atelier logo: a teal and cream wizard hat above a terminal prompt, framed in gold on deep navy" width="168">
</p>

`atelier` is a terminal-native Rust harness that routes each user prompt through
an orchestrator and a sequence of specialized agent profiles.

<p align="center">
  <img src="web/public/images/atelier-wordmark.png" alt="Atelier wordmark: the word 'Atelier' in pixel-art lettering beside a half-teal, half-cream sailboat, on deep navy" width="640">
</p>

## Features

- One-click launch from any directory (or selected `--cwd`).
- Interactive TUI with agent roster, event stream, and composer.
- Orchestrator-driven routing for planning, execution, clarifying questions, and completion.
- Built-in multi-runtime support:
  - `codex` (Codex CLI runtime)
  - `claude` (Claude CLI runtime)
  - `cursor` (Cursor Agent CLI runtime)
  - `zai` (HTTP API runtime)
  - `fake` (test/runtime simulation)
- Configurable agents with explicit capabilities and scopes.
- Preset-based agent overrides, model fallback chains, and per-agent tool allowlists.
- Serial council workflow for high-risk or user-requested decision review.
- Durable per-session history and artifacts under `.atelier/`.
- Strict capability checks for file and command actions.
- Deterministic local tests with optional integration paths.

## Requirements

- Node.js 20+ and npm 10+ for the recommended npm install path.
- Rust toolchain only if you build from source.
- `ZAI_API_KEY` (or another env var configured as `api_key_env`): required for a real run
  with stock defaults — the default orchestrator runs on the `zai` runtime (`glm-5.1`), which
  fails without it. Only the `fake` runtime needs no credentials, and it simulates rather than
  doing real work.
- `codex` CLI with `codex login` completed: required for the default worker agents (explorer,
  fixer, reviewer), which run on the `codex` runtime out of the box.
- Optional: `claude` CLI — only if you opt agents into the Claude runtime through config.
- Optional: `cursor-agent` CLI with `cursor-agent login` completed — only if you opt agents
  into the Cursor runtime through config.

## Install

Recommended user install:

```bash
npm install -g @matheusbbarni/atelier
atelier --doctor
atelier
```

The npm package installs a prebuilt native binary for supported platforms and
does not initialize configuration, probe credentials, or start the TUI during
installation. Supported npm targets are macOS arm64/x64, glibc Linux arm64/x64,
and Windows arm64/x64. Alpine/musl Linux is not supported by the v1 npm
packages.

Update the npm-installed global command with:

```bash
atelier --update
```

Developer/source install:

```bash
cargo install --path .
```

GitHub Releases provide standalone native binary archives and checksum
manifests for release traceability.

## Quick Start

```bash
atelier --doctor
atelier
```

`atelier` opens an interactive TUI in the selected working directory.

## CLI

```bash
atelier
atelier --config <path>
atelier --cwd <path>
atelier --doctor
atelier --doctor --json
atelier --print-config
atelier --init-config
atelier --codemap init
atelier --codemap changes
atelier --codemap update
atelier --clean-sessions
atelier --clean-sessions --yes
atelier --update
atelier --debug
```

Notes:

- `--json` is valid only with `--doctor`.
- `--yes` is valid only with `--clean-sessions`.
- `--print-config` prints merged config with secrets redacted.
- `--init-config` creates starter config/instruction files if missing.
- `--codemap changes` reports stale maps without writing files.
- `--update` is handled by the npm launcher and updates the global npm package.

## Codemaps

Codemaps are visible repository docs for agents and users. `atelier --codemap
init` writes folder-level `codemap.md` files and stores hashes in
`.atelier/codemap.json`. `atelier --codemap changes` compares current file
hashes with that state and reports stale map paths. `atelier --codemap update`
regenerates maps and refreshes the state.

Codemap generation excludes `.git`, `.atelier`, `target`, and common build or
dependency folders such as `node_modules`, `vendor`, `dist`, and `build`.
Generated `codemap.md` files are excluded from hashes so editing a map does not
make the workspace stale by itself.

## TUI commands

Type `/` at the start of the composer to open the command dropdown: it lists the
built-in commands with short descriptions, filters as you type, and inserts the
selected command with `Tab` or `Enter` (text only — it never submits). `Esc`
dismisses it, and an unmatched command shows a compact "No commands found"
state. The dropdown stays disabled during clarification and approval prompts so
slash-prefixed answers like `/tmp/project` remain normal input.

- `/help`: toggle the help overlay.
- `/goal <text>`, `/goal`, `/goal clear`: manage the active session goal.
- `/config`: show active config files, selected preset, and warnings without raw prompt bodies.
- `/subtask <agent> <task>`: run one bounded child task through a specialized enabled agent.
- `/workflow <prompt>`: execute a broad implementation or refactor prompt inside one normal run with plan, child-outcome, verification, and risk evidence. V1 does not support saved workflow scripts, worktree-isolated workflow children, or background workflow execution.
- `/queue <message>` (alias `/q`): queue a follow-up prompt to run after the active run finishes.
- `/agent:<name>`: prefix a prompt with a selected agent; in the TUI, type `/agent:` and use Up/Down plus Enter to insert an enabled agent id.
- `/skill:<skill_name>`: load skill context from a selected project or personal skill; in the TUI, type `/skill:` and use Up/Down plus Enter to insert a cached project or personal skill.
- `/reload:skills`: refresh cached skill names from project and personal skill folders.

Type `@` anywhere in the composer to open the file picker: it fuzzy-searches the
project's files and folders, highlights the matched characters, and ranks the
most likely path first (recently-edited and shallower paths surface above deep,
old ones). A bare `@` lists your most recent files. Use `Up`/`Down` to select
and `Tab` or `Enter` to accept — the `@fragment` is replaced in place by the
bare path (folders end with `/`), a trailing space is added, and the cursor
lands ready to keep typing, so a second `@` adds another reference. `Esc`
dismisses it, and a query with no matches shows a compact "No matching files"
row (without trapping `Enter`). Results respect `.gitignore` and exclude
build/dependency noise and known secret files (e.g. `.env`, private keys); only
paths inside the project appear, and selecting one inserts text only — it never
reads file contents. Like the `/` dropdowns, it stays disabled during
clarification and approval prompts.

## Configuration

Configuration is merged in this order:

1. Built-in defaults
2. Home config: `~/.config/.atelier/atelier.toml`
3. Local override: `./atelier.toml`
4. CLI flags (`--config` or `--cwd`)

You can also set `ATELIER_CONFIG` to choose the home config path (the legacy `MULTIAGENT_CONFIG` is still honored for back-compat).

Important values:

- `preset`: optional selected preset name
- `approval_mode`: `yolo` (default) or `normal`
- `workspace.extra_read_roots` / `workspace.extra_write_roots`: explicit allowed paths
- `[runtimes.*]`: `type`, `command`, `args`, `base_url`, `api_key_env`
- `[presets.<name>.agents.<agent>]`: preset-scoped agent overrides applied before local agent overrides
- `[agents.*]`: profile, model, `model_fallbacks`, effort, capabilities, `tools`, prompt files
- `[council]`: `default_preset`, `timeout_seconds`, `execution_mode = "serial"`
- `[council.presets.<name>.<councillor>]`: runtime, model, `model_fallbacks`, effort, thinking, and `prompt` or `prompt_file`
- `[limits.*]`:
  - `max_agent_steps`
  - `max_step_actions`
  - `max_wall_clock_minutes`
  - `max_step_minutes`
  - `max_command_minutes`
  - `max_review_fix_cycles`
  - `max_parallel_agent_steps`

## Runtimes

- `codex`
  - Default runtime for the built-in worker agents (explorer, fixer, reviewer), so `codex login` is needed for real runs out of the box.
  - Invokes the installed `codex` CLI as a child process.
  - Reuses Codex-owned login state, including ChatGPT subscription login when the CLI is signed in that way.
  - Defaults: `codex exec --skip-git-repo-check --color never`
  - Check setup with `codex login status`; sign in with `codex login` or `codex login --device-auth`.
- `claude`
  - Invokes the installed `claude` CLI as a child process.
  - Keeps Claude credentials owned by the Claude CLI and user environment.
  - Built-in agents do not use Claude unless you opt in through config.
- `cursor`
  - Invokes the installed `cursor-agent` CLI as a child process.
  - Reuses Cursor-owned login state; check setup with `cursor-agent status` and sign in with `cursor-agent login`.
  - Uses `cursor-agent --print --output-format stream-json` internally from an isolated deny-all Cursor permission sandbox.
  - Keeps Cursor-native tool calls behind Harness Actions.
  - Built-in agents do not use Cursor unless you opt in through config.
- `zai`
  - Default runtime for the orchestrator (`glm-5.1`), so `ZAI_API_KEY` is effectively required for any real run.
  - Uses an API key from an env var (example: `ZAI_API_KEY`) and posts to `https://api.z.ai/api/paas/v4`.
- `fake`
  - Local test/runtime simulation mode.

Use `codex` when you want the installed Codex CLI and its login flow. ChatGPT
subscription-backed Codex usage goes through Codex local auth, not an
`OPENAI_API_KEY` in `atelier.toml`.

## Session history

Session artifacts are stored in:

- `.atelier/sessions/<session-id>/events.jsonl`
- `.atelier/sessions/<session-id>/artifacts/*`
- `.atelier/runs/<run-id>.json`

Use `--clean-sessions` to delete project-local history. Use `--yes` to skip confirmation.

## Built-in agents

- `orchestrator` (plan)
- `explorer` (read)
- `oracle` (read, answer)
- `consul` (read, challenge)
- `fixer` (read, edit, command, verify)
- `reviewer` (read, command, verify, review)
- `librarian` (read, answer; disabled by default)
- `designer` (read, edit, verify; disabled by default)

Capabilities are enforced by the harness, not by runtime implementations.
If `tools` is set for an agent, it further narrows the harness actions exposed
inside those capabilities.

## Council

The council is a harness workflow, not a normal agent. The orchestrator may route
to `next_agent = "council"` only for high-risk architecture, security, data
integrity, difficult review, privacy, compliance, migration, rollback, or explicit
user council requests.

Council execution is serial. Each councillor returns an `agent_result`; the
harness records per-councillor diagnostics and synthesizes a council result with
confidence, dissent, risks, recommended action, and stop condition. If every
councillor fails, the council result is `failed`; if some succeed, synthesis still
proceeds with partial confidence.

## Tooling Policy

Harness tools stay centralized in `src/actions`. `apply_patch` validates unified
diff structure and target paths before writing files. `ast_search` and docs/web
fetch actions are intentionally not exposed until ast-grep installation policy
and network/MCP policy are explicit.

## Visibility

The TUI shows active step details and recent runtime stream snippets inside the
Chat while a step is running. External tmux/zellij pane mirroring is not
required for correctness and remains deferred until parallel child-run behavior
needs it.

## Project layout

- `src/main.rs` – program entry point
- `src/cli.rs` – argument parsing and command dispatch
- `src/config` – config loading, validation, merging, init helpers
- `src/app` – orchestration state machine
- `src/orchestrator` – routing decisions that advance the run loop
- `src/tui` – terminal UI and user input
- `src/runtime` – runtime adapters and contracts
- `src/actions` – file and command action execution
- `src/skills` – skill discovery and prompt injection
- `src/file_index` – gitignore-aware file walk and fuzzy ranking for the `@` picker
- `src/history` – event/artifact/run persistence
- `src/codemap` – folder-level repository map generation
- `src/diagnostics` – shared diagnostic severity and message types
- `src/doctor` – diagnostics and availability checks

## Development

Run unit/integration checks:

```bash
cargo test
```

Build locally:

```bash
cargo build
```

## License

MIT
