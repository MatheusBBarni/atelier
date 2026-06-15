# Plan 009: Reconcile Verification Docs And Pull Request CI

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- README.md CLAUDE.md .github/workflows`
> If any in-scope file changed since this plan was written, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P2
- **Effort**: S-M
- **Risk**: LOW
- **Depends on**: `plans/008-align-release-publishing-contract.md`
- **Category**: dx
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

Contributors and agents need one clear verification contract. `README.md` currently documents only `cargo test` and `cargo build`; `CLAUDE.md` says a full fmt/clippy/test gate mirrors release CI; release CI only runs locked tests in its Rust job, while package and web gates live in separate workflow sections. This mismatch causes under-testing locally and surprises in CI.

## Current State

```text
README.md:289 Run unit/integration checks:
README.md:292 cargo test
README.md:295 Build locally:
README.md:298 cargo build
CLAUDE.md:19 Lint: `cargo clippy --all-targets` · Format: `cargo fmt --check`
CLAUDE.md:20 Full pre-commit gate (mirrors CI...): `cargo fmt --check && cargo clippy --all-targets && cargo test --locked`
.github/workflows/release.yml:182 - run: cargo test --locked
.github/workflows/web-checks.yml:67 run: npx astro build
.github/workflows/web-checks.yml:73 run: npm run check:surfaces
```

Repo conventions:
- Commands in repo guidance should use `rtk`.
- Rust, npm package, and web docs are distinct verification surfaces.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Rust full gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |
| npm package gate | `rtk npm --prefix npm test && rtk npm --prefix npm run check:targets && rtk npm --prefix npm run check:versions` | exit 0 |
| web gate | `rtk npm --prefix web test && rtk npm --prefix web run build && rtk npm --prefix web run check:surfaces` | exit 0 |

## Scope

**In scope**:
- `README.md`
- `CLAUDE.md`
- `.github/workflows/ci.yml` or equivalent PR workflow
- Existing workflow docs/comments if needed

**Out of scope**:
- Do not rewrite release publishing logic here; that belongs to Plan 008.
- Do not change source behavior.
- Do not add slow live-runtime tests to default CI.

## Git Workflow

- Branch: `advisor/009-verification-docs-ci`
- Commit message example: `ci: add pull request verification gate`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add A Pull Request CI Workflow

Create `.github/workflows/ci.yml` for pull requests and main pushes. Include jobs for:
- Rust: `cargo fmt --check`, `cargo clippy --all-targets`, `cargo test --locked`.
- npm package: `npm ci --prefix npm --ignore-scripts --force`, `npm --prefix npm test`, `npm --prefix npm run check:targets`, `npm --prefix npm run check:versions`.
- Web: install `web`, generate/build as needed, run `npm --prefix web test`, `npm --prefix web run build`, `npm --prefix web run check:surfaces`.

Use caching consistent with existing workflows.

**Verify**: `rtk rg -n "cargo fmt --check|cargo clippy --all-targets|cargo test --locked|check:targets|check:versions|check:surfaces" .github/workflows/ci.yml` -> all commands present.

### Step 2: Update README Verification Matrix

Replace the minimal Development section with a concise matrix:
- Rust full gate,
- npm package gate,
- web docs gate,
- ignored live-runtime tests and required env vars.

Use `rtk` prefixes in examples.

**Verify**: `rtk rg -n "cargo fmt --check|npm --prefix npm|npm --prefix web" README.md` -> matrix commands present.

### Step 3: Fix CLAUDE.md Wording

Update `CLAUDE.md` so it no longer claims release CI alone mirrors the full pre-commit gate unless the new PR workflow now does. Keep exact test notes about ignored live-runtime tests.

**Verify**: `rtk rg -n "mirrors CI|ci.yml|cargo fmt --check" CLAUDE.md` -> wording accurately points to the current workflow.

### Step 4: Run Local Gates

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
rtk npm --prefix npm test && rtk npm --prefix npm run check:targets && rtk npm --prefix npm run check:versions
rtk npm --prefix web test && rtk npm --prefix web run build && rtk npm --prefix web run check:surfaces
```

**Verify**: all commands exit 0.

## Test Plan

- No app tests are added.
- CI workflow text plus local gates are the verification.

## Done Criteria

- [ ] PR CI runs the documented Rust/package/web gates.
- [ ] README and CLAUDE agree on local versus CI gates.
- [ ] Live-runtime ignored tests remain documented as opt-in.
- [ ] All local gates exit 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- Web build requires generated files that cannot be produced in CI with existing scripts.
- The new workflow would duplicate release publishing jobs.
- Package checks require native release artifacts unavailable in PR CI.

## Maintenance Notes

When adding a new package surface, update both `README.md` and PR CI in the same change.

