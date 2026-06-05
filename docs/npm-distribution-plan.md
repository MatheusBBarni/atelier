# npm Distribution Plan (Execution Plan)

## Objective
Ship `atelier` as `npm install -g atelier` with a small Node launcher that selects and runs the correct Rust binary for the host platform.

## Current decision (locking the rollout)
- Package name: `atelier`
- Entry point: global binary script `atelier`
- Runtime architecture: Node (script) + prebuilt Rust binaries shipped inside the npm package
- Single source of truth for version: `Cargo.toml` and `package.json` must always match
- Release process is **tag -> release workflow -> npm publish -> smoke verification**

## Platform and artifact contract
- Minimum supported OS/arch targets
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
  - `x86_64-pc-windows-msvc`
- Binary filenames (in published package)
  - `npm/binaries/atelier-linux-x64`
  - `npm/binaries/atelier-linux-arm64`
  - `npm/binaries/atelier-darwin-x64`
  - `npm/binaries/atelier-darwin-arm64`
  - `npm/binaries/atelier-win32-x64.exe`
- Launcher mapping (Node):
  - `platform=== 'darwin' && arch==='x64'` -> `atelier-darwin-x64`
  - `platform=== 'darwin' && arch==='arm64'` -> `atelier-darwin-arm64`
  - `platform=== 'linux' && arch==='x64'` -> `atelier-linux-x64`
  - `platform=== 'linux' && arch==='arm64'` -> `atelier-linux-arm64`
  - `platform=== 'win32' && arch==='x64'` -> `atelier-win32-x64.exe`
- Unsupported combos fail fast with a message that includes: OS, arch, supported list, and remediation link.

## Phase 0 — Readiness lock
Goal: lock scope before changes.

1. Confirm artifact naming and launcher map with release maintainers.
2. Define exact publish branch strategy and release owner.
3. Capture required secrets and environment requirements:
   - `id-token: write` for OIDC
   - `NPM_TOKEN` only if using PAT fallback
   - `GITHUB_TOKEN` for releases/assets
4. Document acceptance gate for merge:
   - no unsupported platform in `docs/npm-distribution-plan.md`
   - package metadata and script contract agree on target names

Acceptance criteria:
- Decision lock recorded in this document.
- Owners assigned per phase.

## Phase 1 — Package surface and launcher (Tooling owner)
**Estimate:** 1–2 days

### 1.1 Node package initialization
- Ensure top-level `package.json` exists with fields:
  - `name`, `version`, `bin`, `files`, `scripts`, `os`, `cpu` (if needed)
  - `files` whitelist includes launcher and all binaries
- Add launcher entry under `bin/atelier` and make it executable in repo metadata.

### 1.2 Launcher implementation
Implement `bin/atelier` with behavior:
1. Determine `process.platform` + `process.arch`.
2. Resolve path to matching binary from package `files`.
3. Validate binary exists and is executable.
4. Re-map argv passthrough to selected binary.
5. On unsupported host print explicit guidance:
   - exact tuple
   - supported tuples
   - command to file issue with tuple

### 1.3 Runtime install handling
- Add `prepare` and/or `postinstall` script to normalize executable permission where required:
  - Linux/macOS: `chmod +x` on selected binary
  - Windows: keep `.exe` executable metadata intact
- Ensure launcher works for both:
  - global install (`npm i -g`)
  - local extraction (`npm pack` + install tarball in CI)

### 1.4 Verification
- `node bin/atelier --help` fails only with a clear unsupported-platform error unless matching binary exists.

Acceptance criteria:
- Running launcher on supported host with matching binary returns non-error help/version.
- Unsupported tuple returns expected explicit message.

## Phase 2 — Build + packaging layout (Release owner)
**Estimate:** 2–3 days

### 2.1 Build matrix contract
Create release build jobs for each target triple. For each matrix entry produce:
- binary artifact from existing Rust pipeline
- checksum file `atelier-<version>-<target>.sha256`
- packageable platform file path matching launcher map

