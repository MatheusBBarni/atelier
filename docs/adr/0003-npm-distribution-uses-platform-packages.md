# npm distribution uses platform packages

The first npm distribution for `atelier` uses a scoped top-level package,
`@matheusbbarni/atelier`, plus public platform-specific optional dependency
packages such as `@matheusbbarni/atelier-linux-x64`. The top-level package
contains a small JavaScript launcher and no native binary; each platform package
contains exactly one prebuilt native executable for its operating system and CPU
architecture.

This keeps `npm install -g @matheusbbarni/atelier` fast and familiar without
requiring Rust, Cargo, Xcode, Visual Studio Build Tools, or a source checkout on
the user's machine. It also lets npm skip incompatible platform packages through
`os`, `cpu`, and Linux `libc` metadata while the launcher can report clear
unsupported-platform and missing-optional-dependency errors.

The tradeoff is that a release now publishes seven npm packages instead of one,
so release automation must build all supported native targets, verify local
tarball installs, publish platform packages before the top-level package, and
patch forward if an immutable npm version is published incorrectly. Source
builds during npm install, Homebrew distribution, Cargo registry distribution,
and code signing are separate future decisions.
