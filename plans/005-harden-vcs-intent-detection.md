# Plan 005: Harden VCS Intent Detection Against Negation

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- src/actions/mod.rs`
> If `src/actions/mod.rs` changed, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

VCS mutations are denied unless the user prompt explicitly requested that class of action. Today the gate uses substring checks, so negated prompts like "do not commit" still satisfy the `commit` check. Under the default yolo approval mode, that can let a runtime-requested VCS mutation proceed against user intent.

## Current State

```text
src/actions/mod.rs:523 pub fn is_vcs_mutation(command: &str) -> bool {
src/actions/mod.rs:550 pub fn vcs_action_explicitly_requested(user_prompt: &Option<String>, command: &str) -> bool {
src/actions/mod.rs:557 if command.starts_with("git commit") {
src/actions/mod.rs:558     prompt.contains("commit")
src/actions/mod.rs:559 } else if command.starts_with("git push") {
src/actions/mod.rs:560     prompt.contains("push")
```

Existing positive test coverage:

```text
src/actions/mod.rs:1749 fn commit_request_allows_default_staging_command()
src/actions/mod.rs:2160 async fn vcs_mutations_require_explicit_user_prompt_even_in_yolo()
```

Repo conventions:
- VCS action detection lives in `src/actions/mod.rs`.
- Yolo skips approvals for `Approve` actions, but hard policy denies still apply.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused tests | `rtk cargo test --lib vcs` | VCS policy tests pass |
| Full Rust gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |

## Scope

**In scope**:
- `src/actions/mod.rs`

**Out of scope**:
- Do not implement natural-language understanding beyond conservative intent checks.
- Do not change non-VCS command approval behavior.
- Do not add runtime-specific VCS policy.

## Git Workflow

- Branch: `advisor/005-harden-vcs-intent`
- Commit message example: `fix(actions): reject negated vcs mutation intent`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add Failing Negation Tests

Add tests for `vcs_action_explicitly_requested` showing these return false:
- prompt: `do not commit`, command: `git commit -m test`
- prompt: `don't push`, command: `git push origin main`
- prompt: `without committing`, command: `git commit -m test`
- prompt: `explain git commit`, command: `git commit -m test`
- prompt: `show me how to push later`, command: `git push origin main`

Keep existing positive tests for `commit and push the current changes`.

**Verify**: `rtk cargo test --lib vcs` -> new tests fail before implementation.

### Step 2: Add A Conservative Positive Intent Helper

Replace raw `prompt.contains(...)` checks with a helper that accepts only unambiguous positive wording. A simple acceptable approach:
- reject if a negation phrase appears near the action word (`do not`, `don't`, `dont`, `without`, `avoid`, `never`, `no`);
- reject explanatory phrases such as `explain git commit`, `how to commit`, or quoted mentions;
- accept direct positive phrases such as `commit`, `commit and push`, `please push`, `stage for commit`, `switch branch`, `merge`, etc.

Prefer false negatives over false positives. If intent is unclear, the command should be denied by the VCS explicit-request gate and the model can ask the user.

**Verify**: `rtk cargo test --lib vcs` -> tests pass.

### Step 3: Add Integration-Level Action Test

Add a test near `vcs_mutations_require_explicit_user_prompt_even_in_yolo` that sets `context.user_prompt = Some("do not commit anything".to_string())` and confirms `execute_action_request` denies `git commit -m test`.

**Verify**: `rtk cargo test --lib vcs_mutations` -> integration-style action tests pass.

### Step 4: Run The Full Gate

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
```

**Verify**: exit 0; all tests pass.

## Test Plan

- Unit tests for direct positive, negated, instructional, and quoted VCS mentions.
- Action execution test under default yolo to prove hard denial still applies.

## Done Criteria

- [ ] Negated VCS prompts do not authorize VCS mutations.
- [ ] Existing positive commit/push/stage flows still work.
- [ ] Full Rust gate exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- The desired behavior requires a broad NLP classifier.
- Existing documented workflows rely on negated/ambiguous prompts authorizing VCS commands.
- The fix requires changes outside `src/actions/mod.rs`.

## Maintenance Notes

When adding new VCS mutation prefixes, add positive and negated prompt tests at the same time.

