---
name: atelier-config-setup
description: Sets up or repairs an atelier.toml for the atelier multi-agent harness. Use when a user asks to set up / configure / initialize atelier, create an atelier.toml, pick a runtime (Codex, Claude, Cursor, Z.ai), choose models or approval mode, add an agent or preset, or fix a config that fails to load. Runs an essentials-first wizard, writes a schema-valid config, never writes secrets, and self-validates with `atelier --print-config` when atelier is on PATH.
---

# atelier config setup

You are configuring **atelier** — a terminal-native multi-agent harness whose behavior is
driven entirely by one TOML file, `atelier.toml`. Your job is to produce a config that **loads
cleanly the first time**. The loader uses `deny_unknown_fields`, so a single typo'd key or a
wrong enum value fails the *whole* file with an opaque error. Accuracy is the whole game.

This skill is **engine-agnostic**: it works whether you are atelier's own runtime, Claude Code,
Cursor, Codex, or any other agent. Do not assume `atelier` is installed — degrade gracefully
(see *Self-validate*).

Target config `schema_version`: **1**.

## When to use

Trigger on requests like:

- "set up my atelier config" / "configure atelier" / "initialize atelier"
- "create an `atelier.toml`" / "I just installed atelier, now what"
- "add a runtime / agent / preset to atelier"
- "pick a model / approval mode for atelier"
- "my `atelier.toml` won't load / fails with an unknown-field error"

## Scope (V1)

- **Greenfield.** You build a fresh, correct config. Importing existing `CLAUDE.md`,
  `.claude/skills`, or `.mcp.json` conventions is a *separate* roadmap item — out of scope here.
- **No secrets, ever.** You set the *name* of an environment variable (`api_key_env`); you never
  write a key value into TOML. See *The secret rule*.
- This is **guidance**, not an executable program. It does not bypass atelier's approvals,
  capability gates, or workspace permissions — a written config still obeys them at runtime.

## Before you start (anti-drift)

Hand-written schema knowledge rots as atelier evolves. **Anchor to atelier's own output as
ground truth** whenever `atelier` is on PATH:

1. `atelier --init-config` — writes a fully-annotated starter `atelier.toml` (home scope) and
   tells you where it went. This is the canonical, current template — read it before inventing
   structure.
2. `atelier --print-config` — prints the **effective, merged, redacted** config as TOML. Treat
   this as the live source of truth for field names and defaults.

If neither command is available (atelier not installed), fall back to
`references/config-schema.md` in this skill — but say so, and tell the user to re-validate once
atelier is installed.

Never trust your own memory of the schema over `--print-config` output.

## The wizard protocol (essentials-first)

Keep it short. Ask only what is needed, in this order, and offer the advanced sections only
after the essentials are settled. **Detect the user's installed runtime CLI first** (look for
`codex`, `claude`, `cursor-agent` on PATH, or a Z.ai API key in the environment) and use it to
*suggest* a sensible default at each step.

### Step 0 — Preset or essentials?

Offer a **named starter preset** before the manual flow. Suggest one based on the detected CLI:

| Detected | Suggest preset |
|---|---|
| `claude` only | **Claude-only** |
| `codex` (and `claude`) | **Codex + Claude fallback** |
| `cursor-agent` | **Cursor** |
| `ZAI_API_KEY` set, no CLI | **Z.ai HTTP** |

If the user picks a preset, copy the matching block from `references/presets.md`, confirm the
model strings are current (see *Confirm the model*), then jump straight to *Self-validate*. If
they decline, run the essentials flow below.

### Step 1 — Runtime

Ask which backend agent runs the work. `runtime` is one of the five `RuntimeKind` values:
`codex`, `claude`, `cursor`, `zai`, `fake` (`fake` is for tests only — do not ship it). Declare
it under `[runtimes.<id>]` with a `type = "<kind>"`. The only field the loader *requires* is
`api_key_env` on the `zai` runtime; everything else has a default (CLI runtimes default
`command` to `codex`/`claude`/`cursor-agent`, and `zai` defaults `base_url` to the Z.ai API).
Set `command` explicitly when your CLI isn't on the default name, and note that the `claude` and
`cursor` runtimes **reject** `api_key_env` (those CLIs own their own credentials).

### Step 2 — Model (+ fallbacks)

