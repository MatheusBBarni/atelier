# Task Memory: task_05.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
- Implement Task 05: project `workflow_started` and `workflow_completed` history events into Chat while preserving generic non-workflow `run_completed` behavior.
- Acceptance requires workflow completion severity mapping, evidence-rich body lines, distinct lifecycle keys, focused projection tests, app/fake-runtime chat projection coverage, and full verification before tracking updates.

## Important Decisions
- Keep the implementation scoped to `src/app/chat/projection.rs` unless app tests need direct projected-chat assertions; task 04 already emits the required workflow history payloads.
- Do not promote anything to shared workflow memory yet; current findings are task-local and derive from the Task 05 docs plus existing code.
- Added `ChatLifecycleKey::Workflow { run_id }` for workflow lifecycle projection. `workflow_started` and `workflow_completed` share this key, while generic `run_completed` keeps `ChatLifecycleKey::Run`, so later generic completion cannot overwrite workflow evidence.
- Workflow completion body limits reserve space for each evidence category: target counts, up to 3 unfinished targets plus overflow, and one visible item plus overflow for verification, skipped checks, and residual risks.

## Learnings
- Baseline signal captured before edits: `src/app/chat/projection.rs` had no `workflow_started` or `workflow_completed` handlers.
- `workflow_completed` events are emitted before generic `run_completed`, so projection must use a workflow-specific lifecycle key to avoid overwriting the evidence-rich item.
- `workflow_completed` payload arrays are optional in projection; missing `unfinished_targets`, `verification`, `skipped_checks`, and `residual_risks` now render without panic or placeholder lines.
- Final verification passed after implementation: `cargo fmt -- --check`, `git diff --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, `cargo llvm-cov --locked --summary-only` (91.05% line coverage), and `cargo build --locked`.

## Files / Surfaces
- `src/app/chat/projection.rs` - expected primary implementation and unit tests.
- `src/app/mod.rs` - expected app/fake-runtime chat projection assertions.
- `src/app/chat/mod.rs` - added workflow lifecycle key and item id format.
- `src/app/chat/projection.rs` - implemented `workflow_started` and `workflow_completed` projection, body formatting helpers, and focused unit tests.
- `src/app/mod.rs` - extended fake-runtime app tests for warning workflow chat item and non-workflow generic completion regression.

## Errors / Corrections
- Self-review correction: reduced workflow evidence per-section limits so residual risks cannot be truncated out when unfinished targets, verification, and skipped checks are all present.

## Ready for Next Run
- Task 05 implementation, verification, and tracking updates are complete; create a scoped local commit with code/test changes only unless tracking files are explicitly staged by the workflow owner.
