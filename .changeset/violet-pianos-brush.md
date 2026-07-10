---
"@pnpm/deps.status": patch
"pnpm": patch
---

`verify-deps-before-run` no longer spawns a `pnpm install` when pnpm is executed in a directory that has no `package.json`. A mistyped command run outside a project (for example `pnpm witch 10 login`) used to crash with a confusing error from the spawned install; now it fails with the regular "no package.json found" error.
