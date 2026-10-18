---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now accepts the lockfile it wrote for a `file:` dependency on a directory without a `package.json`. Before, it failed with `ERR_PNPM_OUTDATED_LOCKFILE`.
