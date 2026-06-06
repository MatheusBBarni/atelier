# Technical Specification: npm Distribution

Status: Draft
Date: 2026-06-05

## Executive Summary

This specification defines how `atelier`, the Rust binary in this repository,
will be distributed through npm as `@matheusbbarni/atelier`. The npm package is
a thin global-install wrapper around prebuilt native binaries. Release
automation builds six native artifacts in GitHub Actions, verifies npm tarballs
before publishing, publishes public platform packages first, publishes the
top-level package last, verifies the registry install, and only then creates the
GitHub Release.

## Background / Context

The existing project is a Rust application crate with:

- `Cargo.toml` package name `multiagent`
- `Cargo.toml` version `0.1.0`
- binary name `atelier`
- `src/cli.rs` command name `atelier`
- no npm package files
- no GitHub Actions workflow files

The PRD in `docs/npm-distribution/prd.md` selects npm distribution as the first
user-facing package channel. Cargo source install remains a developer path.

Current implementation gap: `atelier --version` does not work yet. Clap reports
`--version` as an unexpected argument. This must be fixed because release
verification uses binary version output as a hard gate.

## Goals

- Publish `@matheusbbarni/atelier` as the npm package users install globally.
- Ship prebuilt native binaries through public platform-specific optional npm
  packages.
- Keep the Rust crate and `atelier` binary as the source of truth for CLI
  behavior.
- Support six v1 targets:
  - `darwin-arm64`
  - `darwin-x64`
  - `linux-arm64` glibc
  - `linux-x64` glibc
  - `win32-arm64`
  - `win32-x64`
- Use GitHub Actions as the only public release authority.
- Publish npm packages with trusted publishing/OIDC and provenance.
- Create the GitHub Release only after npm publishing and registry install
  verification succeed.

## Non-Goals

- Building Rust from source during npm install.
- Supporting `npm exec` or `npx` as v1 user flows.
- Homebrew distribution.
- Cargo registry distribution.
- Renaming the Rust package from `multiagent` to `atelier`.
- Code signing, notarization, or GitHub artifact attestations.
- musl/Alpine Linux packages.
- Prerelease npm dist-tags.
- Automated npm deprecation of failed versions.

## Requirements

### Functional Requirements

- `npm install -g @matheusbbarni/atelier` installs an `atelier` command.
- The top-level npm package contains no native binary.
- The top-level npm package resolves and executes exactly one matching platform
  package.
- Platform packages contain a single native binary for their target.
- Unsupported platforms fail with a clear message.
- Installs that omit optional dependencies fail with a clear message naming the
  expected package and reinstall command.
- `ATELIER_BINARY_PATH` bypasses platform package resolution for tests and
  recovery.
- `atelier --version` prints the Cargo package version.
- npm package versions, Cargo version, binary version, and release tag must
  match before publishing.

### Operational Requirements

- Release workflow runs only from stable semver tags for publishing.
- `workflow_dispatch` is dry-run only in v1.
- Publish job uses a protected `release` environment.
- Publish job uses npm trusted publishing only. No `NPM_TOKEN` fallback.
- All six platform artifacts must be present before any npm publish command.
- Platform packages publish before the top-level package.
- GitHub Release is created after post-publish registry install verification.
- Failed publish or failed post-publish install requires patch-forward release.

## Proposed Design

### Repository Layout

Add an npm packaging workspace under `npm/`:

```text
npm/
  package.json
  package-lock.json
  package/
    package.json
    README.md
    bin/
      atelier.js
    lib/
      launcher.cjs
      targets.cjs
  platform/
    darwin-arm64/
      package.json
      README.md
    darwin-x64/
      package.json
      README.md
    linux-arm64/
      package.json
      README.md
    linux-x64/
      package.json
      README.md
    win32-arm64/
      package.json
      README.md
    win32-x64/
      package.json
      README.md
  scripts/
    assemble.mjs
    check-metadata.mjs
    check-targets.mjs
    check-versions.mjs
    checksum.mjs
    pack.mjs
    sync-versions.mjs
    targets.mjs
    verify-installed.mjs
  tests/
    launcher.test.mjs
    metadata.test.mjs
    versions.test.mjs
```

Add or update:

```text
.github/workflows/release.yml
LICENSE
.gitignore
README.md
src/cli.rs
```

