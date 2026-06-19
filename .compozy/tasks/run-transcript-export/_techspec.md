# TechSpec: Session Transcript Export

> Packet: `run-transcript-export` · Input: `_prd.md` · Decisions: [ADR-001](adrs/adr-001.md)–[ADR-004](adrs/adr-004.md)

## Executive Summary

Session export is a thin, non-interactive command layered on the **existing chat projection**. `atelier --export-session <id>` resolves a session, calls the already-shipped `build_session_preview(root, id)` (open → `read_events` → `ChatProjection::rebuild` → sanitize), renders the resulting `Vec<ChatItemView>` to a lean Markdown transcript via a new pure serializer, runs a tiered secret scan, applies a risk-adaptive review gate, writes a `0600` quarantined file, and appends a `session_exported` audit event. The only genuinely net-new logic is the serializer, a findings-returning secret scanner, and the orchestrator; the risky read/projection path is reused verbatim.

**Primary technical trade-off:** the design *detects but does not mutate* the change narrative and gates on a **precision-first** scanner with a human acknowledgment, rather than guaranteeing redaction automatically. This trades an (unachievable) airtight automated guarantee for a maintainable, low-false-positive control that fails closed in CI — accepted because automated redaction provably leaks. A secondary trade-off: reusing the projection couples the export format to `ChatItemKind`, so the serializer carries a match arm that must track new item kinds.

## System Architecture

### Component Overview

| Component | New/Mod | Responsibility |
|---|---|---|
| `src/export.rs` | **new** | Orchestrator: `export_session(cfg, opts)` — resolve session → preview → render → scan → gate → write → audit. Owns `ExportOptions`/`ExportOutcome`. |
| `src/app/chat/markdown.rs` | **new** | Pure serializer: `render_session_markdown(preview, scan) → String`. Summary-first, `<details>`-collapsed verbose spans, flagged spans surfaced. |
| `src/runtime/status.rs` | mod | Add `scan_secrets(text) → Vec<SecretFinding>` (single-source secret logic), refactoring `looks_like_secret`/`key_is_sensitive` to feed it. |
| `src/cli.rs` | mod | New flags, relaxed guards, dispatch arm, `confirm_export` helper. |
| `src/history/mod.rs` | mod | `SESSION_EXPORTED_KIND` const; promote `write_private_file`/`set_private_file_permissions` to `pub(crate)`. |
| `src/app/chat/mod.rs`, `src/lib.rs` | mod | `pub mod markdown;` / `pub mod export;`. |

**Data flow:** `cli.rs` (parse/validate) → `export::export_session` → `chat::build_session_preview` (read) → `chat::markdown::render_session_markdown` + `status::scan_secrets` → gate (stdin/stderr or `--yes`) → `history::write_private_file` + `HistoryStore::append_event`. External interaction: `git check-ignore`/`rev-parse` subprocesses for the egress warning.

**PRD → component mapping:** CF1→`export.rs`+`cli.rs`; CF2→`status::scan_secrets`; CF3/CF7→gate in `export.rs`; CF4→fail-closed `bail!` path; CF5→`markdown.rs`; CF6→`write_private_file`+`session_exported`+`git check-ignore`.

## Implementation Design

### Core Interfaces

```rust
// src/export.rs — orchestrator surface
pub enum SessionSelector { Explicit(String), Latest }

pub struct ExportOptions {
    pub session: SessionSelector,
    pub out: Option<PathBuf>,      // None → .atelier/exports/<id>-<ts>.md ; Some("-") → stdout
    pub run: Option<String>,       // narrow to one run_id
    pub assume_yes: bool,          // --yes (non-interactive)
    pub allow_flagged: bool,       // override fail-closed
}

pub struct ExportOutcome {
    pub path: Option<PathBuf>,     // None when emitted to stdout
    pub item_count: usize,
    pub deterministic: usize,      // gating findings
    pub advisory: usize,           // entropy warnings
    pub overridden: bool,
}

pub fn export_session(cfg: &EffectiveConfig, opts: &ExportOptions) -> Result<ExportOutcome>;
```

```rust
// src/runtime/status.rs — single-source secret scan
pub enum Confidence { Deterministic, Advisory }
pub enum SecretCategory { ProviderToken, SensitiveKeyValue, CredentialFile, HighEntropy }

pub struct SecretFinding {
    pub category: SecretCategory,
    pub confidence: Confidence,
    pub start: usize,              // byte span into the scanned text
    pub end: usize,
}

pub fn scan_secrets(text: &str) -> Vec<SecretFinding>;

// src/app/chat/markdown.rs — pure serializer
pub fn render_session_markdown(preview: &SessionPreview, scan: &[SecretFinding]) -> String;
```

