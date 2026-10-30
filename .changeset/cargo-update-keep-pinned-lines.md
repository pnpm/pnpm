---
"pacquet": patch
---

`pnpm update --latest` keeps three kinds of Cargo dependencies where they are, unless the crate is named on the command line. Before, it moved each one to the newest release and left a workspace that did not build.

- A renamed dependency, such as `tower-0-4 = { package = "tower", version = "0.4" }`, stays within its requirement if the workspace also declares the crate under its own name.
- A crate replaced through `[patch.crates-io]` keeps its version, with a warning.
- A crate whose newest release is older than its current version, such as a prerelease newer than the latest stable release, keeps its version, with a warning. Only an explicit version, such as `crate:dashmap@6`, moves it back.

Other new major versions are still selected and may need code changes.
