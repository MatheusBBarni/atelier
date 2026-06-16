# Technical Specification: Config Validation UX with Scriptable Doctor Exit Codes

Status: Draft
Date: 2026-06-15
Source PRD: `.compozy/tasks/config-validation-ux/_prd.md`

## Executive Summary

This feature wires existing, unused machinery into three small, additive behaviors across four `atelier` modules. The doctor's already-defined `DoctorReport::has_errors()` becomes the source of a process exit code under a new opt-in `--doctor --strict` flag; `run_doctor` elevates an **unavailable orchestrator runtime** from `Warn` to `Error` (the only runtime guaranteed to run on every prompt); and the config loader appends a "did you mean `codex`?" hint to three existing "missing field / undefined runtime" errors, reusing a Levenshtein helper relocated to a new `src/util.rs` leaf.

The primary technical trade-off: **elevation is scoped to the orchestrator runtime only (ADR-003), trading breadth of coverage for a zero-false-failure guarantee** — every `Error` it raises is a runtime whose absence makes *every* run fail, so CI never learns to ignore the gate. Elevation is decoupled from `--strict` (the report always reflects true severity; the flag only gates the exit), keeping the exit code a pure function of report severity. V1 emits exit `0`/non-zero only; codes `1`/`2` are reserved and documented per ADR-001. No new packages, no persistent data, no external services.

## System Architecture

### Component Overview

| Component | File | Responsibility |
|---|---|---|
| **Util leaf** (new) | `src/util.rs` | `edit_distance` (moved from skills) + `suggest_nearby_name` threshold helper |
| **Config loader** (modified) | `src/config/mod.rs` | `EffectiveConfig::required_runtime_ids()`; append near-miss hints at 3 load-error sites |
| **Doctor** (modified) | `src/doctor/mod.rs` | Elevate unavailable orchestrator runtime to `Error`; add `error_count()` |
| **CLI** (modified) | `src/cli.rs` | `--strict` flag + validation; gate exit on `has_errors()`; stderr discovery nudge |
| **Skills** (modified) | `src/skills/mod.rs` | Drop private `edit_distance`; import from `util` |
| **Crate root** (modified) | `src/lib.rs` | `pub mod util;` |
| **Docs / CI** (modified) | `README.md`, `.github/workflows/release.yml` | Flag docs + exit-code Notes + CI snippet; dogfood `--doctor --strict` |

**Data flow:** `load_effective_config` → `EffectiveConfig` (now answers `required_runtime_ids()`) → `run_doctor(&config)` reads that set and elevates the orchestrator runtime in its availability loop → `DoctorReport` → `cli.doctor` dispatch renders the report (stdout), then branches: `--strict` + `has_errors()` → return `Err` (→ `main.rs` exit 1); else if errors → stderr nudge, exit 0. Config typo hints are produced earlier, during `into_effective`, before any doctor run.

## Implementation Design

### Core Interfaces

In-use derivation on the already-loaded config (orchestrator id is the well-known `"orchestrator"`):

```rust
impl EffectiveConfig {
    /// Runtime ids whose unavailability is a hard error: they are guaranteed
    /// to run on every prompt-driven run. V1 = the orchestrator's runtime.
    pub fn required_runtime_ids(&self) -> BTreeSet<&str> {
        self.agents
            .get("orchestrator")
            .map(|a| a.runtime.as_str())
            .into_iter()
            .collect()
    }
}
```

Shared leaf helper (`src/util.rs`), reused by config hints and skills:

```rust
pub fn edit_distance(left: &str, right: &str) -> usize { /* moved verbatim from skills */ }

/// Closest known name within an edit-distance threshold, else None.
pub fn suggest_nearby_name<'a>(unknown: &str, known: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let max = 2.max(unknown.len() / 3);
    known.into_iter()
        .map(|k| (edit_distance(unknown, k), k))
        .filter(|(d, _)| *d <= max)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| k)
}
```

Doctor elevation, inside the existing availability loop (doctor/mod.rs:65-99):

```rust
let required = config.required_runtime_ids();
let (status, severity) = match availability.status {
    RuntimeAvailabilityStatus::Available => (DoctorStatus::Ok, DoctorSeverity::Info),
    RuntimeAvailabilityStatus::Unavailable if required.contains(runtime.id.as_str()) =>
        (DoctorStatus::Error, DoctorSeverity::Error),
    RuntimeAvailabilityStatus::Unavailable | RuntimeAvailabilityStatus::Unknown =>
        (DoctorStatus::Warn, DoctorSeverity::Warning),
};
```

