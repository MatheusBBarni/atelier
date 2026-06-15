# Plan 008: Align Release Publishing With The Npm Distribution Contract

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- .github/workflows/release.yml docs/npm-distribution/techspec.md CLAUDE.md README.md`
> If any in-scope file changed since this plan was written, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

The npm distribution techspec says publishing happens only from stable semver tags and uses npm trusted publishing without an `NPM_TOKEN` fallback. The workflow currently also publishes from main-branch version bumps and injects `NPM_TOKEN`. Either the spec or the workflow must be authoritative; this plan chooses to implement the existing spec.

## Current State

Spec:

```text
docs/npm-distribution/techspec.md:85 - Release workflow runs only from stable semver tags for publishing.
docs/npm-distribution/techspec.md:88 - Publish job uses npm trusted publishing only. No `NPM_TOKEN` fallback.
```

Workflow:

```text
.github/workflows/release.yml:3 on:
.github/workflows/release.yml:4   push:
.github/workflows/release.yml:5     tags:
.github/workflows/release.yml:7     branches:
.github/workflows/release.yml:8       - main
.github/workflows/release.yml:151 # Auto-release triggered by a version bump on main.
.github/workflows/release.yml:367 NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
```

Repo conventions:
- Release workflow is the source of published npm packages and GitHub Release assets.
- The package version consistency check is enforced by `npm --prefix npm run check:versions`.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Package checks | `rtk npm --prefix npm test && rtk npm --prefix npm run check:targets && rtk npm --prefix npm run check:versions` | exit 0 |
| Rust tests | `rtk cargo test --locked` | exit 0 |
| Workflow grep | `rtk rg -n "NPM_TOKEN|branches:|should_release|detect-version-bump" .github/workflows/release.yml` | no obsolete auto-publish/token paths |

## Scope

**In scope**:
- `.github/workflows/release.yml`
- `docs/npm-distribution/techspec.md` only for clarifying notes, not changing the chosen contract
- `README.md` or `CLAUDE.md` only if release instructions need matching clarification

**Out of scope**:
- Do not publish packages.
- Do not create tags.
- Do not change package contents or versions.
- Do not add a token fallback.

## Git Workflow

- Branch: `advisor/008-align-release-contract`
- Commit message example: `ci(release): align publishing with tag-only trusted release`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Remove Main-Branch Auto-Publish Path

Refactor `.github/workflows/release.yml` so publishing paths run only for stable semver tag pushes. Remove or simplify `detect-version-bump` if it only exists for main auto-release.

Keep `workflow_dispatch` as dry-run only.

**Verify**: `rtk rg -n "Version changed|should_release|Auto-release|branches:" .github/workflows/release.yml` -> no main auto-publish logic remains.

### Step 2: Remove `NPM_TOKEN` Publishing

Remove the `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}` environment variable from the publish job. Keep `permissions: id-token: write` and `npm publish --provenance` for trusted publishing.

If the workflow needs an explicit npm trusted-publishing setup note, add it to docs rather than adding a token fallback.

**Verify**: `rtk rg -n "NPM_TOKEN|NODE_AUTH_TOKEN" .github/workflows/release.yml` -> no output.

### Step 3: Preserve Dry-Run And Verification Paths

Ensure `workflow_dispatch` still validates a provided version without publishing. Ensure tag pushes still run:
- version validation,
- Rust tests,
- native builds,
- npm package assembly,
- local install verification,
- publish,
- registry install verification,
- GitHub Release creation.

**Verify**: inspect the workflow dependency graph; no job should depend on a deleted job.

### Step 4: Run Local Checks

Run:

```bash
rtk npm --prefix npm test
rtk npm --prefix npm run check:targets
rtk npm --prefix npm run check:versions
rtk cargo test --locked
```

**Verify**: all exit 0.

## Test Plan

- This is CI YAML and release behavior; local tests are package/Rust checks plus text assertions.
- A reviewer should inspect workflow `needs` and `if` conditions carefully.

## Done Criteria

- [ ] Main branch pushes cannot publish npm packages.
- [ ] Publish job does not use `NPM_TOKEN` or `NODE_AUTH_TOKEN`.
- [ ] Tag pushes remain capable of publishing via trusted publishing.
- [ ] Workflow dispatch remains dry-run only.
- [ ] Local package/Rust checks exit 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- The repository is not configured for npm trusted publishing.
- Removing `detect-version-bump` breaks unrelated release validation.
- Maintainers decide main-branch auto-release is intentional; in that case update the techspec instead of this workflow.

## Maintenance Notes

Release workflow and `docs/npm-distribution/techspec.md` must stay in sync. If the team later chooses main auto-release, document that as a deliberate ADR/techspec update.

