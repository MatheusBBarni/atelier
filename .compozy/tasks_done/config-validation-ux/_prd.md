# Product Requirements: Config Validation UX with Scriptable Doctor Exit Codes

Status: Draft
Date: 2026-06-15
Source Idea: `.compozy/tasks/config-validation-ux/_idea.md`

## Overview

`atelier --doctor` is a non-interactive health check, yet it always exits 0 — so CI pipelines and scripts cannot gate on it, which defeats the purpose of a machine-facing doctor. Separately, a one-character typo in a config table name (`[runtimes.codx]`) silently creates a phantom entry and fails later with an opaque "missing required field," giving no hint toward the intended name.

This feature makes `atelier` **scriptable and CI-gateable** for its primary user — the platform/CI engineer — through an opt-in `atelier --doctor --strict` flag that returns a non-zero exit code when health checks report an error, backed by a documented, stable exit-code contract. It also sharpens the most common config error for operators hand-editing TOML with a friendly "did you mean?" hint. It is a deliberate Quick Win: the report machinery and the edit-distance helper already exist; V1 wires them together without surprising existing wrappers or introducing false signals.

## Goals

- Give CI a reliable gate: 100% of doctor reports containing an error exit non-zero under `--strict`, and 0% exit non-zero without it.
- Preserve backward compatibility: default `atelier --doctor` exit behavior is unchanged (stays 0) across every existing fixture.
- Make the strict signal trustworthy: it fails when a runtime the agents *actually* depend on is missing, and stays quiet about runtimes a run never touches — so operators never learn to ignore it.
- Cut typo time-to-diagnosis: a near-miss hint accompanies 100% of single-edit typo'd table-name load failures, in one error message, with zero new warnings for valid custom names.
- Drive adoption of an opt-in capability: the gate is discoverable in-product (stderr nudge) and in docs (CLI Notes + copy-paste CI snippet), and is dogfooded in atelier's own release CI within one release.

## User Stories

1. As a CI engineer, I want `atelier --doctor --strict` to exit non-zero when config or runtime health checks report an error, so my pipeline fails fast instead of deploying a broken setup.
2. As a CI engineer, I want plain `atelier --doctor` to keep exiting 0, so adopting this never breaks my existing wrappers.
3. As a CI engineer, I want a documented exit-code contract I can branch on (`!= 0`), so my gate stays durable when atelier later refines its codes.
4. As a CI engineer, I want the strict check to fail when a runtime my configured agents unconditionally need is unavailable, so the gate reflects real runnability, not just file validity.
5. As a CI engineer, I want the strict check to stay quiet about runtimes I've configured but don't use on every run (council, inactive presets, model fallbacks), so the gate doesn't fail on paths a run never touches.
6. As an operator hand-editing config, I want a "did you mean `codex`?" hint when I typo a runtime/agent/preset table name, so I fix a one-character slip in seconds.
7. As an operator with legitimately unconventional custom names, I want them to keep working with no new warnings, so the hint never punishes a valid config.
8. As an operator who runs plain `--doctor` and sees errors, I want a one-line pointer to `--strict`, so I discover the CI gate without reading docs.
9. As a CI engineer wiring this up, I want a copy-paste CI example, so I can add the gate in minutes.
10. As an atelier maintainer, I want our own release pipeline to run `--doctor --strict`, so we dogfood the gate and catch our own config regressions.

## Core Features

1. **Scriptable Strict Doctor (`--doctor --strict`)**
   An opt-in flag. When present, the doctor returns a non-zero process exit if any check reports an error. Plain `--doctor` is unchanged and always exits 0. `--strict` is valid only with `--doctor`.

2. **Documented Exit-Code Contract**
   A published contract: `0` = healthy; non-zero = unhealthy. Codes `1` (ran and found problems) and `2` (could not evaluate / invalid config) are *reserved*. The only stable guarantee advertised is "branch on `!= 0`, not on a specific code." V1 returns `0` and a generic non-zero; it does not yet distinguish `2`.

