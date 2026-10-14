---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
"pnpm": patch
---

A filtered install, such as `pnpm install --filter <pkg>`, now keeps the `pnpm-lock.yaml` entries of workspace projects that are missing from disk, as in a sparse checkout. Their importers, packages, and catalog entries stay even when the filter matches every project on disk [#16931](https://github.com/pnpm/pnpm/issues/16931).