**Error handling:** all fallible paths return `anyhow::Result`; fail-closed and validation use `bail!` (→ `main.rs` exit 1). No new exit code (ADR-004).

### Data Models

- **`SecretFinding`** (above) is the scan's output; the gate counts `Deterministic` vs `Advisory` and the serializer uses spans to redact-by-label and to mark flagged spans.
- **`session_exported` event payload** (appended via `HistoryStore::open(root, id).append_event`):
  ```json
  { "scope": "session|run:<id>", "out_path": "<path|stdout>", "item_count": 42,
    "flagged": { "deterministic": 1, "advisory": 3 },
    "redacted_categories": ["provider_token"],
    "override": { "flag": "allow_flagged|interactive", "category": "intentional" } }
  ```
  `schema_version` stays `1` (additive); the projection routes the kind to `_ => {}` (audit-only, no chat render).
- **Markdown artifact structure:** `# Session <id>` → TL;DR (files changed, commands, outcome) → **Redaction summary** (counts by category) → per-item sections; verbose `Read`/`CommandResult` bodies wrapped in `<details><summary>…</summary>`; flagged spans rendered expanded with `⚠ **redacted**: <category>` and the `<redacted>` label. Footer: *"Best-effort redaction — review before sharing; rotate any credential the agent touched."*

### CLI Surface

