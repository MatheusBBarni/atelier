---
status: pending
title: Risk assessment & command normalization
type: backend
complexity: medium
dependencies: []
---

# Task 01: Risk assessment & command normalization

## Overview
Add the pure risk-classification layer that every later task builds on: a `RiskNote` (tier, catastrophic flag, reason, trust target) computed by `assess_risk`, plus a single `normalize_command` helper (tilde + `$HOME` expansion) used for both classification and trust-key construction. This is the safety-critical core — an action it fails to flag catastrophic would run silently under Yolo+Warn.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `RiskTier`, `RiskNote`, and `TrustTarget` types in `src/actions/mod.rs` per TechSpec "Core Interfaces".
- MUST add a pure `assess_risk(request, context) -> RiskNote` that returns `catastrophic = true` (and `target = None`) for the catastrophic set, and a Low/Medium/High tier otherwise.
- MUST add a single `normalize_command` helper that expands `~`/`~user`, `$HOME`/`${HOME}`, and `$PWD`, and collapses redundant whitespace, used by BOTH classification and `TrustTarget::Command` construction (ADR-004 drift guard).
- MUST classify on the normalized string and reuse the existing `has_shell_control_syntax` so pipes/substitution/redirects never reach the Low (safe) tier.
- MUST derive `TrustTarget::Command` for RunCommand and `TrustTarget::WritePath` for WriteFile/ApplyPatch; catastrophic actions MUST expose `target = None`.
- SHOULD keep the catastrophic set small and high-precision; uncertain cases fall to Medium/High (which re-prompt), never to catastrophic-by-guess.
</requirements>

## Subtasks
- [ ] 01.1 Define `RiskTier`, `RiskNote`, `TrustTarget` and their serde derives.
- [ ] 01.2 Implement `normalize_command` (tilde/`$HOME`/`$PWD` + whitespace) as the shared helper.
- [ ] 01.3 Implement `assess_risk`, reusing `classify_command`/`has_shell_control_syntax`, mapping tiers and the catastrophic set.
- [ ] 01.4 Derive the `TrustTarget` per action kind; return `None` when catastrophic.
- [ ] 01.5 Add unit + adversarial-variant tests for the catastrophic set and normalization parity.

## Implementation Details
All work is in `src/actions/mod.rs`, beside the existing `classify_command`/`decision_for_command`. No new modules or files. See TechSpec "Implementation Design → Core Interfaces" and "System Architecture → Component Overview" for the type shapes and the normalization rule; see ADR-003 for the fail-closed framing and expansion depth.

### Relevant Files
- `src/actions/mod.rs` — host module; `classify_command` (~371), `has_shell_control_syntax` (~486), `is_default_read_only_command` (~443), `is_vcs_mutation` (~523), `ActionRequest`/`ActionKind` (~30–46) are the building blocks.

### Dependent Files
- `src/actions/mod.rs` (enforcement, task_03) — consumes `assess_risk`/`RiskNote`.
- `src/app/mod.rs` (task_04/05) — `TrustTarget` is the trust key and the approve-and-trust label.

### Related ADRs
- [ADR-003: Enforce floor + trust at the single enforcement point](../adrs/adr-003.md) — fail-closed assessment, tilde+`$HOME` normalization.
- [ADR-002: Phased floor rollout with a non-bypassable catastrophic core](../adrs/adr-002.md) — defines the catastrophic set.

## Deliverables
- `RiskTier`, `RiskNote`, `TrustTarget` types and `assess_risk` + `normalize_command` in `src/actions/mod.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Adversarial-variant test suite for the catastrophic matcher **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `rm -rf ~` → `catastrophic = true`, `tier = High`, `target = None`.
  - [ ] `rm -rf $HOME` and `rm -rf ${HOME}` → `catastrophic = true` (normalization closes the disguise).
  - [ ] `git push --force origin main` and `git push -f` → `catastrophic = true`.
  - [ ] `cat ~/.ssh/id_rsa` (secret read) → `catastrophic = true`.
  - [ ] `cargo test` → `tier = Low`, `catastrophic = false`, `target = Some(Command("cargo test"))`.
  - [ ] `npm install left-pad` → `tier = Medium`, `target = Some(Command(...))`.
  - [ ] WriteFile to a path inside a write root → `target = Some(WritePath(path))`, non-catastrophic.
  - [ ] Adversarial spacing/case/quoting: `RM  -RF   ~`, `rm -rf "$HOME"` → still `catastrophic = true`.
  - [ ] Normalization parity: `normalize_command("rm -rf ~") == normalize_command("rm -rf $HOME")`.
- Integration tests:
  - [ ] Exercised end-to-end via the enforcement matrix in task_03 and the FakeRuntime runs in task_05 (no standalone integration harness for this pure module).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Every catastrophic-set entry and its tilde/`$HOME`/quoting variants classify as catastrophic with no exposed trust target.
- A command and its normalized form produce identical `TrustTarget::Command` keys.
