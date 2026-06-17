---
status: completed
title: "HistoryStore::open() + self-healing metadata cache fields"
type: backend
complexity: medium
dependencies: []
---

# Task 02: HistoryStore::open() + self-healing metadata cache fields

## Overview
Add `HistoryStore::open(root, session_id)` to load and schema-validate an existing session (sibling to `create`), and extend `SessionMetadata` with `goal`, `outcome`, and `last_head_sha` as a derived, self-healing cache. This is the foundation every browse and resume path depends on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `HistoryStore::open(root, session_id)` that loads the session directory, reads `metadata.json`, and fails loud when `schema_version != 1` (no silent mis-load).
- MUST extend `SessionMetadata` with `goal: Option<String>`, `outcome: Option<String>`, and `last_head_sha: Option<String>`, all `#[serde(default)]` so existing metadata files still deserialize.
- MUST provide a self-healing write (`update_metadata_cache`) that rewrites the cache fields; the event log remains the source of truth.
- MUST keep `schema_version` at `1` (cache fields are additive, not a version bump) and preserve existing `0700/0600` file permissions.
</requirements>

## Subtasks
- [x] 2.1 Add `open(root, session_id)` returning a store bound to the existing session, validating metadata schema.
- [x] 2.2 Extend `SessionMetadata` with the three optional cache fields (serde defaults).
- [x] 2.3 Add a read-modify-write that updates the cache fields while preserving permissions.
- [x] 2.4 Add round-trip + legacy-compatibility unit tests.

## Implementation Details
Work in `src/history/mod.rs`. Mirror `create()` (`:93`) for path setup; reuse the private-file write helper for permissions. `SessionMetadata` is at `:75`; `read_events`/`read_events_from_path` (`:180`/`:263`) already validate `schema_version == 1` on the event side — apply the same check to metadata on `open`. See TechSpec "Core Interfaces" and ADR-008. Do not add a new module.

### Relevant Files
- `src/history/mod.rs` — `HistoryStore` (`:83`), `create` (`:93`), `SessionMetadata` (`:75`), private-file write helper (`:396`+).

### Dependent Files
- `src/history/mod.rs` — task_03 (`list_session_summaries`) and task_06 (preview fold) call `open()`.
- `src/app/mod.rs` — task_10 (`adopt_session`) and task_11 (resume) call `open()`; task_13 reads cache outcome.

### Related ADRs
- [ADR-003: Production replay fold as a maintained schema-compatibility contract](adrs/adr-003.md) — `open()` is the schema-validation boundary.
- [ADR-008: Lifecycle events as additive string-kinds + self-healing metadata cache](adrs/adr-008.md) — defines the metadata cache + self-heal.

## Deliverables
- `HistoryStore::open(root, session_id)` with schema validation.
- Extended `SessionMetadata` (goal/outcome/last_head_sha, serde defaults).
- `update_metadata_cache` self-healing write.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: open a `create()`d session and round-trip events **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `open()` on a `create()`d session returns a store whose `read_events()` matches the originally appended events. — `open_loads_existing_session_and_reads_same_events_in_order`
  - [x] `open()` on a session whose `metadata.json` has `schema_version = 2` returns an error (fails loud). — `open_rejects_unsupported_metadata_schema_version`
  - [x] Legacy `metadata.json` lacking `goal`/`outcome`/`last_head_sha` deserializes with `None` defaults. — `legacy_metadata_without_cache_fields_defaults_to_none`
  - [x] `update_metadata_cache(...)` then re-read returns the updated fields and preserves `0600` permissions (Unix). — `update_metadata_cache_persists_fields_and_preserves_permissions` (+ `_round_trips_last_head_sha`)
- Integration tests:
  - [x] `create()` → append events → `open()` same session id → `read_events()` returns the same events in order. — `open_loads_existing_session_and_reads_same_events_in_order` (two events, order asserted)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- An existing session can be loaded and validated without going through `create()`.
- Old metadata files load unchanged; new cache fields default to `None`.

## As-built notes
- `HistoryStore::open(root, session_id)` takes the `.atelier` data root (matching `HistoryStore.root` and `list_session_event_paths`) — not the workspace dir — and binds to `root/sessions/<id>`. It reads + schema-validates `metadata.json` via a shared private `read_metadata` helper, returning a loud error on `schema_version != 1` (mirrors the event reader, ADR-003).
- `SessionMetadata` gains `goal`, `outcome`, `last_head_sha` — all `Option<String>` + `#[serde(default)]`, so pre-cache files still deserialize; `create()` initializes them to `None`. Schema stays `1` (additive).
- **`update_metadata_cache(goal, outcome, last_head_sha)` is 3-arg** (the requirement says it updates "the cache fields" — all three; the task's 2-arg test bullet is illustrative shorthand). It read-modify-writes via `write_private_file`, preserving the authoritative fields and the `0600` permission. Added `read_metadata()` accessor for callers (task_03/06/13).
