# @matheusbbarni/atelier

Global npm distribution for the `atelier` CLI.

```sh
npm install -g @matheusbbarni/atelier
atelier --doctor
atelier
```

This package contains a small JavaScript launcher. The native executable is
provided by a platform-specific optional dependency selected by npm for macOS,
glibc Linux, or Windows on arm64/x64.

Installation does not initialize configuration, probe credentials, or start the
TUI. Run `atelier --doctor` after installation to inspect runtime readiness.
