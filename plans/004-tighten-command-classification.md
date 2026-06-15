# Plan 004: Require Approval For Mutating Command Forms

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

`normal` approval mode is supposed to pause before high-impact commands. The current classifier marks broad prefixes as `Allow`; some of those prefixes include mutating forms such as `cargo fmt` without `--check` and `find ... -delete`. Because allowed commands run through `sh -c`, the allowlist must be conservative.

## Current State

```text
src/actions/mod.rs:356 pub fn decision_for_command(command: &str, approval_mode: &ApprovalMode) -> ActionDecision {
src/actions/mod.rs:358     CommandClassification::Allow => ActionDecision::Allowed,
src/actions/mod.rs:448 let allow_prefixes = [
src/actions/mod.rs:452     "cargo fmt",
src/actions/mod.rs:472     "find ",
src/actions/mod.rs:947 async fn execute_run_command(...)
src/actions/mod.rs:952 let mut child = Command::new("sh");
src/actions/mod.rs:954     .arg("-c")
```

Repo conventions:
- Command policy lives in `src/actions/mod.rs`.
- Existing command tests are in the same file around `command_policy_classifies_vcs_mutations_for_approval`.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Focused tests | `rtk cargo test --lib command_policy` | command policy tests pass |
| Full Rust gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |

## Scope

**In scope**:
- `src/actions/mod.rs`

**Out of scope**:
- Do not change `ApprovalMode::Yolo` semantics except through classification.
- Do not replace shell execution in this plan.
- Do not add a full shell parser.

## Git Workflow

- Branch: `advisor/004-tighten-command-classification`
- Commit message example: `fix(actions): require approval for mutating command forms`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add Failing Command Classification Tests

Add tests showing:
- `cargo fmt --check` is `Allow`.
- `cargo fmt` is `Approve`.
- `cargo clippy --fix` is `Approve`.
- `find . -delete` is `Approve` in normal serial command classification, not just parallel group policy.
- `find . -exec rm {} ;` or an equivalent shell-safe test string is `Approve` or `Deny`, but not `Allow`.
- Existing read-only commands such as `git status --short`, `rg "todo" src`, `pwd`, and `sed -n '1,10p' README.md` remain `Allow`.

**Verify**: `rtk cargo test --lib command_policy` -> new tests fail before implementation.

### Step 2: Replace Broad Allow Prefixes With Validators

Keep the current `has_shell_control_syntax` defense. Replace broad entries with command-specific helpers:
- `cargo fmt` allowed only when it includes `--check`.
- `cargo clippy` allowed unless it includes `--fix`.
- `find` should require approval unless the helper can prove the arguments are read-only. The conservative acceptable fix is to remove `find ` from the allowlist entirely.
- `cargo test`, `cargo check`, and `cargo build` may remain allowed as verification commands.

**Verify**: `rtk cargo test --lib command_policy` -> command policy tests pass.

### Step 3: Confirm Approval Behavior

Add or update tests for `decision_for_command`:
- In `ApprovalMode::Normal`, `cargo fmt` and `find . -delete` return `RequiresApproval`.
- In `ApprovalMode::Yolo`, those commands return `Allowed` only if classified as `Approve`, not `Allow`.

**Verify**: `rtk cargo test --lib approval` -> approval tests pass.

### Step 4: Run The Full Gate

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
```

**Verify**: exit 0; all tests pass.

## Test Plan

- Add command classifier unit tests in `src/actions/mod.rs`.
- Cover safe forms, mutating forms, and normal/yolo decision behavior.

## Done Criteria

- [ ] No known mutating `cargo fmt`, `cargo clippy --fix`, or `find` form is classified as `Allow`.
- [ ] Safe inspection commands still classify as `Allow`.
- [ ] Full Rust gate exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- A test or documented workflow depends on `cargo fmt` without `--check` being approval-free.
- Implementing a safe `find` parser grows beyond a small helper; choose approval instead.
- The fix requires changing runtime action contracts.

## Maintenance Notes

Every new allowlisted command must have tests for mutating flags. Prefer `Approve` over `Allow` when unsure.

