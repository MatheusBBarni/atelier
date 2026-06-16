# Atelier — Compozy Task Packet Execution Order

Recommended order to execute the feature packets under `.compozy/tasks/`, each on its own
worktree + `feat/<slug>` branch + PR.

_Last updated: 2026-06-15_

## Order

| # | Packet | Tasks | Role | Why this position |
|---|--------|-------|------|-------------------|
| 1 | ✓ `governance-spine` | 8 | Foundation | Ships the shared `GovernanceDecisionView` contract that #2 and #7 are meant to consume. Doing it first lets them build on it directly instead of building their own surface and migrating later. |
| 2 | ✓ `approval-trust-list` | 9 | Foundation (safety floor) | The non-bypassable destructive-action gate — highest safety value. Consumes the spine from #1. |
| 3 | ✓ `config-validation-ux` | 6 | Leaf (CI hygiene) | Small, no deps, introduces `--doctor --strict`; unblocks #9. Good early momentum + cheap win. |
| 4 | ✓ `config-driven-keybindings` | 9 | Foundation (low-risk) | Self-contained keybindings module; natural precursor to #6 (which hardcodes `Ctrl-R`). |
| 5 | `lifecycle-hooks` | 9 | Foundation | Clean standalone extension point; no deps. |
| 6 | `session-browser-resume` | 13 | Foundation | Activates the event-sourcing replay path; high user value; no deps. |
| 7 | `subtask-dag-execution` | 8 | Foundation | Parallel execution graph; consumes the spine from #1 (like #2). |
| 8 | `mcp-integration` | 11 | Foundation (large) | Biggest single lift (~2–3 wk, new `src/mcp/` subsystem). High value but heavy — schedule deliberately. |
| 9 | `config-setup-skill` | TBD | Leaf (onboarding) | Soft-needs `--doctor --strict` from #3 (satisfied by its earlier position). |
| 10 | `self-grading-retry-loop` | TBD | Leaf | Reuses existing machinery (`max_review_fix_cycles`, `AgentResult` fields); lowest priority. |

> `#9` and `#10` were early-stage (idea/ADR only) at analysis time; this assumes their PRD, techspec,
> and `task_NN.md` files exist before execution.

## Why this order — the only real constraints

Every packet ships independently behind a feature flag, so there are **no hard blockers**. The order is
driven by exactly three things:

1. **`governance-spine` before `approval-trust-list` and `subtask-dag-execution`.** The spine ships the
   shared decision contract; those two "migrate to populate it in Phase 2 on their own schedule"
   (ADR-002). Spine-first avoids that migration rework in two packets.
2. **`config-validation-ux` before `config-setup-skill`.** The skill gates on `atelier --doctor --strict`,
   which `config-validation-ux` introduces. Soft, but cheap to satisfy by ordering.
3. **Everything else is independent** — sequenced by value / risk / size (quick leaf wins early, the big
   `mcp-integration` lift and the two least-mature packets last).

## How to run it (staged, not all-at-once)

Generating ~73+ tasks of code across 10 PRs unattended is high-variance, and the real bottleneck is
human review of those PRs. Recommended staging:

1. **Pilot `config-validation-ux` first** (smallest, leaf, no dependents). Confirms the full loop
   — worktree → `cy-batch-tasks <slug> all auto-commit=true` → review → PR — actually holds together,
   and that the Compozy skills invoke correctly from inside a spawned subagent.
2. **Checkpoint after `governance-spine` + `approval-trust-list`** (the foundational two). Eyeball those
   PRs before continuing — they're the highest-value to get right.
3. **Then batch the remaining independent packets**, with the two newly-authored ones (#9, #10) last.

Verification tip: scope `cy-final-verify` to `cargo test --lib && cargo clippy --all-targets &&
cargo fmt --check`. A blind `cargo test` can fail packets on this repo's environment-sensitive
`runtime::codex`/`cursor` availability tests rather than on real regressions.

## Isolation vs. chaining

Each branch forks from the **same base**, so a later packet does **not** see an earlier one's code — the
order here is execution *priority*, not dependency *chaining*. This is consistent with the packets'
"Phase 2 migration" design (spine-consumers build their own surface, then converge later).

If you want the ordering to actually compound (e.g. `approval-trust-list` building on `governance-spine`'s
merged code), branch each packet from the **previous packet's branch** instead of the base — but that
produces stacked PRs and means merging machine-generated PRs mid-run.
