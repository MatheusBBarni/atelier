# npm Distribution Plan

Persisted from the latest orchestrator decision.

- Define distribution contract: npm package publishes a Node CLI entrypoint (`atelier`) that is a thin launcher for platform-specific Rust binaries downloaded/packaged per OS+arch.
- Add version alignment policy: link `package.json` version to `Cargo.toml` version via release pipeline (one source of truth) and update both atomically.
- Add version checks and architecture checks in launcher; provide clear failure guidance when unsupported binary is requested.
- Add install-time behavior (`prepare`/`postinstall`) to select and expose the correct platform binary and keep scripts stable across platforms.
- Add release pipeline (new `.github/workflows/release.yml`): build Rust binaries for target matrix (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc` minimum), create checksums, create GitHub Release, and generate npm package artifact.
- Automate npm publish from CI with OIDC/npm token, using signed release artifacts and changelog-driven releases (or tags) to avoid manual publish drift.
- Prepare local verification pipeline: add docs and a `ci-smoke` script that installs package from tarball, runs `atelier --help` on each platform artifact in CI where available.
- Add install/distribution docs: README section for `npm install -g atelier`.
