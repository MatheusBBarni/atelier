---
status: pending
title: "Single-source tiered secret scanner"
type: backend
complexity: medium
dependencies: []
---

# Task 02: Single-source tiered secret scanner

## Overview
Add a findings-returning secret scanner, `scan_secrets`, to `runtime::status` by refactoring the existing `looks_like_secret`/`key_is_sensitive` classification into one source that feeds both the new scanner and the current `redact_secrets`. The scanner returns spans with a Deterministic/Advisory confidence split — the precision-first detector the review gate (task_04) and serializer (task_03) depend on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `SecretFinding`, `Confidence` (Deterministic|Advisory), `SecretCategory` (ProviderToken|SensitiveKeyValue|CredentialFile|HighEntropy), and `pub fn scan_secrets(text: &str) -> Vec<SecretFinding>` in `src/runtime/status.rs`. See TechSpec "Core Interfaces".
- MUST classify as **Deterministic**: the existing provider prefixes plus AWS `AKIA…`, Stripe `sk_live_`/`pk_live_`, Google `AIza…`, JWT `eyJ…`, Slack `xapp-`, GitLab `glpat-`, PEM private-key headers, and sensitive `key=value`/`key: value`.
- MUST classify the ≥20-char entropy heuristic as **Advisory** only (never gating), so git SHAs, base64 blobs, and UUIDs do not gate.
- MUST refactor `looks_like_secret`/`key_is_sensitive` to be the single classifier feeding both `scan_secrets` and the existing `redact_secrets`, with NO regression to current redaction tests.
- MUST return byte spans `(start, end)` for each finding.
- Credential-file detection MUST reuse `file_index::is_secret_name` rather than introduce a new list.
</requirements>

## Subtasks
- [ ] 2.1 Define the finding / confidence / category types.
- [ ] 2.2 Extend the deterministic prefix/pattern set (AWS, Stripe, Google, JWT, Slack, GitLab, PEM) and sensitive-key detection.
- [ ] 2.3 Implement `scan_secrets` returning spans + confidence, reusing the refactored classifier.
- [ ] 2.4 Keep `redact_secrets` and existing redaction behavior intact on the shared classifier.
- [ ] 2.5 Build a seeded secret corpus and assert recall on the deterministic class.

## Implementation Details
All changes are in `src/runtime/status.rs`, reusing `file_index::is_secret_name`. See TechSpec "Core Interfaces" for the type/signature shapes and ADR-004 for the tiering rationale and ruleset. Do not duplicate the weaker write-time pass in `runtime::http_util` — this task reconciles the divergence by making `status` the single source.

### Relevant Files
- `src/runtime/status.rs` — `looks_like_secret` (~1047), `key_is_sensitive` (~1029), `redact_secrets` (~924), existing redaction tests (~1614).
- `src/file_index.rs` — `is_secret_name` (~316) for credential-file names.
- `src/runtime/http_util.rs` — the weaker `redact_sensitive_text` (note divergence; do not extend).

### Dependent Files
- `src/app/chat/markdown.rs` (task_03) — uses findings + spans to label flagged content.
- `src/export.rs` (task_04) — gate tiers on the Deterministic count.

### Related ADRs
- [ADR-004: Tiered secret scan with fail-closed enforcement](../adrs/adr-004.md) — the design, the ruleset, and the single-source mandate.
- [ADR-001: Tiered detect-don't-mutate redaction](../adrs/adr-001.md) — the deterministic-gates / entropy-advises split.

## Deliverables
- `SecretFinding`/`Confidence`/`SecretCategory` types + `scan_secrets`.
- Refactored single-source classifier feeding `scan_secrets` and `redact_secrets`.
- Seeded-corpus recall tests **(REQUIRED)**.
- Unit tests with 80%+ coverage **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `scan_secrets("AKIA0123456789ABCDEF")` → one `ProviderToken`/Deterministic finding with the correct byte span.
  - [ ] `password=hunter2` → `SensitiveKeyValue`/Deterministic.
  - [ ] A 40-char git SHA and a UUID → at most `HighEntropy`/Advisory, never Deterministic.
  - [ ] JWT `eyJhbGci…`, Slack `xapp-…`, GitLab `glpat-…`, Stripe `sk_live_…`, Google `AIza…`, and a PEM header each → Deterministic.
  - [ ] Seeded corpus of labelled secrets → recall ≥99% on the deterministic class.
  - [ ] Existing `redact_secrets` output is unchanged for the current redaction-test inputs (no regression).
- Integration tests:
  - [ ] (covered in task_06) a session whose log contains a known secret yields a Deterministic flag at export time.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- ≥99% deterministic-class recall on the seeded corpus; entropy hits never gate
- No regression in existing `status` redaction tests; one classifier feeds both paths