CLI gate + nudge, in the `cli.doctor` dispatch (cli.rs:156-164):

```rust
if cli.doctor {
    let report = run_doctor(&config).await;
    if cli.json { println!("{}", render_json(&report)?); } else { print!("{}", render_human(&report)); }
    if report.has_errors() {
        if cli.strict { bail!("doctor reported {} error(s)", report.error_count()); }
        eprintln!("{} error(s) found; re-run with --doctor --strict to fail CI", report.error_count());
    }
    return Ok(());
}
```

### Data Models

No persistent or wire models change. Touched in-memory types:

- `Cli` gains `#[arg(long)] pub strict: bool` (mirrors `doctor`/`json`).
- `DoctorReport` gains `error_count(&self) -> usize` (companion to `has_errors()`); `DoctorCheck.status`/`severity` are set to `Error`/`Error` for an unavailable required runtime (existing enums, no new variants).
- `EffectiveConfig::required_runtime_ids() -> BTreeSet<&str>` (new method, no field changes).
- The three config errors are enriched in place via `suggest_nearby_name`; their `anyhow` types are unchanged.

### API Endpoints

CLI surface and exit-code contract (no HTTP API):

| Invocation | Behavior | Exit |
|---|---|---|
| `atelier --doctor` | Render report; if errors, stderr nudge | `0` (unchanged) |
| `atelier --doctor --strict` | Render report; non-zero if any `Error` | `0` healthy / non-zero on error |
| `atelier --doctor --strict --json` | JSON to stdout (clean), error annotation to stderr | as above |
| `atelier --strict` (no `--doctor`) | `bail!("--strict is only valid with --doctor")` | non-zero |

Contract documented in README: **branch on `!= 0`, not on a specific code.** Codes `1` (found problems) and `2` (invalid config) are reserved; V1 returns `0` and a generic non-zero via `main.rs`'s `exit(1)`.

## Integration Points

- **GitHub Actions release pipeline** (`.github/workflows/release.yml`): add a step running `atelier --doctor --strict` as a health gate (dogfood). No credentials required for the orchestrator runtime check beyond CLI presence.
- **README CLI documentation**: `--strict` added to the flag list and "Notes" block; a copy-paste CI example (shell/GitHub Actions) demonstrates the gate.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `src/util.rs` | new | Leaf module; ~25 lines. Low risk | Create; add `pub mod util;` to `lib.rs` |
| `src/skills/mod.rs` | modified | Remove private `edit_distance`, import from util. Low risk | Repoint call sites |
| `src/config/mod.rs` | modified | `required_runtime_ids()`; hints at 3 error sites. Low risk; hints are additive | Add method + 3 enrichments |
| `src/doctor/mod.rs` | modified | Elevation + `error_count()`. Med risk: default render severity changes for orchestrator runtime | Update affected unit-test assertions |
| `src/cli.rs` | modified | `--strict` flag, validation bail, gate, nudge. Low risk | Add flag + dispatch logic |
| `src/main.rs` | unchanged | Existing `Err`→`exit(1)` already provides non-zero | None (reserved `2` is V2) |
| `tests/cli.rs` | modified | New exit-code integration tests | Add cases |
| `README.md`, `release.yml` | modified | Docs + dogfood gate | Edit |

## Testing Approach

### Unit Tests

- **`util`**: `edit_distance` (port any existing skills test); `suggest_nearby_name` returns the closest name for a 1-edit typo and `None` for a wild typo (threshold boundary).
- **`config`**: a typo'd `[runtimes.codx]` (missing `type`) error contains "did you mean `codex`?"; an agent with `runtime = "codx"` error suggests `codex`; a `[agents.fixr]` error suggests `fixer`. **False-positive lock:** a legitimately unconventional custom runtime/agent name that loads successfully produces no hint and no new error/warning.
- **`doctor`**: with the orchestrator runtime `Unavailable`, its check is `Error`; a non-orchestrator runtime `Unavailable` stays `Warn`; the orchestrator runtime `Unknown` stays `Warn`. **Guardrail:** `required_runtime_ids()` on the default config equals exactly the orchestrator's runtime id.

### Integration Tests (`tests/cli.rs`, `assert_cmd`)

- `--doctor --strict` on a healthy temp config → `.success()`.
- `--doctor --strict` on a config whose orchestrator runtime is unavailable → `.failure()`.
- plain `--doctor` with errors → `.success()` and stderr contains `--strict`.
- `--strict` without `--doctor` → `.failure()` with the bail message.
- `--doctor --strict --json` on errors → stdout parses as JSON (clean) and exit is non-zero.

