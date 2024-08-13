---
"@pnpm/workspace.find-packages": patch
"pnpm": patch
---

Remove warnings for non-root `pnpm` field, add warnings for non-root `pnpm` subfields that aren't `executionEnv` [#8143](https://github.com/pnpm/pnpm/issues/8413).
