# Config Validation UX with Scriptable Doctor Exit Codes

## Overview

`atelier --doctor` is a non-interactive health check, yet it always exits 0 — so CI pipelines and scripts cannot gate on it, defeating the entire purpose of a machine-facing doctor. Separately, a single-character typo in a config table name (`[runtimes.codx]`) silently creates a phantom entry and only fails later with an opaque "missing required field," with no hint toward the intended name.

This feature makes `atelier` **scriptable and CI-gateable** for its primary consumer — platform/CI engineers — via an opt-in `--doctor --strict` flag that returns a non-zero exit code when health checks report `Error`, backed by a documented exit-code contract. It also sharpens the most common config error with an additive "did you mean?" hint for developers hand-editing TOML. The V1 is deliberately a **Quick Win**: the report machinery (`DoctorReport::has_errors()`) and the edit-distance helper already exist; this wires them together without new subsystems.

## Problem

`atelier` is increasingly run in automation — its own release pipeline and adopters' CI both invoke it non-interactively. But the two checks an operator most wants to gate on are unreliable signals today:

1. **The doctor cannot fail.** `run_doctor` produces a structured report with per-check `Error`/`Warn`/`Ok` statuses, and `DoctorReport::has_errors()` is defined — but the CLI dispatch (cli.rs:156-164) renders the report and unconditionally `return Ok(())`. A pipeline running `atelier --doctor` to validate a runner gets exit 0 even when the config is broken. Worse, an unavailable runtime CLI (e.g. `codex` not installed) maps only to `Warn` (doctor/mod.rs:69-70), so even if the doctor *could* fail, a missing runtime the agents depend on would never trip it. CI is left parsing `--doctor --json` and grepping statuses by hand — brittle and undocumented.

2. **Config typos fail late and opaquely.** Config merges layers with `or_insert` (config/mod.rs:1170, 1232), so a typo'd `[runtimes.codx]` does not override the intended `codex` — it creates a *new* phantom runtime. The user then sees "runtime codx is missing required field type" with no indication they meant `codex`. A newcomer can burn fifteen minutes on a one-character slip.

Both gaps are concrete, documented operability smells in `docs/feature-roadmap.md`. Neither requires new architecture to fix — the value is in closing them precisely, without surprising existing wrappers or introducing false signals.

### Market Data

- **The ecosystem disagrees on doctor exit codes**, which validates an opt-in approach: `flutter doctor` and `npm doctor` exit 0 even on errors (useless for CI — filed as bugs); `brew doctor` exits non-zero on *everything* including cosmetic warnings (so noisy that users disable it). The proven middle is an **opt-in strict flag** — `helm lint --strict`, `eslint --max-warnings 0`.
- **Graduated exit codes are the validator norm**: ESLint and Terraform use `0`=ok, `1`=ran-but-found-problems, `2`=couldn't-run/config-broken — a "did the check run?" vs "are there findings?" split.
- **serde `deny_unknown_fields` ships no near-miss suggestion**, so a Levenshtein "did you mean?" hint puts `atelier` ahead of the serde baseline and matches the Cobra/git UX (edit-distance ≤ 2) operators already expect.
- **clig.dev**: keep stdout clean for scripts (send hints to **stderr**); suggest, don't auto-apply. Exit codes are a compatibility surface — never silently flip a default 0→non-zero.

## Core Features

