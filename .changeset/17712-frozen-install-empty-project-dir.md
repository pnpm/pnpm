---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now succeeds when a workspace project recorded in `pnpm-lock.yaml` has an empty directory, such as a git submodule that was not checked out [#17712](https://github.com/pnpm/pnpm/issues/17712).
