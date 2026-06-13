# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State

## Shared Decisions

## Shared Learnings
- Built-in agent defaults: `src/config/mod.rs` `insert_builtin_agent` block (~629-765). Orchestrator=`zai`/`glm-5.1`; workers explorer/fixer/reviewer=`codex`/`default`; oracle/consul=`zai`; librarian+designer disabled by default. Runtime defaults (incl. `ZAI_API_KEY` env, `https://api.z.ai/api/paas/v4`) at ~600-627.
- First-run truth (use across all docs pages): only the `fake` runtime is zero-setup; a *real* run needs `ZAI_API_KEY` (orchestrator on zai — `src/runtime/zai.rs:65-67` hard-errors without it) AND a `codex` login (default workers).

## Open Risks

## Handoffs