Ask for the primary `model` and an optional ordered `model_fallbacks` list. atelier retries the
same model on retryable provider errors, then walks the fallback chain. Model strings are
free-form and **not** validated by the loader — so always **confirm the real, current model
name** with the user (or via the provider's docs); a stale model string loads fine but fails at
runtime.

### Step 3 — Approval mode

Set top-level `approval_mode`: `yolo` (auto-run actions; the default) or `normal` (surface an
approval prompt for write/command actions). Recommend `normal` for first-time users on real
repos. Optionally set `[approval] floor = "warn"` (default) or `"enforce"` for the gray-area
tier — the catastrophic core always prompts regardless.

### Step 4 — One starter agent

Define at least an `[agents.orchestrator]` with a `runtime`, `model`, `effort` (an `AgentEffort`:
`minimal` / `low` / `medium` / `high` / `xhigh`), `capabilities` (a list of `Capability`
values), and `instructions` (inline) or `instructions_file` (path). Keep it to one agent for the
essentials flow; more agents are a progressive-disclosure step.

> Use **inline `instructions`** in generated configs unless the user already has prompt files.
> `instructions_file` points at a path that must exist, so an inline string is safer for a
> first config.

### Step 5 — Progressive disclosure (offer, don't force)

Only after essentials work, offer the advanced sections. Each is optional:

- `[presets.*]` — named agent-override bundles, applied before local overrides.
- `[council]` — the serial review workflow (`default_preset`, `timeout_seconds`,
  `execution_mode`, `[council.presets.*]` members).
- `[limits]` — step / action / wall-clock / command ceilings.
- `[ui]` — `hide_banner`, prompt-history toggles.
- `[workspace]` — extra read/write roots, unrestricted-reads toggle.
- `[features]` — feature flags (e.g. `parallel_step_groups`, `mcp_enabled`).
- `[mcp]` — harness-owned MCP servers (gated behind `features.mcp_enabled`).
- `[keybindings]` / `[hooks]` — **user-scope only** (honored from the home config or an explicit
  `--config`, ignored in a project-local file for safety).

Full field-by-field detail, every enum, defaults, and the merge order live in
`references/config-schema.md`. Read it on demand — do not paste it inline.

## Where the file goes

Two layers, merged **builtin defaults → home → local → CLI flags**:

- **Home (user-wide):** `~/.config/.atelier/atelier.toml` — what `atelier --init-config` writes.
- **Local (per-project):** `./atelier.toml` in the working directory — overrides home.

Write to the home file for a global setup, or `./atelier.toml` to scope settings to one repo.
A later layer overrides an earlier one key-by-key.

## The secret rule

**Never write a secret value into TOML.** For any runtime that needs a key, set `api_key_env`
to the *name* of the environment variable that holds it, and tell the user to export it:

- ✅ In `atelier.toml`: `api_key_env = "ZAI_API_KEY"`
- ✅ In the user's shell: `export ZAI_API_KEY=...` (outside the repo, never committed)
- ❌ Never: an `api_key = "sk-..."` literal, or the key value anywhere in the file.

`atelier --print-config` redacts env-var *values*; it only ever shows the variable *name*. Keep
it that way.

## Self-validate (with graceful degradation)

After writing the config, prove it loads. **The gate is `--print-config`, not `--doctor`.**

1. **Is `atelier` on PATH?**
   - **Yes →** run `atelier --print-config`.
     - **Non-zero exit / error →** the config is invalid (unknown key, bad enum, malformed
       TOML). Read the error, fix the offending key, and re-run. Repeat until it exits 0.
     - **Exit 0 →** the config is schema-valid. Then run `atelier --doctor --json` as an
       **advisory** health check (e.g. a runtime CLI not installed). Doctor is advisory: missing
       runtimes are warnings and do not fail the config. (Newer builds support
       `atelier --doctor --strict`, which exits non-zero on errors — use it if you want a hard
       gate.) Summarize any warnings for the user; fix the ones that matter.
   - **No →** **write the schema-correct config anyway** (using `references/config-schema.md`)
     and tell the user exactly how to verify once atelier is installed:
     > "Config written to `<path>`. After installing atelier, run `atelier --print-config` to
     > confirm it loads, then `atelier --doctor` to check your runtime setup."

Never block on atelier being installed — degrade to write-and-instruct.

## Example: an essentials result

A minimal, valid config from the essentials flow (Z.ai HTTP runtime, one orchestrator agent).
Every key and enum value here loads under atelier's config loader:

```toml
schema_version = 1
approval_mode = "normal"

[runtimes.zai]
type = "zai"
base_url = "https://api.z.ai/api/paas/v4"
api_key_env = "ZAI_API_KEY"   # the env-var NAME, never the key value

[agents.orchestrator]
runtime = "zai"
model = "glm-4.6"
model_fallbacks = ["glm-4.5"]
effort = "high"
capabilities = ["plan"]
instructions = "Plan the work, then delegate each step to the right agent."
```

For a CLI runtime (`codex` / `claude` / `cursor`), drop `base_url`/`api_key_env` and set
`command` instead (e.g. `[runtimes.claude]` with `type = "claude"`, `command = "claude"`).

## References

- **`references/config-schema.md`** — the whole-config schema: every section, every field's
  type and default, all five enums (`RuntimeKind`, `ApprovalMode`, `AgentEffort`, `Capability`,
  `ToolName`) by their serde names, the merge order, and file locations. Use it to look up exact
  key names so generated configs pass `--print-config`.
- **`references/presets.md`** — copy-paste-ready named starter presets (Claude-only, Codex +
  Claude fallback, Cursor, Z.ai HTTP), each a self-contained, loadable `atelier.toml`.
