# Multiagent Harness

`multiagent` is a terminal-native Rust harness that routes each user prompt through
an orchestrator and a sequence of specialized agent profiles.

## Features

- One-click launch from any directory (or selected `--cwd`).
- Interactive TUI with agent roster, event stream, and composer.
- Orchestrator-driven routing for planning, execution, clarifying questions, and completion.
- Built-in multi-runtime support:
  - `codex` (CLI runtime)
  - `zai` (HTTP API runtime)
  - `fake` (test/runtime simulation)
- Configurable agents with explicit capabilities and scopes.
- Durable per-session history and artifacts under `.multiagent/`.
- Strict capability checks for file and command actions.
- Deterministic local tests with optional integration paths.

## Requirements

- Rust toolchain (to build from source).
- Optional: `codex` CLI if you plan to use the Codex runtime.
- Optional: `ZAI_API_KEY` (or another env var configured as `api_key_env`) for Z.ai runtime.

## Install

```bash
cargo install --path .
```

## Quick Start

```bash
multiagent --doctor
multiagent
```

`multiagent` opens an interactive TUI in the selected working directory.

## CLI

```bash
multiagent
multiagent --config <path>
multiagent --cwd <path>
multiagent --doctor
multiagent --doctor --json
multiagent --print-config
multiagent --init-config
multiagent --clean-sessions
multiagent --clean-sessions --yes
multiagent --debug
```

Notes:

- `--json` is valid only with `--doctor`.
- `--yes` is valid only with `--clean-sessions`.
- `--print-config` prints merged config with secrets redacted.
- `--init-config` creates starter config/instruction files if missing.

## Configuration

Configuration is merged in this order:

1. Built-in defaults
2. Home config: `~/.config/.multiagent/multiagent.toml`
3. Local override: `./multiagent.toml`
4. CLI flags (`--config` or `--cwd`)

You can also set `MULTIAGENT_CONFIG` to choose the home config path.

Important values:

- `approval_mode`: `yolo` (default) or `normal`
- `workspace.extra_read_roots` / `workspace.extra_write_roots`: explicit allowed paths
- `[runtimes.*]`: `type`, `command`, `args`, `base_url`, `api_key_env`
- `[agents.*]`: profile, model, effort, capabilities, instructions
- `[limits.*]`:
  - `max_agent_steps`
  - `max_step_actions`
  - `max_wall_clock_minutes`
  - `max_step_minutes`
  - `max_command_minutes`
  - `max_review_fix_cycles`

## Runtimes

- `codex`
  - Invokes `codex` as a child process.
  - Defaults: `codex exec --skip-git-repo-check --color never`
- `zai`
  - Uses API key from env var (example: `ZAI_API_KEY`) and posts to `api.z.ai`.
- `fake`
  - Local test/runtime simulation mode.

## Session history

Session artifacts are stored in:

- `.multiagent/sessions/<session-id>/events.jsonl`
- `.multiagent/sessions/<session-id>/artifacts/*`
- `.multiagent/runs/<run-id>.json`

Use `--clean-sessions` to delete project-local history. Use `--yes` to skip confirmation.

## Built-in agents

- `orchestrator` (plan)
- `explorer` (read)
- `oracle` (answer)
- `consul` (challenge)
- `fixer` (edit, command, verify)
- `reviewer` (command, verify, review)

Capabilities are enforced by the harness, not by runtime implementations.

## Project layout

- `src/main.rs` – program entry point
- `src/cli.rs` – argument parsing and command dispatch
- `src/config` – config loading, validation, merging, init helpers
- `src/app` – orchestration state machine
- `src/tui` – terminal UI and user input
- `src/runtime` – runtime adapters and contracts
- `src/actions` – file and command action execution
- `src/history` – event/artifact/run persistence
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
