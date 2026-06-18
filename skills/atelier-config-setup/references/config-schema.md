# atelier.toml — whole-config schema reference

The complete, current reference for `atelier.toml`. atelier's loader uses
`deny_unknown_fields`, so **any key or enum value not listed here fails the entire file** with an
opaque error. When in doubt, anchor to atelier's own output (`atelier --init-config` /
`atelier --print-config`) — it is the live source of truth (see *Anti-drift* in `../SKILL.md`).

Target **`schema_version = 1`**.

## File locations & merge order

Config is merged from up to four layers, each overriding the previous **key by key**:

1. **Built-in defaults** — compiled into atelier; everything below is optional.
2. **Home (user-wide):** `~/.config/.atelier/atelier.toml` — what `atelier --init-config` writes.
3. **Local (per-project):** `./atelier.toml` in the working directory.
4. **CLI flags** — explicit flags / `--config <path>` for the current run.

A later layer wins. `[keybindings]` and `[hooks]` are honored **only** from user-scope layers
(home or an explicit `--config`); a project-local `./atelier.toml` cannot set them (security).

## Enums

These are the only valid values for each typed field, by their exact serde (TOML) names. (Source
of truth: `src/config/mod.rs`.)

**`RuntimeKind`** — a `[runtimes.<id>]` `type`:

```text
codex
claude
cursor
zai
fake
```

(`fake` is a deterministic test runtime — do not ship it in a real config.)

**`ApprovalMode`** — top-level `approval_mode`:

```text
yolo
normal
```

**`AgentEffort`** — an agent's / council member's `effort`:

```text
minimal
low
medium
high
xhigh
```

**`Capability`** — entries in an agent's `capabilities` list:

```text
plan
read
answer
challenge
edit
command
verify
review
mcp_tool
```

`mcp_tool` is required to call harness-brokered MCP tools (default-deny); MCP *resources* are
read-class and need only `read`.

**`ToolName`** — entries in an agent's `tools` list:

```text
read_file
list_files
search_text
run_command
apply_patch
write_file
record_note
call_mcp_tool
read_mcp_resource
list_mcp_resources
```

## Top-level scalars

| Key | Type | Default | Notes |
|---|---|---|---|
| `schema_version` | integer | `1` | Must be `1`; any other value is rejected. |
| `approval_mode` | `ApprovalMode` | `yolo` | `yolo` auto-runs actions; `normal` prompts for write/command actions. |
| `preset` | string | — | Name of a `[presets.<name>]` bundle to activate (applied before local overrides). |

## `[approval]`

| Key | Type | Default | Notes |
|---|---|---|---|
| `floor` | `FloorPolicy` (`warn`/`enforce`) | `warn` | Gray-area posture. `warn` surfaces the risk but still auto-runs under `yolo`; `enforce` re-prompts. The catastrophic core always prompts and cannot be disabled. |

## `[features]`

Feature flags. All default **off**.

| Key | Type | Default | Notes |
|---|---|---|---|
| `parallel_step_groups` | bool | `false` | Run independent step groups concurrently (bounded by `limits.max_parallel_agent_steps`). |
| `governance_early_abort` | bool | `false` | Abort a run early on a governance signal. |
| `execution_graph` | bool | `false` | Use the execution-graph scheduler. |
| `mcp_enabled` | bool | `false` | Master switch for MCP. Servers are *parsed* regardless, but no connection is started and no MCP tools are offered until this is `true`. |

## `[ui]`

| Key | Type | Default | Notes |
|---|---|---|---|
| `hide_banner` | bool | `false` | Suppress the welcome banner. |
| `prompt_history_enabled` | bool | `true` | ↑/↓ recall of past prompts. |
| `prompt_history_max` | integer | `200` | How many past prompts to keep for recall. |

## `[workspace]`

