---
"@pnpm/pkg-manifest.utils": minor
"@pnpm/installing.commands": minor
"@pnpm/types": patch
"pnpm": minor
---

Added `--peer` flag to `pnpm update` command to allow updating packages in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

Previously, `pnpm up` only updated packages in `dependencies`, `devDependencies`, and `optionalDependencies`. Packages listed in `peerDependencies` were silently skipped, requiring manual version range updates.

Now you can use `pnpm up --peer` or `pnpm up --latest --peer` to also update peer dependency version ranges in `package.json`.