Committed source package folders are templates. Generated package trees,
binaries, tarballs, and checksums are written under `target/npm-dist/`.

### Package Topology

Top-level package:

```text
@matheusbbarni/atelier
```

Platform packages:

```text
@matheusbbarni/atelier-darwin-arm64
@matheusbbarni/atelier-darwin-x64
@matheusbbarni/atelier-linux-arm64
@matheusbbarni/atelier-linux-x64
@matheusbbarni/atelier-win32-arm64
@matheusbbarni/atelier-win32-x64
```

The top-level package has exact-version optional dependencies on every platform
package. Platform packages are public implementation packages and expose no
`bin` entry.

### Binary Archives

Native build jobs create final release archives before uploading artifacts:

```text
atelier-v<VERSION>-darwin-arm64.tar.gz
atelier-v<VERSION>-darwin-x64.tar.gz
atelier-v<VERSION>-linux-arm64.tar.gz
atelier-v<VERSION>-linux-x64.tar.gz
atelier-v<VERSION>-win32-arm64.zip
atelier-v<VERSION>-win32-x64.zip
```

Unix archives contain:

```text
atelier
README.md
LICENSE
```

Windows archives contain:

```text
atelier.exe
README.md
LICENSE
```

The npm assembly script extracts only the executable into the platform package
`bin/` directory and copies the package README and root `LICENSE`.

## Architecture / Components

### Rust CLI

Update `src/cli.rs` so Clap exposes version metadata:

```rust
#[command(
    name = "atelier",
    version = env!("CARGO_PKG_VERSION"),
    about = "Terminal-native agent orchestration harness"
)]
```

Add a CLI test using `assert_cmd` that runs `atelier --version` and checks that
stdout contains `atelier` plus `env!("CARGO_PKG_VERSION")`.

Do not rename `Cargo.toml [package].name`; npm distribution is scoped to the
`atelier` binary.

### npm Workspace

`npm/package.json` is a private workspace root:

```json
{
  "private": true,
  "workspaces": ["package", "platform/*"],
  "scripts": {
    "test": "node --test tests/*.test.mjs",
    "sync:versions": "node scripts/sync-versions.mjs",
    "check:versions": "node scripts/check-versions.mjs",
    "check:targets": "node scripts/check-targets.mjs",
    "check:metadata": "node scripts/check-metadata.mjs",
    "assemble": "node scripts/assemble.mjs",
    "pack": "node scripts/pack.mjs",
    "checksum": "node scripts/checksum.mjs",
    "verify:installed": "node scripts/verify-installed.mjs",
    "dry-run:local": "node scripts/dry-run-local.mjs"
  }
}
```

Use Node `>=20` and npm `>=10` for package development, testing, release, and
npm-installed runtime.

### Launcher

`npm/package/bin/atelier.js` is a thin executable with a shebang that calls
`main()` from `npm/package/lib/launcher.cjs`.

`launcher.cjs`:

- reads `process.platform` and `process.arch`
- maps to a supported target key
- honors `ATELIER_BINARY_PATH`
- handles standalone `--update` before native binary resolution
- runs `npm install --global --include=optional --ignore-scripts --no-audit
  --no-fund @matheusbbarni/atelier@latest` for updates, using the current npm
  install prefix when it can be inferred from the package path
- resolves the matching optional dependency binary with `require.resolve`
- spawns the native binary with inherited stdio
- passes argv, env, and cwd unchanged
- exits with the child status code
- exits `1` on resolution or spawn failure

Use `child_process.spawn` rather than `execFileSync` so the interactive TUI can
stream stdio and handle terminal behavior.

Unsupported target error includes:

- detected `platform`
- detected `arch`
- supported target keys
- link or note pointing to GitHub Releases/source builds

Missing optional dependency error includes:

- target key
- expected package name
- note that optional dependencies may be disabled
- reinstall command:

```sh
npm install -g @matheusbbarni/atelier --include=optional
```

### Target Table

`npm/scripts/targets.mjs` is the source of truth for package scripts:

