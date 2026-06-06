# Atelier

`atelier` is a terminal-native Rust harness that routes each user prompt through
an orchestrator and a sequence of specialized agent profiles.

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
- Durable per-session history and artifacts under `.multiagent/`.
- Strict capability checks for file and command actions.
- Deterministic local tests with optional integration paths.

## Requirements

- Node.js 20+ and npm 10+ for the recommended npm install path.
- Rust toolchain only if you build from source.
- Optional: `codex` CLI with `codex login` completed if you plan to use the Codex runtime.
- Optional: `claude` CLI if you plan to opt agents into the Claude runtime.
- Optional: `cursor-agent` CLI with `cursor-agent login` completed if you plan to use the Cursor runtime.
- Optional: `ZAI_API_KEY` (or another env var configured as `api_key_env`) for Z.ai runtime.

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
`.multiagent/codemap.json`. `atelier --codemap changes` compares current file
hashes with that state and reports stale map paths. `atelier --codemap update`
regenerates maps and refreshes the state.

Codemap generation excludes `.git`, `.multiagent`, `target`, and common build or
dependency folders such as `node_modules`, `vendor`, `dist`, and `build`.
Generated `codemap.md` files are excluded from hashes so editing a map does not
make the workspace stale by itself.

## TUI commands

- `/goal <text>`, `/goal`, `/goal clear`: manage the active session goal.
- `/config`: show active config files, selected preset, and warnings without raw prompt bodies.
- `/subtask <agent> <task>`: run one bounded child task through a specialized enabled agent.
- `/skill:<name>`: prefix a prompt with a selected skill; in the TUI, type `/skill:` and use Up/Down plus Enter to insert a project or personal skill.

## Configuration

Configuration is merged in this order:

1. Built-in defaults
2. Home config: `~/.config/.multiagent/multiagent.toml`
3. Local override: `./multiagent.toml`
4. CLI flags (`--config` or `--cwd`)

You can also set `MULTIAGENT_CONFIG` to choose the home config path.

Important values:

- `preset`: optional selected preset name
- `approval_mode`: `yolo` (default) or `normal`
- `workspace.extra_read_roots` / `workspace.extra_write_roots`: explicit allowed paths
- `[runtimes.*]`: `type`, `command`, `args`, `base_url`, `api_key_env`
- `[presets.<name>.agents.<agent>]`: preset-scoped agent overrides applied before local agent overrides
- `[agents.*]`: profile, model, `model_fallbacks`, effort, capabilities, `tools`, prompt files
- `[council]`: `default_preset`, `timeout_seconds`, `execution_mode = "serial"`
- `[council.presets.<name>.<councillor>]`: runtime, model, effort, and `prompt` or `prompt_file`
- `[limits.*]`:
  - `max_agent_steps`
  - `max_step_actions`
  - `max_wall_clock_minutes`
  - `max_step_minutes`
  - `max_command_minutes`
  - `max_review_fix_cycles`

## Runtimes

- `codex`
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
  - Uses API key from env var (example: `ZAI_API_KEY`) and posts to `api.z.ai`.
- `fake`
  - Local test/runtime simulation mode.

Use `codex` when you want the installed Codex CLI and its login flow. ChatGPT
subscription-backed Codex usage goes through Codex local auth, not an
`OPENAI_API_KEY` in `multiagent.toml`.

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
- `librarian` (read, answer; disabled by default)
- `designer` (read, edit, verify; disabled by default)

Capabilities are enforced by the harness, not by runtime implementations.
If `tools` is set for an agent, it further narrows the harness actions exposed
inside those capabilities.

## Council

The council is a harness workflow, not a normal agent. The orchestrator may route
to `next_agent = "council"` only for high-risk architecture, security, data
integrity, difficult review, or explicit user council requests.

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
- `src/tui` – terminal UI and user input
- `src/runtime` – runtime adapters and contracts
- `src/actions` – file and command action execution
- `src/history` – event/artifact/run persistence
- `src/codemap` – folder-level repository map generation
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
