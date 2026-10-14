---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

A filtered install, such as `pnpm install --filter <pkg>`, no longer removes workspace projects that are missing from disk from `pnpm-lock.yaml`. Their importers, packages, and catalog entries stay in the lockfile even when the filter matches every project on disk, as in a sparse checkout [#16931](https://github.com/pnpm/pnpm/issues/16931).
