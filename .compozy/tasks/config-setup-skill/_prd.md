# PRD: Config-Setup Skill — an npm-installable skill that configures `atelier.toml` for you

## Overview

New atelier users hit a configuration cliff: `atelier.toml` spans six sections and five enums,
and because the loader rejects unknown fields, a single typo'd key or wrong enum value fails the
entire config with an opaque error. This feature ships a **default `config-setup` skill** — a
portable `SKILL.md` any LLM agent can invoke (atelier's own runtime *or* an external agent like
Claude Code/Cursor) — that runs an essentials-first wizard with named presets, writes a valid
config, and self-validates it when atelier is available. It is delivered over the **skills.sh
convention** (`npx skills add MatheusBBarni/atelier`) and bundled into atelier's own skill
roots. V1 is **greenfield** (builds a fresh config; importing existing conventions is a separate
roadmap item) and a **Quick Win with a compounding foundation** — it rides existing skill
discovery and reuses `atelier --print-config`, while establishing a reusable distribution rail
for future atelier skills.

## Goals

- **Eliminate the first-run config cliff.** A new user goes from install to a working
  `atelier.toml` without learning the schema.
- **Make any agent schema-accurate.** Give host agents authoritative config knowledge so they
  stop hallucinating field/enum names.
- **Establish the skills.sh distribution rail** as atelier's first installable, sharable skill.
- **Measurable targets:** ≥ 90% first-attempt config validity; 0 hallucinated keys across a
  fixture suite; median time-to-working-config < 5 min; 100% coverage of the 6 sections + 5
  enums.
- **Milestone:** ship within one release; installable via `npx skills add` and listed on
  skills.sh; discoverable in atelier's `/skill:` dropdown.

## User Stories

- **Dana (first-time adopter):** As a new atelier user, I want to ask my agent to set up atelier
  so that I get a working config without reading the schema docs.
- **Dana (preset path):** As a new user who only has the Claude CLI, I want to pick a
  "Claude-only" starter and tweak the model, so that I'm running in under a minute.
- **Omar (returning user):** As an existing user, I want the skill to add a runtime / agent /
  preset with correct field names so that I don't break the config on a typo.
- **Host agent:** As an AI coding agent, I want authoritative, current schema guidance so that
  the config I write loads cleanly the first time.
- **External-agent user (no atelier yet):** As someone driving Claude Code/Cursor before
  installing atelier, I want the skill to still produce a valid config and tell me how to verify
  it later.

## Core Features

| # | Feature | Priority | What it does / functional requirements |
| - | ------- | -------- | -------------------------------------- |
| F1 | Portable `config-setup` skill | Critical | One engine-agnostic `SKILL.md` (minimal `name`+`description` frontmatter; body < 500 lines; detail in `references/`) usable by atelier's runtime *and* external agents. Must not assume which engine reads it. |
| F2 | Essentials-first wizard with named presets | Critical | Offers starter presets (e.g. *Claude-only*, *Codex + Claude fallback*, *Cursor*), suggested by which runtime CLI is detected; otherwise walks runtime → model (+fallbacks) → `approval_mode` → one starter agent, then progressively discloses `presets`/`council`/`limits`/`ui`/`workspace`. Emits a complete, valid config. |
| F3 | Whole-config schema reference | Critical | A `references/` schema doc covering every section, all 5 enums (`RuntimeKind`/`ApprovalMode`/`AgentEffort`/`Capability`/`ToolName`), the merge order, and file locations, with annotated examples. Accuracy is mandatory (unknown keys fail the load). |
| F4 | Self-validate with graceful degradation | High | If `atelier` is on PATH, run `atelier --print-config` (+`--doctor`) and fix until clean; otherwise **write a schema-correct config and instruct** the user to run `atelier --doctor`. Never blocks on atelier being installed. |
| F5 | Anti-drift anchoring | High | Instructs the agent to read `atelier --init-config` / `--print-config` output as ground truth before editing, and pins the skill to the config `schema_version`. |
| F6 | skills.sh + repo-bundle delivery | High | Installable via `npx skills add MatheusBBarni/atelier` into any agent's roots; mirrored into atelier's own `.agents/skills` + `.claude/skills` for its runtime/TUI. |
| F7 | Non-writing install hint | Medium | Any npm lifecycle output only prints a one-line hint pointing to the install command; it never writes to skill roots without explicit user action. |

## User Experience

**Journey:** discover → install → invoke → choose preset or answer essentials → skill writes
`atelier.toml` → self-validate or instruct → done.

1. **Discover** via README, the `/skill:` dropdown, atelier's help "Skills" tab, or the skills.sh
   listing.
2. **Install** with `npx skills add MatheusBBarni/atelier` (external agents) — or it's already
   present for atelier's own runtime (repo-bundled). A post-install hint nudges the one-liner.
3. **Invoke** with `/skill:config-setup` (atelier/host agent) or simply "set up my atelier
   config".
4. **Configure:** the skill suggests a named preset based on the detected runtime CLI, or runs
   the short essentials flow; advanced sections are offered, not forced.
5. **Validate:** if atelier is available, it confirms with `--print-config`/`--doctor` and fixes
   issues; otherwise it writes the config and tells the user exactly how to verify.