```js
[
  { key: "darwin-arm64", os: "darwin", cpu: "arm64", exe: "atelier", archive: "tar.gz" },
  { key: "darwin-x64", os: "darwin", cpu: "x64", exe: "atelier", archive: "tar.gz" },
  { key: "linux-arm64", os: "linux", cpu: "arm64", libc: "glibc", exe: "atelier", archive: "tar.gz" },
  { key: "linux-x64", os: "linux", cpu: "x64", libc: "glibc", exe: "atelier", archive: "tar.gz" },
  { key: "win32-arm64", os: "win32", cpu: "arm64", exe: "atelier.exe", archive: "zip" },
  { key: "win32-x64", os: "win32", cpu: "x64", exe: "atelier.exe", archive: "zip" }
]
```

`npm/package/lib/targets.cjs` is a committed deterministic runtime map generated
from this table. `check:targets` fails if the committed map is stale.

## Data Model and Contracts

### Top-Level Package Manifest

`npm/package/package.json`:

```json
{
  "name": "@matheusbbarni/atelier",
  "version": "0.1.0",
  "license": "MIT",
  "bin": {
    "atelier": "bin/atelier.js"
  },
  "engines": {
    "node": ">=20"
  },
  "files": ["bin/", "lib/", "README.md", "LICENSE"],
  "optionalDependencies": {
    "@matheusbbarni/atelier-darwin-arm64": "0.1.0",
    "@matheusbbarni/atelier-darwin-x64": "0.1.0",
    "@matheusbbarni/atelier-linux-arm64": "0.1.0",
    "@matheusbbarni/atelier-linux-x64": "0.1.0",
    "@matheusbbarni/atelier-win32-arm64": "0.1.0",
    "@matheusbbarni/atelier-win32-x64": "0.1.0"
  }
}
```

Also include `repository`, `homepage`, `bugs`, and `description`.

### Platform Package Manifest

Linux x64 example:

```json
{
  "name": "@matheusbbarni/atelier-linux-x64",
  "version": "0.1.0",
  "description": "Linux x64 native binary for Atelier",
  "license": "MIT",
  "os": ["linux"],
  "cpu": ["x64"],
  "libc": ["glibc"],
  "files": ["bin/", "README.md", "LICENSE"]
}
```

macOS and Windows packages use matching `os` and `cpu` fields. Platform packages
do not define `bin`.

### Pack Manifest

`npm/scripts/pack.mjs` writes:

```text
target/npm-dist/npm-packages.json
```

The file records `npm pack --json` output for each package:

```json
[
  {
    "package": "@matheusbbarni/atelier-linux-x64",
    "version": "0.1.0",
    "filename": "matheusbbarni-atelier-linux-x64-0.1.0.tgz",
    "shasum": "...",
    "integrity": "..."
  }
]
```

Publishing, checksum generation, and GitHub Release upload use this manifest
instead of guessing tarball names.

## APIs / Events

No runtime API or harness event contract changes are required.

New command/script API:

```sh
npm --prefix npm test
npm --prefix npm run sync:versions
npm --prefix npm run check:versions
npm --prefix npm run check:targets
npm --prefix npm run check:metadata
npm --prefix npm run assemble
npm --prefix npm run pack
npm --prefix npm run checksum
npm --prefix npm run verify:installed
npm --prefix npm run dry-run:local
```

Script inputs:

```text
ATELIER_VERSION
ATELIER_RELEASE_ARCHIVES_DIR
ATELIER_NPM_DIST_DIR
ATELIER_ALLOW_MISSING_TARGETS
ATELIER_BINARY_PATH
```

Default paths:

```text
ATELIER_RELEASE_ARCHIVES_DIR=target/npm-dist/archives
ATELIER_NPM_DIST_DIR=target/npm-dist
```

## Security and Privacy

- Do not use `NPM_TOKEN` in v1.
- Configure npm trusted publishing for all seven packages.
- Publish only from stable semver tag workflows.
- Use a protected `release` environment with reviewer approval before publish.
- Workflow permissions:

```yaml
permissions:
  contents: write
  id-token: write
```

- The publish job runs `npm publish <tarball> --access public`.
- The workflow publishes verified tarballs, never live package directories.
- The installer and launcher do not read or write `multiagent.toml`.
- The installer does not probe provider credentials.
- `ATELIER_BINARY_PATH` is a local process escape hatch. It must not be
  persisted in config or documented as normal user configuration.
- Add root `LICENSE` because `Cargo.toml` declares `license = "MIT"` but no
  root license file currently exists.