| #  | Feature | Priority | Description |
| -- | ------- | -------- | ----------- |
| F1 | Scriptable `--doctor --strict` gate | Critical | Opt-in flag. Default `--doctor` keeps exiting 0 (non-breaking). Under `--strict`, `run_cli` returns `Err` (→ `main.rs` exit 1) when the report has any `Error`. Exit code is a pure function of the report's max severity. Validates `--strict` requires `--doctor` (mirrors the existing `--json` bail). |
| F2 | Reserved exit-code contract | Critical | Document `0`=healthy, `1`=ran & found problems, `2`=could-not-evaluate/config-invalid. Publish the one stable guarantee: **branch on `!= 0`, not `== 1`** (1/2 reserved, not-yet-branchable). V1 emits only 0/1; no `main.rs` rework. Docs must not claim V1 emits `2`. |
| F3 | Unconditional-chain runtime elevation | High | A runtime elevates `Warn → Error` under `--strict` **iff** it is the primary (non-fallback) runtime of an agent the default orchestrator chain resolves *unconditionally* on every run. Council-member, inactive-preset-only, and model-fallback runtimes stay `Warn`. Exposed as an owned `EffectiveConfig::referenced_runtime_ids()` (reusing the orchestrator's resolver), guarded by a new-reference-site test. |
| F4 | Additive config typo hint | High | When config load already fails with "missing required field" on a near-miss table name, append "did you mean `<name>`?" using `edit_distance` **relocated to a shared text/util module**. Purely additive — valid custom-named entries gain no new warning or failure (proven by regression test). |
| F5 | Default-doctor discovery nudge | Medium | When plain `--doctor` finds errors, print a one-line **stderr** nudge ("N error(s) found; re-run with `--strict` to fail CI"). The cheapest counter to "an opt-in flag nobody flips"; keeps stdout/JSON clean. |

## KPIs

| KPI | Target | How to Measure |
| --- | ------ | -------------- |
| Strict exit correctness | 100% of `Error` reports → non-zero under `--strict`; 0% under default | `assert_cmd` integration tests over healthy/unhealthy fixtures |
| Backward-compat preservation | 0 change to default `--doctor` exit (stays 0) across all fixtures | Snapshot/integration diff empty for default doctor |
| Typo time-to-diagnosis | Near-miss hint present in 100% of single-edit typo'd table-name load failures, surfaced in ≤1 error message | Unit tests; error string contains the suggestion |
| Hint false-positive rate | 0 new hints/failures for valid custom-named entries that load successfully | Unit test: legitimately unconventional names load clean |
| In-use elevation accuracy | 100% of unconditional-chain unavailable runtimes → Error; 0% for council/preset/fallback runtimes | Unit tests over mixed referenced/idle/conditional configs |
| Dogfood adoption | `--doctor --strict` gate present in atelier's own release CI within 1 release | Presence in `.github/workflows/release.yml` |

## Feature Assessment

| Criteria | Question | Score |
| -------- | -------- | ----- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Maybe |
| **Differentiation** | Does this set us apart or just match competitors? | Strong |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: **Quick Win** — small, well-scoped effort on existing machinery, with disproportionate operability value for the CI/CD consumer.

## Council Insights

- **Recommended approach:** Ship the ~3-line gate, narrow unconditional-chain elevation, the additive hint, and a discovery nudge; reserve-and-document the `0/1/2` exit contract while emitting only `0/1`. Move `edit_distance` to a shared util (not `pub`-in-skills — that inverts layering). Both engineers and the product/skeptic advisors converged on this.
- **Key trade-offs:** Reserve the exit-code granularity now (cheap, protects the frozen API) vs. emit graduated codes now (a global `main.rs` refactor for a code with zero consumers — deferred). Trustworthy-but-narrow elevation (false-failures closed by construction) vs. broad elevation (false CI failures that train adopters to disable the flag).
- **Risks identified:** (1) `--strict` is opt-in → low discovery — mitigated by the stderr nudge + dogfooding. (2) The "unconditional" set could silently widen — mitigated by a new-reference-site guardrail test. (3) Additive-only typos can't catch the load-clean phantom (`[agents.fixr]` silently ignored) → **framing risk** of false confidence — mitigated by honest user-facing wording and a committed V2 task.
- **Stretch goal (V2+):** A unified `validate(config) → Verdict` concept routing doctor + config-load + typo + phantom detection through one severity model (static document-validity vs dynamic environment-readiness as first-class); **JSON Schema export** for the TOML to shift validation left into editors/pre-commit; a **doctor-surfaced phantom-entry Warning** for typos that load clean; and emitting exit `2` (with the `main.rs` structured-error rework) before `--strict` is advertised as the supported CI integration.

## Summary / Differentiator

Most "doctor" tools get the exit code wrong in one of two directions (always 0, or non-zero on everything). `atelier` threads the validated middle — non-breaking by default, scriptable on opt-in — with a *trustworthy* strict signal where every failure is a certain (not probable) broken run. The typo hint exploits a real gap: serde's `deny_unknown_fields` offers no suggestion, so near-miss diagnostics are an immediate, low-cost edge.

## Integration with Existing Features

| Integration Point | How |
| ----------------- | --- |
| `src/doctor/mod.rs` | Consult `has_errors()`; elevate unconditional-chain unavailable runtimes to `Error`; severity → exit code |
| `src/cli.rs` (clap) | Add `--strict` flag + `--strict requires --doctor` validation; gate the doctor dispatch |
| `src/config/mod.rs` | Append near-miss hint at the "missing required field" sites; add `referenced_runtime_ids()` |
| `src/skills/mod.rs` | Relocate `edit_distance` to a shared util both modules depend on |
| `.github/workflows/release.yml` | Dogfood `--doctor --strict` as a health gate |

## Out of Scope (V1)

- **`--check-config` mode** — redundant with `--print-config` (validates + redacted-prints + exits non-zero on parse errors) plus `--doctor --strict`. A dedicated mode would mostly duplicate them.
- **Emitting exit code `2` / `main.rs` structured-error rework** — the contract is reserved and documented, but emitting a distinct `2` requires reworking the global blanket `exit(1)`; deferred until `--strict` is advertised as the supported CI integration (YAGNI: no consumer reads `2` today).
- **Phantom-entry detection for typos that load clean** (`[agents.fixr]` silently ignored) — the dangerous silent case belongs in a V2 doctor-surfaced **stderr Warning**, not a V1 load-time failure, to avoid the false-positive surface on valid custom names.
- **Model-fallback & cross-preset runtime coverage** — fallback runtimes (reached only at provider failure) and runtimes introduced by a non-selected preset stay `Warn`; `--strict` is honestly scoped to the unconditional effective chain.
- **JSON Schema export for the TOML** — a more ambitious shift-left path on a different distribution surface; named as the V2 successor.
- **Warning or hard-failing on valid near-miss custom names** — a behavior change with false-positive risk; the hint is strictly additive to errors that already fire.

## Architecture Decision Records

- [ADR-001: V1 scope and exit-code contract for scriptable config validation](adrs/adr-001.md) — opt-in `--strict`, reserved `0/1/2` contract (emit `0/1`), unconditional-chain runtime elevation, additive typo hint via shared `edit_distance`, discovery nudge.

## Open Questions

- Does `referenced_runtime_ids()` thread invocation context (selected preset, council on/off) as input, or compute against a fixed assumed activation state? (Determines whether cross-preset validation is sound — a techspec decision.)
- Hint phrasing: the friendly "did you mean `X`?" (git/cargo/kubectl convention) vs. rustc-dev-guide's "a table with a similar name exists: `X`" — pick one and apply consistently.
- Edit-distance threshold for config: reuse skills' `2.max(len/3)` or adopt a fixed `≤ 2` (Cobra default)?
- Should built-in runtimes (`fake`, etc.) be explicitly asserted resolvable so they can never raise a missing-runtime `Error`?
- Exact nudge wording, and confirmation it stays out of `--json` output (stderr only).