| Key | Type | Default | Notes |
|---|---|---|---|
| `extra_read_roots` | list of paths | `[]` | Additional directories the model may read. |
| `extra_write_roots` | list of paths | `[]` | Additional directories the model may write. |
| `allow_unrestricted_reads` | bool | `false` | Opt-in: allow reads of any absolute path (reads only; writes still gate on `extra_write_roots`). |

## `[limits]`

Each `*_minutes`/`*_steps`/`*_actions`/`*_cycles` value is a **`Limit`**: a positive integer or
the string `"unlimited"`. `max_parallel_agent_steps` is a plain integer (`0` disables parallelism).

| Key | Type | Default |
|---|---|---|
| `max_agent_steps` | `Limit` | `12` |
| `max_step_actions` | `Limit` | `20` |
| `max_wall_clock_minutes` | `Limit` | `30` |
| `max_step_minutes` | `Limit` | `10` |
| `max_command_minutes` | `Limit` | `10` |
| `max_review_fix_cycles` | `Limit` | `2` |
| `max_parallel_agent_steps` | integer | `2` |

## `[runtimes.<id>]`

Declares a backend agent runtime. `<id>` is the name agents reference via their `runtime` field.
Built-in `codex`/`claude`/`cursor`/`zai` runtimes exist by default; redeclaring one overrides it.

| Key | Type | Default | Notes |
|---|---|---|---|
| `type` | `RuntimeKind` | — | Which runtime implementation. |
| `command` | string | — | Executable for CLI runtimes (`codex`/`claude`/`cursor`). |
| `args` | list of strings | `[]` | Extra args passed to `command`. |
| `prompt_mode` | `stdin` | `stdin` | How the prompt is delivered (only `stdin` in V1). |
| `base_url` | string | — | HTTP base URL for the `zai` runtime. |
| `api_key_env` | string | — | **Name** of the env var holding the API key — never the key itself. |
| `degrade_not_abandon` | bool | — | On exhausted fallbacks, degrade rather than abandon the run. |

## `[mcp]` / `[mcp.servers.<id>]`

Harness-owned MCP servers. Gated by `features.mcp_enabled` — parsed always, connected only when
that flag is on. Declare each server under `[mcp.servers.<id>]`.

| Key | Type | Default | Notes |
|---|---|---|---|
| `transport` | `stdio` / `http` | `stdio` | V1 wires `stdio` only; `http` parses but is inert until V1.1. |
| `command` | string | — | Required for `stdio`: the server executable. |
| `args` | list of strings | `[]` | Args for `command`. |
| `env` | table (name → value) | `{}` | Environment for the server subprocess. Values may reference `${VAR}`; `--print-config` shows only the names, never the values. |
| `url` | string | — | For `http` transport (inert in V1). |

## `[agents.<id>]`

A named agent profile. `<id>` is the agent name (e.g. `orchestrator`). Use **inline
`instructions`** for a self-contained config; the `*_file` variants point at paths that must
exist.

| Key | Type | Default | Notes |
|---|---|---|---|
| `name` | string | `<id>` | Internal name override. |
| `display_name` | string | — | Human-facing label. |
| `runtime` | string | — | A `[runtimes.<id>]` id. |
| `model` | string | — | Model name (free-form; not validated — confirm it is current). |
| `model_fallbacks` | list of strings | `[]` | Ordered fallback models. |
| `effort` | `AgentEffort` | `medium` | Reasoning effort. |
| `thinking` | bool | — | Enable extended thinking where the runtime supports it. |
| `capabilities` | list of `Capability` | `[]` | What the agent is allowed to do. |
| `tools` | list of `ToolName` | — | Action tools the agent may use (each tool implies a capability). |
| `instructions` | string | — | Inline system prompt. |
| `instructions_file` | path | — | System prompt from a file (must exist). |
| `instructions_append_file` | path | — | Extra prompt appended from a file (must exist). |
| `orchestrator_description` | string | — | How the orchestrator describes/dispatches this agent. |
| `orchestrator_description_file` | path | — | Same, from a file (must exist). |
| `enabled` | bool | `true` | Set `false` to keep a profile defined but inactive. |

