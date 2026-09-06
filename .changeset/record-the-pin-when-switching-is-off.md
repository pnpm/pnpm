---
"pacquet": patch
---

Commands other than `pnpm install` now record the pinned pnpm version in `pnpm-lock.yaml` when package manager version switching is turned off. The install family already recorded it, so the two kept rewriting each other's `packageManagerDependencies` block [#14575](https://github.com/pnpm/pnpm/issues/14575).
