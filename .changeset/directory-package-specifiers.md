---
"@pnpm/lockfile.types": patch
"@pnpm/lockfile.fs": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.verification": patch
"pnpm": patch
"pacquet": minor
---

pnpm now records the dependency specifiers of injected workspace packages and `file:` directory dependencies in `pnpm-lock.yaml`, under `specifiers` in the package's entry. `pnpm install --frozen-lockfile` compares them with the package's manifest instead of resolving its dependencies again. A lockfile written by an earlier version still passes, and the next install that resolves dependencies adds the field.
