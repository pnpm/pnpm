---
"pacquet": patch
---

`pnpm update --latest` no longer moves Cargo dependencies onto versions that break the workspace, unless the crate is named on the command line:

- A renamed dependency, such as `tower-0-4 = { package = "tower", version = "0.4" }`, stays on its current line if the workspace also declares the crate under its own name.
- A crate replaced through `[patch.crates-io]` keeps its version, with a warning.

`pnpm update` also no longer downgrades a Cargo dependency, such as a prerelease newer than the latest stable release, unless an explicit version is given.
