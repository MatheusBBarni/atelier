# Config-Setup Skill — a default, npm-installable skill that teaches an LLM to author `atelier.toml`

## Overview

New atelier users hit a wall at first run: `atelier.toml` spans six sections and five enums,
and because the loader is `deny_unknown_fields`, a single typo'd key or wrong enum value fails
the entire config with an opaque error. This idea ships a **default `config-setup` skill** — a
portable `SKILL.md` any LLM agent can invoke — that runs an essentials-first wizard, produces a
valid config, and self-validates it. It is delivered over the **`skills.sh` consent rail**
(`npx skills add …`) and bundled with atelier so atelier's own runtime sees it too. The V1 is a
**Quick Win with a compounding foundation**: the skill rides existing skill-discovery dirs and
reuses `atelier --print-config`, while establishing a reusable distribution rail for every
future atelier skill.

## Summary / Differentiator

No competing AI-agent CLI (Claude Code, OpenCode, Aider, Cursor, Goose, Gemini CLI) ships an
**LLM-runnable "configure me" skill** — they all leave config as manual JSON/TOML editing or
MCP paste. Atelier turns its own config schema into portable agent expertise, installable
through the same `skills.sh` rail 20+ agents already use. The moat isn't the Markdown
(copyable) — it's being the **canonical, version-synced, schema-accurate** source bundled with
the binary.

## Problem

A first-time adopter installs atelier via npm, opens their everyday agent (Claude Code/Cursor),
and is asked to produce an `atelier.toml`. They must know that `runtime` references a
`[runtimes.<id>]` whose `kind` is one of `codex|claude|cursor|zai|fake`; that `approval_mode`
is `yolo|normal`; that `effort` is `minimal|low|medium|high|xhigh`; that `capabilities` and
`tools` are constrained enum lists; and that config merges across
`~/.config/.atelier/atelier.toml`, `./atelier.toml`, and CLI flags. None of this is
discoverable from the error messages. Because `RawConfig` rejects unknown fields, a near-miss
like `[runtimes.codx]` or `effort = "har"` fails the whole load — a newcomer can burn fifteen
minutes on a one-character slip.

The host agent is no better off: lacking ground truth, it hallucinates plausible-but-wrong
field names. The result is trial-and-error against `--doctor` (which today can't even fail — it
exits 0), or abandonment. Atelier is usable by experts without help, but the config cliff
narrows the top of the funnel exactly where first impressions are formed.

### Market Data