V1 supply-chain artifacts:

- npm provenance from trusted publishing
- `SHA256SUMS` for native archives and npm tarballs

Out of scope:

- signing
- notarization
- GitHub artifact attestations
- automated npm deprecation

## Performance and Reliability

- Native npm install should not compile Rust or run heavyweight postinstall
  scripts.
- Launcher overhead is one Node process plus one native child process. This is
  acceptable for v1 global npm install.
- Release workflow uses native runner builds to avoid cross-compilation
  complexity.
- Workflow concurrency:

```yaml
concurrency:
  group: release-${{ github.ref_name }}
  cancel-in-progress: false
```

Dry-run workflows use a separate version-keyed group and may be cancelled
manually.

## Observability

Release workflow logs must include:

- resolved version
- Cargo version
- npm package versions
- runner target key
- archive filename
- npm tarball filename
- checksum manifest path
- package publish order
- `npm view` results after publish
- install verification command output summaries

Do not print secrets. Trusted publishing should avoid npm token exposure.

## Migration and Rollout

### Phase 1: Local Packaging Foundation

- Add `atelier --version`.
- Add root `LICENSE`.
- Add npm workspace, package templates, launcher, target table, scripts, tests,
  and lockfile.
- Add `.gitignore` entries:

```text
target/npm-dist/
npm/**/*.tgz
npm/platform/*/bin/
SHA256SUMS
```

- Update README with npm install path.
- Add ADR `docs/adr/0003-npm-distribution-uses-platform-packages.md`.
- Implement `npm --prefix npm run dry-run:local`.

### Phase 2: CI Dry Run

- Add release workflow with `workflow_dispatch` dry-run.
- Build native binaries on six runners.
- Upload final archives only.
- Download archives in packaging job.
- Assemble package directories under `target/npm-dist`.
- Run metadata checks, `npm pack`, checksums, and local tarball install
  verification.
- Do not publish or create GitHub Releases in dry-run.

### Phase 3: Public Release Path

- Configure npm trusted publishers for all seven packages.
- Protect stable release tags and `release` environment.
- On stable semver tag push:
  - build all artifacts
  - verify all packages
  - publish platform packages first
  - publish top-level package last
  - run `npm view` checks
  - install published package from registry on Linux x64
  - create GitHub Release with assets and generated notes

## GitHub Workflow Design

Workflow triggers:

```yaml
on:
  push:
    tags:
      - "v[0-9]+.[0-9]+.[0-9]+"
  workflow_dispatch:
    inputs:
      version:
        required: true
      publish:
        default: false
```

V1 rule: `workflow_dispatch` is dry-run only. The `publish` input must be
ignored or rejected unless the workflow is later revised.

Job graph:

```text
validate-release
rust-tests
build-native[6 targets]
package-npm
verify-local-install[macOS, Linux, Windows native where possible]
publish-npm
verify-registry-install
create-github-release
```

Target matrix:

```text
darwin-arm64  runs-on: macos-15
darwin-x64    runs-on: macos-15-intel
linux-arm64   runs-on: ubuntu-24.04-arm
linux-x64     runs-on: ubuntu-24.04
win32-arm64   runs-on: windows-11-arm
win32-x64     runs-on: windows-2025
```

If a runner label changes, update to the current equivalent GitHub-hosted
runner label before implementation.

Build commands:

```sh
cargo build --locked --release --bin atelier
```

General Rust gate:

```sh
cargo test --locked
```

Native smoke commands:

```sh
target/release/atelier --version
target/release/atelier --help
target/release/atelier --doctor --json
```

Windows uses `target\release\atelier.exe`.

GitHub Release command:

```sh
gh release create "$TAG" \
  target/npm-dist/archives/* \
  target/npm-dist/*.tgz \
  target/npm-dist/SHA256SUMS \
  --verify-tag \
  --generate-notes
```

## Testing Strategy

### Rust Tests

- Add `atelier --version` CLI test.
- Existing `cargo test --locked` remains the general Rust gate.

### Node Tests

Use Node's built-in test runner:

```sh
npm --prefix npm test
```

Cover:

- target key mapping
- unsupported platform errors
- missing optional dependency errors
- `ATELIER_BINARY_PATH`
- argv/env/cwd pass-through
- child exit code pass-through
- package manifest fields
- version sync/check behavior
- target map drift detection

