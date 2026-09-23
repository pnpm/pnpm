---
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
"pacquet": patch
---

`pnpm list --only-projects` now prints every project selected with `--filter` or `--recursive`, including a project that has no workspace dependencies [#9770](https://github.com/pnpm/pnpm/issues/9770).

`pnpm list --only-projects` no longer reports packages in `node_modules` that are missing from the lockfile. They are not workspace projects [#9528](https://github.com/pnpm/pnpm/issues/9528).