6. **Done:** the user has a loading config and a one-line summary of what each chosen setting
   means.

**UX considerations:** pure text/markdown (works in any agent, accessible by default); writes are
explicit and reviewable; **never writes secrets** — it sets `api_key_env` (the env-var *name*)
and tells the user to export the key.

## High-Level Technical Constraints

- The skill must be placed in **all discovered skill roots** and exposed where `npx skills add`
  resolves it.
- Frontmatter limited to `name` + `description` for atelier compatibility; body < 500 lines
  (progressive disclosure into `references/`).
- Generated config must satisfy the **unknown-field-rejecting** schema (any stray key fails the
  whole load).
- Self-validation gates on `atelier --print-config` (today `--doctor` exits 0 and cannot fail on
  its own).
- The skill is **version-synced** with the config `schema_version`.
- **No secrets in config files** — env-var names only.

## Non-Goals (Out of Scope)

- **Importing existing conventions** (`CLAUDE.md`/`.claude/skills`/`.mcp.json`) — owned by the
  separate "config importer" roadmap item (ADR-003).
- **Native `atelier skill add` registry + lockfile** — deferred to its roadmap item (ADR-002).
- **Writing API keys/secrets into TOML** — env-var names only.
- **Multi-skill pack** (`add-runtime`/`add-agent`/`troubleshoot-doctor`) — V2.
- **Native `atelier --emit-config-schema` / `atelier setup`** — V2.
- **Legacy `multiagent.toml` migration and multi-profile management** — not part of first-config.

## Phased Rollout Plan

### MVP (Phase 1)

F1–F7: greenfield essentials-first wizard with named presets, whole-config `references/`,
self-validate-with-degradation, anti-drift anchoring, skills.sh publish + repo bundle,
non-writing hint.

**Proceed criteria:** ≥ 90% first-attempt validity on a scenario matrix; installable via
`npx skills add`; appears in atelier's `/skill:` dropdown.

### Phase 2

Optional thin `atelier skills install` (atelier-owned consent UX, ADR-002 Alt 2), a
machine-readable schema anchor for F5, and additional presets.

**Proceed criteria:** measurable reduction in install friction / external-tool dependency;
healthy invocation numbers.

### Phase 3

Skill pack (`add-runtime`/`add-agent`/`troubleshoot-doctor`) on the same rail; bridge to the
native skill registry/lockfile and the config importer.

**Long-term:** atelier's skills marketplace on-ramp.

## Success Metrics

| Metric | Target |
| ------ | ------ |
| First-attempt config validity | ≥ 90% pass `atelier --print-config` on first write |
| Schema accuracy | 0 unknown keys / invalid enum values across fixtures |
| Time-to-working-config | Median < 5 min from install → accepted config |
| Config-surface coverage | 100% of 6 sections + 5 enums handled |
| Preset adoption | ≥ 50% of runs start from a named preset |
| Dogfood adoption | Listed on skills.sh + in atelier `/skill:` within 1 release |

## Risks and Mitigations

- **Opt-in install nobody runs** → post-install hint, README, `/skill:` discoverability,
  skills.sh listing.
- **External dependency on skills.sh** (availability/changes) → repo-bundle keeps atelier's own
  runtime independent; document a manual-copy fallback; revisit the native installer (Phase 2).
- **npm v12 disables install scripts by default** → surface the hint via first-run output/README
  rather than relying on a lifecycle script.
- **Schema drift** (config evolves, skill rots) → anchor to atelier's own output, version-sync,
  CI enum-sync check.
- **Confusion vs the config importer** → docs cross-reference clarifying greenfield-vs-import.
- **False "valid" signal** (`--doctor` exits 0 today) → gate on `--print-config`; adopt
  `--doctor --strict` when the sibling packet ships.
- **Low frequency** (config is occasional) → value concentrated at first-run; the references
  double as an ongoing edit aid.

## Architecture Decision Records

- [ADR-001: Portable config-setup skill, essentials-first whole-config wizard, consent-based skills.sh delivery](adrs/adr-001.md) — the overall scope and anti-drift posture.
- [ADR-002: Deliver V1 via the skills.sh convention + repo bundle (not a native installer)](adrs/adr-002.md) — chose Approach A over the native registry/hybrid.
- [ADR-003: V1 builds config greenfield; importing existing tool conventions is out of scope](adrs/adr-003.md) — boundary against the config-importer roadmap item.

## Open Questions

- **Repo layout for `npx skills add`:** does the installer resolve a root `SKILL.md`, a `skills/`
  layout, or a subdir — and how do we expose it without disrupting atelier's own `.agents/skills`
  mirroring (techspec).
- **Which presets ship in V1** (Claude-only, Codex + Claude fallback, Cursor, Zai) and their
  default models.
- **Schema-version mismatch UX:** how the skill detects and warns when it's older than the
  installed atelier.
- **Anchor fidelity:** is `--print-config` text a sufficient ground-truth anchor, or is a
  machine-readable schema needed in V1.
- **CI enum-sync check:** where the assertion lives so config-schema drift fails the build.
- **Post-install hint mechanics** given npm v12's script default change.