### Package Verification

`check:metadata` validates assembled package directories and
`npm pack --dry-run --json` output:

- top-level package contains `package.json`, `README.md`, `LICENSE`,
  `bin/atelier.js`, and `lib/`
- top-level package excludes native binaries
- platform package contains `package.json`, `README.md`, `LICENSE`, and one
  binary under `bin/`
- Unix platform binary has executable bits
- Windows platform binary is named `atelier.exe`
- package `files` allowlists are narrow

### Install Verification

`verify-installed.mjs`:

- creates a temporary directory
- creates a temporary npm global prefix
- installs the top-level tarball or registry package globally
- runs:

```sh
atelier --version
atelier --help
MULTIAGENT_CONFIG=<tmp>/multiagent.toml atelier --doctor --json
```

Doctor verification requires:

- exit code `0`
- parseable JSON
- expected top-level report shape

Runtime availability may be `unavailable` or `unknown`.

## Alternatives Considered

- Build Rust from source during npm install: rejected because it requires local
  toolchains and makes install slow and brittle.
- Cross-compile from one runner: rejected for v1 because native runners give
  clearer compatibility and smoke-test signals.
- Publish top-level package before platform packages: rejected because users
  could install before optional dependencies exist.
- Use `NPM_TOKEN`: rejected because trusted publishing avoids long-lived
  registry secrets.
- Use Jest or Vitest: rejected because Node's built-in test runner is enough for
  packaging scripts.
- Rename the Cargo package to `atelier`: rejected for this scope because the npm
  release contract only needs the binary name.
- Generate a complete workflow from a release framework: rejected for v1 in
  favor of explicit GitHub Actions jobs.

## Risks and Mitigations

- Risk: npm trusted publishing is not configured for every package.
  Mitigation: publish job fails with no token fallback; setup checklist requires
  all seven trusted publisher entries.
- Risk: npm publish succeeds partly and then fails.
  Mitigation: publish platform packages first, top-level last, skip GitHub
  Release on failure, patch forward with a new version.
- Risk: optional dependencies are omitted by user config.
  Mitigation: launcher prints expected package and reinstall command.
- Risk: runner labels change.
  Mitigation: keep matrix labels explicit and verify against current GitHub
  runner docs during implementation.
- Risk: tarballs include extra files.
  Mitigation: narrow `files` allowlists plus dry-run pack validation.
- Risk: `--doctor --json` fails due missing provider credentials.
  Mitigation: verify JSON shape and process health only; provider readiness is
  not a release blocker.
- Risk: root license file is missing.
  Mitigation: add `LICENSE` before npm package assembly can pass.

## Open Questions

- Should the Cargo package eventually be renamed from `multiagent` to `atelier`
  for Cargo registry distribution? Out of scope for this npm release.
- Should future releases add musl/Alpine platform packages?
- Should future releases add artifact attestations, code signing, and
  notarization?
- Should future releases support prerelease npm dist-tags?

## Acceptance Criteria

- `docs/adr/0003-npm-distribution-uses-platform-packages.md` records the
  architecture decision.
- `atelier --version` works and prints the Cargo package version.
- `npm --prefix npm test` passes.
- `npm --prefix npm run check:versions` passes.
- `npm --prefix npm run check:targets` passes.
- `npm --prefix npm run check:metadata` passes after assembly.
- `npm --prefix npm run dry-run:local` installs a local tarball and verifies the
  installed command.
- `workflow_dispatch` dry-run builds all six native archives and assembles npm
  tarballs without publishing.
- Stable tag release publishes all six platform packages, then the top-level
  package.
- Post-publish registry install verification passes before GitHub Release
  creation.
- GitHub Release contains native archives, npm tarballs, `SHA256SUMS`, and
  generated release notes.
- README documents npm global install as the primary user install path.

## Reference Docs

- npm `package.json`: https://docs.npmjs.com/files/package.json/
- npm trusted publishing: https://docs.npmjs.com/trusted-publishers/
- npm provenance: https://docs.npmjs.com/generating-provenance-statements
- GitHub-hosted runners: https://docs.github.com/actions/reference/runners/github-hosted-runners
- `gh release create`: https://cli.github.com/manual/gh_release_create
