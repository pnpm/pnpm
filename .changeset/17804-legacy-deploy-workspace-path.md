---
"pacquet": patch
---

`pnpm deploy` without a shared workspace lockfile now resolves a relative `workspace:` path, such as `workspace:../lib`, from the source project's directory. It used to be resolved from the deploy target and fail [pnpm/pnpm#17804](https://github.com/pnpm/pnpm/issues/17804).