*(repurposes the template's "API Endpoints" — atelier exposes a CLI, not HTTP.)*

```
atelier --export-session <ID|latest> [--run <RUN_ID>] [--out <PATH|->] [--yes] [--allow-flagged]
```

- **Validation:** `--export-session` admitted into the `--yes` guard and the `--update` mega-exclusion; `--allow-flagged` requires `--export-session`; `--run` requires `--export-session`.
- **Exit / streams:** `0` success; `1` on error or fail-closed. Interactive prompts and all warnings/nudges → **stderr**; the written path (or, with `--out -`, the transcript) → **stdout**. Fail-closed nudge: *"N secret(s) flagged; re-run with --allow-flagged to override."*
- **Gate:** interactive clean → `y`/`yes`; interactive flagged → type `approve`; `--yes` clean → write; `--yes` flagged → fail closed unless `--allow-flagged`.

## Integration Points

- **Git subprocess** (egress warning only): `git check-ignore <target>` and repo-membership via `git rev-parse --show-toplevel`, copying the `src/app/git.rs` pattern (500 ms timeout, `kill_on_drop`, `tokio::select!`). Failure or no-repo → treat target as **not ignored** and warn; never blocks the export. No auth; no retry (best-effort advisory).

## Impact Analysis

| Component | Impact | Description & Risk | Required Action |
|---|---|---|---|
| `src/export.rs` | new | Orchestrator + option/outcome types. Low risk (additive). | Create module; `pub mod export` in `lib.rs`. |
| `src/app/chat/markdown.rs` | new | Pure serializer. Low risk. | Create; `pub mod markdown` in `chat/mod.rs`. |
| `src/runtime/status.rs` | modified | Add `scan_secrets` + types; refactor `looks_like_secret`. Medium — must not regress existing redaction tests. | Refactor behind existing tests; add recall tests. |
| `src/cli.rs` | modified | Flags, relaxed `--yes`/`--update` guards, dispatch, `confirm_export`. Medium — **every `Cli` struct-literal in tests must add fields**. | Update flags + all struct-literal test sites. |
| `src/history/mod.rs` | modified | New kind const; promote 0600 writer to `pub(crate)`. Low. | Add const; change two visibilities. |
| `src/main.rs` | unchanged | Exit-1 mapping already suffices for fail-closed. | None. |
| `tests/export_session.rs` | new | E2E coverage. | Create suite. |

## Testing Approach

### Unit Tests
- **`scan_secrets` recall (critical):** a seeded corpus of `(input, expected_findings)` next to the existing `status.rs` redaction tests — assert ≥99% recall on the deterministic class (AWS/Stripe/Google/JWT/Slack/GitLab/PEM/provider prefixes, sensitive `key=value`, credential-file bodies) and that entropy hits (git SHAs, base64, UUIDs) are `Advisory`, never gating.
- **`render_session_markdown`:** asserts (plain `assert!`/`assert_eq!`, no `insta`) — TL;DR + redaction summary present; verbose bodies wrapped in `<details>`; flagged spans expanded and labelled; an unknown `ChatItemKind` renders via the generic fallback, not dropped.
- **`export_session` logic:** build a temp session with `HistoryStore::create` + `append_event` (the `chat/mod.rs` fixture pattern); assert the gate decision function returns write/fail-closed correctly per tier and `--yes`/`--allow-flagged`.

### Integration Tests (`tests/export_session.rs`, `assert_cmd` + `tempfile`)
- Seed a session, then drive the **real binary**: clean export writes the file (perms `0600`), contains the prompt, contains **no** seeded secret, and appends a `session_exported` event.
- **Interactive gate** via `.write_stdin("approve\n")` for a flagged session; `.write_stdin("n\n")` cancels with no file.
- **`--yes` fail-closed:** `.failure()` + stderr nudge + no file; **`--allow-flagged`:** `.success()` + override recorded.
- **Egress warning:** `--out` to a non-ignored path emits the stderr warning.

## Development Sequencing

### Build Order
1. **History primitives** — `SESSION_EXPORTED_KIND` const; promote `write_private_file`/`set_private_file_permissions` to `pub(crate)`. *No dependencies.*
2. **Secret scanner** — `SecretFinding`/`Confidence`/`SecretCategory` + `scan_secrets` in `runtime::status`, refactoring `looks_like_secret`; unit recall tests. *No dependencies.*
3. **Markdown serializer** — `app/chat/markdown.rs` + `pub mod markdown`. *Depends on 2* (uses `SecretFinding` spans to mark flagged content); consumes the existing `SessionPreview`.
4. **Orchestrator** — `src/export.rs` (`ExportOptions`/`ExportOutcome`/`export_session`) + `pub mod export`. *Depends on 1* (writer + audit event), *2* (scan), *3* (render); reuses `build_session_preview`.
5. **Egress warning** — `git check-ignore`/`rev-parse` helper. *Depends on 4* (called from it; otherwise independent).
6. **CLI wiring** — flags, relaxed guards, dispatch arm, `confirm_export`, struct-literal test updates. *Depends on 4*.
7. **E2E suite** — `tests/export_session.rs`. *Depends on 6*.

### Technical Dependencies
None external. All work is in-crate; no new third-party dependency (uses existing `assert_cmd`/`predicates`/`tempfile` and the std/`tokio` subprocess primitives already in `app/git.rs`).

## Monitoring and Observability

- **Primary signal:** the `session_exported` event (fields above) is the local audit + analytics record. Derivable metrics: export adoption (events ÷ sessions), human-review rate (interactive vs `--yes` vs `--allow-flagged`), flagged counts and override categories.
- **No telemetry backend** exists in atelier; metrics are read from the local log. The PR-attach signal is **not** locally observable (Open Question in the PRD).
- No alerting infrastructure; `--allow-flagged` use is recorded and printed loudly to stderr.

## Technical Considerations

### Key Decisions
- **Reuse the projection, don't re-read** (ADR-003): export consumes `build_session_preview`; zero new read path. Trade-off: serializer tracks `ChatItemKind`.
- **Dedicated `src/export.rs`** (ADR-003): cohesion over reusing `history`'s private writer; cost is one `pub(crate)` promotion.
- **Single-source tiered scanner** (ADR-004): extend `runtime::status` rather than add a third secret definition; deterministic gates, entropy advises.
- **Fail-closed via `bail!`, no new exit code** (ADR-004): matches the `--doctor --strict` model; `main.rs` unchanged.
- **`<details>` collapse + `y`/`approve` gate vocab:** GitHub-native readability; reuse of `parse_approval_resolution` semantics.

### Known Risks
- **False-negative leak** (irreducible) → human gate + advisory entropy tier + seeded-corpus recall test + "rotate credentials" notice + `0600` quarantine.
- **`Cli` struct-literal test churn** → mechanical but must update every site or the crate won't compile (caught at build).
- **`<details>` degradation in plain-text viewers** (visible tags) → acceptable; the PR target (GitHub) renders it; summaries remain readable.
- **`git check-ignore` absence / slowness** → 500 ms timeout, fail-open to a warning, never blocks.
- **Interactive-gate coverage** → driven in E2E via `write_stdin`; gate decision also unit-tested as a pure function.

## Architecture Decision Records

- [ADR-001: V1 scope — CLI-first redacted session export with tiered, detect-don't-mutate redaction](adrs/adr-001.md) — scope & redaction-security architecture.
- [ADR-002: V1 product experience — CLI-first export, lean review artifact, risk-adaptive review gate](adrs/adr-002.md) — surface, artifact shape, gate, override handling.
- [ADR-003: Component architecture — dedicated export module over the existing projection](adrs/adr-003.md) — `src/export.rs` orchestrator, `markdown.rs` serializer, reuse of `build_session_preview`.
- [ADR-004: Tiered secret scan (single-source) with fail-closed CLI enforcement](adrs/adr-004.md) — extend `runtime::status`; deterministic-gates/entropy-advises; `bail!` fail-closed; `git check-ignore` egress warning.
