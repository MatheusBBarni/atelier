---
status: pending
title: "Refactor the redacted-config builder for reuse"
type: refactor
complexity: medium
dependencies: []
---

# Refactor the redacted-config builder for reuse

## Overview

The `EffectiveConfig → PrintableConfig` mapping is currently inlined inside
`to_redacted_toml`, so nothing else can reuse the redacted structured view. This task
factors that mapping into a reusable function and exposes the `Printable*` types at crate
visibility, so the docs generator (task_10) can build the same redacted view and render
Markdown — without changing `--print-config`'s output (ADR-003).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST extract the inlined `EffectiveConfig → PrintableConfig` mapping into a function (e.g., `build_printable_config(&EffectiveConfig) -> PrintableConfig`).
- `to_redacted_toml` MUST become a thin wrapper over that function with **byte-identical** output.
- MUST make the `Printable*` types reachable from a sibling `docgen` module (`pub(crate)` or an accessor).
- MUST preserve every redaction behavior: env-var names not secrets, `prompt_source` labels, authored-args-only for runtimes, prompt-file paths without bodies.
- MUST NOT change `[presets.*]` handling (it remains absent from the merged config and is documented separately).
</requirements>

## Subtasks
- [ ] 9.1 Extract `build_printable_config` from `to_redacted_toml`.
- [ ] 9.2 Rewrite `to_redacted_toml` as a thin serialize-only wrapper.
- [ ] 9.3 Expose the `Printable*` types at crate visibility for `docgen`.
- [ ] 9.4 Run the redaction guardrail tests and confirm they still pass.
- [ ] 9.5 Confirm `atelier --print-config` output is unchanged.

## Implementation Details

Refactor within `src/config/mod.rs` (the inlined builder at `:1866-1970`). See TechSpec
"Known Risks" (the `PrintableConfig` refactor) and "Data Models". The behavior contract is
pinned by the existing redaction tests — treat them as guardrails, not as tests to change.

### Relevant Files
- `src/config/mod.rs:1792-1805` — the `PrintableConfig` (and sibling `Printable*`) types to expose.
- `src/config/mod.rs:1866-1970` — `to_redacted_toml` and the inlined builder to extract.
- `src/config/mod.rs:1972` — `prompt_source_label` redaction helper.
- `src/config/mod.rs:2394,2833,2852,2873,2921,2569` — the redaction/format guardrail tests.

### Dependent Files
- `src/docgen/mod.rs` (task_10) — consumes `build_printable_config`.
- `src/cli.rs:128-131` — the `--print-config` handler whose output must stay identical.

### Related ADRs
- [ADR-003: Reference generator — a Rust --emit-docs subcommand](../adrs/adr-003.md) — requires this factor-out for in-process reuse.

## Deliverables
- `build_printable_config` extracted; `to_redacted_toml` reduced to a thin wrapper.
- `Printable*` types reachable from `docgen`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test confirming `--print-config` is unchanged **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] For a representative loaded config, `toml::to_string_pretty(&build_printable_config(cfg))` equals the previous `to_redacted_toml(cfg)` output byte-for-byte.
  - [ ] The existing redaction tests still pass: `api_key_env` is emitted (not the secret), `prompt_source` is `"inline_redacted"`/`"file"`, and prompt bodies / `Bearer` tokens are absent.
  - [ ] `build_printable_config` output exposes the agents/runtimes/council/limits/ui/workspace sections for a default config.
- Integration tests:
  - [ ] `atelier --print-config` over an empty config dir produces the same TOML as before the refactor (the `print_config_renders_toml` path).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The redacted-config builder is reusable and `--print-config` output is byte-identical.
- All redaction invariants are preserved.
