# npm Distribution PRD

## Problem Statement

Developers should be able to install the `atelier` global command through the
npm registry, the same way modern Rust-backed agent CLIs such as Codex expose a
small npm package that installs a native executable.

The current project can be installed from source with Cargo, but that path
requires a Rust toolchain and a source checkout. For normal users, the npm
install path should provide a prebuilt `atelier` binary, work from any working
directory, and preserve the existing harness behavior around home configuration,
local configuration, runtime availability, and session history.

## Proposed Direction

Publish a scoped npm distribution package named `@matheusbbarni/atelier` that
exposes the `atelier` command and resolves to a platform-specific native binary
package. The npm layer is packaging only: the Rust crate remains the source of
truth for the executable, CLI behavior, configuration, runtime adapters, and
tests.

The primary user installation path is:

```sh
npm install -g @matheusbbarni/atelier
atelier --doctor
atelier
```

The package should not initialize configuration, probe credentials, or start
the TUI during installation. First-run setup remains explicit through existing
CLI commands such as `atelier --doctor`, `atelier --init-config`, and `atelier`.

## User Stories

1. As a developer, I want to install `atelier` with npm, so that I can use the
   harness without cloning the repository or installing Rust.
2. As a developer, I want npm installation to provide a prebuilt native binary,
   so that setup is fast and does not require local compiler toolchains.
3. As a developer, I want the installed command to be `atelier`, so that npm,
   Cargo, README examples, and the CLI help all refer to the same executable.
4. As a developer, I want `atelier --doctor` to work immediately after npm
   installation, so that runtime setup problems are visible before I start a
   harness session.
5. As a developer, I want npm installation to avoid writing home configuration,
   so that installing the package does not mutate my environment.
6. As a developer on an unsupported platform, I want a clear error that names
   my platform and the supported targets, so that failure is understandable.
7. As a maintainer, I want one release workflow to build native binaries,
   publish npm packages, and create GitHub Releases, so that public releases are
   consistent and reproducible.
8. As a maintainer, I want npm package versions, Cargo versions, binary
   versions, and GitHub tag versions to match exactly, so that users do not get
   stale wrappers or mismatched binaries.
9. As a maintainer, I want release verification to install the local npm tarball
   and run the real command before publishing, so that broken package layouts are
   caught before users install them.
10. As a maintainer, I want platform binaries attached to GitHub Releases with
    checksums, so that npm artifacts are traceable to visible release artifacts.

## Package Model

- The top-level npm package is `@matheusbbarni/atelier`.
- The top-level package exposes exactly one binary command: `atelier`.
- The top-level package contains no native executable.
- The top-level package contains a small JavaScript launcher, package metadata,
  README, and license.
- The JavaScript launcher resolves the installed platform binary package and
  executes the native `atelier` executable.
- The launcher supports `ATELIER_BINARY_PATH` as a narrow testing and recovery
  override. Harness configuration must still use `multiagent.toml`, environment
  variables, and CLI flags.
- npm-installed `atelier` requires Node at runtime because npm invokes the
  JavaScript launcher. Native binaries attached to GitHub Releases remain
  standalone.
- Global install is the only supported npm UX for v1. `npm exec` and `npx` are
  out of scope even if they incidentally work.

## Platform Packages

Publish public platform packages under the same npm scope:

```text
@matheusbbarni/atelier-darwin-arm64
@matheusbbarni/atelier-darwin-x64
@matheusbbarni/atelier-linux-arm64
@matheusbbarni/atelier-linux-x64
@matheusbbarni/atelier-win32-arm64
@matheusbbarni/atelier-win32-x64
```

The top-level package declares these packages as optional dependencies. Users
should install only `@matheusbbarni/atelier`; platform packages are public
implementation artifacts.

Each platform package contains exactly one prebuilt native executable for its
target:

- macOS packages contain `atelier`.
- Linux packages contain `atelier`.
- Windows packages contain `atelier.exe`.

The v1 Linux targets mean standard glibc Linux built and verified on
GitHub-hosted Linux runners. Alpine/musl targets are out of scope for v1.

Unsupported platforms must fail clearly through install-time validation, the
launcher, or both. The error should include detected `process.platform` and
`process.arch`, list supported targets, and point users to GitHub Release
artifacts or source builds as manual fallbacks. The package must not attempt to
compile Rust from source as an automatic fallback.

## Repository Layout

Keep npm packaging in this repository:

```text
npm/
  package/
    package.json
    bin/atelier.js
    README.md
  platform/
    darwin-arm64/package.json
    darwin-x64/package.json
    linux-arm64/package.json
    linux-x64/package.json
    win32-arm64/package.json
    win32-x64/package.json
```

Each npm package should include a license file. Each platform package README
should state that it is an implementation package for `@matheusbbarni/atelier`,
not the recommended install target.

## Versioning

Releases use strict one-to-one version alignment:

- `Cargo.toml` package version
- `npm/package/package.json` version
- every `npm/platform/*/package.json` version
- `atelier --version` output
- Git tag version

For tag `v0.1.0`, publish `@matheusbbarni/atelier@0.1.0` and all platform
packages at `0.1.0`.

The release workflow must fail before publishing if any version differs from
the tag. `atelier --version` is part of the distribution contract and must print
the release version.