- **`skills.sh` is "npm for Agent Skills"** (Vercel's registry): `npx skills add owner/repo`
  drops a `SKILL.md` into `.claude/skills/` or `~/.agents/skills/`, consumable by 20+ agents —
  the exact dirs atelier already discovers from.
  ([dev.to](https://dev.to/stevengonsalvez/skillssh-npm-for-agent-skills-35jc),
  [Vercel KB](https://vercel.com/kb/guide/agent-skills-creating-installing-and-sharing-reusable-agent-context))
- **Silent npm postinstall is being removed:** npm v12 (~July 2026) disables install scripts by
  default; pnpm v10 already blocks them; security guidance pushes `--ignore-scripts`.
  ([Semgrep](https://semgrep.dev/blog/2026/rip-npm-postinstall-scripts-npm-v12-default-change/))
  → consent-based install is the durable path.
- **Proven onboarding pattern:** `eslint --init`, `prisma init`, `aws configure` — short Q&A →
  valid config with sane defaults.
  ([ESLint CLI](https://eslint.org/docs/latest/use/command-line-interface)) This skill is the
  portable, LLM-driven analog.

## Core Features

| #  | Feature | Priority | Description |
| -- | ------- | -------- | ----------- |
| F1 | Portable `config-setup` SKILL.md | Critical | One engine-agnostic skill (minimal `name`+`description` frontmatter for atelier; richer tolerated by host agents) usable by atelier's own runtime *and* external agents to author/edit `atelier.toml`. |
| F2 | Essentials-first guided wizard | Critical | Short Q&A: runtime → model (+fallbacks) → `approval_mode` → one starter agent; then progressive disclosure into `presets`/`council`/`limits`/`ui`/`workspace`. Emits complete, valid TOML. |
| F3 | Whole-config schema reference | Critical | Documents every section, all 5 enums (`RuntimeKind`/`ApprovalMode`/`AgentEffort`/`Capability`/`ToolName`), merge order, and file locations, with annotated examples. `deny_unknown_fields` makes accuracy mandatory. |
| F4 | Self-validate with graceful degradation | High | After writing, run `atelier --print-config` (+`--doctor`) and fix until clean *if atelier is on PATH*; otherwise write schema-correct TOML and instruct the user to run `atelier --doctor`. No hard dependency on running atelier. |
| F5 | Anti-drift anchoring | High | Skill instructs the agent to read `atelier --init-config`/`--print-config` output as ground truth before editing, and is pinned to the config `schema_version`. |
| F6 | Consent-based skills.sh delivery | High | Publish so `npx skills add MatheusBBarni/atelier` installs the SKILL.md into the user's skills dir; bundle the same file in atelier's repo dirs for its own runtime. "Install with npm" via the skills.sh rail. |
| F7 | Non-writing postinstall hint | Medium | Any npm lifecycle script only prints a one-line hint ("run `npx skills add …` to enable the config helper"); never writes to skills dirs without explicit, TTY-gated consent (`--yes` for CI). |

## KPIs

| KPI | Target | How to Measure |
| --- | ------ | -------------- |
| First-attempt config validity | ≥ 90% of generated configs pass `atelier --print-config` on first write | Eval harness over a scenario matrix |
| Schema accuracy (zero hallucination) | 0 unknown keys / invalid enum values across fixtures | Automated load test (`deny_unknown_fields` makes it binary) |
| Time-to-valid-config | Median < 5 min from install → accepted config | Timed onboarding sessions |
| Consent-safe install | 100% prompt before writing skills dir; 0 writes in CI/non-TTY without `--yes` | Installer integration test (TTY vs piped stdin) |
| Config-surface coverage | 100% of 6 sections + 5 enums handled correctly | Scenario matrix |
| Dogfood adoption | Listed in atelier `/skill:` dropdown + on skills.sh within 1 release | Presence check |

## Feature Assessment

| Criteria | Question | Score |
| -------- | -------- | ----- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Maybe |
| **Differentiation** | Does this set us apart or just match competitors? | Strong |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: **Quick Win with a compounding foundation**

## Council Insights

- **Recommended approach:** One portable SKILL.md running a whole-config, essentials-first
  wizard; self-validates via `--print-config` when available; delivered over the skills.sh
  consent rail + repo-bundled for atelier's runtime; anchored to atelier's own emitted config to
  fight drift.
- **Key trade-offs:** Convenience (auto postinstall) vs safety/longevity (consent-based `npx`)
  → safety wins. Comprehensive coverage vs essentials → keep coverage, sequence essentials-first.
  Hand-written schema vs generated truth → anchor to generated truth.
- **Risks identified:** Schema drift (→ anchor to `--init-config`/`--print-config`, pin
  `schema_version`, CI check on enum lists); install friction (→ printed hint + docs, consider
  native `atelier skills install` in V2); false "valid" signal since `--doctor` exits 0 today
  (→ gate on `--print-config`, adopt `--doctor --strict` when shipped).
- **Stretch goal (V2+):** A skill pack (`add-runtime`, `add-agent`, `troubleshoot-doctor`) on
  the same rail, plus native `atelier --emit-config-schema` + `atelier setup` to eliminate drift
  at the source.

## Integration with Existing Features

| Integration Point | How |
| ----------------- | --- |
| Skill discovery (`src/skills/mod.rs`) | SKILL.md drops into existing roots; appears in `/skill:` dropdown + help "Skills" tab |
| `--print-config` / `--init-config` / `--doctor` | Used for self-validation and as schema ground-truth anchor |
| `config-validation-ux` packet (`--doctor --strict`) | Future: skill can gate on a real non-zero exit once that ships |
| npm assemble / release (`npm/scripts/*`) | Skill bundled into repo skill dirs; version-synced with `schema_version` |

## Out of Scope (V1)

- **Native `atelier --emit-config-schema` / `atelier setup`** — deferred to V2 to avoid core
  Rust work now; V1 anchors drift via existing `--print-config`/`--init-config`.
- **Multi-skill pack (`add-runtime`/`add-agent`/`troubleshoot-doctor`)** — prove the rail with
  one skill first.
- **Silent auto-copy postinstall** — explicitly rejected (npm v12/pnpm + security + "ask the
  user"); consent-based only.
- **Writing secrets into TOML** — the skill sets `api_key_env` (env-var *name*) only; it never
  writes API keys into config.
- **Legacy `multiagent.toml` migration & multi-profile management UI** — not part of the
  first-config experience.

## Architecture Decision Records

- [ADR-001: Portable config-setup skill, essentials-first whole-config wizard, consent-based skills.sh delivery](adrs/adr-001.md) — one portable SKILL.md, whole-config essentials-first, self-validate with graceful degradation, anti-drift anchoring, consent-based skills.sh install (no silent postinstall).

## Open Questions

- **Install entry point:** `npx skills add MatheusBBarni/atelier` (skills.sh telemetry) vs a
  dedicated `@matheusbbarni/atelier-skills` npm package vs a first-class `atelier skills install`
  subcommand — decide in techspec.
- **Schema-version compatibility:** how the skill detects/warns when it's older than the
  installed atelier's `schema_version`.
- **Anchor fidelity:** is `--print-config` text enough, or is a small machine-readable
  `--emit-config-schema` worth pulling into V1 to make F5 robust?
- **Wizard presets:** should it offer ready-made setups (e.g. "Claude-only", "Codex + Claude
  fallback") to cut the Q&A further?
- **CI enum-sync check:** where to assert the skill's documented enums match `src/config/mod.rs`
  so drift fails the build.
