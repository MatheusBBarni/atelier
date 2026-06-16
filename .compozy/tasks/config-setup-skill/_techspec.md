# TechSpec: `atelier-config-setup` Skill

## Executive Summary

The deliverable is a **Markdown agent skill**, not runtime code: a portable `SKILL.md` (+
`references/`) that any LLM agent loads to author a valid `atelier.toml`. The engineering work is
therefore (1) **content** — a lean `SKILL.md` wizard protocol plus a `references/` schema/presets
pack that mirrors `src/config/mod.rs` exactly; (2) **a thin distribution/sync harness** — a canonical
`skills/atelier-config-setup/` source mirrored into atelier's discovery dirs, installable via
`npx skills add`; and (3) **a deterministic test/anti-drift guard** in Rust that proves the
documented schema and every shipped preset load under the real config loader.

The primary trade-off (ADR-005): we accept that a hand-written Markdown schema *can* drift from the
Rust types, and buy that risk down with a CI test (enum coverage + load every ` ```toml ` block)
rather than generating the schema from source (the heavier `--emit-config-schema`, deferred to V2).
Secondary trade-off (ADR-006): we depend on the external `skills` CLI for cross-agent installs
instead of bundling/native code, keeping V1 to zero packaging changes.

## System Architecture

### Component Overview

- **Canonical skill (`skills/atelier-config-setup/`)** — source of truth. Contains `SKILL.md`
  (frontmatter + wizard protocol, body < 500 lines) and `references/config-schema.md` +
  `references/presets.md` (progressive disclosure).
- **Discovery mirrors (`.agents/skills/…`, `.claude/skills/…`)** — generated copies so atelier's own
  runtime and Claude-Code dogfooding discover the skill. Never hand-edited.
- **Sync harness (`npm/scripts/sync-skills.mjs`)** — regenerates mirrors from canonical; a CI check
  asserts mirror equality.
- **Anti-drift/test module (`tests/atelier_config_skill.rs`)** — discoverability, enum-coverage, and
  TOML-load tests against the real config loader + skills module.
- **First-run nudge (atelier startup)** — optional one-line tip when no `atelier-config-setup` skill
  is discovered (PRD F7).
- **Consumed atelier CLI surface** — `--init-config` (ground-truth template), `--print-config` (hard
  validity gate), `--doctor [--json]` (advisory checks).

**Data flow:** author canonical → `sync-skills` → mirrors → discovered by atelier (`/skill:`) or
installed by `npx skills add` into a host agent → agent injects `SKILL.md`, reads `references/` on
demand → wizard collects choices (preset or essentials) → agent writes `atelier.toml` → self-validate
via `--print-config`/`--doctor` when atelier is on PATH, else write-and-instruct.

## Implementation Design

### Core Interfaces

The skill's schema reference is a contract against these existing config types (the drift test
asserts every serde name is documented):

```rust
// src/config/mod.rs — the enums the skill's references/config-schema.md MUST mirror
#[serde(rename_all = "snake_case")] enum RuntimeKind { Codex, Claude, Cursor, Zai, Fake }
#[serde(rename_all = "snake_case")] enum ApprovalMode { Yolo, Normal }
#[serde(rename_all = "snake_case")] enum AgentEffort { Minimal, Low, Medium, High, #[serde(rename="xhigh")] XHigh }
#[serde(rename_all = "snake_case")] enum Capability { Plan, Read, Answer, Challenge, Edit, Command, Verify, Review }
#[serde(rename_all = "snake_case")] enum ToolName { ReadFile, ListFiles, SearchText, RunCommand, ApplyPatch, WriteFile, RecordNote }
```

The skill is parsed through atelier's existing, tolerant manifest type (extra skills.sh/Claude
frontmatter keys are ignored — no `deny_unknown_fields`):

```rust
// src/skills/mod.rs (unchanged) — frontmatter contract
#[derive(Default, Serialize, Deserialize)]
pub struct SkillManifest {
    #[serde(default)] pub name: Option<String>,
    #[serde(default)] pub description: Option<String>,
}
```

Test entry points (new, deterministic — no LLM):

```rust
// tests/atelier_config_skill.rs
fn skill_is_discoverable_with_valid_frontmatter();        // skills module loads `atelier-config-setup`
fn schema_doc_covers_all_enum_variants_and_version();     // every serde name present, no strays, schema_version == 1
fn every_toml_block_loads_under_the_config_loader();      // extract ```toml from SKILL.md + references/, parse each
fn mirrors_equal_canonical_source();                      // .agents/.claude copies == skills/atelier-config-setup/
```

To enumerate variants in the drift test, add `strum::EnumIter` (or a small `all()`) to the four enums
lacking it (`ToolName::all()` already exists).

### Data Models

- **`SKILL.md` frontmatter:** `name: atelier-config-setup`; a "pushy", trigger-rich `description`
  (third person). Optional skills.sh keys (e.g. `license`) are allowed but ignored by atelier.
- **`SKILL.md` body (< 500 lines):** purpose; when-to-use; **wizard protocol** (preset selection →
  essentials: runtime → model(+fallbacks) → `approval_mode` → one starter agent → optional
  progressive disclosure of `presets`/`council`/`limits`/`ui`/`workspace`); **self-validate protocol**
  (PATH check → `atelier --print-config` then `atelier --doctor --json`, fix loop; else
  write-and-instruct); **secret rule** (set `api_key_env` name only, never write keys); **anti-drift
  instruction** (read `atelier --init-config`/`--print-config` as ground truth first); pointers into
  `references/`.
- **`references/config-schema.md`:** every section (`approval_mode`, `[features]`, `[ui]`,
  `[workspace]`, `[runtimes.*]`, `[limits]`, `[council…]`, `[agents.*]`, `[presets.*]`), each field's
  type/default, all 5 enums by serde name, merge order, and file locations
  (`~/.config/.atelier/atelier.toml`, `./atelier.toml`). `schema_version = 1`.
- **`references/presets.md`:** named presets as self-contained ` ```toml ` blocks — **Claude-only**,
  **Codex + Claude fallback**, **Cursor**, **Z.ai HTTP** — each setting `approval_mode`, the needed
  `[runtimes.*]`, and an `[agents.orchestrator]` (+ core agents) with **inline `instructions`** (no
  external file deps) so they load standalone. Model strings are free-form (load regardless); the
  skill tells users to confirm the real model.

### API Endpoints

Not applicable (no network/HTTP surface). The skill's "API" is the **atelier CLI command contract**
it instructs the agent to run — documented under Integration Points.

## Integration Points

| Surface | Purpose | Notes |
|---|---|---|
| `atelier --init-config` / `--print-config` | Ground-truth template + hard validity gate | `--print-config` errors on invalid TOML / unknown fields; the real pass/fail signal |
| `atelier --doctor --json` | Advisory health checks | `DoctorReport{checks: status Ok\|Warn\|Error}`; unavailable runtimes are `Warn`, exit stays 0 — advisory only |
| `npx skills add MatheusBBarni/atelier atelier-config-setup` | Install into host-agent roots (`--global` → personal root) | External `skills` CLI; name-targeted to avoid mirror duplication |
| atelier skills discovery (`src/skills`) | atelier's own runtime + `/skill:` dropdown | Mirror placed in `.agents/skills`; no code change to discovery |

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `skills/atelier-config-setup/` | new | Canonical SKILL.md + references. Low risk (additive content) | Author skill + presets |
| `.agents/skills/…`, `.claude/skills/…` mirrors | new (generated) | Discovery copies. Risk: drift if hand-edited | Generate via sync script; CI equality check |
| `npm/scripts/sync-skills.mjs` | new | Mirror generator + equality check | Add script; wire into CI |
| `src/config/mod.rs` enums | modified | Add `EnumIter`/`all()` to 4 enums for the drift test. Low risk | Derive `strum::EnumIter` (or `all()`) |
| `tests/atelier_config_skill.rs` | new | Discoverability + drift + TOML-load + mirror tests | Implement tests |
| atelier startup | modified | Optional first-run nudge when skill absent. Low risk, read-only | Small, suppressible addition |
| `README.md` | modified | Install section (`npx skills add …`) + manual-copy fallback | Document |
| CI (`.github/workflows/release.yml`) | modified | Run sync-equality check + new tests | Add steps |

## Testing Approach

### Unit Tests
- **Enum coverage:** every serde variant of the 5 enums is documented in `references/config-schema.md`;
  no stray variants; `schema_version == 1`.
- **Frontmatter:** `atelier-config-setup` parses with valid `name`/`description`.
- **Preset shape:** each preset block sets a runtime + an orchestrator agent (smoke).

### Integration Tests
- **TOML-load:** extract every ` ```toml ` block (SKILL.md + references, incl. presets) and assert
  each loads via `load_effective_config`/`RawConfig` — catches unknown keys / bad enums.
- **Discoverability:** the skills-discovery module lists `atelier-config-setup` from a temp root
  (reusing existing test patterns).
- **Mirror equality:** `.agents/skills` + `.claude/skills` copies byte-equal the canonical source.
- **Manual eval checklist** (docs, not CI): run an agent through the wizard for each preset and an
  essentials flow; confirm `atelier --print-config` accepts the output. LLM eval harness deferred
  (env-gated `#[ignore]` if added).

## Development Sequencing

### Build Order
1. **Author canonical skill** — `skills/atelier-config-setup/SKILL.md` + `references/config-schema.md`
   + `references/presets.md`. No dependencies.
2. **Add enum iteration** — derive `strum::EnumIter` / `all()` on `RuntimeKind`/`ApprovalMode`/
   `AgentEffort`/`Capability` in `src/config/mod.rs`. No dependencies.
3. **Sync harness** — `npm/scripts/sync-skills.mjs` generates mirrors + equality check. Depends on **1**.
4. **Rust tests** — discoverability, enum-coverage, TOML-load, mirror-equality. Depends on **1, 2, 3**.
5. **CI wiring** — run sync-equality + the new tests in the pre-commit gate / release workflow.
   Depends on **3, 4**.
6. **First-run nudge** — atelier startup tip when skill absent. Depends on **1** (skill discoverable);
   independent of 2–5.
7. **Docs + publish** — README install section; verify
   `npx skills add MatheusBBarni/atelier atelier-config-setup`. Depends on **1, 3**.

### Technical Dependencies
- External `skills` CLI (skills.sh) for the cross-agent install path (documented fallback: manual copy).
- `strum` crate (or a hand-written `all()`) for enum iteration in the drift test.

## Monitoring and Observability

Largely N/A for a static skill. CI is the operational signal: the drift + TOML-load + mirror tests
gate every change; PRD success metrics (first-attempt validity, schema accuracy) are proxied by these
deterministic tests plus the manual eval checklist. The first-run nudge has no telemetry in V1.

## Technical Considerations

### Key Decisions
- **Markdown-first, no runtime skill code** — the wizard/self-validate behaviors are *instructions*;
  the only Rust touched is enum iteration + tests + an optional nudge. (Rationale: skills are
  prompt-injected, not executed.)
- **Anti-drift via CI test, not codegen** (ADR-005) — cheap, deterministic; codegen deferred to V2.
- **Top-level canonical + generated mirrors** (ADR-004) — single source of truth, enforced.
- **Git-install, no tarball bundle** (ADR-006) — zero packaging changes; sidesteps npm v12.

### Known Risks
- **`npx skills add` layout resolution** lists the skill multiple times (mirrors) — mitigate with
  name-targeted install + README guidance. *(Verify the exact resolution during step 7.)*
- **Enum-iteration change** touches `src/config/mod.rs` — small, localized; covered by existing config
  tests.
- **Model-string volatility** — presets ship plausible models that load (free-form `String`); the
  skill instructs users to confirm the current model. Not asserted by tests.
- **`--doctor` exits 0 today** — self-validate must treat `--print-config` as the gate; adopt
  `--doctor --strict` when the `config-validation-ux` packet ships.

## Architecture Decision Records

- [ADR-001: Portable skill, essentials-first whole-config wizard, consent-based skills.sh delivery](adrs/adr-001.md)
- [ADR-002: Deliver V1 via the skills.sh convention + repo bundle (not a native installer)](adrs/adr-002.md)
- [ADR-003: V1 builds config greenfield; importing existing conventions out of scope](adrs/adr-003.md)
- [ADR-004: Canonical skill in a top-level `skills/` directory with generated mirrors](adrs/adr-004.md)
- [ADR-005: Skill correctness via lightweight Rust tests + an enum/TOML drift guard](adrs/adr-005.md)
- [ADR-006: Distribute via `npx skills add` (git); no npm-tarball bundle; hint via README + first-run nudge](adrs/adr-006.md)