Only stable semantic version tags in the form `vMAJOR.MINOR.PATCH` publish npm
`latest` and create normal GitHub Releases. Alpha, beta, release-candidate, and
custom npm dist-tag flows are out of scope for v1.

## GitHub Release Workflow

Create a direct GitHub Actions workflow instead of adopting a release framework
for v1. The workflow is the single release authority for npm publishing and
GitHub Release creation.

The workflow should run from stable semver tags and may support
`workflow_dispatch` for dry-run validation. A tagged release should:

1. Validate the tag format and derive the release version.
2. Verify version consistency across Cargo, npm package manifests, and
   `atelier --version`.
3. Run normal Rust verification such as `cargo test`.
4. Build release binaries for all six supported targets.
5. Assemble platform npm packages with the matching native binaries.
6. Assemble the top-level npm distribution package.
7. Generate checksums for every native binary archive.
8. Pack npm tarballs locally.
9. Install the top-level npm tarball into a clean npm global prefix on each
   native runner.
10. Run the installed command verification:

    ```sh
    atelier --version
    atelier --help
    atelier --doctor --json
    ```

11. Verify cross-target packages that cannot execute on the current runner by
    checking package metadata, native binary presence, Windows `.exe` naming,
    Unix executable bits, and checksum manifest entries.
12. Publish all platform packages and the top-level package to npm with
    provenance/trusted publishing enabled.
13. Create the public GitHub Release only after npm publishing succeeds.
14. Attach native binary archives and the checksum manifest to the GitHub
    Release.
15. Generate GitHub Release notes automatically.

Publishing must be all-or-nothing across the six supported targets. If any
platform build, package assembly, tarball install, command verification,
checksum generation, or version consistency check fails, the workflow must not
publish npm packages or create a GitHub Release.

npm package versions are immutable once published. If npm publishing fails
partway through, the workflow must stop and avoid creating the GitHub Release.
Maintainers should patch forward with a new version after fixing the issue; the
workflow must not attempt to overwrite an npm version.

If npm publishing succeeds but GitHub Release creation fails, maintainers may
rerun only the idempotent GitHub Release creation step for the same tag and
already-published npm version.

## Documentation Requirements

Update the repository README so npm is the recommended user install path:

```sh
npm install -g @matheusbbarni/atelier
atelier --doctor
atelier
```

Keep `cargo install --path .` documented as the developer/source install path.
README documentation should also state:

- npm installs prebuilt native binaries.
- npm install does not initialize configuration.
- supported npm targets are macOS arm64/x64, glibc Linux arm64/x64, and Windows
  arm64/x64.
- Alpine/musl Linux is not supported by v1 npm packages.
- GitHub Releases provide native binary archives and checksum manifests.

## Testing Requirements

- Add launcher tests for platform package resolution.
- Add launcher tests for unsupported platform errors.
- Add launcher tests for missing optional dependency errors, including installs
  where optional dependencies were omitted.
- Add launcher tests for `ATELIER_BINARY_PATH`.
- Add package metadata tests that verify package names, versions, bin entries,
  optional dependencies, OS/CPU declarations, README presence, and license
  presence.
- Add workflow-level verification that installs local npm tarballs before
  publishing.
- Keep normal tests credential-free. `atelier --doctor --json` may report
  unavailable or unknown runtimes, but it must exit successfully enough to prove
  the installed binary runs.

## Out of Scope

- Building Rust from source during npm install.
- Homebrew distribution.
- `cargo install atelier` registry distribution.
- `npm exec` and `npx` as supported user flows.
- macOS code signing and notarization.
- Windows code signing.
- Linux musl/Alpine platform packages.
- Prerelease npm dist-tags.
- A maintained `CHANGELOG.md`.
- Moving the Rust source of truth into npm packaging.
- Reading or writing harness configuration during npm installation.

## Implementation Checklist

- Align `CONTEXT.md`, README examples, and CLI version output around the
  `atelier` global command.
- Add npm package manifests and the JavaScript launcher.
- Add platform package manifests and minimal package READMEs.
- Add scripts for copying release binaries into platform package folders.
- Add scripts for validating version consistency and package metadata.
- Add checksum generation for native release archives.
- Add GitHub Actions jobs for native builds on macOS, Linux, and Windows.
- Add GitHub Actions jobs for npm tarball assembly and clean-prefix install
  verification.
- Add npm trusted publishing or `npm publish --provenance`.
- Add the final GitHub Release creation job after npm publish succeeds.
- Update README install and release documentation.

## Acceptance Criteria

- `npm install -g @matheusbbarni/atelier` installs the `atelier` command on each
  supported target without requiring Rust.
- `atelier --version`, `atelier --help`, and `atelier --doctor --json` run from
  an npm global install in CI.
- The top-level npm package contains no native binary.
- Each platform package contains exactly the native binary for its target.
- All npm package versions, Cargo version, binary version output, and Git tag
  version match.
- The release workflow refuses to publish when any supported platform artifact
  is missing or unverifiable.
- npm packages publish before the public GitHub Release is created.
- GitHub Releases contain native binary archives, checksum manifest, and
  generated release notes.
- README identifies npm as the recommended user install path and Cargo as the
  developer/source install path.
