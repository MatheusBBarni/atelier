# Plan 006: Validate Codex Runtime Args Against Policy Bypass Flags

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- src/config/mod.rs src/runtime/codex.rs docs/codex-api/techspec.md`
> If any in-scope file changed since this plan was written, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

The Codex runtime is supposed to preserve harness-owned action policy while reusing Codex CLI authentication. Claude and Cursor runtime args already reject protected flags, but Codex runtime args are accepted and then spawned. That allows local config to alter sandbox, approval, model, config/profile, cwd, or output behavior before harness action policy sees any structured request.

## Current State

- Codex args are accepted without a validator:

```text
src/config/mod.rs:1316 RuntimeKind::Codex => RuntimeConfig {
src/config/mod.rs:1319     command: Some(runtime.command.unwrap_or_else(|| "codex".to_string())),
src/config/mod.rs:1320     args: runtime.args.unwrap_or_default(),
```

- Claude and Cursor have validators:

```text
src/config/mod.rs:1787 pub(crate) fn validate_claude_runtime_args(...)
src/config/mod.rs:1798 pub(crate) fn validate_cursor_runtime_args(...)
```

- Codex spawns configured args:

```text
src/runtime/codex.rs:171 let args = codex_step_args(&self.config.args, &request.agent_profile.model);
src/runtime/codex.rs:172 let mut child = Command::new(command)
src/runtime/codex.rs:173     .args(&args)
```

- Docs state the boundary:

```text
docs/codex-api/techspec.md:50 - Preserve harness-owned action policy for file edits and command execution.
docs/codex-api/techspec.md:104 codex exec --skip-git-repo-check --color never
```

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Config tests | `rtk cargo test --lib codex_runtime` | Codex config/runtime tests pass |
| Full Rust gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |

## Scope

**In scope**:
- `src/config/mod.rs`
- `src/runtime/codex.rs`
- `docs/codex-api/techspec.md` only if docs need clarification

**Out of scope**:
- Do not change Codex authentication behavior.
- Do not switch to `codex exec --json`; that is a separate direction item.
- Do not remove support for the documented compatibility args: `exec`, `--skip-git-repo-check`, `--color`, `never`.

## Git Workflow

- Branch: `advisor/006-validate-codex-runtime-args`
- Commit message example: `fix(config): reject protected codex runtime args`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Define Protected Codex Arg Categories

In `src/config/mod.rs`, add `validate_codex_runtime_args(runtime_id, args)`. It should reject flags that let config override policy-owned surfaces, including:
- sandbox/approval bypass flags,
- config/profile/root/cwd flags,
- model flags (`--model`, `-m`) because the agent profile owns model assignment,
- output protocol flags that would break the runtime parser,
- prompt/input flags that bypass stdin envelope ownership.

Allow the existing compatibility args documented in this repo: `exec`, `e`, `--skip-git-repo-check`, `--color`, and the `never` value after `--color`.

**Verify**: `rtk cargo test --lib codex_runtime` -> new tests fail before implementation.

### Step 2: Call The Validator During Config Merge

In the `RuntimeKind::Codex` branch of `ConfigBuilder::into_effective`, unwrap args into a local variable, validate them, and store the validated args.

Match the style of Claude/Cursor validation and error messages.

**Verify**: `rtk cargo test --lib codex_runtime` -> tests pass.

### Step 3: Add Runtime Arg Tests

Add tests for:
- default Codex runtime config remains valid,
- documented compatibility args remain valid,
- `--model`, `-m`, sandbox/approval bypass flags, config/profile flags, cwd flags, and output protocol flags are rejected,
- `codex_step_args` still appends `exec`, `--skip-git-repo-check`, `--color never`, and model when config did not own those fields.

**Verify**: `rtk cargo test --lib codex_step_args` -> tests pass.

### Step 4: Run The Full Gate

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
```

**Verify**: exit 0; all tests pass.

## Test Plan

- Config validation tests in `src/config/mod.rs`.
- Codex argument synthesis tests in `src/runtime/codex.rs`.
- Preserve existing availability and runtime tests.

## Done Criteria

- [ ] Codex runtime args reject protected policy/model/protocol flags.
- [ ] Existing documented Codex compatibility args remain valid.
- [ ] Full Rust gate exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- Current starter config uses a flag this plan would reject.
- Codex CLI installed locally has changed flag names enough that protected categories are unclear.
- The change requires switching runtime output parsing.

## Maintenance Notes

When adding support for `codex exec --json`, update this validator and tests deliberately instead of letting users opt into parser-changing flags through raw config args.

