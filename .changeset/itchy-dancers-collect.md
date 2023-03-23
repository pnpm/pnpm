---
"@pnpm/core": patch
"pnpm": patch
---

Don't write the `pnpm-lock.yaml` file if it has no changes and `pnpm install --frozen-lockfile` was executed [#6158](https://github.com/pnpm/pnpm/issues/6158).