## `[presets.<name>]`

A named bundle of agent overrides, activated by the top-level `preset` field and applied
**before** the local layer. Contains an `agents` map with the same fields as `[agents.<id>]`.

## `[council]`

The serial review workflow.

| Key | Type | Default | Notes |
|---|---|---|---|
| `default_preset` | string | `default` | Which `[council.presets.<name>]` to use. |
| `timeout_seconds` | integer | `900` | Per-council timeout. |
| `execution_mode` | `serial` | `serial` | Only `serial` in V1. |

Members live under `[council.presets.<preset>.<member>]`, each with: `runtime`, `model`,
`model_fallbacks` (list), `effort` (`AgentEffort`), `thinking` (bool), and a prompt via inline
`prompt` or `prompt_file` (path, must exist).

## `[keybindings.<context>]` and `[hooks]` (user-scope only)

These exist but are honored **only** from a user-scope layer (home config or explicit
`--config`); a project-local `./atelier.toml` setting them is ignored. They are large and best
authored from the annotated template — run `atelier --init-config` and read the generated file
for the full, current key/event lists rather than hand-writing them.

---

## Annotated examples

Each block below is a complete, individually-loadable `atelier.toml`.

### Top-level + scalar sections

```toml
schema_version = 1
approval_mode = "normal"

[approval]
floor = "warn"

[features]
parallel_step_groups = false
mcp_enabled = false

[ui]
hide_banner = false
prompt_history_enabled = true
prompt_history_max = 200

[workspace]
extra_read_roots = ["../shared"]
allow_unrestricted_reads = false

[limits]
max_agent_steps = 12
max_command_minutes = "unlimited"
max_parallel_agent_steps = 2
```

### Runtimes (CLI + HTTP)

```toml
schema_version = 1

[runtimes.codex]
type = "codex"
command = "codex"
args = ["exec", "--skip-git-repo-check", "--color", "never"]
prompt_mode = "stdin"

[runtimes.claude]
type = "claude"
command = "claude"

[runtimes.cursor]
type = "cursor"
command = "cursor-agent"

[runtimes.zai]
type = "zai"
base_url = "https://api.z.ai/api/paas/v4"
api_key_env = "ZAI_API_KEY"
```

### MCP servers

```toml
schema_version = 1

[features]
mcp_enabled = true

[mcp.servers.filesystem]
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
env = { FASTMCP_LOG_LEVEL = "ERROR" }
```

### Agents (capabilities + tools + fallbacks)

```toml
schema_version = 1

[agents.orchestrator]
runtime = "claude"
model = "claude-opus-4-6"
effort = "high"
capabilities = ["plan"]
instructions = "Plan the work, then delegate each step to the right agent."

[agents.fixer]
runtime = "codex"
model = "gpt-5-codex"
model_fallbacks = ["gpt-5"]
effort = "high"
capabilities = ["read", "edit", "command", "verify"]
tools = ["read_file", "search_text", "apply_patch", "write_file", "run_command"]
instructions = "Implement the requested change, then verify it builds and passes tests."
```

### Presets + preset selection

```toml
schema_version = 1
preset = "lean"

[presets.lean.agents.orchestrator]
runtime = "claude"
model = "claude-haiku-4-5"
effort = "medium"
capabilities = ["plan"]
instructions = "Keep plans short; prefer the cheapest agent that can do the job."
```

### Council

```toml
schema_version = 1

[council]
default_preset = "default"
timeout_seconds = 900
execution_mode = "serial"

[council.presets.default.architect]
runtime = "zai"
model = "glm-4.6"
effort = "high"
thinking = true
prompt = "Review the design for architectural soundness and risk."

[council.presets.default.reviewer]
runtime = "zai"
model = "glm-4.6"
effort = "medium"
thinking = true
prompt = "Review the diff for correctness and missed edge cases."
```
