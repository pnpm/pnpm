---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update <name>@<version>` now treats the requested version the same way in a workspace as it does in a single project. When the selector matches no direct dependency, the version is ignored — the dependency is updated to what a fresh install would resolve — and pnpm says so and points at `overrides`, which is the mechanism that does pin a transitive dependency. That warning is now shown for recursive updates too, and reaches the user on the Rust CLI.
