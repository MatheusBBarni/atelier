# Named starter presets

Copy-paste-ready `atelier.toml` starters. Each block is a **complete, standalone config** that
loads under atelier's loader on its own (inline agent `instructions`, no external prompt files,
credentials as env-var *names* only). Pick the one matching your installed runtime CLI, paste it
into `./atelier.toml` (or `~/.config/.atelier/atelier.toml`), then do the two-step confirm at the
end of each preset.

> **Models are illustrative.** Model strings are free-form and load regardless, so the names
> below may be stale. Always confirm the current model for your runtime before relying on it.

## Claude-only

For users who only have the Claude CLI on PATH.

```toml
schema_version = 1
approval_mode = "normal"

[runtimes.claude]
type = "claude"
command = "claude"

[agents.orchestrator]
runtime = "claude"
model = "claude-opus-4-6"
effort = "high"
capabilities = ["plan"]
instructions = "Plan the work, break it into steps, and delegate each step to the right agent."
```

**Confirm:** (1) the `model` is a real, current Claude model; (2) the `claude` CLI is installed
and authenticated (Claude owns its own credentials — do **not** set `api_key_env` here).

## Codex + Claude fallback

Codex as the primary runtime, with Claude declared as a second runtime and a fallback model on
the orchestrator. `model_fallbacks` are tried in order when the primary model errors.

```toml
schema_version = 1
approval_mode = "normal"

[runtimes.codex]
type = "codex"
command = "codex"
args = ["exec", "--skip-git-repo-check", "--color", "never"]

[runtimes.claude]
type = "claude"
command = "claude"

[agents.orchestrator]
runtime = "codex"
model = "gpt-5-codex"
model_fallbacks = ["claude-opus-4-6"]
effort = "high"
capabilities = ["plan"]
instructions = "Plan the work, break it into steps, and delegate each step to the right agent."
```

**Confirm:** (1) both model names are current; (2) `codex` (and `claude`, if you switch an agent
to it) are installed and authenticated. `model_fallbacks` run on the agent's own runtime — to run
some agents on Codex and others on Claude, give each agent the matching `runtime`.

## Cursor

For users driving the Cursor agent CLI (`cursor-agent`).

```toml
schema_version = 1
approval_mode = "normal"

[runtimes.cursor]
type = "cursor"
command = "cursor-agent"

[agents.orchestrator]
runtime = "cursor"
model = "gpt-5"
effort = "high"
capabilities = ["plan"]
instructions = "Plan the work, break it into steps, and delegate each step to the right agent."
```

**Confirm:** (1) the `model` is one Cursor supports; (2) `cursor-agent` is installed and signed in
(Cursor owns its credentials — do **not** set `api_key_env` here).

## Z.ai HTTP

The HTTP runtime — no local CLI needed, just an API key exported in your shell.

```toml
schema_version = 1
approval_mode = "normal"

[runtimes.zai]
type = "zai"
base_url = "https://api.z.ai/api/paas/v4"
api_key_env = "ZAI_API_KEY"

[agents.orchestrator]
runtime = "zai"
model = "glm-4.6"
model_fallbacks = ["glm-4.5"]
effort = "high"
capabilities = ["plan"]
instructions = "Plan the work, break it into steps, and delegate each step to the right agent."
```

**Confirm:** (1) export the key under the env var named here — `export ZAI_API_KEY=...` — and
never write the key into the TOML; (2) the `model` is a current Z.ai model.

## After choosing a preset

1. **Add more agents** as needed — copy the `[agents.orchestrator]` shape, change the id, set
   `capabilities`/`tools`, and write inline `instructions`. See `config-schema.md` for every
   field and enum value.
2. **Validate** with `atelier --print-config` (must exit 0 — the schema gate) and then
   `atelier --doctor` (advisory: checks your runtime CLI is installed/authenticated).
3. **MCP and other optional sections** (`[mcp.servers.*]`, `[council]`, `[limits]`, `[ui]`,
   `[workspace]`) are not included here to keep each preset minimal and dependency-free; add them
   from `config-schema.md` when you need them. MCP also requires `[features] mcp_enabled = true`
   and a reachable server.
