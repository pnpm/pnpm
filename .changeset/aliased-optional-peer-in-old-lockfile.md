---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer fails with `ERR_PNPM_NO_MATCHING_VERSION` after a change to `overrides` when the lockfile resolves an optional peer dependency to an npm alias of another package [#16654](https://github.com/pnpm/pnpm/issues/16654).
