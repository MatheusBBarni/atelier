---
status: completed
title: FileIndex walk and candidate model
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 02: FileIndex walk and candidate model

## Overview
Create the `src/file_index.rs` module with the `FileEntry` candidate model and a gitignore-aware filesystem walk rooted at the working directory. This is the only component that touches the filesystem, and it enforces the security guardrails (secret denylist, working-dir pin, no symlinks).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create `src/file_index.rs` and register it as `pub mod file_index;` in `src/lib.rs` (alphabetical, after `codemap`).
- MUST define `FileEntry` with a forward-slashed relative path, an is-directory flag, a modified-time, and a path depth (per TechSpec "Core Interfaces").
- MUST walk via the `ignore` crate rooted at the working directory, including both files AND folders, and including untracked-but-not-ignored entries.
- MUST exclude: `.gitignore`-ignored paths, `.git/`, a project force-exclude set (at minimum `.multiagent`, `target`, `node_modules`), and a secret-name denylist (`.env*`, `*.pem`, `*.key`, `id_rsa*`, `.ssh/`, `.aws/`).
- MUST NOT follow symlinks, and MUST reject any candidate whose canonical path resolves outside the working-directory root.
- MUST return owned results (`Vec<FileEntry>`) and skip unreadable subtrees rather than failing the whole walk.
</requirements>

## Subtasks
- [x] 2.1 Define the `FileEntry` model and register the module in `src/lib.rs`.
- [x] 2.2 Implement the gitignore-aware walk over the working-directory root.
- [x] 2.3 Apply the secret-name denylist and project force-exclude set.
- [x] 2.4 Reject symlinks and any path resolving outside the root.
- [x] 2.5 Populate is-dir, mtime, depth, and forward-slashed relative path.
- [x] 2.6 Add unit tests over a `tempfile` directory tree.

## Implementation Details
Create `src/file_index.rs`; add the module declaration to `src/lib.rs` after `pub mod codemap;`. Use the `ignore` crate's builder; layer the secret denylist and force-excludes on top of gitignore handling. Follow the repo's `anyhow::Result` + `.context(...)` convention for any fallible setup. See TechSpec "Core Interfaces" (`FileEntry`, `FileIndex::walk`) and the `src/codemap` walk for the exclusion + tempfile-test precedent (note: codemap does NOT honor `.gitignore`; this module does).

### Relevant Files
- `src/lib.rs` — module declaration list; add `pub mod file_index;`.
- `src/codemap/mod.rs` — precedent for filesystem walking, the `EXCLUDED_DIRS` set to carry over, and the `tempfile::tempdir()` test pattern.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Core Interfaces" defines `FileEntry` and `FileIndex::walk`.

### Dependent Files
- `src/file_index.rs` — extended by task_03 (query).
- `src/tui/mod.rs` — task_04's worker calls `FileIndex::walk`.

### Related ADRs
- [ADR-001: Scope @-Mention File Dropdown V1](../adrs/adr-001.md) — gitignore walk + security guardrails.
- [ADR-005: Component Placement and Dropdown Integration](../adrs/adr-005.md) — index lives in a new non-TUI module.

## Deliverables
- `src/file_index.rs` with `FileEntry` and the walk function; `src/lib.rs` updated.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests over a mixed tracked/ignored/secret tree **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Walking a `tempfile` tree returns the expected relative paths for both files and nested folders.
  - [ ] A path listed in `.gitignore` (e.g. `ignored.log`) does not appear.
  - [ ] `target/` and `node_modules/` contents do not appear.
  - [ ] A `.env` file and a `server.pem` file are excluded by the secret denylist.
  - [ ] A symlink is not followed and its target is not listed.
  - [ ] A candidate that would resolve outside the root (via symlink or `..`) is rejected.
  - [ ] `is_dir`, `mtime`, and `depth` are populated; nested paths use forward slashes.
- Integration tests:
  - [ ] A `tempfile` repo containing tracked, gitignored, and secret files yields a candidate set with every ignored/secret/symlink entry absent.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Walk returns files and folders, excludes ignored/secret/symlink/out-of-root entries, and is rooted at the working directory
