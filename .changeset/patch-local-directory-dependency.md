---
"pacquet": patch
---

A name-only `patchedDependencies` entry now records its patch hash on a `file:` directory dependency or an injected workspace package. `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_INCONSISTENT_PATCH_HASH` on the lockfile that `pnpm install` wrote.