### 2.2 Package assembly
- Store platform binaries in `npm/binaries/` in the workspace tree.
- Exclude heavy dev/test assets from npm artifact by `package.json` `files`.
- Add `scripts/smoke-prep.sh` (or equivalent npm script) to validate package structure.

### 2.3 Release artifact consistency checks
- Add lightweight script in CI to ensure:
  - all 5 binaries exist
  - names exactly match launcher map
  - checksum file exists for each binary

Acceptance criteria:
- Every required target has binary + checksum.
- Assembly script exits non-zero on naming drift.

## Phase 3 — CI release workflow (CI owner)
**Estimate:** 2–4 days

### 3.1 Workflow file
Create `.github/workflows/release.yml` with:
1. Trigger: `workflow_dispatch` and release tags (`vX.Y.Z`)
2. Build matrix across declared targets
3. Rust compile + artifact upload per job
4. Aggregate step to create GH release and publish checksums
5. Version validation step (`package.json` = `Cargo.toml`)
6. Publish npm package in the same run after release asset verification

### 3.2 Security and trust
- Use `actions/checkout`, `actions/setup-node`, `dtolnay/rust-toolchain` or equivalent.
- Use OIDC for npm auth where possible.
- Keep checksums in release assets and include reproducibility notes in release body.

### 3.3 Failure behavior
- If build matrix fails: do not publish release assets.
- If checksum verification fails: stop before GH release and npm publish.
- If publish fails: leave release in failed state and notify in workflow logs.

Acceptance criteria:
- Tagged workflow run either completes end-to-end or fails before publish.
- Release assets include all binaries and checksum files.

## Phase 4 — Version governance and release process (Release owner)
**Estimate:** 1 day

### 4.1 Version sync
- Add release helper to update both:
  - `package.json.version`
  - `Cargo.toml` package version
- Enforce in CI that versions are equal before publish.

### 4.2 Release sequence
1. Prepare commit on `main` or release branch.
2. Bump versions and changelog entry.
3. Tag `vX.Y.Z`.
4. Run release workflow.
5. Verify npm package installs and returns correct version.

Acceptance criteria:
- Post-release version equality check passes.
- `atelier --version` output matches tag version.

## Phase 5 — Smoke and quality gates (QA owner)
**Estimate:** 1 day recurring in CI

### 5.1 Add script
- `scripts/ci-smoke` command:
  - builds/uses npm tarball
  - installs package locally or via registry
  - runs `atelier --version` and `atelier --help`
- Add OS matrix smoke job where supported runners exist.

### 5.2 Negative path checks
- Validate unsupported tuple message format.
- Validate exit code for missing binary.

Acceptance criteria:
- Smoke pass on all available runners in CI.
- Error cases are deterministic and documented.

## Phase 6 — Docs and rollout (Docs owner)
- Update README install section with:
  - `npm install -g atelier`
  - verify and uninstall commands
  - supported platform matrix
  - troubleshooting checklist
- Add known limitations section:
  - unsupported OS/arch
  - corporate proxy / offline mirrors / checksum validation tips

Acceptance criteria:
- `README` contains exact install path and first command users execute.

## Milestones
1. M1: Phase 1 + 2 complete on branch.
2. M2: First successful release workflow dry run.
3. M3: End-to-end publish to npm + verification of global install.
4. M4: `ci-smoke` green across supported runners.
5. M5: Docs published and rollout complete.

## Dependencies and sequencing
- Phase 0 before all execution.
- Phase 3 cannot start without Phase 2 assembly path.
- Phase 4 depends on version sync from Phase 2.
- Milestone C requires passing Phase 5.

## Rollback
- If a release is published with a broken binary:
  - unpublish within npm window when possible
  - otherwise publish patch release and deprecate broken version
  - keep release note with corrective version and mitigation
- Preserve last known good tag and checksum manifest for support.

## Definition of done
- `npm install -g atelier` works on all supported OS/arch without manual binary selection.
- Version is synchronized between `Cargo.toml` and `package.json`.
- Release workflow is reproducible with checksums and smoke gates.
- Documentation and troubleshooting path are complete.
