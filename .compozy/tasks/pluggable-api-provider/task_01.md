---
status: pending
title: "Extract shared HTTP utilities into http_util.rs"
type: refactor
complexity: high
dependencies: []
---

# Task 01: Extract shared HTTP utilities into http_util.rs

## Overview

The codebase has 6 duplicated functions across `src/runtime/mod.rs` and `src/runtime/zai.rs`: `redact_sensitive_text`, `redact_bearer_tokens`, `redact_raw_secret_tokens`, `next_raw_secret_prefix`, `is_secret_token_character`, and `parse_runtime_output`. Additionally, `codex.rs` has its own private copy of `parse_runtime_output`. This duplication creates a secret-leak risk as redaction logic can silently diverge. This task consolidates all copies into a single `src/runtime/http_util.rs` module and expands `SECRET_PREFIXES` to cover new providers.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC "Core Interfaces" and "Implementation Design" sections for the shared module structure
- FOCUS ON "WHAT" — extract duplicated functions into a shared module
- MINIMIZE CODE — this is a mechanical extraction, not a rewrite
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create `src/runtime/http_util.rs` with all 6 functions moved from their current locations
- MUST make `redact_sensitive_text` and `parse_runtime_output` `pub(crate)` visible (hooks modules import `redact_sensitive_text`)
- MUST expand `SECRET_PREFIXES` from `["sk-", "zai-"]` to `["sk-", "zai-", "or-", "ov-"]` in the shared copy
- MUST update `src/runtime/mod.rs` to re-export from `http_util` (preserves `crate::runtime::redact_sensitive_text` import path for hooks)
- MUST update `src/runtime/codex.rs` to import `parse_runtime_output` from `super::http_util` instead of defining its own copy
- MUST remove all duplicated function definitions from `mod.rs` and `zai.rs`
- MUST NOT change function signatures or behavior — this is a pure extraction
- MUST pass full test suite after extraction
</requirements>

## Subtasks
- [ ] 01.1 Create `src/runtime/http_util.rs` with the 6 functions extracted from `mod.rs` (the `pub(crate)` canonical copies)
- [ ] 01.2 Expand `SECRET_PREFIXES` in the shared module to include `"or-"` and `"ov-"` prefixes
- [ ] 01.3 Update `src/runtime/mod.rs` to re-export `redact_sensitive_text`, `redact_bearer_tokens`, and `parse_runtime_output` from `http_util`
- [ ] 01.4 Remove the 5 duplicated private functions from `src/runtime/zai.rs` (lines 465-530) and import from `super::http_util`
- [ ] 01.5 Remove the private `parse_runtime_output` from `src/runtime/codex.rs` (line 471) and import from `super::http_util`
- [ ] 01.6 Verify that `src/hooks/follow.rs` and `src/hooks/dispatch.rs` still compile (they import `crate::runtime::redact_sensitive_text` which is now re-exported)
- [ ] 01.7 Run `cargo test --lib` and `cargo clippy --all-targets` to verify no regressions

## Implementation Details

The canonical copies of all 6 functions currently live in `src/runtime/mod.rs` (lines 650-770). These are the `pub(crate)` versions that hooks modules already import. The extraction is mechanical: move the function bodies to `http_util.rs`, add `pub(crate)` visibility, and re-export from `mod.rs`.

The `zai.rs` copies (lines 175, 465-530) are private and identical to the `mod.rs` versions — they can be deleted outright after importing from `http_util`.

The `codex.rs` copy of `parse_runtime_output` (line 471) is also private and identical — delete and import.

After extraction, `mod.rs` should contain a `pub use http_util::{redact_sensitive_text, redact_bearer_tokens, parse_runtime_output};` re-export to preserve the existing `crate::runtime::redact_sensitive_text` import path used by hooks.

See TechSpec "Implementation Design" section for the shared module structure.

### Relevant Files
- `src/runtime/mod.rs` — source of canonical copies (lines 650-770); add `pub mod http_util;` and re-exports
- `src/runtime/zai.rs` — remove duplicated private functions (lines 175, 465-530)
- `src/runtime/codex.rs` — remove private `parse_runtime_output` (line 471), update import
- `src/hooks/follow.rs` — imports `crate::runtime::redact_sensitive_text` (line 12); should work unchanged via re-export
- `src/hooks/dispatch.rs` — imports `crate::runtime::redact_sensitive_text` (line 18); should work unchanged via re-export

### Dependent Files
- `src/runtime/http_util.rs` — NEW file to create
- `src/runtime/mod.rs` — add `pub mod http_util;` declaration and re-exports
- `src/runtime/zai.rs` — remove duplicated functions, add `use super::http_util::*;`
- `src/runtime/codex.rs` — remove duplicated `parse_runtime_output`, add `use super::http_util::parse_runtime_output;`

### Related ADRs
- [ADR-003: Extract Shared HTTP Utilities into http_util.rs](adrs/adr-003.md) — Primary decision for this task

## Deliverables
- `src/runtime/http_util.rs` with all 6 functions (`pub(crate)` visibility)
- Updated `src/runtime/mod.rs` with re-exports
- Updated `src/runtime/zai.rs` without duplicated functions
- Updated `src/runtime/codex.rs` without duplicated `parse_runtime_output`
- Unit tests with 80%+ coverage **(REQUIRED)**
- All existing tests pass **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `redact_sensitive_text` redacts `sk-`, `zai-`, `or-`, and `ov-` prefixed tokens
  - [ ] `redact_bearer_tokens` redacts `Authorization: Bearer <token>` patterns
  - [ ] `parse_runtime_output` correctly parses valid JSON agent results
  - [ ] `parse_runtime_output` returns `ParseError` for invalid JSON
  - [ ] `next_raw_secret_prefix` returns all 4 prefixes in sequence
  - [ ] `is_secret_token_character` correctly identifies token characters
- Integration tests:
  - [ ] `cargo test --lib` passes with no regressions
  - [ ] `cargo clippy --all-targets` passes with no warnings
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Zero duplicated redaction/parsing functions (grep confirms single definitions)
- `cargo clippy --all-targets` clean
- Hooks modules compile without import changes
