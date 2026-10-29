---
"pacquet": patch
---

`pnpm install` now resolves again after an edit to an injected `workspace:../dir` dependency outside the workspace patterns under a shared lockfile. It used to keep the lockfile unchanged. The next `pnpm install --frozen-lockfile` then failed [#17476](https://github.com/pnpm/pnpm/issues/17476).
