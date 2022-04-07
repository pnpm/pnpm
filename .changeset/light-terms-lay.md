---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

`dependenciesMeta` should be saved into the lockfile, when it is added to the package manifest by a hook.
