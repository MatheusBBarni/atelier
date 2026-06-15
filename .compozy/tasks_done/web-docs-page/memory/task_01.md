# Task Memory: task_01.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Fix README "Requirements"/"Runtimes" wording so default credentials read as required-by-default (F10). Done.

## Important Decisions
- Kept claude/cursor framed as opt-in "Optional"; only zai (orchestrator) and codex (workers) are required-by-default.
- Added a one-line "default runtime for…" note to the codex and zai entries under "## Runtimes".

## Learnings
- Built-in defaults live in `src/config/mod.rs` `insert_builtin_agent` block (~lines 629-765): orchestrator=zai/glm-5.1; explorer/fixer/reviewer=codex/default; oracle/consul/librarian=zai; designer=codex (disabled). librarian+designer disabled by default.
- `src/runtime/zai.rs:65-67` hard-errors when `api_key_env` (ZAI_API_KEY) is unset → key is effectively mandatory for any real run.

## Files / Surfaces
- README.md "Requirements" block + "Runtimes" codex/zai bullets (only sections changed).

## Errors / Corrections

## Ready for Next Run
- task_04 (quickstart.md) must mirror this honest framing: zai+ZAI_API_KEY and codex login required for a real run; only `fake` is zero-setup.
