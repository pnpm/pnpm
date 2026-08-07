---
"@pnpm/napi": minor
---

Added an `allowUnusedPatches` install option. When `true`, a `patchedDependencies` entry that matches no installed package warns instead of failing the install with `ERR_PNPM_UNUSED_PATCH`.
