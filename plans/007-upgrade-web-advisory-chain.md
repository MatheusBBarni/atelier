# Plan 007: Clear High-Severity Web Dependency Advisories

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- web/package.json web/package-lock.json web/scripts web/tests`
> If any in-scope file changed since this plan was written, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: migration
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

`npm --prefix web audit --audit-level=high` currently reports high-severity advisories through the docs site's Astro/Vite/esbuild chain. The docs site is not the Rust harness runtime, but it is part of the published project surface and CI builds it. The dependency chain should be moved to non-advisory versions without breaking generated docs, surfaces, or links.

## Current State

```text
web/package.json:18 "astro": "^6.3.1"
web/package-lock.json:1724 "node_modules/astro": {
web/package-lock.json:1725   "version": "6.4.4",
web/package-lock.json:1748   "esbuild": "^0.27.3",
web/package-lock.json:2220 "version": "0.27.7",
web/package-lock.json:4689 "version": "7.3.5",
web/package-lock.json:4694 "esbuild": "^0.27.0",
```

Audit evidence from 2026-06-14:
- `rtk npm --prefix web audit --audit-level=high` exited 1 with 3 high-severity vulnerabilities.
- `rtk npm --prefix npm audit --audit-level=high` exited 0.

Repo conventions:
- Web package commands live in `web/package.json`.
- Web CI builds generated docs and runs surface checks.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Audit | `rtk npm --prefix web audit --audit-level=high` | exit 0 |
| Web tests | `rtk npm --prefix web test` | exit 0 |
| Web build | `rtk npm --prefix web run build` | exit 0 |
| Surface checks | `rtk npm --prefix web run check:surfaces` | exit 0 |

## Scope

**In scope**:
- `web/package.json`
- `web/package-lock.json`
- Minimal web test or script updates required by the dependency upgrade

**Out of scope**:
- Do not change Rust source or generated docs content by hand.
- Do not downgrade Astro.
- Do not change the npm distribution package under `npm/`.

## Git Workflow

- Branch: `advisor/007-clear-web-advisories`
- Commit message example: `chore(web): clear high severity dependency advisories`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Refresh Advisory State

Run:

```bash
rtk npm --prefix web audit --audit-level=high
```

Record which package chain remains vulnerable. Do not rely only on the advisory list from this plan if npm has changed.

**Verify**: command still reports the advisory chain, or exits 0. If it exits 0 with no package changes needed, stop and report that the finding is already fixed.

### Step 2: Update The Web Dependency Chain

Use npm in the `web/` package to update Astro and its lockfile-resolved Vite/esbuild chain to non-advisory versions. Prefer the smallest compatible upgrade that clears `npm audit`. If a major Astro upgrade is required, read the current Astro migration notes before changing code.

Expected files to change are `web/package.json` and `web/package-lock.json`.

**Verify**: `rtk npm --prefix web audit --audit-level=high` -> exit 0.

### Step 3: Run Web Verification

Run:

```bash
rtk npm --prefix web test
rtk npm --prefix web run build
rtk npm --prefix web run check:surfaces
```

If `check:links` requires a built `web/dist`, run it after build:

```bash
rtk npm --prefix web run check:links
```

**Verify**: all commands exit 0.

### Step 4: Check Rust Is Unaffected

Run:

```bash
rtk cargo test --locked
```

**Verify**: exit 0.

## Test Plan

- No new tests are expected unless the dependency upgrade changes web behavior.
- Existing web tests, build, surface checks, and high-level Rust tests are the regression suite.

## Done Criteria

- [ ] `rtk npm --prefix web audit --audit-level=high` exits 0.
- [ ] Web tests/build/surface checks exit 0.
- [ ] `web/package-lock.json` no longer resolves the vulnerable chain reported at start.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- Clearing advisories requires a major framework migration with route/content behavior changes.
- The web build changes generated public surfaces in a way not covered by existing tests.
- npm suggests a downgrade to an older Astro major.

## Maintenance Notes

Keep `npm audit --audit-level=high` in the web verification checklist until CI covers it. Reviewers should inspect generated route/surface outputs after framework upgrades.