## Development Sequencing

### Build Order

1. **Create `src/util.rs`** with `edit_distance` + `suggest_nearby_name`; add `pub mod util;` to `lib.rs`; repoint `skills` to it. — no dependencies.
2. **Config near-miss hints** at the three load-error sites using `util` (step 1). — depends on 1.
3. **`EffectiveConfig::required_runtime_ids()`** + guardrail test. — no dependencies (parallel to 1-2).
4. **Doctor elevation** + `error_count()`, using `required_runtime_ids()`. — depends on 3.
5. **CLI `--strict`**: flag, `--strict requires --doctor` bail, dispatch gate, stderr nudge. — depends on 4 (needs `has_errors()` to reflect elevation).
6. **Tests** (unit + integration) across steps 1-5. — depends on 1-5.
7. **Docs & dogfood**: README CLI Notes + CI snippet; `--doctor --strict` gate in `release.yml`. — depends on 5.

### Technical Dependencies

None external. All changes are in-crate; no new third-party dependencies, services, or infrastructure.

## Monitoring and Observability

This is a CLI; the observable surface is the exit code and the report itself.

- **Operational signal:** the dogfood `--doctor --strict` step in `release.yml` — a red job means atelier's own config/runtime health regressed.
- **Machine output:** `--json` keeps its `schema_version` and structure; the only change is that an unavailable orchestrator runtime now serializes `status: "error"` / `severity: "error"`. Consumers parsing JSON see the elevated severity; consumers gating on exit code use `!= 0`.
- No new logs, metrics, or alerting infrastructure.

## Technical Considerations

### Key Decisions

- **Orchestrator-only elevation, decoupled from `--strict`** (ADR-003). *Rationale:* only the orchestrator is unconditional, so its runtime is the sole "absence ⇒ certain failure" signal; decoupling keeps exit a pure function of report severity. *Trade-off:* a missing conditionally-routed runtime stays `Warn` (coverage deferred to V2). *Rejected:* all-enabled-agents elevation (false failures), strict-gated severity (flag-dependent report).
- **Shared `util::edit_distance` + additive hints at 3 sites** (ADR-004). *Rationale:* avoid `config`→`skills` layering inversion; reuse one helper. *Trade-off:* one new module file. *Rejected:* `pub`-in-skills, duplication.
- **Only `Unavailable` elevates; `Unknown` stays `Warn`.** *Rationale:* `Unknown` is an inconclusive probe; elevating it would cause false failures.
- **Reserved exit codes, emit `0`/non-zero only** (ADR-001). *Rationale:* emitting a distinct `2` needs a `main.rs` global error-path rework with zero consumers today. *Trade-off:* documentation must not claim a `1`/`2` split V1 doesn't make.

### Known Risks

- **Default doctor render severity changes** for an unavailable orchestrator runtime (Warn→Error). *Likelihood:* certain in affected fixtures. *Mitigation:* update doctor unit-test assertions; exit code (the compat surface) is unchanged.
- **Elevation keys on the literal id `"orchestrator"`.** *Mitigation:* it is the same well-known id the orchestrator step resolves; the guardrail test pins `required_runtime_ids()`.
- **Suggestion fires on an unintended near-miss.** *Mitigation:* threshold `2.max(len/3)`; only the single closest within-threshold candidate is suggested; wild typos stay silent.
- **Hint perceived as full typo detection.** *Mitigation:* additive-only (augments errors that already fire); load-clean phantom is a documented V2 doctor-warning.

## Architecture Decision Records

- [ADR-001: V1 scope and exit-code contract for scriptable config validation](adrs/adr-001.md) — opt-in `--strict`, reserved `0/1/2` contract (emit `0/1`), unconditional-chain elevation, additive typo hint, discovery nudge.
- [ADR-002: Atomic V1 delivery for config validation UX](adrs/adr-002.md) — ship all five capabilities + docs as one release; friendly "did you mean?" phrasing.
- [ADR-003: Orchestrator-only runtime elevation, decoupled from `--strict`](adrs/adr-003.md) — in-use set = `{orchestrator.runtime}`; only `Unavailable` elevates; exit = function of report severity.
- [ADR-004: Shared `util::edit_distance` and additive near-miss config hints](adrs/adr-004.md) — new `src/util.rs` leaf; hints at three config-load sites against sibling keys.