3. **In-Use Runtime Elevation**
   Under `--strict`, an unavailable runtime is treated as an error only when it is the primary runtime of an agent the default orchestrator chain resolves on every run. Runtimes used only by the council, by an inactive preset, or as a model fallback remain warnings (visible, non-failing). This keeps every strict failure a *certain* broken run, not a probable one.

4. **Additive Typo Hint**
   When config loading already fails on a near-miss table name, the error gains a "did you mean `codex`?" suggestion. The hint is strictly additive — it never introduces a new failure or warning for a config that loads successfully, including unconventional custom names.

5. **Discovery Nudge**
   When plain `--doctor` finds errors, it prints a one-line pointer on stderr ("N error(s) found; re-run with `--strict` to fail CI"). Standard output and `--json` output stay clean for scripts.

6. **Operator Documentation**
   The README CLI list and "Notes" block gain `--strict` and the exit-code contract, plus a short copy-paste CI example. Atelier's own release pipeline adopts `--doctor --strict` as a health gate.

## User Experience

**Primary flow — CI gate (Priya, platform/CI engineer):**
1. A pipeline step runs `atelier --doctor --strict` on the runner.
2. If the config and in-use runtimes are healthy, it exits 0 and the pipeline proceeds.
3. If a check reports an error (e.g. an agent's required runtime CLI is missing), it exits non-zero and the pipeline fails, printing the human report (or `--json` when requested).

**Discovery flow — local (Marco, operator):**
1. The operator runs `atelier --doctor` and sees one or more errors.
2. A stderr nudge points to `--strict`.
3. The operator re-runs with `--strict` to reproduce the CI verdict locally.

**Typo flow — local (Marco):**
1. The operator writes `[runtimes.codx]` intending `[runtimes.codex]`.
2. Config load fails with "...missing required field type; did you mean `codex`?"
3. The operator corrects the name and proceeds.

**Onboarding & discoverability:** `--doctor` is already the first post-install command in the README. `--strict` appears alongside it in the CLI list and Notes, with the CI snippet as the worked example; the stderr nudge is the in-product on-ramp.

## High-Level Technical Constraints

User-facing boundaries that shape the product without prescribing implementation:

- **Non-breaking by default.** Default `--doctor` exit behavior must not change; the new behavior is opt-in only.
- **Script-clean output.** Hints and nudges go to stderr; stdout and `--json` remain uncorrupted for piped consumers.
- **Contract stability.** The exit-code space is a compatibility surface: codes `1`/`2` are reserved and documented as not-yet-branchable, and documentation must not claim a granularity V1 does not emit.
- **Honest framing.** User-facing text describes the typo hint as "better hints on errors that already occur," never as comprehensive "typo detection," because V1 cannot catch a typo that loads successfully.

## Non-Goals (Out of Scope)

- **Emitting a distinct exit code `2`** for invalid-config-vs-found-errors — reserved and documented now, deferred to a later phase (must land before `--strict` is advertised as *the* supported CI integration).
- **Detecting typos that load cleanly** — a phantom `[agents.fixr]` that parses and is silently ignored is deferred to a V2 doctor warning, to avoid false positives on valid custom names.
- **Model-fallback and cross-preset runtime coverage** — fallback runtimes and runtimes introduced only by a non-selected preset remain warnings in V1.
- **JSON Schema export for the TOML config** — a more ambitious shift-left path, named as the V2 successor.
- **A separate `--check-config` mode** — redundant with `--print-config` plus `--doctor --strict`.
- **Failing or warning on valid near-miss custom names** — the hint is additive to errors that already fire; it never punishes a config that loads.
- **Changing the default `--doctor` exit code** to non-zero — explicitly rejected to protect existing wrappers.

## Phased Rollout Plan

### MVP (Phase 1 — this PRD)
All five capabilities plus documentation, shipped together: strict gate, reserved exit-code contract, in-use runtime elevation, additive typo hint, discovery nudge, README Notes + CI snippet, and dogfooding in release CI.
**Success criteria to proceed:** strict exit is correct across healthy/unhealthy fixtures; zero regression to default `--doctor`; typo hint has zero false positives on valid configs; in-use elevation fires only for unconditional-chain runtimes; the dogfood CI gate is green.

### Phase 2
Emit the distinct exit code `2` for invalid-config (with the contract already reserved); add a doctor-surfaced warning for phantom entries that load cleanly; extend runtime coverage to model-fallback and selected-preset paths.
**Success criteria to proceed:** graduated codes adopted by at least the dogfood pipeline without breaking the `!= 0` contract; phantom warning has an acceptance test proving a load-clean near-miss is surfaced.

### Phase 3
Unify doctor, config-load, typo, and phantom detection under a single `validate(config)` verdict model and ship JSON Schema export so editors and pre-commit catch errors before doctor runs.
**Long-term success:** config errors are caught earliest (editor/pre-commit), and every diagnostic flows through one severity model.

## Success Metrics

- **Strict exit correctness:** 100% of error-bearing reports exit non-zero under `--strict`; 0% without it.
- **Backward-compat preservation:** 0 change to default `--doctor` exit across all fixtures.
- **Typo time-to-diagnosis:** near-miss hint present in 100% of single-edit typo'd table-name load failures, in one message.
- **Hint false-positive rate:** 0 new hints or failures for valid custom-named entries.
- **In-use elevation accuracy:** 100% of unconditional-chain unavailable runtimes flagged; 0% for council/preset/fallback runtimes.
- **Dogfood adoption:** `--doctor --strict` present in atelier's release CI within one release.

## Risks and Mitigations

- **Adoption risk — an opt-in flag nobody flips.** The default keeps exiting 0, so the capability could go unused. *Mitigation:* in-product stderr nudge, README Notes + copy-paste CI snippet, and dogfooding in atelier's own release pipeline.
- **Trust risk — false CI failures.** If the strict gate failed on runtimes a run never touches, adopters would disable it. *Mitigation:* elevation is scoped to the unconditional chain, where a missing runtime guarantees a broken run; conditional runtimes stay warnings.
- **False-confidence risk — typo coverage.** Users could believe typos are fully caught when V1 only sharpens errors that already fire. *Mitigation:* honest user-facing framing and a committed Phase 2 phantom-entry warning.
- **Competitive/positioning risk — low.** Health checks are table stakes, but most "doctor" tools get exit codes wrong (always 0, or non-zero on everything) and serde offers no near-miss; the opt-in middle plus suggestions are a modest differentiator. *Mitigation:* lead messaging with the trustworthy, non-breaking gate.
- **Scope-creep risk.** Pressure to resurrect `--check-config`, emit `2`, or build schema export inside V1. *Mitigation:* explicit Non-Goals and a phased plan with the successors named.

## Architecture Decision Records

- [ADR-001: V1 scope and exit-code contract for scriptable config validation](adrs/adr-001.md) — opt-in `--strict`, reserved `0/1/2` contract (emit `0/1`), unconditional-chain runtime elevation, additive typo hint, discovery nudge.
- [ADR-002: Atomic V1 delivery for config validation UX](adrs/adr-002.md) — ship all five capabilities plus docs as one release; friendly "did you mean?" phrasing; docs in V1 scope.

## Open Questions

- Should the copy-paste CI snippet target a specific platform (e.g. GitHub Actions) or stay platform-neutral shell, given atelier's own CI is GitHub Actions?
- Final wording of the stderr discovery nudge.
- Does the exit-code contract warrant its own short docs section, or is the README "Notes" block sufficient for V1?
- *(Deferred to TechSpec)* Whether in-use detection threads invocation context (selected preset, council on/off); the edit-distance threshold; explicit handling of built-in runtimes like `fake`.
